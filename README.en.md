# SimLLMs

**One client shape for every major LLM provider — impossible requests are
refused locally, before you pay for them.**

simllms unifies DeepSeek, Moonshot (Kimi), Qwen/DashScope, Zhipu (GLM),
Xiaomi MiMo, OpenRouter, and any OpenAI-compatible gateway behind one
client: one `chat`, one `generate_image`, one streaming shape, and a
per-model statement of what that model can actually do. The capability
Gate refuses impossible requests before any network call and tells you
what is missing — and which models on the same provider can do it.

- 中文文档 / Chinese: [README.md](README.md)。
- Full docs: [docs/getting-started.md](docs/getting-started.md) (also on
  GitHub Pages once published).

```rust
let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", api_key);
let reply = client.chat(vec![Message::text("你好")]).await?;
```

```python
from simllms import Client

client = Client("deepseek", "deepseek-v4-pro", api_key)
reply = client.chat(["你好"])
```

## Runnable examples

Every example talks to a local mock service when no credentials are
present, and to the real provider when the matching API key is set.

| Topic | Rust (`crates/simllms/examples/`) | Python (`crates/simllms-python/examples/`) |
| --- | --- | --- |
| Chat | [`deepseek_chat.rs`](crates/simllms/examples/deepseek_chat.rs) | [`chat.py`](crates/simllms-python/examples/chat.py) |
| Streaming | [`chat_stream.rs`](crates/simllms/examples/chat_stream.rs) | [`chat_stream.py`](crates/simllms-python/examples/chat_stream.py) |
| Structured output | [`structured_output.rs`](crates/simllms/examples/structured_output.rs) | [`structured_output.py`](crates/simllms-python/examples/structured_output.py) |
| Image generation | [`generate_image.rs`](crates/simllms/examples/generate_image.rs) | [`generate_image.py`](crates/simllms-python/examples/generate_image.py) |
| Custom gateway | [`custom_gateway.rs`](crates/simllms/examples/custom_gateway.rs) | — |

Rust: `cargo run --example deepseek_chat` (same pattern for the rest).
Python: from `crates/simllms-python/`, run `uv run examples/chat.py`
(same pattern for the rest).

The examples cover DeepSeek, Moonshot, OpenRouter, and Zhipu; each
provider's API key is read from its environment variable
(`DEEPSEEK_API_KEY` etc.), falling back to the local mock when unset.

## Install

```bash
# Rust
cargo add simllms
```

```bash
# Python
pip install simllms   # or: uv add simllms
```

Docs: [docs/getting-started.md](docs/getting-started.md) ·
topic guides live under `docs/` (chat / streaming / structured-output /
image-generation / capabilities / custom-gateway).

## License

MIT OR Apache-2.0. The model capability snapshot comes from
[models.dev](https://models.dev) (MIT), see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
