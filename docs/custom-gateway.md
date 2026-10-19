# 自定义网关 / Custom gateway

`Provider::Custom` 面向任何 OpenAI 兼容端点：自建 vLLM、One-API、
公司内部网关。构造时必须给 `base_url`，否则是 `ConfigError`。

```python
from simllms import Auth, Client

client = Client("custom", "my-gateway-model", api_key,
                base_url="https://llm.internal.example.com/v1",
                auth=Auth.header("X-Api-Key", "sk-..."),  # 非标准鉴权头：值原样发送
                default_headers={"X-Dept": "research"})   # 固定请求头
                # options={"extra": {...}} 仍可用：extra 原样进请求体
```

```rust
use simllms::{Auth, Client, Provider};

let client = Client::new(Provider::Custom, "my-gateway-model", "sk-...")
    .with_base_url("https://llm.internal.example.com/v1")
    .with_auth(Auth::Header {
        name: "X-Api-Key".into(),
        value: "sk-...".into(),   // 非标准鉴权头：值原样发送
    })
    .with_default_header("X-Dept", "research");
```

默认鉴权是 `Authorization: Bearer <key>`；网关用别的头时用
`Auth.header(...)`（Python）/ `Auth::Header`（Rust）。额外的固定请求头走
客户端级 `default_headers`（Python 关键字参数）/ `with_default_header`（Rust）。

## 网关模型与能力门（ADR-0005）

网关上的模型 id 不会出现在 simllms 目录里，所以它们是 warn-and-pass：
请求照发（两种语言都能先用 `warnings()` 预览那条警告）。要让门认真把关，两条路：

1. **声明覆盖**：`capability_overrides={"image_input": False, ...}`——
   之后违规请求在本地被拒。
2. **提供商列表**：`Custom` 没有机读列表约定，`refresh_models()`（两种
   语言都有）会明确报错（绝不猜测）；目录 + 覆盖是仅有的两个能力来源。

## 请求体检 / debug_request

上线前想看确切发出的请求体（**不含凭据**）：

```python
print(client.debug_request(["你好"], options={"max_tokens": 32}))
```

完整示例见 [`custom_gateway.rs`](https://github.com/SidneyLYZhang/simllms/blob/main/crates/simllms/examples/custom_gateway.rs)。
