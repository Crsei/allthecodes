# MCP / Skill / Plugin 真实项目能力测试计划

日期：2026-05-17
适用仓库：`claude-code-rust`
目标阶段：Full Build，对齐上游 Claude Code 完整行为，不按 Lite 缩减。

## 1. 测试目标

构建一个真实但可控的端到端测试项目，用社区中常见、可复现的 MCP Server、Skill 和 Plugin 验证 `cc-rust` 是否具备以下能力：

- MCP：能安装、发现、连接、列举工具、调用工具、处理失败、隔离权限和持久化配置。
- Skill：能从项目、用户目录和插件中发现 `SKILL.md`，按描述触发，按需加载引用文件或脚本，并正确处理无关任务不触发。
- Plugin：能添加 marketplace、安装插件、启用/禁用插件、加载插件内的 commands / agents / skills / hooks / MCP 配置，并遵守 `.cc-rust` 路径隔离。
- 组合能力：在同一个真实任务中同时使用 MCP + Skill + Plugin，不互相污染配置、不重复注册工具、不破坏会话恢复。

## 2. 真实测试项目设定

项目名：`cc-rust-capability-lab`

项目形态：一个小型全栈 Issue Tracker，用 TypeScript + SQLite + Playwright 测试覆盖真实工程场景。

建议目录：

```text
/tmp/cc-rust-capability-lab/
├── package.json
├── src/
│   ├── server.ts
│   ├── db.ts
│   └── routes.ts
├── tests/
│   ├── api.spec.ts
│   └── ui.spec.ts
├── docs/
│   ├── product-brief.md
│   ├── prd-draft.md
│   └── report-template.md
├── .cc-rust/
│   ├── settings.json
│   └── skills/
│       └── product-brief-writer/
│           ├── SKILL.md
│           └── references/style-guide.md
└── AGENTS.md
```

核心业务：

- 创建、查询、关闭 issue。
- 提供 `/health`、`/issues`、`/issues/:id/close` API。
- 提供一个最小 HTML 页面用于 Playwright 浏览器测试。
- 包含一个故意不完整的功能请求，要求 Agent 阅读文档、修改代码、运行浏览器检查、生成总结文档。

为什么选择这个项目：

- 有真实代码、文档、测试和浏览器交互，不只是 hello world。
- 可以自然触发 filesystem / git / GitHub / Playwright / Context7 / document skill / code review plugin。
- 数据和副作用都能限制在 `/tmp/cc-rust-capability-lab` 与项目级 `.cc-rust/` 内。

## 3. 社区能力候选清单

### 3.1 MCP Server 候选

| 类别 | 候选 | 来源 | 用途 | 选择理由 |
|---|---|---|---|---|
| 文件系统 | `@modelcontextprotocol/server-filesystem` | `modelcontextprotocol/servers` | 读写测试项目文件 | 官方参考实现，覆盖资源访问、路径授权和工具调用 |
| Git | `mcp-server-git` / reference git server | `modelcontextprotocol/servers` | 查看 diff、commit 历史、状态 | 验证本地仓库型 MCP 和项目上下文 |
| 推理链 | `@modelcontextprotocol/server-sequential-thinking` | `modelcontextprotocol/servers` | 多步骤任务规划 | 验证无外部凭据的 stdio 工具调用和长参数传递 |
| 浏览器 | `@playwright/mcp` | Microsoft Playwright MCP | 操作页面、检查 UI | 社区常用，覆盖长生命周期进程和浏览器依赖 |
| 文档查询 | Context7 MCP | Upstash Context7 / Claude 插件市场 | 查询框架文档 | 社区常用开发场景，验证远程文档类工具 |
| GitHub | `github/github-mcp-server` 或 `@modelcontextprotocol/server-github` | GitHub / MCP 社区 | 查询 issue、PR、repo 元数据 | 测试 OAuth/token 环境变量和远程 API 错误处理 |

最低必测组合：filesystem、git、sequential-thinking、Playwright。

扩展组合：Context7、GitHub。扩展组合需要网络或 token，不能作为离线阻塞项。

### 3.2 Skill 候选

