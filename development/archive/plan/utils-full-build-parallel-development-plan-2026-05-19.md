# Utils Full Build 并行开发计划

日期：2026-05-19

适用仓库：`claude-code-rust`

源缺口文档：

- `docs/utils/plugins-marketplace.md`
- `docs/utils/settings-mdm.md`
- `docs/utils/suggestions-input.md`
- `docs/utils/telemetry-observability.md`

目标阶段：Full Build。实现时按上游完整版行为对齐，不再用 Lite 缩减作为边界。

## 1. 目标

这四组缺口在功能上可以并行推进，但有几个共享集成点容易重复施工：plugin telemetry、plugin-only policy、动态 slash command、skill usage、input submit pipeline、`main.rs` 启动 wiring、IPC protocol。本文把它们拆成可并行的 owner lanes，并把共享文件收束到串行集成阶段。

核心目标：

- 每条并行 lane 拥有清晰写入范围，避免多个开发者同时改同一批共享文件。
- 先建立通用 contract，再让 plugin、input、telemetry 等领域消费 contract。
- 对仍未闭合的 runtime path 给出统一集成窗口，而不是由各 lane 零散修改启动顺序。
- 每个 lane 都能独立验证到 crate 层，最终再做 workspace 级验证。

## 2. 重复点与归属

| 重复点 | 涉及文档 | 唯一 owner | 消费方 |
|--------|----------|------------|--------|
| 插件遥测、安装统计、推荐提示 | plugins-marketplace, telemetry-observability | `cc-observability` / `cc-services::telemetry` 定义通用事件、span、exporter | `cc-plugins` 只发 plugin event，不自建 exporter |
| plugin-only policy、blocklist、allowlist | plugins-marketplace, settings-mdm | `cc-config` 解析 policy，`cc-permissions` 做诊断/规则语义 | `cc-plugins` 执行 policy 决策 |
| 动态 plugin command / skill slash command | plugins-marketplace, suggestions-input | `cc-commands` 负责 command registry contract | TUI/headless suggestions 只消费 registry |
| user-invocable skill 使用排序 | suggestions-input, plugins-marketplace | `cc-skills` 负责 usage schema、持久化、评分 | `cc-commands` 和 UI command palette 消费排序分 |
| LSP 插件推荐和安装动作 | plugins-marketplace, suggestions-input | `cc-lsp-service` 负责推荐判定，`cc-plugins` 负责安装/启用插件 | UI 只展示和触发动作 |
| submit/input pipeline tracing | suggestions-input, telemetry-observability | `cc-engine` 输入管线提供 hook/span 插桩点 | telemetry lane 注册事件，不拥有输入业务逻辑 |
| 启动 wiring / IPC | 四者都可能触碰 | 串行 Integration Lane | 所有 lane 在 contract 稳定后接入 |

## 3. 并行 Lane 切分

### Lane A：Settings / Policy / MDM 基础层

Owner：`cc-config`、`cc-permissions`、settings 诊断命令。

写入范围：

- 新增 `crates/cc-config/src/mdm/mod.rs`
- 新增 `crates/cc-config/src/mdm/constants.rs`
- 新增 `crates/cc-config/src/mdm/raw_read.rs`
- 新增 `crates/cc-config/src/mdm/settings.rs`
- 新增 `crates/cc-config/src/change_detector.rs`
- 新增 `crates/cc-config/src/internal_writes.rs`
- 新增 `crates/cc-config/src/permission_validation.rs`
- 新增 `crates/cc-config/src/validation_tips.rs`
- 修改 `crates/cc-config/src/lib.rs`
- 修改 `crates/cc-config/src/settings.rs`
- 修改 `crates/cc-config/src/validation.rs`
- 修改 `crates/cc-commands/src/doctor.rs`
- 修改 `crates/cc-commands/src/permissions_cmd.rs`
- 只在必要时修改 `crates/start-up/src/runtime_config.rs`

明确不负责：

- 不实现 plugin marketplace、安装器、插件 loader。
- 不实现 telemetry exporter。
- 不直接修改 TUI command palette。

交付物：

- MDM/platform raw-read 模块和 managed policy 规范化入口。
- settings permission validation 等价层，能被 `/doctor` 和 `/permissions` 调用。
- shadowed rule 诊断接入用户可见命令。
- change detector/internal write token 的最小 contract。

