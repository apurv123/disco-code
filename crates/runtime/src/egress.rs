//! Cloud-AI egress policy.
//!
//! Disco Code runs inference exclusively against a local Ollama daemon. The
//! product is *local-inference*, not *offline*: web search, web fetch, code
//! search, MCP servers and package registries are all expected to reach the
//! network freely.
//!
//! This module is the single enforcement point that separates those two cases.
//! It blocks hosted LLM inference endpoints so no prompt or source file can
//! leave the machine toward a cloud model, while leaving ordinary traffic
//! alone.
//!
//! Deleting the cloud providers (checkpoint B2) removed the code that *did*
//! call them. This is the guarantee that a future refactor, a misconfigured
//! `OLLAMA_HOST`, or a third-party MCP server cannot quietly reintroduce it.

use std::fmt;
use std::net::IpAddr;
use std::sync::OnceLock;

use regex::Regex;
use url::Url;

/// Hosts that serve hosted model inference.
///
/// Matching is on the registrable host and its subdomains only — never a
/// substring — so a lookalike such as `api.openai.com.evil.test` does not
/// match.
///
/// Entries containing a `/` are matched as host plus path prefix, for shared
/// hosts where only some paths serve inference.
pub const BLOCKED: &[&str] = &[
    "openai.com",
    "openai.azure.com",
    "anthropic.com",
    "generativelanguage.googleapis.com",
    "aiplatform.googleapis.com",
    "mistral.ai",
    "groq.com",
    "cohere.ai",
    "cohere.com",
    "openrouter.ai",
    "together.xyz",
    "together.ai",
    "deepinfra.com",
    "perplexity.ai",
    "x.ai",
    "cerebras.ai",
    "venice.ai",
    "deepseek.com",
    "moonshot.cn",
    "bigmodel.cn",
    "githubcopilot.com",
    "copilot-proxy.githubusercontent.com",
    "gateway.ai.cloudflare.com",
    "services.ai.azure.com",
    "models.ai.azure.com",
    "inference.ai.azure.com",
    "replicate.com",
    "fireworks.ai",
    "anyscale.com",
    "endpoints.huggingface.cloud",
];

/// Inference hosts that embed a region or account, which a plain suffix list
/// cannot express. AWS Bedrock is `bedrock[-runtime][-fips].<region>.amazonaws.com`,
/// and blocking all of `amazonaws.com` would take S3 and every other AWS
/// service with it.
fn patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![(
            Regex::new(r"^bedrock(-runtime)?(-fips)?\.[a-z0-9-]+\.amazonaws\.com$")
                .expect("bedrock host pattern is a valid regex"),
            "AWS Bedrock",
        )]
    })
}

/// Why a request was refused, carrying enough detail to be actionable in a
/// user-facing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedError {
    pub url: String,
    pub host: String,
    pub reason: String,
}

impl fmt::Display for BlockedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "blocked outbound request to {}: {}", self.url, self.reason)
    }
}

impl std::error::Error for BlockedError {}

/// The outcome of applying the policy to a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Blocked { host: String, reason: String },
}

impl Verdict {
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }

    #[must_use]
    pub fn host(&self) -> Option<&str> {
        match self {
            Self::Allowed => None,
            Self::Blocked { host, .. } => Some(host),
        }
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allowed => None,
            Self::Blocked { reason, .. } => Some(reason),
        }
    }
}

fn why(what: &str) -> String {
    format!(
        "{what} is a hosted model inference endpoint; Disco Code only runs inference \
         through a local Ollama daemon"
    )
}

/// True when `host` is `suffix` itself or a subdomain of it.
///
/// This is deliberately not a substring test: `notopenai.com` and
/// `api.openai.com.evil.test` must both pass.
fn under(host: &str, suffix: &str) -> bool {
    host == suffix || host.strip_suffix(suffix).is_some_and(|rest| rest.ends_with('.'))
}

/// Loopback, unspecified and private ranges — where a user's own Ollama
/// daemon or MCP server lives. Always allowed; this is how inference actually
/// happens.
#[must_use]
pub fn is_local(host: &str) -> bool {
    let bare = host.trim().to_ascii_lowercase();
    let bare = bare.trim_end_matches('.');
    let bare = bare.strip_prefix('[').unwrap_or(bare);
    let bare = bare.strip_suffix(']').unwrap_or(bare);

    if bare == "localhost" {
        return true;
    }
    if bare.ends_with(".local") || bare.ends_with(".localhost") {
        return true;
    }

    // Prefer real address parsing over string prefixes: it gets 172.16/12
    // right, and rejects 172.15 and 172.32, which a `starts_with` cannot.
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            v4.is_loopback() || v4.is_private() || v4.is_unspecified() || v4.is_link_local()
        }
        Ok(IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unspecified(),
        Err(_) => false,
    }
}

