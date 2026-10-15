#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Provider {
    DeepSeek,
    Moonshot,
    DashScope,
    Zhipu,
    MiMo,
    OpenRouter,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Provider::DeepSeek => "deepseek",
            Provider::Moonshot => "moonshot",
            Provider::DashScope => "dashscope",
            Provider::Zhipu => "zhipu",
            Provider::MiMo => "mimo",
            Provider::OpenRouter => "openrouter",
        };
        f.write_str(name)
    }
}

/// The provider's documented OpenAI-compatible root. A Client may always
/// override this with its own `base_url`.
impl Provider {
    pub fn default_base_url(self) -> &'static str {
        match self {
            Provider::DeepSeek => "https://api.deepseek.com",
            Provider::Moonshot => "https://api.moonshot.cn/v1",
            Provider::DashScope => "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Provider::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            Provider::MiMo => "https://api.xiaomimimo.com/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
        }
    }

    /// The conventional environment variable carrying this provider's api
    /// key, used by `Client::from_env`.
    pub fn env_key_name(self) -> &'static str {
        match self {
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::Moonshot => "MOONSHOT_API_KEY",
            Provider::DashScope => "DASHSCOPE_API_KEY",
            Provider::Zhipu => "ZHIPU_API_KEY",
            Provider::MiMo => "MIMO_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
        }
    }
}
