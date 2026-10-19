use crate::capabilities::{Capabilities, ResolvedCapabilities, Source};
use crate::catalog_data;
use crate::provider::Provider;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The builtin catalog, built once and shared: every `Client` resolves
/// against this table unless told otherwise, and so do the module-level
/// query entry points.
pub fn shared() -> &'static Catalog {
    static SHARED: OnceLock<Catalog> = OnceLock::new();
    SHARED.get_or_init(Catalog::builtin)
}

/// The built-in table of Models and their Capabilities, generated from the
/// vendored models.dev snapshot plus hand-maintained image-generation
/// overrides.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: HashMap<(Provider, String), Capabilities>,
}

impl Catalog {
    pub fn resolve(&self, provider: Provider, model: &str) -> ResolvedCapabilities {
        match self.entries.get(&(provider, model.to_string())) {
            Some(caps) => ResolvedCapabilities::Known(caps.clone()),
            None => ResolvedCapabilities::Unknown,
        }
    }

    pub fn insert(&mut self, provider: Provider, model: impl Into<String>, caps: Capabilities) {
        self.entries.insert((provider, model.into()), caps);
    }

    /// Ids of this provider's models satisfying a Capability predicate,
    /// sorted — used to make a refusal actionable ("use one of these").
    pub fn models_with(
        &self,
        provider: Provider,
        pred: impl Fn(&Capabilities) -> bool,
    ) -> Vec<String> {
        let mut ids: Vec<String> = self
            .entries
            .iter()
            .filter(|((p, _), caps)| *p == provider && pred(caps))
            .map(|((_, id), _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    pub fn snapshot_commit(&self) -> &'static str {
        catalog_data::SNAPSHOT_COMMIT
    }

    /// The catalog a Client uses unless told otherwise: codegen'd models.dev
    /// entries overlaid with the hand-maintained image-generation models.
    pub fn builtin() -> Self {
        let mut cat = Catalog::default();
        for (provider, model, caps) in catalog_data::entries() {
            cat.insert(provider, model, caps);
        }
        for (provider, model, caps) in image_generation_overrides() {
            cat.insert(provider, model, caps);
        }
        cat
    }
}

/// Hand-maintained: models.dev is chat-only here and no provider's
/// /v1/models covers text-to-image, so these entries are curated by hand
/// and deliberately minimal — image generation only, no chat Capabilities
/// claimed. Correctness of the ids is owned by the image-generation work.
fn image_generation_overrides() -> Vec<(Provider, &'static str, Capabilities)> {
    vec![
        (
            Provider::Zhipu,
            "cogview-4",
            Capabilities {
                image_generation: true,
                ..Capabilities::text_only(Source::Catalog)
            },
        ),
        (
            Provider::DashScope,
            "wanx2.1-t2i-turbo",
            Capabilities {
                image_generation: true,
                ..Capabilities::text_only(Source::Catalog)
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Modality;
    use crate::error::{Error, MissingCapability};
    use crate::gate::{self, ChatRequest, Warning};
    use crate::message::{ContentBlock, ImageSource, Message, Role};
    use crate::options::Options;

    fn builtin() -> Catalog {
        Catalog::builtin()
    }

    fn image_request() -> ChatRequest {
        ChatRequest {
            messages: vec![Message::blocks(
                Role::User,
                vec![
                    ContentBlock::Text("what is this?".into()),
                    ContentBlock::Image(ImageSource::Url("https://example.com/cat.png".into())),
                ],
            )],
            options: Options::default(),
            stream: false,
        }
    }

    fn text_request() -> ChatRequest {
        ChatRequest {
            messages: vec![Message::text("hello")],
            options: Options::default(),
            stream: false,
        }
    }

    #[test]
    fn real_entries_resolve_with_catalog_source() {
        let cat = builtin();
        match cat.resolve(Provider::DeepSeek, "deepseek-v4-pro") {
            ResolvedCapabilities::Known(c) => {
                assert_eq!(c.source, Source::Catalog);
                assert!(c.structured_output);
                assert!(c.context_window.is_some());
            }
            ResolvedCapabilities::Unknown => panic!("deepseek-v4-pro must be in the snapshot"),
        }
    }

    #[test]
    fn gate_refuses_images_for_text_only_models_on_real_entries() {
        let cat = builtin();
        let resolved = cat.resolve(Provider::DeepSeek, "deepseek-v4-pro");
        let err = gate::gate(
            &resolved,
            &image_request(),
            Provider::DeepSeek,
            "deepseek-v4-pro",
        )
        .expect_err("deepseek-v4-pro is text-only");
        match err {
            Error::Capability(e) => assert_eq!(e.missing, MissingCapability::ImageInput),
            other => panic!("expected Capability error, got {other:?}"),
        }
    }

    #[test]
    fn gate_passes_images_for_vision_models_on_real_entries() {
        let cat = builtin();
        let resolved = cat.resolve(Provider::Moonshot, "kimi-k3");
        let outcome = gate::gate(&resolved, &image_request(), Provider::Moonshot, "kimi-k3")
            .expect("kimi-k3 accepts image input");
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn gate_warns_on_real_deprecated_ids() {
        let cat = builtin();
        // deepseek-v4-flash is marked deprecated in the snapshot.
        let resolved = cat.resolve(Provider::DeepSeek, "deepseek-v4-flash");
        let outcome = gate::gate(
            &resolved,
            &text_request(),
            Provider::DeepSeek,
            "deepseek-v4-flash",
        )
        .expect("deprecated warns, never refuses");
        assert_eq!(
            outcome.warnings,
            vec![Warning::DeprecatedModel {
                model: "deepseek-v4-flash".into()
            }]
        );
    }

    #[test]
    fn unknown_ids_still_warn_and_pass() {
        let cat = builtin();
        let resolved = cat.resolve(Provider::OpenRouter, "some/new-model");
        let outcome = gate::gate(
            &resolved,
            &image_request(),
            Provider::OpenRouter,
            "some/new-model",
        )
        .expect("catalog miss must warn-and-pass");
        assert!(matches!(
            outcome.warnings.as_slice(),
            [Warning::UnknownModel { .. }]
        ));
    }

    #[test]
    fn image_generation_models_are_hand_maintained() {
        let cat = builtin();
        match cat.resolve(Provider::Zhipu, "cogview-4") {
            ResolvedCapabilities::Known(c) => {
                assert!(c.image_generation);
                assert!(!c.input_modalities.contains(&Modality::Image));
            }
            ResolvedCapabilities::Unknown => panic!("hand-maintained override must be present"),
        }
    }
}
