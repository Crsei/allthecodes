# cc-rust 简介（当前阶段：全量构建 / Full Build）

cc-rust 是 Claude Code 的 Rust 后端实现，提供命令式对话引擎与工具系统，支撑 UI/CLI 等前端的交互需求。它与 TypeScript 版本的 Claude Code 相互独立，但在架构与 API 思路上保持一致。

> **阶段声明**：本项目历史上自称 "Rust Lite"，定位为 TypeScript 版本的精简版。现已进入**全量构建阶段**，目标是与上游完整版行为对齐，不再以"精简"为默认语境。新工作默认按上游完整实现对齐，历史登记的缩减/延期条目视为待补齐 TODO。详细规则见 [`CLAUDE.md`](CLAUDE.md) 顶部。

> 重要说明：本项目的目标是学习、研究与防御性用例，严格遵守相关安全与伦理规范。请勿用于未授权的破坏性操作或对外部系统的大规模攻击。本文档仅用于介绍实现和使用，并非攻击手段。

---

## 设计目标与定位

- 提供一个可扩展的后端核心，用于处理自然语言对话、推理流程与工具执行。
- 支持插件化的工具系统，允许内置工具和自定义工具的无缝接入。
- 实现高效的对话循环（QueryEngine + Generator 风格的流式处理），降低内存占用并提升吞吐。
- 与前端 UI 及外部接口（CLI/Remote）通过清晰的 IPC 约束进行通信，保证解耦与容错。
- 数据与配置分离，支持多种持久化策略与路径隔离，确保不同工作区的安全边界。

---

## 架构总览

cc-rust 的核心架构围绕一个流式的查询引擎和一个可扩展的工具系统展开。主要模块分布如下（位于 rust/ 目录下）：

- engine：QueryEngine 与生命周期管理
- query：异步流式查询循环、预算与调度逻辑
- tools：工具实现与工具注册/发现机制
- skills：技能系统（内置技能 + 用户自定义技能）
- compact：上下文压缩管道，帮助记忆管理与長对话压缩
- commands：斜杠命令实现与路由
- api：多后端 API 客户端（Anthropic、OpenAI、Google 等的抽象封装）
- auth：认证与授权相关逻辑
- permissions：工具权限模型，支持默认、自动和豁免等模式
- config：配置管理、环境变量与本地设置
- session：会话持久化与历史管理
- ipc/headless：与前端/UI 的进程间通信协议与头部模式实现
- utils：辅助函数与通用工具

> 参考结构：rust/（包含 engine、query、tools、skills、compact、commands、api、auth、permissions、config、session、ipc、utils 等子模块）

---

## 关键数据流与生命周期（高层描述）

1) 输入入口
- 来自前端 UI、CLI 或远程客户端的消息通过 IPC 层进入后端。消息格式遵循统一的 JSON 结构，包含会话上下文、指令和参数。

2) 消息解析与路由
- IPC 层将输入分发到 QueryEngine，QueryEngine 维护全局对话状态、预算、权限与上下文的元数据。

3) QueryEngine 与 Generator 循环
- QueryEngine 向 generator 风格的循环提交查询，持续产出中间结果与最终回复。该循环实现高效的 token 预算管理、记忆记录、策略选择与容错处理。

4) 工具执行与策略决策
- 当对话需要外部能力时，工具系统会根据 ToolPermissionContext 进行权限检查，再由 Tool 实现执行具体任务（如文本处理、文件操作、网络请求等）。工具执行可并行化、具备阶段性回传能力，便于 UI 展现进度。

5) 上下文管理与压缩
- compact 管道对对话上下文进行压缩、剪裁与回溯点提取，确保长期对话在有限内存中保持可用性，同时尽量保留关键信息用于后续推理。

6) 输出格式化与返回
- 完整回复经过格式化阶段，支持多种输出格式（纯文本、JSON、表格等），并将结果通过 IPC 回传前端。

7) 会话持久化与恢复
- session 模块提供会话级别的持久化能力，允许在重新启动后快速恢复历史与状态。

---

## 模块详解

- engine（引擎与生命周期）
  - QueryEngine 负责统一的对话生命周期：消息接收、状态更新、工具协作、记忆与回退策略的协调。实现了 Phase A/B/I 风格的阶段化生命周期，确保启动、运行、关闭等阶段的可观测性。
  - 主要入口在 engine/lifecycle/ 中，包含 submit_message、deps、helpers 等子模块。

- query（流式查询）
  - 异步流式循环实现，确保在大对话中也能边推理边输出结果，降低单次请求的峰值压力。
  - 通过 token_budget、memory_budget、policy_engine 等机制实现对资源的有效分配。

