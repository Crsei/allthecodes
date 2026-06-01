# 近两个月 Claude Code 功能更新 & allthecodes 实现差距分析

> 生成日期: 2026-05-29
> 范围: Claude Code changelog v2.1.86 (2026-03-27) 至 v2.1.156 (2026-05-29)
> 来源: `docs/reference/anthropic/what's_changed.md` (共~83个版本发布)
> 对比目标: allthecodes (Rust) 代码库 — 识别已实现、部分实现、未实现的功能点

---

## 总览

| 分类 | 数量 |
|------|------|
| **Claude Code 近两个月新功能/变化总条目** | ~400+ (含 bugfix) |
| **其中实质性功能新增** | ~80+ |
| **allthecodes 已实现/部分实现的对应功能** | ~50 |
| **allthecodes 尚未实现的对应功能** | ~20+ |
| **无法直接对应（allthecodes 架构差异）** | ~10 |

---

## 一、Claude Code 近两个月核心功能新增摘要

### 1.1 模型与核心能力 (2026-04~05)
- **Opus 4.8 发布** (v2.1.154, 2026-05-28): 默认高努力，`/effort xhigh`
- **Opus 4.7 xhigh** (v2.1.111, 2026-04-16): 新增 xhigh 努力级别
- **Fast mode 改进**: Opus 4.8 fast mode 降至 2x 成本；Fast mode 默认 Opus 4.7
- **Auto mode**: 无需 opt-in (v2.1.152); Max 订阅用户可用 Opus 4.7 auto mode (v2.1.111); 不再需要 `--enable-auto-mode`
- **Lean system prompt**: 默认对除 Haiku/Sonnet/Opus 4.7 外的模型生效

### 1.2 工作流与多 Agent (Workflows)
- **动态工作流引入** (v2.1.154): 用户可要求 Claude 创建 workflow，跨数十到数百个 agent 后台编排
- **工作流系统全面增强**: 进度显示简化、agent 计数、`budget` token 预算控制
- **`/ultrareview`** (v2.1.111): 云端并行多 agent 代码审查
- **`/ultraplan`** 增强: 自动创建云环境

### 1.3 Agent 视图与会话管理
- **Agent view (Research Preview)** (v2.1.139): `claude agents` 单列表显示所有会话
- **`claude agents` 增强**: `--json` 脚本输出; `--add-dir`, `--settings`, `--mcp-config`, `--plugin-dir`, `--permission-mode`, `--model`, `--effort`, `--dangerously-skip-permissions` 等标志
- **Pinned background sessions** (v2.1.147): Ctrl+T 固定后台会话，空闲保持活跃，更新就地重启
- **`/bg` 改进**: 保留 MCP 配置、settings、fallback-model、permission mode; 后台保持输入状态而不是发送 "continue"
- **后台会话**: 模型/努力级别在唤醒后保留; 工作树隔离守卫; daemon 退出清理
- **`claude agents` dashboard**: 新 tab 布局 (Running/Library); 有限权限模式; 会话重命名即时更新

### 1.4 命令系统改进
- **`/goal` 命令** (v2.1.139): 设置完成条件，跨轮次持续工作
- **`/code-review`** (v2.1.152 → v2.1.147): 替代 `/simplify`; 报告正确性 bug; `--comment` 发 GitHub PR 内联评论; `--fix` 自动应用
- **`/simplify`** (v2.1.152): 改为纯 cleanup-only review (复用/简化/效率)
- **`/usage`** (v2.1.118): 合并 `/cost` 和 `/stats`; 按类别细分 token 消耗; 大会话文件支持; 5小时和周用量即时显示
- **`/feedback`** (v2.1.141): 可附带最近会话 (24h/7d); 改进重试
- **`/reload-skills`** (v2.1.152): 无需重启即可重新扫描技能目录
- **`/less-permission-prompts`** skill (v2.1.111): 扫描 transcript 生成 allowlist
- **`/powerup`** (v2.1.90): 交互式功能教学
- **`/scroll-speed`** (v2.1.139): 鼠标滚轮速度调节
- **`/team-onboarding`** (v2.1.101): 生成 teammate 入门指南
- **`/undo` = `/rewind`** 别名 (v2.1.108)
- **`/proactive` = `/loop`** 别名 (v2.1.105)
- **`/tui fullscreen`** (v2.1.110): 无闪烁全屏渲染
- **`/config` 改动**: `/settings` 更名为 `/config` (v2.1.119), 设置持久化到 `~/.claude/settings.json`

