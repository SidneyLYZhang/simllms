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
