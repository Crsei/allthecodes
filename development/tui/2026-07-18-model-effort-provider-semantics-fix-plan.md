# 模型 Effort Provider 语义与 TUI 展示修复计划

日期：2026-07-18
状态：待实施

## 1. 目标

- 让 `/effort`、模型设置面板、状态栏和真实模型请求使用同一套 provider-aware effort 解析结果。
- Codex/OpenAI Responses 路径只展示并发送 `reasoning.effort` 档位，不再把本地固定预算描述成模型“支持的 thinking tokens”。
- Anthropic 固定 thinking-budget 路径继续支持 token 预算，但 UI 必须明确它是“本地请求预算”，不是 provider 公布的模型上限。
- 修复 Codex 选择 `low`、`medium` 或 `xhigh` 后，重启 TUI 显示成另一档而请求仍发送原档位的问题。
- 消除 `effortLevel`、`output_config.effort`、根级 `model_reasoning_effort`、profile `modelReasoningEffort` 之间没有明确边界的重复读写。
- 模型能力元数据必须带有来源语义；没有 provider 实时证据时，只能称为内置或用户配置值，不能称为实时支持值。

## 2. 已确认的现状与缺陷

### 2.1 “Supported thinking tokens” 不是实时能力

当前 Codex 模型档位来自
`crates/allthecodes-config/src/settings/providers.rs::codex_capability_entries()` 的静态表。
`/login codex` 会把该表复制到 `authProfiles.<profile>.modelCapabilities`，TUI 再从 settings
读取 `supportedReasoningLevels`。该链路没有调用 provider 的模型能力接口，也没有记录目录版本、
更新时间或证据来源。

TUI 进一步调用 `allthecodes_engine::effort::effort_to_budget_tokens()`，把以下本地常量追加到
所有 effort 选项：

| 档位 | 当前本地固定预算 |
| --- | ---: |
| `low` | 4,096 |
| `medium` | 10,240 |
| `high` | 24,576 |
| `xhigh` | 32,768 |
| `max` | 32,768 |

这些数值只参与固定 `thinking.budget_tokens` 路径。Codex Responses 请求实际只发送
`reasoning.effort=<level>`，服务端自行决定 reasoning token 用量。因此当前 Codex TUI 文案把
“本地预算映射”错误展示成了“模型支持值”。

### 2.2 当前值与真实请求可以不一致

当前 `/effort low` 会同时产生：

- `authProfiles.<active>.modelReasoningEffort = "low"`；
- `output_config.effort = "high"`，因为 Claude compatibility 映射会把 low/medium/high 折叠为 high；
- 当前进程的 `AppState.effort_value = "low"`。

重启后，`AppState.effort_value` 优先从 `output_config.effort` 初始化为 high，而 Codex 请求构建又
优先使用 profile 合并得到的 `model_reasoning_effort=low`。结果是 TUI 显示 high、请求发送 low。
`medium` 和 `xhigh` 也有同类漂移。

### 2.3 每轮 override 的优先级错误

`SubmitMessageOverrides.effort` 会写入请求快照中的 `effort_value`，但
`resolve_model_reasoning_effort()` 当前优先采用持久化的 `model_reasoning_effort`。只要 profile
已经设置 effort，每轮 skill/agent/SDK override 就可能无法覆盖它。

### 2.4 `auto` 会冻结旧默认值

当前 `/effort auto` 解析成模型目录中的默认档位并把具体值持久化。后续内置目录或 provider
默认值变化时，用户仍被固定在旧值，无法获得真正的“跟随模型默认”语义。

## 3. 目标行为契约

### 3.1 Provider 分流

| 请求路径 | canonical 设置 | wire 表达 | TUI 展示 |
| --- | --- | --- | --- |
| Codex / OpenAI Responses | active profile `modelReasoningEffort` | `reasoning.effort` 字符串 | 只显示档位；不显示固定 token 数 |
| Anthropic output effort | `output_config.effort` | `output_config.effort` | 显示 provider effort 档位，不伪造 token 上限 |
| Anthropic fixed thinking budget | `effortLevel` 或显式数字预算 | `thinking.budget_tokens` | 可显示“local request budget: N tokens” |
| 未知/custom provider | 明确配置的 provider 能力 | 仅发送该 provider 已实现的字段 | 标注 configured；无能力时禁用 picker |

Codex 路径不得读取或写入 `output_config.effort` 来决定当前档位。Anthropic 路径不得把
`model_reasoning_effort` 当作 Claude output effort 的隐式别名。

### 3.2 单一解析入口和优先级

