---
title: "Skills 技能系统 - SKILL.md 加载与执行架构"
description: "从源码角度解析 allthecodes Skills 系统：SKILL.md 文件解析、Frontmatter 字段、包依赖与版本解析、全局注册表管理、Bundled/User/Project/MCP 多来源加载和 Prompt 展开。"
keywords: ["Skills", "SKILL.md", "技能加载", "Frontmatter", "SkillDefinition", "依赖解析", "版本管理"]
---

## Skill vs Tool：本质差异

| | Tool | Skill |
|---|---|---|
| 粒度 | 单个原子操作（读文件、执行命令） | 一套完整的工作流（代码审查、创建 PR） |
| 触发方式 | AI 自主选择 | 用户 `/skill-name` 或 AI 通过 SkillTool 自动匹配 |
| 本质 | Rust struct + `impl Tool` | **Frontmatter + Prompt body** 的声明式封装 |
| 注册位置 | `registry.rs` → `get_all_tools()` | `skills::lib.rs` → 全局注册表 |
| 执行器 | 各 Tool 的 `call()` 方法 | `expand_prompt()` → 注入对话或 fork 子 Agent |

Skill 的核心洞见：**复杂任务的关键不在代码逻辑，而在 Prompt 质量**。

## Skill 的六个来源

### 1. Bundled Skills（编译时打包）

`allthecodes-skills/src/bundled.rs` 定义了 5 个内置技能：

| 名称 | 描述 | 执行模式 |
|------|------|---------|
| `simplify` | 简化和重构代码 | `Fork` |
| `remember` | 保存信息到 AGENTS.md | `Inline` |
| `debug` | 诊断和修复问题 | `Inline` |
| `stuck` | 在卡住时提供帮助 | `Inline` |
| `update-config` | 更新配置设置 | `Inline`（模型不可调用） |

**版本号**：Bundled skills 使用 `env!("CARGO_PKG_VERSION")` 作为版本，与 app 版本同步。

### 2. 用户 Skills（`~/.claude/skills/`）

### 3. 项目 Skills（`<project>/.claude/skills/`）

### 4. 插件 Skills（Plugin 贡献）

### 5. MCP Skills（通过 MCP 服务器提供）

### 6. Legacy Commands（向后兼容 `/commands/` 目录）

## SKILL.md 文件格式

### 目录结构

```
.claude/skills/
  └── my-skill/
      ├── SKILL.md         # 技能定义（必需）
      ├── assets/          # 打包的资源文件
      └── docs/            # 参考文档
```

### Frontmatter 字段

`allthecodes-skills/src/loader.rs` 的 `parse_skill_frontmatter()` 解析以下字段：

```yaml
---
name: my-skill                    # 显示名称（覆盖目录名）
description: 描述信息              # 技能描述
when-to-use: "用户要求审查代码时"    # AI 自动匹配依据
allowed-tools:                    # 工具白名单
  - Read
  - Grep
  - Bash
argument-hint: "<文件路径>"         # 参数提示
arguments: [path]                  # 命名参数名（用于 $ARGUMENTS 替换）
model: opus                        # 模型覆盖
context: inline                    # 执行模式：inline（默认）| fork
agent: code-reviewer               # 指定 Agent 定义文件
user-invocable: true               # 用户是否可 /调用
disable-model-invocation: false     # 禁止 AI 自主调用
version: "1.0.0"                   # 版本号
compatible-app-version: ">=0.1.0"   # 兼容的 app 版本
dependencies:                      # 依赖的其他 skills
  - base >=1.0.0
paths:                             # 条件激活文件路径
  - "src/**/*.ts"
assets:                            # 打包资源文件
  - assets/template.txt
entry-docs:                        # 参考文档
  - docs/guide.md
---
```

共 16 个 frontmatter 字段（通过 `KNOWN_FRONTMATTER_KEYS` 验证），未知字段会发出警告。

### Prompt Body

Frontmatter 后的 Markdown 正文是技能的 Prompt 内容，支持变量替换：

```
- $ARGUMENTS — 所有参数拼接
- ${NAME} — 命名参数（对应 arguments 字段）
- ${CLAUDE_SKILL_DIR} — 技能目录绝对路径
- ${CLAUDE_SESSION_ID} — 当前会话 ID
```

```rust
pub fn expand_prompt(&self, args: &str, session_id: Option<&str>) -> String {
    let mut body = self.prompt_body.clone();
    if let Some(dir) = &self.base_dir {
        body = body.replace("${CLAUDE_SKILL_DIR}", &dir_str);
    }
    if let Some(sid) = session_id {
        body = body.replace("${CLAUDE_SESSION_ID}", sid);
    }
    body = body.replace("$ARGUMENTS", args);
    // 命名参数替换
    for (i, name) in self.frontmatter.argument_names.iter().enumerate() {
        let val = arg_parts.get(i).copied().unwrap_or("");
        body = body.replace(&format!("${{{}}}", name), val);
    }
    body
}
```

## 全局注册表

`allthecodes-skills/src/lib.rs` 维护了全局技能注册表：

```rust
static REGISTRY: LazyLock<Mutex<Vec<SkillDefinition>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static REGISTRY_DIAGNOSTICS: LazyLock<Mutex<Vec<SkillDiagnostic>>> = ...;
static REGISTRY_REVISION: AtomicU64 = AtomicU64::new(0);
```

