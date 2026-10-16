# 快速开始 / Getting started

中文为主，关键术语附英文。目标：装好包、发出第一次调用、并且看懂新手
最常撞上的那扇门——能力门（Gate）。

## 安装 / Install

**Rust**（crate 名 `simllms`）：

```bash
cargo add simllms
```

**Python**（包名同样是 `simllms`，PyPI；需要 Python 3.10+）：

```bash
pip install simllms
# 或 uv add simllms
```

两者是同一套领域模型的两张皮：请求形状、响应形状、错误分层完全一致。

## 第一次调用 / First call

每个提供商的 API key 走构造参数或约定环境变量
（`DEEPSEEK_API_KEY`、`MOONSHOT_API_KEY`、`DASHSCOPE_API_KEY`、
`ZHIPU_API_KEY`、`MIMO_API_KEY`、`OPENROUTER_API_KEY`）：

```python
from simllms import Client

client = Client("deepseek", "deepseek-v4-pro")          # key 从环境变量读
reply = client.chat(["用一句话介绍你自己。"])
print(reply.content)
```

```python
from simllms import AsyncClient   # 事件循环不阻塞；并发受线程池约束（ADR-0003）

async_client = AsyncClient("deepseek", "deepseek-v4-pro")
reply = await async_client.chat(["你好"])
```

Rust 等价：

```rust
use simllms::{Client, Message, Provider};

let client = Client::new(Provider::DeepSeek, "deepseek-v4-pro", std::env::var("DEEPSEEK_API_KEY")?);
let reply = client.chat(vec![Message::text("你好")]).await?;
println!("{}", reply.content);
```

## 新手最常撞上的错误：CapabilityError

simllms 的核心承诺是：**不可能的请求不发出去**。每个模型在本地目录
（Catalog）里有一条能力记录——能不能看图、能不能结构化输出、支持哪些
推理档位。请求超纲时，库在发任何网络请求之前就拒绝你：

```text
CapabilityError: model `deepseek-v4-pro` cannot serve this request:
missing capability ImageInput; capable models on this provider: deepseek-v4-flash-vision-exp
```

注意这条错误给了你两条信息：缺什么能力（`ImageInput`），以及同一家
提供商里谁能做（`suggestions`）。修法通常是把模型名换掉这一处编辑。

三种放行方式（按优先级 用户声明 > 提供商自述 > 目录）：

1. **确实该放行**：你比目录更了解模型——在客户端上声明覆盖：
   `Client(..., capability_overrides={"image_input": True})`（Python）/
   `with_capability_overrides`（Rust）。
2. **未知模型**：不在目录里的模型 id（比如网关上的新模型）是
   warn-and-pass（ADR-0005）——发出请求，同时通过 `warnings()` 预览警告。
3. **让提供商自己说**：`refresh_models()`（Python/Rust 均有）拉取提供商
   的模型列表并按优先级合并，之后模型从“未知”变成 Known。

详见 [capabilities.md](capabilities.md)。

## 下一步 / Next

- [chat.md](chat.md) — 完整对话选项
- [streaming.md](streaming.md) — 流式输出
- [structured-output.md](structured-output.md) — 结构化输出
- [image-generation.md](image-generation.md) — 图像生成
- [capabilities.md](capabilities.md) — 能力门与覆盖
- [custom-gateway.md](custom-gateway.md) — 自建/网关提供商

所有示例无凭据即可运行（本地模拟），设置对应 API key 后走真实提供商：
见 [README 的示例表](https://github.com/SidneyLYZhang/simllms#可运行的示例)。
