//! Tauri commands: the bridge between the webview and the Rust core.
//!
//! opencode's frontend talks to its backend over a 56-endpoint HTTP API whose
//! payload shapes are generated from Effect schemas, and whose handlers are the
//! opencode TypeScript core. Reimplementing that surface in Rust would mean
//! reimplementing opencode's session model, event bus, permission system and
//! PTY layer to byte-compatible JSON, and then chasing it on every upstream
//! release. This crate takes the other option: the Rust core is already the
//! product, so the webview reaches it through Tauri IPC directly and the HTTP
//! protocol is not adopted at all. What is adopted from opencode is its
//! presentation layer, which carries no protocol coupling.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use crate::agent;

/// Cancel flags for the turns currently in flight, keyed by conversation.
///
/// Keyed rather than single, because conversations run independently: a turn
/// started in one chat must not be cancelled by a turn starting in another, and
/// the stop button has to abandon the turn the user is actually looking at. A
/// local model can spend minutes in a hidden scratchpad, so a turn that cannot
/// be abandoned is a turn that holds the interface hostage.
fn cancel_flags() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Begin a turn for one conversation, cancelling any earlier turn in that same
/// conversation only.
fn begin_turn(turn_id: &str) -> Arc<AtomicBool> {
    let token = Arc::new(AtomicBool::new(false));
    if let Ok(mut flags) = cancel_flags().lock() {
        if let Some(previous) = flags.insert(turn_id.to_string(), Arc::clone(&token)) {
            previous.store(true, Ordering::SeqCst);
        }
    }
    token
}

/// Release a finished turn, so the table does not grow with dead entries.
///
/// Only removes the entry when it is still the one this turn registered: a
/// newer turn for the same conversation must keep its own flag.
fn end_turn(turn_id: &str, token: &Arc<AtomicBool>) {
    if let Ok(mut flags) = cancel_flags().lock() {
        if flags
            .get(turn_id)
            .is_some_and(|current| Arc::ptr_eq(current, token))
        {
            flags.remove(turn_id);
        }
    }
}

/// Ask one conversation's running turn to stop at the next interruption point.
#[tauri::command]
pub fn cancel_turn(turn_id: String) {
    if let Ok(flags) = cancel_flags().lock() {
        if let Some(token) = flags.get(&turn_id) {
            token.store(true, Ordering::SeqCst);
        }
    }
}

/// A model the local daemon can serve, flattened for the webview.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelDto {
    pub id: String,
    pub name: String,
    pub context: u32,
    pub output: u32,
    pub tools: bool,
    pub vision: bool,
    pub thinking: bool,
    /// False when the model cannot call tools.
    ///
    /// Surfaced rather than filtered: a user who pulled a model and cannot see
    /// it in the list will assume detection is broken. Showing it as
    /// unavailable, with the reason, is the honest failure.
    pub usable: bool,
}

impl From<api::OllamaModel> for ModelDto {
    fn from(model: api::OllamaModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            context: model.context,
            output: model.output,
            tools: model.caps.tools,
            vision: model.caps.vision,
            thinking: model.caps.thinking,
            usable: model.caps.tools,
        }
    }
}

/// Whether the daemon is reachable, and what it is serving.
#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub host: String,
    pub reachable: bool,
    pub models: Vec<ModelDto>,
    /// Embedding-only models, reported apart from the chat models.
    ///
    /// These are deliberately kept out of `models`: an embedding model in a
    /// chat picker is a category error, not a disabled choice, because no
    /// setting the user can change will let it answer a prompt. They are still
    /// named so that a pulled model never silently disappears, and because the
    /// workspace index needs one to exist.
    pub embedding_models: Vec<String>,
    /// Present only when the daemon could not be reached.
    pub detail: Option<String>,
}

/// One stage the enhancement harness would run for a request.
#[derive(Debug, Clone, Serialize)]
pub struct StageDto {
    pub stage: String,
    pub directive: String,
}

/// The harness's routing decision for a request.
#[derive(Debug, Clone, Serialize)]
pub struct TriageDto {
    pub complexity: String,
    pub rationale: String,
    pub signals: Vec<String>,
    pub stages: Vec<StageDto>,
}

