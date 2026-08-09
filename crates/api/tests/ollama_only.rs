//! Proves inference cannot leave the local machine.
//!
//! Disco Code's core promise is that your code is never sent to a hosted model.
//! That promise is only as good as the routing layer, so these tests assert the
//! guarantee directly rather than trusting configuration: no model name and no
//! ambient cloud credential may produce a non-local route.

use api::{ollama_base, ollama_host, ProviderKind};

/// Model names that previously selected a hosted provider by name alone.
const HOSTED_LOOKING: &[&str] = &[
    "claude-opus-4-6",
    "claude-3-5-sonnet-20241022",
    "gpt-4.1-mini",
    "gpt-4o",
    "grok-2-latest",
    "qwen-max",
    "o1-preview",
];

#[test]
fn every_model_name_routes_to_the_local_openai_compatible_surface() {
    for model in HOSTED_LOOKING {
        assert_eq!(
            api::detect_provider_kind(model),
            ProviderKind::Ollama,
            "{model} must route to the local Ollama surface, not a hosted provider"
        );
    }
}

#[test]
fn unknown_and_empty_model_names_still_route_locally() {
    for model in ["", "some-model-nobody-has-heard-of", "llama3.2:latest"] {
        assert_eq!(api::detect_provider_kind(model), ProviderKind::Ollama);
    }
}

#[test]
fn base_url_is_loopback_by_default() {
    // Guards the default; a developer machine with OLLAMA_HOST set to a LAN
    // address is legitimate, so only the unset case is asserted here.
    if std::env::var_os("OLLAMA_HOST").is_none() {
        let base = ollama_base();
        assert!(
            base.starts_with("http://127.0.0.1:11434"),
            "default inference base must be loopback, got {base}"
        );
        assert!(base.ends_with("/v1"));
        assert_eq!(ollama_host(), "http://127.0.0.1:11434");
    }
}

#[test]
fn no_hosted_endpoint_appears_in_the_resolved_base_url() {
    let base = ollama_base().to_lowercase();
    for host in [
        "api.openai.com",
        "api.anthropic.com",
        "api.x.ai",
        "dashscope.aliyuncs.com",
        "generativelanguage.googleapis.com",
        "bedrock-runtime",
        "openrouter.ai",
    ] {
        assert!(
            !base.contains(host),
            "resolved inference base {base} must never reference hosted provider {host}"
        );
    }
}

/// The routing tests above prove no *model name* selects a hosted provider.
/// These prove the stronger claim: even when the base URL is forced to a cloud
/// endpoint - which `OLLAMA_HOST` makes a one-variable change - the request is
/// refused before a socket is opened.
mod egress_enforcement {
    use api::{ApiError, InputMessage, MessageRequest, OpenAiCompatClient, OLLAMA_CONFIG};

    fn request() -> MessageRequest {
        MessageRequest {
            model: "qwen3.5:9b".to_string(),
            max_tokens: 16,
            messages: vec![InputMessage::user_text("secret source code")],
            ..MessageRequest::default()
        }
    }

    async fn assert_blocked(base_url: &str) {
        let client = OpenAiCompatClient::new("unused", OLLAMA_CONFIG).with_base_url(base_url);
        let error = client
            .send_message(&request())
            .await
            .expect_err("inference against {base_url} must be refused");

        match error {
            ApiError::EgressBlocked { host, .. } => {
                assert!(!host.is_empty(), "blocked error must name the host");
            }
            other => panic!("expected EgressBlocked for {base_url}, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn inference_against_a_cloud_endpoint_is_refused() {
        for base in [
            "https://api.openai.com/v1",
            "https://api.anthropic.com/v1",
            "https://api.x.ai/v1",
            "https://openrouter.ai/api/v1",
            "https://bedrock-runtime.us-east-1.amazonaws.com",
            "https://my-resource.openai.azure.com/openai/deployments/x",
        ] {
            assert_blocked(base).await;
        }
    }

    #[tokio::test]
    async fn a_blocked_request_is_not_retried() {
        // Retrying a forbidden call would attempt the same egress N more
        // times, so the error must surface directly rather than wrapped in
        // `RetriesExhausted`.
        let client =
            OpenAiCompatClient::new("unused", OLLAMA_CONFIG).with_base_url("https://api.openai.com/v1");
        let error = client
            .send_message(&request())
            .await
            .expect_err("must be refused");
        assert!(
            matches!(error, ApiError::EgressBlocked { .. }),
            "expected a direct EgressBlocked, got: {error:?}"
        );
    }

    #[tokio::test]
    async fn the_refusal_explains_itself() {
        let client =
            OpenAiCompatClient::new("unused", OLLAMA_CONFIG).with_base_url("https://api.openai.com/v1");
        let message = client
            .send_message(&request())
            .await
            .expect_err("must be refused")
            .to_string();
        assert!(message.contains("api.openai.com"), "got: {message}");
        assert!(message.contains("local Ollama daemon"), "got: {message}");
    }
}
