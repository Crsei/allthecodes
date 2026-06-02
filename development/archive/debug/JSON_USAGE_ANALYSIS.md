# rust-lite JSON 使用分析

> 统计时间: 2026-04-07 | 基于 132 个 .rs 源文件

## 总览

| 类别 | 数量 | 涉及文件 |
|---|---|---|
| **序列化调用** (`to_string`/`to_value`/`to_vec`) | 22 | 17 |
| **反序列化调用** (`from_str`/`from_value`/`from_slice`) | 29 | 15 |
| **签名含 `serde_json::Value` 的函数** | ~124 | 30+ |
| **`json!` 宏调用** | 240 | 27 |
| **`#[derive(Serialize, Deserialize)]` 类型** | 37 | 19 |

**合计：约 415 处 JSON 使用点，分布在 ~50 个文件中。**

---

## 按模块分布

| 模块 | `json!` 数量 | 序列化/反序列化 | 说明 |
|---|---|---|---|
| `tools/` (13 个工具) | ~140 | 3 / 9 | 每个工具通过 `Value` 接收输入、构造返回结果 |
| `api/` (流式/多 provider) | ~44 | 4 / 8 | SSE 解析、请求体构建 (OpenAI/Google/Anthropic) |
| `session/` (持久化) | ~22 | 5 / 8 | 会话存储、转录、迁移 (v1→v2→v3) |
| `engine/` (引擎) | ~14 | 4 / 0 | SDK 消息序列化、系统提示词 JSON schema |
| `config/` (配置) | ~4 | 0 / 2 | 全局/项目配置文件读写 |
| `auth/` (认证) | 0 | 2 / 2 | Token 持久化 |
| `services/` (服务) | 0 | 1 / 1 | SessionMemory 配置 |
| `query/` (查询循环) | ~8 | 0 / 0 | loop_impl + stop_hooks 构造消息 |
| `permissions/` | ~2 | 0 / 0 | 权限判定结果 |

---

## json! 宏 Top 文件

| 文件 | 调用次数 |
|---|---|
| `api/openai_compat.rs` | 26 |
| `tools/file_read.rs` | 21 |
| `tools/structured_output.rs` | 16 |
| `api/google_provider.rs` | 15 |
| `tools/orchestration.rs` | 15 |
| `tools/config_tool.rs` | 14 |
| `session/migrations.rs` | 12 |
| `tools/hooks.rs` | 12 |
| `tools/repl.rs` | 12 |
| `tools/skill.rs` | 12 |
| `tools/powershell.rs` | 10 |
| `tools/file_edit.rs` | 9 |
| `engine/lifecycle.rs` | 7 |
| `tools/ask_user.rs` | 6 |
| `tools/grep.rs` | 6 |

---

## 核心数据链路

```
API 响应 (JSON SSE)
  → serde_json::from_str → Value
    → ContentBlock/AssistantMessage (Deserialize)
      → 工具输入 (&Value)
        → 工具结果 json!({...})
          → ToolResult { data: Value }
            → 会话持久化 serde_json::to_string_pretty
```

`serde_json::Value` 是整个系统的**通用数据传递类型**——工具系统的输入输出、API 请求响应、会话存储全部依赖它。

---

## 关键 Serialize/Deserialize 类型 (37 个)

### 消息/内容

| 类型 | 文件 | 用途 |
|---|---|---|
| `ContentBlock` | `types/message.rs` | API 内容块 (Text/ToolUse/ToolResult/Thinking) |
| `AssistantMessage` | `types/message.rs` | 助手响应消息 |
| `Usage` | `types/message.rs` | Token 用量统计 |
| `StructuredOutput` | `types/message.rs` | JSON 格式化输出 |
| `ImageSource` | `types/message.rs` | 图片来源 |

### 会话持久化

| 类型 | 文件 | 用途 |
|---|---|---|
| `SessionFile` | `session/storage.rs` | 完整会话文件 |
| `SessionInfo` | `session/storage.rs` | 会话摘要信息 |
| `SerializableMessage` | `session/storage.rs` | 持久化消息 (含 `Value` 字段) |
| `MemoryEntry` | `session/memdir.rs` | 会话记忆条目 |

### 配置/认证

| 类型 | 文件 | 用途 |
|---|---|---|
| `GlobalConfig` | `config/settings.rs` | 全局配置 |
| `ProjectConfig` | `config/settings.rs` | 项目级配置 |
| `StoredToken` | `auth/token.rs` | API Token 存储 |
| `OAuthTokens` | `auth/mod.rs` | OAuth 令牌 |

### API/SDK

| 类型 | 文件 | 用途 |
|---|---|---|
| `MessagesRequest` | `api/client.rs` | API 请求体 |
| `SystemInitMessage` | `engine/sdk_types.rs` | SDK 初始化消息 |
| `SdkAssistantMessage` | `engine/sdk_types.rs` | SDK 助手消息 |
| `SdkResult` | `engine/sdk_types.rs` | SDK 查询结果 |
| `SdkStreamEvent` | `engine/sdk_types.rs` | SDK 流式事件 |

### 引擎/技能

| 类型 | 文件 | 用途 |
|---|---|---|
| `SessionId` | `bootstrap/ids.rs` | 品牌类型会话 ID |
| `UsageTracking` | `engine/lifecycle.rs` | 用量追踪 |
| `SkillFrontmatter` | `skills/mod.rs` | 技能元数据 |
| `SkillSource` | `skills/mod.rs` | 技能来源 |

---

## Value 签名函数代表示例

### 工具系统 (~80 个函数)

```rust
// tools/bash.rs
fn parse_input(input: &Value) -> (String, u64, Option<String>)

// tools/orchestration.rs
fn execute_single_tool(..., input: &Value) -> ToolExecResult

// 每个 Tool::call() 实现都接受 &Value 输入
```

### API 请求构建 (~15 个函数)

```rust
// api/google_provider.rs
fn build_gemini_request(request: &MessagesRequest) -> Value

// api/openai_compat.rs
fn build_openai_request(request: &MessagesRequest, provider: &str) -> Value
```

### 会话/迁移 (~15 个函数)

```rust
// session/migrations.rs
pub fn migrate_to_current(data: Value) -> Result<(Value, Vec<String>)>
fn migrate_v1_to_v2(data: Value) -> Result<Value>
fn migrate_v2_to_v3(data: Value) -> Result<Value>

// session/export.rs
fn render_content_block_from_json(block: &Value, md: &mut String)
fn extract_user_text_from_data(data: &Value) -> String
```

### 流式解析 (~10 个函数)

```rust
// api/streaming.rs
fn parse_content_block_delta(data: &Value) -> Option<StreamEvent>
fn parse_message_start(data: &Value) -> Option<StreamEvent>
```
