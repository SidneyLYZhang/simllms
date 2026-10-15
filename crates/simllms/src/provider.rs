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

/// DashScope's OpenAI-compatible root is region-shaped: API keys are bound
/// to the region they were issued in, and the recommended production
/// endpoint is a workspace-specific domain rather than the shared regional
/// host. Configure this on the Client instead of overriding the base URL.
///
/// Key-region binding: a key from the Beijing console only works against
/// `DashScopeRegion::Cn` / a `cn-beijing` workspace; a key from the
/// Singapore console against `Singapore` / `ap-southeast-1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashScopeRegion {
    /// Shared Beijing host: `https://dashscope.aliyuncs.com` (the default).
    Cn,
    /// Shared Singapore host: `https://dashscope-intl.aliyuncs.com`.
    Singapore,
    /// Recommended for production: `https://{id}.{region}.maas.aliyuncs.com`,
    /// e.g. `llm-xxx.cn-beijing.maas.aliyuncs.com`. The workspace id is on
    /// the 业务空间详情 page of the console.
    Workspace {
        id: String,
        /// `cn-beijing`, `ap-southeast-1`, `cn-hongkong`, `eu-central-1`,
        /// `ap-northeast-1`, or `us-east-1`.
        region: &'static str,
    },
}

pub const DASHSCOPE_CN_BEIJING: &str = "cn-beijing";
pub const DASHSCOPE_AP_SOUTHEAST_1: &str = "ap-southeast-1";
pub const DASHSCOPE_CN_HONGKONG: &str = "cn-hongkong";
pub const DASHSCOPE_EU_CENTRAL_1: &str = "eu-central-1";
pub const DASHSCOPE_AP_NORTHEAST_1: &str = "ap-northeast-1";
pub const DASHSCOPE_US_EAST_1: &str = "us-east-1";

impl DashScopeRegion {
    pub fn base_url(&self) -> String {
        match self {
            DashScopeRegion::Cn => "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            DashScopeRegion::Singapore => {
                "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into()
            }
            DashScopeRegion::Workspace { id, region } => {
                format!("https://{id}.{region}.maas.aliyuncs.com/compatible-mode/v1")
            }
        }
    }
}
