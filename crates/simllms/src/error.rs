/// What the Gate found missing. Naming the missing Capability is the point:
/// the fix should be one edit away.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MissingCapability {
    ImageInput,
    StructuredOutput,
    Reasoning,
    ReasoningEffort(String),
    ImageGeneration,
    ToolCall,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub struct CapabilityError {
    pub model: String,
    pub missing: MissingCapability,
    /// When the Catalog knows capable models on the same provider, they are
    /// named here — the fix should be one edit away.
    pub suggestions: Vec<String>,
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "model `{}` cannot serve this request: missing capability {:?}",
            self.model, self.missing
        )?;
        if !self.suggestions.is_empty() {
            write!(f, "; capable models on this provider: {}", self.suggestions.join(", "))?;
        }
        Ok(())
    }
}

impl CapabilityError {
    pub fn without_suggestions(model: impl Into<String>, missing: MissingCapability) -> Self {
        CapabilityError { model: model.into(), missing, suggestions: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
#[non_exhaustive]
pub struct ConfigError {
    pub message: String,
}

/// Carries the provider's answer untouched: `raw_body` is the evidence you
/// take to their support, never a reformatting of it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("provider returned {status}: {provider_message}")]
#[non_exhaustive]
pub struct ApiError {
    pub status: u16,
    pub provider_message: String,
    pub raw_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
#[non_exhaustive]
pub struct TransportError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("capability gate refused: {0}")]
    Capability(#[from] CapabilityError),
    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),
    #[error("provider api error: {0}")]
    Api(#[from] ApiError),
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
}

impl Error {
    /// Capability and Config are local: they never reach the network and are
    /// never retried. Api is retryable only for 429/5xx — a 4xx is the
    /// provider saying no, permanently. Transport failures may be retried.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api(e) => e.status == 429 || e.status >= 500,
            Error::Transport(_) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16) -> Error {
        Error::Api(ApiError { status, provider_message: String::new(), raw_body: String::new() })
    }

    #[test]
    fn retryability_follows_the_status() {
        assert!(api(429).is_retryable());
        assert!(api(500).is_retryable());
        assert!(api(503).is_retryable());
        assert!(!api(400).is_retryable());
        assert!(!api(401).is_retryable());
        assert!(!api(404).is_retryable());
        assert!(Error::Transport(TransportError { message: "x".into() }).is_retryable());
        assert!(!Error::Capability(CapabilityError {
            model: "m".into(),
            missing: MissingCapability::ImageInput,
            suggestions: Vec::new(),
        })
        .is_retryable());
    }
}