验证：

- `cargo test -p cc-config`
- `cargo test -p cc-permissions`
- `cargo test -p cc-commands doctor`
- `cargo test -p cc-commands permissions`

### Lane B：Observability / Telemetry 基础层

Owner：`cc-observability`、`cc-services::telemetry`、startup logging。

写入范围：

- 新增 `crates/cc-services/src/telemetry/mod.rs`
- 新增 `crates/cc-services/src/telemetry/instrumentation.rs`
- 新增 `crates/cc-services/src/telemetry/session_tracing.rs`
- 新增 `crates/cc-services/src/telemetry/privacy.rs`
- 新增 `crates/cc-services/src/telemetry/bigquery_exporter.rs`（后置，非 P0）
- 新增 `crates/cc-observability/src/session_tracing.rs`
- 新增 `crates/cc-observability/src/perfetto.rs`（后置，非 P0）
- 修改 `crates/cc-observability/src/lib.rs`
- 修改 `crates/cc-observability/src/event.rs`
- 修改 `crates/cc-observability/src/sink.rs`
- 修改 `crates/cc-services/src/lib.rs`
- 修改 `crates/cc-services/src/langfuse/mod.rs`
- 修改 `crates/cc-services/src/langfuse/client.rs`
- 修改 `crates/cc-services/src/langfuse/tracing.rs`
- 修改 `crates/cc-services/src/langfuse/convert.rs`
- 修改 `crates/cc-services/src/langfuse/sanitize.rs`
- 修改 `crates/cc-services/src/langfuse/stub.rs`
- 修改 `crates/start-up/src/logging.rs`
- 修改 `crates/cc-engine/Cargo.toml`
- 修改 `crates/cc-services/Cargo.toml`
- 修改 `crates/claude-code-rs/Cargo.toml`

明确不负责：

- 不改变插件安装/启用流程。
- 不改变输入解析语义。
- 不直接修改 plugin command registry。

交付物：

- `cc-services` 与 `cc-engine` telemetry feature 闭合，避免 engine call sites 默认走 stub。
- Interaction / model / tool / hook span API contract。
- plugin telemetry 可消费的通用 event schema。
- 已定义但未写入的 audit event 分批接入计划。

验证：

- `cargo test -p cc-observability`
- `cargo test -p cc-services --features telemetry`
- `cargo check -p claude-code-rs --features telemetry`

### Lane C：Command / Skill Registry Contract

Owner：`cc-commands`、`cc-skills`。

写入范围：

- 新增 `crates/cc-commands/src/plugin_commands.rs`
- 新增 `crates/cc-commands/src/dynamic_registry.rs` 或等价 registry 模块
- 修改 `crates/cc-commands/src/lib.rs`
- 修改 `crates/cc-commands/src/skills_cmd.rs`
- 新增 `crates/cc-skills/src/usage.rs`
- 修改 `crates/cc-skills/src/lib.rs`
- 修改 `crates/cc-skills/src/invocation.rs`
- 修改 `crates/cc-engine/src/skill_tool.rs`，仅记录 usage，不改 submit pipeline

明确不负责：

- 不扫描 marketplace，也不安装插件。
- 不做 TUI input ghost suffix。
- 不改 `crates/claude-code-rs/src/ui/command_palette/filter.rs`、`crates/claude-code-rs/src/ui/command_palette/mod.rs`、`crates/claude-code-rs/src/ui/command_palette/render.rs`，只提供可消费的 registry/metadata。

交付物：

- 动态 command registry contract，支持 builtin、user/project、plugin、skill 来源。
- user-invocable skill 动态 `/skill-name` fallback contract。
- skill usage schema、debounce、7 天半衰期评分 API。
- registry metadata 包含 source、hidden、alias、usage score、execution strategy。

验证：

- `cargo test -p cc-commands`
- `cargo test -p cc-skills`
- 针对 `parse_command_input_in()` 和 dynamic fallback 增加单元测试。

### Lane D：Plugin Marketplace / Installer / Component Loader

Owner：`cc-plugins` 和插件命令。

写入范围：

