//! The agent loop: what turns a chat completion into an agent.
//!
//! Without this, the model is handed no tools and can only describe what it
//! would do — it says "I will now search the web" and then invents the answer,
//! because narration is the only thing available to it. The loop gives the
//! model real tools, executes what it asks for, feeds the results back, and
//! repeats until it stops asking.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use api::{
    ContentBlockDelta, InputContentBlock, InputMessage, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition,
};
use serde_json::Value;
use tokio::sync::Notify;

use crate::workspace::Workspace;

/// Tools the desktop app exposes to the model.
///
/// Read-only, plus network research. These are safe to offer unconditionally:
/// the worst outcome of a confused call is a wasted turn.
const ALLOWED_TOOLS: &[&str] = &[
    "WebSearch",
    "WebFetch",
    "read_file",
    "glob_search",
    "grep_search",
    "GitStatus",
    "GitDiff",
    "GitLog",
    "GitShow",
    "GitBlame",
];

/// Tools that change files, offered only once a project folder is chosen.
///
/// There is no per-call approval prompt, so the project folder is the entire
/// boundary: every path these receive is confined to it by
/// [`workspace::Workspace`] before the tool runs. With no folder chosen there
/// is nothing to confine them to, so they are not offered at all.
///
/// `bash` and `PowerShell` stay withheld regardless. A shell command cannot be
/// confined by rewriting an argument — `cd` alone defeats it — so honouring the
/// folder boundary for them needs a real sandbox, not a path check.
const WRITE_TOOLS: &[&str] = &["write_file", "edit_file"];

/// Upper bound on tool round trips in a single turn.
///
/// A model that has misunderstood a tool result will often re-issue the same
/// call forever. The cap turns that into a bounded, explainable stop instead of
/// a session that runs until the user kills it.
const MAX_STEPS: usize = 8;

