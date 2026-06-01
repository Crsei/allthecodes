# cc-rust Slash Command Reference

> 本文按当前源码实现整理：
> - 命令注册表：`crates/cc-commands/src/lib.rs`
> - 每个命令的参数解析：`crates/cc-commands/src/*.rs` 与 `crates/cc-commands/src/mcp/*.rs`
>
> 与旧文档不同，这里优先描述“当前代码实际支持什么”，而不是历史设计目标。
>
> 更新日期: 2026-05-21

## 约定

- 命令格式：`/command [args...]`
- 别名和主命令等价，例如 `/help`、`/h`、`/?`
- 若某命令没有写明子参数，表示当前实现只支持“无参数”或“把整段参数当自由文本”
- KAIROS / proactive / notification 相关命令受 feature gate 控制；未启用时会直接返回提示文本
- Rust TUI 中，部分命令在“无参数”时会先打开 `CommandSurface`；带参数时才直接进入普通命令处理器。本文同时记录命令处理器语义和无参数 TUI surface 行为。

## Core Commands

### `/help`

- Aliases: `/h`, `/?`
- Syntax:
  - `/help`
  - `/help <command-or-alias>`
- Behavior:
  - 无参数时列出所有已注册命令
  - 传命令名或别名时，显示该命令的一行说明和别名
- Examples:
  - `/help`
  - `/help model`
  - `/help q`

### `/clear`

- Aliases: none
- Syntax: `/clear`
- Behavior:
  - 返回 `CommandResult::Clear`，由外层 REPL 清空当前会话消息
- Examples:
  - `/clear`

### `/exit`

- Aliases: `/quit`, `/q`
- Syntax: `/exit`
- Behavior:
  - 退出 REPL
- Examples:
  - `/exit`
  - `/q`

### `/version`

- Aliases: `/v`
- Syntax: `/version`
- Behavior:
  - 输出当前 `claude-code-rs` 版本
- Examples:
  - `/version`
  - `/v`

### `/status`

- Aliases: none
- Syntax: `/status`
- Behavior:
  - 输出当前会话状态摘要：消息数、模型、fast mode、effort、permission mode
- Examples:
  - `/status`

## Model / Config / Permissions

### `/config`

- Aliases: `/settings`
- Syntax:
  - `/config`
  - `/config show`
  - `/config set <key> <value>`
  - `/config reset`
- Supported keys for `set`:
  - `model`
  - `backend`
  - `theme`
  - `verbose`
  - `model_reasoning_effort`
- Notes:
  - `/config` 默认等价于 `/config show`
  - `backend` 会走规范化逻辑：`codex` 保留为 `codex`，其他值会回退成 `native`
- Examples:
  - `/config`
  - `/config set model MOTA`
  - `/config set backend codex`
  - `/config set model_reasoning_effort high`
  - `/config set verbose true`
  - `/config reset`

### `/model`

- Aliases: none
- Syntax:
  - `/model`
  - `/model <model-id|SOTA|MOTA|FOTA>`
- Supported aliases:
  - `SOTA` → `gpt-5.5`
  - `MOTA` → `gpt-5.5`
  - `FOTA` → `gpt-5.5`
- Notes:
  - 旧的 `opus` / `sonnet` / `haiku` 家族别名已移除；请使用 `SOTA` / `MOTA` / `FOTA` 或完整模型 ID
  - 空的 `availableModels` 表示不限制；非空时 `/model`、Web 设置和 `/config set model` 都会拒绝列表之外的模型
  - 未知名字会被当成自定义模型 ID；但已移除的旧别名不会透传给 provider
- Examples:
  - `/model`
  - `/model MOTA`
  - `/model gpt-5.5`

### `/model-add`

- Aliases: `/ma`
- Syntax:
  - `/model-add <name>`
  - `/model-add <name> <input_price> <output_price>`
- Behavior:
  - 把 `CLAUDE_MODEL`、`MODEL_INPUT_PRICE`、`MODEL_OUTPUT_PRICE` 写入当前工作目录的 `.env`
  - 同时更新当前进程环境变量与当前 `app_state.main_loop_model`
- Notes:
  - 价格单位是 USD / 1M tokens
  - 只给模型名时，会尝试查内置 pricing 表；查不到会报错并要求显式传价格