- 新增 `crates/cc-plugins/src/marketplace.rs`
- 新增 `crates/cc-plugins/src/installation.rs`
- 新增 `crates/cc-plugins/src/sources.rs`
- 新增 `crates/cc-plugins/src/zip_cache.rs`
- 新增 `crates/cc-plugins/src/mcpb.rs`
- 新增 `crates/cc-plugins/src/commands.rs`
- 新增 `crates/cc-plugins/src/agents.rs`
- 新增 `crates/cc-plugins/src/hooks.rs`
- 新增 `crates/cc-plugins/src/output_styles.rs`
- 新增 `crates/cc-plugins/src/validation.rs`
- 新增 `crates/cc-plugins/src/dependency_resolver.rs`
- 新增 `crates/cc-plugins/src/autoupdate.rs`
- 新增 `crates/cc-plugins/src/versioning.rs`
- 新增 `crates/cc-plugins/src/reconciler.rs`
- 新增 `crates/cc-plugins/src/lsp.rs`
- 新增 `crates/cc-plugins/src/policy.rs`
- 新增 `crates/cc-plugins/src/blocklist.rs`
- 新增 `crates/cc-plugins/src/flagging.rs`
- 新增 `crates/cc-plugins/src/configuration.rs`
- 修改 `crates/cc-plugins/src/mod.rs`
- 修改 `crates/cc-plugins/src/lib.rs`
- 修改 `crates/cc-plugins/src/manifest.rs`
- 修改 `crates/cc-plugins/src/loader.rs`
- 修改 `crates/cc-plugins/src/refresh.rs`
- 修改 `crates/cc-plugins/src/tools.rs`
- 修改 `crates/cc-commands/src/plugin_cmd.rs`

依赖：

- 依赖 Lane A 的 policy contract 后再接 plugin-only policy。
- 依赖 Lane B 的 event contract 后再发 plugin telemetry。
- 依赖 Lane C 的 dynamic command registry 后再注册 plugin commands。

明确不负责：

- 不直接修改 `cc-commands/src/lib.rs` registry 核心，走 Lane C contract。
- 不直接修改 TUI command palette。
- 不直接改 OTel exporter。

交付物：

- Marketplace add/list/refresh/install/update/uninstall 基线。
- MCPB bundle 解析、hash/cache、配置输入。
- plugin commands / agents / hooks / output styles loader。
- 完整 plugin validation 和诊断。
- plugin LSP declaration 收集 API。

验证：

- `cargo test -p cc-plugins`
- `cargo test -p cc-commands plugin`
- 离线 fixture：local marketplace + file source + zip cache + invalid manifest。

### Lane E：Input / Completion / ProcessUserInput

Owner：Rust TUI input、headless ingress、engine input contract。

写入范围：

- 新增 `crates/claude-code-rs/src/ui/input/completions.rs`
- 新增 `crates/claude-code-rs/src/ui/input/path_completion.rs`
- 新增 `crates/claude-code-rs/src/ui/input/shell_history_completion.rs`
- 新增 `crates/claude-code-rs/src/ui/input/slack_channel_completion.rs`
- 修改 `crates/claude-code-rs/src/ui/mod.rs` 导出新增 input helper
- 修改 `crates/claude-code-rs/src/ui/prompt_input.rs`
- 修改 `crates/claude-code-rs/src/ui/app/input.rs`
- 修改 `crates/claude-code-rs/src/ui/app/render.rs`
- 修改 `crates/claude-code-rs/src/ui/command_palette/filter.rs`
- 修改 `crates/claude-code-rs/src/ui/command_palette/mod.rs`
- 修改 `crates/claude-code-rs/src/ui/command_palette/render.rs`
- 修改 `crates/claude-code-rs/src/ui/components/fuzzy_match.rs`，仅在 command registry contract 稳定后
- 修改 `crates/cc-engine/src/input_processing.rs`
- 修改 `crates/cc-engine/src/lifecycle/submit_message.rs`

依赖：

- 依赖 Lane C 的 command/skill registry contract。
- input-box `!` bash / PowerShell 路由依赖 Bash/Shell parity 计划中的 exec policy，不应在本计划里重做 shell parser。
- telemetry 只通过 Lane B 的 span/event API 插桩，不自建 telemetry 类型。

明确不负责：

- 不加载插件 manifest。
- 不直接改 plugin installer。
- 不负责 MDM/settings watcher。

交付物：

- TUI 内统一 `CompletionItem` / ghost suffix / apply contract。
- command/path/shell-history/Slack completion 的可测试模块。
- Bun 式中央 input contract 的最小 Rust 版：slash、prompt、bash mode、attachments/images、UserPromptSubmit hook 串到同一入口。
- headless IPC 的 completion/input shape 只在 Integration Lane 统一改。