/// Cooperative cancellation that can wake a task blocked waiting for the next
/// model-stream event.
///
/// An atomic flag alone is insufficient: if Ollama spends a minute before
/// yielding another chunk, no code runs to observe the flag. `Notify` provides
/// the missing wake-up edge while the atomic preserves race-free state checks.
pub struct CancelToken {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// What the loop produced, and why it ended.
pub struct Outcome {
    /// Visible assistant text, concatenated across steps.
    pub text: String,
    /// True when the turn ended because it was cancelled.
    pub cancelled: bool,
}

/// A tool call the model asked for, assembled from the stream.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Events the loop reports as it runs.
///
/// `Sync` is required because the loop is held across await points inside a
/// Tauri command, which must be `Send`.
pub trait Sink: Sync {
    fn text(&self, text: &str);
    fn thinking(&self, text: &str);
    fn tool_start(&self, name: &str, summary: &str);
    fn tool_end(&self, name: &str, ok: bool, detail: &str);
}

/// The tool definitions the model is offered.
///
/// Writing tools appear only when there is a project folder to confine them to.
#[must_use]
pub fn definitions(workspace: Option<&Workspace>) -> Vec<ToolDefinition> {
    let mut names: Vec<&str> = ALLOWED_TOOLS.to_vec();
    if workspace.is_some() {
        names.extend_from_slice(WRITE_TOOLS);
    }
    let allowed: BTreeSet<String> = names
        .into_iter()
        .map(tools::canonical_allowed_tool_name)
        .collect();
    tools::GlobalToolRegistry::builtin().definitions(Some(&allowed))
}

/// Instructions that make a local model actually reach for its tools.
///
/// Small local models default to answering from memory and describing tool use
/// in prose. They need to be told plainly that narration is not execution, and
/// they need today's date, because otherwise "latest" is silently resolved
/// against a training cut-off that is months or years stale.
#[must_use]
pub fn system_prompt(today: &str, workspace: Option<&Workspace>) -> String {
    let files = match workspace {
        Some(workspace) => format!(
            "- The project folder is {}. You can create and change files in it with write_file \
             and edit_file, using paths relative to that folder. You cannot touch anything \
             outside it.\n\
             - Read a file before editing it, so you do not overwrite work you have not seen.\n",
            workspace.label()
        ),
        None => "- No project folder is open, so you cannot create or change any files. If a \
                 request needs that, say so and tell the user to open a folder.\n"
            .to_string(),
    };

    format!(
        "You are Disco Code, a coding assistant running entirely on the user's machine.\n\
         Today's date is {today}.\n\
         \n\
         You have real tools. Call them. Describing a tool call in prose does not run it, \
         and saying you have done something you have not done is the worst outcome here.\n\
         \n\
         Rules:\n\
         - Your knowledge has a training cut-off. For anything current — news, releases, \
         versions, prices, events, or whatever is happening 'today' or 'now' — you must call \
         WebSearch first. Never answer such a question from memory.\n\
         - Never invent facts, URLs, headlines or file contents. If a tool did not return it, \
         you do not know it.\n\
         - To read a file, call read_file. To find files, call glob_search. To search contents, \
         call grep_search. Do not guess what a file contains.\n\
         - Use WebFetch to read a specific page you already have a URL for.\n\
         - WebFetch returns page content directly. It does not create data/*.html files; never \
         call read_file or grep_search on an imagined WebFetch output path.\n\
         - When you have used web results, end with a Sources section listing the URLs.\n\
         {files}\
         - You cannot run shell commands. If a request needs one, say so plainly and give the \
         exact command the user should run.\n\
         - When the tools have given you enough, answer directly and concisely. Do not narrate \
         your process."
    )
}

/// Runs the model until it stops asking for tools.
pub async fn run(
    client: &api::ProviderClient,
    model: &str,
    system: String,
    first: String,
    reasoning: bool,
    workspace: Option<&Workspace>,
    cancel: &Arc<CancelToken>,
    sink: &dyn Sink,
) -> Result<Outcome, String> {
    let tool_defs = definitions(workspace);
    let mut messages = vec![InputMessage::user_text(first)];
    let mut transcript = String::new();
    let mut last_web_url: Option<String> = None;

    for step in 0..MAX_STEPS {
        if cancel.is_cancelled() {
            return Ok(Outcome {
                text: transcript,
                cancelled: true,
            });
        }

        let request = api::MessageRequest {
            model: model.to_string(),
            max_tokens: 4096,
            messages: messages.clone(),
            system: Some(system.clone()),
            tools: Some(tool_defs.clone()),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
            // Measured on a local 9.7B model: the same one-word answer took 40.5s
            // with reasoning left on and 1.0s with `reasoning_effort: none`,
            // because the scratchpad is billed at the same tokens-per-second as
            // the answer. It is never displayed, so paying for it is deliberate.
            reasoning_effort: if reasoning {
                None
            } else {
                Some("none".to_string())
            },
            ..Default::default()
        };

        let mut step_result = stream_step(client, &request, cancel, sink).await?;
        if step_result.cancelled {
            return Ok(Outcome {
                text: transcript,
                cancelled: true,
            });
        }

        // Small local models occasionally choose a filesystem reader for an
        // HTTP URL after WebSearch. Repair that envelope mistake before it
        // reaches Windows path handling: the intent is unambiguously to read
        // the page, and passing the URL to grep_search only produces
        // "filename or volume label syntax is incorrect".
        for call in &mut step_result.calls {
            normalize_tool_call(call, last_web_url.as_deref(), workspace);
            if call.name == "WebFetch" {
                last_web_url = call
                    .input
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }

        transcript.push_str(&step_result.text);

        if step_result.calls.is_empty() {
            return Ok(Outcome {
                text: transcript,
                cancelled: false,
            });
        }

        // The assistant's tool requests must be echoed back verbatim: a tool
        // result with no matching call is rejected by the chat protocol.
        let mut assistant_blocks: Vec<InputContentBlock> = Vec::new();
        if !step_result.text.trim().is_empty() {
            assistant_blocks.push(InputContentBlock::Text {
                text: step_result.text.clone(),
            });
        }
        for call in &step_result.calls {
            assistant_blocks.push(InputContentBlock::ToolUse {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.input.clone(),
            });
        }
        messages.push(InputMessage {
            role: "assistant".to_string(),
            content: assistant_blocks,
        });

        let last_step = step + 1 == MAX_STEPS;
        for call in step_result.calls {
            if cancel.is_cancelled() {
                return Ok(Outcome {
                    text: transcript,
                    cancelled: true,
                });
            }

            sink.tool_start(&call.name, &summarize(&call.name, &call.input));
            let (output, is_error) = tokio::select! {
                () = cancel.cancelled() => {
                    return Ok(Outcome {
                        text: transcript,
                        cancelled: true,
                    });
                }
                result = execute(&call, workspace) => result,
            };
            sink.tool_end(&call.name, !is_error, &first_line(&output));

            messages.push(InputMessage::user_tool_result(
                call.id,
                truncate(&output),
                is_error,
            ));
        }

        if last_step {
            // Out of steps with tool results still unanswered. Say so rather
            // than returning whatever partial narration was collected.
            messages.push(InputMessage::user_text(
                "You have reached the tool-call limit for this turn. Answer now using only what \
                 the tool results above actually contain. Do not request more tools.",
            ));
            let request = api::MessageRequest {
                model: model.to_string(),
                max_tokens: 4096,
                messages,
                system: Some(system),
                stream: true,
                reasoning_effort: Some("none".to_string()),
                ..Default::default()
            };
            let final_step = stream_step(client, &request, cancel, sink).await?;
            transcript.push_str(&final_step.text);
            return Ok(Outcome {
                text: transcript,
                cancelled: final_step.cancelled,
            });
        }
    }

    Ok(Outcome {
        text: transcript,
        cancelled: false,
    })
}

struct StepResult {
    text: String,
    calls: Vec<ToolCall>,
    cancelled: bool,
}

/// Repairs unambiguous tool-envelope mistakes made by small local models.
///
/// This intentionally does not guess broadly. Only an absolute HTTP(S) URL
/// sent to a file reader is converted to WebFetch, and a WebFetch call missing
/// its required prompt receives a neutral extraction instruction.
fn normalize_tool_call(
    call: &mut ToolCall,
    last_web_url: Option<&str>,
    workspace: Option<&Workspace>,
) {
    let path = call
        .input
        .get("path")
        .or_else(|| call.input.get("file_path"))
        .and_then(Value::as_str);
    let is_file_reader = matches!(call.name.as_str(), "read_file" | "grep_search");

    if is_file_reader {
        if let Some(url) = path.filter(|candidate| is_http_url(candidate)) {
            let prompt = call
                .input
                .get("pattern")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Extract the page content relevant to the user's request.");
            call.name = "WebFetch".to_string();
            call.input = serde_json::json!({ "url": url, "prompt": prompt });
            return;
        }
    }

    // Granite sometimes treats the already-returned WebFetch body as though it
    // had been saved to `data/1.html`, then tries to grep that invented file.
    // Re-fetching the last page with the requested pattern is deterministic and
    // avoids turning a model convention into a Windows path error.
    if call.name == "grep_search" {
        if let (Some(path), Some(url)) = (path, last_web_url) {
            if looks_like_webfetch_staging_path(path) && !local_path_exists(path, workspace) {
                let prompt = call
                    .input
                    .get("pattern")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("Extract the page content relevant to the user's request.");
                call.name = "WebFetch".to_string();
                call.input = serde_json::json!({ "url": url, "prompt": prompt });
                return;
            }
        }
    }

    if call.name == "WebFetch"
        && call.input.get("url").and_then(Value::as_str).is_some()
        && call.input.get("prompt").and_then(Value::as_str).is_none()
    {
        call.input["prompt"] =
            Value::String("Extract the page content relevant to the user's request.".to_string());
    }
}

fn is_http_url(candidate: &str) -> bool {
    reqwest::Url::parse(candidate).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn looks_like_webfetch_staging_path(candidate: &str) -> bool {
    let normalized = candidate.replace('\\', "/");
    normalized.starts_with("data/")
        && Path::new(&normalized)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "htm" | "html"))
}

fn local_path_exists(candidate: &str, workspace: Option<&Workspace>) -> bool {
    if let Some(workspace) = workspace {
        return workspace
            .confine(candidate)
            .is_ok_and(|path| path.exists());
    }
    Path::new(candidate).exists()
}

/// Recovers a tool call that the model wrote as text instead of calling.
///
/// Measured against the local daemon: `granite4.1:8b` returns a proper
/// `tool_calls` payload, but `qwen2.5-coder:7b` — which advertises the `tools`
/// capability — instead emits `{"name": "WebSearch", "arguments": {...}}` as
/// ordinary content. Without this the model looks like it ignored its tools,
/// when in fact it asked for one in the wrong envelope.
///
/// Only names the model was actually offered are accepted, so arbitrary JSON in
/// an answer cannot be turned into a tool call.
fn salvage(text: &str, known: &BTreeSet<String>) -> Option<ToolCall> {
    let body = strip_wrappers(text.trim());
    let parsed: Value = serde_json::from_str(body).ok()?;

    let name = parsed.get("name")?.as_str()?.to_string();
    if !known.contains(&name) {
        return None;
    }

    let raw_args = parsed
        .get("arguments")
        .or_else(|| parsed.get("parameters"))
        .or_else(|| parsed.get("input"))
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    // Some models double-encode the arguments as a JSON string.
    let input = match raw_args {
        Value::String(encoded) => {
            serde_json::from_str(&encoded).unwrap_or(Value::Object(serde_json::Map::new()))
        }
        other => other,
    };

    Some(ToolCall {
        id: format!("salvaged_{name}"),
        name,
        input,
    })
}

/// Removes the fences and tags local models wrap tool calls in.
fn strip_wrappers(text: &str) -> &str {
    let mut body = text.trim();
    for (open, close) in [
        ("<tool_call>", "</tool_call>"),
        ("```json", "```"),
        ("```", "```"),
    ] {
        if let Some(inner) = body.strip_prefix(open) {
            body = inner.strip_suffix(close).unwrap_or(inner).trim();
        }
    }
    body
}

/// Whether text so far looks like the start of a tool call written as content.
///
/// Used to hold such text back instead of streaming it, so a salvaged call is
/// never shown to the user as if it were the answer.
fn looks_like_tool_json(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with('{') || head.starts_with("<tool_call>") || head.starts_with("```")
}

/// Streams one model response, separating visible text from tool calls.
///
/// Tool arguments arrive split across many `InputJsonDelta` fragments keyed by
/// block index, so they are accumulated per index and parsed once the stream
/// ends rather than being parsed incrementally.
async fn stream_step(
    client: &api::ProviderClient,
    request: &api::MessageRequest,
    cancel: &Arc<CancelToken>,
    sink: &dyn Sink,
) -> Result<StepResult, String> {
    let stream = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            return Ok(StepResult {
                text: String::new(),
                calls: Vec::new(),
                cancelled: true,
            });
        }
        result = client.stream_message(request) => result,
    };
    let mut stream = stream.map_err(|error| error.to_string())?;

    let mut text = String::new();
    let mut emitted = 0usize;
    let mut pending: Vec<(u32, String, String, String)> = Vec::new();

    loop {
        if cancel.is_cancelled() {
            return Ok(StepResult {
                text,
                calls: Vec::new(),
                cancelled: true,
            });
        }

        let event = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                return Ok(StepResult {
                    text,
                    calls: Vec::new(),
                    cancelled: true,
                });
            }
            event = stream.next_event() => event,
        };

        match event {
            Ok(Some(StreamEvent::ContentBlockStart(start))) => {
                if let OutputContentBlock::ToolUse { id, name, .. } = start.content_block {
                    pending.push((start.index, id, name, String::new()));
                }
            }
            Ok(Some(StreamEvent::ContentBlockDelta(delta))) => match delta.delta {
                ContentBlockDelta::TextDelta { text: chunk } => {
                    text.push_str(&chunk);
                    // Held back while the text might turn out to be a tool call
                    // written as content; released below if it is not.
                    if !looks_like_tool_json(&text) {
                        sink.text(&text[emitted..]);
                        emitted = text.len();
                    }
                }
                ContentBlockDelta::ThinkingDelta { thinking } => sink.thinking(&thinking),
                ContentBlockDelta::InputJsonDelta { partial_json } => {
                    if let Some(entry) = pending.iter_mut().find(|(i, ..)| *i == delta.index) {
                        entry.3.push_str(&partial_json);
                    }
                }
                ContentBlockDelta::SignatureDelta { .. } => {}
            },
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error) => return Err(error.to_string()),
        }
    }

    let mut calls: Vec<ToolCall> = pending
        .into_iter()
        .map(|(_, id, name, raw)| ToolCall {
            id,
            name,
            // An empty argument string is a no-argument call, not a parse
            // failure. Anything else that will not parse is passed on as null
            // so the tool reports a useful error rather than the loop dying.
            input: if raw.trim().is_empty() {
                Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&raw).unwrap_or(Value::Null)
            },
        })
        .collect();

    if calls.is_empty() {
        let known: BTreeSet<String> = request
            .tools
            .as_ref()
            .map(|defs| defs.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        if let Some(recovered) = salvage(&text, &known) {
            // The held-back text was a tool call, so it is dropped rather than
            // shown: raw JSON is not an answer.
            return Ok(StepResult {
                text: String::new(),
                calls: vec![recovered],
                cancelled: false,
            });
        }
    }

    // Nothing was salvaged, so anything held back is real output after all.
    if emitted < text.len() {
        sink.text(&text[emitted..]);
    }

    calls.retain(|call| !call.name.is_empty());
    Ok(StepResult {
        text,
        calls,
        cancelled: false,
    })
}