### 1.5 插件系统
- **`plugin.json` 增强**: `defaultEnabled: false` (v2.1.154); `themes`/`monitors` 移入 `experimental` (v2.1.129)
- **Plugin 依赖强制** (v2.1.143): `disable` 检查依赖; `enable` 自动启用传递依赖
- **`/plugin` UI 增强**: 发现/浏览显示 commands/agents/skills/hooks/MCP/LSP; 上次更新时间; 预估 token 成本
- **`claude plugin details`** (v2.1.139): 组件清单和 token 成本估算
- **`claude plugin prune`** (v2.1.121): 清理孤立的自动安装依赖
- **`claude plugin tag`** (v2.1.118): 创建 release git tag + 版本验证
- **`pluginSuggestionMarketplaces`** 托管设置 (v2.1.152)
- **`--plugin-url <url>`** (v2.1.129): 从 URL 获取 plugin zip

### 1.6 MCP 与工具系统
- **`alwaysLoad` MCP 配置** (v2.1.121): 跳过工具搜索延迟
- **工具搜索默认关闭 on Vertex** (v2.1.119)
- **MCP `workspace` 保留名称** (v2.1.128)
- **MCP 工具结果持久化覆盖** (v2.1.91): `_meta["anthropic/maxResultSizeChars"]`
- **流式工具执行始终启用** (v2.1.154)
- **MCP 连接增强**: HTTP/SSE 重试; OAuth 完善; 分页响应

### 1.7 Hook 系统
- **`MessageDisplay` hook** (v2.1.152): 转换或隐藏显示消息
- **`SessionStart` 增强** (v2.1.152): 可返回 `reloadSkills: true`, 设置 `sessionTitle`
- **`PermissionDenied` hook** (v2.1.89): auto mode 拒绝后触发, `{retry: true}` 重试
- **`PreCompact` hook** (v2.1.105): exit code 2 或 `{"decision":"block"}` 阻止压缩
- **Hook `continueOnBlock`** (v2.1.139): PostToolUse 继续执行
- **Hook `terminalSequence`** (v2.1.141): 桌面通知/窗口标题
- **Hook `args: string[]`** (v2.1.139): exec form
- **PostToolUse 可替换工具输出** (v2.1.121): `hookSpecificOutput.updatedToolOutput`
- **Hook 条件过滤** `if` 字段 (v2.1.85): `Bash(git *)` 语法

### 1.8 主题与显示
- **自定义主题** (v2.1.118): 创建/切换命名的主题; 插件可打包 `themes/`
- **"Auto (match terminal)" 主题** (v2.1.111)
- **全屏模式**: Shift+↑/↓ 选区扩展 (v2.1.113); `autoScrollEnabled` 配置 (v2.1.110)
- **焦点模式** (v2.1.97): Ctrl+O 切换，显示精简视图
- **会话回顾/recap** (v2.1.108): 返回会话时提供上下文

### 1.9 权限与安全
- **Sandbox 增强**: `sandbox.bwrapPath`/`socatPath` (v2.1.133); `sandbox.failIfUnavailable` (v2.1.83); `sandbox.network.deniedDomains` (v2.1.113)
- **`CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`** (v2.1.83): 剥离凭据
- **`CLAUDE_CODE_SCRIPT_CAPS`** (v2.1.98): 限制每会话脚本调用
- **`WorktreeCreate` HTTP hook** (v2.1.84): 返回 `worktreePath`
- **`CLAUDE_CODE_PERFORCE_MODE`** (v2.1.98): 只读文件 `p4 edit` 提示
- **Auto mode 分类器**: `hard_deny` (v2.1.136); 不覆盖显式用户边界 (v2.1.90); 权限对话框显示原因 (v2.1.141)

