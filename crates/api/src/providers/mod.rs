#![allow(clippy::cast_possible_truncation)]
#![allow(dead_code)]
use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

use crate::error::ApiError;
use crate::types::{MessageRequest, MessageResponse};

pub mod openai_compat;

#[allow(dead_code)]
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

#[allow(dead_code)]
pub trait Provider {
    type Stream;

    fn send_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, MessageResponse>;

    fn stream_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, Self::Stream>;
}

/// The single provider Disco Code can reach.
///
/// Provider selection was a runtime decision when several hosted backends were
/// supported. Inference now happens only against a local Ollama daemon, so the
/// choice collapsed to a constant. Keeping the type as a one-variant enum
/// preserves the shape of the diagnostics and status surfaces that report which
/// provider served a request, and means reintroducing a second backend would be
/// a compile error at every site that must then make a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProviderKind {
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider: ProviderKind,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelTokenLimit {
    pub max_output_tokens: u32,
    pub context_window_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWireProtocol {
    AnthropicMessages,
    OpenAiChatCompletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFeatureSupport {
    Supported,
    Unsupported,
    PassthroughAsTool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCapabilityReport {
    pub provider: ProviderKind,
    pub wire_protocol: ProviderWireProtocol,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
    pub tool_calls: ProviderFeatureSupport,
    pub streaming: ProviderFeatureSupport,
    pub streaming_usage: ProviderFeatureSupport,
    pub prompt_cache: ProviderFeatureSupport,
    pub custom_parameters: ProviderFeatureSupport,
    pub reasoning_effort: ProviderFeatureSupport,
    pub reasoning_content_history: ProviderFeatureSupport,
    pub fixed_sampling_reasoning_models: ProviderFeatureSupport,
    pub web_search: ProviderFeatureSupport,
    pub web_fetch: ProviderFeatureSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderDiagnostic {
    pub code: &'static str,
    pub severity: ProviderDiagnosticSeverity,
    pub message: String,
    pub action: String,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderDiagnostics {
    pub requested_model: String,
    pub resolved_model: String,
    pub provider: ProviderKind,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
    pub openai_compatible: bool,
    pub reasoning_model: bool,
    pub preserves_reasoning_content_in_history: bool,
    pub strips_tuning_params: bool,
    pub supports_stream_usage: bool,
    pub honors_proxy_env: bool,
    pub supports_extra_body_params: bool,
    pub preserves_slash_model_ids_on_custom_base_url: bool,
}


pub fn resolve_model_alias(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed == crate::ollama::AUTO {
        return crate::ollama::resolve();
    }
    // Every other name is passed through untouched. Aliases such as `opus` or
    // `grok` used to expand to hosted model ids; the valid names are now
    // whatever the local daemon serves, which only the daemon can enumerate.
    trimmed.to_string()
}

/// Describes the local daemon backing every request.
///
/// This used to vary per model so that a name could select a hosted backend and
/// its credentials. Inference is now local-only, so the answer is constant. It
/// stays an `Option` because callers treat `None` as "not a routable model" and
/// collapsing that would ripple further than this checkpoint.
#[must_use]
pub fn metadata_for_model(_model: &str) -> Option<ProviderMetadata> {
    Some(local_metadata())
}

/// Metadata for the local daemon.
///
/// `auth_env` names `OLLAMA_HOST` rather than a key variable: Ollama needs no
/// credentials, and the field is what status output uses to tell the user which
/// variable governs the connection.
#[must_use]
pub const fn local_metadata() -> ProviderMetadata {
    ProviderMetadata {
        provider: ProviderKind::Ollama,
        auth_env: "OLLAMA_HOST",
        base_url_env: "OLLAMA_HOST",
        default_base_url: crate::ollama::DEFAULT_HOST,
    }
}


#[must_use]
pub fn strip_provider_prefix(canonical_model: &str) -> String {
    if let Some(pos) = canonical_model.find('/') {
        canonical_model[pos + 1..].to_string()
    } else {
        canonical_model.to_string()
    }
}

#[must_use]
pub fn provider_diagnostics_for_model(model: &str) -> ProviderDiagnostics {
    let resolved_model = resolve_model_alias(model);
    let metadata = local_metadata();
    // Ollama serves the OpenAI chat-completions shape, so the compatibility
    // flags that used to vary per provider are now uniformly true.
    let openai_compatible = true;
    let reasoning_model = openai_compat::is_reasoning_model(&resolved_model);

    ProviderDiagnostics {
        requested_model: model.to_string(),
        resolved_model: resolved_model.clone(),
        provider: metadata.provider,
        auth_env: metadata.auth_env,
        base_url_env: metadata.base_url_env,
        default_base_url: metadata.default_base_url,
        openai_compatible,
        reasoning_model,
        preserves_reasoning_content_in_history: openai_compatible
            && openai_compat::model_requires_reasoning_content_in_history(&resolved_model),
        strips_tuning_params: false,
        // Confirmed against a live daemon: Ollama honours
        // `stream_options.include_usage`.
        supports_stream_usage: true,
        honors_proxy_env: true,
        supports_extra_body_params: openai_compatible,
        preserves_slash_model_ids_on_custom_base_url: true,
    }
}

fn looks_like_local_openai_model(model: &str) -> bool {
    model.contains(':') || model.contains('.')
}

/// Disco Code performs inference exclusively against a local Ollama daemon.
///
/// Provider selection is therefore not a decision: it is fixed at compile time.
/// Model names, ambient API keys, and base-URL environment variables cannot
/// route traffic anywhere else, which is what makes the local-inference
/// guarantee hold even if a user has cloud credentials in their environment.
///
/// Ollama speaks the OpenAI wire format at `<host>/v1`, so the OpenAI-compatible
/// transport carries the traffic; the endpoint it is pointed at is always local.
#[must_use]
pub const fn detect_provider_kind(_model: &str) -> ProviderKind {
    ProviderKind::Ollama
}

/// Local models are addressed as themselves rather than as a Claude analogue.
///
/// claw-code used this to decide whether the system prompt should adopt a
/// Claude identity. Disco Code drives open models, so the generic identity is
/// always correct.
#[must_use]
pub const fn model_family_identity_for_kind(kind: ProviderKind) -> runtime::ModelFamilyIdentity {
    match kind {
        ProviderKind::Ollama => runtime::ModelFamilyIdentity::Generic,
    }
}

#[must_use]
pub fn model_family_identity_for(model: &str) -> runtime::ModelFamilyIdentity {
    model_family_identity_for_kind(detect_provider_kind(model))
}

#[must_use]
pub fn provider_capabilities_for_model(model: &str) -> ProviderCapabilityReport {
    let metadata = local_metadata();

    let (
        wire_protocol,
        streaming_usage,
        prompt_cache,
        custom_parameters,
        reasoning_effort,
        reasoning_content_history,
        fixed_sampling_reasoning_models,
    ) = (
        ProviderWireProtocol::OpenAiChatCompletions,
        // Verified against a live daemon: Ollama honours
        // `stream_options.include_usage` and emits a final chunk carrying real
        // prompt/completion token counts.
        ProviderFeatureSupport::Supported,
        // Ollama keeps a KV cache internally but exposes no control over it, so
        // there is no cache to opt into from the wire protocol.
        ProviderFeatureSupport::Unsupported,
        ProviderFeatureSupport::Supported,
        ProviderFeatureSupport::Supported,
        if openai_compat::model_requires_reasoning_content_in_history(model) {
            ProviderFeatureSupport::Supported
        } else {
            ProviderFeatureSupport::Unsupported
        },
        // Hosted o-series endpoints rejected temperature/top_p on reasoning
        // models. Ollama accepts them (verified live), so nothing is stripped.
        ProviderFeatureSupport::Unsupported,
    );

    ProviderCapabilityReport {
        provider: metadata.provider,
        wire_protocol,
        auth_env: metadata.auth_env,
        base_url_env: metadata.base_url_env,
        default_base_url: metadata.default_base_url,
        tool_calls: ProviderFeatureSupport::Supported,
        streaming: ProviderFeatureSupport::Supported,
        streaming_usage,
        prompt_cache,
        custom_parameters,
        reasoning_effort,
        reasoning_content_history,
        fixed_sampling_reasoning_models,
        web_search: ProviderFeatureSupport::PassthroughAsTool,
        web_fetch: ProviderFeatureSupport::PassthroughAsTool,
    }
}

#[must_use]
pub fn provider_diagnostics_for_request(request: &MessageRequest) -> Vec<ProviderDiagnostic> {
    let capabilities = provider_capabilities_for_model(&request.model);
    let mut diagnostics = Vec::new();

    // The `reasoning_effort_unsupported` and `reasoning_model_fixed_sampling`
    // diagnostics lived here to explain hosted-endpoint validation rules. Both
    // are unreachable against Ollama, which accepts `reasoning_effort` and
    // sampling parameters together, so they are gone rather than left as
    // branches that can never fire.

    if openai_compat::model_requires_reasoning_content_in_history(&request.model) {
        diagnostics.push(ProviderDiagnostic {
            code: "deepseek_v4_reasoning_history",
            severity: ProviderDiagnosticSeverity::Info,
            message: format!(
                "Model `{}` requires assistant thinking history to be echoed as `reasoning_content`.",
                request.model
            ),
            action: "Keep prior assistant Thinking blocks in history; the OpenAI-compatible serializer will emit `reasoning_content` for DeepSeek V4 models.".to_string(),
        });
    }

    if declares_tool(request, "web_search") {
        diagnostics.push(web_passthrough_diagnostic(
            "web_search_passthrough_tool",
            "web_search",
            capabilities.provider,
        ));
    }
    if declares_tool(request, "web_fetch") {
        diagnostics.push(web_passthrough_diagnostic(
            "web_fetch_passthrough_tool",
            "web_fetch",
            capabilities.provider,
        ));
    }

    diagnostics
}

#[must_use]
fn metadata_for_provider_kind(provider: ProviderKind) -> ProviderMetadata {
    match provider {
        ProviderKind::Ollama => local_metadata(),
    }
}

#[must_use]
const fn provider_label(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Ollama => "Ollama",
    }
}

#[must_use]
fn has_openai_tuning_parameters(request: &MessageRequest) -> bool {
    request.temperature.is_some()
        || request.top_p.is_some()
        || request.frequency_penalty.is_some()
        || request.presence_penalty.is_some()
}

#[must_use]
fn declares_tool(request: &MessageRequest, tool_name: &str) -> bool {
    request.tools.as_ref().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool.name.eq_ignore_ascii_case(tool_name))
    })
}

#[must_use]
fn web_passthrough_diagnostic(
    code: &'static str,
    tool_name: &'static str,
    provider: ProviderKind,
) -> ProviderDiagnostic {
    ProviderDiagnostic {
        code,
        severity: ProviderDiagnosticSeverity::Info,
        message: format!(
            "`{tool_name}` is exposed to {} as a normal function tool, not as a provider-native web capability.",
            provider_label(provider)
        ),
        action: format!(
            "Provide a local `{tool_name}` tool implementation or route through a provider adapter that explicitly supports native web tools."
        ),
    }
}

#[must_use]
pub fn max_tokens_for_model(model: &str) -> u32 {
    let canonical = resolve_model_alias(model);
    let heuristic = if canonical.contains("opus") {
        32_000
    } else {
        64_000
    };

    model_token_limit(model).map_or(heuristic, |limit| heuristic.min(limit.max_output_tokens))
}

/// Returns the effective max output tokens for a model, preferring a plugin
/// override when present. Falls back to [`max_tokens_for_model`] when the
/// override is `None`.
#[must_use]
pub fn max_tokens_for_model_with_override(model: &str, plugin_override: Option<u32>) -> u32 {
    plugin_override.unwrap_or_else(|| max_tokens_for_model(model))
}

#[must_use]
pub fn model_token_limit(model: &str) -> Option<ModelTokenLimit> {
    let canonical = resolve_model_alias(model);
    let base_model = canonical.rsplit('/').next().unwrap_or(canonical.as_str());
    match base_model {
        "claude-opus-4-7" | "claude-opus-4-6" => Some(ModelTokenLimit {
            max_output_tokens: 32_000,
            context_window_tokens: 200_000,
        }),
        "claude-sonnet-4-6" | "claude-haiku-4-5-20251213" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 200_000,
        }),
        "grok-3" | "grok-3-mini" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 131_072,
        }),
        // GPT-4.1 family via the OpenAI API.
        "gpt-4.1" | "gpt-4.1-mini" | "gpt-4.1-nano" => Some(ModelTokenLimit {
            max_output_tokens: 32_768,
            context_window_tokens: 1_047_576,
        }),
        // GPT-5.4 family via the OpenAI API.
        "gpt-5.4" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 1_000_000,
        }),
        "gpt-5.4-mini" | "gpt-5.4-nano" => Some(ModelTokenLimit {
            max_output_tokens: 128_000,
            context_window_tokens: 400_000,
        }),
        // Kimi models via DashScope (Moonshot AI)
        // Source: https://platform.moonshot.cn/docs/intro
        "kimi-k2.5" | "kimi-k1.5" => Some(ModelTokenLimit {
            max_output_tokens: 16_384,
            context_window_tokens: 256_000,
        }),
        "qwen-max" => Some(ModelTokenLimit {
            max_output_tokens: 8_192,
            context_window_tokens: 131_072,
        }),
        "qwen-plus" => Some(ModelTokenLimit {
            max_output_tokens: 8_192,
            context_window_tokens: 131_072,
        }),
        _ => None,
    }
}

