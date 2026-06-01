---
title: "权限模型 - Allow/Ask/Deny 三级权限体系"
description: "详解 allthecodes (Rust 版 Claude Code) 的三级权限模型实现：基于 allthecodes-permissions crate 的规则匹配引擎、多来源优先级、Bash/Read/Edit/WebFetch 等工具的 specifier 匹配模式、denial tracking 死循环防护、shadowed rule 检测。"
keywords: ["权限模型", "Allow Ask Deny", "PermissionRule", "规则引擎", "shadowed rules", "denial tracking"]
---

## 三种权限裁决

每次工具调用，权限系统做出三种裁决之一：

| 行为 | 含义 | 适用阶段 |
|------|------|---------|
| **Allow** | 自动放行，用户无感知 | 匹配 allow 规则、Bypass/Auto 模式 |
| **Ask** | 弹窗请求用户确认 | 默认模式、Plan 模式写操作、Ask 规则 |
| **Deny** | 直接拒绝 | 匹配 deny 规则、DontAsk 模式 |

核心决策类型定义在 `decision.rs` 的 `PermissionDecision` 结构体，包含 `behavior`、`updated_input`、`message` 和 `reason`（携带决策来源追踪）。

## 规则来源与优先级

规则来源共 6 个层级（`decision.rs` 注释标注），优先级从高到低：

1. **Managed** — 企业管理员下发策略，用户不可覆盖
2. **Project** — `.allthecodes/settings.json`（团队共享）
3. **Local** — `.allthecodes/settings.local.json`（gitignored）
4. **User** — `~/.allthecodes/settings.json`（跨项目）
5. **CLI** — 命令行 `--permission-mode` 等参数
6. **Session** — 用户在对话中手动授予的临时权限

每个来源维护三个 HashMap：`always_allow_rules`、`always_deny_rules`、`always_ask_rules`，键为来源名称，值为规则字符串数组。

## 规则匹配引擎

### 核心流程（`has_permissions_to_use_tool_with_hook_and_auto_classifier`）

```
Phase 1a: Hook deny — PreToolUse hook 强制拒绝
Phase 1b: Deny 规则匹配 → 命中则 Deny
Phase 1c: Ask 规则匹配 → 命中则 Ask（覆盖 Allow）
Phase 1d: Allow 规则匹配 → 命中则 Allow
Phase 2:  Hook ask / hook allow — hook 覆盖模式兜底
Phase 3:  会话级授权（session_allow_rules）
Phase 4:  模式兜底 — Default/Plan→Ask, Auto/Bypass→Allow, AcceptEdits→文件编辑自动放行
```

### 规则匹配形态

`rules.rs` 的 `rule_matches` 函数支持四种匹配方式：

1. **精确工具名**：`"Bash"` 精确匹配 Bash 工具
2. **前缀匹配**：`"mcp__server"` 匹配 `"mcp__server__tool"`
3. **Glob 通配符**：`"mcp__*"` 匹配任意以 `mcp__` 开头的工具
4. **Specifier 模式**：`"Bash(prefix:git)"` 或 `"Read(/tmp/*)"` — 在工具名基础上增加输入参数匹配

### Specifier 分派

`specifier_matches` 函数根据工具类型分派不同的匹配逻辑：

- **Bash** → 委托 `bash_matcher::bash_pattern_matches`，支持 compound command 拆分、wrapper stripping（sudo/nohup/env/bash -c 等）、命令名匹配
- **Read/Edit/Write** → 基于 `file_path` 字段的 glob 匹配
- **WebFetch** → 支持 `domain:example.com` 子域名匹配和 URL glob 匹配
- **WebSearch** → 基于 `query` 字段的文本匹配
- **Glob** → 组合 `path` + `pattern` 字段匹配
- **Agent** → 基于 `subagent_type` 字段精确匹配
- **MCP 工具** → 基于工具名的文本匹配

## Bash 权限匹配器

`bash_matcher.rs` 实现了 Bash 命令的语义化匹配，核心在于：

1. **Compound command 拆分**：`split_compound_command` 将 `git status && rm -rf /` 拆分为独立子命令，确保 deny 规则能捕获复合命令中的危险操作
2. **Wrapper stripping**：识别并剥离 `sudo`、`nohup`、`timeout`、`env A=B`、`xargs`、`bash -c` 等包装器，提取真正的命令
3. **Pattern 匹配**：支持 `prefix:git`、`cargo test*` glob、精确命令名

## 危险命令检测

`dangerous.rs` 实现了静态正则匹配的危险命令检测：

- 正则模式涵盖 `rm -rf /`、`git push --force`、`git reset --hard`、`DROP TABLE`、`dd if=`、`mkfs`、`chmod 777 /`、fork bomb、`curl | sh` 等
- PowerShell 专属模式涵盖 `Remove-Item -Recurse`、`Invoke-Expression`、`New-Object` 等 50+ 种危险 cmdlet
- 使用 `New-Object` 类型名与 Constrained Language Mode 白名单对比

## 拒绝追踪（Denial Tracking）

`decision.rs` 的 `DenialTracker` 实现死循环防护：

- `consecutive_denials >= 3` → 触发 fallback
- `total_denials >= 20` → 强制回退到交互式模式
- `record_denial()` 返回 true 表示应回退
- `record_allow()` 重置连续拒绝计数

## 运行时权限更新

`permission_update.rs` 提供 `PermissionUpdate` 枚举，支持 6 种运行时变更：

- `AddRules` / `ReplaceRules` / `RemoveRules` — 规则增删改
- `SetMode` — 切换权限模式
- `AddDirectories` / `RemoveDirectories` — 额外工作目录管理

通过 `apply_permission_update` 应用到 `ToolPermissionContext`，支持持久化到 settings 文件。

## Shadowed Rule 检测

`shadowed_rules.rs` 检测不可达的 allow 规则：

- **Deny shadowing**：工具级 deny 规则使该工具的特定 allow 规则全部不可达
- **Ask shadowing**：工具级 ask 规则覆盖特定 allow 规则
- 提供修复建议：删除冲突规则之一
- 沙箱模式下个人设置的 ask 规则不 shadow Bash allow 规则
