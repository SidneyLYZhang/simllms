use crate::capabilities::{Capabilities, ResolvedCapabilities};
use crate::error::{CapabilityError, ConfigError, Error, MissingCapability};
use crate::message::{ContentBlock, Message};
use crate::options::Options;

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub options: Options,
    pub stream: bool,
}

impl ChatRequest {
    pub fn text(messages: Vec<Message>, options: Options) -> Self {
        ChatRequest { messages, options, stream: false }
    }

    fn has_image_input(&self) -> bool {
        self.messages.iter().any(|m: &Message| {
            m.content.iter().any(|b| matches!(b, ContentBlock::Image(_)))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Warning {
    UnknownModel { provider: String, model: String },
    DeprecatedModel { model: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GateOutcome {
    pub warnings: Vec<Warning>,
}

/// Decides locally, before any network call, whether the request is legal
/// for the model. A catalog miss warns and passes (ADR-0005); a catalog hit
/// is strict. Never performs I/O.
pub fn gate(
    resolved: &ResolvedCapabilities,
    request: &ChatRequest,
    provider: crate::provider::Provider,
    model: &str,
) -> Result<GateOutcome, Error> {
    let caps = match resolved {
        ResolvedCapabilities::Unknown => {
            return Ok(GateOutcome {
                warnings: vec![Warning::UnknownModel {
                    provider: provider.to_string(),
                    model: model.to_string(),
                }],
            });
        }
        ResolvedCapabilities::Known(caps) => caps,
    };

    let mut warnings = Vec::new();
    if caps.status == crate::capabilities::ModelStatus::Deprecated {
        warnings.push(Warning::DeprecatedModel { model: model.to_string() });
    }

    check(caps, request, model)?;

    Ok(GateOutcome { warnings })
}

fn check(caps: &Capabilities, request: &ChatRequest, model: &str) -> Result<(), Error> {
    let model_id = model.to_string();

    if request.has_image_input() && !caps.accepts_image_input() {
        return Err(Error::Capability(CapabilityError {
            model: model_id,
            suggestions: Vec::new(),
            missing: MissingCapability::ImageInput,
        }));
    }
    if request.options.response_schema.is_some() && !caps.structured_output {
        return Err(Error::Capability(CapabilityError {
            model: model_id,
            suggestions: Vec::new(),
            missing: MissingCapability::StructuredOutput,
        }));
    }
    if request.options.reasoning_enabled == Some(true) && caps.reasoning == crate::capabilities::ReasoningMode::None {
        return Err(Error::Capability(CapabilityError {
            model: model_id,
            suggestions: Vec::new(),
            missing: MissingCapability::Reasoning,
        }));
    }
    if let Some(effort) = &request.options.reasoning_effort {
        if !caps.supports_reasoning_effort(&effort.0) {
            return Err(Error::Capability(CapabilityError {
                model: model_id,
                suggestions: Vec::new(),
                missing: MissingCapability::ReasoningEffort(effort.0.clone()),
            }));
        }
    }
    if let (Some(max_tokens), Some(cap)) = (
        request.options.max_tokens.map(u64::from),
        caps.max_output_tokens,
    ) && max_tokens > cap
    {
        return Err(Error::Config(ConfigError {
            message: format!(
                "max_tokens {max_tokens} exceeds the model's max output of {cap}"
            ),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{Capabilities, ReasoningMode, ResolvedCapabilities, Source};
    use crate::message::{ContentBlock, ImageSource, Message, Role};
    use crate::options::Options;
    use crate::provider::Provider;
    use std::collections::BTreeSet;

    fn text_request() -> ChatRequest {
        ChatRequest::text(vec![Message::text("hello")], Options::default())
    }

    fn image_request() -> ChatRequest {
        ChatRequest::text(
            vec![Message::blocks(
                Role::User,
                vec![
                    ContentBlock::Text("what is this?".into()),
                    ContentBlock::Image(ImageSource::Url("https://example.com/cat.png".into())),
                ],
            )],
            Options::default(),
        )
    }

    fn vision_model() -> Capabilities {
        Capabilities {
            input_modalities: BTreeSet::from([crate::capabilities::Modality::Text, crate::capabilities::Modality::Image]),
            structured_output: true,
            tool_call: true,
            ..Capabilities::text_only(Source::Catalog)
        }
    }

    #[test]
    fn image_to_text_only_model_is_refused_locally() {
        let caps = ResolvedCapabilities::Known(Capabilities::text_only(Source::Catalog));
        let err = gate(&caps, &image_request(), Provider::DeepSeek, "deepseek-v4-pro")
            .expect_err("image input must fail the Gate");
        match err {
            Error::Capability(e) => {
                assert_eq!(e.model, "deepseek-v4-pro");
                assert_eq!(e.missing, MissingCapability::ImageInput);
            }
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn image_to_vision_model_passes() {
        let caps = ResolvedCapabilities::Known(vision_model());
        gate(&caps, &image_request(), Provider::Zhipu, "glm-4.5v")
            .expect("vision model must accept image input");
    }

    #[test]
    fn unknown_model_warns_and_passes() {
        let err = gate(
            &ResolvedCapabilities::Unknown,
            &image_request(),
            Provider::OpenRouter,
            "some/new-model",
        )
        .expect("catalog miss must warn-and-pass (ADR-0005)");
        assert_eq!(
            err.warnings,
            vec![Warning::UnknownModel {
                provider: "openrouter".into(),
                model: "some/new-model".into(),
            }]
        );
    }

    #[test]
    fn deprecated_model_warns_and_passes() {
        let mut caps = Capabilities::text_only(Source::Catalog);
        caps.status = crate::capabilities::ModelStatus::Deprecated;
        let outcome = gate(
            &ResolvedCapabilities::Known(caps),
            &text_request(),
            Provider::DeepSeek,
            "deepseek-v4-flash",
        )
        .expect("deprecated id must warn, not refuse");
        assert_eq!(
            outcome.warnings,
            vec![Warning::DeprecatedModel { model: "deepseek-v4-flash".into() }]
        );
    }

    #[test]
    fn user_override_relaxes_image_gate() {
        let base = Capabilities::text_only(Source::Catalog);
        let over = crate::capabilities::CapabilityOverrides {
            image_input: Some(true),
            ..Default::default()
        };
        let caps = ResolvedCapabilities::Known(over.apply_to(base));
        match &caps {
            ResolvedCapabilities::Known(c) => {
                assert_eq!(c.source, Source::User);
                assert!(c.accepts_image_input());
            }
            ResolvedCapabilities::Unknown => panic!("override must produce Known"),
        }
        gate(&caps, &image_request(), Provider::DeepSeek, "deepseek-v4-pro")
            .expect("user override must relax the Gate");
    }

    #[test]
    fn user_override_tightens_structured_output() {
        let base = vision_model();
        let over = crate::capabilities::CapabilityOverrides {
            structured_output: Some(false),
            ..Default::default()
        };
        let caps = ResolvedCapabilities::Known(over.apply_to(base));
        let mut request = text_request();
        request.options.response_schema = Some(serde_json::json!({"type": "object"}));
        match gate(&caps, &request, Provider::Zhipu, "glm-5.3").expect_err("tightened override must refuse") {
            Error::Capability(e) => {
                assert_eq!(e.model, "glm-5.3");
                assert_eq!(e.missing, MissingCapability::StructuredOutput);
            }
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn schema_against_non_structured_model_is_refused_locally() {
        let caps = ResolvedCapabilities::Known(Capabilities::text_only(Source::Catalog));
        let mut request = text_request();
        request.options.response_schema = Some(serde_json::json!({"type": "object"}));
        let err = gate(&caps, &request, Provider::DeepSeek, "deepseek-v4-flash")
            .expect_err("schema must fail the Gate locally");
        match err {
            Error::Capability(e) => assert_eq!(e.missing, MissingCapability::StructuredOutput),
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_reasoning_effort_is_refused() {
        let caps = ResolvedCapabilities::Known(Capabilities {
            reasoning: ReasoningMode::Effort { levels: BTreeSet::from(["low".into(), "high".into()]) },
            ..Capabilities::text_only(Source::Catalog)
        });
        let mut request = text_request();
        request.options.reasoning_effort = Some(crate::options::ReasoningEffort("max".into()));
        match gate(&caps, &request, Provider::Moonshot, "kimi-k3").expect_err("unknown effort must refuse") {
            Error::Capability(e) => {
                assert_eq!(e.missing, MissingCapability::ReasoningEffort("max".into()));
            }
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn max_tokens_over_cap_is_a_config_error() {
        let caps = ResolvedCapabilities::Known(Capabilities {
            max_output_tokens: Some(8192),
            ..Capabilities::text_only(Source::Catalog)
        });
        let mut request = text_request();
        request.options.max_tokens = Some(999_999);
        match gate(&caps, &request, Provider::DeepSeek, "deepseek-v4-pro").expect_err("over-cap must refuse") {
            Error::Config(e) => assert!(e.message.contains("8192")),
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn clean_text_request_produces_no_warnings() {
        let caps = ResolvedCapabilities::Known(Capabilities::text_only(Source::Catalog));
        let outcome = gate(&caps, &text_request(), Provider::DeepSeek, "deepseek-v4-pro")
            .expect("plain text chat must pass");
        assert!(outcome.warnings.is_empty());
    }
}