/// Runs one tool off the async runtime.
///
/// The tool registry is synchronous and its HTTP client blocks, so calling it
/// directly here would stall the runtime that is streaming the reply.
///
/// Paths are confined to the project folder first. A refusal is returned to the
/// model as a tool error, so it can correct itself rather than the turn dying.
async fn execute(call: &ToolCall, workspace: Option<&Workspace>) -> (String, bool) {
    let name = call.name.clone();

    let input = match workspace {
        Some(workspace) => match workspace.rewrite(&call.input) {
            Ok(confined) => confined,
            Err(refusal) => return (refusal, true),
        },
        None => call.input.clone(),
    };

    // The file tools confine themselves to a workspace, which they default to
    // the process working directory — for an installed app, wherever the
    // launcher started it. Stating the folder here is what lets a write inside
    // the project actually land instead of being refused as outside it.
    tools::set_workspace_root(workspace.map(|workspace| workspace.root().to_path_buf()));

    let joined = tokio::task::spawn_blocking(move || {
        tools::GlobalToolRegistry::builtin().execute(&name, &input)
    })
    .await;

    match joined {
        Ok(Ok(output)) => (output, false),
        Ok(Err(error)) => (format!("Tool failed: {error}"), true),
        Err(error) => (format!("Tool did not run: {error}"), true),
    }
}