验证：

- `cargo test -p claude-code-rs ui::command_palette`
- `cargo test -p claude-code-rs ui::input`
- `cargo test -p cc-engine input_processing`

### Lane F：LSP Recommendation / Plugin LSP Runtime

Owner：`cc-lsp-service` 和 LSP UI/command glue。

写入范围：

- 新增 `crates/cc-lsp-service/src/recommendation.rs`
- 修改 `crates/cc-lsp-service/src/mod.rs`
- 修改 `crates/cc-commands/src/lsp_cmd.rs`
- 修改 `crates/claude-code-rs/src/ui/lsp_recommendation/lsp_recommendation_menu.rs`
- 仅在 Lane D 提供 API 后修改 `crates/cc-plugins/src/lsp.rs`（新增，由 Lane D 创建）

依赖：

- 依赖 Lane D 的 plugin LSP declaration 收集 API。
- 安装动作通过 Lane D installer，不在 LSP lane 自建安装器。

交付物：

- 基于项目文件/语言使用情况的推荐请求。
- `set_config_provider()` runtime wiring 的最小封装。
- 推荐 UI 的 `yes` 动作接到 plugin installer contract。

验证：

- `cargo test -p cc-lsp-service`
- `cargo test -p claude-code-rs lsp_recommendation`

## 4. 串行 Integration Lane

以下文件容易被多条 lane 同时修改，必须等 contract 稳定后由一个 integration owner 集中改：

- `crates/claude-code-rs/src/main.rs`
- `crates/claude-code-rs/src/app_subsystem_handlers.rs`
- `crates/claude-code-rs/src/app_runtime_adapters/mod.rs`
- `crates/claude-code-rs/src/app_runtime_adapters/ingress.rs`
- `crates/cc-ipc-protocol/src/protocol/mod.rs`
- `crates/cc-ipc-protocol/src/subsystem_events.rs`
- `crates/cc-engine/src/lifecycle/submit_message.rs`
- `crates/cc-engine/src/input_processing.rs`
- `crates/cc-commands/src/lib.rs`
- `crates/cc-config/src/settings.rs`

Integration 任务：

1. 启动顺序：managed settings / plugin registry / MCP discovery / LSP provider / telemetry 初始化必须有确定顺序。
2. IPC shape：completion、plugin events、telemetry status、LSP recommendation 统一扩展 protocol。
3. Runtime event flow：plugin install/update、input submit、tool execution、permission prompt、LSP recommendation 统一打 audit/telemetry。
4. UI/headless parity：TUI 和 headless ingress 不应各自实现不同输入语义。
5. 文档同步：更新四个 `docs/utils/*.md` 的状态，把已落地项从缺口中移除或标为部分实现。

## 5. 推荐执行顺序

### Phase 0：Contract Freeze

可以并行：

- Lane A 定义 settings/policy validation DTO。
- Lane B 定义 telemetry/event/span API。
- Lane C 定义 command/skill registry metadata。

禁止事项：

- Phase 0 不改 `main.rs`。
- Phase 0 不做 marketplace 网络下载。
- Phase 0 不扩 IPC protocol。

退出标准：

- `cc-config`、`cc-observability`、`cc-services`、`cc-commands`、`cc-skills` 的 contract 单元测试通过。
- 四个 contract 文档或 module docs 写明 owner 和消费方式。

### Phase 1：Domain Implementations

可以并行：

- Lane D 做 marketplace/installer/component loader 的离线 fixture。
- Lane E 做 TUI 内 completion/path/shell history/input contract。
- Lane F 做 LSP recommendation 与 provider 封装。

依赖约束：

- Lane D 的 plugin commands 只注册到 Lane C registry，不改 registry 核心。
- Lane E 的 command palette 只消费 Lane C registry，不扫描插件目录。
- Lane F 的安装动作只调用 Lane D installer，不自己下载插件。

退出标准：

- 每条 lane 的 crate 级 tests 通过。
- 本地 fixtures 覆盖 invalid config、disabled plugin、policy denied、missing command、cache miss。

### Phase 2：Serial Integration

由一个 integration owner 串行完成：

