# EXPERIMENTAL_SKILL_SEARCH — 实验性技能搜索

> 功能标志：无（技能系统基础设施）
> 实现状态：完整可用
> 源码文件数：4

## 一、功能概述

实验性技能搜索（Experimental Skill Search）是 allthecodes 技能系统的一部分，提供技能发现、注册、追踪和检索能力。技能（Skill）是可复用的提示模板，扩展了 Agent 的能力范围，包括内置技能、用户自定义技能、项目技能和插件技能。

### 核心能力

- **技能发现**：从多个来源加载和注册技能
- **使用追踪**：记录技能调用频率，应用时间衰减算法排序
- **依赖解析**：技能间依赖版本管理和拓扑排序
- **模式匹配**：根据文件路径模式自动推荐相关技能
- **模型调用列表**：构建可供模型调用的技能列表

## 二、实现架构

### 2.1 技能定义

文件：`crates/allthecodes-skills/src/lib.rs`

```rust
pub struct SkillDefinition {
    pub name: String,             // 规范名称，如 "commit"、"debug"
    pub source: SkillSource,      // 来源：Bundled/User/Project/Plugin/Mcp
    pub base_dir: Option<PathBuf>, // 基础目录
    pub frontmatter: SkillFrontmatter, // 前端元数据
    pub prompt_body: String,      // Markdown 提示主体
}
```

### 2.2 技能来源

```rust
pub enum SkillSource {
    Bundled,                    // 内置于应用中
    User,                       // 用户目录 ~/.allthecodes/skills/
    Project,                    // 项目目录 .allthecodes/skills/
    Plugin(String),             // 插件注册
    Mcp(String),                // MCP 服务器注册
}
```

### 2.3 Frontmatter 元数据

```rust
pub struct SkillFrontmatter {
    pub name: Option<String>,           // 显示名称
    pub description: String,            // 人类可读描述
    pub when_to_use: Option<String>,    // 何时使用
    pub allowed_tools: Vec<String>,     // 允许的工具（空 = 全部）
    pub argument_hint: Option<String>,  // 参数提示
    pub argument_names: Vec<String>,    // 命名参数
    pub model: Option<String>,          // 模型覆盖
    pub user_invocable: bool,           // 用户可通过 /name 调用
    pub disable_model_invocation: bool, // 禁止模型自动调用
    pub context: SkillContext,          // Inline | Fork
    pub agent: Option<String>,          // Fork 执行的 Agent 类型
    pub effort: Option<String>,         // 努力级别
    pub version: Option<String>,        // 语义版本
    pub compatible_app_version: Option<String>, // 兼容应用版本
    pub dependencies: Vec<SkillDependency>,      // 依赖
    pub paths: Vec<String>,             // 文件路径模式（触发推荐）
    pub assets: Vec<String>,            // 资源文件
    pub entry_docs: Vec<String>,        // 参考文档
}
```

### 2.4 执行上下文

```rust
pub enum SkillContext {
    Inline,  // 当前对话中展开提示（默认）
    Fork,    // 运行在 Fork 子 Agent 中
}
```

Inline 模式直接将技能提示注入为元用户消息。Fork 模式在独立的子 QueryEngine 中执行。

### 2.5 技能注册表

全局技能注册表维护所有已加载的技能：

```rust
static REGISTRY: LazyLock<Mutex<Vec<SkillDefinition>>> = ...;
static REGISTRY_DIAGNOSTICS: LazyLock<Mutex<Vec<SkillDiagnostic>>> = ...;
static REGISTRY_REVISION: AtomicU64 = ...;
```

主要操作：

| 函数 | 功能 |
|------|------|
| `register_skill()` | 注册单个技能（首次注册优先） |
| `register_skills_resolved()` | 批量注册并解析依赖 |
| `get_all_skills()` | 获取所有注册技能 |
| `find_skill(name)` | 按名称查找技能 |
| `get_user_invocable_skills()` | 获取用户可调用的技能列表 |
| `get_model_invocable_skills()` | 获取模型可调用的技能列表 |

### 2.6 技能加载器

文件：`crates/allthecodes-skills/src/loader.rs`

从目录中加载 `SKILL.md` 文件：

```
{skills_dir}/
  commit/SKILL.md          → 技能 "commit"
  debug/SKILL.md           → 技能 "debug"
  review/SKILL.md          → 技能 "review"
```

