# 命令 E2E 测试计划 05：权限与沙箱命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_permissions.rs`
> 离线和在线测试混合。

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 模式 |
|---|---|---|---|
| `/sandbox` | -- | `Output` | 离线 |
| `/sandbox on/off/status` | -- | `Output` | 离线 |
| `/sandbox mode <mode>` | -- | `Output` | 离线 |
| `/sandbox network <on/off>` | -- | `Output` | 离线 |

## 已测试的命令（供参考）
- `/permissions` — 在 `permissions.rs` 和 `script.rs` 中已部分测试（`full access`、`mode bypass`）
- `/permissions mode default/auto/bypass/plan/acceptEdits/dontAsk` — 未测试
- `/permissions allow/ask/deny <rule>` — 未测试
- `/permissions session-grant/clear-session-grants/reset` — 未测试

## 测试用例

### T01：`/sandbox` 显示当前状态
```rust
fn sandbox_shows_status() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("sandbox")
    // 3. Wait(2s)
    // 4. AssertNoPanic
    // 5. Snapshot("sandbox_status")
    // 断言：显示沙箱状态
}
```

### T02：`/sandbox status`
```rust
fn sandbox_status_subcommand() {
    // 步骤：
    // 1. Command("sandbox status")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示沙箱状态信息
}
```

### T03：`/sandbox on` 和 `/sandbox off`
```rust
fn sandbox_toggle() {
    // 步骤：
    // 1. Command("sandbox on")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("sandbox off")
    // 5. Wait(2s)
    // 6. AssertNoPanic
    // 断言：切换操作无 panic
}
```

### T04：`/sandbox mode workspace`
```rust
fn sandbox_mode_workspace() {
    // 步骤：
    // 1. Command("sandbox mode workspace")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：模式已设置
}
```

### T05：`/sandbox mode read-only`
```rust
fn sandbox_mode_readonly() {
    // 步骤：
    // 1. Command("sandbox mode read-only")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：模式已设置
}
```

### T06：`/sandbox mode full`
```rust
fn sandbox_mode_full() {
    // 步骤：
    // 1. Command("sandbox mode full")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：模式已设置（禁用操作系统级沙箱）
}
```

### T07：`/sandbox require`
```rust
fn sandbox_require() {
    // 步骤：
    // 1. Command("sandbox require")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：故障关闭模式已启用
}
```

### T08：`/sandbox network off` 和 `on`
```rust
fn sandbox_network_toggle() {
    // 步骤：
    // 1. Command("sandbox network off")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("sandbox network on")
    // 5. Wait(2s)
    // 6. AssertNoPanic
    // 断言：网络策略已切换
}
```

### T09：`/sandbox no-network`
```rust
fn sandbox_no_network() {
    // 步骤：
    // 1. Command("sandbox no-network")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：无网络策略已设置
}
```

### T10：`/permissions` 尚未测试的模式（离线）
```rust
fn permissions_mode_default() {
    // 步骤：
    // 1. Command("permissions mode default")
    // 2. Wait(2s)
    // 3. AssertScreenContains("default")
    // 断言：模式已设置为 default
}

fn permissions_mode_auto() {
    // 步骤：
    // 1. Command("permissions mode auto")
    // 2. Wait(2s)
    // 3. AssertScreenContains("auto")
    // 断言：模式已设置为 auto
}

fn permissions_mode_plan() {
    // 步骤：
    // 1. Command("permissions mode plan")
    // 2. Wait(2s)
    // 3. AssertScreenContains("plan") 或 AssertScreenContains("readonly")
    // 断言：模式已设置为 plan（只读）
}

fn permissions_mode_acceptEdits() {
    // 步骤：
    // 1. Command("permissions mode acceptEdits")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：模式已设置
}

fn permissions_mode_dontAsk() {
    // 步骤：
    // 1. Command("permissions mode dontAsk")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：模式已设置
}
```

### T11：`/permissions allow/ask/deny`
```rust
fn permissions_allow_rule() {
    // 步骤：
    // 1. Command("permissions allow Bash --session")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：规则已添加
}

fn permissions_ask_rule() {
    // 步骤：
    // 1. Command("permissions ask Edit --session")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：规则已设置为 ask
}

fn permissions_deny_rule() {
    // 步骤：
    // 1. Command("permissions deny Write --session")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：规则已拒绝
}
```

### T12：`/permissions session-grant` 和 `clear-session-grants`
```rust
fn permissions_session_grant() {
    // 步骤：
    // 1. Command("permissions session-grant Bash")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("permissions clear-session-grants")
    // 5. Wait(2s)
    // 6. AssertNoPanic
    // 断言：授权和清除均可用
}
```

### T13：`/permissions reset`
```rust
fn permissions_reset() {
    // 步骤：
    // 1. Command("permissions reset")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：权限已重置
}
```

