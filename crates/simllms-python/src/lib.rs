//! Python bindings for simllms: `simllms._core`.
//!
//! One pyclass (`Client`) mirroring the Rust builder surface as constructor
//! kwargs, one exception per Rust error layer, and a per-call tokio runtime
//! kept off the GIL. Async lives in the pure-Python layer (ADR-0003).

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};

use simllms::capabilities::CapabilityOverrides;
use simllms::error::{Error, MissingCapability};
use simllms::message::{ContentBlock, ImageSource, Message, Role};
use simllms::options::Options;
use simllms::provider::{
    DASHSCOPE_AP_NORTHEAST_1, DASHSCOPE_AP_SOUTHEAST_1, DASHSCOPE_CN_BEIJING,
    DASHSCOPE_CN_HONGKONG, DASHSCOPE_EU_CENTRAL_1, DASHSCOPE_US_EAST_1, DashScopeRegion, Provider,
};
use simllms::reply::Reply;

create_exception!(
    simllms,
    SimLLMError,
    PyException,
    "Base class for every simllms error."
);
create_exception!(
    simllms,
    CapabilityError,
    SimLLMError,
    "The Gate refused this request before any network call. Attributes: model, missing, suggestions."
);
create_exception!(
    simllms,
    ConfigError,
    SimLLMError,
    "Invalid client configuration (bad base_url, unsupported parameter, ...)."
);
create_exception!(
    simllms,
    APIError,
    SimLLMError,
    "The provider returned an error. Attributes: status, provider_message, raw_body — raw_body is the provider's answer untouched, take it to their support."
);
create_exception!(
    simllms,
    TransportError,
    SimLLMError,
    "The request never completed (connection failed, timeout). Retryable."
);

fn missing_name(missing: &MissingCapability) -> String {
    match missing {
        MissingCapability::ImageInput => "ImageInput".into(),
        MissingCapability::StructuredOutput => "StructuredOutput".into(),
        MissingCapability::Reasoning => "Reasoning".into(),
        MissingCapability::ReasoningEffort(effort) => format!("ReasoningEffort({effort})"),
        MissingCapability::ImageGeneration => "ImageGeneration".into(),
        MissingCapability::ToolCall => "ToolCall".into(),
        _ => "Unknown".into(),
    }
}

fn parse_provider(name: &str) -> PyResult<Provider> {
    match name {
        "deepseek" => Ok(Provider::DeepSeek),
        "moonshot" => Ok(Provider::Moonshot),
        "dashscope" => Ok(Provider::DashScope),
        "zhipu" => Ok(Provider::Zhipu),
        "mimo" => Ok(Provider::MiMo),
        "openrouter" => Ok(Provider::OpenRouter),
        "custom" => Ok(Provider::Custom),
        other => Err(PyTypeError::new_err(format!(
            "unknown provider `{other}`; expected deepseek, moonshot, dashscope, zhipu, mimo, openrouter, or custom"
        ))),
    }
}

/// `("bearer",)` or a `(name, value)` pair, as the pure-Python layer sends
/// it. `None` keeps the provider default (Bearer, except MiMo's `api-key:`).
fn apply_auth(client: simllms::Client, auth: &Bound<'_, PyAny>) -> PyResult<simllms::Client> {
    if let Ok((kind, name, value)) = auth.extract::<(String, String, String)>() {
        if kind == "header" {
            return Ok(client.with_auth(simllms::Auth::Header { name, value }));
        }
        return Err(PyTypeError::new_err(format!(
            "unknown auth shape `{kind}`; expected `(\"bearer\",)` or a `(\"header\", name, value)` triple"
        )));
    }
    if let Ok((kind,)) = auth.extract::<(String,)>() {
        if kind == "bearer" {
            return Ok(client.with_auth(simllms::Auth::Bearer));
        }
        return Err(PyTypeError::new_err(format!(
            "unknown auth shape `{kind}`; expected `(\"bearer\",)` or a `(header, name, value)` triple"
        )));
    }
    Err(PyTypeError::new_err(
        "auth must be `(\"bearer\",)` or a `(header, name, value)` triple",
    ))
}