- Examples:
  - `/model-add gpt-4o`
  - `/model-add my-model 1.5 7.0`

### `/cost`

- Aliases: `/usage`
- Syntax: `/cost`
- Behavior:
  - 聚合当前会话 assistant 消息里的 usage / cost 字段
- Examples:
  - `/cost`
  - `/usage`

### `/extra-usage`

- Aliases: `/eu`
- Syntax: `/extra-usage`
- Behavior:
  - 输出扩展用量分析：单次调用拆分、最贵调用 Top 5、缓存命中率、估算成本拆分
- Notes:
  - 当前实现不解析任何子参数
- Examples:
  - `/extra-usage`
  - `/eu`

### `/rate-limit-options`

- Aliases: `/rlo`, `/rate-limit`
- Syntax: `/rate-limit-options`
- Behavior:
  - 显示当前模型命中的速率限制参考表、当前会话里出现的 rate-limit 错误次数和建议
- Notes:
  - 当前实现不解析任何子参数
- Examples:
  - `/rate-limit-options`
  - `/rate-limit`

### `/effort`

- Aliases: none
- Syntax:
  - `/effort`
  - `/effort <low|medium|high>`
- Behavior:
  - 无参数时显示当前 effort
  - 有参数时只接受 `low` / `medium` / `high`
- Notes:
  - 顶部注释提到“numeric budget token count”，但当前代码并不支持数字参数
- Examples:
  - `/effort`
  - `/effort low`
  - `/effort high`

### `/fast`

- Aliases: none
- Syntax:
  - `/fast`
  - `/fast on`
  - `/fast off`
  - `/fast status`
- Supported synonyms:
  - `on` / `enable`
  - `off` / `disable`
- Behavior:
  - 无参数时切换 fast mode
  - 启用时如果当前模型不兼容，会自动切到配置的 `fastModel`，默认 `MOTA`（可由 `motaModel` 覆盖）
- Examples:
  - `/fast`
  - `/fast on`
  - `/fast off`
  - `/fast status`

### `/permissions`

- Aliases: `/perms`
- Syntax:
  - `/permissions`
  - `/permissions mode <mode>`
  - `/permissions allow <rule> [--user|--project|--local|--session]`
  - `/permissions ask <rule> [--user|--project|--local|--session]`
  - `/permissions deny <rule> [--user|--project|--local|--session]`
  - `/permissions session-grant <tool>`
  - `/permissions clear-session-grants`
  - `/permissions reset`
- Supported modes:
  - `default`
  - `auto`
  - `bypass`
  - `plan`
  - `acceptEdits`
  - `dontAsk`
- Mode aliases:
  - `ask` → `default`
  - `full access` / `full-access` → `bypass`
  - `readonly` → `plan`
- Notes:
  - `/permissions full access` is accepted as a TUI-friendly shorthand for `/permissions mode bypass --confirm`
  - `bypass` 还会检查 `is_bypass_permissions_mode_available`
  - 无参数在 Rust TUI 中打开 `PermissionsSurface`，可浏览 mode、workspace/permission rules，并填充常用 `/permissions ...` 命令
  - `allow` / `ask` / `deny` 默认写 user scope；显式 `--session` 只改当前会话
- Examples:
  - `/permissions`
  - `/permissions mode auto`
  - `/permissions full access`
  - `/permissions mode readonly`
  - `/permissions allow Bash`
  - `/permissions ask Edit --project`
  - `/permissions deny Write`
  - `/permissions session-grant Bash`
  - `/permissions clear-session-grants`
  - `/permissions reset`

### `/sandbox`

- Aliases: none
- Syntax:
  - `/sandbox`
  - `/sandbox status`
  - `/sandbox on`
  - `/sandbox off`
  - `/sandbox mode <read-only|workspace|full>`
  - `/sandbox require`
  - `/sandbox optional`
  - `/sandbox no-network`
  - `/sandbox network <on|off>`
- Behavior:
  - 显示或切换当前进程内 sandbox/network policy
  - `mode full` 会关闭 OS-level sandbox；`require` 会在 OS-level primitive 不可用时 fail closed
  - 无参数在 Rust TUI 中打开 `SandboxSurface`，用 tabbed selector 填充 toggle/mode/network 命令