### T14：`/permissions` 别名 `/perms`
```rust
fn permissions_alias_perms() {
    // 步骤：
    // 1. Command("perms")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

## 优先级：高
权限和沙箱是安全关键功能。应测试所有模式和规则。

---

## 补充功能说明

以下为各命令的功能细节，供测试断言与边界覆盖参考。

---

### `/sandbox`

#### 功能描述

显示或切换当前进程内的沙箱与网络策略。修改仅影响运行时 settings snapshot，不会持久化到 `settings.json`。支持开关沙箱、切换沙箱模式（read-only / workspace / full）、设置 OS-level primitive 不可用时的 fail-closed 策略、以及独立控制网络访问。

#### 输出示例

无参数 / `status`（显示当前状态）：
```
Sandbox status
──────────────
  Enabled:  no
  Mode:     workspace
  Escape:   allowUnsandboxedCommands = false
  Fail if unavailable: false
  Workspace: /path/to/workspace

Platform:
  OS-level sandbox: available via landlock

Network:
  Disabled:        false
  Allowed domains: (all, no restriction)

Filesystem:
  allowWrite: (none)
  denyWrite: (none)
  allowRead: (none)
  denyRead: (none)
```

`on`：
```
Sandbox enabled (mode=workspace).

Sandbox status
...
```

`off`：
```
Sandbox disabled.

Sandbox status
...
```

`mode read-only`：
```
Sandbox mode set to 'read-only'.

Sandbox status
...
```

`mode full`（禁用 OS-level 沙箱）：
```
Sandbox mode set to 'full'.

Sandbox status
...
```

`require`：
```
Sandbox now requires an OS-level primitive.

Sandbox status
...
```

`optional`：
```
Sandbox may fall back to Rust-level checks when the OS-level primitive is unavailable.

Sandbox status
...
```

`no-network`：
```
Network disabled for this session.

Sandbox status
...
```

`network on` / `network off`：
```
Network enabled.

Sandbox status
...
```

未知子命令：
```
Unknown /sandbox subcommand 'foobar'.

Usage:
  /sandbox                — show status
  /sandbox on             — enable sandbox
  ...
```

#### 源码路径

- 命令处理器：`crates/cc-commands/src/sandbox_cmd.rs` — `SandboxHandler::execute()`
- 沙箱策略构建：`cc_sandbox::policy_from_app_state()`
- 沙箱模式类型：`cc_sandbox::SandboxMode`
- 平台可用性：`cc_sandbox::Availability` / `cc_sandbox::Mechanism`

#### 边界情况

1. **无参数 / `status` / `show`**：均渲染完整沙箱状态面板（三者等价）
2. **`on` + 首次启用**：`mode` 为 `None` 时自动设置为 `workspace`（`SandboxMode::default_enabled()`）
3. **`on` + 已有 mode**：保留已有 mode，不覆盖
4. **`off`**：仅设置 `enabled=false`，不清除 mode
5. **`mode full`**：同时设置 `enabled=false`（因为 `full` 等价于禁用 OS-level 沙箱）
6. **`mode` 无参数**：返回 "Usage: /sandbox mode \<read-only | workspace | full\>"
7. **`mode` 非法值**：返回解析错误信息
8. **`require` + 首次**：同时设置 `enabled=true` 和 `fail_if_unavailable=true`，mode 默认 `workspace`
9. **`network` 无参数**：返回 "Usage: /sandbox network \<on|off\>"
10. **`network` 非法值**：返回 "Unknown subcommand 'xxx'. Try: /sandbox network \<on|off\>"
11. **`enable` / `disable`**：分别作为 `on` / `off` 的同义词被接受
12. **`required` / `strict` / `fail-closed`**：均作为 `require` 的同义词
13. **`best-effort`**：作为 `optional` 的同义词
14. **`offline`**：作为 `no-network` 的同义词
15. **所有修改仅影响运行时**：不写 `settings.json`，需持久化应使用 `/config set sandbox.*`

---

### `/permissions` (别名 `/perms`)

#### 功能描述

管理权限模式和规则。支持切换权限模式（default / auto / bypass / plan / acceptEdits / dontAsk）、添加 always-allow / always-ask / always-deny 规则、管理临时 session 授权、以及重置所有内存中的权限规则。持久化范围可选 user / project / local / session。auto 模式会自动剥离过于宽泛的 allow 规则，交由安全分类器审查。

#### 输出示例

无参数（显示当前设置）：
```
Permission settings:

  Mode:                default (bypass available=true, auto available=Some(true))

  Always deny  (deny > ask > allow):
    (none)

  Always ask:
    (none)

  Always allow:
    (none)

  Session grants:
    (none — cleared on session end)

  Additional working directories:
    (none)

  No custom permission rules configured.