/// `"cn"`, `"singapore"`, or a `("workspace", id, region)` triple. The region
/// must be one DashScope documents for workspaces — anything else is a typo,
/// and a typo here points real traffic at a host that does not exist.
fn apply_dashscope_region(
    client: simllms::Client,
    region: &Bound<'_, PyAny>,
) -> PyResult<simllms::Client> {
    if let Ok(kind) = region.extract::<String>() {
        let region = match kind.as_str() {
            "cn" => DashScopeRegion::Cn,
            "singapore" => DashScopeRegion::Singapore,
            other => {
                return Err(PyTypeError::new_err(format!(
                    "unknown DashScope region `{other}`; expected \"cn\", \"singapore\", or a (\"workspace\", id, region) triple"
                )));
            }
        };
        return Ok(client.with_dashscope_region(region));
    }
    if let Ok((kind, id, workspace_region)) = region.extract::<(String, String, String)>() {
        if kind != "workspace" {
            return Err(PyTypeError::new_err(format!(
                "unknown DashScope region shape `{kind}`; expected \"cn\", \"singapore\", or a (\"workspace\", id, region) triple"
            )));
        }
        let known = match workspace_region.as_str() {
            DASHSCOPE_CN_BEIJING => DASHSCOPE_CN_BEIJING,
            DASHSCOPE_AP_SOUTHEAST_1 => DASHSCOPE_AP_SOUTHEAST_1,
            DASHSCOPE_CN_HONGKONG => DASHSCOPE_CN_HONGKONG,
            DASHSCOPE_EU_CENTRAL_1 => DASHSCOPE_EU_CENTRAL_1,
            DASHSCOPE_AP_NORTHEAST_1 => DASHSCOPE_AP_NORTHEAST_1,
            DASHSCOPE_US_EAST_1 => DASHSCOPE_US_EAST_1,
            other => {
                return Err(PyTypeError::new_err(format!(
                    "unknown DashScope workspace region `{other}`; expected cn-beijing, ap-southeast-1, cn-hongkong, eu-central-1, ap-northeast-1, or us-east-1"
                )));
            }
        };
        return Ok(client.with_dashscope_region(DashScopeRegion::Workspace { id, region: known }));
    }
    Err(PyTypeError::new_err(
        "dashscope_region must be \"cn\", \"singapore\", or a (\"workspace\", id, region) triple",
    ))
}

fn map_error(err: Error) -> PyErr {
    Python::attach(|py| {
        let py_err = match &err {
            Error::Capability(e) => {
                let pe = CapabilityError::new_err(e.to_string());
                let v = pe.value(py);
                let _ = v.setattr("model", e.model.clone());
                let _ = v.setattr("missing", missing_name(&e.missing));
                let _ = v.setattr("suggestions", e.suggestions.clone());
                pe
            }
            Error::Config(e) => ConfigError::new_err(e.message.clone()),
            Error::Api(e) => {
                let pe = APIError::new_err(e.to_string());
                let v = pe.value(py);
                let _ = v.setattr("status", e.status);
                let _ = v.setattr("provider_message", e.provider_message.clone());
                let _ = v.setattr("raw_body", e.raw_body.clone());
                pe
            }
            Error::Transport(e) => TransportError::new_err(e.message.clone()),
            _ => SimLLMError::new_err(err.to_string()),
        };
        py_err
    })
}

