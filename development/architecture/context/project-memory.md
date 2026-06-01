---
title: "项目记忆系统 — AGENTS.md / CLAUDE.md 与自动记忆"
description: "allthecodes（Claude Code Rust 移植版）的项目记忆系统。涵盖 AGENTS.md（优先）和 CLAUDE.md（回退）的多级发现与合并、MemDir 自动记忆的 CRUD 与召回、记忆上下文在系统提示词中的注入位置。"
keywords: ["AGENTS.md", "CLAUDE.md", "项目记忆", "Memory", "MemDir", "自动记忆"]
---

<!--
本文对应源文件：
  crates/allthecodes-config/src/claude_md.rs       — AGENTS.md / CLAUDE.md 加载
  crates/allthecodes-session/src/memdir/mod.rs     — MemDir 核心逻辑
  crates/allthecodes-session/src/memdir/crud.rs    — 记忆 CRUD
  crates/allthecodes-session/src/memdir/recall.rs  — 相关记忆召回
  crates/allthecodes-session/src/memdir/index.rs   — 记忆索引
  crates/allthecodes-session/src/memdir/types.rs   — 记忆类型定义
  crates/allthecodes-engine/src/system_prompt/mod.rs — 记忆上下文注入点
-->

## 项目指令文件系统（AGENTS.md / CLAUDE.md）

源文件：`crates/allthecodes-config/src/claude_md.rs`

allthecodes 支持两种项目指令文件：**AGENTS.md**（首选）和 **CLAUDE.md**（回退）。系统从当前工作目录向上遍历目录树，发现并合并所有匹配的文件。

### 文件发现算法

```rust
pub fn find_agents_md_files(cwd: &Path) -> Vec<PathBuf> {
    // 从 cwd 开始向上遍历到根目录
    // 每个目录：先检查 AGENTS.md，存在则使用（忽略同目录下的 CLAUDE.md）
    // 若 AGENTS.md 不存在，回退到 CLAUDE.md
    // 结果反转：根目录最远的文件在前（通用指令先），工作目录最近的文件在后（特定指令后）
}
```

优先级规则：
- 同一目录下 `AGENTS.md` 和 `CLAUDE.md` 同时存在时，使用 `AGENTS.md`，输出警告
- 目录级合并：从根到工作目录，每个目录最多一个指令文件
- 多个文件内容以 `---` 分隔符连接，每个文件带有头注 `Contents of <path>`（项目说明，纳入版本控制）

### 禁用条件

- `CLAUDE_CODE_DISABLE_CLAUDE_MDS` 环境变量
- `--bare` 模式（除非通过 `--add-dir` 显式指定目录）

### 注入位置

在系统提示词组装管道的 `build_system_prompt_with_memory_contexts()` 中，AGENTS.md 内容注入在所有动态分区之后，记忆上下文之前：

```rust
// system_prompt/mod.rs
match claude_md::build_agents_md_context(cwd_path) {
    Ok(context) if !context.is_empty() => {
        parts.push(format!(
            "# Project Instructions (AGENTS.md)\n\n\
             IMPORTANT: These instructions OVERRIDE any default behavior \
             and you MUST follow them exactly as written.\n\n{}",
            context
        ));
    }
    // ...
}
```

## 自动记忆系统（MemDir）

源文件：`crates/allthecodes-session/src/memdir/`

MemDir 是一个基于文件的持久化记忆系统，允许会话间保持用户偏好、项目事实和持久上下文。

### 记忆范围与存储

记忆以文件形式存储在 `~/.allthecodes/memdir/` 目录下，按范围（scope）和键（key）组织：

```
~/.allthecodes/memdir/
  ├── global/           # 全局记忆（跨项目）
  │   ├── preferences.json
  │   └── facts.json
  ├── project-<hash>/   # 项目级记忆（按工作目录哈希）
  │   └── architecture.json
  └── team-<name>/      # 团队记忆（需 Feature::TeamMemory 启用）
      └── conventions.json
```

### 记忆类型

```rust
// memdir/types.rs
pub enum MemoryScope {
    Global,       // 跨所有项目
    Project,      // 当前项目特有
    Team,         // 团队共享（feature-gated）
}

pub struct MemoryEntry {
    pub key: String,
    pub scope: MemoryScope,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub ttl: Option<chrono::Duration>,        // 可选过期时间
    pub importance: Option<Importance>,        // 重要性评级
}

pub enum Importance {
    Low,
    Normal,
    High,
    Critical,
}
```

### CRUD 操作

```rust
// memdir/crud.rs
pub fn create_memory(scope: MemoryScope, key: &str, content: &str) -> Result<()>;
pub fn read_memory(scope: MemoryScope, key: &str) -> Result<Option<MemoryEntry>>;
pub fn update_memory(scope: MemoryScope, key: &str, content: &str) -> Result<()>;
pub fn delete_memory(scope: MemoryScope, key: &str) -> Result<()>;
pub fn list_memories(scope: MemoryScope) -> Result<Vec<MemoryEntry>>;
```