pub fn preflight_message_request(request: &MessageRequest) -> Result<(), ApiError> {
    let Some(limit) = model_token_limit(&request.model) else {
        return Ok(());
    };

    let estimated_input_tokens = estimate_message_request_input_tokens(request);
    let estimated_total_tokens = estimated_input_tokens.saturating_add(request.max_tokens);
    if estimated_total_tokens > limit.context_window_tokens {
        return Err(ApiError::ContextWindowExceeded {
            model: resolve_model_alias(&request.model),
            estimated_input_tokens,
            requested_output_tokens: request.max_tokens,
            estimated_total_tokens,
            context_window_tokens: limit.context_window_tokens,
        });
    }

    Ok(())
}

fn estimate_message_request_input_tokens(request: &MessageRequest) -> u32 {
    let mut estimate = estimate_serialized_tokens(&request.messages);
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.system));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tools));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tool_choice));
    estimate
}

fn estimate_serialized_tokens<T: Serialize>(value: &T) -> u32 {
    serde_json::to_vec(value)
        .ok()
        .map_or(0, |bytes| (bytes.len() / 4 + 1) as u32)
}

/// Check whether an env var is set to a non-empty value either in the real
/// process environment or in the working-directory `.env` file. Mirrors the
/// credential discovery path used by `read_env_non_empty` so the hint text
/// stays truthful when users rely on `.env` instead of a real export.
fn env_or_dotenv_present(key: &str) -> bool {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => true,
        Ok(_) | Err(std::env::VarError::NotPresent) => {
            dotenv_value(key).is_some_and(|value| !value.is_empty())
        }
        Err(_) => false,
    }
}

