---
title: Phase 2 删除记录 — workspace suppression debt 清理
date: 2026-05-20
source: development/archive/plan/workspace-all-targets-warning-budget-plan-2026-05-20.md
---

# Phase 2 删除记录

Phase 2 清理了长期 `allow(dead_code|unused_imports|unused)` 债务，共删除 5 个未使用函数、2 个 struct 字段、1 个 struct + impl 块，移除 2 个文件级 allow。

## cc-tools/src/hooks

### `get_hook_default_timeout` (hook_helpers.rs:71-79)

```rust
/// Get the default timeout for a hook entry type.
#[allow(unused)]
pub fn get_hook_default_timeout(hook: &HookEntry) -> u64 {
    match hook {
        HookEntry::Command { .. } => 60,
        HookEntry::Prompt { .. } => 30,
        HookEntry::Agent { .. } => 60,
        HookEntry::Http { .. } => 600,
    }
}
```

- **原因**：从未被外部调用，是上游 TypeScript 同名函数的未接线端口。
- **恢复方式**：必要时从 git 历史恢复，或在 hooks runtime 接线时重新实现。

### `emit_hook_progress` (hook_events.rs:80-101)

```rust
#[allow(unused)]
pub fn emit_hook_progress(
    hook_id: &str,
    hook_name: &str,
    hook_event: &HookEvent,
    stdout: &str,
    stderr: &str,
    output: &str,
) {
    if !should_emit(hook_event) {
        return;
    }
    emit(HookExecutionEvent::Progress(HookProgressEvent {
        hook_id: hook_id.to_string(),
        hook_name: hook_name.to_string(),
        hook_event: hook_event.to_string(),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        output: output.to_string(),
    }));
}
```

- **原因**：仅被 `HookEventEmitter::start_progress_interval` 调用，而后者及其宿主均未被外部引用。
- **恢复方式**：整条链（`HookEventEmitter` + `start_progress_interval` + `emit_hook_progress`）已在同一提交中删除；需要时从 git 历史统一恢复。

### `HookEventEmitter` + `start_progress_interval` (hook_events.rs:138-193)

```rust
pub struct HookEventEmitter;

impl HookEventEmitter {
    pub fn start_progress_interval(
        hook_id: String,
        hook_name: String,
        hook_event: HookEvent,
        get_output: Box<dyn Fn() -> Option<(String, String, String)> + Send + 'static>,
        interval_ms: u64,
    ) -> Box<dyn Fn() + Send> { ... }
}
```

- **原因**：`HookEventEmitter` 未在任何地方构造或引用；其唯一方法 `start_progress_interval` 无人调用。
- **恢复方式**：与 `emit_hook_progress` 一并从 git 历史恢复。

## cc-query/src/deps.rs

### `ModelResponse.stream_events` + `ModelResponse.usage`

```rust
pub struct ModelResponse {
    pub assistant_message: AssistantMessage,
    // Missing implementation reserve: streaming events are returned by deps but
    // the query loop has not yet wired them into downstream observability.
    #[allow(dead_code)]
    pub stream_events: Vec<StreamEvent>,
    // Missing implementation reserve: usage is preserved for the model-call
    // contract until token accounting consumes it from ModelResponse.
    #[allow(dead_code)]
    pub usage: Usage,
}
```

- **原因**：`stream_events` 从未被读取；`usage` 在 `ModelResponse` 级别的存储与 `AssistantMessage.usage` 重复，后者已是生产路径使用的来源。测试中 `resp.usage` 引用已迁移到 `resp.assistant_message.usage`。
- **恢复方式**：如果需要将 `stream_events` 接入 observability，需在 `loop_impl.rs` 中实际读取该字段而非仅赋值。

## cc-engine/src/lifecycle/submit_message.rs

### `build_attachment_content_blocks` (submit_message.rs:1667-1685)

```rust
#[allow(dead_code)]
pub fn build_attachment_content_blocks(
    attachments: &[crate::input_processing::AttachmentInfo],
) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    for attachment in attachments {
        if let Some(block) = &attachment.content_block {
            blocks.push(block.clone());
        } else if let Some(path) = &attachment.file_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                blocks.push(ContentBlock::Text { text: content });
            }
        }
    }
    blocks
}
```

### `build_submit_progress_message` (submit_message.rs:1688-1699)

```rust
#[allow(dead_code)]
pub fn build_submit_progress_message(prompt_len: usize) -> Message {
    Message::Progress(crate::types::message::ProgressMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        tool_use_id: "submit".to_string(),
        data: serde_json::json!({
            "event": "user_prompt_submit",
            "prompt_len": prompt_len,
        }),
    })
}
```

### `submit_preprocessed_input` (submit_message.rs:1708-1741)

```rust
#[allow(dead_code)]
pub fn submit_preprocessed_input(
    mut preprocessed: crate::input_processing::ProcessedInput,
) -> crate::input_processing::ProcessedInput {
    if !preprocessed.attachments.is_empty() {
        let blocks = build_attachment_content_blocks(&preprocessed.attachments);
        if let Some(Message::User(ref mut user)) = preprocessed.messages.first_mut() {
            // ... merge blocks into user message ...
        }
    }
    preprocessed
}
```

- **原因**：三个函数构成一条未接线的 TUI/headless 提交管道入口。`submit_preprocessed_input` 是唯一外部入口，但无人调用；它内部调用了 `build_attachment_content_blocks`；`build_submit_progress_message` 则完全孤立。
- **恢复方式**：当 TUI 或 headless 前端需要 `ProcessedInput → submit pipeline` 通路时，从 git 历史恢复三个函数。

## cc-ipc-protocol

### 文件级 `#![allow(dead_code)]` (subsystem_types.rs:13)

- **删除结果**：该文件所有类型已被 `subsystem_events.rs`、`protocol/mod.rs`、`protocol/subsystem.rs` 或 `#[cfg(test)]` 模块充分使用，移除文件级 allow 后无 warning。

### 文件级 `#![allow(dead_code)]` (subsystem_events.rs:12)

- **删除结果**：该文件所有类型已被外部消费者或测试使用，移除文件级 allow 后无 warning。
