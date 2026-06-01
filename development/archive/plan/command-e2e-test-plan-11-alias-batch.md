# 命令 E2E 测试计划 11：别名与批量冒烟测试

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_aliases.rs`
> 所有测试均为**离线**。目的：验证每个别名均可解析且不会使 TUI 崩溃。

## 策略

对于每个带别名的命令，发送别名作为斜杠命令并断言无 panic。
此方法可捕获别名注册错误，无需验证输出内容。

## 完整别名映射

| 主命令 | 待测试的别名 |
|---|---|
| `/help` | `/h`、`/?` |
| `/exit` | `/quit`、`/q` |
| `/version` | `/v` |
| `/config` | `/settings` |
| `/context` | `/ctx` |
| `/copy` | `/cp` |
| `/memory` | `/mem`、`/global-search`、`/quick-open` |
| `/permissions` | `/perms` |
| `/gbranch` | `/gitbranch` |
| `/branch` | `/br` |
| `/export` | `/markdown-export` |
| `/audit-export` | `/audit` |
| `/session-export` | `/sexport`、`/structured-export` |
| `/experimental` | `/experiments`、`/exp` |
| `/keybindings` | `/keys`、`/shortcuts` |
| `/statusline` | `/status-line` |
| `/terminal-setup` | `/term-setup`、`/terminal` |
| `/doctor` | `/diagnostics`、`/diag` |
| `/plugin` | `/plugins` |
| `/assistant` | `/kairos` |
| `/channels` | （无） |
| `/dream` | `/logs` |
| `/team` | `/teams` |
| `/team-onboarding` | `/teamonboarding` |
| `/coordinator` | `/coord` |
| `/security-review` | `/secreview` |
| `/schedule` | `/cron` |

## 测试用例

### T01：批量别名冒烟测试（第 1 组 - 安全命令）
```rust
fn alias_batch_group1() {
    // 按顺序测试以下别名（全部返回 Output，无副作用）：
    // /h、/?、/v、/settings、/ctx、/cp、
    // /mem、/perms、/gitbranch、/keys、/shortcuts、/status-line、
    // /term-setup、/terminal、/diagnostics、/diag、/exp、/experiments
    //
    // 每个执行：
    // 1. Command(alias)
    // 2. Wait(1s)
    // 3. AssertNoPanic
    //
    // 断言：全部 22 个别名均可解析且无 panic
}
```

### T02：批量别名冒烟测试（第 2 组 - 退出类命令）
```rust
fn alias_batch_group2_exit() {
    // 单独测试退出别名，因为它们会终止 REPL：
    // 分别启动新的 PTY：
    // - "/quit" → 应退出
    // - "/q" → 应退出
    //
    // 断言：两者均正常退出
}
```

### T03：批量别名冒烟测试（第 3 组 - 导出/查询命令）
```rust
fn alias_batch_group3() {
    // 按顺序测试以下别名：
    // /markdown-export、/audit、/sexport、/structured-export、
    // /plugins、/kairos、/logs、/teams、/teamonboarding、
    // /coord、/secreview、/cron、/br
    //
    // 每个执行：
    // 1. Command(alias)
    // 2. Wait(2s)
    // 3. AssertNoPanic
    //
    // 断言：全部 14 个别名均可解析且无 panic
}
```

### T04：未知命令处理
```rust
fn unknown_command_no_crash() {
    // 步骤：
    // 1. Command("nonexistent-command-xyz")
    // 2. Wait(2s)
    // 3. AssertScreenContains("unknown") 或 AssertScreenContains("not found")
    //    或 AssertNoPanic
    // 断言：未知命令被优雅处理
}
```

### T05：空命令
```rust
fn empty_slash_command() {
    // 步骤：
    // 1. TypeText("/") 然后 Key(Enter)  // 空命令
    // 2. Wait(1s)
    // 3. AssertNoPanic
    // 断言：空命令不会崩溃
}
```

### T06：带多余空白的命令
```rust
fn command_with_whitespace() {
    // 步骤：
    // 1. Command("  help  ")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：空白被去除，命令正常工作
}
```

### T07：命令大小写敏感性
```rust
fn command_case_sensitivity() {
    // 步骤：
    // 1. Command("HELP")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：大小写处理（可能可用或显示 "unknown"）
}
```

## 优先级：高
别名测试成本低（离线、快速）且能捕获注册错误。作为 CI 的一部分运行。

---

## 补充功能说明

### 完整别名参考表（含源码规范来源）

下表按 `COMMAND_REFERENCE.md` 整理每条别名的规范定义和对应源码注册位置。"主命令"为规范名称，"别名"为等效的缩写或同义词。别名在命令注册表 `crates/cc-commands/src/lib.rs` 中统一注册，每个别名与主命令路由到同一个 `CommandHandler`。

| 主命令 | 别名 | 源码 Handler | 说明 |
|---|---|---|---|
| `/help` | `/h`、`/?` | `help::HelpHandler` | 无参数列出所有命令；传命令名显示详情 |
| `/exit` | `/quit`、`/q` | `exit::ExitHandler` | 返回 `Exit` 类型，终止 REPL |
| `/version` | `/v` | `version::VersionHandler` | 输出版本号 |
| `/config` | `/settings` | `config_cmd::ConfigHandler` | 显示/设置/重置配置 |
| `/context` | `/ctx` | `context::ContextHandler` | 上下文 token 估算 |
| `/copy` | `/cp` | `copy::CopyHandler` | 提取最后 assistant 文本消息 |
| `/memory` | `/mem`、`/global-search`、`/quick-open` | `memory::MemoryHandler` | 记忆管理（注意 `/global-search` 和 `/quick-open` 作为别名注册） |
| `/permissions` | `/perms` | `permissions_cmd::PermissionsHandler` | 权限管理 |
| `/gbranch` | `/gitbranch` | `gbranch::GbranchHandler` | Git 分支操作 |
| `/branch` | `/br` | `branch::BranchHandler` | Fork 当前会话到新 session |
| `/export` | `/markdown-export` | `export::ExportHandler` | 导出 Markdown |
| `/audit-export` | `/audit` | `audit_export::AuditExportHandler` | 导出可校验 audit record |
| `/session-export` | `/sexport`、`/structured-export` | `session_export::SessionExportHandler` | 导出结构化 session JSON |
| `/experimental` | `/experiments`、`/exp` | `experimental::ExperimentalHandler` | 功能门控检查/覆盖 |
| `/keybindings` | `/keys`、`/shortcuts` | `keybindings_cmd::KeybindingsHandler` | 快捷键管理 |
| `/statusline` | `/status-line` | `statusline_cmd::StatusLineHandler` | 状态栏配置 |
| `/terminal-setup` | `/term-setup`、`/terminal` | `terminal_setup::TerminalSetupHandler` | 终端环境诊断 |
| `/doctor` | `/diagnostics`、`/diag` | `doctor::DoctorHandler` | 聚合诊断 |
| `/plugin` | `/plugins` | `plugin_cmd::PluginHandler` | 插件管理 |
| `/assistant` | `/kairos` | `assistant::AssistantHandler` | KAIROS 助手状态（feature gate） |
| `/channels` | （无） | `channels::ChannelsHandler` | 出站通道管理（feature gate） |
| `/dream` | `/logs` | `dream::DreamHandler` | 日志蒸馏（feature gate） |
| `/team` | `/teams` | `team_cmd::TeamHandler` | Agent Teams 管理 |
| `/team-onboarding` | `/teamonboarding` | `team_onboarding::TeamOnboardingHandler` | 团队引导 |
| `/coordinator` | `/coord` | `coordinator::CoordinatorHandler` | 协调器模式 |
| `/security-review` | `/secreview` | `security_review::SecurityReviewHandler` | 安全审查 |
| `/schedule` | `/cron` | `schedule::ScheduleHandler` | 定时任务 |

**补充说明：**
- `/memory` 的别名 `/global-search` 和 `/quick-open` 与传统项目搜索/快速打开功能不同，在此项目中它们是 `/memory` 的别名，执行记忆管理操作
- `/branch`（别名 `/br`）的主测试在 `command-e2e-test-plan-02` 中，此文件仅测试别名解析
- feature gate 控制的命令（`/assistant`、`/channels`、`/dream`）在未启用时返回提示文本而非报错
- 所有别名在逻辑上与主命令完全等效，包括参数传递和返回类型
- 已隐藏命令（`/extra-usage`、`/rate-limit-options`、`/model-add`、`/login-code`、`/voice`）仍可手动输入执行，但不在命令面板和 `/help` 列表中显示，此计划中也跳过其别名测试

---

### 边界情况详述

#### 1. 未知命令处理

**行为描述：** 用户输入一个未注册的斜杠命令名时，TUI 应优雅处理而非崩溃。

**源码行为：** 命令分发器在 `lib.rs` 的命令注册表中查找命令名；未命中时返回 "Unknown command" 提示文本（`CommandResult::Output`）。

**测试要点：**
- 发送完全不存在的命令名（如 `/nonexistent-command-xyz`）
- 断言无 panic，屏幕显示 "unknown" 或 "not found" 或类似提示
- 不应导致 REPL 退出或消息丢失
- 后续输入应仍可正常处理

#### 2. 空命令处理

**行为描述：** 用户输入单独的 `/` 然后按回车。

**源码行为：** 空命令名在注册表中查找为空字符串，匹配到无参数的 `/help` 或返回空命令提示。

**测试要点：**
- 输入 `/` 后直接回车
- 断言无 panic
- 不应导致 REPL 退出
- 后续命令应正常工作

#### 3. 空白处理

**行为描述：** 命令前后有多余空白字符时的处理。

**源码行为：** 命令解析器会对输入做 `trim()` 处理，去除首尾空白。每个 handler 的 `execute` 方法也对 args 做 `trim()`。

**测试要点：**
- `  help  ` 应等效于 `help`
- `/ help` 中间有空格的行为（可能被解析为无命令名 + "help" 参数）
- 制表符和多个连续空格的处理
- 命令参数部分的前导/尾随空白应被正确去除

#### 4. 大小写敏感性

**行为描述：** 斜杠命令名是否区分大小写。

**源码行为：** 命令注册表使用小写键存储命令名和别名。输入的命令名在查找前通常会被转为小写（具体取决于命令分发器实现）。部分 handler 内部对子命令参数做 `to_ascii_lowercase()` 处理。

**测试要点：**
- `/HELP` 和 `/Help` 是否能正确路由到 `/help`
- `/EXIT` 和 `/Exit` 是否能正确路由到 `/exit`
- 子命令参数的大小写处理（如 `/config SET model test`）
- 完全大写的命令名不应导致 panic

#### 5. 带参数的别名

**行为描述：** 别名后跟参数时应与主命令行为一致。

**测试要点：**
- `/h model` 应与 `/help model` 输出一致
- `/settings show` 应与 `/config show` 输出一致
- `/ctx json` 应与 `/context json` 输出一致

#### 6. 连续别名调用

**行为描述：** 快速连续发送多个别名命令。

**测试要点：**
- 快速连续发送 `/v`、`/ctx`、`/cp` 等非副作用命令
- 每个命令都应正确执行并输出结果
- 不应出现命令交错或输出混乱
