---
title: "系统提示词组装 — 静态/动态分区与缓存策略"
description: "allthecodes（Claude Code Rust 移植版）System Prompt 的动态组装过程。涵盖静态分区与动态分区的划分、DYNAMIC_BOUNDARY 边界标记、Section 缓存注册表、AGENTS.md 与记忆上下文的注入位置。"
keywords: ["System Prompt", "系统提示词", "动态组装", "CLAUDE.md", "Prompt Cache", "缓存策略", "allthecodes"]
---

<!--
本文对应源文件：
  crates/allthecodes-engine/src/system_prompt/mod.rs         — 主组装函数
  crates/allthecodes-engine/src/system_prompt/static_sections.rs  — 静态分区
  crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs — 动态分区计算
  crates/allthecodes-engine/src/prompt_sections.rs              — Section 缓存注册表
  crates/allthecodes-engine/src/types/config.rs                 — QuerySource 定义
  crates/allthecodes-engine/src/types/app_state.rs              — AppState 运行时状态
  crates/allthecodes-config/src/claude_md.rs                    — AGENTS.md 加载
-->

## 从片段到 API 调用：系统提示词的完整链路

allthecodes 的系统提示词（System Prompt）不是一段写死的文本，而是一个 **`Vec<String>` 数组**，经过多层组装后发送给 API。

### 核心函数

```rust
// mod.rs — 公共入口
pub fn build_system_prompt_with_memory_contexts(
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    tools: &[Arc<dyn Tool>],
    model: &str,
    cwd: &str,
    language: Option<&str>,
    output_style: Option<&str>,
    include_auto_memory: bool,
    memory_context_override: Option<&str>,   // 外部预构建记忆上下文
    session_memory_context: Option<&str>,    // 会话洞察
) -> (Vec<String>, HashMap<String, String>, HashMap<String, String>)
```

返回值由三部分组成：
1. `Vec<String>` — 系统提示词片段（每个元素为一个分区）
2. `HashMap<String, String>` — 用户上下文（注入为首条 `<system-reminder>` 消息）
3. `HashMap<String, String>` — 系统上下文（当前为预留空 Map）

## 优先级体系

### build_effective_system_prompt()

```rust
pub fn build_effective_system_prompt(
    default_prompt: Vec<String>,
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    override_prompt: Option<&str>,
    agent_prompt: Option<&str>,
) -> Vec<String>
```

五级优先级：

| 优先级 | 条件 | 行为 |
|--------|------|------|
| 0. Override | `override_prompt` 非空 | 完全替换为单元素 `[override]` |
| 1. Agent | `agent_prompt` 非空 | 替换默认提示词 |
| 2. Custom | `custom_prompt` 非空 | 替换默认提示词 |
| 3. Default | 无特殊条件 | 使用 `build_system_prompt_with_memory_contexts()` 完整输出 |
| + Append | `append_prompt` 非空 | 始终追加在末尾（Override 除外） |

## 默认提示词的组装顺序

当没有自定义/覆盖提示词时，默认提示词按以下顺序组装：

### 静态分区（可缓存）

位于 `DYNAMIC_BOUNDARY` 标记之前。这部分内容在同一模型下跨会话不变，理论上可共享全局缓存。

| 分区 | 源函数 | 内容 |
|------|--------|------|
| Intro Section | `intro_section()` | 基础角色定义 + 网络风险指令 + 安全工作说明 |
| System Section | `system_section()` | 系统级行为规范（8 条规则：通信格式、权限模式、Hook 理解等） |
| Doing Tasks | `doing_tasks_section()` | 软件工程任务规范（13 条规则） |
| Actions Section | `actions_section()` | 谨慎行动指南：破坏性操作的反转成本和确认流程 |
| Using Tools | `using_tools_section()` | 工具使用规范：优先使用专用工具而非 Bash |
| Tone & Style | `tone_and_style_section()` | 沟通风格规范（5 条规则） |
| Output Efficiency | `output_efficiency_section()` | 输出效率指南（简短、直接、无废话） |

### 动态边界标记

```rust
// prompt_sections.rs
pub const DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";
```

此标记是**分界点**，不会发送给 AI。在 Rust 移植版中，它仅作为静态分区和动态分区的分割标记，供 API 层在构建 `cache_control` 时使用。

### 动态分区（会话特定）

每个分区通过 `prompt_sections` 注册表管理：

```rust
let dynamic_sections = vec![
    cached_section("env_info_simple", || Some(env_info_section(&model, &cwd))),
    cached_section("git_status", || git_status_section(&cwd)),
    uncached_section("language", || language_section(language), "..."),
    uncached_section("output_style", || output_style_resolution(name, &cwd), "..."),
    cached_section("summarize_tool_results", || Some(SUMMARIZE_TOOL_RESULTS)),
    uncached_section("mcp_instructions", mcp_instructions_section, "MCP servers connect/disconnect"),
    cached_section("brief_mode", brief_mode_section),
    cached_section("proactive_mode", proactive_mode_section),
    cached_section("external_channels", external_channels_section),
    uncached_section("coordinator_mode", coordinator_prompt_section, "..."),
    cached_section("subsystem_status", build_subsystem_status_reminder),
];
```

### 动态分区详解

