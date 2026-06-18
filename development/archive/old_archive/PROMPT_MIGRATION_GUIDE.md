# 提示词迁移指南 — TypeScript → Rust

> 基于 TS 源码全量审计。目标：Rust 侧的函数签名、段落名、组装流程与 TS 一一对应。

---

## 1. TS 提示词组装流程

### 1.1 入口调用链

```
QueryEngine.submitMessage()
  → getSystemPrompt(tools, model, dirs, mcpClients)     // constants/prompts.ts
      → 返回 string[]  (静态段落 + 动态段落)
  → buildEffectiveSystemPrompt({                          // utils/systemPrompt.ts
        defaultSystemPrompt,    // ← 上面的 string[]
        customSystemPrompt,     // --system-prompt
        appendSystemPrompt,     // --append-system-prompt
        overrideSystemPrompt,   // loop mode
        mainThreadAgentDefinition,
    })
      → 返回 SystemPrompt (branded string[])
  → buildSystemPromptBlocks(systemPrompt, cacheEnabled)   // services/api/claude.ts
      → 在 DYNAMIC_BOUNDARY 处拆分为 [{text, cache_control}, ...]
      → 发送给 API
```

### 1.2 getSystemPrompt() 返回的 string[] 结构

```typescript
return [
  // ─── 静态段落 (全局可缓存) ───
  getSimpleIntroSection(outputStyleConfig),        // 身份 + 安全
  getSimpleSystemSection(),                         // 系统规则
  getSimpleDoingTasksSection(),                     // 编码实践 (可被 outputStyle 跳过)
  getActionsSection(),                              // 危险操作
  getUsingYourToolsSection(enabledTools),            // 工具偏好
  getSimpleToneAndStyleSection(),                   // 语气
  getOutputEfficiencySection(),                     // 输出效率

  // ─── 缓存边界 ───
  ...(shouldUseGlobalCacheScope() ? [SYSTEM_PROMPT_DYNAMIC_BOUNDARY] : []),

  // ─── 动态段落 (通过 systemPromptSection 注册) ───
  ...resolvedDynamicSections,
].filter(s => s !== null)
```

### 1.3 buildEffectiveSystemPrompt() 优先级

```
优先级 0: overrideSystemPrompt   → 直接使用，忽略一切
优先级 1: coordinator 模式       → 替换 default
优先级 2: agent 系统提示词        → 替换 default (proactive 模式下追加)
优先级 3: customSystemPrompt     → 替换 default
优先级 4: defaultSystemPrompt    → 标准提示词

+ appendSystemPrompt 始终追加到末尾 (除 override)
```

---

## 2. 静态段落详解 (对应 Rust 函数)

每个段落是 `constants/prompts.ts` 中的独立函数。Rust 迁移时**保持同名同签名**。

### 2.1 getSimpleIntroSection(outputStyleConfig)

```
TS 位置: prompts.ts:175
TS 签名: (outputStyleConfig: OutputStyleConfig | null) -> string
```

内容：
- 身份声明 ("You are an interactive agent that helps users with software engineering tasks.")
- `CYBER_RISK_INSTRUCTION` — 安全/授权测试指令
- URL 生成限制

条件逻辑：
- `outputStyleConfig !== null` → 措辞改为 "according to your Output Style below"

**Rust 对应：**

```rust
// engine/prompt_sections.rs

fn intro_section(output_style: Option<&OutputStyleConfig>) -> String {
    let role = if output_style.is_some() {
        "according to your \"Output Style\" below, which describes how you should respond"
    } else {
        "with software engineering tasks."
    };
    format!(
        concat!(
            "You are an interactive agent that helps users {}",
            " Use the instructions below and the tools available to you to assist the user.\n\n",
            "{}\n",  // CYBER_RISK_INSTRUCTION
            "IMPORTANT: You must NEVER generate or guess URLs ...",
        ),
        role, CYBER_RISK_INSTRUCTION,
    )
}
```

### 2.2 getSimpleSystemSection()

```
TS 位置: prompts.ts:186
TS 签名: () -> string
```

