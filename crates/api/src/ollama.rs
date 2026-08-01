//! Native Ollama model discovery.
//!
//! Disco Code performs inference exclusively against a local Ollama daemon, so
//! the daemon — not a baked-in catalog — is the source of truth about which
//! models exist and what they can do. A model pulled a minute ago is available
//! immediately, and its real context window is used rather than a guess.
//!
//! Inference traffic itself goes through the OpenAI-compatible surface Ollama
//! exposes at `<host>/v1`; this module only covers discovery, which uses
//! Ollama's native `/api/tags` and `/api/show` endpoints because the
//! OpenAI-compatible `/v1/models` response omits context length and
//! capabilities.

use serde::Deserialize;

use crate::error::ApiError;

/// Where the Ollama daemon listens when nothing says otherwise.
pub const DEFAULT_HOST: &str = "http://127.0.0.1:11434";

/// Context window assumed when the daemon reports no usable value.
const FALLBACK_CONTEXT: u32 = 8_192;

/// Upper bound applied to the derived output-token budget.
const MAX_OUTPUT: u32 = 32_768;

/// A model the local daemon can serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Identifier as Ollama knows it, e.g. `qwen3.5:9b`.
    pub id: String,
    /// Human-facing label.
    pub name: String,
    /// True context window, read from the model's own metadata.
    pub context: u32,
    /// Output-token budget derived from the context window.
    pub output: u32,
    pub caps: Caps,
}

/// Capabilities the daemon advertises for a model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// Model can call tools. Without this the agent loop cannot run.
    pub tools: bool,
    /// Model accepts image input.
    pub vision: bool,
    /// Model emits a separate reasoning channel.
    pub thinking: bool,
}