在共享 engine/config 层建立 provider-aware resolver，返回至少以下信息：

```text
ResolvedEffort {
  level,
  source: TurnOverride | SessionSelection | Profile | LegacyFallback | ModelDefault,
  transport: CodexReasoning | AnthropicOutput | AnthropicBudget | Unsupported,
  local_budget_tokens,
  capability_provenance,
}
```

统一优先级：

1. 当前轮显式 override；
2. 当前会话选择；
3. active provider profile 的 canonical effort；
4. 仅限同一 provider 语义的 legacy fallback；
5. 当前模型默认值；
6. 无可靠默认值时保持 unset，不猜测。

请求构建、TUI picker、`/effort` 无参数输出、status line、hook/status payload 必须消费同一个
resolver 结果，不再各自拼接不同 fallback 顺序。

### 3.3 `auto` 语义

- `auto` 表示清除当前 provider 的显式覆盖，不持久化当时解析出来的具体默认档位。
- UI 同时显示 `Auto` 和当前解析结果，例如 `Auto (bundled default: low)`。
- 切换模型后重新解析默认值，不沿用上一模型的冻结默认。
- 如果默认值仅来自内置目录，必须展示 `bundled`，不能展示 `provider default`。

### 3.4 能力来源与真实性

为 reasoning capability 增加可传播的来源状态：

- `provider_reported`：只有 provider 响应确实携带相应能力时使用；
- `bundled_catalog`：随二进制发布的版本化静态目录；
- `user_configured`：用户或项目显式覆盖；
- `unknown`：没有证据。

TUI 用词相应调整：

- `provider_reported`：`Supported by provider`；
- `bundled_catalog`：`Available in bundled catalog; not live-verified`；
- `user_configured`：`Configured`；
- `unknown`：禁用选择并显示 `Reasoning levels unavailable`。

如果现有 Codex endpoint 不能返回 effort 能力，仍使用内置目录作为 fallback，但不得把它包装成
实时查询结果。不得通过 AI 推测或模型描述文本生成 token 数、context window 或支持档位。

## 4. 配置与迁移策略

### 4.1 停止持久化内置能力快照

- `/login codex`、`/model` 和 `/fast` 不再把整份 `codex_model_capabilities()` 复制到用户 settings。
- profile 只持久化 provider、backend、model、canonical effort 以及真正的用户 override。
- 运行时使用“版本化内置目录 + 用户 override”合并结果构建 picker。
- 保留 `modelCapabilities` schema，供 custom provider 和显式用户覆盖使用。

### 4.2 兼容已有 settings

- 不在普通启动时无条件重写 `~/.allthecodes/settings.json`。
- 对 canonical Codex profile 中与已知旧版内置目录完全匹配的 capability map，标记为 legacy
  generated snapshot；运行时忽略其目录优先级，并在下一次成功 `/login` 或相关配置写入时移除。
- 任何与已知快照不完全一致的配置都视为用户自定义，保留并标记 `user_configured`。
- 写入继续使用现有无损/原子 settings writer，保留未知字段；日志和测试不得输出 token、API key
  或完整 env。

### 4.3 Provider-specific 持久化

- Codex 选择具体档位：只更新 active profile `modelReasoningEffort`；不新增或改写
  `output_config.effort`。
- Codex 选择 auto：删除 active profile 的 `modelReasoningEffort`；旧根级
  `model_reasoning_effort` 仅作为兼容输入，不再作为新写入目标。
- Anthropic output effort：只更新 `output_config.effort`。
- Anthropic fixed-budget effort：只更新 `effortLevel` 或显式预算字段。
- 成功提示只报告本 provider 实际更新的 canonical 字段。

## 5. 实施阶段

### 阶段 A：先固定失败行为

1. 在 `development/archive/KNOWN_ISSUES.md` 记录 Codex token 文案误导、重启显示漂移和 override
   失效三个用户可见问题。
2. 增加失败测试，覆盖 profile=low + output_config=high 时 TUI/请求结果不一致。
3. 增加每轮 override=high、profile=low 时 wire 请求必须为 high 的测试。
4. 增加 `/effort auto` 不得持久化具体默认值的测试。

### 阶段 B：集中 effort 语义

1. 在 `allthecodes-engine` 建立 provider-aware effort transport、source 和 resolver。
2. 移除 `startup_model.rs`、`AppState::from_settings`、TUI config surface 和 status payload 中重复的
   `output_config -> effortLevel` fallback 实现。