- Notes:
  - 命令修改当前 runtime settings snapshot；持久化配置仍应通过 `/config set sandbox.*` 或 settings 文件完成
  - Windows OS-level sandbox primitive 是 intentional crop，`require` 在不可用平台保持 fail-closed 诊断
- Examples:
  - `/sandbox`
  - `/sandbox on`
  - `/sandbox mode workspace`
  - `/sandbox require`
  - `/sandbox network off`

## Session / Context / Workspace

### `/session`

- Aliases: none
- Syntax:
  - `/session`
  - `/session list`
  - `/session list all`
- Supported aliases:
  - `list` / `ls`
- Behavior:
  - 无参数时显示当前 session 信息和最近的工作区 session
  - `list` 只列当前 workspace
  - `list all` 列所有 workspace
- Examples:
  - `/session`
  - `/session list`
  - `/session ls all`

### `/resume`

- Aliases: none
- Syntax:
  - `/resume`
  - `/resume <session-id-or-prefix>`
- Behavior:
  - 无参数时恢复当前工作区最近的一次历史会话
  - 传 ID 时先尝试当前 workspace，再尝试全局；支持前缀匹配
- Notes:
  - 如果前缀命中多个 session，会返回候选列表让你继续缩小范围
- Examples:
  - `/resume`
  - `/resume 550e8400`

### `/context`

- Aliases: `/ctx`
- Syntax: `/context`
- Behavior:
  - 输出本地估算的上下文 token 使用情况和消息分布
- Notes:
  - 这里是本地启发式估算，不是精确 token 数
- Examples:
  - `/context`
  - `/ctx`

### `/compact`

- Aliases: none
- Syntax:
  - `/compact`
  - `/compact <free-text-instructions>`
- Behavior:
  - 尝试对当前对话做本地压缩
  - 若给了参数，会把整段参数当“压缩指令”文本
- Notes:
  - 这不是子命令接口，参数是自由文本
  - 当前主要走本地 compaction pipeline，不是远程总结
- Examples:
  - `/compact`
  - `/compact focus on code changes only`

### `/files`

- Aliases: none
- Syntax: `/files`
- Behavior:
  - 从消息和工具调用里提取当前上下文中引用过的文件路径
- Examples:
  - `/files`

### `/copy`

- Aliases: `/cp`
- Syntax: `/copy`
- Behavior:
  - 提取最后一条 assistant 纯文本消息并返回“Copied to clipboard …”提示
- Notes:
  - 当前实现并没有真正写系统剪贴板，而是把要复制的文本回显出来
- Examples:
  - `/copy`
  - `/cp`

### `/add-dir`

- Aliases: none
- Syntax:
  - `/add-dir`
  - `/add-dir <path>`
- Behavior:
  - 无参数时列出当前 session 已添加的额外工作目录
  - 有参数时解析路径、做 canonicalize，然后加入 `additional_working_directories`
- Notes:
  - 支持相对路径和 `~`
  - 如果目标目录已经在当前工作目录之内，或已经添加过，会直接提示
  - 当前新增目录默认不是只读
- Examples:
  - `/add-dir`
  - `/add-dir ..\\shared-lib`
  - `/add-dir ~/projects/other-repo`

### `/init`

- Aliases: none
- Syntax: `/init`
- Behavior:
  - 在当前工作目录下创建 `.cc-rust/settings.json`
- Notes:
  - 如果已存在，只会提示现有路径，不会覆盖
- Examples:
  - `/init`

### `/memory`

- Aliases: `/mem`
- Syntax:
  - `/memory`
  - `/memory show`
  - `/memory path`
  - `/memory edit`
  - `/memory list`
  - `/memory get <key>`
  - `/memory set <key> <value> [--global] [--category=<cat>]`
  - `/memory rm <key> [--global]`
  - `/memory search <query>`
- Supported aliases:
  - `list` / `ls`
  - `rm` / `delete` / `del`
  - `search` / `find`
- Behavior:
  - `show`：显示当前 `CLAUDE.md` 汇总内容，且是默认行为
  - `path`：列出找到的 `CLAUDE.md`
  - `edit`：在当前目录创建或定位 `CLAUDE.md`
  - `list/get/set/rm/search`：操作 memdir 记忆项