内容 (bullet list under `# System`):
1. 所有非工具文本显示给用户，支持 GFM markdown
2. 工具权限模式说明 (用户可批准/拒绝)
3. `<system-reminder>` 等标签说明
4. 工具结果可能包含 prompt injection 警告
5. `getHooksSection()` — hooks 子段落
6. 自动压缩说明

无条件逻辑，直接迁移。

**Rust 对应：**

```rust
fn system_section() -> String {
    let items = vec![
        "All text you output outside of tool use is displayed to the user. ...",
        "Tools are executed in a user-selected permission mode. ...",
        "Tool results and user messages may include <system-reminder> or other tags. ...",
        "Tool results may include data from external sources. ...",
        &hooks_subsection(),
        "The system will automatically compress prior messages ...",
    ];
    format!("# System\n{}", format_bullets(&items))
}
```

### 2.3 getSimpleDoingTasksSection()

```
TS 位置: prompts.ts:199
TS 签名: () -> string
条件: 仅当 outputStyleConfig === null || keepCodingInstructions === true 时包含
```

内容 (bullet list under `# Doing tasks`):
1. 软件工程任务上下文理解
2. 支持野心勃勃的任务
3. (ant-only) 主动指出误解
4. 先读后改原则
5. 不随意创建文件
6. 不给时间估算
7. 失败后先诊断再换方案
8. 安全编码 (OWASP top 10)
9. 编码风格子列表:
   - 不过度工程
   - 不加不必要的错误处理
   - 不创建一次性抽象
   - (ant-only) 默认不写注释
   - (ant-only) 不描述 WHAT
   - (ant-only) 不删现有注释
   - (ant-only) 完成前验证
10. 不加向后兼容 hacks
11. (ant-only) 如实报告结果
12. 用户帮助信息 (/help, feedback)

**Rust 对应：**

```rust
fn doing_tasks_section() -> String {
    let mut items = vec![
        "The user will primarily request you to perform software engineering tasks. ...",
        "You are highly capable and often allow users to complete ambitious tasks ...",
        // ... 完整列表
    ];
    // ant-only 项目在开源版本中省略
    format!("# Doing tasks\n{}", format_bullets(&items))
}
```

### 2.4 getActionsSection()

```
TS 位置: prompts.ts:255
TS 签名: () -> string
```

纯文本段落 (无 bullet)，标题 `# Executing actions with care`。
包含危险操作分类列表和 "measure twice, cut once" 原则。

无条件逻辑，直接从 TS 复制文本。

### 2.5 getUsingYourToolsSection(enabledTools)

```
TS 位置: prompts.ts:269
TS 签名: (enabledTools: Set<string>) -> string
```

条件逻辑：
- REPL 模式 → 精简版 (仅 TaskCreate)
- 正常模式 → 完整工具偏好列表:
  - Read > cat/head/tail
  - Edit > sed/awk
  - Write > echo heredoc
  - Glob > find/ls (非 embedded 模式)
  - Grep > grep/rg (非 embedded 模式)
  - Bash 仅用于系统命令
- TaskCreate/TodoWrite 任务追踪指导
- 并行工具调用指导

**Rust 对应：**

```rust
fn using_tools_section(enabled_tools: &[&str]) -> String {
    // 根据 enabled_tools 动态生成工具偏好列表
    let mut items = vec![
        format!("Do NOT use the Bash to run commands when a relevant dedicated tool is provided. ..."),
        // 子列表: Read>cat, Edit>sed, Write>echo, Glob>find, Grep>grep
    ];
    if enabled_tools.contains(&"TaskCreate") {
        items.push("Break down and manage your work with the TaskCreate tool. ...".into());
    }
    items.push("You can call multiple tools in a single response. ...".into());
    format!("# Using your tools\n{}", format_bullets(&items))
}
```

### 2.6 getSimpleToneAndStyleSection()

```
TS 位置: prompts.ts:430
TS 签名: () -> string
```

Bullet list under `# Tone and style`:
1. 不用 emoji (除非用户要求)
2. (非 ant) 回复简短
3. 引用代码用 file_path:line_number
4. GitHub issue/PR 用 owner/repo#123
5. 工具调用前不用冒号

### 2.7 getOutputEfficiencySection()

```
TS 位置: prompts.ts:403
TS 签名: () -> string
```

