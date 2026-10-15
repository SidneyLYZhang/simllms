#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// The normalized answer to a Chat: the assistant's text plus the Usage the
/// provider reported. Provider-specific extras stay out of this shape.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    pub content: String,
    pub usage: Option<Usage>,
}
