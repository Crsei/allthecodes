# Langfuse 集成改进方案

> 基于 `langfuse-ccb.md`（理想设计）与 `langfuse-runtime-status.md`（运行状态）的差距分析。
> 最后更新: 2026-05-30

---

## 总览

本文档将全部改进项拆分为可按阶段执行的修改方案。每个方案包含：修改目标、涉及文件、代码变更要点、验收标准。

---

## 阶段一：高优先级（P0）— 正确性与安全性

### P0-1：工具输入脱敏按工具类型区分

**现状**：`sanitize_tool_input()` 接受 `tool_name` 参数但完全忽略（`let _ = tool_name`），所有工具输入走同一套全局脱敏。

**理想**：文件类工具的 `file_path`/`path`/`directory` 字段应做 Home 目录替换；敏感工具应全遮蔽。

**涉及文件**（两处需同步修改）：

| 模块 | 文件 |
|---|---|
| engine | `crates/allthecodes-engine/src/services/langfuse/sanitize.rs` |
| services | `crates/allthecodes-services/src/langfuse/sanitize.rs` |

**代码变更**：

```diff
- pub fn sanitize_tool_input(tool_name: &str, input: &Value) -> Value {
-     let _ = tool_name;
-     sanitize_global_value(input)
- }
+ pub fn sanitize_tool_input(tool_name: &str, input: &Value) -> Value {
+     // 敏感工具 → 完全遮蔽输入
+     if REDACTED_SENSITIVE_TOOLS.contains(&tool_name) {
+         return Value::String(format!("[{} input redacted]", tool_name));
+     }
+ 
+     // 文件工具 → 对路径字段做 Home 目录替换 + 全局脱敏
+     if REDACTED_FILE_TOOLS.contains(&tool_name) {
+         return sanitize_file_tool_input(input);
+     }
+ 
+     // Shell 工具及其他 → 全局脱敏
+     sanitize_global_value(input)
+ }
+ 
+ fn sanitize_file_tool_input(input: &Value) -> Value {
+     match input {
+         Value::Object(map) => {
+             let mut result = Map::new();
+             for (key, value) in map {
+                 let key_lower = key.to_ascii_lowercase();
+                 if PATH_FIELDS.contains(&key_lower.as_str()) {
+                     // 路径字段 → Home 目录替换
+                     if let Value::String(s) = value {
+                         result.insert(key.clone(), Value::String(replace_home_dir(s)));
+                     } else {
+                         result.insert(key.clone(), sanitize_global_value(value));
+                     }
+                 } else {
+                     result.insert(key.clone(), sanitize_global_value(value));
+                 }
+             }
+             Value::Object(result)
+         }
+         other => sanitize_global_value(other),
+     }
+ }
+ 
+ const PATH_FIELDS: &[&str] = &["file_path", "path", "directory", "output_path", "input_path"];
```

同时新增测试：

```rust
#[cfg(test)]
mod tests {
    // ... 已有测试 ...

    #[test]
    fn sanitize_tool_input_redacts_sensitive_tools() {
        let input = json!({"command": "ls"});
        let result = sanitize_tool_input("Config", &input);
        assert!(result.as_str().unwrap().contains("Config input redacted"));
    }

    #[test]
    fn sanitize_tool_input_replaces_home_dir_in_path() {
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let input = json!({"file_path": format!("{}/secret.txt", home)});
        let result = sanitize_tool_input("Read", &input);
        assert!(result["file_path"].as_str().unwrap().contains("~/secret.txt"));
    }

    #[test]
    fn sanitize_tool_input_ignores_non_path_fields() {
        let input = json!({"content": "visible", "mode": "write"});
        let result = sanitize_tool_input("Write", &input);
        assert_eq!(result["content"], "visible");
    }
}
```

**验收标准**：

- [ ] `sanitize_tool_input("Config", ...)` 返回 `[Config input redacted]`
- [ ] `sanitize_tool_input("Read", {"file_path": "/Users/foo/x"})` 中路径被替换为 `~/x`
- [ ] `sanitize_tool_input("Bash", {"command": "ls"})` 正常全局脱敏
- [ ] 新测试全部通过
- [ ] engine 和 services 两处代码同步修改

---

### P0-2：补充集成测试

**现状**：仅有 6 个内联单元测试，无端到端测试覆盖。

**涉及文件**（新建）：