```rust
pub fn init_skills(user_skills_dir: &Path, project_dir: Option<&Path>)
pub fn reload_skills_with_extra(
    user_skills_dir: &Path,
    project_dir: Option<&Path>,
    extra_skills: Vec<SkillDefinition>,
    options: SkillLoadOptions,
) -> SkillLoadReport
```

### 2.7 依赖解析与验证

技能系统支持完整的依赖管理：

**验证项**：
- 技能名格式（字母数字 + `-` `_` `:` `.`）
- 描述非空
- 版本格式（语义版本号）
- 应用版本兼容性
- 依赖存在性
- 依赖版本匹配
- 依赖循环检测

**拓扑排序**：`topo_sort_skills()` 确保依赖项先于依赖者加载。

**版本比较**：支持 `=`, `>`, `>=`, `<`, `<=`, `^`, `~` 操作符。

### 2.8 使用追踪

文件：`crates/allthecodes-skills/src/usage.rs`

`SkillUsageTracker` 提供基于时间衰减的使用评分：

```rust
pub struct SkillUsageTracker {
    inner: HashMap<String, SkillUsageData>,
    last_invocation: HashMap<String, Instant>,  // 60 秒防抖
}
```

**评分公式**：

```
decay_factor = 0.5^(hours_since_last / 168)    // 7 天半衰期
new_score    = old_score * decay_factor + (1.0 - decay_factor) * 1.0
```

**持久化**：
- `save(path)` — 序列化为 JSON 文件
- `load(path)` — 从 JSON 文件加载
- `merge(other)` — 合并两个追踪器（高调用计数优先）

### 2.9 技能调用准备

文件：`crates/allthecodes-skills/src/invocation.rs`

```rust
pub fn prepare_skill_invocation(
    skill: &SkillDefinition,
    args: &str,
    main_loop_model: &str,
    session_id: Option<&str>,
) -> PreparedSkillInvocation
```

根据上下文类型返回不同的准备结果：

```rust
pub enum PreparedSkillInvocation {
    Inline {
        data: Value,
        new_messages: Vec<Message>,  // 包含展开后的提示
    },
    Fork {
        data: Value,
        request: SkillForkRequest,   // Fork 执行请求
    },
}
```

### 2.10 模型技能列表构建

```rust
pub fn build_model_skills_listing() -> String
pub fn build_skill_tool_prompt() -> String
```

为模型构建可调用的技能列表，包含描述和使用条件，注入到系统提示中。

## 三、内置技能示例

file `crates/allthecodes-skills/src/bundled.rs` 定义了多个内置技能：

| 技能名 | 描述 | 工具 |
|--------|------|------|
| `debug` | 诊断和修复问题 | Read, Grep, Glob, Bash |
| `stuck` | 遇到困难时寻求帮助 | — |
| `explain` | 解释代码或概念 | Read |
| `simplify` | 简化或重构代码 | Read, Edit, Bash |
| `test` | 编写和运行测试 | Read, Write, Bash |
| `review` | 审查代码变更 | Read, Grep, Bash |

## 四、关键设计决策

1. **全局注册表**：单一注册表管理所有技能，支持 revision 追踪
2. **防抖追踪**：60 秒内相同技能多次调用只计一次
3. **7 天半衰期**：使用频率评分随时间自然衰减，不活跃技能自动降权
4. **首次注册优先**：同名技能只保留第一个注册的，防止覆盖
5. **依赖完整性**：加载前验证所有依赖存在且版本兼容
6. **路径触发**：`paths` 模式匹配，文件操作时自动推荐相关技能

## 五、使用方式

```bash
# 技能自动在启动时加载
# 用户技能目录: ~/.allthecodes/skills/
# 项目技能目录: .allthecodes/skills/

# 用户调用
# /debug "服务启动失败"

# 模型自动调用
# 当用户描述问题时，模型自动选择技能
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-skills/src/lib.rs` | 核心类型 + 注册表 + 依赖解析 |
| `crates/allthecodes-skills/src/bundled.rs` | 内置技能定义 |
| `crates/allthecodes-skills/src/loader.rs` | 从文件系统加载技能 |
| `crates/allthecodes-skills/src/invocation.rs` | 技能调用准备 |
| `crates/allthecodes-skills/src/usage.rs` | 使用追踪（时间衰减评分） |