### 记忆索引

```rust
// memdir/index.rs
pub struct MemoryIndex {
    pub entries: Vec<IndexEntry>,
    pub last_built: chrono::DateTime<chrono::Utc>,
}

pub struct IndexEntry {
    pub key: String,
    pub scope: MemoryScope,
    pub content_preview: String,
    pub importance: Importance,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub keywords: Vec<String>,
}
```

索引支持关键词搜索，用于高效召回。

### 相关记忆召回

```rust
// memdir/recall.rs
pub fn recall_relevant_memories(
    query: &str,
    scopes: &[MemoryScope],
    max_results: usize,
) -> Result<Vec<MemoryEntry>>;

pub fn recall_with_embedding(
    query: &str,
    scopes: &[MemoryScope],
    max_results: usize,
) -> Result<Vec<ScoredMemory>>;

pub struct ScoredMemory {
    pub entry: MemoryEntry,
    pub score: f64,
}
```

`recall_relevant_memories()` 基于关键词匹配和重要性排序进行检索。`recall_with_embedding()` 需要启用嵌入模型支持（可选，feature-gated）。

### 上游调用：构建记忆上下文

```rust
// memdir/mod.rs
pub fn build_memory_context_with(
    cwd: &Path,
    include_auto_memory: bool,
) -> Result<String> {
    // 1. 读取项目级记忆（按 cwd 哈希）
    // 2. 如果 include_auto_memory 为 true，读取全局记忆
    // 3. 读取团队记忆（如果 TeamMemory feature 启用）
    // 4. 合并所有记忆内容为格式化字符串
}
```

## 会话记忆上下文注入

在系统提示词组装时，记忆上下文通过多个通道注入到 `# Memory Context` 分区：

```rust
// system_prompt/mod.rs — build_system_prompt_with_memory_contexts()
let mut memory_context_parts = Vec::new();

// 通道 1：memory_context_override（如果外部预构建了记忆上下文）
let memory_context_result = memory_context_override
    .map(|context| Ok(context.to_string()))
    .unwrap_or_else(|| {
        allthecodes_session::memdir::build_memory_context_with(cwd_path, include_auto_memory)
    });

// 通道 2：session_memory_context（来自持久化会话洞察）
if let Some(context) = session_memory_context {
    memory_context_parts.push(context.to_string());
}

// 合并输出
if !memory_context_parts.is_empty() {
    parts.push(format!(
        "# Memory Context\n\n\
         The following memories may contain user preferences, project facts, \
         and durable context from previous work. ...\n\n{}",
        memory_context_parts.join("\n\n")
    ));
}
```

## 记忆的表面去重

在 `AppState`（`crates/allthecodes-engine/src/types/app_state.rs`）中维护了一个 `surfaced_memory_keys: HashSet<String>`，记录了本会话中已向模型展示过的记忆键。格式为 `<scope>:<key>`，避免同一段记忆在多个查询轮次中被重复注入。

```rust
pub struct AppState {
    // ...
    /// Memory identities already surfaced by relevant-memory recall in this
    /// session. Stored as `<scope>:<key>` to avoid repeating the same recall.
    pub surfaced_memory_keys: HashSet<String>,
    // ...
}
```

## 用户上下文注入（User Context）

除了直接注入到系统提示词的记忆内容外，User Context 还会通过 `<system-reminder>` 标签注入为首条用户消息：

```rust
// system_prompt/mod.rs
user_context.insert("cwd".to_string(), cwd.to_string());
user_context.insert("date".to_string(), chrono::Utc::now().format("%Y-%m-%d").to_string());
user_context.insert("platform".to_string(), std::env::consts::OS.to_string());
user_context.insert("model".to_string(), model.to_string());
```

## System Context（系统上下文）

当前 `build_system_prompt()` 返回的 `system_context` 是一个空 `HashMap`，预留用于未来扩展。在 TypeScript 原版中，system context 包含 git 状态快照和缓存破坏器等，这些信息在 Rust 移植版中已直接注入到动态分区（`git_status_section`）。

## 记忆的生命周期管理

- **创建**：通过 `create_memory()` 在工具执行或用户指令时写入
- **刷新**：`update_memory()` 更新内容并刷新 `updated_at` 时间戳
- **过期**：`ttl` 字段支持自动过期，`build_memory_context_with()` 会跳过过期条目
- **清除**：`/clear` 命令或 `delete_memory()` 显式删除
- **去重**：`surfaced_memory_keys` 确保每轮查询中同一条记忆只展示一次