```text
crates/allthecodes-services/src/langfuse/tests/
├── mod.rs                  # 测试模块入口
├── sanitize_integration.rs # 脱敏策略全覆盖
├── convert_integration.rs  # 消息格式转换全覆盖
└── export_integration.rs   # mock OTLP receiver 集成测试
```

**测试覆盖范围**：

**sanitize_integration.rs**：

```rust
/// 脱敏策略全覆盖
#[cfg(test)]
mod tests {
    use super::*;

    /// 1. 文件工具：全遮蔽，保留字符数
    #[test]
    fn file_tool_output_fully_redacted() {
        for tool in &["Read", "Write", "Edit", "MultiEdit"] {
            let output = sanitize_tool_output(tool, "secret content 12345");
            assert!(output.contains("file content redacted"), "{}", tool);
            assert!(output.contains("14 chars"), "{} should report char count", tool);
        }
    }

    /// 2. 敏感工具：全遮蔽，含工具名
    #[test]
    fn sensitive_tool_output_fully_redacted() {
        for tool in &["Config", "MCP"] {
            let output = sanitize_tool_output(tool, "anything");
            assert!(output.contains(&format!("[{} output redacted", tool)), "{}", tool);
        }
    }

    /// 3. Shell 工具：截断至 500 字符
    #[test]
    fn shell_tool_output_truncated() {
        let long = "a".repeat(600);
        let output = sanitize_tool_output("Bash", &long);
        assert!(output.contains("[truncated]"));
        assert!(output.chars().count() <= 500 + "[truncated]".len());
    }

    /// 4. 其他工具：仅全局脱敏，40K 截断
    #[test]
    fn other_tool_global_sanitize_only() {
        let long = "a".repeat(41_000);
        let output = sanitize_tool_output("CustomTool", &long);
        assert!(output.contains("[truncated]"));
    }

    /// 5. 敏感 Key 脱敏
    #[test]
    fn sensitive_keys_redacted_in_object() {
        let value = json!({"api_key": "sk-xxx", "token": "abc", "safe": "ok"});
        let sanitized = sanitize_global_value(&value);
        assert_eq!(sanitized["api_key"], "[REDACTED]");
        assert_eq!(sanitized["token"], "[REDACTED]");
        assert_eq!(sanitized["safe"], "ok");
    }

    /// 6. Home 目录替换
    #[test]
    fn home_dir_replaced() {
        if let Some(home) = dirs::home_dir() {
            let s = format!("{}/.config/file", home.to_string_lossy());
            assert_eq!(replace_home_dir(&s), "~/.config/file");
        }
    }

    /// 7. 超大输出截断边界
    #[test]
    fn output_under_max_len_not_truncated() {
        let s = "a".repeat(39_000);
        assert_eq!(sanitize_global_string(&s).len(), 39_000);
    }

    /// 8. 非 ASCII 字符的字符数统计
    #[test]
    fn unicode_char_count() {
        let output = sanitize_tool_output("Read", "你好世界");
        assert!(output.contains("4 chars"));
    }
}
```

**convert_integration.rs**：

```rust
/// 消息格式转换全覆盖
#[cfg(test)]
mod tests {
    use super::*;

    /// 1. 完整的 submit → generation → end 全链路
    #[test]
    fn full_conversion_roundtrip() {
        // 构建包含 system + user + assistant (with tool_use) + user (tool_result) 的消息序列
        let messages = build_complex_message_sequence();
        let converted = convert_generation_input(&messages, &["You are a helpful assistant."], &Tools::default());
        let msgs = converted["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[3]["role"], "tool");
    }

    /// 2. all ContentBlock types covered
    #[test]
    fn all_content_block_types_handled() {
        let message = Message::Assistant(AssistantMessage {
            content: vec![
                ContentBlock::Text { text: "hello".into() },
                ContentBlock::Thinking { thinking: "deep thought".into(), signature: None, .. },
                ContentBlock::RedactedThinking { .. },
                ContentBlock::Image { source: MediaSource::Base64 { media_type: "image/png".into(), data: "abc".into() } },
                ContentBlock::ToolUse { id: "t1".into(), name: "Bash".into(), input: json!({}) },
                ContentBlock::ServerToolUse { .. },
            ],
            ..
        });
        let result = convert_assistant_output(&message);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("[thinking redacted]"));
        assert!(content.contains("[image omitted]"));
        assert!(content.contains("[server tool use omitted]"));
        assert!(result["tool_calls"].as_array().unwrap().len() == 1);
    }

    /// 3. ToolResult with Blocks → omitted
    #[test]
    fn tool_result_blocks_omitted() {
        let message = Message::User(UserMessage {
            content: MessageContent::Blocks(vec![
                ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: ToolResultContent::Blocks(vec![]),
                    is_error: false,
                },
            ]),
            ..
        });
        let converted = convert_generation_input(&[message], &[], &Tools::default());
        let msg = &converted["messages"][0];
        assert_eq!(msg["content"], "[complex tool result omitted]");
    }

    /// 4. Empty messages
    #[test]
    fn empty_messages() {
        let converted = convert_generation_input(&[], &[], &Tools::default());
        assert!(converted["messages"].as_array().unwrap().is_empty());
    }

    /// 5. Tools conversion
    #[test]
    fn tool_definitions_converted() {
        // Verify tool names and schemas appear correctly
    }
}
```

