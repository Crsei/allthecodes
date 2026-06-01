# Bash 解析与 Shell 提供者移植缺口

对应: `claude-code-bun/src/utils/bash/` + `src/utils/shell/`  
Rust 对应: `cc-utils/bash.rs`, `cc-utils/shell.rs`, `cc-tools/exec/bash.rs`, `cc-engine/command_runtime.rs`

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| bash/bashParser.ts | 4,432 | 纯 TS Bash 解析器 (自实现 tokenizer + parser) |
| bash/ast.ts | 2,679 | tree-sitter AST 安全分析 |
| bash/parser.ts | 230 | 高层解析 API 封装 |
| bash/commands.ts | 1,339 | 正则/shell-quote 解析 (传统方案) |
| bash/ShellSnapshot.ts | 583 | Shell 快照管理 |
| bash/shellQuote.ts | 303 | shell-quote 安全封装 |
| bash/shellQuoting.ts | 128 | 引用/转义 |
| bash/heredoc.ts | 733 | Heredoc 提取和恢复 |
| bash/treeSitterAnalysis.ts | 506 | Tree-sitter AST 安全验证 |
| bash/ParsedCommand.ts | 318 | 解析命令接口 |
| bash/prefix.ts | 204 | 命令前缀提取 |
| bash/bashPipeCommand.ts | 294 | 管道命令重排 |
| bash/registry.ts | 53 | Fig 补全 spec 注册表 |
| shell/bashProvider.ts | 255 | Bash Shell 提供者 |
| shell/shellProvider.ts | 33 | ShellProvider 接口 |
| shell/powershellProvider.ts | 123 | PowerShell 提供者 |
| shell/readOnlyCommandValidation.ts | 1,893 | 只读命令验证 |
| shell/prefix.ts | 366 | Haiku LLM 前缀提取 |
| shell/outputLimits.ts | 14 | 输出限制 |
| shell/specPrefix.ts | 241 | Spec 前缀提取 |
| **Bash 解析合计** | **~12,089** | |
| **Shell 提供者合计** | **~3,068** | |

## Rust 已实现

### cc-utils/bash.rs
- 基础命令分割 (`split_command_with_operators`)
- 重定向提取
- Heredoc 处理 (提取/恢复)
- 管道命令检测
- Shell-quote 风格引用处理

### cc-utils/shell.rs
- Shell 转义
- 控制台输出处理
- 简单解析

### cc-tools/exec/bash.rs
- Bash 执行工具（子进程执行）
- 信号处理
- 超时处理

### cc-tools/exec/powershell.rs
- PowerShell 子进程执行
- Base64 编码命令发送

## Rust 缺失的主要功能

### 1. Bash 解析器 (4,432 行) — 完全缺失
Bun 版本有一个**自实现的纯 TypeScript Bash 解析器**，包含：
- 完整 Tokenizer: WORD, NUMBER, OP, NEWLINE, COMMENT, DQUOTE, SQUOTE, ANSI_C, DOLLAR, DOLLAR_PAREN, DOLLAR_BRACE, BACKTICK 等
- Parser: 处理 shell 关键字、声明、复合命令
- 超时保护 (50ms) + 节点预算 (50K)
- UTF-8 字节偏移追踪
- Rust 目前使用 `shell-words` crate，功能简单得多

**建议**: Rust 可直接使用 `bash AST parser` crate (如 `tree-sitter-bash`) 替代，无需重写纯 Rust 解析器。

### 2. Tree-sitter AST 安全分析 (2,679 行) — 完全缺失
- FAIL-CLOSED 设计的节点类型允许列表
- 变量作用域追踪 (`VarScope`)
- 安全分析: 只读命令检测、危险参数检测
- 命令替换提取和占位符替换
- Heredoc 解析嵌套追踪

### 3. Shell 提供者抽象 (255+123 行) — 未移植
- `ShellProvider` trait (Rust 中可以用 trait)
- `BashProvider`: extglob 禁用、管道重排、shell 快照、cwd 追踪
- `PowerShellProvider`: EncodedCommand 编码、临时文件、sandbox tmp dir

### 4. 只读命令验证 (1,893 行) — 完全缺失
- `GIT_READ_ONLY_COMMANDS` 完整映射 (所有 git 子命令标记)
- `GH_READ_ONLY_COMMANDS` 完整映射
- `EXTERNAL_READONLY_COMMANDS` 完整映射
- Flag 类型系统: `none`, `number`, `string`, `char`, `{}`, `EOF`
- UNC 路径漏洞检测
- **安全关键**: 这个模块用于防止读取模式下的写操作

### 5. Haiku LLM 前缀提取 (366 行) — 未移植
- `createCommandPrefixExtractor()` 基于 Haiku LLM
- `createSubcommandPrefixExtractor()`
- 危险 shell 前缀列表
- 用于权限规则建议

### 6. Fig Autocomplete Spec Registry (53 行) — 未移植
- `@withfig/autocomplete` 补全 spec 的懒加载
- 子命令感知的前缀提取
- Spec-based 回退

### 7. Shell 快照管理 (583 行) — 未移植
- 创建嵌入式搜索工具的 shell 函数 (rg, bfs, ugrep)
- ARGV0 dispatch
- `~/.claude/shell-snapshot-*` 文件管理
- PATH 设置、session env 脚本

### 8. Heredoc 完整处理 (733 行)
Bun: 完整的 heredoc 提取/恢复，支持所有变体 (基本、引号定界符、dash 前缀、dash+引号组合)  
Rust: cc-utils/bash.rs 有基本 heredoc 支持，功能可能不够完整

### 9. 管道命令重排 (294 行) — 未移植
- `rearrangePipeCommand()` 用于 eval 兼容
- 标准输入重定向在管道命令中的正确放置

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| Bash 解析器 | 自实现 TypeScript 解析器 (4,432 行) | shell-words crate | Rust 需集成 tree-sitter-bash |
| 安全 AST 分析 | tree-sitter 集成 (2,679 行) | 无 | **安全关键缺口** |
| 只读命令验证 | 完整命令白名单 (1,893 行) | 无 | **安全关键缺口** |
| Shell 提供者 | bash + powershell 提供者 | exec/bash.rs 执行 | 缺少提供者抽象层 |
| 前缀提取 | Haiku LLM + Fig specs | 无 | 权限 UX 降级 |
| Shell 快照 | 搜索工具 ARGV0 dispatch | 无 | 搜索工具需要 PATH 配置 |
| Heredoc | 完整处理 | 基本支持 | 边缘情况不兼容 |