- Notes:
  - `set` 支持 `--global` 和 `--category=<cat>`
  - `rm` 支持 `--global`
  - 当前 `search` 查询在实现上更适合单 token；多词查询不会完整保留
- Examples:
  - `/memory`
  - `/memory path`
  - `/memory edit`
  - `/memory list`
  - `/memory get style`
  - `/memory set style use_rustfmt --category=code`
  - `/memory set api_base https://example.com --global`
  - `/memory rm api_base --global`
  - `/memory search rustfmt`

### `/skills`

- Aliases: none
- Syntax:
  - `/skills`
  - `/skills list`
  - `/skills <skill-name>`
- Behavior:
  - 无参数或 `list` 时列出所有已加载技能
  - 传技能名时显示该技能的详细信息
- Examples:
  - `/skills`
  - `/skills list`
  - `/skills review`

### `/agents`

- Aliases: none
- Syntax:
  - `/agents`
  - `/agents list`
  - `/agents show <name>`
  - `/agents info <name>`
  - `/agents sources`
- Behavior:
  - 普通命令路径按 source 分组列出 built-in、bundled/user/project/plugin/MCP skill-backed agents 和 active team members
  - `show` / `info` 展示同名 agent 的详细定义、来源、shadowing/active 状态
  - `sources` 显示 agent 加载路径
  - 无参数在 Rust TUI 中打开 `AgentsSurface`，支持 source tab、list/detail、User/Project agent 创建与可编辑 agent 的 tools/model/color 编辑
- Notes:
  - `/agent` 不是别名；保留 `/agents` 作为唯一入口，避免 help/palette 路由歧义
  - TUI 创建/编辑持久化通过 `AgentSettingsCommand::Upsert` 写入 user/project agent definitions；built-in/plugin/MCP/team source 不作为可写目标
- Examples:
  - `/agents`
  - `/agents show reviewer`
  - `/agents sources`

## Auth Commands

### `/login`

- Aliases: none
- Syntax:
  - `/login`
  - `/login status`
  - `/login claude-code`
  - `/login claude-ai`
  - `/login console`
  - `/login codex`
  - `/login codex-oauth`
  - `/login codex-cli`
  - `/login custom`
  - `/login openai-api`
  - `/login openai-api sk-...`
  - `/login sk-ant-...`
  - `/login sk-...`
  - `/login bedrock`
  - `/login vertex`
  - `/login cloud`
- Top-level entries:
  - `claude-code`：select and persist the Claude Code / Anthropic-compatible auth profile
  - `claude-ai`：Claude.ai OAuth
  - `console`：Console OAuth
  - `codex`：select and persist the OpenAI Codex auth profile
  - `codex-oauth`：OpenAI Codex OAuth
  - `codex-cli`：check/import/refresh `~/.codex/auth.json`
  - `custom`：select an existing `authProfiles.custom` entry
  - `openai-api`：OpenAI Platform API Key，写入 cc-rust 的 OpenAI keychain account
- Compatibility: `claude_code`、`anthropic`、`anthropic_method`、`anthropic-method` 仍等价于 `claude-code`；数字 `/login 1/2/3/...` 不再作为入口。
- Examples:
  - `/login`
  - `/login status`
  - `/login claude-code`
  - `/login codex`
  - `/login custom`
  - `/login sk-ant-api03-...`
  - `/login openai-api sk-proj-...`
  - `/login codex-oauth`
  - `/login codex-cli`
  - `/login bedrock`
  - `/login vertex`

### `/login-code`

- Aliases: none
- Syntax:
  - `/login-code <authorization-code>`
  - `/login-code <redirect-url-containing-code>`
- Behavior:
  - 完成由 `/login claude-ai`、`/login console`、`/login codex-oauth` 发起的 OAuth 流程
- Notes:
  - 如果没有 pending OAuth state，会直接提示先跑 `/login claude-ai`、`/login console` 或 `/login codex-oauth`
  - 如果传的是整条回调 URL，会自动尝试提取 `code=` 查询参数
- Examples:
  - `/login-code eyJhbGciOi...`
  - `/login-code https://example/callback?code=abc123&state=xyz`