- tools（工具系统）
  - 提供 28+ 种基础工具（如尖端文本处理、文件读取/写入、正则匹配、网络请求等），并支持自定义插件扩展。
  - Tool 是核心抽象，包含 ToolInputJSONSchema、call()、isEnabled()、权限等字段。
  - 工具注册与发现托管在 tools.rs/ToolRegistry 中，方便前端对可用能力进行展示与选择。

- skills（技能系统）
  - 内置技能集成（simplify、remember、updateConfig 等），并提供机制接入用户自定义技能。
  - 技能与工具的协作，支持记忆、策略推断以及对复杂任务的分解。

- compact（上下文压缩）
  - 将对话历史和上下文以高效的编码方式进行压缩，降低长期会话的内存占用，同时保持可用性以支撑后续推理。

- commands（斜杠命令）
  - 实现了多条斜杠命令，用于调试、配置、状态查看及其他开发辅助功能。

- api（API 客户端抽象）
  - 封装对 Anthropic、OpenAI、Google 等多家服务提供商的调用，内部实现了重试、限流、错误处理与流式接口。
  - 支持切换不同提供商以实现容错与性能优化。

- auth（认证）
  - 提供 API Key/Token 的集中管理与解析，支持本地 Keychain 的加密存储及安全获取流程。

- permissions（权限模型）
  - 针对工具与能力的使用设置三种模式：默认、自动、绕过。按工具粒度控制能力的访问，提升安全性。

- config（配置管理）
  - 集中管理本地设定、环境变量覆盖、MDM 与自定义配置信息。
  - 提供清晰的加载、合并和验证流程，确保在不同环境下行为可预测。

- session（会话持久化）
  - 会话数据、对话历史与状态信息的持久化存储，方便跨-session 的上下文恢复。

- ipc/headless（IPC 协议与头部模式）
  - 与 UI/前端的通信协议，定义了事件、消息格式和协作流程。headless 模式支持无界面部署，适合自动化测试与服务化场景。

- utils（工具函数）
  - 提供日志、错误处理、辅助类型、序列化/反序列化等底层能力，支撑其他模块的稳定性。

---

## 构建与运行（本地开发指南）

前提条件
- Rust 工具链：请先安装 rustup，并确保 rustc、cargo 版本在稳定分支。
- 对于 UI/前端部分：需要 Bun（包含 bun install 与 bun run）。UI 子项目位于 ui/ 目录。
- Windows 路径隔离：本仓库强调路径隔离，遵循在不同工作区下的持久化目录。

构建步骤（后端）
1) 进入 Rust 目录：
   cd F:\AIclassmanager\cc\rust
2) 构建发布版本：
   cargo build --release
3) 运行后端：
   cargo run --release
   注：在默认配置下，后端会监听与前端通信的 IPC 通道，请确保前端相应配置已就绪。

构建步骤（前端 UI）
1) 进入 UI 目录：
   cd F:\AIclassmanager\cc\rust\../ui
2) 安装依赖与构建：
   bun install
   bun run dev
3) 运行前端（开发模式）通常直接执行 run dev 脚本，默认会与后端进程进行对接，可通过环境变量指定后端地址。

运行注意
- 数据路径隔离：后端会在用户主目录创建以 cc-rust 名称开头的目录，例如 ~/.cc-rust/，用于日志、缓存、以及会话数据。
- 配置优先级：环境变量 > 本地配置 > 代码中默认值。请确保在生产环境中对敏感信息进行妥善管理。
- 日志级别：默认输出到控制台，生产环境建议配置为更高等级的日志收集策略。

示例配置路径（示意）
- 会话数据：~/.cc-rust/sessions/
- 日志：~/.cc-rust/logs/
- 记忆缓存：~/.cc-rust/cache/

### Storage paths

cc-rust writes all runtime data (sessions, logs, credentials, ...) under a
single data root — `$CC_RUST_HOME` if set, otherwise `~/.cc-rust/`. See
[docs/STORAGE.md](docs/STORAGE.md) for the complete layout.

---

## 流式对话与安全注意

- cc-rust 采用流式查询循环，能够在推理过程中逐步输出中间结果，以提升交互体验与可观测性。若对话涉及对外部系统的操作，系统会进行权限检查与审计日志记录。
- 为避免潜在的安全隐患，后端严格遵循工具权限模型，尽量将高风险操作进行最小化的授权，并提供撤销/回滚机制。
- 与外部服务的交互请确保 API Key 与 Token 的安全存储，优先使用本地加密存储或受信任的 Keychain 服务。

---

## 开发与扩展指南

