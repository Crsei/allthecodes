# P1 执行计划 — 使系统端到端可用

> 目标: 完成 4 个 P1 任务后，Rust 版本可以真实调用 Anthropic API、执行工具、处理 hooks、压缩上下文。

---

## 目录

1. [任务总览与依赖图](#1-任务总览)
2. [P1.1 API 客户端接入 Anthropic API](#2-p11-api-客户端)
3. [P1.2 Hooks 真实执行](#3-p12-hooks-真实执行)
4. [P1.3 tool_result_budget 完成 async I/O](#4-p13-tool_result_budget)
5. [P1.4 /compact 命令接 API 压缩](#5-p14-compact-命令)
6. [验收标准](#6-验收标准)

---

## 1. 任务总览

```
依赖图:

  P1.3 tool_result_budget ──────────────────────────────────┐
       (独立, 无依赖)                                        │
                                                             ▼
  P1.1 API 客户端 ──→ P1.4 /compact 命令 ──→ [端到端可用]
       │
       └──→ QueryEngineDeps.call_model() 不再 bail!
            engine/lifecycle.rs 可驱动真实对话

  P1.2 Hooks 真实执行 ──────────────────────────────────────→ [端到端可用]
       (独立, 无依赖)
```

| 任务 | 文件 | 预估新增行数 | 依赖 | 建议顺序 |
|------|------|-------------|------|---------|
| P1.1 API 客户端 | `api/client.rs` | ~250 | `network` feature | 第 1 步 |
| P1.2 Hooks 执行 | `tools/hooks.rs` | ~350 | 无 | 第 1 步 (可并行) |
| P1.3 tool_result_budget | `compact/tool_result_budget.rs` | ~30 | 无 | 第 1 步 (可并行) |
| P1.4 /compact 命令 | `commands/compact.rs` + `engine/` | ~120 | P1.1 | 第 2 步 |

**建议执行**: P1.1 + P1.2 + P1.3 并行 → P1.4。

---

## 2. P1.1 API 客户端接入 Anthropic API

### 2.1 当前状态

- `api/client.rs`: `ApiClient` struct 存在，`messages_stream()` 和 `messages()` 均 `bail!("not yet implemented")`
- `api/streaming.rs`: SSE 解析器 `parse_sse_event()` + `StreamAccumulator` **已完整实现**
- `api/retry.rs`: 错误分类 + 退避计算 **已完整实现**
- `api/providers.rs`: Provider trait + 3 家提供商 URL 构建 **已完整实现**
- `engine/lifecycle.rs`: `QueryEngineDeps.call_model()` 返回 bail，阻断端到端流程

### 2.2 实现方案

#### 文件: `src/api/client.rs`

**改动范围**: 替换 `messages_stream()` 和 `messages()` 的 `bail!` 为真实 HTTP 调用。

```rust
// messages_stream() 实现伪代码:
pub async fn messages_stream(&self, request: MessagesRequest)
    -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>
{
    let url = self.build_url();
    let headers = self.build_headers();
    let body = serde_json::to_string(&request)?;

    let response = self.http
        .post(&url)
        .headers(headers)
        .body(body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await?;
        let category = retry::categorize_api_error(status, &body);
        anyhow::bail!("API error {}: {}", status, body);
    }

    // 将 response body 转为 SSE Stream
    let byte_stream = response.bytes_stream();
    let event_stream = parse_sse_stream(byte_stream);  // 新函数
    Ok(Box::pin(event_stream))
}
```

**需要新增的功能**:

| 函数/方法 | 职责 | 行数 |
|----------|------|------|
| `build_url(&self) -> String` | 根据 provider 构建请求 URL | ~20 |
| `build_headers(&self) -> HeaderMap` | 构建请求头 (api-key, version, beta, content-type) | ~30 |
| `parse_sse_stream(bytes) -> Stream<StreamEvent>` | 将 bytes 流转为 SSE 事件流 | ~60 |
| `messages_stream()` 实现 | 发送 HTTP 请求 + 错误处理 | ~50 |
| `messages()` 实现 | 非流式: 调用流式然后收集 | ~30 |
| 重试包装 `with_retry()` | 封装重试逻辑到 call_model | ~40 |

**SSE 流解析**: HTTP response body 是 `text/event-stream` 格式:

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_...","usage":{...}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
```

解析逻辑: 按 `\n\n` 分割事件 → 提取 `event:` 和 `data:` 行 → 调用已有的 `streaming::parse_sse_event()`。

**请求头**:

```
content-type: application/json
anthropic-version: 2023-06-01
x-api-key: {api_key}
anthropic-beta: interleaved-thinking-2025-05-14,prompt-caching-2024-07-16
```

#### 文件: `src/engine/lifecycle.rs`

**改动范围**: `QueryEngineDeps.call_model()` 接入真实 `ApiClient`。

```rust
// QueryEngineDeps 新增字段:
struct QueryEngineDeps {
    aborted: Arc<AtomicBool>,
    app_state: Arc<RwLock<AppState>>,
    tools: Arc<RwLock<Tools>>,
    api_client: Option<Arc<ApiClient>>,  // 新增
}

// call_model 实现:
async fn call_model(&self, params: ModelCallParams) -> Result<ModelResponse> {
    let client = self.api_client.as_ref()
        .ok_or_else(|| anyhow::anyhow!("no API client configured"))?;

    let request = build_messages_request(&params);
    let mut accumulator = StreamAccumulator::new();
    let mut stream_events = Vec::new();

    let stream = client.messages_stream(request).await?;
    pin_mut!(stream);

    while let Some(event) = stream.next().await {
        let event = event?;
        accumulator.process_event(&event);
        stream_events.push(event.clone());
    }

    let assistant = accumulator.build();
    Ok(ModelResponse {
        assistant_message: assistant,
        stream_events,
        usage: accumulator.usage,
    })
}
```

### 2.3 测试策略

| 测试 | 方法 |
|------|------|
| SSE 解析 | 单元测试: 构造 SSE 文本 → 验证 StreamEvent |
| Header 构建 | 单元测试: 验证所有必要头存在 |
| 错误处理 | 单元测试: 模拟 429/500/413 响应 → 验证分类 |
| 端到端 | 集成测试 (需 API key): 发送 "say hi" → 收到文本回复 |

---

## 3. P1.2 Hooks 真实执行

### 3.1 当前状态

- `tools/hooks.rs`: 类型定义完整 (`PreToolHookResult`, `PostToolHookResult`, `PermissionOverride`)，但所有 `run_*` 函数返回固定值
- `config/settings.rs`: `GlobalConfig.hooks` 字段已存在 (`Option<HashMap<String, Value>>`)

### 3.2 Hook 配置格式

settings.json 中的 hooks 配置:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "python3 /path/to/validate.py"
          }
        ]
      }
    ],
    "PostToolUse": [...],
    "Stop": [...]
  }
}
```

### 3.3 实现方案

#### 文件: `src/tools/hooks.rs` — 重写

**新增类型**:

```rust
/// Hook 配置 (从 settings.json 解析)
#[derive(Debug, Clone, Deserialize)]
pub struct HookConfig {
    pub matcher: Option<String>,
    pub hooks: Vec<HookEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum HookEntry {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default = "default_timeout")]
        timeout: u64,
    },
}

/// Hook 子进程的 JSON 输出
#[derive(Debug, Deserialize)]
#[serde(default)]
struct HookOutput {
    #[serde(rename = "continue")]
    should_continue: bool,
    stop_reason: Option<String>,
    decision: Option<String>,       // "approve" | "block"
    reason: Option<String>,

    // PreToolUse 特有
    permission_decision: Option<String>,  // "allow" | "deny"
    updated_input: Option<Value>,
    additional_context: Option<String>,
}
```

**核心执行函数**:

```rust
/// 执行单个 command hook
async fn execute_command_hook(
    command: &str,
    stdin_data: &Value,
    timeout_secs: u64,
    env: HashMap<String, String>,
) -> Result<HookOutput> {
    let mut child = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(&env)
        .spawn()?;

    // 写 stdin
    if let Some(mut stdin) = child.stdin.take() {
        let data = serde_json::to_string(stdin_data)?;
        stdin.write_all(data.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
    }

    // 带超时等待
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        child.wait_with_output(),
    ).await??;

    // 解析输出
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hook_output(&stdout)
}
```

**run_pre_tool_hooks 改造**:

```rust
pub async fn run_pre_tool_hooks(
    tool_name: &str,
    input: &Value,
    hook_configs: &[HookConfig],    // 新参数: 从 settings 传入
) -> Result<PreToolHookResult> {
    let matching = hook_configs.iter()
        .filter(|c| matches_tool(c.matcher.as_deref(), tool_name));

    let stdin = json!({
        "tool_name": tool_name,
        "tool_input": input,
    });

    for config in matching {
        for entry in &config.hooks {
            match entry {
                HookEntry::Command { command, timeout } => {
                    let output = execute_command_hook(
                        command, &stdin, *timeout, HashMap::new(),
                    ).await?;

                    if !output.should_continue {
                        return Ok(PreToolHookResult::Stop {
                            message: output.stop_reason.unwrap_or_default(),
                        });
                    }
                    if let Some(decision) = &output.permission_decision {
                        match decision.as_str() {
                            "allow" => return Ok(PreToolHookResult::Continue {
                                updated_input: output.updated_input,
                                permission_override: Some(PermissionOverride::Allow),
                            }),
                            "deny" => return Ok(PreToolHookResult::Continue {
                                updated_input: None,
                                permission_override: Some(PermissionOverride::Deny {
                                    reason: output.reason.unwrap_or_default(),
                                }),
                            }),
                            _ => {}
                        }
                    }
                    if output.updated_input.is_some() {
                        return Ok(PreToolHookResult::Continue {
                            updated_input: output.updated_input,
                            permission_override: None,
                        });
                    }
                }
            }
        }
    }

    Ok(PreToolHookResult::Continue {
        updated_input: None,
        permission_override: None,
    })
}
```

**函数签名变更影响**:

`run_pre_tool_hooks` 新增 `hook_configs` 参数 → 需更新以下调用方:
- `tools/execution.rs` → `run_tool_use()` 需从 `ToolUseContext` 获取 hook configs
- `tools/orchestration.rs` → `execute_tool_call()` 同上

**推荐方案**: 在 `ToolUseContext` 或 `AppState` 中添加 `hook_configs: Vec<HookConfig>` 字段。

### 3.4 需新增的功能清单

| 函数 | 职责 | 行数 |
|------|------|------|
| `HookConfig` + `HookEntry` 类型 | 配置反序列化 | ~30 |
| `HookOutput` 类型 | 子进程输出解析 | ~20 |
| `execute_command_hook()` | 子进程执行 + 超时 + 输出收集 | ~60 |
| `parse_hook_output()` | JSON/plain text 输出解析 | ~30 |
| `matches_tool()` | matcher 模式匹配工具名 | ~15 |
| `load_hooks_from_settings()` | 从 settings.json 加载 hooks | ~20 |
| `run_pre_tool_hooks()` 重写 | 完整逻辑 | ~60 |
| `run_post_tool_hooks()` 重写 | 完整逻辑 | ~40 |
| `run_stop_hooks()` 重写 | 完整逻辑 | ~40 |
| 测试 | 5-8 个 | ~80 |

### 3.5 子进程环境变量

```
CLAUDE_PROJECT_DIR=/path/to/cwd
TOOL_NAME=Bash
TOOL_INPUT={"command":"ls"}
SESSION_ID=xxx
```

### 3.6 测试策略

| 测试 | 方法 |
|------|------|
| 配置解析 | 单元测试: JSON → HookConfig |
| 输出解析 | 单元测试: JSON stdout → HookOutput |
| matcher | 单元测试: "Bash" 匹配 "Bash", 不匹配 "Read" |
| 超时 | 单元测试: 子进程 sleep 超时 → 返回错误 |
| 集成 | 集成测试: echo hook → 验证 PreToolHookResult |

---

## 4. P1.3 tool_result_budget 完成 async I/O

### 4.1 当前状态

`compact/tool_result_budget.rs` 已有完整逻辑，但 `apply_tool_result_budget()` 是 `async fn`，其调用方 `compact/pipeline.rs` 的 `run_context_pipeline()` 是同步函数，因此跳过了 tool_result_budget 步骤。

### 4.2 实现方案

**改动 1**: `pipeline.rs` → `run_context_pipeline()` 改为 `async fn`

```rust
pub async fn run_context_pipeline(
    messages: Vec<Message>,
    tracking: Option<AutoCompactTracking>,
    model: &str,
    session_id: &str,
) -> PipelineResult {
    // Step 1: Tool result budget (async I/O)
    let mut state = tool_result_budget::ContentReplacementState::default();
    let current = tool_result_budget::apply_tool_result_budget(
        messages, &mut state, 100_000,
    ).await;
    // ... rest unchanged
}
```

**改动 2**: 更新 `query/deps.rs` 的 `microcompact()` 签名 (如果 pipeline 集成了 budget)

**影响**: `compact/pipeline.rs` + `query/loop_impl.rs` (如果调用了 pipeline)

**实际行数**: ~30 行改动。

---

## 5. P1.4 /compact 命令接 API 压缩

### 5.1 当前状态

`commands/compact.rs` 返回 "Compaction is not available without an API connection"。

### 5.2 实现方案

**依赖**: P1.1 完成后，`ApiClient` 可用。

**改动**: `CompactHandler.execute()` 调用压缩管线:

```rust
async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    if ctx.messages.is_empty() {
        return Ok(CommandResult::Output("Nothing to compact.".into()));
    }

    // 1. 构建压缩提示词
    let prompt = compaction::build_compaction_prompt();
    let custom = if args.is_empty() { None } else { Some(args) };

    // 2. 提取需要压缩的消息
    let messages_to_compact = compact_messages::get_messages_after_compact_boundary(&ctx.messages);

    // 3. 构建摘要请求
    let conversation_text = format_messages_for_summary(&messages_to_compact);
    let summary_prompt = format!(
        "{}\n\n{}\n\n{}",
        prompt,
        custom.map(|c| format!("Additional instructions: {}", c)).unwrap_or_default(),
        conversation_text,
    );

    // 4. 调用模型生成摘要 (需要 API client)
    // 这里需要访问 API client — 通过 CommandContext 传入或全局状态
    // Phase 1 简化: 使用本地摘要 (截断前 N 条消息)
    let summary = generate_local_summary(&messages_to_compact);

    // 5. 构建 post-compact 消息
    let config = CompactionConfig {
        model: ctx.app_state.main_loop_model.clone(),
        session_id: String::new(),
        query_source: "compact".into(),
    };
    let post_messages = compaction::build_post_compact_messages(
        &summary, &ctx.messages, &config,
    );

    // 6. 替换消息
    let pre_tokens = tokens::estimate_messages_tokens(&ctx.messages);
    let post_tokens = tokens::estimate_messages_tokens(&post_messages);

    Ok(CommandResult::Output(format!(
        "Compacted: {} → {} tokens ({} messages → {} messages)",
        pre_tokens, post_tokens,
        ctx.messages.len(), post_messages.len(),
    )))
}
```

**改动范围**:
- `commands/compact.rs`: ~80 行重写
- `commands/mod.rs`: CommandContext 可能需要新增 api_client 字段 (后续接入)

---

## 6. 验收标准

### 6.1 P1.1 API 客户端

- [ ] `cargo test` 编译通过
- [ ] 单元测试: SSE 流解析正确 (message_start → content_block_delta → message_stop)
- [ ] 单元测试: 请求头包含 `x-api-key`, `anthropic-version`, `content-type`
- [ ] 单元测试: 错误分类 (429 → RateLimit, 413 → PromptTooLong)
- [ ] `QueryEngineDeps.call_model()` 不再 bail
- [ ] 集成测试 (手动): `ANTHROPIC_API_KEY=sk-... cargo run -- -p "say hi"` 输出文本

### 6.2 P1.2 Hooks

- [ ] 配置加载: 从 settings.json 读取 hooks 配置
- [ ] 子进程执行: `echo '{"continue":true}' | run_pre_tool_hooks` 返回 Continue
- [ ] 超时: 超时子进程正确终止
- [ ] 权限覆盖: `permission_decision: "deny"` → PreToolHookResult 携带 Deny
- [ ] 输入修改: `updated_input: {...}` → PreToolHookResult 携带新输入
- [ ] 停止: `continue: false` → PreToolHookResult::Stop
- [ ] plain text fallback: 非 JSON 输出 → 作为消息文本处理

### 6.3 P1.3 tool_result_budget

- [ ] pipeline 异步化: `run_context_pipeline` 为 async fn
- [ ] 大结果持久化: >100K 字符的工具结果保存到磁盘
- [ ] 预览生成: 保留 head 500 + tail 200 字符 + 文件路径引用

### 6.4 P1.4 /compact 命令

- [ ] 空对话: 返回 "Nothing to compact"
- [ ] 有对话: 返回压缩统计 (前后 token 数, 消息数)
- [ ] 自定义指令: `/compact focus on code` 将指令传入摘要

### 6.5 端到端验收

```bash
# 1. 编译
cargo build

# 2. 基本对话
ANTHROPIC_API_KEY=sk-... cargo run -- -p "What is 2+2?"
# 预期: 输出包含 "4"

# 3. 工具使用
ANTHROPIC_API_KEY=sk-... cargo run -- -p "List files in /tmp"
# 预期: 工具调用 Bash → 返回文件列表

# 4. 多轮对话 (REPL)
ANTHROPIC_API_KEY=sk-... cargo run
> What is Rust?
# 预期: 输出 Rust 介绍
> /compact
# 预期: 压缩统计
```

---

## 附录: 关键文件变更矩阵

| 文件 | P1.1 | P1.2 | P1.3 | P1.4 |
|------|------|------|------|------|
| `api/client.rs` | **重写** | | | |
| `engine/lifecycle.rs` | 改动 call_model | | | |
| `tools/hooks.rs` | | **重写** | | |
| `tools/execution.rs` | | 签名适配 | | |
| `types/tool.rs` | | 新增字段 | | |
| `compact/tool_result_budget.rs` | | | 无变化 | |
| `compact/pipeline.rs` | | | **async 化** | |
| `commands/compact.rs` | | | | **重写** |
| `config/settings.rs` | | hooks 加载 | | |
| `Cargo.toml` | | | | |