条件逻辑：
- ant → `# Communicating with the user` (长段落，~200 字)
- 外部 → `# Output efficiency` (短版本，~100 字)

---

## 3. 动态段落系统 (对应 Rust 段落注册)

### 3.1 TS 段落注册机制

```typescript
// constants/systemPromptSections.ts

// 缓存型：计算一次，直到 /clear 或 /compact 清除
systemPromptSection('name', () => computeValue())

// 易变型：每轮重算，值变化时打破缓存 (名字带 DANGEROUS 警告)
DANGEROUS_uncachedSystemPromptSection('name', () => computeValue(), '原因')

// 解析所有段落
resolveSystemPromptSections(sections) → Promise<(string|null)[]>

// 清除缓存 (/clear, /compact 触发)
clearSystemPromptSections()
```

### 3.2 动态段落清单

```typescript
const dynamicSections = [
  systemPromptSection('session_guidance',    () => getSessionSpecificGuidanceSection(...)),
  systemPromptSection('memory',              () => loadMemoryPrompt()),
  systemPromptSection('env_info_simple',     () => computeSimpleEnvInfo(model, dirs)),
  systemPromptSection('language',            () => getLanguageSection(settings.language)),
  systemPromptSection('output_style',        () => getOutputStyleSection(outputStyleConfig)),
  DANGEROUS_uncachedSystemPromptSection(
    'mcp_instructions', () => getMcpInstructionsSection(mcpClients),
    'MCP servers connect/disconnect between turns',
  ),
  systemPromptSection('scratchpad',          () => getScratchpadInstructions()),
  systemPromptSection('frc',                 () => getFunctionResultClearingSection(model)),
  systemPromptSection('summarize_tool_results', () => SUMMARIZE_TOOL_RESULTS_SECTION),
  // ant-only:
  systemPromptSection('numeric_length_anchors', () => '...'),
  // feature-gated:
  systemPromptSection('token_budget',        () => '...'),
  systemPromptSection('brief',               () => getBriefSection()),
]
```

### 3.3 各段落内容

| 段落名 | 来源函数 | 内容摘要 |
|--------|---------|---------|
| `session_guidance` | `getSessionSpecificGuidanceSection()` | AskUser 使用、Agent 并行、Explore 搜索指导、Skill 列表 |
| `memory` | `loadMemoryPrompt()` (memdir.ts) | MEMORY.md 内容 (≤200行/25KB) |
| `env_info_simple` | `computeSimpleEnvInfo()` | cwd, git?, platform, shell, OS version, model, cutoff |
| `language` | `getLanguageSection()` | 语言偏好 (如 "Respond in 中文") |
| `output_style` | `getOutputStyleSection()` | 自定义输出风格 |
| `mcp_instructions` | `getMcpInstructionsSection()` | 已连接 MCP 服务器的 instructions |
| `scratchpad` | `getScratchpadInstructions()` | 草稿目录路径和使用说明 |
| `frc` | `getFunctionResultClearingSection()` | 模型相关的函数结果清理指令 |
| `summarize_tool_results` | 常量 | "When working with tool results..." |

---

## 4. 工具提示词 (每个 Tool 的 prompt.ts)

### 4.1 TS 模式

每个工具在 `tools/XxxTool/prompt.ts` 中导出提示词函数：

```typescript
// 模式 A: 纯函数
export function getSimplePrompt(): string { return `...` }

// 模式 B: 模板函数 (运行时参数)
export function renderPromptTemplate(lineFormat, maxSize, offset): string { ... }

// 模式 C: 常量
export const DESCRIPTION = 'Read a file from the local filesystem.'
```

工具的 `Tool.prompt` 属性调用这些函数：

```typescript
// tools/FileReadTool/FileReadTool.ts
prompt: renderPromptTemplate(lineFormat, maxSizeInstruction, offsetInstruction)
```

### 4.2 Rust 对应方式

Rust 中工具通过 `async fn prompt(&self) -> String` 返回提示词。
迁移时**直接从 TS 的 prompt.ts 复制文本到 Rust 的 prompt() 方法**：