### 1.10 托管设置与企业
- **`managed-settings.d/`** 碎片目录 (v2.1.83)
- **`parentSettingsBehavior`** admin-tier (v2.1.133)
- **`allowAllClaudeAiMcps`** (v2.1.149)
- **`forceRemoteSettingsRefresh`** (v2.1.92)
- **`allowedChannelPlugins`** (v2.1.84)
- **`CLAUDE_CODE_USE_MANTLE`** (v2.1.94): Bedrock Mantle 支持

### 1.11 杂项
- **`CLAUDE_CODE_SESSION_ID`** 环境变量 (v2.1.132 → v2.1.154)
- **`CLAUDE_CODE_FORK_SUBAGENT=1`** (v2.1.117): 外部构建启用分叉 subagent
- **`ENABLE_PROMPT_CACHING_1H`** (v2.1.108): 1小时提示缓存
- **`CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN`** (v2.1.132): 禁用 alternate screen
- **`CLAUDE_CODE_PLUGIN_PREFER_HTTPS`** (v2.1.141)
- **Push notification tool** (v2.1.110)
- **`claude project purge`** (v2.1.126): 删除项目所有状态
- **`--bare` 标志** (v2.1.81): 脚本调用跳过 hooks/LSP/plugin
- **`--channels` 权限转发** (v2.1.81)
- **`prUrlTemplate`** (v2.1.119): 自定义 PR badge URL
- **PowerShell 工具默认启用 on Windows** (v2.1.143)
- **Verbose 模式持久化** (v2.1.119); **`--exclude-dynamic-system-prompt-sections`** (v2.1.98)
- **CJK 支持改进**: @-mention 不需要前置空格; Indic 脚本渲染; IME 合成在光标位置

---

## 二、allthecodes 实现状态对比

### 2.1 已实现的功能（对应 Claude Code 近两月更新）

