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
            detail: Some(
                "No models found. Check that Ollama is running and that you have pulled at \
                 least one model with `ollama pull`."
                    .to_string(),
            ),
        };
    }

    DaemonStatus {
        host,
        reachable: true,
        models: models.into_iter().map(ModelDto::from).collect(),
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
}

/// Run a request against the local daemon, streaming output to the webview.
///
/// Local generation legitimately runs for minutes, so this streams rather than
/// returning a completed string: a chat that blanks for ten minutes is
/// indistinguishable from one that has crashed. When `enhance` is set the
/// request is routed through the staged harness, and each stage is announced so
/// the interface can show which one is running.
#[tauri::command]
pub async fn send_prompt(
    app: tauri::AppHandle,
    channel: tauri::ipc::Channel<TurnEvent>,
    request: String,
    model: String,
    enhance: bool,
) -> Result<(), String> {
    let _ = &app;

    let stages: Vec<runtime::enhance::Stage> = if enhance {
        runtime::enhance::triage(&request).stages
    } else {
        Vec::new()
    };

    let prompt = runtime::enhance::EnhancedPrompt::new(&request);
    let client = api::ProviderClient::from_model(&model).map_err(|error| error.to_string())?;

    if stages.is_empty() {
        stream_once(&client, &model, &request, &channel, None, 0, 1).await?;
        return Ok(());
    }

    let total = stages.len();
    let mut carry: Vec<(runtime::enhance::Stage, String)> = Vec::new();
    for (index, stage) in stages.iter().enumerate() {
        let rendered = prompt.render_stage(*stage, &carry);
        let produced = stream_once(
            &client,
            &model,
            &rendered,
            &channel,
            Some(stage.label()),
            index,
            total,
        )
        .await?;
        carry.push((*stage, produced));
    }

    let _ = channel.send(TurnEvent::Done);
    Ok(())
}

/// Stream one turn, returning the visible text it produced.
///
/// The text is returned as well as streamed because the harness needs to carry
/// each stage's output into the next one.
async fn stream_once(
    client: &api::ProviderClient,
    model: &str,
    prompt: &str,
    channel: &tauri::ipc::Channel<TurnEvent>,
    stage: Option<&str>,
    index: usize,
    total: usize,
) -> Result<String, String> {
    if let Some(label) = stage {
        let _ = channel.send(TurnEvent::StageStart {
            stage: label.to_string(),
            index,
            total,
        });
    }

    let request = api::MessageRequest {
        model: model.to_string(),
        max_tokens: 4096,
        messages: vec![api::InputMessage::user_text(prompt)],
        stream: true,
        ..Default::default()
    };

    let mut stream = match client.stream_message(&request).await {
        Ok(stream) => stream,
        Err(error) => {
            let message = error.to_string();
            let _ = channel.send(TurnEvent::Failed {
                message: message.clone(),
            });
            return Err(message);
        }
    };

    let mut collected = String::new();
    loop {
        match stream.next_event().await {
            Ok(Some(event)) => {
                if let api::StreamEvent::ContentBlockDelta(delta) = event {
                    match delta.delta {
                        api::ContentBlockDelta::TextDelta { text } => {
                            collected.push_str(&text);
                            let _ = channel.send(TurnEvent::Text { text });
                        }
                        api::ContentBlockDelta::ThinkingDelta { thinking } => {
                            let _ = channel.send(TurnEvent::Thinking { text: thinking });
                        }
                        _ => {}
                    }
                }
            }
            Ok(None) => break,
            Err(error) => {
                let message = error.to_string();
                let _ = channel.send(TurnEvent::Failed {
                    message: message.clone(),
                });
                return Err(message);
            }
        }
    }

    if stage.is_none() {
        let _ = channel.send(TurnEvent::Done);
    }
    Ok(collected)
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
    /// without a real daemon — mocking it would only assert that the mock works.
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
    }
}