fn pyany_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if value.is_none() {
        return Ok(serde_json::Value::Null);
    }
    // bool before int: bool is a subclass of int on the wire.
    if let Ok(b) = value.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = value.extract::<i64>() {
        return Ok(serde_json::Value::Number(i.into()));
    }
    if let Ok(f) = value.extract::<f64>() {
        return serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .ok_or_else(|| PyTypeError::new_err("cannot convert non-finite float to JSON"));
    }
    if let Ok(s) = value.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = value.cast::<PyList>() {
        let items: PyResult<Vec<serde_json::Value>> =
            list.iter().map(|v| pyany_to_json(&v)).collect();
        return Ok(serde_json::Value::Array(items?));
    }
    if let Ok(dict) = value.cast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, pyany_to_json(&v)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    Err(PyTypeError::new_err(format!(
        "cannot convert {} to JSON",
        value.get_type().name()?
    )))
}

fn parse_options(dict: &Bound<'_, PyDict>) -> PyResult<Options> {
    let mut options = Options::default();
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        match key.as_str() {
            "temperature" => options.temperature = v.extract()?,
            "top_p" => options.top_p = v.extract()?,
            "top_k" => options.top_k = v.extract()?,
            "max_tokens" => options.max_tokens = v.extract()?,
            "stop" => options.stop = Some(v.extract()?),
            "seed" => options.seed = v.extract()?,
            "presence_penalty" => options.presence_penalty = v.extract()?,
            "frequency_penalty" => options.frequency_penalty = v.extract()?,
            "reasoning_enabled" => options.reasoning_enabled = v.extract()?,
            "reasoning_effort" => {
                options.reasoning_effort =
                    Some(simllms::options::ReasoningEffort(v.extract::<String>()?))
            }
            "response_schema" => options.response_schema = Some(pyany_to_json(&v)?),
            "extra" => {
                let extra = v
                    .cast::<PyDict>()
                    .map_err(|_| PyTypeError::new_err("`extra` must be a dict"))?;
                for (ek, ev) in extra.iter() {
                    let ekey: String = ek.extract()?;
                    options.extra.insert(ekey, pyany_to_json(&ev)?);
                }
            }
            other => {
                return Err(PyTypeError::new_err(format!(
                    "unknown option `{other}`; provider-specific extras go under `extra`"
                )));
            }
        }
    }
    Ok(options)
}

fn parse_overrides(dict: &Bound<'_, PyDict>) -> PyResult<CapabilityOverrides> {
    let mut overrides = CapabilityOverrides::default();
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        match key.as_str() {
            "image_input" => overrides.image_input = v.extract()?,
            "structured_output" => overrides.structured_output = v.extract()?,
            "tool_call" => overrides.tool_call = v.extract()?,
            "image_generation" => overrides.image_generation = v.extract()?,
            "context_window" => overrides.context_window = v.extract()?,
            "max_output_tokens" => overrides.max_output_tokens = v.extract()?,
            "reasoning" => {
                let mode: String = v.extract()?;
                overrides.reasoning = Some(match mode.as_str() {
                    "none" => simllms::capabilities::ReasoningMode::None,
                    "toggle" => simllms::capabilities::ReasoningMode::Toggle,
                    other => {
                        let levels: Vec<String> = other
                            .split('+')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if levels.is_empty() {
                            return Err(PyTypeError::new_err(
                                "reasoning override must be \"none\", \"toggle\", or effort levels joined by `+` (e.g. \"low+high\")",
                            ));
                        }
                        simllms::capabilities::ReasoningMode::Effort {
                            levels: levels.into_iter().collect(),
                        }
                    }
                });
            }
            other => {
                return Err(PyTypeError::new_err(format!(
                    "unknown capability override `{other}`"
                )));
            }
        }
    }
    Ok(overrides)
}

fn parse_role(role: &str) -> PyResult<Role> {
    match role {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        other => Err(PyTypeError::new_err(format!(
            "unknown role `{other}`; expected system, user, or assistant"
        ))),
    }
}

