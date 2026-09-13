//! The agent loop: what turns a chat completion into an agent.
//!
//! Without this, the model is handed no tools and can only describe what it
//! would do — it says "I will now search the web" and then invents the answer,
//! because narration is the only thing available to it. The loop gives the
//! model real tools, executes what it asks for, feeds the results back, and
//! repeats until it stops asking.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use api::{
    ContentBlockDelta, InputContentBlock, InputMessage, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition,
};
use serde_json::Value;

/// Tools the desktop app exposes to the model.
///
/// Deliberately read-only, plus network research. The desktop has no approval
/// prompt yet, and a local model that has misread the request should not be one
/// malformed tool call away from running a shell command or rewriting a file.
/// Everything here either reads or fetches, so the worst outcome of a confused
/// call is a wasted turn.
///
/// `bash`, `PowerShell`, `write_file` and `edit_file` are withheld for exactly
/// that reason, and should be added when there is a UI to approve them.
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

/// Upper bound on tool round trips in a single turn.
///
/// A model that has misunderstood a tool result will often re-issue the same
/// call forever. The cap turns that into a bounded, explainable stop instead of
/// a session that runs until the user kills it.
const MAX_STEPS: usize = 8;

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
#[must_use]
pub fn definitions() -> Vec<ToolDefinition> {
    let allowed: BTreeSet<String> = ALLOWED_TOOLS
        .iter()
        .map(|name| tools::canonical_allowed_tool_name(name))
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
pub fn system_prompt(today: &str) -> String {
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
         - When you have used web results, end with a Sources section listing the URLs.\n\
         - You cannot run shell commands or modify files in this app. If a request needs that, \
         say so plainly and give the exact commands the user should run.\n\
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
    cancel: &Arc<AtomicBool>,
    sink: &dyn Sink,
) -> Result<Outcome, String> {
    let tool_defs = definitions();
    let mut messages = vec![InputMessage::user_text(first)];
    let mut transcript = String::new();

    for step in 0..MAX_STEPS {
        if cancel.load(Ordering::SeqCst) {
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

        let step_result = stream_step(client, &request, cancel, sink).await?;
        if step_result.cancelled {
            return Ok(Outcome {
                text: transcript,
                cancelled: true,
            });
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
            if cancel.load(Ordering::SeqCst) {
                return Ok(Outcome {
                    text: transcript,
                    cancelled: true,
                });
            }

            sink.tool_start(&call.name, &summarize(&call.name, &call.input));
            let (output, is_error) = execute(&call).await;
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
    cancel: &Arc<AtomicBool>,
    sink: &dyn Sink,
) -> Result<StepResult, String> {
    let mut stream = client
        .stream_message(request)
        .await
        .map_err(|error| error.to_string())?;

    let mut text = String::new();
    let mut emitted = 0usize;
    let mut pending: Vec<(u32, String, String, String)> = Vec::new();

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(StepResult {
                text,
                calls: Vec::new(),
                cancelled: true,
            });
        }

        match stream.next_event().await {
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
async fn execute(call: &ToolCall) -> (String, bool) {
    let name = call.name.clone();
    let input = call.input.clone();

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

    #[test]
    fn web_search_is_offered_to_the_model() {
        let names: Vec<String> = definitions().into_iter().map(|d| d.name).collect();
        assert!(
            names.iter().any(|n| n == "WebSearch"),
            "without WebSearch the model answers current-events questions from memory: {names:?}"
        );
        assert!(names.iter().any(|n| n == "read_file"));
        assert!(!names.is_empty());
    }

    #[test]
    fn mutating_tools_are_withheld_until_there_is_an_approval_ui() {
        let names: Vec<String> = definitions().into_iter().map(|d| d.name).collect();
        for withheld in ["bash", "PowerShell", "write_file", "edit_file"] {
            assert!(
                !names.iter().any(|n| n == withheld),
                "{withheld} can change the machine and the desktop cannot yet ask permission"
            );
        }
    }

    #[test]
    fn the_system_prompt_dates_the_conversation_and_forbids_invention() {
        let prompt = system_prompt("2026-09-13");
        assert!(
            prompt.contains("2026-09-13"),
            "without today's date, 'latest' resolves against the training cut-off"
        );
        assert!(prompt.contains("WebSearch"));
        assert!(prompt.to_lowercase().contains("never invent"));
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
        let cancel = Arc::new(AtomicBool::new(false));
        let recorder = Recorder::default();

        let outcome = run(
            &client,
            &model,
            system_prompt("2026-09-13"),
            "What are today's top news headlines?".to_string(),
            false,
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
            !outcome.text.trim().is_empty(),
            "the loop produced no answer at all"
        );
    }
}