**mock OTLP receiver 集成测试**（`export_integration.rs`）：

```rust
/// 使用 mock OTLP HTTP receiver 验证实际 span 导出
#[cfg(test)]
mod tests {
    // 方案：启动一个本地 HTTP 服务（mock OTLP receiver），
    // 配置 LANGFUSE_BASE_URL=localhost:port，然后执行真实 trace 创建和结束，
    // 验证 receiver 收到的 span 内容符合预期。
    //
    // 使用 otlp-http 协议而非 gRPC，减少依赖。
    // 可选的测试，需要用 #[cfg(feature = "telemetry_integration_tests")] 条件编译，
    // 日常 CI 中默认跳过。
}
```

**验收标准**：

- [ ] 新增 8+ sanitize 测试全部通过
- [ ] 新增 5+ convert 测试全部通过
- [ ] 所有 ContentBlock 类型在 convert 中有测试覆盖
- [ ] mock OTLP receiver 可以接收并验证 span 内容
- [ ] 测试不依赖外部网络或真实 Langfuse 实例

---

### P0-3：两套 Langfuse 模块收敛（因架构约束的部分收敛）

**现状**：engine crate 和 services crate 各有几乎相同的 langfuse 模块（共 12 个文件），因为 services 依赖 engine（`allthecodes-services → allthecodes-engine`），engine 不能反向依赖 services。

**收敛策略**：不能简单删除一方，但可以做三件事减少重复：

**策略 A：提取公共 crate `allthecodes-langfuse`（推荐）**

新建 `crates/allthecodes-langfuse/`，将三份无外部 crate 依赖的逻辑移入：

| 公共模块 | 来源 | 说明 |
|---|---|---|
| `sanitize.rs` | engine/services 公共 | 脱敏逻辑，无 crate 依赖 |
| `convert.rs` | engine/services 公共 | 消息格式转换，依赖 `allthecodes-types` |
| `stub.rs` | engine/services 公共 | no-op stub，几乎相同 |

不提取 `tracing.rs` 和 `client.rs`，因为它们依赖 `opentelemetry` 和 `tracing-opentelemetry`（这两个 crate 都拉了，但跨 crate 类型签名难统一；保留在各自有 feature gate 下更简单）。

**代码变更**：

```diff
--- a/crates/Cargo.toml (workspace)
+++ b/crates/Cargo.toml
@@ ... @@ members = [
+    "allthecodes-langfuse",
```

```toml
# crates/allthecodes-langfuse/Cargo.toml
[package]
name = "allthecodes-langfuse"
version.workspace = true
edition.workspace = true

[dependencies]
allthecodes-types = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
dirs = { workspace = true }

[features]
default = []
telemetry = []  # 用于 future 扩展；目前均为 pure logic
```

```diff
--- a/crates/allthecodes-engine/src/services/langfuse/mod.rs
+++ b/crates/allthecodes-engine/src/services/langfuse/mod.rs
- pub mod convert;
- pub mod sanitize;
+ // Re-export from shared crate
+ pub use allthecodes_langfuse::convert;
+ pub use allthecodes_langfuse::sanitize;
```

```diff
--- a/crates/allthecodes-services/src/langfuse/mod.rs
+++ b/crates/allthecodes-services/src/langfuse/mod.rs
- pub mod convert;
- pub mod sanitize;
+ // Re-export from shared crate
+ pub use allthecodes_langfuse::convert;
+ pub use allthecodes_langfuse::sanitize;
```

删除 `engine/src/services/langfuse/convert.rs`、`engine/src/services/langfuse/sanitize.rs`、`services/src/langfuse/convert.rs`、`services/src/langfuse/sanitize.rs`（共 4 个文件）。

