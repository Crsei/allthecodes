# MCP Skills — 技能系统

> 实现状态：完整实现
> 来源类型：Bundled / User / Project / Plugin / MCP
> 执行模式：Inline（注入提示词） / Fork（子代理独立执行）

## 一、功能概述

Skills 系统让用户和模型可以定义、发现、调用可复用的能力模块。每个 Skill 是一个包含 YAML 前置元数据和 Markdown 提示词体的 `SKILL.md` 文件。系统支持从多个来源加载技能，执行版本解析、依赖检查和拓扑排序。

## 二、实现架构

### 2.1 Crate 结构

`allthecodes-skills` crate 包含以下模块：

| 模块 | 文件 | 职责 |
|------|------|------|
| `lib` | `lib.rs` | 核心类型定义、全局注册表、加载编排、版本解析 |
| `invocation` | `invocation.rs` | 技能调用准备（内联注入 / Fork 请求构建） |
| `loader` | `loader.rs` | 目录扫描、SKILL.md 解析、前端元数据提取 |
| `bundled` | `bundled.rs` | 内置技能定义 |
| `usage` | `usage.rs` | 技能使用频率跟踪与衰减评分 |

### 2.2 核心类型

| 类型 | 说明 |
|------|------|
| `SkillDefinition` | 完整技能定义：名称、来源、目录、元数据、提示词体 |
| `SkillFrontmatter` | 元数据：描述、工具白名单、参数、模型、版本、依赖 |
| `SkillSource` | 来源枚举：Bundled / User / Project / Plugin / MCP |
| `SkillContext` | 执行模式：Inline（注入） / Fork（分支） |
| `SkillDependency` | 包依赖：名称 + 版本要求 |
| `SkillDiagnostic` | 诊断信息：警告 / 错误，含代码和消息 |

### 2.3 数据流

```
技能加载流程（init_skills / reload_skills_with_extra）
       │
       ├── bundled::bundled_skills()     — 加载内置技能
       ├── loader::load_skills_from_dir() — 扫描用户技能目录
       ├── loader::load_skills_from_dir() — 扫描项目技能目录
       └── 合并额外技能
       │
       ▼
  resolve_skill_packages()
       │
       ├── validate_skill_package()       — 验证名称、版本、依赖格式
       ├── dependency_invalid_names()     — 检查缺失依赖和版本匹配
       ├── detect_dependency_cycles()     — 检测循环依赖
       └── topo_sort_skills()             — 拓扑排序
       │
       ▼
  更新全局注册表 + 发送 SkillsLoaded 事件
```

### 2.4 调用方式

**用户调用**：斜杠命令 `/skill-name`
**模型调用**：通过 SkillTool 工具自动发现并调用

两种执行模式：

```
Inline（内联）                    Fork（分支）
     │                                │
     ▼                                ▼
  扩展示例提示词                   创建子代理
     │                                │
     ▼                                ▼
  以 Meta User Message            独立执行环境（30轮）
  注入当前对话                    指定模型 + 工具白名单
     │                                │
     ▼                                ▼
  模型继续当前推理                子代理完成返回结果
```

`SkillForkRequest` 包含字段：

| 字段 | 说明 |
|------|------|
| `skill` | 技能名称 |
| `expanded_prompt` | 展开后的完整提示词 |
| `allowed_tools` | 工具白名单 |
| `model` | 使用的模型 |
| `fallback_model` | 备用模型 |
| `max_turns` | 最大轮数（默认 30） |

### 2.5 全局注册表

技能存储在全局静态 `REGISTRY` 中，提供以下查询接口：

| API | 说明 |
|-----|------|
| `get_all_skills()` | 获取所有已注册技能 |
| `find_skill(name)` | 按名称查找 |
| `get_user_invocable_skills()` | 用户可调用的技能列表 |
| `get_model_invocable_skills()` | 模型可调用的技能列表 |
| `register_skills_resolved()` | 带解析能力的批量注册 |

## 三、技能元数据规格

SKILL.md 支持的 frontmatter 字段：

| 字段 | 必填 | 说明 |
|------|------|------|
| `description` | 是 | 技能描述 |
| `name` | 否 | 显示名称覆盖 |
| `when_to_use` | 否 | 模型使用时机提示 |
| `allowed_tools` | 否 | 工具白名单 |
| `argument_hint` | 否 | 参数输入提示 |
| `argument_names` | 否 | 命名参数列表 |
| `model` | 否 | 模型覆盖 |
| `user_invocable` | 否 | 用户可调用（默认否） |
| `disable_model_invocation` | 否 | 禁用模型调用（默认否） |
| `version` | 否 | 语义化版本号 |
| `context` | 否 | 执行模式（inline/fork） |
| `dependencies` | 否 | 依赖的其他技能 |
| `compatible_app_version` | 否 | 兼容的 app 版本要求 |
| `paths` | 否 | 文件路径 glob，仅匹配时可见 |
| `assets` | 否 | 打包的文件资源 |
| `entry_docs` | 否 | 引用的文档文件 |

## 四、版本解析与依赖检查

支持的版本操作符：

| 操作符 | 示例 | 语义 |
|--------|------|------|
| `=` | `=1.2.3` | 精确匹配 |
| `>=` | `>=1.0.0` | 大于等于 |
| `<=` | `<=2.0.0` | 小于等于 |
| `>` | `>1.0.0` | 大于 |
| `<` | `<2.0.0` | 小于 |
| `^` | `^1.0.0` | 兼容版本（主版本相同） |
| `~` | `~1.2.0` | 近似版本（主次版本相同） |

验证失败产生 `SkillDiagnostic`，分为 Warning 和 Error 两级。

## 五、使用方式

```bash
# 技能目录位置
~/.allthecodes/skills/         # 用户技能
<project>/.allthecodes/skills/ # 项目技能

# 用户调用
/技能名称 [参数...]
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-skills/src/lib.rs` | 核心类型、注册表、版本解析 |
| `crates/allthecodes-skills/src/invocation.rs` | 调用准备与 Fork 请求 |
| `crates/allthecodes-skills/src/loader.rs` | SKILL.md 解析 |
| `crates/allthecodes-skills/src/bundled.rs` | 内置技能 |
| `crates/allthecodes-skills/src/usage.rs` | 使用频率追踪 |