/// Parse a `.env` file body into key/value pairs using a minimal `KEY=VALUE`
/// grammar. Lines that are blank, start with `#`, or do not contain `=` are
/// ignored. Surrounding double or single quotes are stripped from the value.
/// An optional leading `export ` prefix on the key is also stripped so files
/// shared with shell `source` workflows still parse cleanly.
pub(crate) fn parse_dotenv(content: &str) -> std::collections::HashMap<String, String> {
    let mut values = std::collections::HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let trimmed_key = raw_key.trim();
        let key = trimmed_key
            .strip_prefix("export ")
            .map_or(trimmed_key, str::trim)
            .to_string();
        if key.is_empty() {
            continue;
        }
        let trimmed_value = raw_value.trim();
        let unquoted = if (trimmed_value.starts_with('"') && trimmed_value.ends_with('"')
            || trimmed_value.starts_with('\'') && trimmed_value.ends_with('\''))
            && trimmed_value.len() >= 2
        {
            &trimmed_value[1..trimmed_value.len() - 1]
        } else {
            trimmed_value
        };
        values.insert(key, unquoted.to_string());
    }
    values
}

/// Load and parse a `.env` file from the given path. Missing files yield
/// `None` instead of an error so callers can use this as a soft fallback.
pub(crate) fn load_dotenv_file(
    path: &std::path::Path,
) -> Option<std::collections::HashMap<String, String>> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(parse_dotenv(&content))
}

