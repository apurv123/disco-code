mod client;
mod error;
mod http_client;
mod ollama;
mod prompt_cache;
mod providers;
mod sse;
mod types;

pub use ollama::{
    base as ollama_base, choose as ollama_choose, host as ollama_host, list as ollama_list,
    pick as ollama_pick, resolve as ollama_resolve, tags as ollama_tags,
    tags_blocking as ollama_tags_blocking, Caps as OllamaCaps,
    Model as OllamaModel, AUTO as OLLAMA_AUTO, DEFAULT_HOST as OLLAMA_DEFAULT_HOST,
};

pub use client::{read_base_url, MessageStream, ProviderClient};
pub use error::ApiError;
pub use http_client::{
    build_http_client, build_http_client_or_default, build_http_client_with,
    build_http_client_with_opts, ProxyConfig, TimeoutConfig,
};
pub use prompt_cache::{
    CacheBreakEvent, PromptCache, PromptCacheConfig, PromptCachePaths, PromptCacheRecord,
    PromptCacheStats,
};
pub use providers::openai_compat::{
    build_chat_completion_request, check_request_body_size, estimate_request_body_size,
    flatten_tool_result_content, is_reasoning_model, model_rejects_is_error_field,
    model_requires_reasoning_content_in_history, translate_message, OpenAiCompatClient,
    OpenAiCompatConfig, OLLAMA_CONFIG,
};
pub use providers::{
    detect_provider_kind, max_tokens_for_model, max_tokens_for_model_with_override,
    model_family_identity_for, model_family_identity_for_kind, provider_diagnostics_for_model,
    resolve_model_alias, ProviderDiagnostics, ProviderKind,
};
pub use sse::{parse_frame, SseParser};
pub use types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
    InputContentBlock, InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest,
    MessageResponse, MessageStartEvent, MessageStopEvent, OutputContentBlock, StreamEvent,
    ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};

pub use telemetry::{
    AnalyticsEvent, ClientIdentity, JsonlTelemetrySink, MemoryTelemetrySink, SessionTraceRecord,
    SessionTracer, TelemetryEvent, TelemetrySink,
};
