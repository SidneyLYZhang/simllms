# 对话 / Chat

一次 `chat` 把消息列表变成一次补全。这是所有提供商共享的唯一请求形状。

## 消息 / Messages

消息是 `{role, content}`：role 是 `system` / `user` / `assistant`；
content 可以是纯字符串，也可以是块列表（文本块或图像块）。

```python
from simllms import Client

client = Client("deepseek", "deepseek-v4-pro", api_key)

# 快捷形式：裸字符串按 user 消息处理
reply = client.chat(["你好"])

# 完整形式
reply = client.chat([
    {"role": "system", "content": "你是一个简洁的助手。"},
    {"role": "user", "content": [
        {"type": "text", "text": "这张图里是什么？"},
        {"type": "image_url", "url": "https://example.com/cat.png"},
    ]},
])
print(reply.content, reply.prompt_tokens)
```

Rust：

```rust
use simllms::{Client, Message, Provider};

let reply = client.chat(vec![
    Message::system("你是一个简洁的助手。"),
    Message::text("你好"),
]).await?;
```

## 选项 / Options

每次调用可传选项，与客户端级选项按字段合并（请求级优先）：
`temperature`、`top_p`、`top_k`、`max_tokens`、`stop`、`seed`、
`presence_penalty`、`frequency_penalty`、`reasoning_enabled`、
`reasoning_effort`、`response_schema`。提供商特有的参数放 `extra`
字典（Rust：`Options.extra`），按原样到达线上。

```python
reply = client.chat(["写一首诗"], options={"temperature": 0.7, "max_tokens": 512,
                                          "extra": {"enable_search": True}})
```

## 响应 / Reply

```text
Reply { content: String, usage: Option<Usage> }
Usage { prompt_tokens, completion_tokens, total_tokens }
```

提供商没报 usage 时为 `None`。提供商特有的额外字段不会进入这个形状。

## 错误 / Errors

见 [getting-started.md](getting-started.md) 的错误分层：
`CapabilityError`（本地拒绝）、`ConfigError`（配置问题）、
`APIError`（提供商返回错误，`raw_body` 原样保留）、
`TransportError`（请求没完成，可重试）。
