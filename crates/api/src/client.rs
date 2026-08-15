use crate::error::ApiError;
use crate::providers::openai_compat::{self, OpenAiCompatClient};
use crate::providers::ProviderKind;
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

/// The client used for every request.
///
/// This was an enum over hosted backends chosen by model name and ambient
/// credentials. Inference is now local-only, so it wraps the single transport
/// that speaks to the Ollama daemon. The wrapper is retained because it is the
/// type threaded through the runtime and the CLI, and because it remains the
/// one place where the choice of transport is made.
#[derive(Debug, Clone)]
pub struct ProviderClient {
    inner: OpenAiCompatClient,
}

impl ProviderClient {
    /// Builds the client for a model.
    ///
    /// The model name is accepted for symmetry with the call sites but cannot
    /// influence where the request goes: that is the guarantee this type exists
    /// to enforce.
    pub fn from_model(_model: &str) -> Result<Self, ApiError> {
        Ok(Self {
            inner: OpenAiCompatClient::ollama()
                .expect("the local client needs no credentials, so it always builds"),
        })
    }

    #[must_use]
    pub const fn provider_kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    /// The endpoint this client will actually send to.
    ///
    /// Exposed so tests can assert the no-egress guarantee directly rather than
    /// inferring it from the absence of a credential.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        self.inner.send_message(request).await
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        self.inner
            .stream_message(request)
            .await
            .map(MessageStream::OpenAiCompat)
    }
}

#[derive(Debug)]
pub enum MessageStream {
    OpenAiCompat(openai_compat::MessageStream),
}

impl MessageStream {
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::OpenAiCompat(stream) => stream.request_id(),
        }
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        match self {
            Self::OpenAiCompat(stream) => stream.next_event().await,
        }
    }
}

/// The base URL every request is sent to.
#[must_use]
pub fn read_base_url() -> String {
    crate::ollama::base()
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::ProviderClient;
    use crate::providers::{detect_provider_kind, ProviderKind};

    /// Serializes every test in this module that mutates process-wide
    /// environment variables so concurrent test threads cannot observe
    /// each other's partially-applied state.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn provider_detection_is_fixed_to_the_local_ollama_surface() {
        // Model naming no longer selects a provider: everything runs locally.
        assert_eq!(detect_provider_kind("grok-3"), ProviderKind::Ollama);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::Ollama
        );
        assert_eq!(detect_provider_kind("qwen3.5:9b"), ProviderKind::Ollama);
    }

    /// Snapshot-restore guard for a single environment variable. Mirrors
    /// the pattern used in `providers/mod.rs` tests: captures the original
    /// value on construction, applies the override, and restores on drop so
    /// tests leave the process env untouched even when they panic.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
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
    fn hosted_credentials_in_the_environment_cannot_redirect_inference() {
        // A user may legitimately have cloud keys set for other tools. They must
        // never cause Disco Code to send code off the machine.
        let _lock = env_lock();
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("test-dashscope-key"));
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("test-openai-key"));
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", Some("test-anthropic-key"));

        let client = ProviderClient::from_model("qwen-plus").expect("client should build");
        let base = client.base_url().to_lowercase();
        assert!(
            !base.contains("dashscope.aliyuncs.com") && !base.contains("api.openai.com"),
            "hosted keys must not redirect inference, got: {base}"
        );
        assert!(base.contains("11434"), "expected the Ollama port, got: {base}");
    }

    #[test]
    fn openai_base_url_override_cannot_repoint_inference() {
        let _lock = env_lock();
        // Even an explicit remote override is ignored; the route is compiled in.
        let _base_url = EnvVarGuard::set("OPENAI_BASE_URL", Some("https://api.openai.com/v1"));
        let _openai_key = EnvVarGuard::set("OPENAI_API_KEY", Some("test-openai-key"));

        let client = ProviderClient::from_model("qwen2.5-coder:7b").expect("should resolve");
        assert!(
            !client.base_url().contains("api.openai.com"),
            "OPENAI_BASE_URL must not repoint inference, got: {}",
            client.base_url()
        );
    }
}