| 类别 | 候选 | 来源 | 用途 | 验收重点 |
|---|---|---|---|---|
| 项目自定义 Skill | `product-brief-writer` | 本测试项目自带 | 按模板生成产品简报 | 项目级 `.cc-rust/skills` 发现、描述触发、引用文件按需读取 |
| 官方文档 Skill | `document-skills` | `anthropics/anthropic-agent-skills` | 生成/解析 docx、xlsx、pptx、pdf 类任务 | 插件内 Skill 发现与触发 |
| 前端设计 Skill | `frontend-design` | `anthropics/claude-code-plugins` / 社区注册表 | 改进 issue 页面 | 插件内 Skill 与真实代码编辑结合 |
| 语言工作流 Skill | `javascript-typescript` | `wshobson/claude-code-workflows` | TypeScript 项目开发建议 | 第三方社区 Skill 的安装、触发和安全审查 |
| Code review Skill | `code-review` 或 `pr-review-toolkit` | `anthropics/claude-code-plugins` | 审查变更 | 多 agent / command / skill 组合能力 |

最低必测组合：项目自定义 Skill + 一个插件内 Skill。

扩展组合：文档处理 Skill、TypeScript 工作流 Skill。

### 3.3 Plugin 候选

| 类别 | 候选 | 来源 | 用途 | 验收重点 |
|---|---|---|---|---|
| 官方插件市场 | `anthropics/claude-plugins-official` | Anthropic-managed marketplace | 安装官方与社区管理插件 | marketplace 添加、安装、缓存、启禁用 |
| Context7 Plugin | `context7@claude-plugins-official` | Claude 插件市场 | 自动注入 Context7 MCP | 插件携带 MCP 配置并成功注册 |
| TypeScript LSP Plugin | `typescript-lsp@claude-plugins-official` | Claude 插件市场 | TS/JS 代码智能 | 插件长期后台服务、LSP 生命周期 |
| Frontend Design Plugin | `@anthropics/claude-code-plugins/frontend-design` | 社区注册表 | UI 开发任务 | 插件内 Skill/Agent 触发 |
| Code Review Plugin | `@anthropics/claude-code-plugins/code-review` | 社区注册表 | 代码审查 | commands / agents 编排 |
| Community Workflow Plugin | `@wshobson/claude-code-workflows/javascript-typescript` | 社区注册表 | TS 工作流 | 第三方插件审查、隔离安装、禁用恢复 |

最低必测组合：官方 marketplace + Context7 plugin + TypeScript LSP plugin。

扩展组合：frontend-design、code-review、wshobson workflows。

## 4. 环境隔离规则

必须遵守本仓库路径隔离：

- 全局目录使用 `~/.cc-rust/`，禁止写入 `~/.Codex/` 或 `~/.claude/`。
- 项目配置使用 `/tmp/cc-rust-capability-lab/.cc-rust/settings.json`。
- 项目 Skill 使用 `/tmp/cc-rust-capability-lab/.cc-rust/skills/`。
- Keychain 服务名使用 `cc-rust`。
- 第三方插件缓存、marketplace cache、MCP 配置如尚未实现 `.cc-rust` 映射，应作为测试失败记录，而不是临时改用上游路径。

