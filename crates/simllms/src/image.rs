use crate::error::{CapabilityError, ConfigError, Error, MissingCapability};
use crate::message::ImageSource;
use crate::provider::Provider;

/// One text-to-image ask. The parameter set is the union across supported
/// providers; each provider maps what it knows and the Gate refuses
/// unsupported combinations locally, naming the parameter and the model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImageRequest {
    pub prompt: String,
    /// Provider-specific pixel size, e.g. "1024x1024".
    pub size: Option<String>,
    /// e.g. "1:1". No supported provider maps this yet; populated, it is
    /// refused with the parameter named.
    pub aspect_ratio: Option<String>,
    pub n: Option<u32>,
    pub negative_prompt: Option<String>,
    pub seed: Option<i64>,
    /// e.g. "standard" / "hd".
    pub quality: Option<String>,
    /// e.g. "png" / "jpeg" / "webp".
    pub output_format: Option<String>,
    /// e.g. "transparent" / "opaque" / "auto".
    pub background: Option<String>,
    /// Image-to-image references. Not mapped by any v1 provider; populated,
    /// they are refused with the parameter named.
    pub input_references: Vec<ImageSource>,
}

impl ImageRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        ImageRequest {
            prompt: prompt.into(),
            ..Default::default()
        }
    }
}

/// One generated image. Invariant: at most one of `data` / `url` is
/// populated — the shape the provider returned, normalized.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GeneratedImage {
    pub data: Option<Vec<u8>>,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    /// The provider's revised prompt, where offered.
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImageReply {
    pub images: Vec<GeneratedImage>,
}

/// Tunables for `generate_image`: the DashScope submit-then-poll lifecycle
/// is fully internal, bounded by `timeout` and paced by `poll_interval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageKnobs {
    pub timeout: std::time::Duration,
    pub poll_interval: std::time::Duration,
}

impl Default for ImageKnobs {
    fn default() -> Self {
        ImageKnobs {
            timeout: std::time::Duration::from_secs(120),
            poll_interval: std::time::Duration::from_secs(1),
        }
    }
}

/// Which parameters a provider maps. Everything else is refused locally
/// with the parameter named — honest gating beats silent dropping.
fn supports(provider: Provider, parameter: &str) -> bool {
    match provider {
        Provider::Zhipu => matches!(parameter, "prompt" | "size" | "seed"),
        Provider::DashScope => {
            matches!(
                parameter,
                "prompt" | "negative_prompt" | "n" | "size" | "seed"
            )
        }
        Provider::OpenRouter => matches!(parameter, "prompt" | "n" | "size"),
        _ => false,
    }
}

/// Refuses locally when the request populates a parameter the provider
/// does not map, or when the provider offers no image generation at all.
pub fn check(
    image_generation: bool,
    provider: Provider,
    model: &str,
    request: &ImageRequest,
) -> Result<(), Error> {
    if !image_generation {
        return Err(Error::Capability(CapabilityError::without_suggestions(
            model,
            MissingCapability::ImageGeneration,
        )));
    }
    let populated: Vec<(&str, bool)> = [
        ("prompt", request.prompt.is_empty()),
        ("size", request.size.is_none()),
        ("aspect_ratio", request.aspect_ratio.is_none()),
        ("n", request.n.is_none()),
        ("negative_prompt", request.negative_prompt.is_none()),
        ("seed", request.seed.is_none()),
        ("quality", request.quality.is_none()),
        ("output_format", request.output_format.is_none()),
        ("background", request.background.is_none()),
        ("input_references", request.input_references.is_empty()),
    ]
    .into_iter()
    .filter(|(_, absent)| !absent)
    .map(|(name, _)| (name, true))
    .collect();

    for (parameter, _) in populated {
        if !supports(provider, parameter) {
            return Err(Error::Config(ConfigError {
                message: format!("model `{model}` does not support the `{parameter}` parameter"),
            }));
        }
    }
    Ok(())
}

/// The request body per provider's wire shape. Pure function.
pub fn translate(provider: Provider, model: &str, request: &ImageRequest) -> serde_json::Value {
    match provider {
        Provider::DashScope => {
            let mut input = serde_json::Map::new();
            input.insert("prompt".into(), serde_json::json!(request.prompt));
            if let Some(np) = &request.negative_prompt {
                input.insert("negative_prompt".into(), serde_json::json!(np));
            }
            let mut parameters = serde_json::Map::new();
            if let Some(n) = request.n {
                parameters.insert("n".into(), serde_json::json!(n));
            }
            if let Some(size) = &request.size {
                parameters.insert("size".into(), serde_json::json!(size));
            }
            if let Some(seed) = request.seed {
                parameters.insert("seed".into(), serde_json::json!(seed));
            }
            serde_json::json!({
                "model": model,
                "input": serde_json::Value::Object(input),
                "parameters": serde_json::Value::Object(parameters),
            })
        }
        _ => {
            let mut body = serde_json::Map::new();
            body.insert("model".into(), serde_json::json!(model));
            body.insert("prompt".into(), serde_json::json!(request.prompt));
            if let Some(n) = request.n {
                body.insert("n".into(), serde_json::json!(n));
            }
            if let Some(size) = &request.size {
                body.insert("size".into(), serde_json::json!(size));
            }
            if let Some(seed) = request.seed {
                body.insert("seed".into(), serde_json::json!(seed));
            }
            serde_json::Value::Object(body)
        }
    }
}

