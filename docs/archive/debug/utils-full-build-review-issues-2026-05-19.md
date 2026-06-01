# Utils Full Build 代码审查问题清单

日期：2026-05-19

计划来源：`docs/plan/utils-full-build-parallel-development-plan-2026-05-19.md`

审查提交范围：

- `8594ffd` Phase 0 contract freeze (MDM/policy, telemetry, dynamic registry)
- `0fc7c35` Phase 1 domain implementations (plugin marketplace, input completions, LSP recommendation)
- `e0a28f7` Phase 2 serial integration (IPC protocol, startup wiring, telemetry bridge)

审查方式：

- Lane A/B：`cc-config` MDM/policy/settings validation，`cc-observability`/`cc-services` telemetry contract。
- Lane C/E：dynamic command registry、skill usage、TUI input completion、engine input pipeline。
- Lane D/F：plugin marketplace/installer/component loader、LSP recommendation。
- Integration：`main.rs`、IPC protocol、headless ingress、runtime adapters、telemetry bridge。

## 验证结果

- `cargo check --workspace`：通过，但 `claude-code-rs` 新增 10 个 warning，包含未使用的 completion/IPC stub 参数、未使用的 command palette 导出和 dead code。
- `cargo check -p claude-code-rs --features telemetry`：失败，`cc_engine::telemetry_bridge` 未启用。
- `cargo check -p cc-engine --features telemetry`：失败，缺少 `opentelemetry`、`opentelemetry_sdk`、`opentelemetry_langfuse`、`tracing_opentelemetry` 等依赖，并触发多处 `Span::set_attribute` / `set_status` 方法不可用。

## Critical

### 1. `claude-code-rs --features telemetry` 无法编译

证据：

- `crates/claude-code-rs/Cargo.toml:25` 的 `telemetry` feature 只启用 `cc-services/telemetry` 和 `cc-startup/telemetry`，没有启用 `cc-engine/telemetry`。
- `crates/claude-code-rs/src/main.rs:545` 在 telemetry block 中导入 `cc_engine::telemetry_bridge`。
- `crates/cc-engine/src/lib.rs:36` 将 `telemetry_bridge` gate 在 `#[cfg(feature = "telemetry")]` 后面。

复现：

```bash
cargo check -p claude-code-rs --features telemetry
```

结果：`E0432 unresolved imports cc_engine::telemetry_bridge`。

影响：Lane B 计划要求的 `cargo check -p claude-code-rs --features telemetry` 退出标准不成立，telemetry/full-build 路径不可构建。

缺失测试：CI/build matrix 必须覆盖 `cargo check -p claude-code-rs --features telemetry`。

### 2. `cc-engine --features telemetry` 本身也无法编译

证据：

- `crates/cc-engine/Cargo.toml:66` 的 `telemetry = []` 是空 feature。
- `crates/cc-engine/src/services/langfuse/client.rs:4`、`crates/cc-engine/src/services/langfuse/tracing.rs:1` 等 telemetry-gated Langfuse 代码引用 OTel/Langfuse crates。

复现：

```bash
cargo check -p cc-engine --features telemetry
```

结果：缺少 `opentelemetry`、`opentelemetry_sdk`、`opentelemetry_langfuse`、`tracing_opentelemetry`，并继续触发 tracing span 扩展方法不可用。

影响：即使 root feature 改成启用 `cc-engine/telemetry`，engine telemetry feature 仍不闭合。

缺失测试：CI/build matrix 必须覆盖 `cargo check -p cc-engine --features telemetry`。

## High

### 3. Telemetry bridge 安装了，但真实 submit/hook 路径没有调用

证据：

- `crates/claude-code-rs/src/main.rs:649` 安装 `telemetry_bridge`。
- `crates/cc-engine/src/telemetry_bridge.rs:70` 提供 `with_bridge()`。
- `crates/cc-engine/src/lifecycle/submit_message.rs:636` 开始执行 `UserPromptSubmit` hook，`submit_message.rs:691` 处理输入，但没有任何 `telemetry_bridge::with_bridge` call site。

影响：feature 编译问题修复后，交互 submit 和 hook 仍不会产生 Lane B 声称的 interaction/hook span。

缺失测试：mock bridge 或 telemetry handle 集成测试，断言 `submit_message` start/end submit span，`UserPromptSubmit` start/end hook span。

### 4. MDM managed policy 可被 user/project/local/env 覆盖

证据：

- `crates/cc-config/src/mdm/mod.rs:20` 文档声称 managed policy fields always win。
- `crates/cc-config/src/settings.rs:1073` 先 merge managed，随后 `settings.rs:1081` 继续 merge user/project/local，最后 env。
- `crates/start-up/src/runtime_config.rs:123` 运行时直接使用合并后的 `merged.permissions.enable_bypass_mode` / `enable_auto_mode`。