#[derive(Debug, Deserialize)]
struct Tags {
    #[serde(default)]
    models: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
struct Tag {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct Show {
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    model_info: serde_json::Map<String, serde_json::Value>,
}

/// Resolves the daemon address, tolerating the common bare `host:port` form.
///
/// `OLLAMA_HOST` is frequently set without a scheme (`127.0.0.1:11434`), which
/// is not a parseable URL, so a scheme is supplied when absent.
#[must_use]
pub fn host() -> String {
    match std::env::var("OLLAMA_HOST") {
        Ok(raw) => normalize(&raw),
        Err(_) => DEFAULT_HOST.to_string(),
    }
}

/// Normalizes a user-supplied host into an absolute URL without a trailing slash.
#[must_use]
pub fn normalize(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return DEFAULT_HOST.to_string();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    format!("http://{trimmed}")
}

/// Base URL for the OpenAI-compatible inference surface.
#[must_use]
pub fn base() -> String {
    format!("{}/v1", host())
}

impl Caps {
    fn parse(raw: &[String]) -> Self {
        Self {
            tools: raw.iter().any(|c| c == "tools"),
            vision: raw.iter().any(|c| c == "vision"),
            thinking: raw.iter().any(|c| c == "thinking"),
        }
    }
}

/// Extracts the context window from a model's metadata.
///
/// The key is architecture-prefixed (`qwen2.context_length`). The daemon's
/// declared `general.architecture` does not always agree with the prefix
/// actually used, so the declared architecture is tried first and any
/// `*.context_length` key is accepted as a fallback before giving up.
fn context(info: &serde_json::Map<String, serde_json::Value>) -> u32 {
    let arch = info
        .get("general.architecture")
        .and_then(serde_json::Value::as_str);

    if let Some(found) = arch
        .and_then(|a| info.get(&format!("{a}.context_length")))
        .and_then(serde_json::Value::as_u64)
    {
        return u32::try_from(found).unwrap_or(FALLBACK_CONTEXT);
    }

    info.iter()
        .find(|(k, _)| k.ends_with(".context_length"))
        .and_then(|(_, v)| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(FALLBACK_CONTEXT)
}

/// Derives an output-token budget from the context window.
///
/// Ollama reports no per-model output cap, so a quarter of the window is
/// reserved for the response and bounded so very large windows do not imply an
/// unreasonable single reply.
#[must_use]
pub fn output(context: u32) -> u32 {
    (context / 4).clamp(1, MAX_OUTPUT).min(context)
}

/// Chooses a sensible default from the installed models.
///
/// Tool-calling capability is preferred because the agent loop cannot function
/// without it; a chat-only model is returned only when nothing better exists.
#[must_use]
pub fn pick(models: &[Model]) -> Option<&Model> {
    models
        .iter()
        .find(|m| m.caps.tools)
        .or_else(|| models.first())
}

/// Model name meaning "whichever local model is best for the job".
///
/// Disco Code cannot ship a meaningful compiled-in default because the usable
/// models are whichever ones the user happens to have pulled. Startup therefore
/// stays completely offline and this placeholder travels through config, status
/// output and argument parsing untouched; it is exchanged for a real model name
/// only on the inference path, by [`resolve`].
pub const AUTO: &str = "ollama/auto";

/// Exchanges [`AUTO`] for a concrete model served by the local daemon.
///
/// Blocking, and deliberately so: it sits behind the synchronous alias
/// resolution that every request already passes through. The daemon is asked at
/// most once per process, and only once inference is actually attempted, so
/// commands like `status` or `version` never pay for it.
///
/// If nothing can be resolved the placeholder is returned unchanged so the
/// eventual error names the real problem rather than some unrelated model.
pub fn resolve() -> String {
    static PICK: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PICK.get_or_init(|| {
        // A nested runtime would panic when called from inside one, so the
        // lookup gets a thread of its own.
        std::thread::spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?
                .block_on(choose())
        })
        .join()
        .ok()
        .flatten()
        .unwrap_or_else(|| AUTO.to_string())
    })
    .clone()
}

/// Resolves a default model straight from the local daemon.
///
/// Convenience wrapper so callers that have no HTTP client of their own — the
/// CLI, for instance — do not need to depend on the transport crate.
pub async fn choose() -> Option<String> {
    // Discovery sits in front of inference, so an absent or wedged daemon must
    // surface fast rather than hang the agent. The connect budget is tighter
    // than the overall one because this is a loopback socket: on Windows a
    // closed local port retransmits in SYN_SENT rather than refusing outright,
    // so without this the wait is measured in tens of seconds.
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    pick(&list(&http).await).map(|m| m.id.clone())
}

/// Lists every model the local daemon can serve, with real context windows and
/// capabilities.
///
/// A daemon that is not running is not an error here: it yields an empty list
/// so callers can tell "Ollama is unreachable" apart from "Ollama has no models"
/// and guide the user accordingly.
pub async fn list(http: &reqwest::Client) -> Vec<Model> {
    let Ok(tags) = tags(http).await else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(tags.len());
    for name in tags {
        // A model whose metadata cannot be read is still usable for chat, so it
        // is kept with conservative defaults rather than dropped.
        let show = show(http, &name).await.unwrap_or_default();
        let window = context(&show.model_info);
        out.push(Model {
            id: name.clone(),
            name,
            context: window,
            output: output(window),
            caps: Caps::parse(&show.capabilities),
        });
    }
    out
}

/// Fetches the installed model names from the daemon.
pub async fn tags(http: &reqwest::Client) -> Result<Vec<String>, ApiError> {
    let url = format!("{}/api/tags", host());
    let body = http
        .get(&url)
        .send()
        .await
        .map_err(ApiError::Http)?
        .error_for_status()
        .map_err(ApiError::Http)?
        .json::<Tags>()
        .await
        .map_err(ApiError::Http)?;

    Ok(body
        .models
        .into_iter()
        .map(|m| m.name)
        .filter(|n| !n.is_empty())
        .collect())
}

async fn show(http: &reqwest::Client, name: &str) -> Result<Show, ApiError> {
    let url = format!("{}/api/show", host());
    http.post(&url)
        .json(&serde_json::json!({ "model": name }))
        .send()
        .await
        .map_err(ApiError::Http)?
        .error_for_status()
        .map_err(ApiError::Http)?
        .json::<Show>()
        .await
        .map_err(ApiError::Http)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().expect("object").clone()
    }

    #[test]
    fn normalize_supplies_missing_scheme() {
        assert_eq!(normalize("127.0.0.1:11434"), "http://127.0.0.1:11434");
        assert_eq!(normalize("localhost:11434"), "http://localhost:11434");
    }

    #[test]
    fn normalize_preserves_explicit_scheme_and_strips_trailing_slash() {
        assert_eq!(normalize("https://ollama.box:443/"), "https://ollama.box:443");
        assert_eq!(normalize("http://127.0.0.1:11434"), "http://127.0.0.1:11434");
    }

    #[test]
    fn normalize_falls_back_when_blank() {
        assert_eq!(normalize("   "), DEFAULT_HOST);
        assert_eq!(normalize(""), DEFAULT_HOST);
    }