fn parse_block(dict: &Bound<'_, PyDict>) -> PyResult<ContentBlock> {
    let kind: String = dict
        .get_item("type")?
        .ok_or_else(|| PyTypeError::new_err("content block needs a `type`"))?
        .extract()?;
    match kind.as_str() {
        "text" => {
            let text: String = dict
                .get_item("text")?
                .ok_or_else(|| PyTypeError::new_err("text block needs `text`"))?
                .extract()?;
            Ok(ContentBlock::Text(text))
        }
        "image_url" => {
            let url: String = dict
                .get_item("url")?
                .ok_or_else(|| PyTypeError::new_err("image_url block needs `url`"))?
                .extract()?;
            Ok(ContentBlock::Image(ImageSource::Url(url)))
        }
        "image_base64" => {
            let data: String = dict
                .get_item("data")?
                .ok_or_else(|| PyTypeError::new_err("image_base64 block needs `data`"))?
                .extract()?;
            let mime_type: String = dict
                .get_item("mime_type")?
                .ok_or_else(|| PyTypeError::new_err("image_base64 block needs `mime_type`"))?
                .extract()?;
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| {
                    PyTypeError::new_err(format!("image_base64 `data` is not valid base64: {e}"))
                })?;
            Ok(ContentBlock::Image(ImageSource::Base64 {
                data: bytes,
                mime_type,
            }))
        }
        other => Err(PyTypeError::new_err(format!(
            "unknown content block type `{other}`; expected text, image_url, or image_base64"
        ))),
    }
}

fn parse_messages(list: &Bound<'_, PyList>) -> PyResult<Vec<Message>> {
    let mut messages = Vec::with_capacity(list.len());
    for item in list.iter() {
        if let Ok(text) = item.extract::<String>() {
            messages.push(Message::text(text));
            continue;
        }
        let dict = item
            .cast::<PyDict>()
            .map_err(|_| PyTypeError::new_err("each message must be a str or a dict"))?;
        let role: String = dict
            .get_item("role")?
            .ok_or_else(|| PyTypeError::new_err("message dict needs `role`"))?
            .extract()?;
        let role = parse_role(&role)?;
        let content_value = dict
            .get_item("content")?
            .ok_or_else(|| PyTypeError::new_err("message dict needs `content`"))?;
        let content = if let Ok(text) = content_value.extract::<String>() {
            vec![ContentBlock::Text(text)]
        } else if let Ok(blocks) = content_value.cast::<PyList>() {
            blocks
                .iter()
                .map(|b| {
                    parse_block(
                        b.cast::<PyDict>()
                            .map_err(|_| PyTypeError::new_err("content blocks must be dicts"))?,
                    )
                })
                .collect::<PyResult<Vec<_>>>()?
        } else {
            return Err(PyTypeError::new_err(
                "message `content` must be a str or a list of blocks",
            ));
        };
        messages.push(Message::blocks(role, content));
    }
    Ok(messages)
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for simllms")
    })
}

/// `Reply` as Python sees it: the content plus the usage the provider
/// reported (None when the provider sent none).
#[pyclass]
struct PyReply {
    #[pyo3(get)]
    content: String,
    #[pyo3(get)]
    prompt_tokens: Option<u64>,
    #[pyo3(get)]
    completion_tokens: Option<u64>,
    #[pyo3(get)]
    total_tokens: Option<u64>,
}

impl From<Reply> for PyReply {
    fn from(reply: Reply) -> Self {
        let usage = reply.usage;
        PyReply {
            content: reply.content,
            prompt_tokens: usage.as_ref().and_then(|u| u.prompt_tokens),
            completion_tokens: usage.as_ref().and_then(|u| u.completion_tokens),
            total_tokens: usage.and_then(|u| u.total_tokens),
        }
    }
}

/// One Gate warning: an unknown Model Id passes through (ADR-0005) and a
/// deprecated one is still served, and `warnings()` makes both visible
/// before a call is paid for. `kind` is `"unknown_model"` or
/// `"deprecated_model"`; `provider` is set only for the unknown kind.
#[pyclass(eq)]
#[derive(Clone, PartialEq, Eq)]
struct PyWarning {
    #[pyo3(get)]
    kind: String,
    #[pyo3(get)]
    provider: Option<String>,
    #[pyo3(get)]
    model: String,
}

