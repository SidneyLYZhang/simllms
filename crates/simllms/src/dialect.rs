use crate::error::{ApiError, Error};
use crate::message::{ContentBlock, Message};
use crate::options::Options;
use crate::provider::Provider;
use crate::reply::Reply;

/// The v1 dialect is OpenAI-compatible: one JSON shape, translated from our
/// domain types. Pure function — same inputs, same bytes, no I/O.
pub fn chat_body(
    _provider: Provider,
    model: &str,
    messages: &[Message],
    options: &Options,
    stream: bool,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("model".into(), serde_json::Value::String(model.to_string()));
    body.insert(
        "messages".into(),
        serde_json::Value::Array(messages.iter().map(translate_message).collect()),
    );
    if stream {
        body.insert("stream".into(), serde_json::Value::Bool(true));
        body.insert(
            "stream_options".into(),
            serde_json::json!({ "include_usage": true }),
        );
    }

    let scalar: [(&str, Option<serde_json::Value>); 9] = [
        ("temperature", options.temperature.map(|v| v.into())),
        ("top_p", options.top_p.map(|v| v.into())),
        ("top_k", options.top_k.map(|v| v.into())),
        ("max_tokens", options.max_tokens.map(|v| v.into())),
        ("stop", options.stop.clone().map(|v| v.into())),
        ("seed", options.seed.map(|v| v.into())),
        (
            "presence_penalty",
            options.presence_penalty.map(|v| v.into()),
        ),
        (
            "frequency_penalty",
            options.frequency_penalty.map(|v| v.into()),
        ),
        (
            "reasoning_effort",
            options
                .reasoning_effort
                .as_ref()
                .map(|e| e.0.clone().into()),
        ),
    ];
    for (key, value) in scalar {
        if let Some(v) = value {
            body.insert(key.into(), v);
        }
    }
    if options.reasoning_enabled.is_some() {
        body.insert(
            "reasoning".into(),
            serde_json::Value::Object(
                [(
                    "enabled".to_string(),
                    serde_json::Value::Bool(options.reasoning_enabled.unwrap()),
                )]
                .into_iter()
                .collect(),
            ),
        );
    }
    if let Some(schema) = &options.response_schema {
        // Native-only (ADR-0004): the schema reaches the provider as a
        // schema-constrained response_format, never as prompt text.
        body.insert(
            "response_format".into(),
            serde_json::json!({
                "type": "json_schema",
                "json_schema": { "name": "response", "strict": true, "schema": schema }
            }),
        );
    }
    for (k, v) in &options.extra {
        body.insert(k.clone(), v.clone());
    }

    serde_json::Value::Object(body)
}

fn translate_message(m: &Message) -> serde_json::Value {
    let role = match m.role {
        crate::message::Role::System => "system",
        crate::message::Role::User => "user",
        crate::message::Role::Assistant => "assistant",
    };
    // A single text block collapses to a plain string; anything else becomes
    // the content-array form the wire format uses.
    let content = if m.content.len() == 1 {
        match &m.content[0] {
            ContentBlock::Text(t) => serde_json::Value::String(t.clone()),
            other => serde_json::Value::Array(vec![translate_block(other)]),
        }
    } else {
        serde_json::Value::Array(m.content.iter().map(translate_block).collect())
    };
    serde_json::json!({ "role": role, "content": content })
}

fn translate_block(b: &ContentBlock) -> serde_json::Value {
    match b {
        ContentBlock::Text(t) => serde_json::json!({ "type": "text", "text": t }),
        ContentBlock::Image(crate::message::ImageSource::Url(url)) => {
            serde_json::json!({ "type": "image_url", "image_url": { "url": url } })
        }
        ContentBlock::Image(crate::message::ImageSource::Base64 { data, mime_type }) => {
            use base64::Engine as _;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            serde_json::json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{mime_type};base64,{encoded}") }
            })
        }
    }
}

/// Normalizes an OpenAI-compatible chat completion response into a Reply.
/// Never reformats provider errors — those arrive here already carrying
/// their `raw_body`.
pub fn parse_reply(body: &serde_json::Value) -> Result<Reply, Error> {
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            Error::Api(ApiError {
                status: 200,
                provider_message: "response is missing choices[0].message.content".into(),
                raw_body: body.to_string(),
            })
        })?
        .to_string();

    let usage = body.get("usage").and_then(|u| {
        Some(crate::reply::Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()),
            completion_tokens: u.get("completion_tokens").and_then(|v| v.as_u64()),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()),
        })
    });

    Ok(Reply { content, usage })
}
