# cc-rust docs map

This directory separates current project documentation from historical records.

## Active entry points

- [WORK_STATUS.md](WORK_STATUS.md): current project status and active work areas.
- [reference/](reference/): architecture and technical reference (crate migration, IPC protocol, daemon ops, API reference, etc.).
- [schemas/](schemas/): JSON schema definitions.
- [archive/](archive/): historical plans, reference docs, debug logs, and completed records.

## `.cc-rust` 文件位置

cc-rust 的数据和配置文件分布在两个层级：

### 用户级全局目录 (`~/.cc-rust/`)

| 路径 | 用途 |
|------|------|
| `~/.cc-rust/settings.json` | 全局设置（模型、后端、API key、主题等） |
| `~/.cc-rust/settings.json.backup-*` | 设置文件自动备份 |
| `~/.cc-rust/projects/` | 项目级配置快照 |
| `~/.cc-rust/logs/` | 运行日志 |
| `~/.cc-rust/sessions/` | 会话持久化数据 |
| `~/.cc-rust/transcripts/` | 会话转录记录 |
| `~/.cc-rust/runs/` | 历史运行记录 |
| `~/.cc-rust/tasks/` | 任务数据 |
| `~/.cc-rust/teams/` | Team Memory 数据 |
| `~/.cc-rust/memory/` | auto-memory 持久化 |
| `~/.cc-rust/file-write-history/` | 文件写入历史 |
| `~/.cc-rust/keybindings.json` | 键盘快捷键绑定 |
| `~/.cc-rust/trusted-workspaces.json` | 受信工作区列表 |
| `~/.cc-rust/plan-workflow.json` | 计划工作流配置 |
| `~/.cc-rust/skill-usage.json` | 技能使用统计 |

### 项目级目录 (`<repo>/.cc-rust/`)

| 路径 | 用途 |
|------|------|
| `<repo>/.cc-rust/settings.json` | 项目级设置覆盖 |
| `<repo>/.cc-rust/plan-workflow.json` | 项目级计划工作流配置 |
| `<repo>/.cc-rust/readme.md` | 项目级读我文件 |

## Archive policy

- Historical plans, closed phase records, debug logs, user-facing configuration docs, and other non-structural documents live under [archive/](archive/).
- When a TODO is implemented, move its detailed completion note to archive and leave only a short current-state reference in active docs.
- Do not keep completed phase logs at the top of [WORK_STATUS.md](WORK_STATUS.md).
