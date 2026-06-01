---
title: "代码搜索工具 - Glob、Grep、FileSearch 源码解析"
description: "从源码角度解析 allthecodes 的代码搜索工具：Glob 文件模式匹配、Grep 多后端搜索引擎、排序策略与输出限制的实现细节。"
keywords: ["Glob 工具", "Grep 工具", "代码搜索", "ripgrep", "文件匹配", "正则搜索"]
---

## 概述

allthecodes 提供两个专用搜索工具，位于 `allthecodes-tools/src/fs/`：

| 工具 | 源码文件 | 功能 | 并发安全 |
|------|---------|------|---------|
| **Glob** | `glob_tool.rs` | 按通配符模式查找文件 | `is_concurrency_safe() → true` |
| **Grep** | `grep.rs` | 按正则表达式搜索文件内容 | `is_concurrency_safe() → true` |

两者都是只读操作（`is_read_only() → true`），可以安全地并行执行。

## Glob：快速文件模式匹配

`GlobTool` 基于 `glob` crate 实现，用于按名称模式查找文件。

### 输入参数

| 字段 | 类型 | 说明 |
|------|------|------|
| `pattern` | `string` (必填) | Glob 通配符模式，如 `**/*.rs`、`src/**/*.ts` |
| `path` | `string` (可选) | 搜索目录，默认当前工作目录 |

路径处理逻辑：

```rust
fn parse_input(input: &Value) -> (String, Option<String>) {
    let path = input.get("path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())  // 空字符串视为 None
        .map(|s| s.to_string());
    (pattern, path)
}
```

### 绝对路径处理

```rust
let full_pattern = if pattern.starts_with('/') || pattern.contains(':') {
    pattern.clone()  // 绝对模式直接使用
} else {
    // 相对路径拼接 base_dir
    format!("{}/{}", base, pattern)
};
```

支持跨平台路径分隔符归一化（`\\` → `/`）。

### 排序策略

结果按修改时间**降序**排列（最新的文件在前），同时间戳的按路径名升序：

```rust
fn sort_matches_by_modified(files: &mut [GlobMatch]) {
    files.sort_by(|a, b| {
        let modified_order = match (a.modified, b.modified) {
            (Some(a), Some(b)) => b.cmp(&a),  // 最新在前
            (Some(_), None) => Less,            // 有时间的排前面
            (None, Some(_)) => Greater,
            (None, None) => Equal,
        };
        modified_order.then_with(|| a.path.cmp(&b.path))
    });
}
```

### 输出限制

当匹配结果超过 `max_result_size_chars()` 时自动截断，并追加 `"... (output truncated)"` 标记。

## Grep：多后端搜索引擎

`GrepTool` 实现了双后端搜索策略：优先使用 ripgrep，降级到 Rust 内置的 `regex` + `ignore` walker。

### 双后端架构

```
try_ripgrep()
  ├── 检查 rg 是否可用（rg --version）
  ├── 可用 → 构建 rg 参数并执行
  ├── 成功 → 返回 rg 输出
  └── 失败 → 返回 None，触发 fallback

fallback:
  ├── Regex::new() 编译用户提供的正则
  ├── ignore::WalkBuilder 遍历文件系统
  ├── 逐文件读取 + 逐行匹配
  └── 按 output_mode 格式化输出
```

### ripgrep 参数映射

| Grep 参数 | rg 参数 | 说明 |
|-----------|---------|------|
| `pattern` | `-e PATTERN` | 搜索模式 |
| `path` | `<search_path>` | 搜索路径 |
| `glob` | `--glob PATTERN` | 文件过滤 |
| `file_type` | `--type TYPE` | 文件类型过滤 |
| `case_insensitive` | `-i` | 不区分大小写 |
| `multiline` | `-U --multiline-dotall` | 多行模式 |
| `context` | `-C N` | 上下文行数 |
| `output_mode: content` | `-n` | 显示行号+内容 |
| `output_mode: files_with_matches` | `-l` | 仅显示文件名 |
| `output_mode: count` | `-c` | 仅显示计数 |

**关键设计**：ripgrep 的 `--max-count` 是按文件限制的，所以 Grep 工具在应用端做全局限制（`head_limit` + `offset`）。

### fallback 引擎

当 ripgrep 不可用时，内置引擎使用：

- `regex::Regex` — 编译用户的正则表达式（支持 `(?i)` 等 inline 标志）
- `ignore::WalkBuilder` — 支持 `.gitignore` 过滤、隐藏文件控制
- `glob::Pattern` — 支持自定义文件扩展名过滤

### 输出模式

三种输出模式覆盖不同场景：

**content 模式**（默认需要行号）：
```
src/main.rs:42:    let x = 1;
src/utils.rs:17:    let x = 2;
```

**files_with_matches 模式**：
```
src/main.rs
src/utils.rs
```

**count 模式**：
```
src/main.rs:3
src/utils.rs:1
```

### 偏移与限制

通过 `offset` 和 `head_limit` 参数实现翻页：

```rust
fn apply_offset_and_limit(output: &str, offset: usize, head_limit: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let after_offset: Vec<&str> = lines.into_iter().skip(offset).collect();
    let limited = if head_limit > 0 && after_offset.len() > head_limit {
        &after_offset[..head_limit]
    } else {
        &after_offset[..]
    };
    limited.join("\n")
}
```

### 完整输入 Schema

```json
{
  "type": "object",
  "properties": {
    "pattern": { "type": "string" },
    "path": { "type": "string" },
    "glob": { "type": "string" },
    "type": { "type": "string" },
    "output_mode": { "enum": ["content", "files_with_matches", "count"] },
    "-C": { "type": "number" },
    "-A": { "type": "number" },
    "-B": { "type": "number" },
    "-i": { "type": "boolean" },
    "-n": { "type": "boolean" },
    "head_limit": { "type": "number" },
    "multiline": { "type": "boolean" },
    "offset": { "type": "number" }
  },
  "required": ["pattern"]
}
```

## 工程决策

### 为什么优先使用 ripgrep

- **性能**：ripgrep 比 Rust 内置的逐行读取 + 正则匹配快 5-10 倍（使用 SIMD 加速）
- **功能完整**：支持 `.gitignore`、二进制文件跳过、文件类型过滤等
- **正确性**：ripgrep 的 UTF-8 处理和正则语义经过广泛验证

### fallback 限制

- 不支持多行模式（ripgrep 的 `-U` 标志在 fallback 中不生效）
- 大仓库下性能显著低于 ripgrep
- 文件类型过滤仅支持扩展名级别

### 空路径处理

两个工具都将空字符串路径视为"当前工作目录"：

```rust
// Grep
let search_path = params.path.clone()
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| ".".to_string());

// Glob
let base_dir = match &path {
    Some(p) => PathBuf::from(p),
    None => std::env::current_dir()...
};
```
