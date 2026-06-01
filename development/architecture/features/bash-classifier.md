---
title: "Bash 分类器"
description: "基于 LLM 的 Shell 命令安全分类器，用于自动模式下 Bash/PowerShell 工具的 allow/deny/ask 决策。"
keywords: ["bash", "classifier", "安全", "safety", "自动模式", "权限"]
---

## 概述

Bash 分类器（Safety Classifier）是 allthecodes 自动模式（Auto Mode）的核心安全组件。当用户在自动模式下执行 Bash 或 PowerShell 命令时，分类器使用独立的 LLM 调用对命令进行安全评估，输出 `allow`（允许）、`deny`（拒绝）或 `ask`（询问用户）三种判决。

分类器运行在 `allthecodes-safety` crate 中，通过 `allthecodes-shell-command` crate 的解析能力理解命令结构。

## 架构

### 两阶段判决

分类器采用 **Fast → Thinking 两阶段流水线**：

1. **Fast 阶段**：使用低 effort 设置（`effort=low`）的快速 LLM 调用，对命令进行初步安全分类
2. **Thinking 阶段**：当 Fast 阶段结果为 `Ask`、高风险命令或需要升级时，使用更高 effort（`effort=medium`）的 Thinking 模型进行二次评估

判决流程：
- Fast 返回 `Deny` → 直接拒绝，无需 Thinking
- Fast 返回 `Allow` 且非高风险 → 直接允许（低延迟路径）
- Fast 返回 `Allow` 但命令高风险 → 升级到 Thinking 阶段
- Fast 返回 `Ask` 或无法决策 → 升级到 Thinking 阶段
- 分类器不可用 → 安全降级返回 `Ask`

### 高风险命令检测

高风险命令通过 `is_high_risk()` 方法检测，委托给 `allthecodes-permissions` 的 `dangerous` 模块：

```rust
fn is_high_risk(&self) -> bool {
    match self.tool_name.as_str() {
        "Bash" => is_dangerous_command(&command).is_some(),
        "PowerShell" => is_dangerous_powershell_command(&command).is_some(),
        _ => false,
    }
}
```

高风险命令即使 Fast 阶段判定 `Allow`，仍然会升级到 Thinking 阶段。

## 分类器 Prompt 结构

系统提示定义了分类器的角色和行为约束：

```
You are the cc-rust safety classifier for tool use.
Return exactly one JSON object and no prose.
Allowed verdicts are "allow", "deny", and "ask".
Use "allow" only when the tool use is clearly safe...
Use "deny" for destructive, credential-exposing... actions.
Use "ask" for ambiguity, missing context, or cases needing user confirmation.
```

用户提示包含以下上下文：
- `purpose` — 分类目的（auto_mode_tool_use / plan_allowed_prompt）
- `stage` — 当前阶段（Fast / Thinking）
- `permission_mode` — 权限模式
- `sandbox_mode` — 沙箱模式
- `cwd` — 工作目录
- `tool_name` — 工具名（Bash / PowerShell）
- `tool_input` — 工具输入
- `tool_classifier_input` — 分类器专用输入
- `auto_mode_policy` — 自动模式策略（environment / allow / soft_deny）
- `hook_context` — Hook 上下文
- `recent_transcript` — 最近的对话转录窗口

输出要求严格 JSON 格式：`{"verdict":"allow|deny|ask","reason":"short reason","thinking":"optional","escalate":false}`

## Shell 命令解析

`allthecodes-shell-command` crate 为分类器提供命令结构理解能力：

### 解析模式

- **Permissive**（宽松模式）：用于显示和非安全路径，解析失败时回退到 shell-words 分割
- **FailClosedSecurity**（安全关闭模式）：严格解析用于安全决策，未终止引用、格式错误的 token 或解析失败时直接拒绝

### 模型

`ParsedShellCommand` 是核心数据结构，包含：
- `original` — 原始命令字符串
- `segments` — 通过 `&&`、`||`、`;`、`|` 分割的命令段
- `diagnostics` — 解析诊断信息（警告/错误）

每个 `ShellSegment` 包含：
- `SimpleCommand` — 命令名、参数、环境变量
- `Redirection` — 重定向操作符和目标
- `Heredoc` — heredoc 规范
- `is_pipeline` — 是否为管道成员

### Shell 方言

支持 `Bash`、`Zsh`、`Fish`、`PowerShell`、`Cmd`、`Sh` 六种 Shell 方言。

## 安全机制

### 秘密值脱敏

分类器在构建 Prompt 之前对转录文本和工具输入进行脱敏处理，防止 API key、token、密码等敏感信息泄露给分类模型：

- 正则匹配 `api_key`、`token`、`secret`、`password` 等常见秘密字段名
- 替换 `sk-*`、`sk-ant-*`、`AIza*` 等已知密钥格式
- JSON 对象中的秘密字段值被替换为 `<redacted>`

### 预算控制

```
max_transcript_bytes: 48KB  — 转录窗口大小限制
max_prompt_bytes: 64KB      — 总 Prompt 大小限制
max_estimated_tokens: 16000 — 预估 token 上限
```

超出预算时安全降级，返回 `transcript_too_long` 状态。

### Transcript 窗口选择

从后向前选择消息，优先保留最近的内容。超出字节预算的旧消息被省略，并在 Prompt 中标记 `[N older message(s) omitted]`。

## 判决类型

```
Allow — 命令安全，可以直接执行
Deny  — 命令危险或违反策略，直接拒绝
Ask   — 模棱两可或需要用户确认，提示用户决策
```

当分类器不可用时（模型错误、格式错误响应、转录过长），统一降级为 `Ask` + `unavailable` 标记。

## 使用方式

```bash
# 自动模式下启用分类器（默认行为）
# 分类器在 auto_mode_tool_use 时自动触发

# 配置自动模式策略（settings.json）
# auto_mode_policy: { environment: [...], allow: [...], soft_deny: [...] }
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-safety/src/classifier.rs` | 分类器核心逻辑：两阶段判决、Prompt 构建、响应解析 |
| `crates/allthecodes-shell-command/src/model.rs` | 命令解析 DTO：ParsedShellCommand、ShellDialect |
| `crates/allthecodes-shell-command/src/bash_ast.rs` | Bash AST 解析器（tree-sitter） |
| `crates/allthecodes-shell-command/src/fallback.rs` | 回退解析器（shell-words 分割） |
| `crates/allthecodes-shell-command/src/provider.rs` | ShellProvider 抽象 |
| `crates/allthecodes-permissions/src/dangerous.rs` | 高风险命令检测 |
