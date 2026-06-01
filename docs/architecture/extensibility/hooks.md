---
title: "Hooks 生命周期钩子 - Hook 系统架构与执行引擎"
description: "从源码角度解析 allthecodes Hook 系统：Hook 事件类型、ShellHookRunner 执行引擎、Hook 输出协议、pre-tool/post-tool 拦截、SSRF 防护和异步 Hook 实现。"
keywords: ["Hooks", "生命周期钩子", "ShellHookRunner", "PreToolUse", "PostToolUse", "Hook 协议"]
---

## Hook 系统概述

Hook 系统位于 `allthecodes-tools/src/hooks/`，是用户定义的事件响应机制。用户在 `settings.json` 的 `hooks` 字段中配置 shell 命令，在特定事件发生时自动执行。

核心模块：

| 文件 | 职责 |
|------|------|
| `mod.rs` | `ShellHookRunner` 实现 `HookRunner` trait |
| `execution.rs` | Shell 命令执行引擎（子进程 spawn + I/O + 超时） |
| `hook_events.rs` | Hook 执行事件广播系统 |
| `pre_tool.rs` | PreToolUse 钩子运行 |
| `post_tool.rs` | PostToolUse / PostToolUseFailure / Stop 钩子运行 |
| `http_hook.rs` | HTTP 钩子执行 |
| `ssrf_guard.rs` | SSRF 地址拦截 |
| `async_registry.rs` | 异步 Hook 注册与完成管理 |

## ShellHookRunner：执行引擎

`ShellHookRunner` 是对 `HookRunner` trait 的具体实现，所有方法委托给 hooks 模块中的自由函数：

```rust
impl HookRunner for ShellHookRunner {
    fn load_hook_configs(&self, hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig>;
    async fn run_pre_tool_hooks(&self, tool_name, input, hook_configs) -> Result<PreToolHookResult>;
    async fn run_post_tool_hooks(&self, tool_name, input, result_data, hook_configs) -> Result<PostToolHookResult>;
    async fn run_post_tool_failure_hooks(&self, tool_name, input, error, hook_configs) -> Result<()>;
    async fn run_event_hooks(&self, event_name, payload, hook_configs) -> Result<HookOutput>;
    async fn run_stop_hooks(&self, hook_configs) -> Result<PostToolHookResult>;
}
```

## Hook 事件类型

目前支持的 Hook 事件包括（定义在 `allthecodes-types::hooks`）：

| 事件 | 触发时机 |
|------|---------|
| `SessionStart` | 会话启动 |
| `Setup` | 初始化完成 |
| `PreToolUse` | 工具调用前（可拦截） |
| `PostToolUse` | 工具调用成功后 |
| `PostToolUseFailure` | 工具调用失败后 |
| `Stop` | Agent 停止响应 |
| `StopFailure` | Agent 停止失败 |
| `PermissionRequest` | 权限请求 |
| `PermissionDenied` | 权限被拒绝 |
| `FileChanged` | 文件变更（Read/Edit/Write 触发） |
| `TaskCreated` | 任务创建 |
| `TaskCompleted` | 任务完成 |

## 执行引擎：execute_command_hook

`execution.rs` 的 `execute_command_hook()` 是 Hook 命令执行的核心：

### 执行流程

```
1. spawn_shell_command() — 创建子进程
   ├── Unix: bash -c "{command}" (process_group(0) 便于超时后 kill 整个组)
   └── Windows: 优先 bash，降级到 cmd /C
2. stdin 写入 JSON 输入行（如工具名称和输入参数）
3. 并发读取 stdout / stderr + 等待子进程退出
4. 超时控制（默认 timeout_secs）
5. 解析 stdout 首行 JSON → HookOutput
```

### 超时与进程组杀

Unix 下使用进程组 kill 确保 Shell 和其所有子进程都被终止：

```rust
#[cfg(unix)]
if let Some(pid) = child.id() {
    let process_group = -(pid as libc::pid_t);
    unsafe { libc::kill(process_group, libc::SIGKILL) };
}
```

这是因为 Hook 命令通过 `bash -c` 运行，可能产生孙子进程（如 `sleep 60`），只杀 Shell 不能保证所有子进程终止。

### Shell 选择