```rust
// tools/file_read.rs
async fn prompt(&self) -> String {
    format!(
        concat!(
            "Reads a file from the local filesystem. You can access any file directly.\n",
            "Assume this tool is able to read all files on the machine.\n\n",
            "Usage:\n",
            "- The file_path parameter must be an absolute path, not a relative path\n",
            "- By default, it reads up to {} lines starting from the beginning of the file\n",
            // ... 从 TS renderPromptTemplate() 复制
        ),
        MAX_LINES_TO_READ,
    )
}
```

### 4.3 工具提示词迁移清单

TS prompt.ts 与 Rust prompt() 的对应关系：

| TS 文件 | TS 导出函数 | Rust 文件 | 预估行数 |
|---------|-----------|-----------|---------|
| `BashTool/prompt.ts` | `getSimplePrompt()` | `tools/bash.rs` | ~370 |
| `FileReadTool/prompt.ts` | `renderPromptTemplate()` | `tools/file_read.rs` | ~40 |
| `FileWriteTool/prompt.ts` | `getWriteToolDescription()` | `tools/file_write.rs` | ~30 |
| `FileEditTool/prompt.ts` | `getEditToolPrompt()` | `tools/file_edit.rs` | ~40 |
| `GlobTool/prompt.ts` | `getGlobToolPrompt()` | `tools/glob_tool.rs` | ~15 |
| `GrepTool/prompt.ts` | `getGrepToolPrompt()` | `tools/grep.rs` | ~30 |
| `AgentTool/prompt.ts` | `getAgentToolSection()` | `tools/agent.rs` | ~60 |
| `NotebookEditTool/prompt.ts` | — | `tools/notebook_edit.rs` | ~20 |
| `AskUserQuestionTool/prompt.ts` | — | `tools/ask_user.rs` | ~10 |
| `ToolSearchTool/prompt.ts` | — | `tools/tool_search.rs` | ~15 |
| `SkillTool/prompt.ts` | — | `tools/skill.rs` | ~20 |
| `EnterPlanModeTool/prompt.ts` | — | `tools/plan_mode.rs` | ~30 |
| `ExitPlanModeTool/prompt.ts` | — | `tools/plan_mode.rs` | ~15 |
| `EnterWorktreeTool/prompt.ts` | — | `tools/worktree.rs` | ~20 |
| `ExitWorktreeTool/prompt.ts` | — | `tools/worktree.rs` | ~15 |
| `WebFetchTool/prompt.ts` | — | `tools/web_fetch.rs` | ~20 |
| `WebSearchTool/prompt.ts` | — | `tools/web_search.rs` | ~15 |
| `LSPTool/prompt.ts` | — | `tools/lsp.rs` | ~30 |
| `TaskCreateTool/prompt.ts` | — | `tools/tasks.rs` | ~15 |
| `TaskGetTool/prompt.ts` | — | `tools/tasks.rs` | ~10 |
| `TaskUpdateTool/prompt.ts` | — | `tools/tasks.rs` | ~15 |
| `TaskListTool/prompt.ts` | — | `tools/tasks.rs` | ~10 |
| `TaskStopTool/prompt.ts` | — | `tools/tasks.rs` | ~10 |
| `TaskOutputTool/prompt.ts` | — | `tools/tasks.rs` | ~10 |

---

## 5. 上下文注入

### 5.1 CLAUDE.md 层级

```
TS 位置: utils/claudemd.ts
TS 函数: getClaudeMdFiles(cwd, roots)
```

加载顺序 (优先级递增):
1. `/etc/claude-code/CLAUDE.md` — 管理员全局
2. `~/.cc-rust/CLAUDE.md` — 用户全局
3. 从 git root 到 cwd 的每级 `CLAUDE.md` 和 `.cc-rust/CLAUDE.md`
4. `.cc-rust/rules/*.md` — 项目规则文件
5. `CLAUDE.local.md` — 本地私有 (.gitignore 友好)

**@include 指令解析:**
- `@path` → 绝对路径
- `@./relative` → 相对于当前 CLAUDE.md 所在目录
- `@~/path` → HOME 相对
- 循环引用检测、不存在文件静默跳过