/// Apply the policy to a URL.
///
/// Malformed input is refused rather than passed through: a URL we cannot
/// parse is a URL whose destination we cannot vouch for.
#[must_use]
pub fn classify(url: &str) -> Verdict {
    let Ok(parsed) = Url::parse(url) else {
        return Verdict::Blocked {
            host: String::new(),
            reason: "malformed URL".to_string(),
        };
    };

    let Some(raw_host) = parsed.host_str() else {
        return Verdict::Blocked {
            host: String::new(),
            reason: "URL has no host".to_string(),
        };
    };

    let host = raw_host
        .to_ascii_lowercase()
        .trim_end_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    // A local daemon is always fine, and is how inference actually happens.
    if is_local(&host) {
        return Verdict::Allowed;
    }

    for entry in BLOCKED {
        if let Some(cut) = entry.find('/') {
            let (entry_host, prefix) = entry.split_at(cut);
            if under(&host, entry_host) && parsed.path().starts_with(prefix) {
                return Verdict::Blocked {
                    host,
                    reason: why(entry),
                };
            }
        } else if under(&host, entry) {
            return Verdict::Blocked {
                host,
                reason: why(entry),
            };
        }
    }

    for (re, name) in patterns() {
        if re.is_match(&host) {
            return Verdict::Blocked {
                host,
                reason: why(name),
            };
        }
    }

    Verdict::Allowed
}

/// Convenience predicate over [`classify`].
#[must_use]
pub fn allowed(url: &str) -> bool {
    classify(url).is_allowed()
}