**策略 B（备选）**：engine 的 langfuse 模块保持现状，只把 sanitize 和 convert 抽到 `allthecodes-types` 或新建模块。如果提取公共 crate 的跨 workspace 迁移成本高，可以先只统一 `sanitize.rs`（无外部依赖），convert.rs 稍后。

**验收标准**：

- [ ] `cargo check -p allthecodes-engine --features telemetry` 通过
- [ ] `cargo check -p allthecodes-services --features telemetry` 通过
- [ ] `cargo check -p allthecodes` 通过（无 feature）
- [ ] 两处代码不再各自维护 sanitize 和 convert 的副本
- [ ] sanitize 测试在新位置通过

---

## 阶段二：中优先级（P1）— 数据完整性与可观测性

### P1-1：TTFT 存入 Langfuse OTel 语义字段

**现状**：`ttft_ms` 被存入 `OBSERVATION_METADATA_ATTR`（metadata JSON 中的 `"ttftMs"`），不是 Langfuse OTel 原生字段。

**Langfuse OTel 语义**：Langfuse 的 OTel 桥接层在 `generation` type span 上应读取以下字段：
- `langfuse.observation.completion_start_time`（可选）— 对于流式响应，第一个 token 产生的时间
- 或者使用 OTel span 的 `Event` 机制记录时间点

但实际上 `opentelemetry-langfuse` exporter 目前对 generation span 的语义字段映射有限。**在确认 exporter 是否支持该字段前，在 metadata 中保留 `ttftMs` 同时，增加冗余写入 OTel Event**。

**涉及文件**：

| 模块 | 文件 |
|---|---|
| engine | `crates/allthecodes-engine/src/services/langfuse/tracing.rs` |
| services | `crates/allthecodes-services/src/langfuse/tracing.rs` |

**代码变更**：

```diff
 pub fn finish_generation_span(
     ...
     ttft_ms: Option<u64>,
     ...
 ) {
     ...
     if let Some(usage) = usage {
-        span.span.set_attribute(
-            OBSERVATION_METADATA_ATTR,
-            metadata_json(vec![
-                ("ttftMs", ttft_ms.map(|v| json!(v)).unwrap_or(Value::Null)),
-                ...
-            ]),
-        );
+        let mut metadata_entries = vec![
+            ("cacheReadInputTokens", json!(usage.cache_read_input_tokens)),
+            ("cacheCreationInputTokens", json!(usage.cache_creation_input_tokens)),
+        ];
+        if let Some(ttft) = ttft_ms {
+            metadata_entries.push(("ttftMs", json!(ttft)));
+            // 也写入 OTel span event 以便 Langfuse exporter 在未来版本中利用
+            span.span.add_event(
+                "completion_start".to_string(),
+                vec![KeyValue::new("ttft_ms", ttft as i64)],
+            );
+        }
+        span.span.set_attribute(
+            OBSERVATION_METADATA_ATTR,
+            metadata_json(metadata_entries),
+        );
     }
 }
```

**验收标准**：

- [ ] `ttftMs` 保留在 metadata JSON 中（向后兼容）
- [ ] 有 TTFT 时，span 上新增 `completion_start` event
- [ ] 无 TTFT 时不产生多余 event
- [ ] 所有现有测试通过

---

### P1-2：thinking 内容格式对齐

**现状**：`ContentBlock::Thinking` / `RedactedThinking` 转为字符串 `"[thinking redacted]"`，混入 assistant content body。

**理想**：CCB 设计文档要求按独立的 `{type: 'thinking', thinking: '...'}` 块映射。但 Langfuse 的 OpenAI 兼容 input/output 格式**不原生支持** `type: thinking`（OpenAI API 没有 thinking 块）。因此实际保留 `"[thinking redacted]"` 字符串内嵌方案，但需要在设计文档中注明原因。

**涉及文件**：

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/docs/archive/cache/langfuse-ccb.md`

**代码变更**（无）：当前实现是正确的。只需要更新设计文档。

**设计文档更新**：

```diff
- thinking / redacted_thinking → { type: 'thinking', thinking }
+ thinking / redacted_thinking → "[thinking redacted]" 字符串
+   // 注：Langfuse 的 input/output 格式基于 OpenAI 兼容格式，不支持
+   // 独立的 type:thinking 块。所有 thinking 内容均被脱敏为占位字符串。
```

**验收标准**：

- [ ] `langfuse-ccb.md` 中 thinking 映射部分与实际代码一致
- [ ] 添加注释说明原因

---

## 阶段三：低优先级（P2）— 文档与运维

### P2-1：Provider 映射表同步

**现状**：

| CCB 设计文档 | 实际代码 |
|---|---|
| `firstParty` → ChatAnthropic | `anthropic` → ChatAnthropic |
| `foundry` → ChatFoundry | `azure-foundry` → （未映射，走 `_ -> ChatModel`） |
| `azure` 未列 | `azure` → ChatAzureOpenAI |
| `grok` → ChatXAI | 无此 provider |
| `openai-codex` 未列 | `openai-codex` → ChatOpenAI |

**涉及文件**：

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/docs/archive/cache/langfuse-ccb.md`

