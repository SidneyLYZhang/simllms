use std::sync::OnceLock;
use std::time::Duration;

use crate::catalog::Catalog;
use crate::capabilities::{CapabilityOverrides, ResolvedCapabilities};
use crate::dialect;
use crate::error::{ApiError, Error, TransportError};
use crate::gate::{self, ChatRequest};
use crate::message::Message;
use crate::options::Options;
use crate::provider::Provider;
use crate::reply::Reply;

/// How the Client re-sends a request that failed for a possibly-transient
/// reason. Attempts count total sends, not retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy { enabled: true, max_attempts: 3 }
    }
}

impl RetryPolicy {
    pub fn disabled() -> Self {
        RetryPolicy { enabled: false, max_attempts: 1 }
    }
}

const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(300);

fn default_catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(Catalog::placeholder)
}

/// One provider, one Model Id, one credential. Construct it, then Chat.
pub struct Client {
    provider: Provider,
    model: String,
    api_key: String,
    base_url: Option<String>,
    options: Options,
    overrides: CapabilityOverrides,
    retry: RetryPolicy,
    timeout: Duration,
    http: reqwest::Client,
}

impl Client {
    pub fn new(provider: Provider, model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Client {
            provider,
            model: model.into(),
            api_key: api_key.into(),
            base_url: None,
            options: Options::default(),
            overrides: CapabilityOverrides::default(),
            retry: RetryPolicy::default(),
            timeout: DEFAULT_CHAT_TIMEOUT,
            http: reqwest::Client::builder()
                .build()
                .expect("reqwest client with rustls should always build"),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn with_options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }

    pub fn with_capability_overrides(mut self, overrides: CapabilityOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Renders the exact JSON body a Chat would send, without sending it and
    /// without any credential material — the api key travels only in the
    /// Authorization header, which never appears here.
    pub fn debug_request(&self, messages: &[Message], options: &Options) -> String {
        let body = dialect::chat_body(self.provider, &self.model, messages, options);
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
    }

    pub async fn chat(&self, messages: Vec<Message>) -> Result<Reply, Error> {
        self.chat_with(messages, Options::default()).await
    }

    /// Chat with per-request Options layered over the client's own
    /// (per-request wins, see `Options::layered`).
    pub async fn chat_with(&self, messages: Vec<Message>, options: Options) -> Result<Reply, Error> {
        let merged = Options::layered(self.options.clone(), options);
        let request = ChatRequest { messages, options: merged, stream: false };

        let resolved = self.resolve();
        gate::gate(&resolved, &request, self.provider, &self.model)?;

        let body = dialect::chat_body(self.provider, &self.model, &request.messages, &request.options);
        self.send(&body).await
    }

    fn resolve(&self) -> ResolvedCapabilities {
        match default_catalog().resolve(self.provider, &self.model) {
            ResolvedCapabilities::Known(base) if self.overrides != CapabilityOverrides::default() => {
                ResolvedCapabilities::Known(self.overrides.apply_to(base))
            }
            resolved => resolved,
        }
    }

    fn url(&self) -> String {
        let base = self
            .base_url
            .clone()
            .unwrap_or_else(|| self.provider.default_base_url().to_string());
        format!("{}/chat/completions", base.trim_end_matches('/'))
    }

    async fn send(&self, body: &serde_json::Value) -> Result<Reply, Error> {
        #[cfg(feature = "tracing")]
        let span = tracing::info_span!(
            "simllms.chat",
            provider = %self.provider,
            model = %self.model,
        );
        #[cfg(feature = "tracing")]
        let _guard = span.enter();

        let url = self.url();
        let mut attempt = 1u32;
        loop {
            match self.send_once(&url, body).await {
                Ok(reply) => {
                    #[cfg(feature = "tracing")]
                    tracing::info!(
                        parent: &span,
                        prompt_tokens = reply.usage.as_ref().and_then(|u| u.prompt_tokens),
                        "chat completed"
                    );
                    return Ok(reply);
                }
                Err(outcome) => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!(parent: &span, attempt, "chat attempt failed");
                    let (error, retry_after) = outcome;
                    if !self.retry.enabled || attempt >= self.retry.max_attempts {
                        return Err(error);
                    }
                    let delay = retry_after.unwrap_or_else(|| backoff_delay(attempt));
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    /// One send. `Err` carries the mapped error plus a `Retry-After` delay
    /// when the provider sent one on a retryable status.
    async fn send_once(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<Reply, (Error, Option<Duration>)> {
        let response = self
            .http
            .post(url)
            .timeout(self.timeout)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                (
                    Error::Transport(TransportError { message: e.to_string() }),
                    None,
                )
            })?;

        let status = response.status().as_u16();
        // Headers are gone once the body is read, so Retry-After is captured
        // first. Only seconds-form is honoured; date-form falls back to the
        // backoff schedule.
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs);
        let raw_body = response
            .text()
            .await
            .map_err(|e| (Error::Transport(TransportError { message: e.to_string() }), None))?;

        if !(200..300).contains(&status) {
            let retryable = status == 429 || status >= 500;
            let error = Error::Api(ApiError {
                status,
                provider_message: extract_provider_message(&raw_body),
                raw_body,
            });
            return Err((error, if retryable { retry_after } else { None }));
        }

        let parsed: serde_json::Value = serde_json::from_str(&raw_body).map_err(|e| {
            (
                Error::Api(ApiError {
                    status,
                    provider_message: format!("malformed success response: {e}"),
                    raw_body: raw_body.clone(),
                }),
                None,
            )
        })?;

        dialect::parse_reply(&parsed).map_err(|e| (e, None))
    }
}

fn extract_provider_message(raw_body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw_body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1u64 << (attempt - 1).min(6)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Modality;
    use crate::message::{ContentBlock, ImageSource};
    use mockito::{Matcher, Server};
    use crate::message::Role;

    fn success_body() -> String {
        serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": "hi there" } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })
        .to_string()
    }

    async fn mock_success(server: &mut Server) -> mockito::Mock {
        server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_body(success_body())
            .create_async()
            .await
    }

    #[tokio::test]
    async fn sends_exact_request_shape() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("authorization", "Bearer sk-test")
            .match_header("content-type", Matcher::Regex("application/json".into()))
            .match_body(Matcher::PartialJson(serde_json::json!({
                "model": "deepseek-v4-pro",
                "messages": [{ "role": "user", "content": "hello" }],
                "max_tokens": 64
            })))
            .with_status(200)
            .with_body(success_body())
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url())
            .with_options(Options { max_tokens: Some(64), ..Default::default() });
        let reply = client.chat(vec![Message::text("hello")]).await.expect("chat must succeed");

        mock.assert_async().await;
        assert_eq!(reply.content, "hi there");
        assert_eq!(reply.usage.map(|u| u.total_tokens), Some(Some(15)));
    }

    #[tokio::test]
    async fn provider_error_maps_to_api_with_raw_body_intact() {
        let mut server = Server::new_async().await;
        let raw = r#"{"error":{"message":"Authentication Fails (no such user)","code":"invalid_request_error"}}"#;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(401)
            .with_body(raw)
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-wrong")
            .with_base_url(server.url())
            .with_retry(RetryPolicy::disabled());
        let err = client.chat(vec![Message::text("hello")]).await.expect_err("401 must fail");
        assert!(!err.is_retryable());
        mock.assert_async().await;
        match err {
            Error::Api(e) => {
                assert_eq!(e.status, 401);
                assert_eq!(e.raw_body, raw);
                assert_eq!(e.provider_message, "Authentication Fails (no such user)");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsendable_request_maps_to_transport() {
        // A base_url that is not a URL makes reqwest fail before any bytes
        // hit the wire — deterministic everywhere, no network conditions.
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url("not a url at all")
            .with_retry(RetryPolicy::disabled());
        let err = client.chat(vec![Message::text("hello")]).await.expect_err("must fail");
        assert!(matches!(err, Error::Transport(_)), "got {err:?}");
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn retries_5xx_then_succeeds() {
        let mut server = Server::new_async().await;
        let failing = server
            .mock("POST", "/chat/completions")
            .with_status(503)
            .with_body(r#"{"error":{"message":"overloaded"}}"#)
            .expect(2)
            .create_async()
            .await;
        let ok = mock_success(&mut server).await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url());
        let reply = client.chat(vec![Message::text("hello")]).await.expect("retry must recover");

        failing.assert_async().await;
        ok.assert_async().await;
        assert_eq!(reply.content, "hi there");
    }

    #[tokio::test]
    async fn retry_after_header_takes_precedence_over_backoff() {
        let mut server = Server::new_async().await;
        let failing = server
            .mock("POST", "/chat/completions")
            .match_header("authorization", "Bearer sk-test")
            .with_status(429)
            .with_header("retry-after", "0")
            .with_body(r#"{"error":{"message":"rate limited"}}"#)
            .expect(2)
            .create_async()
            .await;
        let ok = mock_success(&mut server).await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url());
        let start = std::time::Instant::now();
        client.chat(vec![Message::text("hello")]).await.expect("must recover");
        let elapsed = start.elapsed();

        failing.assert_async().await;
        ok.assert_async().await;
        // Default backoff after attempt 1 is 500ms; Retry-After: 0 must beat it.
        assert!(elapsed < Duration::from_millis(450), "took {elapsed:?}");
    }

    #[tokio::test]
    async fn retry_can_be_disabled() {
        let mut server = Server::new_async().await;
        let failing = server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_body(r#"{"error":{"message":"rate limited"}}"#)
            .expect(1)
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url())
            .with_retry(RetryPolicy::disabled());
        let err = client.chat(vec![Message::text("hello")]).await.expect_err("429 must fail");

        failing.assert_async().await;
        assert!(matches!(err, Error::Api(e) if e.status == 429));
    }

    #[tokio::test]
    async fn timeout_maps_to_transport_error() {
        // A non-routable address: the request cannot complete, so the only
        // path out is the per-request timeout.
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url("http://10.255.255.1:1")
            .with_timeout(Duration::from_millis(100))
            .with_retry(RetryPolicy::disabled());
        let err = client.chat(vec![Message::text("hello")]).await.expect_err("must time out");
        assert!(matches!(err, Error::Transport(_)), "got {err:?}");
    }

    #[test]
    fn default_chat_timeout_is_300s() {
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test");
        assert_eq!(client.timeout, Duration::from_secs(300));
    }

    #[test]
    fn debug_request_has_no_credential_material() {
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-secret-do-not-leak");
        let rendered = client.debug_request(
            &[Message::text("hello")],
            &Options { max_tokens: Some(32), ..Default::default() },
        );
        assert!(rendered.contains("deepseek-v4-pro"));
        assert!(rendered.contains("hello"));
        assert!(!rendered.contains("sk-secret-do-not-leak"));
    }

    #[tokio::test]
    async fn gate_refusal_never_touches_the_network() {
        // No mockito server exists in this test at all: if the client tries
        // to connect it will fail with Transport, not Capability.
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test");
        let messages = vec![Message::blocks(
            Role::User,
            vec![
                ContentBlock::Text("what is this?".into()),
                ContentBlock::Image(ImageSource::Url("https://example.com/cat.png".into())),
            ],
        )];
        let err = client.chat(messages).await.expect_err("image to text-only model must fail locally");
        match err {
            Error::Capability(e) => {
                assert_eq!(e.model, "deepseek-v4-pro");
                assert_eq!(e.missing, crate::error::MissingCapability::ImageInput);
            }
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn default_catalog_knows_vision_models() {
        let caps = default_catalog().resolve(Provider::Moonshot, "kimi-k3");
        match caps {
            ResolvedCapabilities::Known(c) => assert!(c.input_modalities.contains(&Modality::Image)),
            other => panic!("kimi-k3 must be known, got {other:?}"),
        }
    }
}