| 功能 | Changelog 版本 | allthecodes 位置 | 备注 |
|------|---------------|-----------------|------|
| Agent view / 仪表板 | v2.1.139+ | `crates/allthecodes/src/dashboard.rs` | SubagentDashboard 功能门控 |
| Vim visual mode | v2.1.118 | `crates/allthecodes/src/ui/input/vim.rs` | Normal/Insert/Visual 完整支持 |
| 插件系统全生命周期 | 多个版本 | `crates/allthecodes-plugins/` | marketplace, install, enable, disable, update, validate |
| `/plugin info/details` | v2.1.139 | `crates/allthecodes-commands/src/plugin_cmd.rs` | 组件清单展示 |
| MCP 工具系统 | 多个版本 | `crates/allthecodes-mcp/` | stdio/SSE/HTTP 传输, 搜索, 分页 |
| 工具延迟/工具搜索 | v2.1.121+ | `crates/allthecodes-tools/src/deferred_tools.rs` | 含 SearchExtraTools |
| PowerShell tool | v2.1.84+ | `crates/allthecodes-tools/src/exec/powershell.rs` | engine 层也有实现 |
| Sandbox 沙箱 | v2.1.133+ | `crates/allthecodes-sandbox/` | bwrap, network, filesystem |
| Hook 系统 | 多个版本 | `crates/allthecodes-tools/src/hooks/` | PreToolUse, PostToolUse, SessionStart, Stop 等 |
| Monitor tool | v2.1.98+ | `crates/allthecodes-tools/src/product_tools.rs` | MonitorTool struct |
| 焦点模式 / 简洁视图 | v2.1.97+ | `crates/allthecodes/src/ui/runtime/transcript.rs` | Ctrl+O 切换 Prompt/Transcript/Focus |
| OpenTelemetry | v2.1.89+ | `crates/allthecodes-observability/` | telemetry 功能门控 |
| 技能系统 | v2.1.152+ | `crates/allthecodes-skills/` | 加载/重载/路径过滤/依赖 |
| `/skills` | v2.1.111+ | `crates/allthecodes-commands/src/skills_cmd.rs` | list, reload, 排序 |
| `/skills reload` | v2.1.152 | `crates/allthecodes-commands/src/skills_cmd.rs` | 热重载 |
| `/loop` / 计划任务 | v2.1.139+ | `crates/allthecodes-commands/src/loop_cmd.rs` | CronCreate/CronDelete/CronList |
| 工作树隔离 | v2.1.133+ | `crates/allthecodes-worktree/` | EnterWorktree/ExitWorktree |
| Agent Teams | v2.1.111+ | `crates/allthecodes-teams/` | coordinator, runner, supervisor |
| LSP 服务 | v2.1.111+ | `crates/allthecodes-lsp-service/` | ide, recommendation, client |
| `/insights` | v2.1.111+ | `crates/allthecodes-commands/src/insights.rs` | 会话历史分析 |
| `/doctor` | v2.1.119+ | `crates/allthecodes-commands/src/doctor.rs` | 认证/配置/MCP/快捷键诊断 |
| `/export` | v2.1.119+ | `crates/allthecodes-commands/src/export.rs` | Markdown 导出 |
| `/config` 设置持久化 | v2.1.119 | `crates/allthecodes-commands/src/config_cmd.rs` | user/project/local 三级作用域 |
| `/review` | v2.1.147+ | `crates/allthecodes-commands/src/review.rs` | 代码审查 |
| `/simplify` | v2.1.147+ | `crates/allthecodes-commands/src/simplify.rs` | 多 agent 简化审查 |
| `/cost` | v2.1.118 | `crates/allthecodes-commands/src/cost.rs` | Token 用量与成本 (尚未合并到 /usage) |
| `/extra-usage` (= 旧版 `/usage`) | v2.1.149+ | `crates/allthecodes-commands/src/extra_usage.rs` | 扩展分析 |
| `/remote` / Remote Control | 多个版本 | `crates/allthecodes-commands/src/remote_cmd.rs` + gateway crate | gateway bridge |
| 远程 Control / daemon | 多个版本 | `crates/allthecodes-daemon/src/gateway_bridge.rs` | 会话密钥, 策略 |
| 推送通知 | v2.1.110 | `crates/allthecodes-commands/src/notify.rs` | 功能门控 |
| `/team-onboarding` | v2.1.101 | `crates/allthecodes-commands/src/team_onboarding.rs` | 入门指南生成 |
| `/scroll-speed` | v2.1.139 | `crates/allthecodes/src/ui/platform/terminal_env.rs` | ALLTHECODES_SCROLL_SPEED |
| `/terminal-setup` | 多个版本 | `crates/allthecodes-commands/src/terminal_setup.rs` | 终端配置 |
| Transcript search | v2.1.83 | `crates/allthecodes/src/ui/runtime/transcript.rs` | 大小写不敏感搜索, n/N 导航 |
| Resume / --resume | 多个版本 | `crates/allthecodes-commands/src/resume.rs` | 基本功能 |
| Deferred tools | v2.1.121+ | `crates/allthecodes-tools/src/deferred_tools.rs` | WebSearch/WebFetch 延迟执行 |
| `/hooks` | 多个版本 | `crates/allthecodes-commands/src/hooks_cmd.rs` | list/path/open 子命令 |
| `/sandbox` | v2.1.133+ | `crates/allthecodes-commands/src/sandbox_cmd.rs` | 沙箱控制 |

### 2.2 部分实现的功能

| 功能 | Changelog 版本 | allthecodes 实现状态 | 差距 |
|------|---------------|---------------------|------|
| 主题系统 | v2.1.111+ | 主题渲染/设置/验证已实现 | 无 "Auto (match terminal)"、无主题 marketplace、无 `themes/` 目录插件支持 |
| Voice 语音 | v2.1.111+ | `allthecodes-voice` crate 存在 | 仅兼容性存根 (NullAudioBackend)，无真实录音/转录 |
| `/usage` 统一 | v2.1.118 | `/cost` + `/extra-usage` 分开 | 未合并为统一 `/usage` 命令 |
| `/plugin prune/tag` | v2.1.118+ | 无对应子命令 | 无孤依赖清理、无 git tag 发布 |
| `/bg` 改进 | v2.1.143+ | 无对应命令 | 无 `/bg` 保留标志、无后台分叉 |
| `SessionStart reloadSkills` | v2.1.152 | `reload_skills_with_extra` 已存在 | `SessionStart` hook 未集成 `reloadSkills: true` 返回值 |
| `disallowed-tools` 前端设置 | v2.1.152 | 危险命令检测存在 | 无显式"禁用内联 shell 执行"用户配置切换 |