```rust
fn spawn_shell_command(command: &str, shell_override: Option<&str>) -> Result<Child> {
    let shell_program = shell_override.unwrap_or("bash");
    Command::new(shell_program).arg("-c").arg(command)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn()
}
```

Unix 使用 `bash`（可配置），Windows 使用 bash 或 cmd /C。

## Hook 输出协议

Hook 进程通过 stdout 的第一行 `{}` JSON 输出与控制引擎通信：

```json
{
  "continue": true,
  "stop_reason": "optional reason",
  "permission_decision": "allow" | "deny" | "ask",
  "updated_input": { "command": "modified command" },
  "additional_context": "extra context string",
  "system_message": "system warning message"
}
```

关键字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `continue` | bool | 是否继续执行（默认为 true） |
| `stop_reason` | string | `continue=false` 时的原因 |
| `permission_decision` | string | 权限决策：allow/deny/ask |
| `updated_input` | object | 修改后的工具输入 |
| `additional_context` | string | 注入到对话的额外上下文 |
| `system_message` | string | 注入到系统的警告信息 |

### 非 JSON 输出

如果 stdout 首行不是 `{` 开头，视为纯文本输出：

```rust
if first_line.trim_start().starts_with('{') {
    serde_json::from_str::<HookOutput>(first_line)
} else {
    HookOutput { additional_context: Some(trimmed.to_string()), ..Default::default() }
}
```

纯文本输出被注入为附加上下文（`additional_context`）。

## Pre-Tool 拦截

`pre_tool.rs` 的 `run_pre_tool_hooks()` 在工具调用前执行：

1. 加载匹配的 Hook 配置（按事件名称和 tool_name 过滤器）
2. 串行执行每个 Hook 命令
3. 收集 `permission_decision`、`updated_input` 等结果
4. 如果任何 Hook 设置 `continue=false`，工具执行被阻止

## Post-Tool 处理

`post_tool.rs` 提供以下功能：

- `run_event_hooks()` — 通用事件 Hook 执行（如 FileChanged、TaskCreated）
- `run_post_tool_failure_hooks()` — 工具失败时触发
- `run_stop_hooks()` — 停止事件 Hook

## Hook 事件广播

`hook_events.rs` 实现了独立于主消息流的 Hook 执行事件系统：

```rust
pub fn register_hook_event_handler(handler: Option<Box<dyn Fn(HookExecutionEvent) + Send>>);
pub fn emit_hook_started(hook_id: &str, hook_name: &str, hook_event: &HookEvent);
pub fn emit_hook_response(hook_id, hook_name, hook_event, output, stdout, stderr, exit_code, outcome);
```

- 始终发射的事件：`SessionStart`、`Setup`
- 其他事件需要 `set_all_hook_events_enabled(true)` 开启
- 支持事件队列（最大 100 个未处理事件）：handler 注册前的事件会被暂存，注册后自动转发
- 这允许 hook 数据的延迟消费，在 handler 就绪前不会丢失事件

## SSRF 防护

`ssrf_guard.rs` 的 `SsrfGuard` 和 `is_blocked_address()` 阻止 Hook 命令向内部网络地址发起请求，防止服务端请求伪造攻击。

## 异步 Hook

`async_registry.rs` 管理异步 Hook 的生命周期：

- `register_pending_async_hook()` — 注册一个异步 Hook
- `complete_async_hook()` — 标记异步 Hook 完成
- `check_for_async_hook_responses()` — 检查是否有完成的异步 Hook
- `clear_all_async_hooks()` — 清除所有异步 Hook

## HTTP Hook

`http_hook.rs` 的 `exec_http_hook()` 支持通过 HTTP 请求触发的 Hook（替代 shell 命令执行）。

## Hook 配置格式

在 `settings.json` 中的配置示例：

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Write|Edit",
        "command": "check-path.sh"
      }
    ],
    "PostToolUse": [
      {
        "command": "notify.sh"
      }
    ]
  }
}
```

每个事件可以配置多个 Hook 条目（`HookEventConfig`），每个条目可以包含：
- `matcher` — 可选，匹配规则（精确匹配或 `|` 分隔的多值匹配）
- `command` — 要执行的 shell 命令
- `timeout` — 超时时间（秒）