### `/logout`

- Aliases: none
- Syntax: `/logout`
- Behavior:
  - 清理 keychain 与 `~/.cc-rust/credentials.json`
- Notes:
  - 环境变量里的 token / API key 不会自动 unset
- Examples:
  - `/logout`

## Git / Export Commands

### `/diff`

- Aliases: none
- Syntax:
  - `/diff`
  - `/diff --staged`
  - `/diff --cached`
- Behavior:
  - 默认同时显示 staged 和 unstaged diff
  - `--staged` / `--cached` 只显示 staged diff
- Examples:
  - `/diff`
  - `/diff --staged`

### `/branch`

- Aliases: `/br`
- Syntax:
  - `/branch`
  - `/branch <branch-name>`
- Behavior:
  - fork 当前会话 transcript 到一个新 session，并立即把 active session 切到 fork 后的 session
  - 无参数时生成默认 fork 名称
  - 有参数时使用给定名称作为 fork title/label
  - 命令结果通过 `SwitchSession` 返回新 `session_id` 和切换后的可见 transcript；TUI、headless、daemon 和 Web command path 都应同步 session pointer
- Examples:
  - `/branch`
  - `/br investigate-parser`

### `/commit`

- Aliases: none
- Syntax:
  - `/commit`
  - `/commit <message>`
- Behavior:
  - 无参数时不会直接提交，而是生成一条 `Query` 消息，让模型帮你审查变更并创建提交
  - 有参数时执行 `git commit -m "<message>"`
- Notes:
  - 当前实现不会自动 `git add`
  - 注释写了 “Stages all changes”，但代码实际没有做 stage
- Examples:
  - `/commit`
  - `/commit docs: add command reference`

### `/export`

- Aliases: none
- Syntax:
  - `/export`
  - `/export list`
  - `/export <path>`
  - `/export <session-id-or-prefix>`
- Behavior:
  - 无参数：导出当前 session 到默认导出目录
  - `list`：列出已有 Markdown 导出
  - 带 `/`、`\\` 或 `.md` 结尾的参数：当成路径
  - 其他参数：当成 session id / prefix
- Examples:
  - `/export`
  - `/export list`
  - `/export notes/session.md`
  - `/export 550e8400`

### `/audit-export`

- Aliases: `/audit`
- Syntax:
  - `/audit-export`
  - `/audit-export list`
  - `/audit-export verify <path>`
  - `/audit-export <path>`
  - `/audit-export <session-id-or-prefix>`
- Behavior:
  - 导出可校验 audit record，或校验已有 audit 文件
  - 路径识别规则：包含 `/`、`\\` 或 `.json` 后缀
- Examples:
  - `/audit-export`
  - `/audit-export list`
  - `/audit-export verify ~/.cc-rust/audits/run.audit.json`
  - `/audit-export artifacts/run.audit.json`
  - `/audit 550e8400`

### `/session-export`

- Aliases: `/sexport`
- Syntax:
  - `/session-export`
  - `/session-export list`
  - `/session-export <path>`
  - `/session-export <session-id-or-prefix>`
- Behavior:
  - 导出结构化 session JSON 包
  - 路径识别规则：包含 `/`、`\\` 或 `.json` 后缀
- Examples:
  - `/session-export`
  - `/session-export list`
  - `/session-export artifacts/session.json`
  - `/sexport 550e8400`

## MCP / Plugin Commands

### `/hooks`

- Aliases: none
- Syntax:
  - `/hooks`
  - `/hooks list`
  - `/hooks list <event>`
  - `/hooks path <managed|user|project|local>`
  - `/hooks open <managed|user|project|local>`
- Behavior:
  - 展示合并后的 hook tree，按 event → matcher → hook 分组
  - `list <event>` 只显示指定 hook event
  - `path` 输出对应 settings 文件路径
  - `open` 创建缺失文件后通过 `$VISUAL` / `$EDITOR` 打开
  - 无参数在 Rust TUI 中打开 `HooksSurface`，可浏览 settings scope、event、matcher 和 hook 列表
- Examples:
  - `/hooks`
  - `/hooks list PreToolUse`
  - `/hooks path project`
  - `/hooks open user`

### `/mcp`

