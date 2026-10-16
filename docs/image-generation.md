# 图像生成 / Image generation

一次 `generate_image` 调用，归一化三种提供商线上形状：

| 提供商 | 形状 | 端点 |
| --- | --- | --- |
| Zhipu（GLM） | 同步，返回 URL | `POST /images/generations` |
| OpenRouter | 同步，返回 base64 | `POST /images` |
| DashScope | 异步：提交后轮询任务 | `POST .../image-synthesis` → `GET /tasks/{id}` |

## 结果不变式 / bytes-or-URL invariant

每张图**恰好**是 `data`（字节）或 `url`（链接）之一——你永远不需要猜
提供商这次返回了哪种。

```python
images = client.generate_image("水彩风格的中国龙", size="1024x1024")
for image in images:
    if image.url:
        print(image.url)
    else:
        open("out.png", "wb").write(image.data)
    if image.revised_prompt:
        print("提供商改写了提示词：", image.revised_prompt)
```

```rust
let reply = client.generate_image(ImageRequest::new("水彩风格的中国龙")).await?;
for image in &reply.images {
    // image.url 或 image.data 二者恰有一个
}
```

## 参数矩阵

每个提供商只支持部分参数。不支持的参数在任何网络请求之前被
`ConfigError` 拒绝，错误信息点名参数和模型：

```text
model `cogview-4` does not support the `negative_prompt` parameter
```

当前矩阵：Zhipu（prompt/size/seed）、DashScope
（prompt/negative_prompt/n/size/seed）、OpenRouter（prompt/n/size）。
参数支持不明的未知模型是 warn-and-pass（ADR-0005）。

## DashScope 轮询细节

提交返回 `task_id`，随后按 `poll_interval`（默认 1s）轮询任务状态，
超时（默认 120s）时以 `TransportError` 结束。任务失败（如触发内容审核）
以 `APIError`（status 0）返回，`raw_body` 保留提供商的原始说明。
Python 的 `AsyncClient` 在轮询期间不阻塞事件循环。
