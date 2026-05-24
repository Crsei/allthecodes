# P0 安全加固 — 执行管道集中拦截

> Status: approved
> Date: 2026-04-10
> Scope: `src/tools/execution.rs` + 集成测试

---

## 1. 目标

将已有但未接入的三项安全机制接入工具执行管道:

1. **危险命令拦截** — `is_dangerous_command()` 阻断 `rm -rf /` 等
2. **路径边界强制** — `validate_file_path()` + `is_path_within_allowed_directories()` 阻断越界写入
3. **Plan 模式写入拦截** — `is_read_only()` 元数据强制执行

---

## 2. 设计

### 2.1 插入点

在 `execution.rs :: run_tool_use()` 管道中，Stage 3b (sanitization) 之后、Stage 4 (pre-tool hooks) 之前插入 **Stage 3c: Security validation**。

```
现有管道:
  Stage 1: Tool lookup
  Stage 2: Abort check
  Stage 3a: Input schema validation
  Stage 3b: Input sanitization
  ── Stage 3c: Security validation ← 新增 ──
  Stage 4: Pre-tool hooks
  Stage 5: Permission check (rule engine)
  Stage 6: Tool execution
  Stage 7: Post-tool hooks
  Stage 8: Result assembly
```

### 2.2 Stage 3c 内部流程

```
Stage 3c: Security validation
  │
  ├── 3c.1: Plan mode gate
  │   └── if mode == Plan && !tool.is_read_only(input) → DENY
  │
  ├── 3c.2: Dangerous command check
  │   └── if tool_name ∈ {"Bash", "PowerShell"} && input.command is_dangerous → DENY
  │
  └── 3c.3: Path boundary check
      └── if tool_name ∈ {"Write", "Edit"} && input.file_path outside allowed dirs → DENY
```

三项检查按顺序执行，任一失败立即返回 `make_error_result()`，不进入后续阶段。

### 2.3 各检查详细规格

#### 3c.1 Plan mode gate

```rust
let app_state = (ctx.get_app_state)();
let mode = &app_state.tool_permission_context.mode;
if *mode == PermissionMode::Plan && !tool.is_read_only(&effective_input) {
    return make_error_result(
        tool_use_id, tool_name,
        &format!("Tool '{}' is not available in Plan mode (read-only exploration only)", tool_name),
        started,
    );
}
```

- 依赖: `PermissionMode::Plan` 已定义, `is_read_only()` 已在所有工具上标注
- Bypass 模式跳过此检查 (Bypass 跳过所有 3c)

#### 3c.2 Dangerous command check

```rust
if tool_name == "Bash" || tool_name == "PowerShell" {
    if let Some(command) = effective_input.get("command").and_then(|v| v.as_str()) {
        if let Some(reason) = crate::permissions::dangerous::is_dangerous_command(command) {
            return make_error_result(
                tool_use_id, tool_name,
                &format!("Dangerous command blocked: {}", reason),
                started,
            );
        }
    }
}
```

- 依赖: `permissions::dangerous::is_dangerous_command()` 已完整实现 (16+ 模式)
- Bypass 模式跳过此检查

#### 3c.3 Path boundary check

```rust
let file_tool_names = ["Write", "Edit", "FileWrite", "FileEdit"];
if file_tool_names.contains(&tool_name) {
    if let Some(file_path_str) = effective_input.get("file_path").and_then(|v| v.as_str()) {
        // Step 1: Validate path (traversal, null bytes, etc.)
        let canonical = match crate::permissions::path_validation::validate_file_path(file_path_str) {
            Ok(p) => p,
            Err(e) => {
                return make_error_result(
                    tool_use_id, tool_name,
                    &format!("Invalid file path: {}", e),
                    started,
                );
            }
        };

        // Step 2: Check within allowed directories
        let cwd = crate::bootstrap::PROCESS_STATE.read().original_cwd.clone();
        let app_state = (ctx.get_app_state)();
        let perm_ctx = &app_state.tool_permission_context;

        if !crate::permissions::path_validation::is_path_within_allowed_directories(
            &canonical, &cwd, perm_ctx,
        ) {
            return make_error_result(
                tool_use_id, tool_name,
                &format!(
                    "Path '{}' is outside the allowed working directories. \
                     Allowed: {} (and {} additional directories)",
                    file_path_str,
                    cwd.display(),
                    perm_ctx.additional_working_directories.len()
                ),
                started,
            );
        }
    }
}
```

- 依赖: `permissions::path_validation::validate_file_path()` 和 `is_path_within_allowed_directories()` 已完整实现
- cwd 从 `bootstrap::PROCESS_STATE.read().original_cwd` 获取
- `ToolPermissionContext` 从 `ctx.get_app_state().tool_permission_context` 获取
- Bypass 模式跳过此检查

### 2.4 Bypass 模式豁免

整个 Stage 3c 在 `PermissionMode::Bypass` 下跳过:

```rust
let app_state = (ctx.get_app_state)();
let mode = &app_state.tool_permission_context.mode;
if *mode != PermissionMode::Bypass {
    // 3c.1, 3c.2, 3c.3 ...
}
```

理由: Bypass 模式是用户明确选择的"跳过所有安全检查"，语义上等同于 Codex 的 `danger-full-access`。

---

## 3. 文件变更清单

| 文件 | 变更 |
|------|------|
| `src/tools/execution.rs` | 在 Stage 3b 之后插入 `security_validate()` 函数调用 |
| `src/permissions/dangerous.rs` | 移除 `#![allow(unused)]` |
| `src/permissions/path_validation.rs` | 移除 `#![allow(dead_code)]` |

**不需要修改的文件:**
- 各工具的 `check_permissions()` — 安全检查集中在 execution.rs
- `ToolUseContext` — 通过 `get_app_state` 回调获取所需数据
- `ProcessState` — 已有 `original_cwd`

---

## 4. 测试计划

### 4.1 单元测试 (在 execution.rs 中)

| 测试 | 预期 |
|------|------|
| `test_plan_mode_blocks_write_tools` | Plan 模式下 Write/Edit/Bash 被拦截 |
| `test_plan_mode_allows_read_tools` | Plan 模式下 Read/Grep/Glob 放行 |
| `test_dangerous_command_blocked` | `rm -rf /` 被拦截，返回 reason |
| `test_safe_command_allowed` | `ls -la` 放行 |
| `test_path_outside_cwd_blocked` | `/tmp/evil.sh` 写入被拦截 |
| `test_path_within_cwd_allowed` | `src/main.rs` 写入放行 |
| `test_path_traversal_blocked` | `/../../../etc/passwd` 被拦截 |
| `test_bypass_mode_skips_all` | Bypass 模式下所有安全检查跳过 |

### 4.2 集成验证

- `cargo check` 无新 warning
- 现有 `permissions::dangerous` 和 `permissions::path_validation` 测试仍通过
- `cargo test` 全量通过

---

## 5. 不做的事

- **不修改各工具的 `check_permissions()`** — 安全逻辑集中管理
- **不添加 OS 级沙盒** — P2 范围
- **不添加网络限制** — P2 范围
- **不修改 `ToolUseContext` 结构体** — 通过已有回调获取数据