### 2.3 未实现的功能（allthecodes 缺失）

| # | 功能 | Changelog 版本 | 描述 | 优先级 |
|---|------|---------------|------|--------|
| 1 | **Workflow 系统** | v2.1.154 | 动态工作流编排引擎，跨多 agent 后台编排 | **P0** |
| 2 | **`/goal` 命令** | v2.1.139 | 设置完成条件，跨轮次持续工作，显示 live elapsed/turns/tokens | P1 |
| 3 | **全屏模式 (`/tui fullscreen`)** | v2.1.110 | 无闪烁渲染，虚拟化滚动回退 | P1 |
| 4 | **`/feedback` 命令** | v2.1.141 | 含会话附件的反馈，支持 24h/7d 范围 | P1 |
| 5 | **`/less-permission-prompts` skill** | v2.1.111 | 扫描 transcript 生成 allowlist | P1 |
| 6 | **`/powerup` 交互式教学** | v2.1.90 | 动画演示的 CLI 功能教程 | P2 |
| 7 | **`/ultraplan` / `/ultrareview`** | v2.1.111 | 云端并行多 agent 分析 (需远程后端) | P2 |
| 8 | **代理视图 `--json`** | v2.1.145 | `claude agents --json` 脚本输出 | P2 |
| 9 | **`MessageDisplay` hook** | v2.1.152 | 转换/隐藏助手消息文本 | P2 |
| 10 | **Pinned background sessions** | v2.1.147 | Ctrl+T 固定后台会话，空闲保持活跃 | P2 |
| 11 | **后台会话/daemon 管理** | v2.1.143+ | daemon 退出清理、工作树守卫、唤醒保留设置 | P2 |
| 12 | **`/buddy` 彩蛋** | v2.1.89 | 愚人节功能 (小生物) | P3 |
| 13 | **`/proactive` 别名** | v2.1.105 | `/loop` 别名 | P3 |
| 14 | **`/undo` 别名** | v2.1.108 | `/rewind` 别名 | P3 |
| 15 | **`/model` 仅当前会话** | v2.1.144 | 默认仅当前会话，`d` 设为默认；重命名 `modelPicker` 快捷键 | P3 |
| 16 | **`--bare` 标志** | v2.1.81 | 脚本模式跳过多余初始化 | P3 |
| 17 | **`--channels` 权限转发** | v2.1.81 | 通道服务器权限推送 | P3 |
| 18 | **`prUrlTemplate`** | v2.1.119 | 自定义 PR badge URL | P3 |
| 19 | **`CLAUDE_CODE_SESSION_ID` env var** | v2.1.132 | Bash 工具子进程环境变量 | P3 |
| 20 | **`skillOverrides` 设置** | v2.1.129 | off/user-invocable-only/name-only | P3 |
| 21 | **`xhigh` 努力级别** | v2.1.111 | Opus 4.7 新努力级别 | 模型相关 |
| 22 | **`ENABLE_PROMPT_CACHING_1H`** | v2.1.108 | 1h 提示缓存 TTL | 模型相关 |

### 2.4 架构性差异（不直接对应）

| 功能 | 说明 |
|------|------|
| Opus 4.7/4.8 模型支持 | allthecodes 不需要跟随每个模型发布 |
| Claude Code 原生二进制 | allthecodes 本身就是 Rust 原生构建 |
| Bun → 原生迁移 | 不适用，allthecodes 已经是 Rust |
| `CLAUDE_CODE_OPUS_4_6_FAST_MODE_OVERRIDE` 弃用 | 模型版本策略差异 |
| VS Code 扩展特定 | 不适用 |
| Windows-only 修复 | 部分不适用 |
| macOS-only 修复 | 部分不适用 |

---

## 三、高优先级缺失功能详细分析

### 3.1 Workflow 系统（P0）

