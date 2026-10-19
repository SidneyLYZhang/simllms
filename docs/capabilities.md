# 能力门 / Capabilities

simllms 的差异化承诺：**不可能的请求在本地被拒绝**，而不是撞上提供商
的 400 才发现。每个 (提供商, 模型 id) 有一条能力记录：

```text
input_modalities      能读什么（文本/图像）
structured_output     能否原生 JSON Schema 输出
tool_call             能否工具调用
image_generation      能否生成图像
reasoning             None / Toggle / Effort{levels}
context_window        上下文长度
max_output_tokens     最大输出
status                Active / Deprecated
```

## 三层数据来源（ADR-0002 / ADR-0005）

优先级从高到低：**用户声明 > 提供商自述 > 目录**。

1. **目录（Catalog）**：随包内置的 models.dev 过滤快照（515 条），
   由 `cargo xtask refresh-catalog` 离线生成、每周自动提 PR 更新——
   目录变更永远经人审，绝不静默改变行为。
2. **提供商自述（Provider）**：显式调用 `refresh_models()` 拉取
   DeepSeek / Moonshot / DashScope / OpenRouter 的模型列表并按字段合并
   （列表确认存在、明说的字段覆盖目录、没说的继承目录）。Zhipu / MiMo
   没有机读列表，仅目录 + 用户覆盖。
3. **用户声明（User）**：`capability_overrides` 覆盖任意字段，你比目录
   更了解自己的网关时使用；覆盖后的记录报告 `Source::User`。

## 未知模型：warn-and-pass

不在任何来源里的模型 id **不会被拒绝**（ADR-0005）——请求照发，同时
产生警告。这保证新模型上线第一天就能用：

```rust
client.warnings(&messages, &options);   // 预览警告，不发请求
client.refresh_models().await?;         // 一次显式调用后，未知 → Known（Source::Provider）
```

```python
client.warnings(messages, options)   # 预览警告，不发请求
client.refresh_models()              # 一次显式调用后，未知 → Known（Source::Provider）
```

两侧入口一致；`refresh_models()` 只在 DeepSeek、Moonshot、DashScope、
OpenRouter 可用——其余提供商没有机器可读的模型列表，会得到 ConfigError，
目录保持权威。

## 覆盖示例

```python
client = Client("deepseek", "gateway-model", api_key,
                capability_overrides={"image_input": True,
                                      "context_window": 131072})
```

```rust
let client = Client::new(Provider::DeepSeek, "gateway-model", api_key)
    .with_capability_overrides(CapabilityOverrides {
        image_input: Some(true),
        ..Default::default()
    });
```

覆盖既能放宽也能收紧；收紧后的拒绝同样发生在本地。

## 拒绝的样子

```text
CapabilityError: model `deepseek-v4-pro` cannot serve this request:
missing capability StructuredOutput; capable models on this provider: deepseek-v4-flash
```

错误点名模型、缺失能力，以及（当目录知道时）同提供商的替代模型。
Python 侧对应异常属性：`model` / `missing` / `suggestions`。
