# 命令 E2E 测试计划 03：认证与模型命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_auth.rs`
> 离线和在线测试混合。
>
> 已隐藏命令（不在此计划中测试）：`/login-code`、`/model-add`

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 模式 |
|---|---|---|---|
| `/logout` | -- | `Output` | 离线 |
| `/advisor` | -- | `Output` | 离线 |

## 已测试的命令（供参考）
- `/login` — 在 `model_flow.rs` 和 `script.rs` 中已测试
- `/model` — 在 `model_flow.rs` 和 `script.rs` 中已测试
- `/permissions` — 在 `permissions.rs` 和 `script.rs` 中已测试

## 测试用例

### T01：`/logout` 命令
```rust
fn logout_command() {
    // 步骤：
    // 1. Command("logout")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Snapshot("logout_result")
    // 断言：无 panic，显示退出确认
}
```

### T02：`/advisor` 显示当前
```rust
fn advisor_show() {
    // 步骤：
    // 1. Command("advisor")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示当前 advisor 模型或 "none"
}
```

### T03：`/advisor set <model>`
```rust
fn advisor_set_model() {
    // 步骤：
    // 1. Command("advisor set gpt-5.5")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：advisor 模型已设置
}
```

### T04：`/advisor clear`
```rust
fn advisor_clear() {
    // 步骤：
    // 1. Command("advisor clear")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：advisor 已清除
}
```

## 优先级：中
认证命令已部分测试。重点关注未测试的子命令和错误路径。

---

## 补充功能说明

以下为各命令的功能细节，供测试断言与边界覆盖参考。

---

### `/logout`

#### 功能描述

清理 keychain 中的认证凭据与 `~/.cc-rust/credentials.json` 中的 OAuth token，同时清除 onboarding 向导状态。该命令是幂等的——即使没有认证凭据也会返回清理摘要。不会清除环境变量中的 `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN`，但会检测并提示用户手动 unset。

#### 输出示例

正常退出（已认证状态）：
```
Logout complete. Cleanup summary:
  ✓ Auth credentials (keychain + credentials.json, cleared)
  ✓ Onboarding state (onboarding.json, cleared)

Heads up: ANTHROPIC_API_KEY is set in the environment — the environment still authenticates; unset it in your shell before restarting cc-rust.
```

未认证状态（无任何凭据）：
```
Not currently authenticated and no onboarding state — nothing to clear.
```

有 managed settings 时：
```
Managed (policy) settings at /etc/cc-rust/managed-settings.json were NOT touched — they are administrator-owned and outside the scope of /logout.
```

#### 源码路径

- 命令处理器：`crates/cc-commands/src/logout.rs` — `LogoutHandler::execute()`
- 清理认证：`cc_auth::oauth_logout()`
- 清理 onboarding：`crates/cc-commands/src/logout.rs` — `clear_onboarding_for_logout()`（通过 `set_onboarding_logout_clearer` 注入的回调）
- 环境变量检测：`crates/cc-commands/src/logout.rs` — `detect_env_override()`
- Managed settings 检测：`crates/cc-commands/src/logout.rs` — `detect_managed_settings()`

#### 边界情况

1. **未认证 + 无 onboarding 状态**：直接输出 "nothing to clear"，不调用任何清理逻辑
2. **认证但 keychain 清理失败**：报告 "failed: \<error\>" 并继续清理 onboarding
3. **环境变量存在**：检测 `ANTHROPIC_API_KEY` 和 `ANTHROPIC_AUTH_TOKEN`，如有则输出 Heads up 警告，但不自动 unset
4. **Managed settings 存在**：输出 "NOT touched" 提示，不做任何修改
5. **Auth resolution 出错但凭据存在**：仍然尝试清理，报告 "present but invalid"
6. **Onboarding runtime adapter 未安装**：报告 "onboarding runtime adapter is not installed" 错误
7. **参数忽略**：`/logout` 接受任意参数但均被忽略（`_args` 参数未使用）
8. **幂等性**：多次调用 `/logout` 不应产生错误

---

### `/advisor`

#### 功能描述

管理 advisor 模型——一个可选的二级模型，附加到发送给支持 advisor 的 provider（Anthropic、Azure、Bedrock、Vertex）的 API 请求中。支持查看、设置和清除 advisor 模型。设置后同时更新 `app_state`（立即生效）和 `settings.json`（持久化）。

#### 输出示例

查看当前状态（未设置）：
```
Advisor model

  Current: (unset)
  Persisted: (not set)

  Main model: gpt-5.5

Usage:
  /advisor               — show current state
  /advisor <model>       — set advisor model (alias or full id)
  /advisor unset         — clear advisor
```

查看当前状态（已设置）：
```
Advisor model

  Current: my-advisor-model
  Persisted (settings.json::advisorModel): my-advisor-model

  Main model: gpt-5.5

  Status: active — advisor_model will be attached to outbound API requests when the provider supports it (Anthropic, Azure, Bedrock, Vertex). Providers that don't support it ignore the field and log a debug-level notice.

Usage:
  /advisor               — show current state
  /advisor <model>       — set advisor model (alias or full id)
  /advisor unset         — clear advisor
```

设置模型：
```
Advisor model set: gpt-5.5
Persisted to: ~/.cc-rust/settings.json

Note: `advisor_model` is only attached to requests for providers that support it (Anthropic, Azure, Bedrock, Vertex). For other providers the setting is preserved but inactive.
```

更新已有模型：
```
Advisor model updated: old-model -> gpt-5.5
Persisted to: ~/.cc-rust/settings.json
```

清除：
```
Advisor model cleared (was: gpt-5.5).
```

清除（已未设置）：
```
Advisor model cleared (was already unset).
```

已移除的旧别名（如 `sonnet`）：
```
Rejected: <removed alias error message>
```

#### 源码路径

- 命令处理器：`crates/cc-commands/src/advisor.rs` — `AdvisorHandler::execute()`
- 模型别名解析：`cc_models::resolve_model_alias_with_settings()` / `cc_models::resolve_model_alias()`
- 旧别名检测：`cc_models::is_removed_legacy_model_alias()` / `cc_models::removed_legacy_model_alias_error()`
- 持久化：`cc_config::settings::write_user_settings()`

#### 边界情况

1. **无参数**：显示当前 advisor 状态和使用帮助
2. **设置模型名为空/纯空白**：返回 "Rejected: model name required"
3. **使用已移除的旧别名**（如 `sonnet`、`opus`）：返回 Rejected 错误并建议使用新别名
4. **使用 SOTA/MOTA/FOTA 别名**：自动解析为完整模型 ID
5. **清除子命令的多种写法**：`unset`、`none`、`off`、`clear` 均路由到清除逻辑
6. **settings.json 写入失败**：模型在当前会话仍生效，但输出 "Warning: failed to persist" 警告
7. **已在 advisor 状态下再次设置**：输出 "Advisor model updated: old -> new"
8. **在未设置状态下清除**：输出 "was already unset"（幂等）