- Aliases: none
- Syntax:
  - `/mcp`
  - `/mcp list`
  - `/mcp ls`
  - `/mcp status`
  - `/mcp add <name> --transport=streamable-http --url=<url>`
  - `/mcp connect <name>`
  - `/mcp disconnect <name>`
  - `/mcp reconnect <name>`
  - `/mcp remove <name> [--scope=<user|project|local>]`
  - `/mcp auth start <name>`
  - `/mcp auth complete <name> --code=<code> [--state=<state>]`
  - `/mcp auth status <name>`
  - `/mcp auth clear <name>`
- Notes:
  - `connect` / `disconnect` / `reconnect` route through the shared runtime MCP manager.
  - `auth start` / `auth complete` perform manual OAuth PKCE setup for OAuth-enabled MCP servers; tokens are stored under `~/.cc-rust/` or `CC_RUST_HOME`, not echoed in command output.
  - `auth status` / `auth clear` show redacted credential state or remove stored OAuth state.
  - `streamable-http` is the current standard MCP HTTP transport; legacy `sse` remains supported for compatibility.
- Behavior:
  - 普通命令无参数时显示帮助和 `mcpServers` 配置示例；Rust TUI 无参数时打开 `McpSurface`
  - `list`：列出当前发现到的 MCP servers
  - `status`：输出 discovery 视图，运行态提示转到 SystemStatus / headless IPC
  - TUI surface 支持 server list、server detail、kind/settings view、tool list、tool detail 和 reconnect/status actions
- Examples:
  - `/mcp`
  - `/mcp list`
  - `/mcp status`
  - `/mcp add remote --transport=streamable-http --url=https://mcp.example.com/mcp`
  - `/mcp remove remote --scope=user`
  - `/mcp auth status context7`

## Agent Teams / Background Tasks

### `/tasks`

- Aliases: none
- Syntax:
  - `/tasks`
  - `/tasks show <id>`
  - `/tasks stop <id>`
  - `/tasks delete <id>`
- Behavior:
  - 聚合 tool-driven tasks 和 in-process teammate tasks
  - `show` 展示单个 task 的 retained output、metadata、remote/team detail
  - `stop` 只取消 tool task；team task 会提示改用 `/team kill <name>`
  - `delete` 删除 persisted tool task；team task 是 runtime-only，不能从 tool storage 删除
  - 无参数在 Rust TUI 中打开 `TasksSurface`，支持 task list/detail、shell/remote/agent/team/MCP/dream/workflow detail renderers，以及 stop/delete/refresh action
- Examples:
  - `/tasks`
  - `/tasks show task-123`
  - `/tasks stop task-123`
  - `/tasks delete task-123`

### `/team`

- Aliases: `/teams`
- Syntax:
  - `/team`
  - `/team status`
  - `/team list`
  - `/team create <name> [description]`
  - `/team spawn <name> <prompt>`
  - `/team send <name> <message>`
  - `/team kill <name>`
  - `/team leave`
  - `/team delete <name>`
- Behavior:
  - 管理 Agent Teams：创建/激活 team、spawn in-process teammate、发送 mailbox 消息、强制停止 teammate、离开或删除 team
  - 无参数在 Rust TUI 中打开 `TeamSurface`，显示 team status 概览和 teams dialog detail；`s` 填充 send prompt，`k` kill 选中 teammate，`p`/`c` 填充 spawn/create prompt
- Notes:
  - `leave` 只清除当前 session 的 active team context，不删除磁盘数据
  - `delete` 会删除 team config/mailbox 并尝试停止非 lead teammate
- Examples:
  - `/team`
  - `/team create ui-port Finish UI wiring`
  - `/team spawn builder Implement task panel`
  - `/team send builder please summarize status`
  - `/team kill builder`
  - `/teams list`

### `/plugin`

- Aliases: none
- Syntax:
  - `/plugin`
  - `/plugin list`
  - `/plugin ls`
  - `/plugin status`
  - `/plugin enable <plugin-id>`
  - `/plugin disable <plugin-id>`
- Behavior:
  - 无参数时显示帮助
  - `enable/disable` 会改 `~/.cc-rust/plugins/installed_plugins.json`
- Notes:
  - `enable` / `disable` 缺少 plugin id 时会返回真正的错误，而不是普通文本输出