影响：managed `permissions.enableAutoMode=false`、`enableBypassMode=false` 或 permission mode 可以被低优先级配置重新打开，违反 Lane A 的 managed precedence 要求。

缺失测试：managed non-overridable policy 对 user/project/local/env override 的覆盖测试，至少覆盖 `enableAutoMode`、`enableBypassMode`、permission mode。

### 5. `allowManagedReadPathsOnly` / `allowManagedDomainsOnly` 被解析但未执行

证据：

- `crates/cc-config/src/settings.rs:256` 定义 `allow_managed_read_paths_only`。
- `crates/cc-sandbox/src/policy.rs:332` 直接消费合并后的 `settings.filesystem.allow_read`。
- `crates/cc-sandbox/src/policy.rs:351` 直接消费合并后的 `settings.network.allowed_domains`。

影响：managed policy 要求只允许 managed paths/domains 时，user/project/local 仍可以扩展 read allowlist 或 network domain allowlist。

缺失测试：managed-only flags + lower-source allow paths/domains 的 sandbox policy 构建测试。

### 6. `/plugin install` 和 `/plugin update` 在 async command 中嵌套 Tokio runtime

证据：

- `crates/cc-commands/src/plugin_cmd.rs:82` 的 command handler 是 async。
- `crates/claude-code-rs/src/command_runtime_bridge.rs:530` 和 `:588` 创建新 `tokio::runtime::Runtime` 并 `block_on()` installer/update。

影响：TUI/headless 中执行 `/plugin install` 或 `/plugin update` 可能因 nested runtime panic，而不是安装/更新插件。

缺失测试：在 Tokio runtime 下通过真实 command runtime adapter 执行 `/plugin install` / `/plugin update`。

### 7. `/plugin install` 未接入 managed policy、依赖上下文和真实 manifest 集合

证据：

- `crates/claude-code-rs/src/command_runtime_bridge.rs:526` 构造空 `available_plugins`、空 `all_manifests`。
- `command_runtime_bridge.rs:533` 调 `install_plugin(..., None, &available_plugins, &all_manifests)`，policy 永远是 `None`。
- `crates/cc-plugins/src/installation.rs:138` 只有传入 policy 时才执行 plugin install policy。

影响：managed plugin policy、max plugins、依赖解析和 cycle 检查在真实 `/plugin install` path 中失效或结果错误。

缺失测试：真实 `/plugin install` adapter 的 managed `allowedSources`、blocklist、max plugins、dependency present/missing fixture。

### 8. Marketplace refresh/search 仍不是可用 marketplace

证据：

- `crates/claude-code-rs/src/command_runtime_bridge.rs:548` 的 `list_marketplace_for_commands(_query)` 忽略 query。
- `command_runtime_bridge.rs:567` 只从 `known_marketplaces.json` load source 配置，没有 fetch/parse entries 进入 cache。

影响：`/plugin marketplace refresh` 可返回成功但没有可安装插件；`/plugin marketplace search <q>` 不按 query 过滤。

缺失测试：offline local marketplace fixture，覆盖 refresh populate entries、search filter、install resolves marketplace entry。

### 9. npm/tgz 安装路径错误 （暂不解决）

证据：

- `crates/cc-plugins/src/sources.rs:179` 从 npm registry 取响应。
- `sources.rs:193` 将响应 bytes 写成 `package.tgz`。
- `crates/cc-plugins/src/installation.rs:176` 将 `tgz` 交给 ZIP extraction。

影响：npm registry metadata JSON 会被当成 tgz/zip 包处理；真实 npm 插件安装在 manifest load 前失败。

缺失测试：offline npm metadata fixture，跟随 `dist.tarball` 下载；`.tgz` 解包测试。

### 10. MCPB integrity 校验没有接入 installer

证据：

- `crates/cc-plugins/src/installation.rs:171` parse MCPB 后直接 extract `bundle.payload`。
- `crates/cc-plugins/src/mcpb.rs:150` 已实现 `verify_mcpb_integrity()`，但 install path 未调用。

影响：被篡改的 MCPB payload 仍可被接受并解包。

缺失测试：corrupted MCPB fixture 必须在 extract/register 前失败。

### 11. Dynamic command registry 只进 UI/metadata，不进真实执行路径

证据：

- `crates/claude-code-rs/src/main.rs:523` 将 plugin commands 注册进 `DYNAMIC_REGISTRY`。
- `crates/cc-commands/src/lib.rs:583` 有 `get_dynamic_metadata()`，但 `lib.rs:1081` 的 `parse_command_input()` 仍使用 `command_metadata(&get_all_commands())`。
- `crates/cc-commands/src/lib.rs:1095` 的 `DefaultCommandDispatcher::for_full_registry()` 也只使用 builtin command metadata。

