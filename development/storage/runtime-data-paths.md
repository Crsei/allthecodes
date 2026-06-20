# Runtime Data Storage Paths

> 本文档记录了 allthecodes 的所有持久化路径，分两个维度：
> 
> **维度一：范围** — 用户级全局 (`data_root`) 与 项目级 (`{cwd}/.allthecodes/`)
> 
> **维度二：统一路径根** — allthecodes 只读写 `.allthecodes`，不再读取旧版 `.cc-rust`
>
> 所有路径集中定义在 `crates/allthecodes-config/src/paths.rs`（全局根目录）和
> `crates/allthecodes-config/src/settings/paths.rs`（settings.json 路径），
> 各 crate 统一调用 `data_root().join("subdir")`，不自行拼路径。

---

## 一、解析链

### 用户级全局根目录 `data_root()`

```
优先级: $ALLTHECODES_HOME  →  ~/.allthecodes/  →  $TMPDIR/allthecodes (警告)
```

**运行时只选一个根**。所有"全局"路径都挂在这个选定的根下面。`CC_RUST_HOME` 和 `~/.cc-rust/` 不再参与解析，也不会作为读兼容来源。

| 条件 | 最终根目录 | 举例：sessions/ |
|------|-----------|----------------|
| 设了 `ALLTHECODES_HOME=/custom/path` | `/custom/path` | `/custom/path/sessions/` |
| 没设 env，home 目录可解析 | `~/.allthecodes/` | `~/.allthecodes/sessions/` |
| 没 home 目录 | `$TMPDIR/allthecodes`（警告） | `$TMPDIR/allthecodes/sessions/` |

> 即使机器上存在旧版 `~/.cc-rust/`，运行时也会使用 `~/.allthecodes/`。

### 项目级根目录 `{cwd}/.allthecodes/`

```
查找方式: 从 cwd 向上找最近祖先含 .allthecodes/ 的目录
读写目标: 始终使用 .allthecodes/
旧路径: .cc-rust/ 不再作为项目标记或 settings 读取来源
```

---

## 二、用户级全局路径（在 `data_root()` 之下）

### 目录

| 相对路径 | 写入的 Crate | 用途 |
|---------|-------------|------|
| `sessions/` | `allthecodes-session` (`storage.rs`) | 会话持久化 / 恢复 |
| `runs/{session_id}/` | `allthecodes` (`dashboard.rs`) | subagent 事件 NDJSON 日志 |
| `logs/YYYY/MM/` | 全局 (由 `ensure_data_root`) | 守护进程每日日志 |
| `daemon/` | `allthecodes-daemon` (`process_state/`) | 守护进程状态发布 |
| `gateway/` | `allthecodes-config` | 网关目录 |
| `gateway/runs/` | `allthecodes-config` | 网关运行记录 |
| `gateway/adapters/` | `allthecodes-config` | 网关适配器 |
| `gateway/webhooks/` | `allthecodes-config` | 网关 webhook |
| `exports/` | `allthecodes-session`, `allthecodes-commands` | 会话导出 |
| `audits/` | `allthecodes-session`, `allthecodes-commands` | 审计导出 |
| `transcripts/` | `allthecodes-config` | 对话记录 |
| `memory/` | `allthecodes-config`, `allthecodes-web` (`memory.rs`) | 记忆系统 |
| `auto_memory/` | `allthecodes-config` (memdir) | 自动捕获记忆 |
| `session-insights/` | `allthecodes-config` | 会话洞察 |
| `plugins/` (+ `cache/`) | `allthecodes-plugins`, `allthecodes-commands` | 插件系统和缓存 |
| `skills/` | `allthecodes-skills`, `allthecodes-commands` | 全局技能定义 |
| `teams/` | `allthecodes-config` | 团队协作目录 |
| `tasks/` | `allthecodes-config` | 任务系统 |
| `goals/` | `allthecodes-config` | 目标系统 |
| `workflows/` | `allthecodes-config` | 工作流持久化 |
| `vault/` | `allthecodes-config` | 保管库 |
| `notifications/` | `allthecodes-config` | 通知持久化 |
| `worktrees/` | `allthecodes-config` | Git worktree 管理 |
| `projects/{sanitized_cwd}/memory/team/` | `allthecodes-config` | 团队记忆（按项目隔离） |
| `local-memory/` | `allthecodes-tools` (`memory/mod.rs`) | 工具本地记忆 |
| `file-write-history/` | `allthecodes-tools` (`fs/safe_write.rs`) | 文件写入历史 |
| `chrome/` | `allthecodes-browser` (`setup.rs`) | Chrome 原生消息包装脚本 |
| `web/` | `allthecodes-web` (多个 handler) | Web 状态 DB、群聊、技能、看板、jobs、launchpad-snapshots、people |
| `speech/models/` | `allthecodes-web` (`settings_phase1.rs`) | Whisper STT 模型下载存储（handler 当前返回 NOT_IMPLEMENTED） |