如何添加一个新工具
1) 在 rust/src/tools/ 下实现一个 Tool，遵循 Tool trait 的定义，包含 name、description、inputJSONSchema、call() 等成员。
2) 将工具注册到工具注册中心（通常在 tools.rs 或 registry 模块中完成），确保 isEnabled() 返回正确的运行时开关。
3) 测试新工具的输入输出，确保在权限模型下行为正确。

如何新增技能
1) 在 rust/src/skills/ 下实现一个 Skill，定义入口、参数、以及与工具的交互逻辑。
2) 将技能注册到技能加载器，以便 query 引擎在对话中调用。

如何扩展 API 提供商
1) 在 rust/src/api/ 下实现对新服务提供商的封装，遵循现有 Client 设计模式。
2) 集成到 config 中的 provider 切换逻辑，确保在运行时可选切换。

已内置的第三方云 Claude 接入（AWS Bedrock / GCP Vertex AI）使用方法及已知限制，参见
[`docs/cloud-providers.md`](docs/cloud-providers.md)。启用示例：

```bash
# AWS Bedrock
export CLAUDE_CODE_USE_BEDROCK=1
export AWS_REGION=us-east-1
export AWS_BEARER_TOKEN_BEDROCK=...   # or AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY

# GCP Vertex AI
export CLAUDE_CODE_USE_VERTEX=1
export ANTHROPIC_VERTEX_PROJECT_ID=my-project
export CLOUD_ML_REGION=us-east5
gcloud auth application-default login
```

测试策略
- 单元测试：为核心组件（QueryEngine、Tool、Skill、Permissions 等）编写单元测试。
- 集成测试：通过模拟前端输入，验证整个对话流的正确性与边界条件。
- 性能测试：对流式输出、内存压缩和工具并发执行进行基准测试。

代码风格与安全
- 遵循 Rust 社区常用的代码风格，避免不安全代码的滥用。对外部输入进行严格校验，避免注入和越权操作。
- 对所有可能涉及网络请求的工具，注意超时、重试与断路策略。

---

## 版本控制与贡献

- 本项目遵循分支策略，特性开发在功能分支进行，完成后提交合并至主分支前要通过 PR 审核。
- 提交前请确保本地通过 cargo test，确保变更不会破坏现有功能。
- 贡献前请阅读 CLAUDE.md 的相关内容，遵循项目的代码风格与审阅流程。

---

## 常见问题（FAQ）

Q1：如何在没有前端 UI 的情况下使用后端？
A：运行 cargo run --release 即可。后端提供 IPC 通道，理论上可与任意前端实现对接；你可以实现一个简单的前端代理来发送 JSON 消息。

Q2：如何查看历史对话与状态？
A：通过 session 模块的持久化机制，历史会被写入本地存储。请查看 ~/.cc-rust/sessions/ 目录。

Q3：遇到版本不兼容怎么办？
A：首先拉取最新子模块与依赖，执行 cargo update；如问题仍存在，请在 Issue 中提供具体的 Cargo.lock 与错误日志。

---

## 附：关键文件与路径引用
- rust/engine/lifecycle/：QueryEngine 的生命周期实现，核心入口与状态机定义，参见 rust/src/engine/lifecycle/mod.rs。
- rust/query/：流式查询循环及辅助工具，参见 rust/src/query/mod.rs 与 rust/src/query/loop_impl.rs。
- rust/tools/：工具系统及具体实现，参见 rust/src/tools/tooltip.rs（示例工具）与 rust/src/tools/mod.rs。
- rust/skills/：技能系统实现，参见 rust/src/skills/mod.rs。
- rust/compact/：上下文压缩管道，参见 rust/src/compact/mod.rs。
- rust/commands/：斜杠命令实现，参见 rust/src/commands/mod.rs。
- rust/api/：多提供商 API 客户端，参见 rust/src/api/client.rs 与 rust/src/api/provider.rs。
- rust/auth/：认证实现，参见 rust/src/auth/mod.rs。
- rust/permissions/：权限模型，参见 rust/src/permissions/mod.rs。
- rust/config/：配置管理，参见 rust/src/config/mod.rs。
- rust/session/：会话持久化，参见 rust/src/session/mod.rs。
- rust/ipc/headless.rs：IPC 与前端通信头部模式实现，参见 rust/src/ipc/headless.rs。
- rust/utils/：工具函数集，参见 rust/src/utils/*。

> 注：文件和模块名称以实际实现为准，文中目录仅作示意性描述。

---

如果你需要，我可以把这份介绍扩展成“用户文档版 README”、“开发者手册版 API 文档”或生成一个可导航的文档站点草案（如 Markdown 转 HTML 的静态站点）以便在线浏览。也可以把内容拆分成多份文档，放在 docs/ 目录下，以便不同读者快速定位。