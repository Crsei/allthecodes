# cc-rust docs map

This directory separates active status documents from historical records.

## Active entry points

- [WORK_STATUS.md](WORK_STATUS.md): current project status and active work areas.
- [FINAL_RELEASE_PLAN.md](FINAL_RELEASE_PLAN.md): final release sequencing, gates, remaining gaps, and expected release effects.
- [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md): remaining Full Build TODOs, runtime caveats, and intentional crops.
- [KNOWN_ISSUES.md](KNOWN_ISSUES.md): single active issue and review-finding tracker.
- [COMMAND_REFERENCE.md](COMMAND_REFERENCE.md), [CLI_REFERENCE.md](CLI_REFERENCE.md), [USAGE_GUIDE.md](USAGE_GUIDE.md): command and user-facing usage references.
- [reference/CRATE_MIGRATION_GUIDE.md](reference/CRATE_MIGRATION_GUIDE.md): operational checklist for moving `claude-code-rs` modules into workspace crates.
- [reference/CRATE_MIGRATION_TARGET_STATE.md](reference/CRATE_MIGRATION_TARGET_STATE.md): ideal acceptance state for a completed crate migration.
- [plan/crate-migration-phase-plan-2026-05-14.md](plan/crate-migration-phase-plan-2026-05-14.md): ordered phase plan for completing crate migration.
- [STORAGE.md](STORAGE.md): cc-rust data-root and path isolation reference.
- [DAEMON_OPERATIONS.md](DAEMON_OPERATIONS.md): daemon operation guide.
- [cloud-providers.md](cloud-providers.md): provider configuration reference.
- [RATATUI_UI_PARITY.md](RATATUI_UI_PARITY.md): Rust TUI parity matrix and residual UI work.

## Active plans

These are still planning or implementation references, not completed history:

- [computer-use-implementation-checklist.md](computer-use-implementation-checklist.md)
- [session-export-implementation-guide.md](session-export-implementation-guide.md)
- [ipc-refactor-plan.md](ipc-refactor-plan.md)
- [traceable-logging-plan.md](traceable-logging-plan.md)
- [daemon-usability-plan.md](daemon-usability-plan.md)
- [ui-parity-update-plan.md](ui-parity-update-plan.md)
- [superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md](superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md)
- [superpowers/plans/2026-04-11-team-memory-sync.md](superpowers/plans/2026-04-11-team-memory-sync.md)
- [superpowers/plans/2026-04-12-tools-commands-test-coverage.md](superpowers/plans/2026-04-12-tools-commands-test-coverage.md)
- [superpowers/specs/2026-04-11-team-memory-sync-design.md](superpowers/specs/2026-04-11-team-memory-sync-design.md)
- [superpowers/specs/2026-04-20-workspace-split-design.md](superpowers/specs/2026-04-20-workspace-split-design.md)

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

- Completed implementation notes, closed phase records, historical issue reports, and old review documents live under [archive/](archive/).
- Long raw review reports live under [archive/issues/](archive/issues/); active summaries live in [KNOWN_ISSUES.md](KNOWN_ISSUES.md).
- When a TODO is implemented, move its detailed completion note to archive and leave only a short current-state reference in active docs.
- Do not keep completed phase logs at the top of [WORK_STATUS.md](WORK_STATUS.md) or [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md).