    #[test]
    fn context_reads_declared_architecture_key() {
        let got = context(&info(json!({
            "general.architecture": "qwen2",
            "qwen2.context_length": 262_144,
        })));
        assert_eq!(got, 262_144);
    }

    #[test]
    fn context_falls_back_when_declared_architecture_mismatches_key_prefix() {
        // Observed in the wild: general.architecture disagrees with the prefix
        // actually used for the metadata keys.
        let got = context(&info(json!({
            "general.architecture": "gemma3",
            "gemma4.context_length": 131_072,
        })));
        assert_eq!(got, 131_072);
    }

    #[test]
    fn context_defaults_when_absent() {
        let got = context(&info(json!({ "general.architecture": "llama" })));
        assert_eq!(got, FALLBACK_CONTEXT);
    }

    fn model(id: &str, tools: bool) -> Model {
        Model {
            id: id.to_string(),
            name: id.to_string(),
            context: FALLBACK_CONTEXT,
            output: output(FALLBACK_CONTEXT),
            caps: Caps {
                tools,
                ..Caps::default()
            },
        }
    }

    #[test]
    fn pick_prefers_a_tool_capable_model() {
        let models = [model("chat-only", false), model("agentic", true)];
        assert_eq!(
            pick(&models).map(|m| m.id.as_str()),
            Some("agentic"),
            "the agent loop requires tool calling"
        );
    }

    #[test]
    fn pick_falls_back_to_the_first_model_when_none_support_tools() {
        let models = [model("first", false), model("second", false)];
        assert_eq!(pick(&models).map(|m| m.id.as_str()), Some("first"));
    }

    #[test]
    fn pick_returns_nothing_when_no_models_are_installed() {
        assert!(pick(&[]).is_none());
    }

    #[test]
    fn caps_map_daemon_vocabulary() {
        let caps = Caps::parse(&[
            "completion".to_string(),
            "tools".to_string(),
            "thinking".to_string(),
        ]);
        assert!(caps.tools, "tools capability drives the agent loop");
        assert!(caps.thinking);
        assert!(!caps.vision);
    }

    #[test]
    fn caps_default_to_none_when_unreported() {
        assert_eq!(Caps::parse(&[]), Caps::default());
    }

    #[test]
    fn output_is_a_bounded_share_of_context() {
        assert_eq!(output(32_768), 8_192);
        // Very large windows stay bounded rather than implying a huge reply.
        assert_eq!(output(262_144), MAX_OUTPUT);
    }

    #[test]
    fn output_never_exceeds_context() {
        assert!(output(1_024) <= 1_024);
        assert!(output(1) <= 1);
    }

    #[test]
    fn base_targets_the_openai_compatible_surface() {
        assert!(base().ends_with("/v1"), "inference uses the /v1 surface");
    }

    /// Exercises discovery against a real daemon. Ignored by default because it
    /// needs Ollama running; run with `cargo test -p api live_daemon -- --ignored`.
    #[test]
    #[ignore = "requires a running Ollama daemon"]
    fn live_daemon_reports_models_with_real_context_windows() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let models = rt.block_on(async { list(&reqwest::Client::new()).await });

        assert!(
            !models.is_empty(),
            "expected at least one pulled model; is Ollama running?"
        );
        for m in &models {
            println!(
                "{} context={} output={} tools={} vision={} thinking={}",
                m.id, m.context, m.output, m.caps.tools, m.caps.vision, m.caps.thinking
            );
            assert!(!m.id.is_empty());
            assert!(
                m.context >= FALLBACK_CONTEXT,
                "{} reported an implausible context window: {}",
                m.id,
                m.context
            );
            assert!(m.output <= m.context);
        }
    }

    /// Proves the deferred-resolution path really reaches the daemon and lands
    /// on a model it actually serves, rather than leaving the placeholder in
    /// place. Ignored by default for the same reason as above.
    #[test]
    #[ignore = "requires a running Ollama daemon"]
    fn live_daemon_swaps_auto_for_a_served_model() {
        let wire = crate::providers::resolve_model_alias(AUTO);
        println!("{AUTO} -> {wire}");
        assert_ne!(wire, AUTO, "daemon should have offered a model");

        let served = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async { tags(&reqwest::Client::new()).await })
            .expect("tags");
        assert!(
            served.contains(&wire),
            "resolved {wire} but daemon serves {served:?}"
        );
    }
}