3. `ModelCallParams` 携带已解析的 effort 或足够的 provider/source 信息，避免请求层再次用另一套
   顺序解析。
4. 调整 Codex 优先级，使 turn/session override 高于 profile baseline。
5. 保持 Anthropic budget 和 output effort 的现有 wire 兼容，但通过 transport 分流，禁止交叉污染。

### 阶段 C：修复命令和持久化

1. 将 `/effort` validation、current-value 输出、set 和 auto 全部改为 resolver 驱动。
2. 将 `persist_user_profile_reasoning_effort()` 拆成 provider-specific persistence。
3. 删除 Codex 命令路径对 `set_output_config_effort()` 和
   `set_raw_output_config_effort()` 的调用。
4. 实现 legacy capability snapshot 识别与惰性清理。
5. 模型切换后重新验证显式 effort：不支持时回到 auto 并给出可见提示，不静默发送未知档位。

### 阶段 D：修复所有 TUI 展示面

1. Effort picker 使用 resolved value，Codex 项目只显示档位与 capability provenance。
2. Anthropic budget 项目把 token 文案改成 `local request budget`。
3. `/effort` 无参数输出不再对 Codex 调用 `budget_summary()`。
4. Config summary、status widget、statusline payload、模型详情和相关快照统一使用 resolved effort。
5. 把 `Supported levels` 改为与 provenance 一致的 `Bundled levels`、`Configured levels` 或
   `Provider-reported levels`。

### 阶段 E：目录和文档收口

1. 为内置 Codex catalog 增加显式版本/更新时间和维护来源说明。
2. `/login` 与 `/model` 运行时刷新到当前二进制目录，不再依赖旧 settings 快照。
3. 修订 `development/reference/codex-backend.md`，避免把静态档位描述成实时确认事实。
4. 修订 settings schema 中三个 effort 字段的 provider 边界和弃用/兼容说明。
5. 生成本任务 worktree HTML artifact，记录配置迁移前后、wire 示例和测试证据。

## 6. 预计修改文件

| 路径 | 计划改动 |
| --- | --- |
| `crates/allthecodes-engine/src/effort.rs` | 新增 provider-aware resolver、source/transport 类型；保留 budget 映射但限定语义。 |
| `crates/allthecodes-engine/src/types/app_state.rs` | 从共享 resolver 初始化 effective effort，删除 generic output_config 优先逻辑。 |
| `crates/allthecodes-engine/src/query/turn_context.rs` | 快照单一 resolved effort，保持 turn override 优先级。 |
| `crates/allthecodes-engine/src/lifecycle/helpers.rs` | 按 transport 构造 Codex reasoning、Claude output effort 或 thinking budget。 |
| `crates/allthecodes-commands/src/effort.rs` | provider-specific validation、展示、auto 和持久化。 |
| `crates/allthecodes-config/src/settings/providers.rs` | 内置目录版本、provenance 与 legacy snapshot 辨识。 |
| `crates/allthecodes-config/src/settings/{load,raw,effective,schema}.rs` | canonical 字段边界、兼容读取与迁移测试。 |
| `crates/allthecodes-commands/src/{login,login_code,model,fast}.rs` | 停止复制内置 capability 快照，改用运行时目录。 |
| `crates/allthecodes/src/startup_model.rs` | 删除重复 effort fallback，调用共享 resolver。 |
| `crates/allthecodes/src/startup/app_state_factory.rs` | 使用 provider-aware 初始化结果。 |
| `crates/allthecodes/src/ui/command_surface/surfaces/config.rs` | 修复 picker 当前值、token 文案和来源标签。 |
| `crates/allthecodes/src/ui/app/status.rs` | statusline 使用真实 resolved effort。 |
| `crates/allthecodes/src/ui/command_surface/tests.rs` 及 snapshots | 覆盖 Codex/Anthropic 展示分流与重启一致性。 |
| `crates/allthecodes-api/src/api/openai_compat/{builder,format}.rs` | wire 断言与实际 reasoning usage 边界测试。 |
| `crates/allthecodes/tests/pty_tui_e2e/tests/commands_{core_info,surface}.rs` | `/effort` 交互和用户可见回归。 |
| `development/archive/KNOWN_ISSUES.md` | 记录问题并在验证完成后标记解决。 |
| `development/reference/codex-backend.md` | 修订能力来源和 effort wire 说明。 |

实际实施时先以 `rg` 审核所有 `effort_value`、`model_reasoning_effort`、`output_config.effort`、
`effortLevel` 和 `effort_to_budget_tokens` 消费点，防止遗漏 ACP、Web、hook/statusline 或 agent/skill
override 路径；新增文件以审核结果为准。