/// A short, human-readable description of what a call is about to do.
fn summarize(name: &str, input: &Value) -> String {
    for key in ["query", "url", "path", "pattern", "file_path"] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            return value.chars().take(90).collect();
        }
    }
    name.to_string()
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

/// Caps a tool result so one large page cannot evict the conversation from a
/// local model's context window.
fn truncate(text: &str) -> String {
    const LIMIT: usize = 24_000;
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let mut cut = LIMIT;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n\n[truncated]", &text[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> Workspace {
        let dir = std::env::temp_dir().join(format!("disco-agent-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        Workspace::open(dir.to_str().unwrap()).unwrap()
    }

    #[test]
    fn web_search_is_offered_to_the_model() {
        let names: Vec<String> = definitions(None).into_iter().map(|d| d.name).collect();
        assert!(
            names.iter().any(|n| n == "WebSearch"),
            "without WebSearch the model answers current-events questions from memory: {names:?}"
        );
        assert!(names.iter().any(|n| n == "read_file"));
        assert!(!names.is_empty());
    }

    /// A shell command cannot be confined by rewriting a path argument, so no
    /// project folder makes it safe to offer.
    #[test]
    fn shell_tools_are_withheld_even_with_a_project_open() {
        let workspace = project("shell");
        let names: Vec<String> = definitions(Some(&workspace))
            .into_iter()
            .map(|d| d.name)
            .collect();

        for withheld in ["bash", "PowerShell"] {
            assert!(
                !names.iter().any(|n| n == withheld),
                "{withheld} can leave the project folder and cannot be confined by path"
            );
        }
    }

    #[test]
    fn writing_is_offered_only_once_there_is_a_folder_to_confine_it_to() {
        let without: Vec<String> = definitions(None).into_iter().map(|d| d.name).collect();
        assert!(
            !without.iter().any(|n| n == "write_file"),
            "with no project folder there is no boundary, so writing must not be offered"
        );

        let workspace = project("writes");
        let with: Vec<String> = definitions(Some(&workspace))
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert!(with.iter().any(|n| n == "write_file"));
        assert!(with.iter().any(|n| n == "edit_file"));
    }

    #[test]
    fn the_system_prompt_dates_the_conversation_and_forbids_invention() {
        let prompt = system_prompt("2026-09-13", None);
        assert!(
            prompt.contains("2026-09-13"),
            "without today's date, 'latest' resolves against the training cut-off"
        );
        assert!(prompt.contains("WebSearch"));
        assert!(prompt.to_lowercase().contains("never invent"));
    }

    /// The model is told which folder it may write to, so it uses relative
    /// paths inside the project rather than guessing an absolute location.
    #[test]
    fn the_system_prompt_names_the_project_folder_when_one_is_open() {
        let workspace = project("named");

        let open = system_prompt("2026-09-13", Some(&workspace));
        assert!(open.contains(&workspace.label()));
        assert!(open.contains("write_file"));

        let closed = system_prompt("2026-09-13", None);
        assert!(
            closed.contains("No project folder is open"),
            "the model must be told it cannot write, or it will claim it did"
        );
    }

    #[test]
    fn tool_arguments_are_summarized_by_their_most_telling_field() {
        let input = serde_json::json!({ "query": "today's news" });
        assert_eq!(summarize("WebSearch", &input), "today's news");
        assert_eq!(
            summarize("Mystery", &serde_json::json!({})),
            "Mystery",
            "an unrecognised shape still has to name the tool"
        );
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let text = "é".repeat(20_000);
        let cut = truncate(&text);
        assert!(cut.ends_with("[truncated]"));
        // Round-tripping proves the cut landed on a boundary.
        assert!(cut.chars().count() > 0);
    }

    /// `qwen2.5-coder:7b` advertises tool support but, measured against the
    /// local daemon, writes the call out as content instead of calling it.
    /// Recovering that is the difference between the model looking broken and
    /// the search actually happening.
    #[test]
    fn a_tool_call_written_as_text_is_recovered() {
        let known: BTreeSet<String> = ["WebSearch".to_string()].into_iter().collect();
        let raw = r#"{"name": "WebSearch", "arguments": {"query": "top news"}}"#;

        let call = salvage(raw, &known).expect("qwen writes its calls in this exact shape");
        assert_eq!(call.name, "WebSearch");
        assert_eq!(call.input["query"], "top news");
    }

    #[test]
    fn fenced_and_tagged_tool_calls_are_recovered_too() {
        let known: BTreeSet<String> = ["WebSearch".to_string()].into_iter().collect();
        let body = r#"{"name": "WebSearch", "arguments": {"query": "x"}}"#;

        for wrapped in [
            format!("```json\n{body}\n```"),
            format!("<tool_call>{body}</tool_call>"),
        ] {
            assert!(
                salvage(&wrapped, &known).is_some(),
                "a wrapper is still a tool call: {wrapped}"
            );
        }
    }

    #[test]
    fn double_encoded_arguments_are_decoded() {
        let known: BTreeSet<String> = ["WebSearch".to_string()].into_iter().collect();
        let raw = r#"{"name": "WebSearch", "arguments": "{\"query\": \"news\"}"}"#;

        let call = salvage(raw, &known).expect("some models encode arguments twice");
        assert_eq!(call.input["query"], "news");
    }

    /// Salvage must not be a way to invoke anything the model was not offered.
    #[test]
    fn json_naming_an_unoffered_tool_is_not_salvaged() {
        let known: BTreeSet<String> = ["WebSearch".to_string()].into_iter().collect();
        let raw = r#"{"name": "bash", "arguments": {"command": "del /s C:\\"}}"#;

        assert!(
            salvage(raw, &known).is_none(),
            "withheld tools must stay unreachable through the text channel"
        );
    }

    #[test]
    fn ordinary_json_in_an_answer_is_left_alone() {
        let known: BTreeSet<String> = ["WebSearch".to_string()].into_iter().collect();
        assert!(salvage(r#"{"name": "config.json", "size": 12}"#, &known).is_none());
        assert!(salvage("Here is the plan: search the web.", &known).is_none());
    }

    /// Text is held back only while it might still be a tool call, so ordinary
    /// prose keeps streaming token by token.
    #[test]
    fn only_json_shaped_output_is_held_back_from_the_user() {
        assert!(looks_like_tool_json("{\"name\""));
        assert!(looks_like_tool_json("  ```json"));
        assert!(looks_like_tool_json("<tool_call>"));
        assert!(!looks_like_tool_json("Today's headlines are"));
    }

    #[test]
    fn a_url_sent_to_grep_is_repaired_to_web_fetch() {
        let mut call = ToolCall {
            id: "call-1".to_string(),
            name: "grep_search".to_string(),
            input: serde_json::json!({
                "path": "https://example.com/news/today",
                "pattern": "headline"
            }),
        };

        normalize_tool_call(&mut call, None, None);

        assert_eq!(call.name, "WebFetch");
        assert_eq!(call.input["url"], "https://example.com/news/today");
        assert_eq!(call.input["prompt"], "headline");
        assert_eq!(call.id, "call-1", "the protocol call id must not change");
    }

    #[test]
    fn ordinary_file_grep_is_not_rewritten() {
        let mut call = ToolCall {
            id: "call-2".to_string(),
            name: "grep_search".to_string(),
            input: serde_json::json!({ "path": "src", "pattern": "main" }),
        };
        let original = call.clone();

        normalize_tool_call(&mut call, None, None);

        assert_eq!(call.name, original.name);
        assert_eq!(call.input, original.input);
    }

    #[test]
    fn web_fetch_missing_its_required_prompt_is_completed() {
        let mut call = ToolCall {
            id: "call-3".to_string(),
            name: "WebFetch".to_string(),
            input: serde_json::json!({ "url": "https://example.com" }),
        };

        normalize_tool_call(&mut call, None, None);

        assert!(call.input["prompt"]
            .as_str()
            .is_some_and(|prompt| !prompt.is_empty()));
    }

    #[test]
    fn non_http_schemes_are_never_promoted_to_web_fetch() {
        let mut call = ToolCall {
            id: "call-4".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({ "path": "file:///C:/secret.txt" }),
        };

        normalize_tool_call(&mut call, None, None);

        assert_eq!(call.name, "read_file");
    }

    #[test]
    fn an_invented_webfetch_staging_file_reuses_the_last_url() {
        let mut call = ToolCall {
            id: "call-5".to_string(),
            name: "grep_search".to_string(),
            input: serde_json::json!({
                "path": "data/1.html",
                "pattern": "headline"
            }),
        };

        normalize_tool_call(
            &mut call,
            Some("https://news.example.com/today"),
            None,
        );

        assert_eq!(call.name, "WebFetch");
        assert_eq!(call.input["url"], "https://news.example.com/today");
        assert_eq!(call.input["prompt"], "headline");
    }

    #[test]
    fn a_real_staging_file_is_not_rewritten() {
        let root = std::env::temp_dir().join("disco-real-webfetch-staging");
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("1.html"), "real local file").unwrap();
        let workspace = Workspace::open(root.to_str().unwrap()).unwrap();
        let mut call = ToolCall {
            id: "call-6".to_string(),
            name: "grep_search".to_string(),
            input: serde_json::json!({ "path": "data/1.html", "pattern": "local" }),
        };

        normalize_tool_call(
            &mut call,
            Some("https://news.example.com/today"),
            Some(&workspace),
        );

        assert_eq!(call.name, "grep_search");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancellation_wakes_a_task_that_is_waiting() {
        let token = Arc::new(CancelToken::new());
        let cancelling = Arc::clone(&token);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            cancelling.cancel();
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("cancellation must wake without waiting for a model event");
        assert!(token.is_cancelled());
    }

    /// Collects what the loop did, so a live run can be asserted on.
    #[derive(Default)]
    struct Recorder {
        text: std::sync::Mutex<String>,
        tools: std::sync::Mutex<Vec<String>>,
        failures: std::sync::Mutex<Vec<String>>,
    }

    impl Sink for Recorder {
        fn text(&self, text: &str) {
            self.text.lock().unwrap().push_str(text);
        }
        fn thinking(&self, _: &str) {}
        fn tool_start(&self, name: &str, _: &str) {
            self.tools.lock().unwrap().push(name.to_string());
        }
        fn tool_end(&self, name: &str, ok: bool, detail: &str) {
            if !ok {
                self.failures
                    .lock()
                    .unwrap()
                    .push(format!("{name}: {detail}"));
            }
        }
    }

    /// The regression this whole module exists for.
    ///
    /// Asking for current events used to produce confident invented prose,
    /// because the model was handed no tools and narration was all it had. A
    /// passing run proves the model actually reached the network.
    #[tokio::test]
    #[ignore = "requires a running Ollama daemon and network access"]
    async fn a_current_events_question_actually_calls_web_search() {
        let model = std::env::var("DISCO_TEST_MODEL")
            .unwrap_or_else(|_| "qwen2.5-coder:7b".to_string());
        let client = api::ProviderClient::from_model(&model).expect("client");
        let cancel = Arc::new(CancelToken::new());
        let recorder = Recorder::default();

        let outcome = run(
            &client,
            &model,
            system_prompt("2026-09-13", None),
            "What are today's top news headlines?".to_string(),
            false,
            None,
            &cancel,
            &recorder,
        )
        .await
        .expect("the loop ran");

        let tools_used = recorder.tools.lock().unwrap().clone();
        let failures = recorder.failures.lock().unwrap().clone();
        assert!(
            tools_used.iter().any(|name| name == "WebSearch"),
            "model answered a current-events question without searching; tools used: \
             {tools_used:?}, failures: {failures:?}, text: {}",
            outcome.text.chars().take(400).collect::<String>()
        );
        assert!(
            !failures.iter().any(|failure| {
                failure.contains("filename, directory name, or volume label syntax")
                    || failure.contains("filename or volume label syntax")
            }),
            "an HTTP URL reached a Windows filesystem tool: {failures:?}"
        );
        assert!(
            !outcome.text.trim().is_empty(),
            "the loop produced no answer at all"
        );
    }

    /// A local model may spend a long time before its next visible token. Stop
    /// must interrupt that wait rather than merely set a flag to be noticed
    /// after generation has already finished.
    #[tokio::test]
    #[ignore = "requires a running Ollama daemon"]
    async fn a_live_generation_can_be_cancelled_without_waiting_for_a_token() {
        let model = std::env::var("DISCO_TEST_MODEL")
            .unwrap_or_else(|_| "granite4.1:8b".to_string());
        let client = api::ProviderClient::from_model(&model).expect("client");
        let cancel = Arc::new(CancelToken::new());
        let cancelling = Arc::clone(&cancel);
        let recorder = Recorder::default();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            cancelling.cancel();
        });

        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run(
                &client,
                &model,
                system_prompt("2026-09-13", None),
                "Write a detailed analysis of five approaches to designing a compiler."
                    .to_string(),
                true,
                None,
                &cancel,
                &recorder,
            ),
        )
        .await
        .expect("stop should not wait for another model token")
        .expect("the cancelled run should end normally");

        assert!(outcome.cancelled);
    }

    /// The user's screenshot showed the model claiming it had created
    /// `hello_world.py` when no file existed. This asserts the file is really
    /// on disk afterwards, which narration cannot fake.
    #[tokio::test]
    #[ignore = "requires a running Ollama daemon"]
    async fn asking_for_a_file_actually_writes_it() {
        let model =
            std::env::var("DISCO_TEST_MODEL").unwrap_or_else(|_| "granite4.1:8b".to_string());
        let dir = std::env::temp_dir().join("disco-agent-live-write");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        let client = api::ProviderClient::from_model(&model).expect("client");
        let cancel = Arc::new(CancelToken::new());
        let recorder = Recorder::default();

        let outcome = run(
            &client,
            &model,
            system_prompt("2026-09-13", Some(&workspace)),
            "Create hello_world.py containing a program that prints Hello, World!".to_string(),
            false,
            Some(&workspace),
            &cancel,
            &recorder,
        )
        .await
        .expect("the loop ran");

        let written = dir.join("hello_world.py");
        assert!(
            written.is_file(),
            "model said it wrote the file but nothing is on disk; tools: {:?}, failures: {:?}, \
             text: {}",
            recorder.tools.lock().unwrap(),
            recorder.failures.lock().unwrap(),
            outcome.text.chars().take(400).collect::<String>()
        );
        let body = std::fs::read_to_string(&written).unwrap();
        assert!(body.contains("Hello"), "wrote the wrong contents: {body}");
    }
}