**代码变更**（可选）：确认是否需要补充 `azure-foundry` 的显式映射：

```diff
 fn generation_name(provider: &str) -> &str {
     match provider {
         "anthropic" => "ChatAnthropic",
         "bedrock" => "ChatBedrockAnthropic",
         "vertex" => "ChatVertexAnthropic",
+        "azure-foundry" | "microsoft-foundry" => "ChatAzureOpenAI",
+        "foundry" => "ChatAzureOpenAI",
         "azure" => "ChatAzureOpenAI",
         ...
```

**验收标准**：

- [ ] `langfuse-ccb.md` 的 Provider 映射表与代码一致
- [ ] （可选）添加 `azure-foundry` 等遗漏 provider 映射

---

### P2-2：补充运维检查清单和使用指南

**涉及文件**：

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/docs/archive/cache/langfuse-runtime-status.md`

**在运行状态文档末尾追加内容**：

```markdown
## 运维检查清单

### 启用前确认
- [ ] `--features telemetry` 包含在 build 命令中
- [ ] `LANGFUSE_PUBLIC_KEY` 已设置且非空
- [ ] `LANGFUSE_SECRET_KEY` 已设置且非空
- [ ] 自部署时 `LANGFUSE_BASE_URL` 已指向正确地址

### 启动确认
- [ ] 启动日志无 `langfuse init failed` 警告

### 运行时确认
- [ ] Trace 名称在 Langfuse UI 中可见：`agent-run:xxx`
- [ ] Generation span 名称正确（ChatAnthropic / ChatOpenAI 等）
- [ ] Tool span 显示工具名称和执行结果
- [ ] Token 用量数据正确（input/output/cache tokens）
- [ ] Session ID 可用于聚合查看

### 退出确认
- [ ] 程序 exit 时无 `failed to shutdown langfuse` 警告

### Langfuse Dashboard 使用指南
- 按 Session 聚合：在 Langfuse UI 中使用 Session 筛选器输入 session_id
- 按 Trace Name 过滤：搜索 `agent-run:` 前缀
- 按 Tag 过滤：Tags 包括 `allthecodes`、`submit`、`subagent`、agent_type 等
- 查看 TTFT：在 Generation span 的 Metadata JSON 中查找 `ttftMs` 字段

### 异常排查流程
1. 确认 feature 已开启：`cargo check -p allthecodes --features telemetry 2>&1 | grep Finished`
2. 确认 key 已设置：`echo $LANGFUSE_PUBLIC_KEY | head -c 8`
3. 检查 exporter 初始化：启动日志中搜索 `init_langfuse`
4. 网络连通性：`curl -v $LANGFUSE_BASE_URL/api/public/health`
5. 退出时 flush：添加 `shutdown_langfuse()` 调用（已在 main.rs 中）
6. 检查 batch 参数：flush_at/flush_interval 是否匹配实际请求频率
```

**验收标准**：

- [ ] 运维检查清单完整
- [ ] Dashboard 使用指南包含实际可操作的搜索和过滤方法
- [ ] 异常排查步骤落实到具体命令

---

### P2-3：补充自部署文档

**涉及文件**：

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/docs/archive/cache/langfuse-runtime-status.md`

**在运行状态文档末尾追加内容**：

```markdown
## 自部署 Langfuse（Docker Compose 最小配置）

```yaml
version: "3.8"
services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: langfuse
      POSTGRES_USER: langfuse
      POSTGRES_PASSWORD: changeme
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U langfuse"]
      interval: 5s

  langfuse:
    image: langfuse/langfuse:latest
    ports:
      - "3000:3000"
    environment:
      DATABASE_URL: postgresql://langfuse:changeme@postgres:5432/langfuse
      NEXTAUTH_SECRET: random-secret
      NEXTAUTH_URL: http://localhost:3000
      SALT: random-salt
    depends_on:
      postgres:
        condition: service_healthy