## 7. 测试矩阵

### 7.1 Resolver 与请求单元测试

- Codex profile=low、legacy output_config=high：resolved/display/wire 均为 low。
- Codex turn override=high、profile=low：本轮 wire=high，下一轮恢复 session/profile 基线。
- Codex auto：清除显式值，解析到当前模型 bundled default，但 settings 中不写具体默认。
- Codex `none/minimal/low/medium/high/xhigh/max` 逐项验证；未列入当前 capability 的值不得静默发送。
- Anthropic low/medium/high/xhigh/max 继续按既定 output/budget transport 构造。
- 未知 provider 无 capability 时不发送 effort 字段。
- 响应中的 `reasoning_tokens` 只作为事后 usage 展示，不反向声明模型上限。

### 7.2 持久化与重启测试

- `/effort low|medium|xhigh` 设置、保存、重新加载后，picker、status 和 wire 保持同一档。
- Codex effort 变更不改动已有 `output_config`；Anthropic 变更不改动 Codex profile 字段。
- auto 删除 provider-specific override，并保留 settings 中其他字段和秘密占位状态。
- 内置 legacy snapshot 可识别并惰性清理；用户修改过的 capability map 不被删除。
- 根级旧 `model_reasoning_effort` 仍可兼容读取，但 active profile 和当前轮 override 优先。

### 7.3 TUI 与 PTY

- Codex picker 不包含 `4096/10240/24576/32768 tokens`。
- Anthropic fixed-budget picker 显示 `local request budget`，不出现 `supported thinking tokens`。
- capability 来源标签在 bundled、user-configured、unknown 三种 fixture 下正确。
- `/effort`、config surface、status widget 和 statusline 对同一 fixture 显示相同档位。
- 使用隔离 `ALLTHECODES_HOME` 完成设置后重启 PTY，验证 low/medium/xhigh 不漂移。

## 8. 验证命令

在任务 worktree 根目录使用仓库指定的本地 Rust 环境，先执行定向测试，再执行全量门禁：

```bash
cargo test -p allthecodes-engine effort -- --nocapture
cargo test -p allthecodes-commands effort -- --nocapture
cargo test -p allthecodes-config settings -- --nocapture
cargo test -p allthecodes-api openai_compat -- --nocapture
cargo test -p allthecodes --bin allthecodes ui::command_surface::tests -- --nocapture
cargo test -p allthecodes --test pty_tui_e2e commands_core_info -- --nocapture --test-threads=1
cargo test -p allthecodes --test pty_tui_e2e commands_surface -- --nocapture --test-threads=1
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
git diff --check
```

如果实际 test target 名称与过滤器不匹配，先用 `cargo test ... -- --list` 确认，不把“0 tests”
当成验证通过。完整 workspace test 只有取得最终 exit 0 才能标记通过；PTY 卡住时按 `AGENTS.md`
的进程活动与日志增长规则排查。

## 9. 验收标准

- Codex TUI 不再显示固定 thinking-token 数，也不使用“实时支持”措辞描述内置目录。
- 任一 Codex effort 设置在当前会话、重启后、statusline 和实际 wire 请求中一致。
- 每轮 override 能覆盖 profile baseline，结束后不会污染其他轮或其他并发 session。
- `/effort auto` 真正跟随模型默认，不冻结旧目录值。
- Codex effort 变更不再写 Claude `output_config.effort`；配置文件不再新增四套相互冲突的值。
- 已有 settings 可无损加载；legacy generated snapshot 会安全迁移，用户自定义 capability 不丢失。
- provider 无实时能力数据时，UI 明确显示 bundled/configured/unknown，不生成或暗示未经证实的值。
- 定向单元测试、TUI/PTY 回归、fmt、clippy 和 workspace release build 均取得明确成功结果。

## 10. 工作流与提交边界

- 本计划先在主分支 `allthecodes` 单独提交。
- 实施时从该计划提交创建 `.worktrees/model-effort-provider-semantics` 和
  `worktree/model-effort-provider-semantics`。
- 代码、测试、文档、迁移 fixture 和
  `development/worktree-workflow-artifacts/2026-07-18-model-effort-provider-semantics.html`
  全部在独立 worktree 完成。
- 建议按“失败测试与 resolver”“provider-specific persistence”“TUI/PTY 与文档”拆分提交，
  每次只显式暂存本任务路径。
- 最终只允许 `git merge --ff-only` 合并回主分支；验证、推送成功后删除 worktree 和临时分支。
