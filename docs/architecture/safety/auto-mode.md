---
title: "自动模式 - AI 自动决策的权限机制"
description: "详解 allthecodes Auto Mode 的实现：基于 allthecodes-safety classifier 的两阶段分类器、危险规则剥离、拒绝追踪与 fallback、转录窗口管理。"
keywords: ["Auto Mode", "classifier", "两阶段", "denial tracker", "自动模式"]
---

## 概述

Auto 模式是 Allow/Ask/Deny 权限体系中的一种权限模式。启用后，AI 工具调用无需用户逐条确认，由分类器自动裁决。

这相当于让模型在限定范围内拥有自主执行权。系统通过多层安全护栏确保自动范围可控：危险规则自动剥离、两阶段分类器精确裁决、拒绝追踪防止死循环、机密信息脱敏防止泄露。

## 权限模式的兜底行为

在 `rules.rs` 的 `check_tool_permission` 中，`PermissionMode::Auto` 的兜底行为是 **Allow**——这意味着当没有 deny/ask/allow 规则命中时，Auto 模式默认放行工具调用。

## 危险规则剥离

由于 Auto 模式默认放行的特性，`dangerous.rs` 实现了 `strip_dangerous_permissions_for_auto_mode` 函数，在切换到 Auto 模式时自动移除过于宽泛或危险的 allow 规则：

- **Bare shell allow**：裸 `"Bash"` 或 `"PowerShell"` 规则被剥离
- **代码执行/提权规则**：匹配 `CROSS_PLATFORM_CODE_EXEC_AUTO_ALLOW_PATTERNS`（python、node、bash 等）和 `DANGEROUS_BASH_AUTO_ALLOW_PATTERNS`（sudo、eval、zsh 等）的模式被剥离
- **Agent 规则**：任何 Agent allow 规则都被剥离
- **PowerShell 专属危险模式**：`Invoke-Expression`、`Add-Type`、`Start-Process RunAs` 等被剥离

剥离的规则存储在 `auto_mode_stripped_always_allow_rules` 和 `auto_mode_stripped_session_allow_rules` 中，退出 Auto 模式时通过 `restore_auto_mode_stripped_permissions` 恢复。

## 拒绝追踪与 Fallback

`decision.rs` 的 `DenialTracker` 与 Auto 模式紧密配合：

- 当分类器连续拒绝同一类操作 3 次或总计拒绝 20 次时，系统自动回退到 Ask 交互
- `has_permissions_to_use_tool_with_hook_and_auto_classifier` 在 Phase 4 的 Auto 分支中首先检查 `should_fallback_to_interactive()`
- Fallback 是为了避免 AI 反复请求同一操作的死循环

## 策略控制

Auto 模式还支持细粒度的 `AutoModeSettings` 策略控制，包含四个维度：

- **`environment`** — 环境描述（如 "Trusted repo"），帮助分类器理解当前上下文
- **`allow`** — 明确允许的模式（如 "Allow cargo verification"）
- **`soft_deny`** — 倾向拒绝的模式（如 "Avoid network writes"）
- **`extra`** — 额外的自定义策略字段

这些策略通过 `render_auto_mode_policy` 序列化后注入分类器 prompt，作为分类器判断的参考依据。

## Hook 上下文

`hook_context` 字段允许 PreToolUse hook 向分类器传递额外的上下文信息。例如 hook 可以在不修改输入的情况下标注某次工具调用的风险等级，分类器结合这些信息做出更准确的判断。

## 两阶段分类器

`classifier.rs` 的 `SharedSafetyClassifier` 实现了两阶段分类：

### Fast 阶段
- 配置 `effort: "low"`，快速判断
- 如果判为 `Allow` 且非高风险命令，直接返回结果
- 如果判为 `Deny`，直接拒绝，不再浪费资源
- 如果判为 `Ask` 或标记 `escalate: true`，升级到 Thinking 阶段

### Thinking 阶段
- 配置 `effort: "medium"`，更慢但更准确的判断
- 对高风险命令（通过 `is_dangerous_command` 静态检测）即使 Fast 阶段 Allow 也要进入 Thinking
- 最终裁决覆盖 Fast 阶段结果

### 分类器 Prompt

系统 prompt 要求返回严格 JSON：
```json
{"verdict":"allow|deny|ask","reason":"短原因","thinking":"可选的私有摘要","escalate":false}
```

分类器输入包含：
- `purpose`、`stage`、`permission_mode`、`sandbox_mode`
- `tool_name`、`tool_input`、`tool_classifier_input`
- `auto_mode_policy`（environment / allow / soft_deny / extra）
- `hook_context`
- `recent_transcript`（最近消息的窗口）

### 转录窗口管理

`build_stage_prompt` 从最近消息开始逆向截取，确保不超过：
- `max_transcript_bytes`（默认 48KB）
- `max_prompt_bytes`（默认 64KB）
- `max_estimated_tokens`（默认 16000）

超时时返回 `transcript_too_long` 状态，标记为不可用，回退到 Ask。

### 机密信息脱敏

`redact_classifier_text` 对输入中的密钥模式进行正则替换：

- `api_key`、`token`、`secret`、`password` 等字段的值被替换为 `<redacted>`
- `sk-`、`sk-ant-`、`AIza` 等常见 API 密钥前缀被匹配
- 同时处理 JSON 结构和非结构化的密钥赋值

### 错误处理

- 模型不可用 → `unavailable = true`，回到 Ask
- 响应格式错误 → `unavailable = true`，回到 Ask
- 转录过长 → `transcript_too_long = true`，回到 Ask

所有失败路径都闭合到 Ask，保证安全工作。

## 与权限系统的集成

在完整的决策流程 `has_permissions_to_use_tool_with_hook_and_auto_classifier` 中，Auto 模式的分类器结果在 Phase 4（模式兜底）发挥作用：

1. 先检查 denial tracker 是否需要 fallback — 连续拒绝过多则直接 Ask
2. 如果有 `auto_classifier` 参数，调用 `auto_classifier_decision`
3. 如果分类器不可用或转录过长，返回 Ask
4. 分类器 Allow → 允许，记录 allow（重置 denial tracker）
5. 分类器 Deny → 拒绝，记录 denial；若连续 denials >= 3 则转为 Ask（fallback）
6. 分类器 Ask → 弹窗确认

分类器裁决具有最高优先级的 deny/ask 规则穿透能力：即使分类器判为 Allow，Phase 1 的 deny/ask 规则仍然优先。这种设计确保企业策略和白名单规则始终凌驾于模型判断之上。
