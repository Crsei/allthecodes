---
title: "命令执行工具 - BashTool 安全设计与 Shell 解析"
description: "从源码角度解析 allthecodes BashTool：tree-sitter AST 安全解析、heredoc 提取与恢复、只读命令判定、输出截断策略和安全权限检查的设计。"
keywords: ["Bash 工具", "命令执行", "Shell 执行", "tree-sitter", "heredoc", "安全 AST", "输出截断"]
---

## 执行链路总览

一条 Bash 命令从 AI 决策到实际执行的完整路径：

```
AI 生成 tool_use: { command: "npm test" }
  ↓
BashTool.validateInput()         ← 基础输入校验
  ↓
BashTool.checkPermissions()      ← 权限检查（AST 解析 + 语义匹配）
  ↓ allow / deny / ask
BashTool.call()                  ← 执行命令
  ↓
shell-words 解析 → heredoc 处理 → 子进程创建
  ↓
输出收集 → 截断 → 返回 ToolResult
```

## tree-sitter AST 安全解析

`allthecodes-shell-command` crate 使用 tree-sitter 对 Bash 命令进行安全解析，确保不依赖正则表达式即可获得精确的命令结构。

`crates/allthecodes-shell-command/src/bash_ast.rs` 中的 `parse_for_security()` 函数：

### 节点类型白名单

采用**白名单机制**——只允许已知安全的节点类型通过：

```rust
const STRUCTURAL_TYPES: &[&str] = &["program", "list", "pipeline", "redirected_statement", "command"];
const COMMAND_CHILD_TYPES: &[&str] = &["command_name", "word", "string", "raw_string", ...];
const DECLARATION_TYPES: &[&str] = &["if_statement", "for_statement", "function_definition", ...];
```

任何不在白名单中的节点类型都会被标记为 `TooComplex`。

### 安全防御

- **解析超时**：`PARSE_TIMEOUT_MS = 50ms`，超过则返回 `TooComplex`
- **节点预算**：`NODE_BUDGET = 50,000`，防止恶意构造的命令导致 CPU 耗尽
- **Fail-closed（默认拒绝）**：解析失败时视为不可信任，需要用户确认

### 不安全结构拒绝

以下结构在安全模式下被明确拒绝：

| 结构 | 原因 |
|------|------|
| `$(whoami)` 命令替换 | 动态内容无法静态分析 |
| `$HOME` 参数展开 | 运行时值不可知 |
| `<(cmd)` 进程替换 | 类似命令替换 |
| `if`/`while`/`for`/`case` | 控制流过于复杂 |
| `(subshell)` | 嵌套执行无法追踪 |
| 未闭合引号 | 解析器无法处理 |

### SecurityParseResult

```rust
pub enum SecurityParseResult {
    Simple { commands: Vec<SimpleCommand> },           // 可信任的解析结果
    TooComplex { reason: String, node_type: Option<String> }, // 需要用户确认
    ParseUnavailable,                                   // tree-sitter 不可用
}
```

## Bash AST 分析

`tree_sitter_analysis.rs` 提供三个维度的分析：

### 引号上下文（QuoteContext）

```rust
pub struct QuoteContext {
    pub with_double_quotes: String,    // 移除单引号内容，保留双引号内容
    pub fully_unquoted: String,        // 移除所有引号内容
    pub unquoted_keep_quote_chars: String, // 移除内容但保留引号字符
}
```

用于权限匹配和只读判定中正确处理引号。

### 复合结构分析（CompoundStructure）

```rust
pub struct CompoundStructure {
    pub has_compound_operators: bool,
    pub has_pipeline: bool,
    pub has_subshell: bool,
    pub has_command_group: bool,
    pub operators: Vec<String>,
    pub segments: Vec<String>,
}
```

识别 `&&`、`||`、`|`、`;` 等操作符，将复合命令拆分为独立段。

