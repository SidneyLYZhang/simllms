# Roadmap

更新日期：2026-10-16。对应 spec：[issue #1](https://github.com/SidneyLYZhang/simllms/issues/1)。v1 核心已落地（chat / chat_stream / structured output / generate_image / 能力 Gate / Catalog / Python 双客户端），本文件承接 spec「Out of Scope」中明确推迟的事项，并结合当前代码状态与新需求排定优先级。

## 现状梳理

### 已有能力

| 能力 | 状态 | 位置 |
|---|---|---|
| Provider（6 家 + Custom） | 完成 | `crates/simllms/src/provider.rs` |
| chat / chat_stream / debug_request | 完成 | `crates/simllms/src/client.rs`、`stream.rs` |
| Structured output（native-only，本地 Gate） | 完成（ADR-0004） | `options.rs`、`gate.rs` |
| 图像生成（三种 provider 形态统一） | 完成 | `image.rs` |
| 能力 Catalog（models.dev 快照 + 每周自动 bump PR） | 完成（ADR-0002） | `capabilities.rs`、`catalog_data.rs`、`xtask` |
| Python：同步 `Client` + `AsyncClient` | 完成（ADR-0003） | `crates/simllms-python/`、`python/simllms/` |
| CI（测试 + 目录刷新检查 + mkdocs 站点） | 完成 | `.github/workflows/` |

### 代码整理发现的优化点

1. ~~**版本号三处硬编码**~~ **已解决（#18）**：根 workspace `Cargo.toml` 的 `[workspace.package] version` 是唯一权威源（词汇表：**Authoritative Version**），派生副本只剩 Python 绑定 crate 的 manifest 一处——ADR-0001 把它留在根 workspace 之外，继承不到；`pyproject.toml` 改 `dynamic`，`__version__` 由编译期注入。`cargo xtask sync-version --check` 已在 CI。#19 又新增一批派生副本（各包目录自带的 README / 两份 License / THIRD-PARTY-NOTICES / CHANGELOG，因为 cargo 与 maturin 都不允许 `include` 指向包目录之上），由同一族的 `sync-package` / `check-package` 守住字节一致。
2. **Python 侧配置面缺口（实测，2026-10-16）**：`capability_overrides` 在 Python **一直可用**（绑定层会解析并在未知键上报 `TypeError`），spec user story 9 并不缺失。真正未绑定的是核心 Client 的 8 个配置入口——自定义 `Auth` 头、DashScope 区域、Zhipu 国际端点、default headers、重试策略、流式超时、`refresh_models()`、以及未知 Model Id 的结构化**警告**可见性（ADR-0005）——对应 user story 13/24/26/27 与 8 的警告部分；另 `Catalog` / `Capabilities` / `Capability Source` 的只读视图与类型化覆盖也缺失（覆盖参数目前是裸 dict，静态检查看不到拼写与值类型错误）。issue #12 标题所称的 "Python parity" 范围比标题窄。
3. **方言层缝隙未使用**：`dialect.rs` 目前是单一 OpenAI 兼容翻译，`_provider` 参数空置。这是为非 OpenAI 方言（Qwen 原生 DashScope、Zhipu 原生异步端点）预留的缝，暂不需要动，但新增能力（tool、file、audio）时应继续只在此层分叉。
4. **测试缝窄**：spec 约定 dialect 翻译作为纯函数用 mockito 做线上层测试；后续每个新能力（tool loop、file 上传、audio）都应先补这层测试再实现。
5. 文档缺口：tool calling、embedding、发布流程均无文档页（随对应功能补齐）。

## 路线图

### P0 — 发布与工程基础（需求 1）

**R1. tag 触发自动发布（GitHub Actions）**
- 触发：push tag `vX.Y.Z`。
- crates.io：`cargo publish -p simllms`，token 走 GitHub Secrets（或 Trusted Publishing 一旦 crates.io 支持）。
- PyPI：maturin 构建 sdist + wheel 矩阵（abi3-py310；Linux/macOS/Windows），PyPI Trusted Publishing（OIDC，无需长期 token）。
- 发布前跑一遍完整 CI（测试 + `xtask refresh-catalog --check`）作为门禁。
- 同步补上 `CHANGELOG.md` 检查与 release notes 生成。
- **依赖项**：先完成 R2 的版本单一来源，否则 tag 与三个包版本会对不上。

**R2. 版本单一来源**
- 以 workspace `Cargo.toml` 为唯一版本源（词汇表称 **Authoritative Version**）；派生副本只剩一处（Python 绑定 crate 的 manifest，因 ADR-0001 它在根 workspace 之外，无法继承），`pyproject.toml` 改用 `dynamic` 由构建后端读取，`__version__` 改为编译期注入并由纯 Python 层再导出。`xtask` 增加 `sync-version` / `--check`，发布路径永不改写仓库内容。
- Python 的能力面与 parity 缺口**不在 P0**：见 [#22](https://github.com/SidneyLYZhang/simllms/issues/22) 与整理发现点 2，走 0.2.0。P0 只负责在文档里把这批缺口如实说出来。

**票与缝（已发布，2026-10-16）**：spec [#16](https://github.com/SidneyLYZhang/simllms/issues/16) 之下五张垂直票，原生阻塞边已接——[#17](https://github.com/SidneyLYZhang/simllms/issues/17) 文档说出 Rust-only 清单、[#18](https://github.com/SidneyLYZhang/simllms/issues/18) 版本单一来源（17/18 可并行）→ [#19](https://github.com/SidneyLYZhang/simllms/issues/19) 产物自述 → [#20](https://github.com/SidneyLYZhang/simllms/issues/20) 全链 dry-run → [#21](https://github.com/SidneyLYZhang/simllms/issues/21) 真发布 + runbook + ADR-0006/0007。三个测试缝：**S1 发布产物**（唯一主缝，dry-run/真发布/本地共用）、**S2 xtask 的纯函数 `--check` 家族**、**S3 配置自述一致性**（挡「改了 environment 名 → 发布 403」与文档漂移）。现有两缝（Gate 合法性、方言翻译）不动。

**P0 进度（2026-10-16）**：#17 / #18 / #19 已落地。#19 给两份 manifest 各加了显式 `include` 白名单，Python 侧换成 SPDX 表达式 `MIT OR Apache-2.0` + `license-files`（PEP 639，wheel 里落在 `*.dist-info/licenses/`），描述保持一句 provider 无关的话，关键词带上各家 provider 名与 `openai-compatible`；registry 页面渲染的是 `README.en.md`，仓库默认 README（中文）不动。同一件事由两条缝分别证明：S2 的 `cargo xtask check-package` 读 manifest，S1 的 `tests/test_package_inventory.py` 拆真实的 `.crate` / wheel / sdist 看里面到底有什么。注意一个平台硬约束：crates.io 只收 5 个关键词，所以 crate 侧放 `llm` + `openai-compatible` + 三家 provider，PyPI 侧放全 12 个，`check-package` 要求前者是后者的子集。下一张是 #20。

**版本语义**：新增公共 API 走 minor（0.2.0），patch 只留给修复；`1.0.0` 的门槛绑 P1（tool calling 落地 = 请求/响应形状不再变）。

### P1 — Tool calling 与快速判断模块（需求 2、3 前半）

**R3. Tool calling 与多轮 tool loop（spec v1.1 计划项）**
- 类型：`Tool`（name/description/parameters）、`ContentBlock::ToolCall` / `ToolResult`，`ContentBlock` 枚举本就为扩展而写。
- API：`chat` 接受 tools；提供 `tool_loop`（或由调用方手写循环、库只负责单跳）——按「先单跳、循环随后」的顺序拆两个 PR。
- Gate：`Capabilities.tool_call` 元数据已在 Catalog 中（`capabilities.rs`、`refresh.rs` 映射已就绪），只需接上本地拒绝。
- 方言：仅 OpenAI 兼容层改动；dialect 纯函数测试先行。

**R4. 快速判断类模型模块（需求 2）**
- 目标场景：分类、评审、打分这类「一次调用、短输出、高并发」的小模型用法（judge/eval 风格）。
- 形态：与 `chat` / `generate_image` 并列的独立入口（如 `judge` / `classify`），底层强制 structured output + 低 max_tokens 默认值 + 独立超时档位（复用 spec 用户故事 27 的分操作超时机制）。
- 这是小而独立的特性，放在 P1 是因为它不依赖 tool calling、工作量小、且是 structured output 能力的直接复用。

### P2 — 文件上传、MCP、Embeddings（需求 3 后半、4）

**R5. 文件上传（需求 3 前半）**
- 各家 `/v1/files` 语义不一（Moonshot 有完整文件 API，DashScope/Zhipu 各有形制）；先支持 OpenAI 兼容文件端点的公共子集，其余 Gate 拒绝。
- 新增 `ContentBlock::File` 或文件引用块，与 R3 的 tool 协议解耦。
- 优先级低于 tool calling：多模态输入当前仅图像可满足主要用例。

**R6. MCP 支持（需求 3 后半）**
- 依赖 R3：库内置 MCP client，从 MCP server 拉取 tool 定义转成 `Tool` 注入 tool loop。
- 仅 Rust 侧实现、Python 经绑定复用；不引入 tokio 之外的运行时依赖（MCP 的 stdio/SSE transport 需评估对 reqwest/tokio 栈的兼容）。

**R7. Embeddings（需求 4）**
- 独立能力维度（spec Out of Scope 里的判断依然成立：不同于 chat 的能力轴），新增 `Capabilities.embedding` 与 `embed` 入口、`Embedding` 返回类型。
- Catalog 侧扩展 embedding 模型元数据（bge 系列、Qwen embedding 等）；`refresh-catalog` 相应扩展。
- 重排（rerank）模型与 embedding 同轴，排在 embedding 之后评估。

### P3 — 语音与多模态扩展（需求 5）

**R8. TTS / ASR（需求 5）**
- Catalog 已收录 `qwen3-asr-flash`、`gpt-audio` 等音频模型元数据（`catalog_data.rs`），具备元数据基础。
- 新增 `generate_speech` / `transcribe` 入口与 `Capabilities` 维度；流式 ASR 单列（SSE 形态与 chat_stream 不同，涉及增量转写语义）。
- 排在最后：spec 判断「不同模型类、不同能力维度、无当前用例」，除非语音场景先落地，否则优先级最低。

**R9.（远期，spec 预留）**
- `ContentBlock::Audio` / `Video`（枚举已留位）。
- Qwen 原生 DashScope 方言、Zhipu 原生异步端点（OpenAI 兼容路径被 Gate 证明不够用时才启动，见 spec「三条正交轴」决策）。

## 优先级判断依据

1. **发布自动化（R1/R2）排最前**：代码已可用但拿不到包，一切外部使用都被卡住；且它成本低、独立、是其余所有工作的「可交付」前提。
2. **Tool calling（R3）次之**：它是 spec 明文的 v1.1 计划项，Catalog 元数据已就绪，且是 MCP（R6）的前置依赖——先做它使后续两条线（file、MCP）不阻塞。
3. **快速判断模块（R4）搭 P1 顺风车**：独立、小、复用既有 structured output 与超时机制。
4. **File / MCP / Embeddings（R5–R7）进 P2**：有价值但非阻塞，且都依赖 P0/P1 建立的模式（能力维度扩展、dialect 扩展、测试缝）。
5. **语音（R8）殿后**：无当前用例驱动，元数据已备好，随用例成熟再启动。

## 与 spec 的对应关系

| 本路线图 | spec Out of Scope 原文 |
|---|---|
| R1 | Wheel build/publish matrix and crates.io/PyPI release automation（Q29 step two, its own spec） |
| R3 | Tool/function calling and the multi-turn tool loop（planned v1.1） |
| R7 | Embeddings… different model classes, different capability dimensions |
| R8 | TTS, ASR；R9 对应 Audio and video Content Blocks |
| R2 | Rust and Python package versions move in lockstep, single version source |