/// Normalizes a synchronous image response (Zhipu URL shape, OpenRouter
/// base64 shape) into GeneratedImages.
pub fn parse_sync(provider: Provider, body: &serde_json::Value) -> Result<ImageReply, Error> {
    let items = body.get("data").and_then(|d| d.as_array()).ok_or_else(|| {
        Error::Api(crate::error::ApiError {
            status: 200,
            provider_message: "image response has no `data` array".into(),
            raw_body: body.to_string(),
        })
    })?;

    let mut images = Vec::new();
    for item in items {
        let b64 = item.get("b64_json").and_then(|b| b.as_str());
        let url = item.get("url").and_then(|u| u.as_str());
        let image = match (b64, url) {
            (Some(b64), _) => GeneratedImage {
                data: Some(
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                        .map_err(|e| {
                            Error::Api(crate::error::ApiError {
                                status: 200,
                                provider_message: format!("undecodable b64_json: {e}"),
                                raw_body: body.to_string(),
                            })
                        })?,
                ),
                mime_type: match provider {
                    Provider::OpenRouter => Some("image/png".into()),
                    _ => None,
                },
                revised_prompt: item
                    .get("revised_prompt")
                    .and_then(|r| r.as_str())
                    .map(str::to_string),
                ..Default::default()
            },
            (_, Some(url)) => GeneratedImage {
                url: Some(url.to_string()),
                revised_prompt: item
                    .get("revised_prompt")
                    .and_then(|r| r.as_str())
                    .map(str::to_string),
                ..Default::default()
            },
            _ => {
                return Err(Error::Api(crate::error::ApiError {
                    status: 200,
                    provider_message: "image entry has neither b64_json nor url".into(),
                    raw_body: body.to_string(),
                }));
            }
        };
        debug_assert!(
            image.data.is_none() || image.url.is_none(),
            "bytes-or-URL invariant"
        );
        images.push(image);
    }
    Ok(ImageReply { images })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_provider_refuses_locally() {
        for provider in [Provider::DeepSeek, Provider::Moonshot, Provider::MiMo] {
            let err = check(
                false,
                provider,
                "deepseek-v4-pro",
                &ImageRequest::new("a cat"),
            )
            .expect_err("must refuse locally");
            match err {
                Error::Capability(e) => assert_eq!(e.missing, MissingCapability::ImageGeneration),
                other => panic!("expected Capability, got {other:?}"),
            }
        }
    }

    #[test]
    fn unsupported_parameter_names_parameter_and_model() {
        let req = ImageRequest {
            negative_prompt: Some("rain".into()),
            ..ImageRequest::new("a cat")
        };
        let err = check(true, Provider::Zhipu, "cogview-4", &req).expect_err("must refuse");
        match err {
            Error::Config(e) => {
                assert!(e.message.contains("negative_prompt"), "got: {}", e.message);
                assert!(e.message.contains("cogview-4"), "got: {}", e.message);
            }
            other => panic!("expected Config, got {other:?}"),
        }
    }

    #[test]
    fn supported_combinations_pass() {
        let req = ImageRequest {
            size: Some("1024x1024".into()),
            ..ImageRequest::new("a cat")
        };
        assert!(check(true, Provider::Zhipu, "cogview-4", &req).is_ok());
        assert!(check(true, Provider::OpenRouter, "x/y", &req).is_ok());
    }

    #[test]
    fn translates_dashscope_nested_shape() {
        let req = ImageRequest {
            n: Some(2),
            size: Some("1024x1024".into()),
            negative_prompt: Some("blur".into()),
            ..ImageRequest::new("a cat")
        };
        let body = translate(Provider::DashScope, "wanx2.1-t2i-turbo", &req);
        assert_eq!(
            body.pointer("/input/prompt"),
            Some(&serde_json::json!("a cat"))
        );
        assert_eq!(
            body.pointer("/input/negative_prompt"),
            Some(&serde_json::json!("blur"))
        );
        assert_eq!(body.pointer("/parameters/n"), Some(&serde_json::json!(2)));
        assert_eq!(
            body.pointer("/parameters/size"),
            Some(&serde_json::json!("1024x1024"))
        );
    }

    #[test]
    fn parses_url_and_base64_shapes_with_invariant() {
        let zhipu = parse_sync(
            Provider::Zhipu,
            &serde_json::json!({
                "data": [{ "url": "https://img.example/a.png" }]
            }),
        )
        .unwrap();
        assert_eq!(
            zhipu.images[0].url.as_deref(),
            Some("https://img.example/a.png")
        );
        assert!(zhipu.images[0].data.is_none());

        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"\x89PNG");
        let or = parse_sync(
            Provider::OpenRouter,
            &serde_json::json!({
                "data": [{ "b64_json": b64, "revised_prompt": "a big cat" }]
            }),
        )
        .unwrap();
        assert_eq!(or.images[0].data.as_deref(), Some(&b"\x89PNG"[..]));
        assert!(or.images[0].url.is_none());
        assert_eq!(or.images[0].revised_prompt.as_deref(), Some("a big cat"));
    }
}
