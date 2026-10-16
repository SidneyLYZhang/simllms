# 自定义网关 / Custom gateway

`Provider::Custom` 面向任何 OpenAI 兼容端点：自建 vLLM、One-API、
公司内部网关。构造时必须给 `base_url`，否则是 `ConfigError`。

```python
from simllms import Client

client = Client("custom", "my-gateway-model", api_key,
                base_url="https://llm.internal.example.com/v1",
                options={"extra": {"X-Dept": "research"}})   # extra 原样进请求体
```

```rust
use simllms::{Auth, Client, Provider};

let client = Client::new(Provider::Custom, "my-gateway-model", "sk-...")
    .with_base_url("https://llm.internal.example.com/v1")
    .with_auth(Auth::Header {
        name: "X-Api-Key".into(),
        value: "sk-...".into(),   // 非标准鉴权头：值原样发送
    });
```

默认鉴权是 `Authorization: Bearer <key>`；网关用别的头时用 `Auth::Header`
（或 Python 侧等价物）。额外的固定请求头走客户端级 `default_headers`。

## 网关模型与能力门（ADR-0005）

网关上的模型 id 不会出现在 simllms 目录里，所以它们是 warn-and-pass：
请求照发、警告可预览。要让门认真把关，两条路：

1. **声明覆盖**：`capability_overrides={"image_input": False, ...}`——
   之后违规请求在本地被拒。
2. **提供商列表**：`Custom` 没有机读列表约定，`refresh_models()` 会明确
   报错（绝不猜测）；目录 + 覆盖是仅有的两个能力来源。

## 请求体检 / debug_request

上线前想看确切发出的请求体（**不含凭据**）：

```python
print(client.debug_request(["你好"], options={"max_tokens": 32}))
```

完整示例见 [`custom_gateway.rs`](https://github.com/SidneyLYZhang/simllms/blob/main/crates/simllms/examples/custom_gateway.rs)。
