# 权限系统与 LLM 分类器移植缺口

对应: `claude-code-bun/src/utils/permissions/`  
Rust 对应: `cc-permissions/`, `cc-safety/`

> 更新日期: 2026-05-18
> 当前状态: 自动模式 LLM 分类器已接入运行时，旧版核心缺口结论已失效。本文保留 Bun 端规模对照，并改为记录 Rust 端已补齐范围与仍需追踪的问题。

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| permissions.ts | 1,507 | 核心权限逻辑 |
| permissionSetup.ts | 1,521 | 权限基础设施 |
| yoloClassifier.ts | 1,510 | 自动模式 LLM 分类器 |
| filesystem.ts | 1,782 | 文件权限检查 |
| pathValidation.ts | 494 | 路径验证 |
| PermissionMode.ts | 136 | 权限模式定义 |
| PermissionRule.ts | 40 | 权限规则类型 |
| PermissionResult.ts | 35 | 权限决策结果 |
| PermissionUpdate.ts | 389 | 权限更新管理 |
| PermissionUpdateSchema.ts | 78 | 更新请求模式 |
| PermissionPromptToolResultSchema.ts | 127 | 工具结果模式 |
| permissionRuleParser.ts | 198 | 规则字符串解析 |
| permissionsLoader.ts | 296 | 磁盘持久化 |
| dangerousPatterns.ts | 80 | 危险命令模式 |
| bashClassifier.ts | 61 | Bash 分类器 (存根) |
| classifierDecision.ts | 98 | 分类器决策类型 |
| shadowedRuleDetection.ts | 234 | 规则遮蔽检测 |
| shellRuleMatching.ts | 228 | Shell 规则匹配 |
| bypassPermissionsKillswitch.ts | 124 | 绕过权限开关 |
| denialTracking.ts | 45 | 拒绝频率追踪 |
| **合计** | **~9,378** | |

## Rust 当前已实现

### cc-permissions/
- **rules.rs**: 规则引擎，支持 Managed/Project/Local/User/CLI/Session 来源
- **decision.rs**: 决策状态机，覆盖 deny/ask/allow/auto 分支
- **dangerous.rs**: 危险命令检测与 Auto mode 危险 allow 规则剥离/恢复
- **bash_matcher.rs**: Bash 专用模式匹配
- **path_validation.rs**: 路径验证，包括规范化和路径遍历保护
- **permission_update.rs**: `PermissionUpdate` 类型、规则/目录/模式更新应用、读规则建议
- **shadowed_rules.rs**: allow 规则被更高优先级 deny/ask 规则遮蔽时的检测和修复建议
- **DenialTracker**: Auto classifier 拒绝频率追踪，重复拒绝后回退到交互 ask

### cc-safety/
- **classifier.rs**: Auto mode classifier request、prompt 构建、策略渲染、redaction、响应解析与 `SafetyClassifier` trait
- Auto classifier prompt 包含:
  - `permission_mode`
  - `sandbox_mode`
  - `cwd`
  - tool name
  - raw tool input
  - tool-specific classifier input
  - `permissions.autoMode` policy
  - hook context
  - recent transcript
- `SafetyClassifierRequest::auto_mode_tool_with_classifier_input()` 支持工具传入脱敏/摘要后的 classifier 输入。
- 当工具输入与 classifier 输入不同，prompt 会保留结构化摘要，避免把文件写入正文或替换内容直接交给分类器。

### cc-engine / claude-code-rs runtime
- `QueryEngine` 已接入 `AutoClassifierFn`。
- Auto mode 权限顺序已调整为:
  1. 先执行工具本地 `check_permissions()`
  2. 再应用中心 deny/ask/allow 规则
  3. 只有在规则分支落到 Auto mode 时才调用 LLM classifier
  4. classifier deny/ask/allow 再映射为最终 `PermissionResult`
- 显式 deny/ask/allow 规则不会被 classifier 覆盖。
- 工具本地 deny/ask 不会调用 classifier。
- classifier 不可用时 fail closed 到 ask。
- `DenialTracker` 已存入 `QueryEngineState`，重复 classifier deny 后会回退到交互确认。
- `claude-code-rs/src/classifier_model.rs` 已实现 ApiClient-backed classifier 调用。
- Thinking stage 使用独立请求限制，避免 `thinking.budget_tokens` 大于 `max_tokens`。
- Runtime 传入合并后的 `permissions.autoMode`，不再使用空 `AutoModeSettings`。

### 工具侧 classifier 输入
- `Write` 已覆盖 `to_auto_classifier_input()`，只传路径、内容字节数、行数等摘要。
- `Edit` 已覆盖 `to_auto_classifier_input()`，只传路径、old/new 字节数、行数等摘要。
- `Grep` 已覆盖 `to_auto_classifier_input()`，避免把不必要的大输入传入 classifier。

## 最近修复的问题

