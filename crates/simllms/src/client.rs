use std::sync::OnceLock;
use std::time::Duration;

use crate::catalog::Catalog;
use crate::capabilities::{CapabilityOverrides, ResolvedCapabilities};
use crate::dialect;
use crate::error::{ApiError, CapabilityError, ConfigError, Error, MissingCapability, TransportError};
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
const DEFAULT_FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

fn default_catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(Catalog::builtin)
}

/// How the api key travels. `Bearer` is the OpenAI-compatible default;
/// `Header` sends the credential under a provider-specific header (MiMo
/// uses `api-key:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    Bearer,
    Header { name: String, value: String },
}

fn provider_default_auth(provider: Provider, api_key: &str) -> Auth {
    match provider {
        Provider::MiMo => Auth::Header {
            name: "api-key".into(),
            value: api_key.to_string(),
        },
        _ => Auth::Bearer,
    }
}

/// One provider, one Model Id, one credential. Construct it, then Chat.
pub struct Client {
    provider: Provider,
    model: String,
    api_key: String,
    base_url: Option<String>,
    options: Options,
    overrides: CapabilityOverrides,
    auth: Auth,
    retry: RetryPolicy,
    timeout: Duration,
    stream_first_token_timeout: Duration,
    stream_idle_timeout: Duration,
    http: reqwest::Client,
}

impl Client {
    pub fn new(provider: Provider, model: impl Into<String>, api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Client {
            provider,
            model: model.into(),
            api_key,
            base_url: None,
            options: Options::default(),
            overrides: CapabilityOverrides::default(),
            auth: Auth::Bearer,
            retry: RetryPolicy::default(),
            timeout: DEFAULT_CHAT_TIMEOUT,
            stream_first_token_timeout: DEFAULT_FIRST_TOKEN_TIMEOUT,
            stream_idle_timeout: DEFAULT_IDLE_TIMEOUT,
            http: reqwest::Client::builder()
                .build()
                .expect("reqwest client with rustls should always build"),
        }
        .with_default_auth()
    }

    fn with_default_auth(mut self) -> Self {
        self.auth = provider_default_auth(self.provider, &self.api_key);
        self
    }