impl From<simllms::Warning> for PyWarning {
    fn from(warning: simllms::Warning) -> Self {
        match warning {
            simllms::Warning::UnknownModel { provider, model } => PyWarning {
                kind: "unknown_model".into(),
                provider: Some(provider),
                model,
            },
            simllms::Warning::DeprecatedModel { model } => PyWarning {
                kind: "deprecated_model".into(),
                provider: None,
                model,
            },
            // `Warning` is `#[non_exhaustive]`, so the compiler cannot make a
            // future variant a compile error here. A warning that reaches
            // Python unmapped would silently lie about its kind — crash in
            // development instead.
            _ => unreachable!("a new `simllms::Warning` variant must be mapped here"),
        }
    }
}

#[pyclass]
struct Client {
    inner: Arc<simllms::Client>,
}

/// One normalized streaming step: a text or reasoning delta, plus the final
/// chunk's finish reason and usage (token fields are None until the final
/// chunk carries them).
#[pyclass]
struct PyChunk {
    #[pyo3(get)]
    index: u64,
    #[pyo3(get)]
    text: Option<String>,
    #[pyo3(get)]
    reasoning: Option<String>,
    #[pyo3(get)]
    finish_reason: Option<String>,
    #[pyo3(get)]
    prompt_tokens: Option<u64>,
    #[pyo3(get)]
    completion_tokens: Option<u64>,
    #[pyo3(get)]
    total_tokens: Option<u64>,
}

impl From<simllms::stream::Chunk> for PyChunk {
    fn from(chunk: simllms::stream::Chunk) -> Self {
        use simllms::stream::Delta;
        let (text, reasoning) = match chunk.delta {
            Some(Delta::Text { text }) => (Some(text), None),
            Some(Delta::Reasoning { text }) => (None, Some(text)),
            _ => (None, None),
        };
        PyChunk {
            index: chunk.index,
            text,
            reasoning,
            finish_reason: chunk.finish_reason,
            prompt_tokens: chunk.usage.as_ref().and_then(|u| u.prompt_tokens),
            completion_tokens: chunk.usage.as_ref().and_then(|u| u.completion_tokens),
            total_tokens: chunk.usage.as_ref().and_then(|u| u.total_tokens),
        }
    }
}

/// Sync iterator over a `chat_stream`. Each `next()` parks the calling
/// thread on one SSE event; the GIL is released while waiting. Errors
/// raised mid-stream end the iteration on the following step.
#[pyclass]
struct PyChatStream {
    stream: Arc<std::sync::Mutex<simllms::stream::ChatStream>>,
}

#[pymethods]
impl PyChatStream {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<Option<PyChunk>> {
        let stream = self.stream.clone();
        py.detach(move || {
            let mut guard = stream.lock().expect("chat stream lock");
            runtime()
                .block_on(async { guard.next().await })
                .map(|result| result.map(PyChunk::from))
                .transpose()
        })
        .map_err(map_error)
    }
}

/// One generated image: exactly one of `data` (bytes) or `url`.
#[pyclass]
struct PyGeneratedImage {
    #[pyo3(get)]
    data: Option<Vec<u8>>,
    #[pyo3(get)]
    url: Option<String>,
    #[pyo3(get)]
    mime_type: Option<String>,
    #[pyo3(get)]
    revised_prompt: Option<String>,
}

impl From<simllms::image::GeneratedImage> for PyGeneratedImage {
    fn from(g: simllms::image::GeneratedImage) -> Self {
        PyGeneratedImage {
            data: g.data,
            url: g.url,
            mime_type: g.mime_type,
            revised_prompt: g.revised_prompt,
        }
    }
}