| ID | 状态 | 摘要 | 对应记录 |
|----|------|------|----------|
| SAFETY-006 | Fixed | ApiClient-backed classifier 的 Thinking stage 不能用 `budget_tokens=2048` 搭配 `max_tokens=512`；现已把 Thinking 请求上限提高到大于 thinking budget。 | `development/archive/KNOWN_ISSUES.md` |
| SAFETY-007 | Fixed | Runtime 原先传入空 `AutoModeSettings`，导致 `permissions.autoMode` policy 不进 classifier prompt；现已传入合并后的配置。 | `development/archive/KNOWN_ISSUES.md` |

## Rust 仍需追踪的问题

### 1. 文件系统权限检查仍未达到 Bun 完整度
- Rust 已有路径验证和部分工具本地权限检查，但 Bun 端 `filesystem.ts` 的完整能力仍未全部对齐。
- 仍需按工具补齐:
  - glob pattern 权限匹配
  - 项目目录边界检测
  - 危险文件列表
  - MCP 路径工具语义
  - 文件工具与权限规则的端到端 UI 提示文案

### 2. 权限流程协调仍有产品面差距
- Runtime 已把工具本地检查、中心规则和 Auto classifier 串起来。
- 仍需继续补齐:
  - `submitPermissionPrompt()` 的完整交互语义
  - 权限提示结果格式化
  - UI 中对规则来源、建议更新、遮蔽规则的展示
  - MCP/Agent 等非文件工具的端到端权限提示覆盖

### 3. 权限设置持久化仍不完整
- Rust 已有 `PermissionUpdate` 数据结构和内存应用逻辑。
- 仍缺 Bun `permissionsLoader.ts` 等价的跨设置源读写闭环:
  - user/project/local/session/CLI 来源的持久化写回
  - 管理规则的完整读写策略
  - 设置更新后的 UI/运行时刷新契约

### 4. Schema 和工具结果格式仍需收口
- `PermissionUpdate` 已有 Rust 类型，但还缺完整 JSON schema 验证口径。
- `PermissionPromptToolResultSchema` 的工具结果格式化仍需与上游对齐。

### 5. 规则解析器仍需对齐边界行为
- Rust 规则引擎已能匹配现有规则。
- 仍需按 Bun `permissionRuleParser.ts` 补齐:
  - `Tool(content)` 字符串解析边界
  - 转义处理
  - 传统工具名规范化
  - 非法规则的错误恢复和提示

### 6. 规则遮蔽检测已实现 core，尚未完整产品化
- `cc-permissions::shadowed_rules` 已能检测被 deny/ask 遮蔽的 allow 规则并生成建议。
- 仍需把检测结果接入 `/permissions`、配置 UI 或权限提示 UI，避免只存在库层能力。

### 7. Bypass permissions killswitch 仍缺等价实现
- Bun 的 `bypassPermissionsKillswitch.ts` 仍没有完整 Rust 等价物。
- 需要明确 bypass-permissions 模式的远程/本地紧急停止策略、配置来源、错误提示和测试覆盖。

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| 规则引擎 | 完整 | 完整 (rules.rs) | ✅ 已移植 |
| 决策状态机 | 完整 | 完整 (decision.rs) | ✅ 已移植 |
| 危险命令检测 | 完整 | 完整 (dangerous.rs) | ✅ 已移植 |
| 路径验证 | 完整 | 完整 (path_validation.rs) | ✅ 已移植 |
| Bash 模式匹配 | 完整 | 完整 (bash_matcher.rs) | ✅ 已移植 |
| **YOLO/Auto 分类器** | **完整 (1,510 行)** | **已接入 runtime + ApiClient classifier** | 仍需继续补齐上游 prompt/策略细节 |
| 文件权限检查 | 完整 (1,782 行) | 部分 | **功能缺口** |
| 核心权限流程 | checkPermissions() | 部分完成 | 交互提示和非文件工具覆盖仍需补齐 |
| 权限更新 | PermissionUpdate.ts | 部分完成 | 持久化写回仍缺 |
| 权限持久化 | permissionsLoader.ts | 缺失 | 功能缺口 |
| 规则遮蔽检测 | shadowedRuleDetection.ts | core 已实现 | UI 接线缺口 |
| Schema 验证 | PermissionUpdateSchema | 缺失 | 安全缺口 |

## 安全影响

当前 Rust 自动模式已经不再只是规则回退:
- 显式 deny/ask/allow 规则优先，classifier 不能覆盖。
- 只有中心规则落到 Auto mode 时才调用 LLM classifier。
- classifier unavailable 或重复 deny 时回退到 ask，维持 fail-closed。
- 文件写入/编辑工具向 classifier 传摘要输入，降低把敏感正文交给分类器的风险。

剩余安全风险主要来自产品面未收口:
- 文件系统权限语义尚未完全对齐 Bun。
- 权限更新持久化和 schema 验证还不完整。
- 规则遮蔽检测尚未接到用户可见 UI。
- bypass-permissions killswitch 尚缺 Rust 等价能力。