    /// Builds the Client with the provider's api key read from its
    /// conventional environment variable (see `Provider::env_key_name`).
    /// Fails with a `Config` error when the variable is unset or empty.
    pub fn from_env(provider: Provider, model: impl Into<String>) -> Result<Self, Error> {
        let name = provider.env_key_name();
        match std::env::var(name) {
            Ok(key) if !key.is_empty() => Ok(Self::new(provider, model, key)),
            _ => Err(Error::Config(ConfigError {
                message: format!(
                    "{name} is not set; it is the conventional environment variable for {provider}"
                ),
            })),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Overrides how the api key travels (default: Bearer). For `Auth::Header`
    /// the value is sent verbatim, so embed the key yourself when overriding.
    pub fn with_auth(mut self, auth: Auth) -> Self {
        self.auth = auth;
        self
    }

    /// Selects a DashScope region (or workspace domain) instead of the
    /// shared Beijing host. See `DashScopeRegion` for the key-region
    /// binding constraint — a raw `with_base_url` override bypasses this
    /// deliberately and is not checked.
    pub fn with_dashscope_region(mut self, region: crate::provider::DashScopeRegion) -> Self {
        debug_assert_eq!(self.provider, Provider::DashScope);
        self.base_url = Some(region.base_url());
        self
    }

    /// Selects Zhipu's international host (`api.z.ai`) instead of the CN
    /// default (`open.bigmodel.cn`). Keys are issued per site.
    pub fn with_zhipu_international(mut self) -> Self {
        debug_assert_eq!(self.provider, Provider::Zhipu);
        self.base_url = Some("https://api.z.ai/api/paas/v4".into());
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

    /// Overrides the streaming timeouts: the first event must arrive within
    /// `first_token`, and every subsequent event within `idle`.
    pub fn with_stream_timeouts(
        mut self,
        first_token: Duration,
        idle: Duration,
    ) -> Self {
        self.stream_first_token_timeout = first_token;
        self.stream_idle_timeout = idle;
        self
    }

    /// Renders the exact JSON body a Chat would send, without sending it and
    /// without any credential material — the api key travels only in the
    /// Authorization header, which never appears here.
    pub fn debug_request(&self, messages: &[Message], options: &Options) -> String {
        let body = dialect::chat_body(self.provider, &self.model, messages, options, false);
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
        gate::gate(&resolved, &request, self.provider, &self.model).map_err(|e| self.enrich(e))?;

        let body = dialect::chat_body(self.provider, &self.model, &request.messages, &request.options, false);
        self.send(&body).await
    }

    /// Streaming variant of `chat`: same Client, same Gate, same dialect.
    /// The returned `ChatStream` normalizes every provider into the same
    /// chunk sequence; dropping it cancels the request.
    pub async fn chat_stream(&self, messages: Vec<Message>) -> Result<crate::stream::ChatStream, Error> {
        self.chat_stream_with(messages, Options::default()).await
    }

    pub async fn chat_stream_with(
        &self,
        messages: Vec<Message>,
        options: Options,
    ) -> Result<crate::stream::ChatStream, Error> {
        let merged = Options::layered(self.options.clone(), options);
        let request = ChatRequest { messages, options: merged, stream: true };

        let resolved = self.resolve();
        gate::gate(&resolved, &request, self.provider, &self.model).map_err(|e| self.enrich(e))?;

        let body = dialect::chat_body(self.provider, &self.model, &request.messages, &request.options, true);
        let url = self.url();
        let mut request = self.apply_auth(self.http.post(&url).timeout(self.timeout).json(&body));
        // Qwen's streaming endpoints require an explicit SSE opt-in header;
        // harmless on compatible-mode, required on native mode.
        if self.provider == Provider::DashScope {
            request = request.header("X-DashScope-SSE", "enable");
        }
        let response = request
            .send()
            .await
            .map_err(|e| Error::Transport(TransportError { message: e.to_string() }))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let raw_body = response
                .text()
                .await
                .map_err(|e| Error::Transport(TransportError { message: e.to_string() }))?;
            return Err(Error::Api(ApiError {
                status,
                provider_message: extract_provider_message(&raw_body),
                raw_body,
            }));
        }

        Ok(crate::stream::ChatStream::new(
            response,
            self.stream_first_token_timeout,
            self.stream_idle_timeout,
        ))
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

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Auth::Bearer => request.bearer_auth(&self.api_key),
            Auth::Header { name, value } => request.header(name, value),
        }
    }

    /// Makes a Gate refusal actionable: for structured-output refusals the
    /// Catalog names capable models on the same provider, when it knows any.
    fn enrich(&self, error: Error) -> Error {
        match error {
            Error::Capability(ce)
                if ce.missing == MissingCapability::StructuredOutput && ce.suggestions.is_empty() =>
            {
                Error::Capability(CapabilityError {
                    suggestions: default_catalog().models_with(self.provider, |c| c.structured_output),
                    ..ce
                })
            }
            other => other,
        }
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
        let request = self.apply_auth(self.http.post(url).timeout(self.timeout).json(body));
        let response = request
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

    fn sse_body() -> String {
        let mut s = String::new();
        for event in [
            r#"{"choices":[{"index":0,"delta":{"content":"He"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"llo"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"hmm"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#,
        ] {
            s.push_str(&format!("data: {event}\n\n"));
        }
        s.push_str("data: [DONE]\n\n");
        s
    }

    #[tokio::test]
    async fn stream_replays_normalized_chunks_with_final_usage() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "stream": true,
                "stream_options": { "include_usage": true }
            })))
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body())
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url());
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");

        let mut deltas = Vec::new();
        let mut last_usage = None;
        let mut finish = None;
        while let Some(item) = stream.next().await {
            let chunk = item.expect("no chunk may fail");
            deltas.push(chunk.delta);
            if chunk.finish_reason.is_some() {
                finish = chunk.finish_reason;
            }
            if let Some(u) = chunk.usage {
                last_usage = Some(u);
            }
        }
        mock.assert_async().await;

        assert_eq!(
            deltas,
            vec![
                Some(crate::stream::Delta::Text { text: "He".into() }),
                Some(crate::stream::Delta::Text { text: "llo".into() }),
                Some(crate::stream::Delta::Reasoning { text: "hmm".into() }),
                None,
            ]
        );
        assert_eq!(finish.as_deref(), Some("stop"));
        assert_eq!(last_usage.map(|u| u.total_tokens), Some(Some(5)));
    }

    #[tokio::test]
    async fn mid_stream_provider_error_surfaces_as_api_after_partial_content() {
        let mut server = Server::new_async().await;
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: {{\"error\":{{\"message\":\"overloaded mid-stream\"}}}}\n\n",
            r#"{"choices":[{"delta":{"content":"par"}}]}"#,
            r#"{"choices":[{"delta":{"content":"tial"}}]}"#,
        );
        server
            .mock("POST", "/chat/completions")
            .with_body(body)
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url());
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");

        let mut partial = String::new();
        let err = loop {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    if let Some(crate::stream::Delta::Text { text }) = chunk.delta {
                        partial.push_str(&text);
                    }
                }
                Some(Err(e)) => break e,
                None => panic!("expected a mid-stream error"),
            }
        };

        assert_eq!(partial, "partial");
        match err {
            Error::Api(e) => {
                assert!(e.provider_message.contains("overloaded mid-stream"));
                assert!(e.raw_body.contains("overloaded mid-stream"));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn first_token_timeout_enforced_and_overridable() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_chunked_body(|w| {
                std::thread::sleep(Duration::from_millis(600));
                let _ = w.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n");
                Ok(())
            })
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url())
            .with_stream_timeouts(Duration::from_millis(50), Duration::from_secs(90));
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");
        let err = stream.next().await.expect("must yield an item").expect_err("must be a timeout");
        assert!(matches!(err, Error::Transport(_)), "got {err:?}");
        assert!(err.to_string().contains("first stream event"));
    }

    #[tokio::test]
    async fn idle_timeout_enforced_and_overridable() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_chunked_body(|w| {
                let _ = w.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"soon\"}}]}\n\n");
                std::thread::sleep(Duration::from_millis(600));
                let _ = w.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"too late\"}}]}\n\n");
                Ok(())
            })
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url())
            .with_stream_timeouts(Duration::from_secs(60), Duration::from_millis(50));
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");

        let first = stream.next().await.expect("first chunk").expect("first chunk ok");
        assert!(matches!(first.delta, Some(crate::stream::Delta::Text { ref text }) if text == "soon"));
        let err = stream.next().await.expect("must yield an item").expect_err("must be idle timeout");
        assert!(matches!(err, Error::Transport(_)), "got {err:?}");
        assert!(err.to_string().contains("idle timeout"));
    }

    #[tokio::test]
    async fn dropping_the_stream_does_not_hang() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .with_body(sse_body())
            .create_async()
            .await;

        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url());
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");
        let _ = stream.next().await;
        drop(stream); // cancels the request; must not block or panic
    }

    #[tokio::test]
    async fn moonshot_uses_bearer_auth_and_v1_path() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("authorization", "Bearer sk-kimi")
            .with_body(success_body())
            .create_async()
            .await;
        let client = Client::new(Provider::Moonshot, "kimi-k3", "sk-kimi")
            .with_base_url(server.url());
        client.chat(vec![Message::text("hello")]).await.expect("chat must succeed");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn openrouter_passes_composite_ids_through() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("authorization", "Bearer sk-or")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "model": "deepseek/deepseek-v4-pro"
            })))
            .with_body(success_body())
            .create_async()
            .await;
        let client = Client::new(Provider::OpenRouter, "deepseek/deepseek-v4-pro", "sk-or")
            .with_base_url(server.url());
        client.chat(vec![Message::text("hello")]).await.expect("chat must succeed");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mimo_authenticates_with_api_key_header() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("api-key", "sk-mimo")
            .with_body(success_body())
            .create_async()
            .await;
        let client = Client::new(Provider::MiMo, "mimo-v2.5", "sk-mimo")
            .with_base_url(server.url());
        client.chat(vec![Message::text("hello")]).await.expect("chat must succeed");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn mimo_streaming_uses_the_same_auth() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .match_header("api-key", "sk-mimo")
            .with_body(sse_body())
            .create_async()
            .await;
        let client = Client::new(Provider::MiMo, "mimo-v2.5", "sk-mimo")
            .with_base_url(server.url());
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");
        let chunk = stream.next().await.expect("first chunk").expect("chunk ok");
        assert!(chunk.delta.is_some());
    }

    #[test]
    fn env_key_fallback_per_provider() {
        let name = Provider::OpenRouter.env_key_name();
        assert_eq!(name, "OPENROUTER_API_KEY");
        // SAFETY: tests run in one process per binary; rustc test harness runs
        // tests on multiple threads, so a name collision would race — these
        // names are unique per test below.
        unsafe { std::env::set_var(name, "sk-env") };
        let client = Client::from_env(Provider::OpenRouter, "deepseek/deepseek-v4-pro").expect("env key must be found");
        assert_eq!(client.api_key, "sk-env");

        unsafe { std::env::remove_var(name) };
        let err = match Client::from_env(Provider::OpenRouter, "deepseek/deepseek-v4-pro") {
            Ok(_) => panic!("missing env key must fail"),
            Err(err) => err,
        };
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("OPENROUTER_API_KEY"));
    }

    #[tokio::test]
    async fn dashscope_chat_uses_bearer_and_compatible_path() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("authorization", "Bearer sk-dash")
            .match_body(Matcher::PartialJson(serde_json::json!({ "model": "qwen-plus" })))
            .with_body(success_body())
            .create_async()
            .await;
        let client = Client::new(Provider::DashScope, "qwen-plus", "sk-dash")
            .with_base_url(server.url());
        client.chat(vec![Message::text("hello")]).await.expect("chat must succeed");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn dashscope_streaming_sends_the_sse_header() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/chat/completions")
            .match_header("X-DashScope-SSE", "enable")
            .with_body(sse_body())
            .create_async()
            .await;
        let client = Client::new(Provider::DashScope, "qwen-plus", "sk-dash")
            .with_base_url(server.url());
        let mut stream = client.chat_stream(vec![Message::text("hello")]).await.expect("stream must start");
        let chunk = stream.next().await.expect("first chunk").expect("chunk ok");
        assert!(chunk.delta.is_some());
    }

    #[test]
    fn dashscope_region_shapes_endpoints() {
        use crate::provider::DashScopeRegion;
        assert_eq!(
            DashScopeRegion::Cn.base_url(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(
            DashScopeRegion::Singapore.base_url(),
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(
            DashScopeRegion::Workspace {
                id: "llm-xxx".into(),
                region: crate::provider::DASHSCOPE_CN_BEIJING,
            }
            .base_url(),
            "https://llm-xxx.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
        );

        let client = Client::new(Provider::DashScope, "qwen-plus", "sk")
            .with_dashscope_region(DashScopeRegion::Workspace {
                id: "llm-xxx".into(),
                region: crate::provider::DASHSCOPE_AP_SOUTHEAST_1,
            });
        assert_eq!(
            client.url(),
            "https://llm-xxx.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
    }

    #[tokio::test]
    async fn zhipu_chat_on_cn_default() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("authorization", "Bearer sk-glm")
            .match_body(Matcher::PartialJson(serde_json::json!({ "model": "glm-5.3" })))
            .with_body(success_body())
            .create_async()
            .await;
        let client = Client::new(Provider::Zhipu, "glm-5.3", "sk-glm")
            .with_base_url(server.url());
        client.chat(vec![Message::text("hello")]).await.expect("chat must succeed");
        mock.assert_async().await;
    }

    #[test]
    fn zhipu_international_host_selectable() {
        let client = Client::new(Provider::Zhipu, "glm-5.3", "sk").with_zhipu_international();
        assert_eq!(client.url(), "https://api.z.ai/api/paas/v4/chat/completions");
        // CN remains the default.
        let client = Client::new(Provider::Zhipu, "glm-5.3", "sk");
        assert_eq!(client.url(), "https://open.bigmodel.cn/api/paas/v4/chat/completions");
    }

    #[tokio::test]
    async fn structured_output_uses_native_response_format() {
        let mut server = Server::new_async().await;
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "answer": { "type": "integer" } },
            "required": ["answer"]
        });
        let mock = server
            .mock("POST", "/chat/completions")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "response", "strict": true, "schema": schema }
                }
            })))
            .with_body(success_body())
            .create_async()
            .await;
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test")
            .with_base_url(server.url());
        client
            .chat_with(
                vec![Message::text("give an answer")],
                Options { response_schema: Some(schema), ..Default::default() },
            )
            .await
            .expect("capable model must serve the schema");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn structured_output_refused_locally_with_suggestions() {
        // glm-5 has structured_output=false in the real snapshot.
        let client = Client::new(Provider::Zhipu, "glm-5", "sk-test");
        // No mockito server exists: any network attempt fails as Transport.
        let err = client
            .chat_with(
                vec![Message::text("give an answer")],
                Options { response_schema: Some(serde_json::json!({"type": "object"})), ..Default::default() },
            )
            .await
            .expect_err("glm-5 must be refused locally");
        match err {
            Error::Capability(e) => {
                assert_eq!(e.model, "glm-5");
                assert_eq!(e.missing, MissingCapability::StructuredOutput);
                assert!(
                    e.suggestions.contains(&"glm-5.3".to_string()),
                    "suggestions must name capable models, got {:?}",
                    e.suggestions
                );
            }
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn no_prompt_simulation_path_exists() {
        // The schema appears only in response_format; messages are untouched
        // and no JSON instructions are injected anywhere in the body.
        let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", "sk-test");
        let schema = serde_json::json!({"type": "object", "properties": {"a": {"type": "integer"}}});
        let rendered = client.debug_request(
            &[Message::text("give an answer")],
            &Options { response_schema: Some(schema), ..Default::default() },
        );
        let body: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let messages = body.get("messages").expect("messages present");
        assert_eq!(messages.as_array().unwrap().len(), 1);
        assert_eq!(messages[0]["content"], "give an answer");
        assert_eq!(body.pointer("/response_format/type"), Some(&serde_json::json!("json_schema")));
        let outside_messages = rendered.split("\"messages\"").next().unwrap_or("");
        assert!(
            !outside_messages.to_lowercase().contains("json"),
            "no prompt text may mention json outside response_format"
        );
    }
}