### 文件

| 相对路径 | 写入的 Crate | 用途 |
|---------|-------------|------|
| `settings.json` | `allthecodes-mcp`, `allthecodes-lsp-service` | 用户配置 |
| `credentials.json` | `allthecodes-auth` | OAuth / API 凭据 |
| `keybindings.json` | `allthecodes-config` | 用户快捷键覆盖 |
| `skill-usage.json` | `allthecodes-config` | 技能使用统计 |
| `pr-activity-subscriptions.json` | `allthecodes-config` | PR 活动订阅 |
| `plan.md` | `allthecodes-config` | 全局 plan file |
| `plan-workflow.json` | `allthecodes-config` | 全局 plan workflow |
| `mcp-oauth.json` | `allthecodes-mcp` (`auth.rs`) | MCP OAuth 令牌 |
| `mcp-oauth-pending.json` | `allthecodes-mcp` (`auth.rs`) | MCP OAuth 待完成 |
| `scheduled_tasks.json` | `allthecodes-commands` (`schedule.rs`) | 定时任务 |
| `auth.json` | `allthecodes-daemon` (`account_auth.rs`) | 账号认证令牌 |
| `peers.json` | `allthecodes-tools` (`product/peers.rs`) | 对等节点列表 |
| `remote-trigger-audit.ndjson` | `allthecodes-tools` (`product/peers.rs`) | 远程触发审计日志 |
| `search-cookies.json` | `allthecodes-web` (`settings_phase1.rs`) | 搜索 cookie |
| `quick-prompts.json` | `allthecodes-web` (`prompts.rs`) | 快速提示 |
| `memory/entries.json` | `allthecodes-web` (`memory.rs`) | Web 记忆条目 |

---

## 三、项目级路径（在 `{cwd}/.allthecodes/` 之下）

| 相对路径 | 用途 |
|---------|------|
| `settings.json` | 项目级配置覆盖 |
| `settings.local.json` | 项目级本地配置覆盖 |
| `plan.md` | 项目级 plan file |
| `plan-workflow.json` | 项目级 plan workflow |
| `skills/` | 项目级技能包 |
| `commands/` | 项目级命令（旧版兼容） |
| `memory/` | 项目级记忆 |
| `agents/` | 项目级 Agent 定义 |
| `tasks/` | 项目级任务列表 |
| `rules/*.md` | 项目级规则文件 |

项目级路径 **始终读写 `.allthecodes/`**，不再兼容旧 `.cc-rust/settings.json`。

---

## 四、TTS

allthecodes **没有**独立的 `tts/` 存储目录。TTS（文本转语音）仅作为配置项保存在 `settings.json` 中：

- `tts_provider`
- `tts_api_key`
- `tts_voice`
- `tts_voice_custom_id`
- `tts_model`

语音相关目录 `speech/models/` 用于 Whisper **STT**（语音→文本）模型存储，而非 TTS。