| 分区名 | 计算函数 | 缓存 | 内容 |
|--------|----------|------|------|
| `env_info_simple` | `env_info_section()` | 缓存（会话级） | 工作目录、平台、Shell、模型信息、知识截止日期 |
| `git_status` | `git_status_section()` | 缓存（会话级） | Git 分支、默认分支、User、状态快照（20 文件）、最近 10 条提交 |
| `language` | `language_section()` | 不缓存 | 用户指定语言（如"始终用中文回答"） |
| `output_style` | `resolve_with_diagnostic()` | 不缓存 | 从文件系统读取的输出风格配置 |
| `summarize_tool_results` | 常量 `SUMMARIZE_TOOL_RESULTS` | 缓存 | 提示 AI 在工具结果中记录重要信息 |
| `mcp_instructions` | `mcp_instructions_section()` | 不缓存 | MCP 服务器指令（当前返回 None） |
| `brief_mode` | `brief_mode_section()` | 缓存 | Brief 工具独占输出模式（feature-gated） |
| `proactive_mode` | `proactive_mode_section()` | 缓存 | 主动模式：Tick 驱动、终端焦点感知（feature-gated） |
| `external_channels` | `external_channels_section()` | 缓存 | 外部频道消息处理（feature-gated） |
| `coordinator_mode` | `coordinator_prompt_section()` | 不缓存 | 协调者模式提示词（feature-gated） |
| `subsystem_status` | `build_subsystem_status_reminder()` | 缓存 | 活跃子系统概览（LSP/MCP/Plugin/Skill/Agent 数量） |

### 工具描述的动态追加

在动态分区之后，系统会追加启用的工具列表及其输入 JSON Schema：

```rust
let enabled: Vec<&Arc<dyn Tool>> = tools.iter().filter(|t| t.is_enabled()).collect();
if !enabled.is_empty() {
    let mut tool_section = String::from("\n# Available tools\n");
    for tool in &enabled {
        tool_section.push_str(&format!("\n## {}\n", tool.name()));
        tool_section.push_str(&format!("Input schema: {}\n", serde_json::to_string(&schema)?));
    }
    parts.push(tool_section);
}
```

### 计算机使用与浏览器分区

当检测到计算机使用（Computer Use）工具或浏览器 MCP 工具时，自动插入对应分区：

```rust
// Computer Use system prompt（检测 CU 工具前缀）
if let Some(cu_prompt) = computer_use_system_prompt(tools) {
    parts.push(cu_prompt);
}

// Browser automation（通过 browser_server_snapshot() 检测）
if let Some(browser_prompt) = browser_system_prompt(tools, &browser_servers) {
    parts.push(browser_prompt);
}
```

### 后期注入分区

在所有动态分区之后，注入以下内容：

1. **AGENTS.md 上下文** — `# Project Instructions (AGENTS.md)` 分区
2. **记忆上下文** — `# Memory Context` 分区（详见 project-memory.md）
3. **追加提示词** — `append_prompt` 始终在最后

## Section 缓存注册表

源文件：`prompt_sections.rs`

### 两个工厂函数

```rust
// 缓存式 Section：计算一次，/clear 或 /compact 后重新计算
pub fn cached_section(
    name: &str,
    compute: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> PromptSection;

// 危险：每轮重新计算，会破坏 Prompt Cache
pub fn uncached_section(
    name: &str,
    compute: impl Fn() -> Option<String> + Send + Sync + 'static,
    reason: &str,  // 必须给出破坏缓存的理由
) -> PromptSection;
```

### 缓存机制

```rust
static SECTION_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> = ...;

pub fn resolve_sections(sections: &[PromptSection]) -> Vec<String> {
    // 对于 cached_section → 检查缓存，存在且非 None 则直接返回
    // 对于 uncached_section → 始终重新计算
    // 结果存入缓存（uncached 也存，但下次会被覆盖）
}

pub fn clear_cache() {
    // 在 /clear 和 /compact 时调用
}
```

## Git Status 快照

`git_status_section()` 是动态分区中最复杂的函数。它通过 `git2` crate 直接读取仓库状态：

```rust
pub(super) fn git_status_section(cwd: &str) -> Option<String> {
    // 1. 检查是否为 git 仓库
    // 2. 读取当前分支和默认分支
    // 3. 读取 Git User Name（来自 git config）
    // 4. 获取文件状态（porcelain 风格，最多 20 个文件）
    // 5. 获取最近 10 条提交
    // 6. 拼接为 gitStatus 格式字符串
}
```

### 文件状态分类

```rust
// 每个文件附带状态前缀：
// staged:   "M " (已暂存修改), "D " (已暂存删除), "R " (重命名)
//           "MM" (已暂存且再次修改), "UU" (冲突)
// unstaged: " M" (未暂存修改), " D" (未暂存删除)
// untracked: "??" (未追踪)
```

## 输出风格解析

当用户配置了 `output_style` 时，系统调用 `resolve_with_diagnostic()` 从文件系统读取输出风格配置并生成对应分区。

## 环境信息分区

```rust
pub(super) fn env_info_section(model: &str, cwd: &str) -> String {
    "<env>\n\
     Working directory: {cwd}\n\
     Is directory a git repo: {is_git}\n\
     Platform: {platform}\n\
     Shell: {shell}\n\
     </env>\n\
     You are powered by the model {model_name}."
}
```

## 设计要点

- **静态分区 vs 动态分区**：静态分区（Intro、System、Rules 等）在同一模型版本下内容固定，可利用 Anthropic Prompt Cache 的 `scope: 'global'` 进行跨组织缓存。动态分区因包含会话特定内容（git 状态、环境信息、Feature 门控条件），只能使用会话级或组织级缓存。
- **不缓存分区的选择**：`language`、`output_style`、`mcp_instructions`、`coordinator_mode` 使用 `uncached_section()`，因为这些内容可能在会话中变化，缓存会导致过时指令。
- **CLAUDE_CODE_SIMPLE 模式**：Rust 移植版目前未实现此快速路径，所有分区完整组装。
