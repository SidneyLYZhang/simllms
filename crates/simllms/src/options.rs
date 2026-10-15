use serde_json::Map;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningEffort(pub String);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub reasoning_enabled: Option<bool>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub response_schema: Option<serde_json::Value>,
    pub extra: Map<String, serde_json::Value>,
}

impl Options {
    /// Per-request values win over client-level values; `extra` merges per key.
    pub fn layered(base: Options, over: Options) -> Options {
        let mut extra = base.extra;
        for (k, v) in over.extra {
            extra.insert(k, v);
        }
        Options {
            temperature: over.temperature.or(base.temperature),
            top_p: over.top_p.or(base.top_p),
            top_k: over.top_k.or(base.top_k),
            max_tokens: over.max_tokens.or(base.max_tokens),
            stop: over.stop.or(base.stop),
            seed: over.seed.or(base.seed),
            presence_penalty: over.presence_penalty.or(base.presence_penalty),
            frequency_penalty: over.frequency_penalty.or(base.frequency_penalty),
            reasoning_enabled: over.reasoning_enabled.or(base.reasoning_enabled),
            reasoning_effort: over.reasoning_effort.or(base.reasoning_effort),
            response_schema: over.response_schema.or(base.response_schema),
            extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn override_wins_per_field() {
        let base = Options {
            temperature: Some(0.7),
            max_tokens: Some(1024),
            extra: Map::from_iter([("enable_search".to_string(), json!(true))]),
            ..Default::default()
        };
        let over = Options {
            temperature: Some(0.0),
            extra: Map::from_iter([("thinking_budget".to_string(), json!(4096))]),
            ..Default::default()
        };
        let merged = Options::layered(base, over);
        assert_eq!(merged.temperature, Some(0.0));
        assert_eq!(merged.max_tokens, Some(1024));
        assert_eq!(merged.extra.get("enable_search"), Some(&json!(true)));
        assert_eq!(merged.extra.get("thinking_budget"), Some(&json!(4096)));
    }

    #[test]
    fn default_is_empty() {
        let o = Options::default();
        assert_eq!(o, Options { extra: Map::new(), ..Default::default() });
    }
}