/// Report the daemon's reachability and the models it is serving.
///
/// Inference is Ollama-only by construction, so this doubles as the app's
/// health check: if this is empty, nothing else in the product can work, and
/// the interface should say so rather than presenting an inert chat box.
#[tauri::command]
pub async fn daemon_status() -> DaemonStatus {
    let host = api::ollama_host();
    let http = reqwest_client();

    let models = api::ollama_list(&http).await;
    if models.is_empty() {
        return DaemonStatus {
            host,
            reachable: false,
            models: Vec::new(),
            embedding_models: Vec::new(),
            detail: Some(
                "No models found. Check that Ollama is running and that you have pulled at \
                 least one model with `ollama pull`."
                    .to_string(),
            ),
        };
    }

    let (chat, embedding): (Vec<_>, Vec<_>) =
        models.into_iter().partition(|m| m.caps.is_chat());
    let embedding_models: Vec<String> = embedding.into_iter().map(|m| m.id).collect();

    // Having only embedding models is indistinguishable from having none, as
    // far as holding a conversation goes, so it is reported as such instead of
    // presenting a picker that cannot produce an answer.
    if chat.is_empty() {
        return DaemonStatus {
            host,
            reachable: false,
            models: Vec::new(),
            embedding_models,
            detail: Some(
                "Only embedding models are installed. Embedding models index text and cannot \
                 hold a conversation; pull a chat model, for example `ollama pull \
                 qwen2.5-coder:7b`."
                    .to_string(),
            ),
        };
    }

    DaemonStatus {
        host,
        reachable: true,
        models: chat.into_iter().map(ModelDto::from).collect(),
        embedding_models,
        detail: None,
    }
}

/// Report how the enhancement harness would route a request.
///
/// Deliberately exposed on its own. Triage is deterministic and costs no
/// inference, so the interface can show what a request will trigger *before*
/// committing minutes of local generation to it.
#[tauri::command]
#[must_use]
pub fn triage_request(request: String) -> TriageDto {
    let triage = runtime::enhance::triage(&request);
    TriageDto {
        complexity: triage.complexity.label().to_string(),
        rationale: triage.rationale(),
        signals: triage
            .signals
            .iter()
            .map(|signal| signal.label().to_string())
            .collect(),
        stages: triage
            .stages
            .iter()
            .map(|stage| StageDto {
                stage: stage.label().to_string(),
                directive: stage.directive().to_string(),
            })
            .collect(),
    }
}

fn reqwest_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_default()
}

/// A chunk of a running generation, pushed to the webview as it arrives.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnEvent {
    /// A stage of the harness began. Absent for unenhanced runs.
    StageStart { stage: String, index: usize, total: usize },
    /// Visible assistant text.
    Text { text: String },
    /// Reasoning output, kept separate so the interface can fold it away.
    Thinking { text: String },
    /// The run finished normally.
    Done,
    /// The run failed. Carries the reason rather than a bare failure.
    Failed { message: String },
    /// The run was abandoned at the user's request.
    Cancelled,
    /// A tool call started. Shown so tool use is visible rather than implied.
    ToolStart { name: String, summary: String },
    /// A tool call finished, with a one-line trace of what came back.
    ToolEnd { name: String, ok: bool, detail: String },
}

/// Bridges loop events onto the webview channel.
struct ChannelSink<'a> {
    channel: &'a tauri::ipc::Channel<TurnEvent>,
}

impl agent::Sink for ChannelSink<'_> {
    fn text(&self, text: &str) {
        let _ = self.channel.send(TurnEvent::Text {
            text: text.to_string(),
        });
    }

    fn thinking(&self, text: &str) {
        let _ = self.channel.send(TurnEvent::Thinking {
            text: text.to_string(),
        });
    }

    fn tool_start(&self, name: &str, summary: &str) {
        let _ = self.channel.send(TurnEvent::ToolStart {
            name: name.to_string(),
            summary: summary.to_string(),
        });
    }

    fn tool_end(&self, name: &str, ok: bool, detail: &str) {
        let _ = self.channel.send(TurnEvent::ToolEnd {
            name: name.to_string(),
            ok,
            detail: detail.to_string(),
        });
    }
}

/// Run a request against the local daemon, streaming output to the webview.
///
/// Local generation legitimately runs for minutes, so this streams rather than
/// returning a completed string: a chat that blanks for ten minutes is
/// indistinguishable from one that has crashed. When `enhance` is set the
/// request is routed through the staged harness, and each stage is announced so
/// the interface can show which one is running.
///
/// Every stage runs the full agent loop, so the model can call tools at any
/// point. An earlier version sent no tools at all, which meant the model could
/// only describe the research it would have done and then answer from memory.
#[tauri::command]
pub async fn send_prompt(
    app: tauri::AppHandle,
    channel: tauri::ipc::Channel<TurnEvent>,
    turn_id: String,
    request: String,
    model: String,
    enhance: bool,
    reasoning: bool,
) -> Result<(), String> {
    let _ = &app;
    let cancel = begin_turn(&turn_id);
    let result = run_turn(&channel, &request, &model, enhance, reasoning, &cancel).await;
    end_turn(&turn_id, &cancel);

    match result {
        Ok(()) => Ok(()),
        Err(message) => {
            let _ = channel.send(TurnEvent::Failed {
                message: message.clone(),
            });
            Err(message)
        }
    }
}