volumes:
  pgdata:
```

启动后：
1. 注册管理员账号
2. Project Settings → API Keys 获取 `LANGFUSE_PUBLIC_KEY` / `LANGFUSE_SECRET_KEY`
3. 设置 `LANGFUSE_BASE_URL=http://localhost:3000`
4. 构建并运行

```

**验收标准**：

- [ ] Docker Compose 配置可正常启动 Langfuse
- [ ] 文档中包含密钥获取步骤

---

### P2-4：更新 `langfuse-ccb.md` 与代码同步

将设计文档中所有与实际代码不一致的表述更新为同步状态（已在上述各 P 项中指定具体修改位置）。

**汇总修改清单** `langfuse-ccb.md` 中需修改的条目：

| 条目 | 当前内容 | 应改为 |
|---|---|---|
| Provider `firstParty` | ChatAnthropic | `anthropic`（实际使用的 provider 字符串） |
| Provider `foundry` | ChatFoundry | `azure-foundry` → ChatAzureOpenAI（或 fallback ChatModel）|
| Provider `grok` | ChatXAI | 删除（代码中不存在） |
| Provider `azure` | 未列 | 添加 `azure` → ChatAzureOpenAI |
| Provider `openai-codex` | 未列 | 添加 `openai-codex` → ChatOpenAI |
| 消息格式 thinking 映射 | `{ type: 'thinking', thinking }` | 字符串 `[thinking redacted]`（附原因说明）|
| 模块结构 | 单模块 | 注明 services 和 engine 各有一份（已标记会收敛）|

---

## 执行路线图

```mermaid
gantt
    title Langfuse 改进执行路线图
    dateFormat  YYYY-MM-DD
    axisFormat  %m-%d
    
    section P0 高优
    工具输入脱敏按类型区分           :p0-1, 2026-06-02, 2d
    补充集成测试（sanitize + convert） :p0-2a, after p0-1, 2d
    mock OTLP 集成测试             :p0-2b, after p0-2a, 3d
    提取公共 langfuse crate        :p0-3, 2026-06-02, 3d
    收敛 engine/services 模块       :p0-3b, after p0-3, 1d
    
    section P1 中优
    TTFT 增加 OTel Event           :p1-1, 2026-06-09, 1d
    设计文档同步（thinking 部分）  :p1-2, 2026-06-09, 0.5d
    
    section P2 低优
    Provider 映射表同步            :p2-1, 2026-06-10, 0.5d
    运维检查清单 + 使用指南        :p2-2, 2026-06-10, 0.5d
    自部署文档补充                  :p2-3, 2026-06-10, 0.5d
    langfuse-ccb.md 全量同步        :p2-4, 2026-06-11, 0.5d
```

## 影响范围总结

| 文件 | 操作 | 对应 P 项 |
|---|---|---|
| `engine/.../sanitize.rs` | 修改 `sanitize_tool_input`，新增 `sanitize_file_tool_input`、`PATH_FIELDS` | P0-1 |
| `services/.../sanitize.rs` | 同上同步修改 | P0-1 |
| `engine/.../sanitize.rs` | 新增测试函数 | P0-1 |
| `services/.../sanitize.rs` | 新增测试函数 | P0-1 |
| `services/.../tests/*.rs` | 新建集成测试目录和文件 | P0-2 |
| `engine/.../convert.rs` | 删除（移到公共 crate），新增 re-export | P0-3 |
| `services/.../convert.rs` | 删除（移到公共 crate），新增 re-export | P0-3 |
| `engine/.../sanitize.rs` | 删除（移到公共 crate），新增 re-export | P0-3 |
| `services/.../sanitize.rs` | 删除（移到公共 crate），新增 re-export | P0-3 |
| `crates/allthecodes-langfuse/src/` | 新建公共 crate | P0-3 |
| `engine/.../tracing.rs` | 修改 `finish_generation_span` 增加 OTel Event | P1-1 |
| `services/.../tracing.rs` | 同上同步修改 | P1-1 |
| `docs/.../langfuse-ccb.md` | 更新 thinking 映射、Provider 表、模块结构 | P1-2, P2-1, P2-4 |
| `docs/.../langfuse-runtime-status.md` | 补充运维清单、Dashboard 指南、自部署文档 | P2-2, P2-3 |
| `crates/allthecodes-services/.../tracing.rs` | 补充 `azure-foundry` 等 provider 映射（可选） | P2-1 |