**Claude Code 实现** (v2.1.154):
- 动态 workflow：用户要求后，Claude 创建跨数十到数百 agent 的后台编排
- 进度显示：实时 agent 计数，持久 workflow 状态行
- `budget` token 预算控制
- Workflow 脚本 DSL

**allthecodes 现状**:
- `TASK_KIND_LOCAL_WORKFLOW` 常量存在 (`crates/allthecodes-tasks/src/domain.rs:13`) 但仅为任务路由标签
- `plan_workflow.rs` 是 plan-mode 工作流持久化，非通用编排引擎
- 缺少 Workflow 工具、DSL、多 agent 编排框架

**实现建议**: 参考 `docs/reference/anthropic/what's_changed.md` v2.1.154 中 Workflow 设计。核心组件：
1. Workflow 脚本解析器 (JavaScript-like DSL)
2. Agent 调度池 (并发约束 min(16, cpu-2))
3. 进度追踪与展示
4. `budget` token 预算
5. `agent()`, `parallel()`, `pipeline()`, `phase()`, `log()` API

### 3.2 `/goal` 命令（P1）

**Claude Code 实现** (v2.1.139):
- 设置完成条件后跨轮次持续工作
- 交互式、`-p` 和 Remote Control 均支持
- Live 显示 elapsed/turns/tokens 叠加面板
- 完成后自动停止

**allthecodes 现状**:
- 无对应实现

### 3.3 全屏模式 `/tui fullscreen`（P1）

**Claude Code 实现** (v2.1.110):
- 无闪烁 alternate screen 渲染
- 虚拟化滚动回退
- 低内存占用、鼠标支持、选择即自动复制
- `autoScrollEnabled` 配置

**allthecodes 现状**:
- 无对应实现。所有 fullscreen 引用仅与截屏相关

### 3.4 `/feedback` 命令（P1）

**Claude Code 实现** (v2.1.141):
- 可附带最近会话 (24h/7d)
- 改进重试逻辑
- 反馈包含收集前对话历史 (利于调试长会话问题)
- `/feedback` 不可用时说明原因

**allthecodes 现状**:
- 无对应实现

### 3.5 `/less-permission-prompts` skill（P1）

**Claude Code 实现** (v2.1.111):
- 扫描 transcripts 中常见只读 Bash 和 MCP 工具调用
- 生成优先 allowlist 写入 `.claude/settings.json`
- 减少权限提示频率

**allthecodes 现状**:
- 仅在 config types 和依赖模块中有引用，无独立 skill 实现

---

## 四、建议行动路线

### 第一阶段 (P0-P1, 快速跟进)
1. **Workflow 系统** — 核心编排引擎，最显著的功能差距
2. **`/goal` 命令** — 跨轮次持续工作，用户体验提升大
3. **全屏模式 `/tui fullscreen`** — 显示体验基础能力
4. **`/feedback` 命令** — 用户反馈渠道

### 第二阶段 (P1-P2, 生态补齐)
5. **`/less-permission-prompts` skill** — 权限体验优化
6. **`MessageDisplay` hook** — Hook 系统扩展
7. **`/usage` 统一** — 合并 `/cost` + `/extra-usage`
8. **`claude agents --json`** — 脚本化输出
9. **`/bg` 改进** — 保留标志、后台分叉

### 第三阶段 (P2-P3, 锦上添花)
10. **`/powerup`** — 交互式教学
11. **`/plugin prune/tag`** — 插件维护
12. **`/proactive` / `/undo` 别名** — 易用性
13. **`/buddy`** — 彩蛋
14. **`prUrlTemplate`** — 自定义集成

---

## 五、相关文档

- [Claude Code 近两月完整变更记录](docs/reference/anthropic/what's_changed.md)
- [allthecodes 缺失工具更新适配计划](docs/archive/plan/missing-tools-adaptation-plan-2026-05-28.md)
- [bash-shell-parity-migration-plan-2026-05-18.md](docs/archive/plan/bash-shell-parity-migration-plan-2026-05-18.md)
- [cc-engine-migration-plan-2026-05-13.md](docs/archive/plan/cc-engine-migration-plan-2026-05-13.md)