- Examples:
  - `/plugin`
  - `/plugin list`
  - `/plugin status`
  - `/plugin enable github`
  - `/plugin disable github`

## KAIROS / Assistant Commands

### `/brief`

- Aliases: none
- Syntax:
  - `/brief`
  - `/brief on`
  - `/brief off`
  - `/brief status`
- Supported synonyms:
  - `on` / `enable`
  - `off` / `disable`
- Behavior:
  - 无参数时切换 brief mode
- Feature gate:
  - 需要 `FEATURE_KAIROS_BRIEF=1`
- Examples:
  - `/brief`
  - `/brief on`
  - `/brief status`

### `/sleep`

- Aliases: none
- Syntax:
  - `/sleep`
  - `/sleep <seconds>`
- Behavior:
  - 无参数时显示用法和当前 tick interval
  - 参数必须是 `1..3600` 范围内的整数秒
- Feature gate:
  - 需要 `FEATURE_PROACTIVE=1`
- Examples:
  - `/sleep`
  - `/sleep 60`
  - `/sleep 300`

### `/assistant`

- Aliases: `/kairos`
- Syntax: `/assistant`
- Behavior:
  - 显示 KAIROS / assistant mode 状态
- Notes:
  - 当前实现不解析子参数；任何额外参数都会被忽略
- Feature gate:
  - 需要 `FEATURE_KAIROS=1`
- Examples:
  - `/assistant`
  - `/kairos`

### `/daemon`

- Aliases: none
- Syntax:
  - `/daemon`
  - `/daemon status`
  - `/daemon stop`
- Behavior:
  - 无参数默认等价于 `status`
  - `stop` 目前只是返回“stop requested”提示
- Feature gate:
  - 需要 `FEATURE_KAIROS=1`
- Examples:
  - `/daemon`
  - `/daemon status`
  - `/daemon stop`

### `/notify`

- Aliases: none
- Syntax:
  - `/notify`
  - `/notify status`
  - `/notify test`
  - `/notify on`
  - `/notify off`
- Behavior:
  - 无参数默认等价于 `status`
  - 当前实现主要返回状态 / 提示文本
- Feature gate:
  - 需要 `FEATURE_KAIROS_PUSH_NOTIFICATION=1`
- Examples:
  - `/notify`
  - `/notify test`
  - `/notify on`

### `/channels`

- Aliases: none
- Syntax:
  - `/channels`
  - `/channels list`
  - `/channels status`
- Behavior:
  - 无参数默认等价于 `list`
  - feature gate 开启后显示 gateway-backed outbound adapter status/control
  - Telegram/Lark 当前仅支持 adapter 连接状态、健康检查和 allowlisted test-message；不表示 inbound channel session 已接通
- Feature gate:
  - 需要 `FEATURE_KAIROS_CHANNELS=1`
- Examples:
  - `/channels`
  - `/channels list`
  - `/channels status`

### `/dream`

- Aliases: none
- Syntax:
  - `/dream`
  - `/dream --days <N>`
  - `/dream help`
  - `/dream --help`
- Behavior:
  - 无参数默认蒸馏最近 7 天日志
  - `--days N` 要求 `N > 0`
- Feature gate:
  - 需要 `FEATURE_KAIROS=1`
- Examples:
  - `/dream`
  - `/dream --days 14`
  - `/dream help`

## Source Notes

一些命令的“文案目标”和“代码行为”目前有差异，写文档时已按代码落地行为处理：

- `/commit`：当前不会自动 stage，只在有 message 时跑 `git commit -m`
- `/effort`：当前只支持 `low|medium|high`
- `/copy`：当前不真正写系统剪贴板
- `/notify`、`/daemon stop`：目前偏状态/占位接口
- `/channels`：2026-05-10 起在 feature gate 打开后显示 gateway-backed outbound adapter status；inbound channel sessions / `/teleport` 仍为 deferred，不应当作已接通能力。
- `/mcp status`：当前偏 discovery 视图，不是实时连接面板

如果后续继续补文档，下一步适合增加：

- 每个命令的失败示例
- 每个命令对应源码路径
- 哪些命令会返回 `Clear` / `Exit` / `Query` 而不是普通文本
