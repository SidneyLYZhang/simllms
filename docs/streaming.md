# 流式输出 / Streaming

`chat_stream` 把补全归一化成一个块序列：文本增量、推理增量
（reasoning delta），以及最后一个携带 finish reason 和 usage 的块。
所有提供商的 SSE 差异在核心层被抹平。

## Rust

```rust
use simllms::stream::Delta;

let mut stream = client.chat_stream(vec![Message::text("讲个故事")]).await?;
while let Some(chunk) = stream.next().await {
    let chunk = chunk?;
    match chunk.delta {
        Some(Delta::Text { text }) => print!("{text}"),
        Some(Delta::Reasoning { text }) => eprint!("{text}"),
        None => {}
    }
    if chunk.finish_reason.is_some() {
        println!("\n[finish] {}", chunk.finish_reason.unwrap());
    }
}
```

丢弃流（drop）即取消请求。超时有两个：首个事件必须在 first-token
超时内到达，后续每个事件必须在 idle 超时内到达，否则流以
`TransportError` 结束（默认 60s / 90s，可用 `with_stream_timeouts` 调整）。

## Python（同步）

```python
for chunk in client.chat_stream(["讲个故事"]):
    if chunk.text is not None:
        print(chunk.text, end="")
    if chunk.finish_reason is not None:
        print("\n[finish]", chunk.finish_reason)
```

每次 `next()` 会阻塞调用线程等待一个 SSE 事件。

## Python（异步）

```python
async for chunk in async_client.chat_stream(["讲个故事"]):
    ...
```

异步版是 async generator：每个块在工作线程上取回（ADR-0003），
事件循环在流式等待期间保持响应。

## 流中的错误

提供商在流中途发错误事件时，块以 `APIError` 形式抛出：`status` 为 0
（流中没有 HTTP 状态码），`raw_body` 保留该事件的原始 JSON 作为证据。