测试前准备：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
export CCRUST_HOME=/tmp/cc-rust-capability-lab/home/.cc-rust
export HOME=/tmp/cc-rust-capability-lab/home
```

说明：`HOME` 指向测试沙箱后，仍要确认实现内部没有硬编码上游 `~/.claude`、`~/.Codex` 或 `~/.codex` 持久化路径。

## 5. 分阶段测试计划

### Phase 0：基线项目与夹具

目标：构建可重复的真实项目夹具。

任务：

1. 创建 `/tmp/cc-rust-capability-lab`。
2. 初始化 TypeScript + SQLite + Playwright 项目。
3. 添加 3 个已知 issue fixture。
4. 添加一个失败测试：关闭 issue 后列表页仍显示旧状态。
5. 添加项目级 Skill：`product-brief-writer`。
6. 添加项目 `AGENTS.md`，要求所有输出写入项目 `docs/`。

验收：

- Agent 能读取项目文档并理解任务。
- 未安装任何 MCP / Plugin 时，基础 headless 对话不崩溃。
- 所有生成文件都留在测试项目目录或测试 HOME 下。

### Phase 1：MCP 单项能力

目标：逐个验证 MCP stdio 连接、工具列表、调用、错误恢复。

测试用例：

| ID | MCP | Prompt | 预期 |
|---|---|---|---|
| MCP-01 | filesystem | “读取 `docs/product-brief.md` 并列出需求。” | 只访问允许目录，返回文档摘要 |
| MCP-02 | filesystem | “尝试读取 `/etc/passwd`。” | 被拒绝或明确权限错误，不泄漏内容 |
| MCP-03 | git | “总结当前工作区 diff。” | 能通过 MCP 获得变更摘要 |
| MCP-04 | sequential-thinking | “为修复 failing UI test 制定 5 步计划。” | 工具调用成功，结果进入模型上下文 |
| MCP-05 | Playwright | “打开本地页面，确认 issue 状态显示。” | 能启动浏览器并返回可观察结果 |
| MCP-06 | Context7 | “查询当前 Playwright locator 推荐写法。” | 网络可用时返回文档；网络失败时错误可读 |
| MCP-07 | GitHub | “读取测试仓库 issue 标题。” | token 缺失时提示凭据问题；有 token 时成功 |

故障注入：

- MCP command 不存在。
- MCP server 启动后立即退出。
- MCP 返回 malformed JSON-RPC。
- MCP 工具耗时超过超时阈值。
- 同名工具来自两个 server。

验收：

- UI/headless 都能显示 MCP 工具错误。
- 失败不会导致主进程退出。
- 禁用 MCP 后工具不再出现在可用工具列表。
- MCP 配置变更后会话恢复行为明确且可记录。

### Phase 2：Skill 单项能力

目标：验证 Skill 发现、选择、加载、脚本执行和不触发边界。

测试用例：

| ID | Skill | Prompt | 预期 |
|---|---|---|---|
| SKILL-01 | project `product-brief-writer` | “根据 `docs/product-brief.md` 生成一页产品简报。” | 自动触发项目 Skill，并读取 style guide |
| SKILL-02 | project skill negative | “解释 `src/routes.ts` 的 close issue 逻辑。” | 不触发 product brief Skill |
| SKILL-03 | plugin document skill | “把产品简报整理为 docx 结构大纲。” | 触发文档类 Skill 或给出缺依赖说明 |
| SKILL-04 | frontend-design | “改善 issue 列表页面视觉层次。” | 触发前端 Skill，输出具体代码修改建议或修改 |
| SKILL-05 | javascript-typescript | “修复 TypeScript API bug，并说明类型影响。” | 触发 TS 工作流 Skill |

安全检查：

- Skill 中脚本必须显示命令和权限请求。
- Skill 引用文件只在触发后加载。
- 第三方 Skill 安装前记录来源、版本、commit 或 tag。
- 第三方 Skill 内容中如含网络、shell、token 访问，必须标注人工审查。

验收：

- `SKILL.md` metadata 能被解析。
- 同名 Skill 冲突时优先级清晰：项目级 > 插件级 > 用户级，或按实现文档记录。
- Skill 失败信息能回传给用户，不被吞掉。
- 禁用插件后，插件内 Skill 不再可用。

### Phase 3：Plugin 单项能力

目标：验证 marketplace、安装、缓存、启禁用、插件组件加载。

测试用例：

| ID | Plugin | 操作 | 预期 |
|---|---|---|---|
| PLUGIN-01 | marketplace | 添加 `anthropics/claude-plugins-official` | 写入 `.cc-rust` 配置，不写入上游路径 |
| PLUGIN-02 | Context7 | 安装并启用 `context7` | MCP server 注册成功，工具可见 |
| PLUGIN-03 | TypeScript LSP | 安装并启用 `typescript-lsp` | LSP 生命周期正常，失败可读 |
| PLUGIN-04 | frontend-design | 安装并触发 UI 任务 | 插件内 Skill/Agent 可被发现 |
| PLUGIN-05 | code-review | 对本项目 diff 执行 review | slash command / agent 编排可执行 |
| PLUGIN-06 | disable | 禁用 Context7 plugin | 相关 MCP 工具和 Skill 从列表消失 |
| PLUGIN-07 | uninstall | 卸载第三方 workflow plugin | 缓存、配置和启用状态一致 |

故障注入：

- marketplace URL 不存在。
- plugin manifest 缺字段。
- plugin source 指向不存在 commit。
- plugin 试图写入 `~/.claude`。
- plugin hooks 中包含需要拒绝的 shell 操作。

验收：

- 插件 manifest 解析错误定位到具体字段。
- 插件版本、source、启用状态可查询。
- 插件带来的 commands / agents / skills / hooks / MCP 配置按 scope 生效。
- 任何插件持久化路径都符合 `.cc-rust` 隔离要求。

### Phase 4：组合真实任务

目标：模拟真实用户用 cc-rust 完成一个功能变更。

主任务 Prompt：

```text
在这个 Issue Tracker 项目中修复“关闭 issue 后列表页仍显示旧状态”的问题。
要求：
1. 先阅读产品简报并生成修复计划。
2. 使用可用 MCP 工具检查文件、git diff 和本地页面状态。
3. 修改代码后，用 Playwright MCP 检查页面行为。
4. 用 code-review 插件审查你的变更。
5. 用 product-brief-writer Skill 更新 docs/fix-summary.md。
6. 输出所有使用过的 MCP、Skill、Plugin 名称和结果。
```

预期链路：

- Skill：`product-brief-writer` 被触发用于文档输出。
- MCP：filesystem / git / Playwright 至少各调用一次。
- Plugin：Context7 或 TypeScript LSP 可参与开发建议，code-review 参与审查。
- 权限：写入只发生在测试项目内。
- 会话：中途重启 headless 后能恢复上下文或给出明确恢复限制。

验收：

- 功能 bug 被修复。
- UI 检查有可追溯结果。
- `docs/fix-summary.md` 存在且符合 Skill 模板。
- 最终报告列出能力调用轨迹。
- 失败路径不会伪造成功。

### Phase 5：回归自动化

目标：将手工计划沉淀为 e2e test。

建议新增测试层级：

- `tests/e2e/mcp_capability.rs`：MCP discovery / call / failure。
- `tests/e2e/skill_capability.rs`：项目 Skill、插件 Skill、负触发。
- `tests/e2e/plugin_capability.rs`：marketplace、install、enable、disable。
- `tests/e2e/capability_lab.rs`：组合真实任务。

最小自动化策略：

- 第三方网络依赖默认 mock 或 fixture 化。
- MCP reference servers 可以通过 `npx` / `uvx` 做可选真实测试。
- CI 默认跑离线 mock；本机 nightly 跑真实社区包。
- 对真实社区包固定版本或 commit，避免测试随上游漂移。

## 6. 成功标准

必须满足：

- `cc-rust` 不向 `~/.claude`、`~/.Codex` 写入任何持久化文件。
- MCP server 可添加、可列举、可调用、可禁用，失败可恢复。
- Project Skill 能自动触发，插件 Skill 能随插件启禁用变化。
- Plugin marketplace 能添加，插件能安装、启用、禁用、卸载。
- MCP + Skill + Plugin 在同一真实任务中可组合使用。
- 所有错误都能在 headless JSONL 或 TUI 中被用户理解。

建议满足：

- 支持 pin plugin source 到 tag 或 sha。
- 支持列出插件贡献的所有组件。
- 支持 MCP 工具命名冲突提示。
- 支持第三方 Skill / Plugin 安装前安全摘要。
- 支持测试 HOME 一键清理。

## 7. 主要风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| 社区包上游变化 | 测试不稳定 | 固定版本、记录 commit、离线 mock |
| 第三方插件执行 shell | 安全风险 | 测试 HOME、临时目录、人工审查、权限门禁 |
| GitHub / Context7 需要网络 | CI 不稳定 | 标记为 extended，不作为基础阻塞 |
| Playwright 浏览器依赖缺失 | 本机失败 | 预检依赖，失败时输出明确缺失项 |
| 路径隔离不完整 | 污染原版 Claude/Codex 配置 | 所有测试前后扫描 `HOME` 下写入路径 |
| 上游 Claude 路径与 cc-rust 路径不同 | 兼容实现容易误写 | 测试明确断言 `.cc-rust` 路径 |

## 8. 推荐执行顺序

1. 先实现 Phase 0 夹具项目。
2. 再跑 filesystem、sequential-thinking、git 三个无 token MCP。
3. 然后实现项目 Skill 的 discovery 和 trigger e2e。
4. 再接官方 marketplace 与 Context7 plugin。
5. 最后接 Playwright、TypeScript LSP、code-review 等组合场景。

不建议一开始就接所有社区插件。MCP / Skill / Plugin 的失败面不同，应先分别收敛，再做组合任务。

## 9. 调研来源

- MCP 官方参考服务器：<https://github.com/modelcontextprotocol/servers>
- Playwright MCP：<https://www.npmjs.com/package/@playwright/mcp>
- Claude Code 插件发现文档：<https://code.claude.com/docs/en/discover-plugins>
- Claude Code 插件 marketplace 文档：<https://code.claude.com/docs/en/plugin-marketplaces>
- Anthropic 官方插件市场：<https://github.com/anthropics/claude-plugins-official>
- Claude Code Skills 文档：<https://code.claude.com/docs/en/skills>
- Claude Skills 总览：<https://claude.com/docs/skills/overview>
- ClaudSkills 社区注册表：<https://claudskills.com/>
- Claude plugins 社区注册表：<https://claude-plugins.dev/>

## 10. 后续落地拆分

建议拆成以下 commit：

1. `Add capability lab fixture plan`
2. `Add MCP capability e2e fixtures`
3. `Add skill discovery e2e fixtures`
4. `Add plugin marketplace e2e fixtures`
5. `Add combined MCP skill plugin capability test`
6. `Document verified full-build capability gaps`

---

## 11. 落地状态（2026-05-21）

已落地第一轮回归自动化：

- 新增 `crates/claude-code-rs/tests/capability_lab_support/`，运行时生成隔离的 `cc-rust-capability-lab` 夹具项目、项目级 Skill、项目级 `.cc-rust/settings.json`、本地插件 fixture，并扫描禁止写入的上游路径。
- 新增 `e2e_mcp_capability.rs`，默认覆盖 MCP discovery、disabled server、missing command recovery；ignored/live 覆盖 pinned community packages：
  - `@modelcontextprotocol/server-filesystem@2026.1.14`
  - `@modelcontextprotocol/server-sequential-thinking@2025.12.18`
  - `@playwright/mcp@0.0.75`
  - `@upstash/context7-mcp@2.3.0`
  - `@modelcontextprotocol/server-github@2025.4.8`（deprecated，仅作兼容错误/凭据路径 live 覆盖）
  - `uvx --from mcp-server-git mcp-server-git`
- 新增 `e2e_skill_capability.rs`，覆盖项目级 Skill discovery、reference 文件不预加载、插件 Skill 随贡献启用/移除。
- 新增 `e2e_plugin_capability.rs`，覆盖本地 marketplace/plugin fixture、install、enable/disable、MCP/Skill contribution discovery、uninstall、missing source/bad manifest 错误。
- 新增 `e2e_capability_lab.rs`，覆盖 MCP + Skill + Plugin 同项目组合 smoke；真实社区 MCP 连接链路作为 ignored/live tests。

默认验证命令：

```bash
cargo test -p claude-code-rs --test e2e_mcp_capability --test e2e_skill_capability --test e2e_plugin_capability --test e2e_capability_lab
```

已知保留：

- 真实社区包连接、Playwright 浏览器、Context7/GitHub 网络/token 路径不进入默认 gate，需显式 `-- --ignored` 或后续 live job 跑。
- 当前组合任务仍是工具/注册/发现层 smoke，不是带真实模型凭据的端到端代码修改链路。
