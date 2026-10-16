# SimLLMs

**一个客户端形状，所有主流大模型提供商；不可能的请求在你花钱之前就被本地拒绝。**

simllms 把 DeepSeek、Moonshot（Kimi）、Qwen/DashScope、智谱（GLM）、小米 MiMo、
OpenRouter 以及任意 OpenAI 兼容网关统一成一个客户端：一次 `chat`，一次 `generate_image`，
一套流式输出，每个模型声明自己真正能做到的事——能力门（Gate）在发出任何网络请求之前，
就把不可能的请求拒绝掉，并告诉你缺什么、换哪个模型。

- 中文为主，英文对照见下方 [English](#english)。
- 完整文档：[docs/getting-started.md](docs/getting-started.md)（发布后见 GitHub Pages）。

```rust
let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", api_key);
let reply = client.chat(vec![Message::text("你好")]).await?;
```

```python
from simllms import Client

client = Client("deepseek", "deepseek-v4-pro", api_key)
reply = client.chat(["你好"])
```

## English

**One client shape for every major LLM provider — impossible requests are
refused locally, before you pay for them.**

simllms unifies DeepSeek, Moonshot (Kimi), Qwen/DashScope, Zhipu (GLM),
Xiaomi MiMo, OpenRouter, and any OpenAI-compatible gateway behind one
client: one `chat`, one `generate_image`, one streaming shape, and a
per-model statement of what that model can actually do. The capability
Gate refuses impossible requests before any network call and tells you
what is missing — and which models on the same provider can do it.

## 可运行的示例 / Runnable examples

每个示例无凭据时走本地模拟服务，设置了对应 API key 时走真实提供商。

| 主题 Topic | Rust (`crates/simllms/examples/`) | Python (`crates/simllms-python/examples/`) |
| --- | --- | --- |
| 对话 Chat | [`deepseek_chat.rs`](crates/simllms/examples/deepseek_chat.rs) | [`chat.py`](crates/simllms-python/examples/chat.py) |
| 流式 Streaming | [`chat_stream.rs`](crates/simllms/examples/chat_stream.rs) | [`chat_stream.py`](crates/simllms-python/examples/chat_stream.py) |
| 结构化输出 Structured | [`structured_output.rs`](crates/simllms/examples/structured_output.rs) | [`structured_output.py`](crates/simllms-python/examples/structured_output.py) |
| 图像生成 Image | [`generate_image.rs`](crates/simllms/examples/generate_image.rs) | [`generate_image.py`](crates/simllms-python/examples/generate_image.py) |
| 自定义网关 Custom gateway | [`custom_gateway.rs`](crates/simllms/examples/custom_gateway.rs) | — |

Rust：`cargo run --example deepseek_chat`（其余同理）。
Python：在 `crates/simllms-python/` 下 `uv run examples/chat.py`（其余同理）。

示例覆盖 DeepSeek、Moonshot、OpenRouter、Zhipu 四家提供商；每家的 API key
通过对应环境变量（`DEEPSEEK_API_KEY` 等）提供，未设置时自动落到本地模拟。

## 安装 / Install

```bash
# Rust
cargo add simllms
```

```bash
# Python
pip install simllms   # or: uv add simllms
```

文档：[docs/getting-started.md](docs/getting-started.md) ·
主题指南在 `docs/` 下（chat / streaming / structured-output /
image-generation / capabilities / custom-gateway）。

## 许可 / License

MIT OR Apache-2.0。模型能力快照来自 [models.dev](https://models.dev)（MIT），
见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