```

`mode default`：
```
Permission mode set to: default
```

`mode auto`（无 confirm）：
```
Auto mode lets cc-rust answer permission prompts with a safety classifier. It can still make mistakes; use isolated workspaces for risky tasks.
Confirm with: /permissions mode auto --confirm
```

`mode auto --confirm`：
```
Permission mode set to: auto
Auto mode safety: stripped 1 broad allow rule(s) for Auto mode classifier review.
```

`mode bypass`（无 confirm）：
```
Bypass permissions mode skips permission prompts for potentially dangerous actions. Use only in a sandbox/container/VM you can restore.
Confirm with: /permissions mode bypass --confirm
```

`mode bypass --confirm`：
```
Permission mode set to: bypass
```

`full access`（快捷别名）：
```
Permission mode set to: bypass
```

`mode plan`：
```
Permission mode set to: plan
<plan workflow summary>
```

`mode acceptEdits`：
```
Permission mode set to: acceptEdits
```

`mode dontAsk`：
```
Permission mode set to: dontAsk
```

`allow Bash`：
```
allow rule 'Bash' added (scope=user). → persisted to ~/.cc-rust/settings.json
```

`allow Bash --session`：
```
allow rule 'Bash' added (scope=session). (session-only; not persisted)
```

`ask Edit --project`：
```
ask rule 'Edit' added (scope=project). → persisted to /path/.cc-rust/settings.json
```

`deny Write --session`：
```
Session scope only supports allow grants. Use --user/--project/--local for ask/deny.
```

`session-grant Bash`：
```
Session grant added for 'Bash' (transient — cleared on session end).
```

`clear-session-grants`：
```
Session grants cleared.
```

`reset`：
```
Permission rules reset to defaults (in-memory only — files on disk untouched).
```

未知子命令：
```
Unknown permissions subcommand: 'foobar'
Usage:
  /permissions                              -- show effective settings + sources
  ...
```

Bypass 模式被配置禁用：
```
Bypass mode is disabled by configuration (permissions.enableBypassMode=false).
```

Auto 模式被配置禁用：
```
Auto mode is disabled by configuration (permissions.enableAutoMode=false).
```

#### 源码路径

- 命令处理器：`crates/cc-commands/src/permissions_cmd.rs` — `PermissionsHandler::execute()`
- 权限模式切换（含 auto mode safety）：`cc_permissions::dangerous::set_permission_mode_with_auto_mode_safety()`
- Auto mode 规则剥离：`cc_permissions::dangerous::strip_dangerous_permissions_for_active_auto_mode()`
- Plan 模式工作流：`crates/cc-commands/src/plan_workflow.rs` — `enter_plan_mode_state()` / `persist()`
- 规则持久化：`cc_config::settings::write_user_settings()` / `write_project_settings()` / `write_local_settings()`
- Managed policy shadowing：`cc_config::permission_validation::find_shadowed_rules()`
- 模式别名解析：`cc_engine::types::tool::PermissionMode::parse_configured()`

#### 边界情况

1. **mode 别名映射**：`ask` → `default`，`full access` / `full-access` → `bypass`，`readonly` → `plan`
2. **`full access` 无子命令前缀**：当第一个 token 不是已知子命令时，尝试作为 mode shorthand 解析，自动追加 `--confirm`
3. **bypass 需要确认**：未传 `--confirm` 时返回安全提示，不切换模式
4. **auto 需要确认**：未传 `--confirm` 时返回安全提示，不切换模式
5. **bypass 被配置禁用**（`permissions.enableBypassMode=false`）：返回 "Bypass mode is disabled"
6. **auto 被配置禁用**（`permissions.enableAutoMode=false`）：返回 "Auto mode is disabled"
7. **auto 模式下的 allow 规则剥离**：进入 auto 时自动剥离宽泛 allow 规则（如 `Bash`），保留带 pattern 的规则（如 `Bash(cargo test*)`）；退出 auto 时恢复
8. **session-grant 在 auto 模式下**：新 session grant 立即被 auto mode safety 剥离
9. **`ask` / `deny` + `--session` scope**：被拒绝，返回 "Session scope only supports allow grants"
10. **`--session` scope 不持久化**：仅修改内存中状态，不写 settings 文件
11. **规则持久化失败**：先持久化再修改内存，确保持久化失败时不会留下孤立的内存态
12. **`reset` 仅重置内存**：不修改磁盘上的 settings 文件
13. **plan 模式**：进入时记录 `pre_plan_mode`（用于退出时恢复），创建工作流记录到 workspace root 的 `.cc-rust/plan-workflow.json`
14. **managed policy shadowing**：如果有 managed settings 中的权限规则覆盖了用户规则，会在 `/permissions` 输出中标注 "Shadowed rules"
15. **auto mode 状态显示**：被剥离的规则数量会在 `/permissions` 输出的 "Auto mode stripped allow rules" 段落中展示
16. **未知子命令**：返回 "Unknown permissions subcommand" 并附带完整 usage