async fn run_turn(
    channel: &tauri::ipc::Channel<TurnEvent>,
    request: &str,
    model: &str,
    enhance: bool,
    reasoning: bool,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let stages: Vec<runtime::enhance::Stage> = if enhance {
        runtime::enhance::triage(request).stages
    } else {
        Vec::new()
    };

    let client = api::ProviderClient::from_model(model).map_err(|error| error.to_string())?;
    let system = agent::system_prompt(&chrono::Local::now().format("%Y-%m-%d").to_string());
    let sink = ChannelSink { channel };

    if stages.is_empty() {
        let outcome = agent::run(
            &client,
            model,
            system,
            request.to_string(),
            reasoning,
            cancel,
            &sink,
        )
        .await?;
        let _ = channel.send(if outcome.cancelled {
            TurnEvent::Cancelled
        } else {
            TurnEvent::Done
        });
        return Ok(());
    }

    let prompt = runtime::enhance::EnhancedPrompt::new(request);
    let total = stages.len();
    let mut carry: Vec<(runtime::enhance::Stage, String)> = Vec::new();

    for (index, stage) in stages.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            let _ = channel.send(TurnEvent::Cancelled);
            return Ok(());
        }

        let _ = channel.send(TurnEvent::StageStart {
            stage: stage.label().to_string(),
            index,
            total,
        });

        let outcome = agent::run(
            &client,
            model,
            system.clone(),
            prompt.render_stage(*stage, &carry),
            reasoning,
            cancel,
            &sink,
        )
        .await?;

        if outcome.cancelled {
            let _ = channel.send(TurnEvent::Cancelled);
            return Ok(());
        }
        carry.push((*stage, outcome.text));
    }

    let _ = channel.send(TurnEvent::Done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_without_tool_support_is_reported_unusable_rather_than_hidden() {
        let model = api::OllamaModel {
            id: "toolless:latest".to_string(),
            name: "toolless".to_string(),
            context: 8192,
            output: 4096,
            caps: api::OllamaCaps {
                tools: false,
                vision: false,
                thinking: false,
                completion: true,
                embedding: false,
            },
        };

        let dto = ModelDto::from(model);
        assert!(
            !dto.usable,
            "a model that cannot call tools cannot drive the agent loop"
        );
        assert_eq!(
            dto.id, "toolless:latest",
            "it must still be listed, so the user can see why it is unavailable"
        );
    }

    #[test]
    fn model_capabilities_survive_the_conversion() {
        let model = api::OllamaModel {
            id: "qwen3.5:9b".to_string(),
            name: "qwen3.5".to_string(),
            context: 262_144,
            output: 32_768,
            caps: api::OllamaCaps {
                tools: true,
                vision: false,
                thinking: true,
                completion: true,
                embedding: false,
            },
        };

        let dto = ModelDto::from(model);
        assert_eq!(dto.context, 262_144);
        assert_eq!(dto.output, 32_768);
        assert!(dto.tools && dto.thinking && !dto.vision);
        assert!(dto.usable);
    }

    #[test]
    fn triage_routing_is_exposed_to_the_interface() {
        let trivial = triage_request("fix the typo in README.md".to_string());
        assert_eq!(trivial.complexity, "trivial");
        assert_eq!(
            trivial.stages.len(),
            1,
            "a trivial request must not be turned into a pipeline"
        );

        let vague = triage_request("clean this up".to_string());
        assert_eq!(vague.stages.len(), 5, "a vague request earns the full run");
        assert!(
            !vague.stages[0].directive.is_empty(),
            "the interface needs the directive text to explain the stage"
        );
    }

    /// Opt-in: requires a running Ollama daemon.
    ///
    /// Ignored by default so the suite stays hermetic, but kept in the tree
    /// because model detection is the one behaviour that cannot be proven
    /// without a real daemon â€” mocking it would only assert that the mock works.
    /// Run with `cargo test -p desktop -- --ignored`.
    #[tokio::test]
    #[ignore = "requires a running Ollama daemon"]
    async fn daemon_status_detects_live_models() {
        let status = daemon_status().await;
        assert!(
            status.reachable,
            "expected a running daemon at {}: {:?}",
            status.host, status.detail
        );
        assert!(!status.models.is_empty(), "a reachable daemon served no models");
        assert!(
            status.models.iter().any(|model| model.usable),
            "no detected model can call tools, so the agent loop cannot run"
        );
        for model in &status.models {
            assert!(
                model.context > 0,
                "{} reported no context window, so budgeting would be guesswork",
                model.id
            );
        }
        assert!(
            !status
                .models
                .iter()
                .any(|model| status.embedding_models.contains(&model.id)),
            "an embedding model reached the chat picker, where it can never answer"
        );
    }
}