- 改 `main.rs` 启动 wiring。
- 改 IPC protocol 和 subsystem events。
- 改 app runtime adapters / subsystem handlers。
- 合并 input submit pipeline 与 telemetry 插桩。
- 接 plugin MCP/LSP provider 到真实 runtime。

退出标准：

- `cargo check --workspace`
- `cargo test -p cc-plugins`
- `cargo test -p cc-config`
- `cargo test -p cc-observability`
- `cargo test -p cc-services`
- `cargo test -p cc-commands`
- `cargo test -p cc-skills`
- `cargo test -p cc-engine`
- `cargo test -p claude-code-rs`

### Phase 3：End-to-End Capability Test

复用 `development/archive/plan/2026-05-17-mcp-skill-plugin-real-project-test-plan.md`：

- local marketplace 添加、刷新、安装、禁用、启用、卸载。
- plugin skills、commands、MCP、LSP 同时存在时不重复注册。
- TUI 输入补全能看到 builtin、skill、plugin command，并按 usage 排序。
- headless IPC 能提交等价 prompt、slash、附件/图片 metadata。
- telemetry/audit 能记录 install、reload、submit、permission、tool、error。
- managed policy 能阻断被 blocklist 或 policy 禁止的 plugin/install/tool。

## 6. 并行开发规则

1. 每个 lane 使用独立分支或 worktree，提交只包含自己 lane 的文件。
2. 新文件优先，少改共享入口；共享入口只在 Integration Lane 修改。
3. 如果必须修改共享文件，先在 lane PR/commit 说明中标记为 integration-needed，不在同一轮并行落地。
4. 所有 registry、policy、telemetry、completion 都通过 contract 交互，禁止跨 crate 直接读对方内部状态。
5. 每个 lane 完成后更新对应 `docs/utils/*.md` 的“已实现/缺失”状态。
6. 文档更新按任务拆分；每完成一个文档更新任务单独 commit。

## 7. 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| 多 lane 同时改 `main.rs` | 启动顺序冲突，MCP/plugin/LSP/telemetry 互相覆盖 | `main.rs` 只允许 Integration Lane 修改 |
| plugin command 与 skill command 重复注册 | `/` 命令补全和执行行为不稳定 | Lane C 统一 registry source/priority/duplicate policy |
| telemetry 直接写进业务 crate | 难以关闭或替换 exporter | 业务 crate 只发 event/span，exporter 在 Lane B |
| managed policy 被 user/project 覆盖 | 企业策略失效 | Lane A 明确 managed precedence 和 source-aware merge |
| TUI/headless 输入语义分叉 | e2e 行为不一致 | Lane E 输出共享 input contract，Integration Lane 接 IPC |
| LSP 推荐自建安装路径 | 与 plugin installer 重复 | Lane F 只调用 Lane D installer API |
| marketplace 网络依赖导致测试不稳定 | CI/本机验证不可复现 | Phase 1 先做 local/file/zip fixture，网络 marketplace 后置 |

## 8. 最小并行分工建议

如果一次开 4 个 worker，推荐如下：

| Worker | 负责 lane | 初始写入范围 |
|--------|-----------|--------------|
| Worker 1 | Lane A | `crates/cc-config/src/mdm/`, `permission_validation.rs`, `validation_tips.rs` |
| Worker 2 | Lane B | `crates/cc-observability/`, `crates/cc-services/src/telemetry/`, telemetry feature closure |
| Worker 3 | Lane C + Lane E contract | `cc-commands` registry contract、`cc-skills/src/usage.rs`、TUI completion DTO |
| Worker 4 | Lane D | `cc-plugins` marketplace/installer/validation/component loader |

Integration owner 单独排期，不和上述 worker 并行修改共享入口。

如果一次开 6 个 worker，Lane F 可单独拆出；Lane E 也可从 Worker 3 拆出，但必须等 Lane C registry contract 合并后再动 command palette。

## 9. 完成定义

计划完成不等于四个 utils 文档全部关闭。完成定义是：

- 每个缺口都有唯一 owner crate。
- 重复功能没有两个 crate 各自实现一套状态或导出器。
- shared startup/IPC/input files 由 Integration Lane 串行接入。
- 四个 `docs/utils/*.md` 与实际 runtime path 一致。
- release 前能用真实项目测试证明 plugin、settings/policy、input completion、telemetry 同时启用时不会互相污染。
