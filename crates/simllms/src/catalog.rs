use crate::capabilities::{Capabilities, ResolvedCapabilities, Source};
use crate::provider::Provider;
use std::collections::HashMap;

/// The built-in table of Models and their Capabilities. This ticket ships a
/// hand-written placeholder; the vendored models.dev snapshot and codegen
/// replace it without changing this interface.
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

    /// Hand-written entries standing in for the real snapshot. Values are
    /// conservative and exist to make Gate behaviour testable.
    pub fn placeholder() -> Self {
        let mut cat = Catalog::default();
        use crate::capabilities::{Modality, ModelStatus, ReasoningMode};
        use std::collections::BTreeSet;

        let mut deepseek_pro = Capabilities::text_only(Source::Catalog);
        deepseek_pro.structured_output = true;
        deepseek_pro.tool_call = true;
        deepseek_pro.reasoning = ReasoningMode::Toggle;
        deepseek_pro.max_output_tokens = Some(8192);
        cat.insert(Provider::DeepSeek, "deepseek-v4-pro", deepseek_pro);

        let deepseek_chat = Capabilities::text_only(Source::Catalog);
        cat.insert(Provider::DeepSeek, "deepseek-chat", Capabilities {
            status: ModelStatus::Deprecated,
            ..deepseek_chat
        });

        let mut kimi = Capabilities::text_only(Source::Catalog);
        kimi.input_modalities = BTreeSet::from([Modality::Text, Modality::Image]);
        kimi.reasoning = ReasoningMode::Effort {
            levels: BTreeSet::from(["low".into(), "high".into(), "max".into()]),
        };
        cat.insert(Provider::Moonshot, "kimi-k3", kimi);

        let mut glm = Capabilities::text_only(Source::Catalog);
        glm.structured_output = true;
        glm.reasoning = ReasoningMode::Toggle;
        cat.insert(Provider::Zhipu, "glm-5.3", glm);

        cat
    }
}
