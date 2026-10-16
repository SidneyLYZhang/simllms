//! `Client::refresh_models()`: consult the provider's own model listing and
//! merge it under the Source precedence user > provider > catalog.
//!
//! The parsing half is pure: same body in, same `RefreshedModel`s out. Only
//! DeepSeek, Moonshot, DashScope and OpenRouter publish machine-readable
//! listings; Zhipu and MiMo have undocumented schemas and stay catalog +
//! user-override only.

use crate::capabilities::{Capabilities, ReasoningMode, Source};
use crate::error::{ApiError, Error};

/// What the listing said about one model. Every capability field is
/// optional: presence in the listing confirms the model exists, and
/// anything the listing does not state is inherited from the catalog,
/// never guessed.
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshedModel {
    pub id: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub image_input: Option<bool>,
    pub structured_output: Option<bool>,
    pub tool_call: Option<bool>,
    pub reasoning: Option<ReasoningMode>,
}

pub fn providers_with_listing() -> &'static [crate::provider::Provider] {
    use crate::provider::Provider;
    &[
        Provider::DeepSeek,
        Provider::Moonshot,
        Provider::DashScope,
        Provider::OpenRouter,
    ]
}

/// Parses an OpenAI-compatible `GET /models` body. Entries without an `id`
/// are skipped; fields with unexpected shapes degrade to None. A body
/// without a `data` array is a schema error: `Api` with status 0 and the
/// raw body intact.
pub fn parse_listing(body: &serde_json::Value) -> Result<Vec<RefreshedModel>, Error> {
    let data = body.get("data").and_then(|d| d.as_array()).ok_or_else(|| {
        Error::Api(ApiError {
            status: 0,
            provider_message: "model listing response has no `data` array".into(),
            raw_body: body.to_string(),
        })
    })?;

    let mut models = Vec::with_capacity(data.len());
    for entry in data {
        let Some(id) = entry.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let as_u64 = |v: &serde_json::Value| v.as_u64();
        let context_window = entry.get("context_length").and_then(as_u64).or_else(|| {
            entry
                .pointer("/top_provider/context_length")
                .and_then(as_u64)
        });
        let max_output_tokens = entry
            .pointer("/top_provider/max_completion_tokens")
            .and_then(as_u64);
        // An explicit modality array is authoritative both ways: "image"
        // present means yes, absent means no.
        let image_input = entry
            .pointer("/architecture/input_modalities")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(|m| m.as_str() == Some("image")));
        let params = entry.get("supported_parameters").and_then(|v| v.as_array());
        let has = |name: &str| {
            params
                .map(|p| p.iter().any(|x| x.as_str() == Some(name)))
                .unwrap_or(false)
        };
        let structured_output = params.map(|_| has("response_format") || has("structured_outputs"));
        let tool_call = params.map(|_| has("tools") || has("tool_choice"));
        let reasoning = params.map(|_| {
            if has("reasoning") || has("reasoning_effort") {
                ReasoningMode::Toggle
            } else {
                ReasoningMode::None
            }
        });

        models.push(RefreshedModel {
            id: id.to_string(),
            context_window,
            max_output_tokens,
            image_input,
            structured_output,
            tool_call,
            reasoning,
        });
    }
    Ok(models)
}

/// Field-wise merge onto the catalog's record: refreshed fields win, absent
/// fields inherit. The result reports Source::Provider.
pub fn apply(base: Capabilities, refreshed: &RefreshedModel) -> Capabilities {
    let mut caps = Capabilities {
        source: Source::Provider,
        ..base
    };
    if refreshed.image_input == Some(true) {
        caps.input_modalities
            .insert(crate::capabilities::Modality::Image);
    }
    if refreshed.image_input == Some(false) {
        caps.input_modalities
            .remove(&crate::capabilities::Modality::Image);
    }
    if let Some(v) = refreshed.structured_output {
        caps.structured_output = v;
    }
    if let Some(v) = refreshed.tool_call {
        caps.tool_call = v;
    }
    if let Some(v) = refreshed.context_window {
        caps.context_window = Some(v);
    }
    if let Some(v) = refreshed.max_output_tokens {
        caps.max_output_tokens = Some(v);
    }
    if let Some(mode) = refreshed.reasoning.clone() {
        caps.reasoning = mode;
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openrouter_rich_entry_parses() {
        let body = json!({
            "data": [{
                "id": "vendor/model",
                "context_length": 131072,
                "architecture": { "input_modalities": ["text", "image"] },
                "supported_parameters": ["tools", "response_format", "reasoning"],
                "top_provider": { "context_length": 131072, "max_completion_tokens": 8192 }
            }]
        });
        let models = parse_listing(&body).expect("parses");
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.id, "vendor/model");
        assert_eq!(m.context_window, Some(131072));
        assert_eq!(m.max_output_tokens, Some(8192));
        assert_eq!(m.image_input, Some(true));
        assert_eq!(m.structured_output, Some(true));
        assert_eq!(m.tool_call, Some(true));
        assert_eq!(m.reasoning, Some(ReasoningMode::Toggle));
    }

    #[test]
    fn id_only_listing_degrades_to_existence() {
        let body = json!({ "object": "list", "data": [{ "id": "m1" }, { "object": "model" }] });
        let models = parse_listing(&body).expect("parses");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "m1");
        assert_eq!(models[0].context_window, None);
        assert_eq!(models[0].image_input, None);
    }

    #[test]
    fn wrong_typed_fields_degrade_not_fail() {
        let body = json!({
            "data": [{
                "id": "m1",
                "context_length": "very big",
                "architecture": { "input_modalities": "text" },
                "supported_parameters": "everything"
            }]
        });
        let models = parse_listing(&body).expect("parses");
        assert_eq!(models[0].context_window, None);
        assert_eq!(models[0].image_input, None);
        assert_eq!(models[0].structured_output, None);
    }

    #[test]
    fn missing_data_array_is_a_schema_error() {
        let body = json!({ "models": [{ "id": "m1" }] });
        let err = parse_listing(&body).expect_err("schema error");
        match &err {
            Error::Api(e) => {
                assert_eq!(e.status, 0);
                assert!(e.raw_body.contains("models"));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn explicit_text_only_modalities_tighten() {
        let body = json!({
            "data": [{
                "id": "m1",
                "architecture": { "input_modalities": ["text"] },
                "supported_parameters": ["temperature"]
            }]
        });
        let m = &parse_listing(&body).expect("parses")[0];
        assert_eq!(m.image_input, Some(false));
        assert_eq!(m.structured_output, Some(false));
        assert_eq!(m.reasoning, Some(ReasoningMode::None));
    }

    #[test]
    fn apply_wins_fieldwise_and_reports_provider_source() {
        let base = Capabilities::text_only(Source::Catalog);
        let refreshed = RefreshedModel {
            id: "m1".into(),
            context_window: Some(4096),
            max_output_tokens: None,
            image_input: Some(true),
            structured_output: None,
            tool_call: None,
            reasoning: None,
        };
        let caps = apply(base, &refreshed);
        assert_eq!(caps.source, Source::Provider);
        assert!(caps.accepts_image_input());
        assert_eq!(caps.context_window, Some(4096));
        assert!(!caps.structured_output);
    }
}