/// Returns an error when the URL targets hosted inference. Call before any
/// outbound request.
pub fn guard(url: &str) -> Result<(), BlockedError> {
    match classify(url) {
        Verdict::Allowed => Ok(()),
        Verdict::Blocked { host, reason } => Err(BlockedError {
            url: url.to_string(),
            host,
            reason,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{allowed, classify, guard, is_local, Verdict, BLOCKED};

    #[test]
    fn blocks_the_major_cloud_model_apis() {
        let blocked = [
            "https://api.openai.com/v1/chat/completions",
            "https://api.anthropic.com/v1/messages",
            "https://generativelanguage.googleapis.com/v1beta/models/gemini:generateContent",
            "https://api.mistral.ai/v1/chat/completions",
            "https://api.groq.com/openai/v1/chat/completions",
            "https://openrouter.ai/api/v1/chat/completions",
            "https://api.together.xyz/v1/chat/completions",
            "https://api.deepinfra.com/v1/openai/chat/completions",
            "https://api.perplexity.ai/chat/completions",
            "https://api.x.ai/v1/chat/completions",
            "https://api.cohere.ai/v1/chat",
            "https://api.deepseek.com/chat/completions",
            "https://api.githubcopilot.com/chat/completions",
            "https://gateway.ai.cloudflare.com/v1/acct/gw/openai",
            "https://my-resource.openai.azure.com/openai/deployments/x/chat/completions",
        ];
        for url in blocked {
            assert!(!allowed(url), "should have blocked {url}");
        }
    }

    #[test]
    fn blocks_regionalized_inference_hosts_that_a_suffix_list_cannot_express() {
        let blocked = [
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/x/invoke",
            "https://bedrock-runtime.eu-west-2.amazonaws.com/model/x/invoke",
            "https://bedrock.ap-southeast-1.amazonaws.com/foundation-models",
            "https://bedrock-runtime-fips.us-gov-west-1.amazonaws.com/model/x/invoke",
            "https://my-project.eastus.services.ai.azure.com/models/chat",
        ];
        for url in blocked {
            assert!(!allowed(url), "should have blocked {url}");
        }
    }

    #[test]
    fn does_not_block_other_aws_services_in_the_same_domain() {
        assert!(allowed("https://s3.us-east-1.amazonaws.com/bucket/key"));
        assert!(allowed("https://sqs.us-east-1.amazonaws.com/123/queue"));
    }

    #[test]
    fn blocks_subdomains_of_a_blocked_host() {
        assert!(!allowed("https://api.openai.com/v1/models"));
        assert!(!allowed("https://eu.api.anthropic.com/v1/messages"));
    }

    #[test]
    fn explains_why_a_request_was_blocked() {
        let verdict = classify("https://api.openai.com/v1/chat/completions");
        assert!(!verdict.is_allowed());
        assert_eq!(verdict.host(), Some("api.openai.com"));
        assert!(verdict
            .reason()
            .expect("blocked verdict carries a reason")
            .contains("local Ollama daemon"));
    }

    #[test]
    fn is_case_and_trailing_dot_insensitive() {
        assert!(!allowed("https://API.OpenAI.COM/v1/chat"));
        assert!(!allowed("https://api.openai.com./v1/chat"));
    }

    #[test]
    fn does_not_match_lookalike_hosts_by_substring() {
        assert!(allowed("https://api.openai.com.evil.test/v1/chat"));
        assert!(allowed("https://notopenai.com/docs"));
        assert!(allowed("https://myopenai.com/"));
        assert!(allowed("https://openai.com.attacker.example/v1"));
    }

    #[test]
    fn rejects_malformed_urls_rather_than_passing_them_through() {
        assert!(!allowed("not a url"));
        assert!(!allowed(""));
        assert!(!allowed("https://"));
    }

    #[test]
    fn allows_web_search_fetch_and_documentation() {
        let ok = [
            "https://mcp.exa.ai/mcp",
            "https://duckduckgo.com/?q=rust",
            "https://developer.mozilla.org/en-US/docs/Web/API/fetch",
            "https://docs.rs/serde/latest/serde/",
            "https://stackoverflow.com/questions/1",
        ];
        for url in ok {
            assert!(allowed(url), "should have allowed {url}");
        }
    }

    #[test]
    fn allows_code_search_and_source_hosts() {
        assert!(allowed("https://api.github.com/search/code?q=foo"));
        assert!(allowed("https://raw.githubusercontent.com/o/r/main/f.rs"));
        assert!(allowed("https://grep.app/api/search?q=foo"));
    }

    #[test]
    fn allows_package_registries() {
        assert!(allowed("https://registry.npmjs.org/react"));
        assert!(allowed("https://crates.io/api/v1/crates/serde"));
        assert!(allowed("https://static.crates.io/crates/serde/serde-1.0.0.crate"));
        assert!(allowed("https://pypi.org/simple/requests/"));
    }

    #[test]
    fn allows_playwright_mcp_and_browser_automation_downloads() {
        assert!(allowed(
            "https://playwright.azureedge.net/builds/chromium/1/chromium-win64.zip"
        ));
        assert!(allowed(
            "https://cdn.playwright.dev/dbazure/download/playwright/builds/x"
        ));
    }

    #[test]
    fn allows_non_inference_google_and_aws_services() {
        assert!(allowed("https://www.googleapis.com/customsearch/v1?q=x"));
        assert!(allowed("https://s3.us-east-1.amazonaws.com/bucket/key"));
    }

    #[test]
    fn treats_loopback_and_private_ranges_as_local() {
        for host in [
            "localhost",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
            "0.0.0.0",
            "192.168.1.10",
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.255",
            "box.local",
        ] {
            assert!(is_local(host), "{host} should be local");
        }
    }

    #[test]
    fn does_not_treat_public_addresses_as_local() {
        for host in [
            "8.8.8.8",
            "172.15.0.1",
            "172.32.0.1",
            "example.com",
            "11.0.0.1",
        ] {
            assert!(!is_local(host), "{host} should not be local");
        }
    }

    #[test]
    fn allows_a_local_ollama_daemon_on_any_form_of_loopback() {
        assert!(allowed("http://127.0.0.1:11434/api/chat"));
        assert!(allowed("http://localhost:11434/v1/chat/completions"));
        assert!(allowed("http://[::1]:11434/api/tags"));
        assert!(allowed("http://192.168.1.50:11434/api/chat"));
    }

    #[test]
    fn guard_passes_allowed_urls_through_silently() {
        assert!(guard("https://registry.npmjs.org/react").is_ok());
    }

    #[test]
    fn guard_reports_the_host_for_blocked_urls() {
        let err = guard("https://api.openai.com/v1/chat/completions")
            .expect_err("hosted inference must be refused");
        assert_eq!(err.host, "api.openai.com");
        assert!(err.to_string().contains("api.openai.com"));
    }

    #[test]
    fn blocked_list_has_no_duplicate_entries() {
        let mut seen = std::collections::HashSet::new();
        for entry in BLOCKED {
            assert!(seen.insert(*entry), "duplicate entry in BLOCKED: {entry}");
        }
    }

    #[test]
    fn every_blocked_entry_is_actually_blocked_by_the_classifier() {
        for entry in BLOCKED {
            let url = format!("https://{entry}/");
            assert!(
                !allowed(&url),
                "BLOCKED lists {entry} but the classifier allows it"
            );
        }
    }

    #[test]
    fn a_blocked_host_cannot_be_smuggled_in_as_a_userinfo_field() {
        // `https://api.openai.com@evil.test/` has host `evil.test`, not
        // OpenAI - the verdict must describe the host actually dialled.
        let verdict = classify("https://api.openai.com@evil.test/v1/chat");
        assert_eq!(verdict, Verdict::Allowed);
    }
}