**Rust 现状:** `config/claude_md.rs` 仅实现了基本层级遍历，缺少 rules/*、local、@include。

### 5.2 MEMORY.md

```
TS 位置: memdir/memdir.ts
TS 函数: loadMemoryPrompt()
常量: MAX_ENTRYPOINT_LINES = 200, MAX_ENTRYPOINT_BYTES = 25_000
```

读取 `~/.cc-rust/memory/MEMORY.md`，截断后注入为动态段落。

**Rust 对应:** `session/memdir.rs` 已有 CRUD，需要 `load_memory_prompt()` 函数 + 接入 `env_info` 段落。

---

## 6. Rust 侧实现计划

### 6.1 新增文件结构

```
engine/
├── system_prompt.rs        ← 现有，重构为调用 prompt_sections
├── prompt_sections.rs      ← 新增：段落注册 + 缓存 + 解析
└── prompt_constants.rs     ← 新增：CYBER_RISK_INSTRUCTION 等常量
```

### 6.2 prompt_sections.rs — 段落注册系统

与 TS `systemPromptSections.ts` 一一对应：

```rust
//! 段落注册与缓存系统。
//! 对应 TS: constants/systemPromptSections.ts

use std::sync::{LazyLock, Mutex};
use std::collections::HashMap;

/// 缓存边界标记
pub const DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

/// 段落计算函数类型
type ComputeFn = Box<dyn Fn() -> Option<String> + Send + Sync>;

struct PromptSection {
    name: String,
    compute: ComputeFn,
    cache_break: bool,  // true = DANGEROUS_uncached
}

/// 段落缓存
static SECTION_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 对应 TS: systemPromptSection(name, compute)
pub fn cached_section(name: &str, compute: impl Fn() -> Option<String> + Send + Sync + 'static)
    -> PromptSection { ... }

/// 对应 TS: DANGEROUS_uncachedSystemPromptSection(name, compute, reason)
pub fn uncached_section(name: &str, compute: impl Fn() -> Option<String> + Send + Sync + 'static,
    _reason: &str) -> PromptSection { ... }

/// 对应 TS: resolveSystemPromptSections(sections)
pub fn resolve_sections(sections: &[PromptSection]) -> Vec<String> { ... }

/// 对应 TS: clearSystemPromptSections()  (由 /clear, /compact 调用)
pub fn clear_cache() { ... }
```

### 6.3 system_prompt.rs — 重构后的组装函数

与 TS `getSystemPrompt()` + `buildEffectiveSystemPrompt()` 一一对应：

```rust
//! 对应 TS: constants/prompts.ts::getSystemPrompt()
//!        + utils/systemPrompt.ts::buildEffectiveSystemPrompt()

use crate::engine::prompt_sections::*;

// ─── 静态段落函数 (与 TS 同名) ───

/// 对应 TS: getSimpleIntroSection(outputStyleConfig)
fn intro_section(output_style: Option<&OutputStyleConfig>) -> String { ... }

/// 对应 TS: getSimpleSystemSection()
fn system_section() -> String { ... }

/// 对应 TS: getSimpleDoingTasksSection()
fn doing_tasks_section() -> String { ... }

/// 对应 TS: getActionsSection()
fn actions_section() -> &'static str { ... }

/// 对应 TS: getUsingYourToolsSection(enabledTools)
fn using_tools_section(enabled_tools: &[&str]) -> String { ... }

/// 对应 TS: getSimpleToneAndStyleSection()
fn tone_and_style_section() -> String { ... }

/// 对应 TS: getOutputEfficiencySection()
fn output_efficiency_section() -> &'static str { ... }

// ─── 动态段落计算函数 (与 TS 同名) ───

/// 对应 TS: getSessionSpecificGuidanceSection(enabledTools, skillCommands)
fn session_guidance_section(enabled_tools: &[&str], skill_commands: &[String]) -> Option<String> { ... }

/// 对应 TS: computeSimpleEnvInfo(model, dirs)
fn env_info_section(model: &str, cwd: &str) -> String { ... }

/// 对应 TS: getLanguageSection(language)
fn language_section(language: Option<&str>) -> Option<String> { ... }

/// 对应 TS: getMcpInstructionsSection(mcpClients)
fn mcp_instructions_section(mcp_clients: &[McpClient]) -> Option<String> { ... }

// ─── 主组装函数 ───

/// 对应 TS: getSystemPrompt(tools, model, dirs, mcpClients)
pub fn get_system_prompt(
    tools: &Tools,
    model: &str,
    cwd: &str,
    mcp_clients: &[McpClient],
) -> Vec<String> {
    let enabled_tools: Vec<&str> = tools.iter().map(|t| t.name()).collect();

    // 静态段落
    let mut parts: Vec<Option<String>> = vec![
        Some(intro_section(None)),
        Some(system_section()),
        Some(doing_tasks_section()),       // TODO: outputStyle 门控
        Some(actions_section().to_string()),
        Some(using_tools_section(&enabled_tools)),
        Some(tone_and_style_section()),
        Some(output_efficiency_section().to_string()),
    ];

    // 缓存边界
    parts.push(Some(DYNAMIC_BOUNDARY.to_string()));

    // 动态段落
    let dynamic = vec![
        cached_section("session_guidance", || session_guidance_section(...)),
        cached_section("memory",           || load_memory_prompt()),
        cached_section("env_info_simple",  || Some(env_info_section(model, cwd))),
        cached_section("language",         || language_section(language)),
        uncached_section("mcp_instructions",
            || mcp_instructions_section(mcp_clients),
            "MCP servers connect/disconnect between turns",
        ),
    ];
    let resolved = resolve_sections(&dynamic);

    let mut result: Vec<String> = parts.into_iter().flatten().collect();
    result.extend(resolved);
    result
}

/// 对应 TS: buildEffectiveSystemPrompt({...})
pub fn build_effective_system_prompt(
    default_prompt: Vec<String>,
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    override_prompt: Option<&str>,
    agent_definition: Option<&AgentDefinition>,
) -> Vec<String> {
    // 优先级 0: override
    if let Some(ov) = override_prompt {
        return vec![ov.to_string()];
    }

    // 优先级 2: agent (替换 default)
    // 优先级 3: custom (替换 default)
    // 优先级 4: default
    let mut base = if let Some(agent) = agent_definition {
        vec![agent.system_prompt.clone()]
    } else if let Some(custom) = custom_prompt {
        vec![custom.to_string()]
    } else {
        default_prompt
    };

    // 追加
    if let Some(append) = append_prompt {
        base.push(append.to_string());
    }

    base
}
```

### 6.4 工具提示词迁移方法

每个工具的 prompt() 直接从 TS 的 `prompt.ts` 复制文本。示例：

```rust
// tools/file_read.rs — 对应 TS: tools/FileReadTool/prompt.ts::renderPromptTemplate()

const MAX_LINES_TO_READ: usize = 2000;

async fn prompt(&self) -> String {
    format!(
        "Reads a file from the local filesystem. You can access any file directly by using this tool.\n\
         Assume this tool is able to read all files on the machine. ...\n\n\
         Usage:\n\
         - The file_path parameter must be an absolute path, not a relative path\n\
         - By default, it reads up to {} lines starting from the beginning of the file\n\
         - When you already know which part of the file you need, only read that part. ...\n\
         - Results are returned using cat -n format, with line numbers starting at 1\n\
         - This tool can read images (PNG, JPG, etc). ...\n\
         - This tool can read PDF files (.pdf). ...\n\
         - This tool can read Jupyter notebooks (.ipynb). ...\n\
         - This tool can only read files, not directories. To read a directory, use an ls command via the Bash tool.\n\
         - You will regularly be asked to read screenshots. ...\n\
         - If you read a file that exists but has empty contents you will receive a system reminder warning.",
        MAX_LINES_TO_READ,
    )
}
```

**关键：** 不要改写文本内容，直接复制 TS 原文。提示词文本经过大量 A/B 测试调优，每个措辞都有意义。

---

## 7. TS 函数 → Rust 函数 映射表

| TS 函数 | TS 文件 | Rust 函数 | Rust 文件 |
|---------|---------|-----------|-----------|
| `getSystemPrompt()` | `constants/prompts.ts:444` | `get_system_prompt()` | `engine/system_prompt.rs` |
| `buildEffectiveSystemPrompt()` | `utils/systemPrompt.ts:41` | `build_effective_system_prompt()` | `engine/system_prompt.rs` |
| `getSimpleIntroSection()` | `prompts.ts:175` | `intro_section()` | `engine/system_prompt.rs` |
| `getSimpleSystemSection()` | `prompts.ts:186` | `system_section()` | `engine/system_prompt.rs` |
| `getSimpleDoingTasksSection()` | `prompts.ts:199` | `doing_tasks_section()` | `engine/system_prompt.rs` |
| `getActionsSection()` | `prompts.ts:255` | `actions_section()` | `engine/system_prompt.rs` |
| `getUsingYourToolsSection()` | `prompts.ts:269` | `using_tools_section()` | `engine/system_prompt.rs` |
| `getSimpleToneAndStyleSection()` | `prompts.ts:430` | `tone_and_style_section()` | `engine/system_prompt.rs` |
| `getOutputEfficiencySection()` | `prompts.ts:403` | `output_efficiency_section()` | `engine/system_prompt.rs` |
| `getSessionSpecificGuidanceSection()` | `prompts.ts:344` | `session_guidance_section()` | `engine/system_prompt.rs` |
| `computeSimpleEnvInfo()` | `prompts.ts:651` | `env_info_section()` | `engine/system_prompt.rs` |
| `getLanguageSection()` | `prompts.ts` | `language_section()` | `engine/system_prompt.rs` |
| `getMcpInstructionsSection()` | `prompts.ts:579` | `mcp_instructions_section()` | `engine/system_prompt.rs` |
| `loadMemoryPrompt()` | `memdir/memdir.ts` | `load_memory_prompt()` | `session/memdir.rs` |
| `systemPromptSection()` | `systemPromptSections.ts:20` | `cached_section()` | `engine/prompt_sections.rs` |
| `DANGEROUS_uncachedSystemPromptSection()` | `systemPromptSections.ts:32` | `uncached_section()` | `engine/prompt_sections.rs` |
| `resolveSystemPromptSections()` | `systemPromptSections.ts:43` | `resolve_sections()` | `engine/prompt_sections.rs` |
| `clearSystemPromptSections()` | `systemPromptSections.ts:65` | `clear_cache()` | `engine/prompt_sections.rs` |
| `buildSystemPromptBlocks()` | `services/api/claude.ts` | `build_system_prompt_blocks()` | `api/client.rs` |
| `getClaudeMdFiles()` | `utils/claudemd.ts` | `find_claude_md_files()` | `config/claude_md.rs` |
| `renderPromptTemplate()` (Read) | `FileReadTool/prompt.ts` | `prompt()` | `tools/file_read.rs` |
| `getSimplePrompt()` (Bash) | `BashTool/prompt.ts` | `prompt()` | `tools/bash.rs` |
| `getWriteToolDescription()` | `FileWriteTool/prompt.ts` | `prompt()` | `tools/file_write.rs` |
| `getEditToolPrompt()` | `FileEditTool/prompt.ts` | `prompt()` | `tools/file_edit.rs` |

---

## 8. 迁移执行顺序

按依赖关系排列，每步可独立编译测试：

```
Step 1: 段落注册系统骨架
  新增 engine/prompt_sections.rs (cached_section, uncached_section, resolve, clear)
  ~80 行

Step 2: 静态段落文本
  扩展 engine/system_prompt.rs:
    intro_section()
    system_section()
    doing_tasks_section()
    actions_section()
    using_tools_section()
    tone_and_style_section()
    output_efficiency_section()
  ~300 行 (从 TS prompts.ts 复制文本)

Step 3: 重构 get_system_prompt()
  改为: 静态段落 + BOUNDARY + 动态段落 的组装模式
  改为: build_effective_system_prompt() 优先级逻辑
  ~60 行

Step 4: 动态段落实现
  session_guidance_section()
  env_info_section() (扩展: git, shell, OS version)
  language_section()
  mcp_instructions_section()
  load_memory_prompt() (接入 session/memdir.rs)
  ~150 行

Step 5: 工具提示词迁移 (最大工作量)
  按优先级: Bash → FileRead/Write/Edit → Glob/Grep → Agent → 其余
  ~1200 行 (从各 TS prompt.ts 复制文本)

Step 6: 上下文增强
  claude_md.rs: 支持 rules/*.md, CLAUDE.local.md, @include
  ~100 行
```

**总计:** ~1890 行，跨 ~20 文件。
