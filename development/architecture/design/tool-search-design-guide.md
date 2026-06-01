---
title: "工具搜索与选择设计 - Brief 模式与工具注册"
description: "详解 allthecodes 的工具搜索与选择设计：基于 allthecodes-tools crate 的 Tool trait 和 BriefTool 实现，包括工具注册、启用条件、只读属性、输入验证、调用与提示。"
keywords: ["工具搜索", "Tool trait", "BriefTool", "工具注册", "工具选择", "Brief 模式"]
---

## 概述

allthecodes 的工具系统基于 `allthecodes-tools` crate 的 `Tool` trait 实现。每个工具（Read、Write、Bash、Brief 等）都实现该 trait，系统通过统一的接口注册、筛选和调用工具。

## Tool Trait

`Tool` trait 是工具系统的基础抽象，定义了每个工具必须实现的方法：

| 方法 | 返回类型 | 描述 |
|------|---------|------|
| `name()` | `&str` | 工具的唯一名称标识 |
| `description()` | `String` | 工具的描述信息 |
| `input_json_schema()` | `Value` | 工具输入的 JSON Schema |
| `is_enabled()` | `bool` | 工具是否在当前配置下可用 |
| `is_read_only()` | `bool` | 工具是否为只读操作 |
| `is_concurrency_safe()` | `bool` | 工具是否安全支持并发调用 |
| `validate_input()` | `ValidationResult` | 校验输入参数的有效性 |
| `call()` | `Result<ToolResult>` | 执行工具的核心逻辑 |
| `prompt()` | `String` | 返回工具的 system prompt 描述 |

## BriefTool：结构化输出工具

`brief.rs` 实现的 `BriefTool` 是 Brief 模式下的唯一通信渠道。当 `FEATURE_KAIROS_BRIEF` 启用时，模型在 Brief 模式下只能通过 BriefTool 与用户通信。

### 启用条件

```rust
fn is_enabled(&self) -> bool {
    features::enabled(Feature::KairosBrief)
}
```

当 Feature Flag `FEATURE_KAIROS_BRIEF` 未设置时，BriefTool 不被注册到可用工具列表中。这体现了 Feature Flag 系统对工具可见性的控制。

### 输入 Schema

BriefTool 接受三个参数：

- **message**（必需）— Markdown 格式的文本消息
- **attachments**（可选）— 文件路径数组，用于附带文件
- **status**（可选）— 消息状态：`"normal"`（默认）或 `"proactive"`

### 输入校验

`validate_input` 执行两层校验：
1. `message` 不能为空
2. `status` 必须是 `"normal"` 或 `"proactive"`

校验失败返回 `ValidationResult::Error`，带错误码和描述。

### 只读与并发

```rust
fn is_read_only(&self, _input: &Value) -> bool { true }
fn is_concurrency_safe(&self, _input: &Value) -> bool { true }
```

BriefTool 被标记为只读和并发安全，因此可以在 Plan 模式下正常使用，且不会触发写操作权限检查。

### Prompt 注入

BriefTool 的 `prompt()` 返回注入 AI 的系统提示，强调：

> Brief 是 Brief 模式下与用户通信的唯一方式。纯文本输出被视为内部推理，不会显示给用户。

## 工具搜索与选择机制

虽然 `brief.rs` 本身只定义单个工具，但从中可以归纳出 allthecodes 的通用工具搜索与选择模式：

### 1. 注册阶段
每个工具实现 `Tool` trait 后注册到工具注册表。`is_enabled()` 控制工具是否出现在可用列表中，由 Feature Flag 或配置决定。

### 2. 筛选阶段
系统根据当前模式（Plan/Auto/Default 等）和权限规则筛选可用工具：
- Plan 模式下只读工具优先
- deny 规则匹配的工具被移除
- 权限模式影响 Ask/Allow 决策

### 3. 调用阶段
工具调用经过完整的权限决策流程（Permission Decision Flow）：
- Phase 1：Hook deny + 规则匹配
- Phase 2：Hook ask/allow
- Phase 3：会话级授权
- Phase 4：模式兜底

### 4. 结果阶段
工具返回 `ToolResult`，包含执行结果数据，可选的新消息列表，以及进度状态。

## 设计要点

- **Feature Flag 门控**：工具的启用与 Feature Flag 绑定，实现功能渐进式发布
- **只读声明**：工具声明 `is_read_only` 影响 Plan 模式的自动放行策略
- **并发安全声明**：`is_concurrency_safe` 影响工具是否可并行执行
- **Prompt 注入**：工具的 `prompt()` 方法在 system prompt 中描述自己的使用方式，指导模型行为
- **校验分离**：`validate_input` 在 `call` 之前执行，提前拒绝无效输入