影响：plugin/user/project/skill dynamic commands 可以出现在 completion/palette，但 TUI/headless/engine slash 执行路径无法解析或会按错误 static index dispatch。

缺失测试：注册 dynamic/plugin command 后，覆盖 `parse_command_input`、`DefaultCommandDispatcher`、TUI command execution、headless `SlashCommand`。

### 12. `/skill-name` fallback 会丢输入

证据：

- `crates/cc-engine/src/input_processing.rs:221` 对已知 skill 返回 `should_query = true`，但 `messages = Vec::new()`。
- `crates/cc-engine/src/lifecycle/submit_message.rs:710` 只 append `processed.messages`，没有消费 `skill_invocation`。

影响：用户输入 `/skill arg` 时不会注入/执行 skill prompt，可能用旧 transcript 继续 query，当前输入被丢弃。

缺失测试：注册 user-invocable skill 后 `submit_message("/skill arg")` 应注入 skill prompt、保留 args、记录 usage。

### 13. `!` bash mode 只包装成普通用户消息

证据：

- `crates/cc-engine/src/input_processing.rs:183` 将 `!cmd` 包成 `MessageContent::Text("!cmd")`，并标记 `bash_mode = true`。
- `crates/cc-engine/src/lifecycle/submit_message.rs:760` 之后没有任何 `bash_mode` 分支。

影响：`!git status` 会作为普通文本发给 LLM，而不是运行 Bash/PowerShell 并生成 bash stdout/stderr context。

缺失测试：fake shell executor 下 submit `!echo ok`，断言走 shell execution/progress，不是 literal prompt dispatch。

### 14. Phase 2 headless IPC 新消息都是 stub

证据：

- `crates/claude-code-rs/src/app_runtime_adapters/ingress.rs:237` 明确标注 `Stub handlers`。
- `ingress.rs:239` `RequestCompletions` 返回空 `items`。
- `ingress.rs:255` `AcceptCompletion` 无 side effect。
- `ingress.rs:266` `InstallRecommendedPlugin` 无 side effect。
- `ingress.rs:271` `RefreshPluginTelemetry` 无 side effect。
- `ingress.rs:276` `RequestLspRecommendations` 返回空 recommendations。

影响：IPC 协议表面接受 Phase 2 能力，但 external/headless client 得到成功空响应，TUI/headless parity 不成立。

缺失测试：JSONL/headless tests 覆盖 completions、recommended plugin install、telemetry refresh、LSP recommendations 的真实响应/side effect。

### 15. LSP recommendation engine 和 `/lsp` command 没有真实 wiring

证据：

- `crates/cc-lsp-service/src/mod.rs:94` 提供 `set_recommendation_engine()`，但当前 root 未调用。
- `crates/cc-commands/src/lib.rs:257` 提供 `set_lsp_recommendations_provider()`，但 `command_runtime_bridge` 未设置该 provider。
- `crates/cc-commands/src/lsp_cmd.rs:122` 从默认空 provider 读取 recommendations。

影响：`/lsp recommend` 和 `/lsp recommendations --all` 永远看不到项目推荐，即使 `cc-lsp-service` 已实现扫描逻辑。

缺失测试：真实 command runtime provider + temp Rust/Python project 返回非空 recommendations。

### 16. LSP recommendation 的 `yes` 动作仍未安装插件

证据：

- `crates/claude-code-rs/src/app_subsystem_handlers.rs:309` 对 `yes` 返回 “install path not wired” 文案。
- `crates/cc-lsp-service/src/recommendation.rs:551` 的 `install_recommended_plugin()` 注释仍声明 STUB。

影响：TUI recommendation accept 不能调用 Lane D installer，违反 Lane F “yes action 接到 plugin installer contract” 交付物。

缺失测试：recommendation response `yes` 调用 installer 并发 success/failure event。

### 17. Telemetry redaction 配置没有应用到 record/export path

证据：

- `crates/cc-services/src/telemetry/mod.rs:53` 定义 `TelemetryConfig.redaction`。
- `crates/cc-services/src/telemetry/privacy.rs:45` 实现 `redact_for_telemetry()`。
- `crates/cc-services/src/telemetry/mod.rs:243` 的 `TelemetryHandle::record()` 直接 push raw event。

影响：一旦 exporter 接通，input summary/tool summary 可能未按配置脱敏就被导出。

缺失测试：带敏感 keys/file paths 的 record/export 测试，断言 flush/export 前已经 redaction。

## Medium

