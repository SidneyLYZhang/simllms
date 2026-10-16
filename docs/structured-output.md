# 结构化输出 / Structured output

把 JSON Schema 交给 `response_schema` 选项，回答以受约束的 JSON 返回。

## 只走原生通道（ADR-0004）

Schema 以 `response_format: json_schema`（`strict: true`）到达提供商——
**从不**拼进提示词。这是被测试钉死的约定：如果某模型不支持原生结构化
输出，simllms 拒绝而不是降级模拟，因为提示词模拟出的“JSON”不可信。

## 用法

```python
schema = {
    "type": "object",
    "properties": {"capital": {"type": "string"}},
    "required": ["capital"],
    "additionalProperties": False,
}
reply = client.chat(["法国的首都是哪座城市？"], options={"response_schema": schema})
data = json.loads(reply.content)
```

```rust
let reply = client.chat_with(
    vec![Message::text("法国的首都是哪座城市？")],
    Options { response_schema: Some(schema), ..Default::default() },
).await?;
```

## 拒绝是本地的、可行动的

模型不支持时，在任何网络请求之前抛 `CapabilityError`
（`missing: StructuredOutput`），并且当目录知道同提供商有能干的模型时，
`suggestions` 会把它们列出来——修法是一处编辑。

## JSON 内容的容错

有的提供商把 JSON 包在 ```json 代码围栏里；解析 `reply.content` 时
建议先剥围栏（示例 `structured_output.py` 直接 `json.loads`，配合
`strict: true` 的提供商通常返回裸 JSON）。