/// Look up `key` in a `.env` file located in the current working directory.
/// Returns `None` when the file is missing, the key is absent, or the value
/// is empty.
pub(crate) fn dotenv_value(key: &str) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let values = load_dotenv_file(&cwd.join(".env"))?;
    values.get(key).filter(|value| !value.is_empty()).cloned()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    use serde_json::json;

    use crate::error::ApiError;
    use crate::types::{InputContentBlock, InputMessage, MessageRequest, ToolDefinition};

    use super::{
        detect_provider_kind, load_dotenv_file, max_tokens_for_model,
        max_tokens_for_model_with_override, model_family_identity_for,
        parse_dotenv, preflight_message_request, provider_diagnostics_for_request, ProviderKind,
    };

    /// Serializes every test in this module that mutates process-wide
    /// environment variables so concurrent test threads cannot observe
    /// each other's partially-applied state while probing the foreign
    /// provider credential sniffer.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Snapshot-restore guard for a single environment variable. Captures
    /// the original value on construction, applies the requested override
    /// (set or remove), and restores the original on drop so tests leave
    /// the process env untouched even when they panic mid-assertion.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let original = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn detects_the_local_provider_regardless_of_model_name() {
        assert_eq!(detect_provider_kind("grok"), ProviderKind::Ollama);
        assert_eq!(detect_provider_kind("claude-sonnet-4-6"), ProviderKind::Ollama);
    }

    #[test]
    fn every_model_name_maps_to_a_generic_family_identity() {
        // Inference is local, so no model may claim a frontier vendor identity
        // simply because of how it is named.
        for model in ["claude-opus-4-6", "openai/gpt-4.1-mini", "grok-3"] {
            assert_eq!(
                model_family_identity_for(model),
                runtime::ModelFamilyIdentity::Generic,
                "{model} must resolve to a generic identity"
            );
        }
    }

    #[test]
    fn provider_diagnostics_explain_deepseek_reasoning_and_web_tool_passthrough() {
        let request = MessageRequest {
            model: "openai/deepseek-v4-pro".to_string(),
            max_tokens: 1024,
            messages: vec![InputMessage::user_text("research this")],
            tools: Some(vec![
                ToolDefinition {
                    name: "web_search".to_string(),
                    description: Some("Search the web".to_string()),
                    input_schema: json!({"type": "object"}),
                },
                ToolDefinition {
                    name: "web_fetch".to_string(),
                    description: Some("Fetch a URL".to_string()),
                    input_schema: json!({"type": "object"}),
                },
            ]),
            stream: true,
            ..Default::default()
        };

        let diagnostics = provider_diagnostics_for_request(&request);
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();

        assert!(codes.contains(&"deepseek_v4_reasoning_history"));
        assert!(codes.contains(&"web_search_passthrough_tool"));
        assert!(codes.contains(&"web_fetch_passthrough_tool"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.action.contains("provider adapter")));
    }

    #[test]
    fn provider_diagnostics_explain_openai_compatible_capabilities() {
        let diagnostics = super::provider_diagnostics_for_model("openai/deepseek-v4-pro");

        assert_eq!(diagnostics.provider, ProviderKind::Ollama);
        assert_eq!(diagnostics.auth_env, "OLLAMA_HOST");
        assert!(diagnostics.openai_compatible);
        assert!(diagnostics.preserves_reasoning_content_in_history);
        assert!(diagnostics.supports_extra_body_params);
        assert!(diagnostics.honors_proxy_env);
        assert!(diagnostics.preserves_slash_model_ids_on_custom_base_url);
    }

    #[test]
    fn keeps_existing_max_token_heuristic() {
        assert_eq!(max_tokens_for_model("opus"), 32_000);
        assert_eq!(max_tokens_for_model("grok-3"), 64_000);
        assert_eq!(max_tokens_for_model("gpt-5.4"), 64_000);
    }

    #[test]
    fn plugin_config_max_output_tokens_overrides_model_default() {
        // given
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("api-plugin-max-tokens-{nanos}"));
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        std::fs::create_dir_all(cwd.join(".claw")).expect("project config dir");
        std::fs::create_dir_all(&home).expect("home config dir");
        std::fs::write(
            home.join("settings.json"),
            r#"{
              "plugins": {
                "maxOutputTokens": 12345
              }
            }"#,
        )
        .expect("write plugin settings");

        // when
        let loaded = runtime::ConfigLoader::new(&cwd, &home)
            .load()
            .expect("config should load");
        let plugin_override = loaded.plugins().max_output_tokens();
        let effective = max_tokens_for_model_with_override("claude-opus-4-6", plugin_override);

        // then
        assert_eq!(plugin_override, Some(12345));
        assert_eq!(effective, 12345);
        assert_ne!(effective, max_tokens_for_model("claude-opus-4-6"));

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn max_tokens_for_model_with_override_falls_back_when_plugin_unset() {
        // given
        let plugin_override: Option<u32> = None;

        // when
        let effective = max_tokens_for_model_with_override("claude-opus-4-6", plugin_override);

        // then
        assert_eq!(effective, max_tokens_for_model("claude-opus-4-6"));
        assert_eq!(effective, 32_000);
    }

    #[test]
    fn preflight_blocks_oversized_requests_for_gpt_5_4() {
        let request = MessageRequest {
            model: "gpt-5.4".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(3_900_000),
                }],
            }],
            system: Some("Keep the answer short.".to_string()),
            tools: None,
            tool_choice: None,
            stream: true,
            ..Default::default()
        };

        let error = preflight_message_request(&request)
            .expect_err("oversized gpt-5.4 request should be rejected before the provider call");

        match error {
            ApiError::ContextWindowExceeded {
                model,
                requested_output_tokens,
                context_window_tokens,
                ..
            } => {
                assert_eq!(model, "gpt-5.4");
                assert_eq!(requested_output_tokens, 64_000);
                assert_eq!(context_window_tokens, 1_000_000);
            }
            other => panic!("expected context-window preflight failure, got {other:?}"),
        }
    }

    #[test]
    fn preflight_skips_unknown_models() {
        let request = MessageRequest {
            model: "unknown-model".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(600_000),
                }],
            }],
            system: None,
            tools: None,
            tool_choice: None,
            stream: false,
            ..Default::default()
        };

        preflight_message_request(&request)
            .expect("models without context metadata should skip the guarded preflight");
    }

    #[test]
    fn parse_dotenv_extracts_keys_handles_comments_quotes_and_export_prefix() {
        // given
        let body = "\
# this is a comment

ANTHROPIC_API_KEY=plain-value
XAI_API_KEY=\"quoted-value\"
OPENAI_API_KEY='single-quoted'
export GROK_API_KEY=exported-value
   PADDED_KEY  =  padded-value  
EMPTY_VALUE=
NO_EQUALS_LINE
";

        // when
        let values = parse_dotenv(body);

        // then
        assert_eq!(
            values.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("plain-value")
        );
        assert_eq!(
            values.get("XAI_API_KEY").map(String::as_str),
            Some("quoted-value")
        );
        assert_eq!(
            values.get("OPENAI_API_KEY").map(String::as_str),
            Some("single-quoted")
        );
        assert_eq!(
            values.get("GROK_API_KEY").map(String::as_str),
            Some("exported-value")
        );
        assert_eq!(
            values.get("PADDED_KEY").map(String::as_str),
            Some("padded-value")
        );
        assert_eq!(values.get("EMPTY_VALUE").map(String::as_str), Some(""));
        assert!(!values.contains_key("NO_EQUALS_LINE"));
        assert!(!values.contains_key("# this is a comment"));
    }

    #[test]
    fn load_dotenv_file_reads_keys_from_disk_and_returns_none_when_missing() {
        // given
        let temp_root = std::env::temp_dir().join(format!(
            "api-dotenv-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&temp_root).expect("create temp dir");
        let env_path = temp_root.join(".env");
        std::fs::write(
            &env_path,
            "ANTHROPIC_API_KEY=secret-from-file\n# comment\nXAI_API_KEY=\"xai-secret\"\n",
        )
        .expect("write .env");
        let missing_path = temp_root.join("does-not-exist.env");

        // when
        let loaded = load_dotenv_file(&env_path).expect("file should load");
        let missing = load_dotenv_file(&missing_path);

        // then
        assert_eq!(
            loaded.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("secret-from-file")
        );
        assert_eq!(
            loaded.get("XAI_API_KEY").map(String::as_str),
            Some("xai-secret")
        );
        assert!(missing.is_none());

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    // NOTE: a "OPENAI_BASE_URL without OPENAI_API_KEY" test is omitted
    // because workspace-parallel test binaries can race on process env
    // (env_lock only protects within a single binary). The detection logic
    // is covered: OPENAI_BASE_URL alone routes to OpenAi as a last-resort
    // fallback in detect_provider_kind().
}