### 18. Normalized IPC 对新增 backend message 仍返回 Unsupported

证据：

- `crates/cc-ipc-protocol/src/normalized.rs:191` 为 `Completions`、`PluginInstallProgress`、`TelemetryStatus`、`LspRecommendations` 分配 legacy type。
- `normalized.rs:348` 的 `legacy_backend_to_payload()` 对这些 variant 落入 `Unsupported`。

影响：envelope/normalized IPC client 收到 unsupported payload，不能消费 Phase 2 新 backend messages。

缺失测试：新增 backend variant 的 normalized payload round-trip。

### 19. TUI completion 只注册 command provider

证据：

- `crates/claude-code-rs/src/ui/app/input.rs:29` 只 `add_provider(CommandCompletionProvider::new())`。

影响：Path、shell history、Slack channel completion 模块已导出但 TUI Tab 永远不可达。

缺失测试：`CompletionState`/TUI Tab tests 覆盖 `src/`、`!gi`、`#gen`。

### 20. Skill usage 不持久化，debounce 也不符合计划

证据：

- `crates/cc-skills/src/lib.rs:417` 注释说明 global tracker 只在 process lifetime 内，persistence is caller responsibility。
- `cc-skills/src/lib.rs:444` 提供 save/load API，但未见启动/退出路径调用。
- `crates/cc-skills/src/usage.rs:51` debounce 为 30 秒，计划要求 60 秒。

影响：usage 排序重启后丢失；31-59 秒内重复调用会被多计。

缺失测试：global usage restart/load-save round trip；30s/60s debounce boundary。

### 21. Plugin command unregister 是 stub，且注册时丢掉 plugin_id

证据：

- `crates/cc-commands/src/plugin_commands.rs:16` candidate 有 `plugin_id`。
- `plugin_commands.rs:30` 注册 `DynamicCommandEntry` 时未保存 plugin_id。
- `plugin_commands.rs:52` `unregister_plugin_commands()` 直接返回 0。

影响：disable/uninstall/reload 某个插件时无法移除其 dynamic commands，suggestions 可能出现 stale commands 直到重启。

缺失测试：两个插件各注册 commands，unregister 一个插件后只移除该插件 commands。

### 22. Local plugin directory 不能安装

证据：

- `crates/cc-plugins/src/sources.rs:211` local resolver 只按 file 读取。
- `sources.rs:216` 对 directory 直接 `std::fs::read(path)`。

影响：`/plugin install <local-dir>` 对包含 `plugin.json` 的本地插件目录失败。

缺失测试：local directory fixture with `plugin.json`。

### 23. Completion 排序和 ghost suffix 错误

证据：

- `crates/claude-code-rs/src/ui/input/completions.rs:338` exact check 比较 `filter_text`（例如 `/help`）和 query（例如 `help`）。
- `completions.rs:409` `CombinedCompleter` 再按 label 字母序排序，覆盖 provider 的 exact/source/usage 排序。
- `completions.rs:290` ghost suffix 用 `cursor_pos - range.start`，包含 leading `/`，`/he` 对 `/help` 产生 `p` 而不是 `lp`。

影响：command/skill/plugin completion 的 exact、source、usage order 失效，ghost text 误导用户。

缺失测试：`/help` exact order、高 usage dynamic skill、plugin tie-break、`/h` / `/he` / mid-input `foo /he` ghost suffix。

### 24. `/plugin validate` 成功也显示为 issues

证据：

- `crates/claude-code-rs/src/command_runtime_bridge.rs:616` 成功时返回 `vec!["Plugin validation passed"]`。
- `crates/cc-commands/src/plugin_cmd.rs:600` 将任何非空 vector 当成 `Validation issues`。

影响：valid plugin 被展示为有 validation issues。

缺失测试：`/plugin validate <valid-plugin>` 输出不进入 issues branch。

## 建议修复顺序

1. 先修复 telemetry feature closure：root feature -> `cc-engine/telemetry`，并整理 `cc-engine` telemetry deps 或移除重复 Langfuse impl。
2. 修复 managed policy precedence 和 sandbox managed-only flags，避免企业策略绕过。
3. 改 `/plugin install/update` runtime API，避免 nested Tokio runtime，并传入 managed policy、dependency/manifests。
4. 将 dynamic command registry 接入 parser/dispatcher/executor，定义 plugin command execution strategy 的真实 handler。
5. 补齐 headless IPC Phase 2 handlers，不再返回成功空响应。
6. 接通 LSP recommendation engine/provider/yes install path。
7. 补齐 TUI completion provider 注册和排序/ghost 行为。
8. 最后清理 `cargo check --workspace` 产生的新 warning，尤其是 unused stub 参数和 dead code。
