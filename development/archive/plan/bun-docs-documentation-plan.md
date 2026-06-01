# Bun Docs Implementation Documentation Plan

本文档是写作计划，不是实现核查报告。当前阶段只规划如何基于
`F:\AIclassmanager\cc\claude-code-bun\docs` 中的 `agent`、`context`、
`extensibility`、`safety`、`tools` 文档，整理 cc-rust 项目的相关实现，并
输出到项目文档中。

当前计划不要求搜索代码、不判断具体实现是否真实存在、不运行测试。后续真正
撰写实现映射文档时，才进入实现核查阶段。

## 1. 目标

在项目文档中形成一份有顺序、可维护的实现整理文档，用来回答：

- Bun 版文档描述了哪些能力。
- cc-rust 项目预计应有哪些对应实现。
- 哪些能力已经实现、部分实现、未实现、待确认或属于故意裁剪。
- 后续补齐和核查的优先级是什么。

建议最终输出主文档：

- `architecture/bun-docs-implementation-map.md`

如内容过长，可进一步拆分为：

- `architecture/agent-implementation-map.md`
- `architecture/context-implementation-map.md`
- `architecture/extensibility-implementation-map.md`
- `architecture/safety-implementation-map.md`
- `architecture/tools-implementation-map.md`

## 2. 输入范围

只以以下 Bun 文档目录为输入范围：

- `F:\AIclassmanager\cc\claude-code-bun\docs\agent`
- `F:\AIclassmanager\cc\claude-code-bun\docs\context`
- `F:\AIclassmanager\cc\claude-code-bun\docs\extensibility`
- `F:\AIclassmanager\cc\claude-code-bun\docs\safety`
- `F:\AIclassmanager\cc\claude-code-bun\docs\tools`

第一轮只读取这些文档的标题、主题和功能点。不要在第一轮做代码搜索。

## 3. 写作顺序

按下面顺序整理，顺序不要反过来：

1. `agent`
2. `context`
3. `extensibility`
4. `safety`
5. `tools`
6. 总结已实现能力
7. 总结未实现、部分实现、待确认能力
8. 整理后续核查和补齐优先级

## 4. 统一记录模板

每篇 Bun 文档在实现整理文档中使用同一个模板：

```md
### <上游文档名>

- 上游主题：
- 需要映射的能力：
- cc-rust 预计对应模块：
- 当前状态：待确认
- 已实现内容：
- 部分实现内容：
- 未实现内容：
- 故意裁剪内容：
- 后续核查入口：
- 备注：
```

状态字段在实现核查前统一写为 `待确认`，不要提前推断。

## 5. Agent 章节计划

覆盖 Bun 文档中的 agent 相关主题，包括：

- coordinator / swarm / team coordination
- sub-agents
- worktree isolation

写作重点：

- agent 调度模型
- 子代理生命周期
- 后台任务与并发模型
- agent 间通信方式
- worktree 隔离边界
- 与 Bun 版能力的差异

产出要求：

- 先列上游能力清单。
- 再列 cc-rust 预计需要映射的实现面。
- 最后列待核查项，不直接写结论。

## 6. Context 章节计划

覆盖 Bun 文档中的 context 相关主题，包括：

- compaction
- project memory
- system prompt
- token budget

写作重点：

- 上下文构造流程
- 自动压缩策略
- memory 注入策略
- system prompt 组成
- token 预算、截断和压缩触发条件
- 估算 token 与真实 API token 统计之间的差异

产出要求：

- 区分“上下文输入来源”和“上下文压缩机制”。
- 区分“会话内 memory”和“项目 / 全局 memory”。
- 对 token 预算相关结论保持待核查，避免凭文档推断实现。

## 7. Extensibility 章节计划

覆盖 Bun 文档中的 extensibility 相关主题，包括：

- custom agents
- hooks
- MCP configuration
- MCP protocol
- skills

写作重点：

- 扩展点类型
- 配置来源和优先级
- hook 事件矩阵
- MCP transport 支持情况
- MCP tool / resource 映射方式
- skill 加载、隔离、执行方式
- 自定义 agent 与 skill / MCP / hooks 的关系

产出要求：

- 对每类扩展点分别列“配置层”“运行时层”“安全层”。
- MCP 不只写配置，还要单独记录协议和 transport 支持状态。
- skills 与 custom agents 不要混写，除非实现中确实存在共享入口。

## 8. Safety 章节计划

覆盖 Bun 文档中的 safety 相关主题，包括：

- auto mode
- permission model
- plan mode
- sandbox
- why safety matters

写作重点：

- 权限决策顺序
- 允许、拒绝、询问、绕过的模式边界
- plan mode 对工具和文件写入的限制
- sandbox 策略与平台差异
- hooks 对权限决策的影响
- 已实现防护层和未完成风险点

产出要求：

- 先描述安全模型层次：prompt、permissions、hooks、sandbox、plan mode。
- 再按 Bun 文档逐项映射。
- Windows 平台 sandbox 能力必须单独标注，不能默认等同 Linux / macOS。

## 9. Tools 章节计划

覆盖 Bun 文档中的 tools 相关主题，包括：

- what are tools
- file operations
- search and navigation
- shell execution
- task management

写作重点：

- 工具注册模型
- 工具输入输出 schema
- 文件读写安全策略
- 搜索和导航工具能力
- shell 执行、超时、取消、危险命令检测
- task / todo 管理能力
- WebFetch / WebSearch 等网络工具差异

产出要求：

- 按工具族群整理，不按源码文件散列。
- 文件操作、shell 执行、搜索导航、任务管理分别成小节。
- 对高级能力，例如多模态文件、JS 渲染、远程任务轮询，先标为待核查。

## 10. 状态分类规则

实现整理文档统一使用以下状态：

| 状态 | 含义 |
| --- | --- |
| 已实现 | cc-rust 已有稳定实现，并有明确实现入口 |
| 部分实现 | 核心路径存在，但缺少 Bun 版某些行为 |
| 未实现 | 没有对应能力，或只有占位 |
| 待确认 | 尚未进行代码核查，不能凭文档判断 |
| 故意裁剪 | 项目明确决定不完全对齐 Bun 版 |

在没有代码证据前，默认使用 `待确认`。

## 11. 最终汇总表格式

实现整理文档末尾放总表：

```md
| 分类 | Bun 文档 | 上游能力 | cc-rust 状态 | 说明 | 后续动作 |
| --- | --- | --- | --- | --- | --- |
| Agent | sub-agents.mdx | 子代理执行 | 待确认 | 需要核查实现入口 | 后续代码核查 |
```

## 12. 后续执行边界

当前计划阶段：

- 不搜索代码。
- 不修改源码。
- 不运行测试。
- 不判断具体实现是否完成。
- 只保存写作计划。

后续实现核查阶段：

- 先读 Bun 文档。
- 再按章节核查 cc-rust 实现入口。
- 最后写入 `architecture/` 下的实现映射文档。
- 每个“已实现”或“部分实现”结论都需要有文件路径依据。