#[pymethods]
impl Client {
    #[new]
    #[pyo3(signature = (provider, model, api_key=None, *, base_url=None, options=None, capability_overrides=None, timeout=None, auth=None, dashscope_region=None, zhipu_international=false, default_headers=None, retry=None, stream_timeouts=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        provider: &str,
        model: &str,
        api_key: Option<String>,
        base_url: Option<String>,
        options: Option<&Bound<'_, PyDict>>,
        capability_overrides: Option<&Bound<'_, PyDict>>,
        timeout: Option<f64>,
        auth: Option<&Bound<'_, PyAny>>,
        dashscope_region: Option<&Bound<'_, PyAny>>,
        zhipu_international: bool,
        default_headers: Option<&Bound<'_, PyDict>>,
        retry: Option<(bool, u32)>,
        stream_timeouts: Option<(f64, f64)>,
    ) -> PyResult<Self> {
        let provider = parse_provider(provider)?;
        let mut client = match api_key {
            Some(key) => simllms::Client::new(provider, model, key),
            None => simllms::Client::from_env(provider, model).map_err(map_error)?,
        };
        if let Some(dict) = options {
            client = client.with_options(parse_options(dict)?);
        }
        if let Some(dict) = capability_overrides {
            client = client.with_capability_overrides(parse_overrides(dict)?);
        }
        if let Some(auth) = auth {
            client = apply_auth(client, auth)?;
        }
        if let Some(region) = dashscope_region {
            client = apply_dashscope_region(client, region)?;
        }
        if zhipu_international {
            client = client.with_zhipu_international();
        }
        if let Some(headers) = default_headers {
            for (name, value) in headers.iter() {
                client = client
                    .with_default_header(name.extract::<String>()?, value.extract::<String>()?);
            }
        }
        if let Some((enabled, max_attempts)) = retry {
            client = client.with_retry(simllms::RetryPolicy {
                enabled,
                max_attempts,
            });
        }
        if let Some((first_token, idle)) = stream_timeouts {
            client = client.with_stream_timeouts(
                Duration::from_secs_f64(first_token),
                Duration::from_secs_f64(idle),
            );
        }
        if let Some(seconds) = timeout {
            client = client.with_timeout(Duration::from_secs_f64(seconds));
        }
        // Applied last on purpose: an explicit base_url outranks the host a
        // region or the international endpoint would pick, which is also the
        // escape hatch that keeps the binding tests on the mock gateway.
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        Ok(Client {
            inner: Arc::new(client),
        })
    }

    /// Chat once. `messages` is a list of str (treated as user messages) or
    /// dicts `{"role": ..., "content": ...}`. Returns a Reply.
    #[pyo3(signature = (messages, options=None))]
    fn chat(
        &self,
        py: Python<'_>,
        messages: &Bound<'_, PyList>,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyReply> {
        let messages = parse_messages(messages)?;
        let options = match options {
            Some(dict) => parse_options(dict)?,
            None => Options::default(),
        };
        let inner = self.inner.clone();
        py.detach(move || {
            runtime()
                .block_on(async move { inner.chat_with(messages, options).await })
                .map(PyReply::from)
                .map_err(map_error)
        })
    }

    /// Stream a chat. Returns an iterator of Chunk; each `next()` blocks the
    /// calling thread on one SSE event (the async path lives in the pure-
    /// Python layer, which parks each step on a worker thread).
    #[pyo3(signature = (messages, options=None))]
    fn chat_stream(
        &self,
        py: Python<'_>,
        messages: &Bound<'_, PyList>,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyChatStream> {
        let messages = parse_messages(messages)?;
        let options = match options {
            Some(dict) => parse_options(dict)?,
            None => Options::default(),
        };
        let inner = self.inner.clone();
        let stream = py
            .detach(move || {
                runtime().block_on(async move { inner.chat_stream_with(messages, options).await })
            })
            .map_err(map_error)?;
        Ok(PyChatStream {
            stream: Arc::new(std::sync::Mutex::new(stream)),
        })
    }

    /// Generate an image. Unsupported parameters for this provider/model
    /// raise ConfigError naming the parameter; result is a list of
    /// GeneratedImage (bytes-or-URL invariant).
    #[pyo3(signature = (prompt, *, size=None, aspect_ratio=None, n=None, negative_prompt=None, seed=None, quality=None, output_format=None, background=None, poll_interval=None))]
    #[allow(clippy::too_many_arguments)]
    fn generate_image(
        &self,
        py: Python<'_>,
        prompt: String,
        size: Option<String>,
        aspect_ratio: Option<String>,
        n: Option<u32>,
        negative_prompt: Option<String>,
        seed: Option<i64>,
        quality: Option<String>,
        output_format: Option<String>,
        background: Option<String>,
        poll_interval: Option<f64>,
    ) -> PyResult<Vec<PyGeneratedImage>> {
        let request = simllms::ImageRequest {
            prompt,
            size,
            aspect_ratio,
            n,
            negative_prompt,
            seed,
            quality,
            output_format,
            background,
            input_references: Vec::new(),
        };
        let mut knobs = simllms::ImageKnobs::default();
        if let Some(seconds) = poll_interval {
            knobs.poll_interval = Duration::from_secs_f64(seconds);
        }
        let inner = self.inner.clone();
        py.detach(move || {
            runtime()
                .block_on(async move { inner.generate_image_with(request, knobs).await })
                .map(|reply| {
                    reply
                        .images
                        .into_iter()
                        .map(PyGeneratedImage::from)
                        .collect()
                })
                .map_err(map_error)
        })
    }

    /// The exact request this client would send, credential-free.
    #[pyo3(signature = (messages, options=None))]
    fn debug_request(
        &self,
        messages: &Bound<'_, PyList>,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        let messages = parse_messages(messages)?;
        let options = match options {
            Some(dict) => parse_options(dict)?,
            None => Options::default(),
        };
        Ok(self.inner.debug_request(&messages, &options))
    }

    /// The Gate's structured warnings for this request, answered from the
    /// catalog alone — no network call, no refusal.
    #[pyo3(signature = (messages, options=None))]
    fn warnings(
        &self,
        messages: &Bound<'_, PyList>,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Vec<PyWarning>> {
        let messages = parse_messages(messages)?;
        let options = match options {
            Some(dict) => parse_options(dict)?,
            None => Options::default(),
        };
        Ok(self
            .inner
            .warnings(&messages, &options)
            .into_iter()
            .map(PyWarning::from)
            .collect())
    }

    /// Consult the provider's own model listing. One blocking call, no retry;
    /// providers without a machine-readable listing raise ConfigError.
    fn refresh_models(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let inner = self.inner.clone();
        py.detach(move || {
            runtime()
                .block_on(async move { inner.refresh_models().await })
                .map_err(map_error)
        })
    }

    fn __repr__(&self) -> String {
        "simllms.Client(...)".to_string()
    }
}

#[pymodule(name = "_core")]
fn simllms_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("SimLLMError", m.py().get_type::<SimLLMError>())?;
    m.add("CapabilityError", m.py().get_type::<CapabilityError>())?;
    m.add("ConfigError", m.py().get_type::<ConfigError>())?;
    m.add("APIError", m.py().get_type::<APIError>())?;
    m.add("TransportError", m.py().get_type::<TransportError>())?;
    m.add_class::<Client>()?;
    m.add("Reply", m.py().get_type::<PyReply>())?;
    m.add("Chunk", m.py().get_type::<PyChunk>())?;
    m.add_class::<PyChatStream>()?;
    m.add("GeneratedImage", m.py().get_type::<PyGeneratedImage>())?;
    m.add("Warning", m.py().get_type::<PyWarning>())?;
    Ok(())
}
