use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Catalog,
    Provider,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Modality {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningMode {
    None,
    Toggle,
    Effort { levels: BTreeSet<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    Active,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Capabilities {
    pub source: Source,
    pub input_modalities: BTreeSet<Modality>,
    pub structured_output: bool,
    pub tool_call: bool,
    pub image_generation: bool,
    pub reasoning: ReasoningMode,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub status: ModelStatus,
}

impl Capabilities {
    pub fn text_only(source: Source) -> Self {
        Capabilities {
            source,
            input_modalities: BTreeSet::from([Modality::Text]),
            structured_output: false,
            tool_call: false,
            image_generation: false,
            reasoning: ReasoningMode::None,
            context_window: None,
            max_output_tokens: None,
            status: ModelStatus::Active,
        }
    }

    pub fn accepts_image_input(&self) -> bool {
        self.input_modalities.contains(&Modality::Image)
    }

    pub fn supports_reasoning_effort(&self, effort: &str) -> bool {
        match &self.reasoning {
            ReasoningMode::Effort { levels } => levels.contains(effort),
            _ => false,
        }
    }
}

/// Field-level overrides declared on a Client. Whatever is `Some` outranks
/// the catalog and provider data, and the resulting record reports
/// `Source::User` (ADR-0005: the user can tighten or relax the Gate).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CapabilityOverrides {
    pub image_input: Option<bool>,
    pub structured_output: Option<bool>,
    pub tool_call: Option<bool>,
    pub image_generation: Option<bool>,
    pub reasoning: Option<ReasoningMode>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

impl CapabilityOverrides {
    pub fn apply_to(&self, base: Capabilities) -> Capabilities {
        let mut caps = Capabilities {
            source: Source::User,
            ..base
        };
        if self.image_input == Some(true) {
            caps.input_modalities.insert(Modality::Image);
        }
        if self.image_input == Some(false) {
            caps.input_modalities.remove(&Modality::Image);
        }
        if let Some(v) = self.structured_output {
            caps.structured_output = v;
        }
        if let Some(v) = self.tool_call {
            caps.tool_call = v;
        }
        if let Some(v) = self.image_generation {
            caps.image_generation = v;
        }
        if let Some(v) = self.reasoning.clone() {
            caps.reasoning = v;
        }
        if let Some(v) = self.context_window {
            caps.context_window = Some(v);
        }
        if let Some(v) = self.max_output_tokens {
            caps.max_output_tokens = Some(v);
        }
        caps
    }
}

/// What the Catalog knows about a (Provider, Model Id) pair. `Unknown` means
/// warn-and-pass (ADR-0005); a user override would have produced `Known`.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedCapabilities {
    Known(Capabilities),
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_relaxes_image_input() {
        let base = Capabilities::text_only(Source::Catalog);
        let over = CapabilityOverrides { image_input: Some(true), ..Default::default() };
        let caps = over.apply_to(base);
        assert_eq!(caps.source, Source::User);
        assert!(caps.accepts_image_input());
    }

    #[test]
    fn override_tightens_structured_output() {
        let base = Capabilities { structured_output: true, ..Capabilities::text_only(Source::Catalog) };
        let over = CapabilityOverrides { structured_output: Some(false), ..Default::default() };
        let caps = over.apply_to(base);
        assert!(!caps.structured_output);
    }

    #[test]
    fn effort_membership() {
        let caps = Capabilities {
            reasoning: ReasoningMode::Effort { levels: BTreeSet::from(["low".into(), "high".into()]) },
            ..Capabilities::text_only(Source::Catalog)
        };
        assert!(caps.supports_reasoning_effort("low"));
        assert!(!caps.supports_reasoning_effort("max"));
    }
}