### 关键操作

| 函数 | 说明 |
|------|------|
| `get_all_skills()` | 获取所有注册技能 |
| `find_skill(name)` | 按名称查找 |
| `get_user_invocable_skills()` | 用户可调用的技能（`/name`） |
| `get_model_invocable_skills()` | 模型可调用的技能 |
| `register_skills_resolved()` | 带解析的批量注册 |
| `reload_skills_with_extra()` | 重新加载所有来源 |

### 初始化和重新加载

```rust
pub fn init_skills(user_skills_dir: &Path, project_dir: Option<&Path>) {
    reload_skills_with_extra(user_skills_dir, project_dir, Vec::new(), options)
}
```

加载流程：
1. `bundled::bundled_skills()` — 编译时打包的技能
2. `loader::load_skills_from_dir(user_dir, User)` — 用户目录
3. `loader::load_skills_from_dir(project_dir, Project)` — 项目目录
4. 额外技能（插件/MCP）
5. `resolve_skill_packages()` — 依赖解析和版本冲突检查
6. 替换全局注册表

## 包依赖与版本解析

`allthecodes-skills/src/lib.rs` 的 `resolve_skill_packages()` 执行完整的依赖解析。

### 版本格式

```rust
struct VersionParts(u64, u64, u64);  // (major, minor, patch)
```

支持语义化版本号（如 `1.2.3`），可选 `v` 前缀，可带 `-prerelease` 或 `+build` 后缀。

### 版本运算符

| 运算符 | 含义 | 示例 |
|--------|------|------|
| `=` | 精确匹配 | `=1.2.3` |
| `>` | 大于 | `>1.0.0` |
| `>=` | 大于等于 | `>=1.0.0` |
| `<` | 小于 | `<2.0.0` |
| `<=` | 小于等于 | `<=1.5.0` |
| `^` | 兼容（主版本一致） | `^1.0.0` → `>=1.0.0, <2.0.0` |
| `~` | 近似（次版本一致） | `~1.2.0` → `>=1.2.0, <1.3.0` |

### 验证检查

包验证（`validate_skill_package()`）包含以下检查：

1. **名称格式**：首字符必须为字母数字，后续支持 `-`、`_`、`:`、`.`
2. **描述非空**：description 字段不能为空
3. **版本格式**：version 字段必须是有效的语义版本
4. **App 版本兼容**：`compatible-app-version` 必须匹配当前 app 版本
5. **依赖有效性**：依赖项名称必须合法，版本要求格式正确
6. **资源文件存在**：assets 和 entry-docs 声明的路径必须存在
7. **路径安全**：所有相对路径不能使用 `../` 逃逸

### 依赖循环检测

使用三色标记 DFS 检测依赖循环：

```rust
fn visit_cycle(name, deps_by_name, state, stack, ...) {
    // 状态: 0=未访问, 1=访问中(灰色), 2=已完成(黑色)
    // 遇到灰色节点 → 循环检测
}
```

### 拓扑排序

解析后的技能列表按依赖关系拓扑排序，确保依赖在前、消费者在后。

## 执行路径：Inline vs Fork

### Inline 模式（默认）

Skill 的 Prompt 内容被注入为对话中的消息，在主对话流中继续执行：

```
expand_prompt() → 替换 $ARGUMENTS、${CLAUDE_SKILL_DIR} 等
  ↓
contextModifier() → 注入 allowedTools + model + effort
  ↓
注入到对话流
```

### Fork 模式

Skill 在独立子 Agent 中执行，上下文隔离：

```rust
pub enum SkillContext {
    Inline,  // 默认
    Fork,    // 隔离执行
}
```

当前 `simplify` 技能使用 `SkillContext::Fork`，因为它需要独立上下文和专用的 agent 配置。

## 使用频率追踪

`allthecodes-skills/src/usage.rs` 中的 `SkillUsageTracker` 追踪技能使用频率：

```rust
static SKILL_USAGE: LazyLock<Mutex<usage::SkillUsageTracker>> = ...;
```

- `record_skill_usage(name)` — 记录调用（带 60 秒去抖）
- `ranked_skill_usage()` — 按使用频率排序
- 通过 `save_skill_usage()` / `load_skill_usage()` 持久化到磁盘

## 诊断系统

技能加载过程中产生的诊断信息被收集为 `SkillDiagnostic`：

```rust
pub struct SkillDiagnostic {
    pub severity: SkillDiagnosticSeverity,  // Warning | Error
    pub code: String,                       // 诊断代码
    pub message: String,                    // 描述信息
    pub skill: Option<String>,              // 相关技能名称
    pub source: Option<SkillSource>,        // 来源
    pub path: Option<PathBuf>,              // 文件路径
}
```

常见诊断代码：

| 代码 | 严重级别 | 说明 |
|------|---------|------|
| `malformed-frontmatter` | Error | Frontmatter 格式错误 |
| `unknown-frontmatter-key` | Warning | 未知的 frontmatter 字段 |
| `missing-description` | Error | 缺少 description |
| `invalid-name` | Error | 技能名称不合法 |
| `version-conflict` | Error | 同名不同版本冲突 |
| `missing-dependency` | Error | 依赖项缺失 |
| `dependency-cycle` | Error | 依赖循环 |
| `incompatible-app-version` | Error | app 版本不兼容 |
