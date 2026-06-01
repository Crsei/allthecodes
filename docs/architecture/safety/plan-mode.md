---
title: "计划模式 - 只读探索的权限控制"
description: "详解 allthecodes Plan Mode 的实现：Bash/PowerShell 只读命令分类器、git/gh/docker/rg/pyright 的 safe flags 配置、UNC 路径漏洞检测、fail-closed 设计原则。"
keywords: ["Plan Mode", "只读命令", "read-only", "shell classifier", "flag validation", "Explore"]
---

## 概述

Plan Mode（`PermissionMode::Plan`）是权限系统的一种模式，设计用于探索阶段——AI 只能执行读操作，写操作需要用户确认。

在 `rules.rs` 的 `check_tool_permission` 中，Plan 模式的兜底行为是 **Ask**（写工具）和文件编辑工具默认不可用。

```rust
PermissionMode::Plan => PermissionCheckResult::Ask {
    message: format!("Tool '{}' requires confirmation in plan mode.", tool_name),
},
```

Plan 模式常用于代码审查、项目分析、架构探索等场景，在这些场景中 AI 应当只观察而不修改。

## 与 AcceptEdits 模式的对比

Plan 模式和 AcceptEdits 模式构成权限模式的两个极端：

| 维度 | Plan | AcceptEdits |
|------|------|-------------|
| 文件编辑工具 | Ask | Allow |
| Bash 文件系统命令 | Ask | 工作区内自动 Allow |
| 只读 Bash 命令 | Allow | Allow |
| 网络访问 | Ask | Ask |

Plan 模式下所有写操作都需确认，而 AcceptEdits 对已知安全的文件编辑和工作区命令自动放行。

## 只读 Shell 命令分类

`read_only_shell/mod.rs` 的 `is_read_only_shell_command` 提供细粒度的只读判定，用于 Plan 模式和 Explore 模式下 Bash/PowerShell 命令的自动放行。

### 分类流程

1. **空 argv 检查** — 空命令直接判为 Unsupported
2. **UNC 路径漏洞检查** — 检测命令中是否包含 `\\host\share` 模式（可能泄露 NTLM hash）
3. **命令名提取** — 从 argv[0] 提取 basename
4. **已知命令匹配** — 按 git → gh → docker → rg → pyright → external 顺序匹配
5. **Flag 校验** — 验证所有 flags 在安全白名单内

### git 只读命令

`commands.rs` 的 `make_git_read_only_commands` 定义了 git 子命令的安全 flags：

- **status、log、diff、show、branch、remote -v** — 基本只读命令
- subcommand 级配置：如 `diff` 允许 `--stat`、`--name-only`、`--cached` 等
- 未知子命令或不允许的 flags → 判为 NotReadOnly

例如 `git log` 允许 `--oneline`、`--graph`、`--decorate`、`--since` 等 flag，但不允许 `--format=%H`（如果未在安全白名单中）。每个子命令维护独立的 flag 白名单，实现细粒度控制。

### gh 只读命令

`make_gh_read_only_commands` 定义 GitHub CLI 的安全命令：

- **pr view、issue view、search、repo list** 等

### docker 只读命令

`make_docker_read_only_commands` 定义 Docker 的安全命令：

- **ps、images、inspect、logs、network ls** 等

### ripgrep / pyright

`rg` 和 `pyright`/`pyright-langserver` 被整体视为只读，只需校验 flags。

### 外部命令

`make_external_readonly_commands` 定义常见只读命令：

- `cat`、`ls`、`head`、`tail`、`echo`、`printf`、`which`、`type`、`find`、`grep`、`awk` 等

无参数限制的命令直接放行；有参数时检查是否为 flags 或路径。

## Flag 校验

`flag_validation.rs` 的 `validate_flags` 实现严格的 flag 校验：

1. 解析 argv 中的 flags（短选项 `-f`、长选项 `--flag`、连写 `-abc`）
2. 对照 `ExternalCommandConfig.safe_flags` 中定义的允许 flag 和参数类型
3. 检查参数类型：`None`（无参）、`Number`（整数）、`String`（字符串）、`Char`（单字符）、`Brace`（仅 `{}`）
4. 遇到未知 flag 立即判为不安全

## Bash/PowerShell Shell 分类器

`shell_classifier.rs` 的 `is_read_only_bash_command` 和 `is_read_only_powershell_command` 是原始命令文本的入口点：

1. 检查 UNC 路径漏洞
2. 扫描禁止的 shell 语法：重定向、管道、变量展开、子 shell、后台进程、heredoc
3. PowerShell 额外禁止：转义符、表达式、script block、括号
4. 解析为 AST 段，要求恰好 1 个段且无重定向/heredoc
5. 委托 `is_read_only_shell_command` 进行 argv 级匹配

## Fail-Closed 与 PowerShell 分类

PowerShell 分类比 Bash 更严格，因为 PowerShell 的语言特性更丰富（变量、表达式、script block、管道对象）。`is_read_only_powershell_command` 额外禁止：

- 反引号转义符（`）
- `$()` 子表达式
- `{}` script block（除非在安全的 cmdlet 如 `Where-Object` 内）
- `&` 调用运算符
- `.` 点 source
- `::` 静态成员访问
- `@` splatting

这些限制确保 PowerShell 命令无法通过语言特性隐藏写操作。

## Fail-Closed 原则

整个只读分类系统采用 **fail-closed** 设计：

- 无法分类的命令 → `Unsupported`
- 解析失败的命令 → `ParseFailed`
- 包含不安全的语法/flag → `NotReadOnly`
- 只有明确安全的白名单命令 → `ReadOnly`

在 Plan 模式下，只有被分类为 `ReadOnly` 的命令可以自动执行，其余均要求用户确认。

## 与权限系统的关系

Plan Mode 的只读检查与权限规则引擎协同工作：

- deny/ask 规则优先级高于模式兜底
- 只读分类仅在 Plan 模式兜底阶段（Phase 4）影响决策
- Plan 模式下文件编辑工具（Write、Edit、MultiEdit 等）默认 Ask
- Bash 只读命令自动 Allow，非只读命令 Ask