### 危险模式检测（DangerousPatterns）

检测命令替换、进程替换、参数展开、heredoc 和注释。

## Heredoc 提取与恢复

`crates/allthecodes-shell-command/src/heredoc.rs` 实现了完整的 heredoc 处理：

### 提取流程

```
1. 快速检查是否有 `<<` 操作符
2. 安全检查：$'...' / $"..." / 反引号 / 未闭合 `((` → 跳过
3. 查找所有 `<<` 位置（排除 `<<<`）
4. 增量扫描器追踪引号状态，跳过注释和转义
5. 解析 heredoc 操作符（`<<EOF`、`<<'EOF'`、`<<"EOF"`、`<<-EOF`）
6. 查找闭合分隔符（支持 PST_EOFTOKEN 检测）
7. 替换为占位符 `__HEREDOC_N_SALT__`
8. 返回处理后的命令 + heredoc 映射
```

### 占位符与恢复

```rust
// 提取
let result = extract_heredocs(command, None);
let processed = result.processed_command;  // 含占位符
let heredocs = result.heredocs;            // 占位符 → 原文映射

// 解析
let tokens = shell_words::split(&processed)?;

// 恢复
let restored = restore_heredocs(&tokens, &heredocs);
```

这种设计解决了 `shell_words` crate 无法正确处理 heredoc 的问题——它将 `<<` 误解析为两个 `<` 重定向操作符。

### 安全检测

- `$'...'` / `$"..."` ANSI-C/本地引用 → 跳过（扫描器无法处理 `$` 前缀引号）
- 反引号命令替换在第一个 `<<` 之前 → 跳过（防止嵌套替换混淆状态机）
- `((` 算术展开不平衡 → 跳过（可能误判 `<<` 为位移操作符而非 heredoc）
- 反斜杠换行连续 → 跳过（可能隐藏后续命令）
- PST_EOFTOKEN 检测 → 跳过（bash 遇到特定字符时提前闭合 heredoc）

## 只读命令判定

`exec/bash.rs` 的 `truncate_output()` 函数实现了头尾保留的输出截断策略：

```
策略:
1. 输出长度 ≤ max_chars → 原样返回
2. 行数 ≤ 200 + 100 → 字符级截断 + "(output truncated)"
3. 行数 > 300 → 保留前 200 行 + 省略标记 + 后 100 行
   省略标记: "--- (N lines omitted) ---"
4. 如果候选结果仍超限 → head_lines 折半（贪婪收缩）
```

这个 head+tail 策略确保关键信息（命令执行的开始和结束部分）不会丢失。

## 输入 Schema

```json
{
  "type": "object",
  "properties": {
    "command": { "type": "string", "description": "The command to execute" },
    "timeout": { "type": "number", "description": "Optional timeout in ms (max 600000)" },
    "description": { "type": "string", "description": "Clear description of what this command does" }
  },
  "required": ["command"]
}
```

- 默认超时：120000ms（2 分钟）
- 最大超时：600000ms（10 分钟，用户显式设置时）
- 支持 `run_in_background` 参数将命令转为后台任务

## 为什么用专用工具而不是直接调 shell

allthecodes 为文件读写、代码搜索等操作提供了专用工具（Read、Grep、Glob），而不是让 AI 用 `cat`、`grep` 等 shell 命令：

| 维度 | 专用工具 | Bash 命令 |
|------|---------|----------|
| **权限粒度** | `Read` 是只读操作 → 自动放行 | `Bash: cat file` 需要审批整条命令 |
| **输出结构化** | 返回结构化数据，UI 可渲染 | 纯文本输出 |
| **性能优化** | 文件缓存、分页、token 预算控制 | 每次都是新进程 |
| **并发安全** | `is_concurrency_safe()` 支持并行 | Bash 命令串行执行 |
| **安全审计** | 工具名精确匹配权限规则 | 需 AST 解析命令结构后匹配 |
