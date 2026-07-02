# 会话管理当前实现情况

日期：2026-07-02

范围：运行时会话生命周期、持久化、恢复、分支、导出、Web/TUI 入口。

## 总体状态

会话管理已经形成运行时闭环：启动时可以新建、恢复或继续会话；运行中由 `QueryEngine` 持有当前会话状态；关闭时落盘消息、转录、审计和回放记录；Web/TUI 可以列出、恢复、归档和分支会话。

当前实现不是单一存储源。主存储位于 `allthecodes-session`：

- 新写入优先走共享 SQLite 状态库。
- 同时保留 `~/.allthecodes/sessions/<session_id>.json`，用于迁移兼容、文件工具和现有统计逻辑。
- 运行记录、API 请求快照、审计事件是旁路数据源，不替代消息会话文件。

## 启动与生命周期

启动流程在 `crates/allthecodes/src/full_init.rs` 中组装：

- 普通启动创建新的 `QueryEngine` 会话。
- `--resume` 通过 `allthecodes_session::resume::get_last_session(cwd)` 找到当前工作区最近会话。
- `--continue` 加载显式会话 ID。
- 恢复时会把历史消息作为 `initial_messages` 注入 `QueryEngine`。
- dashboard/runtime 状态会同步当前 `session_id`。

关闭流程在 `crates/allthecodes/src/shutdown.rs` 中集中处理：

- 调用 `engine.shutdown_session_record().await` 关闭 durable record/replay 写入。
- flush transcript。
- 调用 `storage::save_session(session_id, messages, cwd)` 保存消息会话。
- 保存 skill usage。
- 写入 `session.end` 审计事件，包含 usage/cost 摘要。

## 持久化模型

会话摘要字段由 `SessionInfo` 表示，包含：

- `session_id`
- `created_at`
- `last_modified`
- `message_count`
- `cwd`
- `title`
- `custom_title`
- `chat_mode_override`
- `workspace_key`
- `workspace_root`
- `workspace_name`

JSON 会话文件由 `SessionFile` 表示，核心字段是：

- `session_id`
- `created_at`
- `last_modified`
- `cwd`
- `custom_title`
- `chat_mode_override`
- `messages`

路径隔离已经按 allthecodes 规则实现：会话目录来自 `allthecodes_config::paths::sessions_dir()`，即 `~/.allthecodes/sessions/`，不是原版 Codex 的 `~/.Codex/`。

## 工作区归属

会话列表按工作区归属聚合。当前规则在 `allthecodes-session` 中实现：

- Git 仓库使用 git common dir 推导稳定 `workspace_key`，支持 worktree。
- 非 Git 目录使用 canonical path。
- `workspace_root` 和 `workspace_name` 会进入会话摘要，供 Web/TUI 展示和过滤。

列表分页支持 keyset 排序：

- 排序：`last_modified DESC, created_at DESC, session_id ASC`
- 单页限制：`1..=200`
- 支持全局列表和当前工作区列表。

## 恢复、截断、归档、分支

当前已有能力：

- 恢复：按最近会话或显式 `session_id` 恢复。
- 截断：`truncate_session()` 会写入 `{session_id}.rewind-{epoch}.json` 备份。
- 归档：`archive_session()` 会同步归档 SQLite 状态，并把 JSON 文件移动到 `archive/`。
- 分支：Web 侧通过 `allthecodes_session::fork::fork_session()` 创建消息分支。
- 导出：`session_export` 可以组合原始 transcript、API 视图、API 请求快照、工具调用、压缩信息、上下文和 rollout metadata。

归档会保护活跃会话。Web 新建、恢复、归档会在 engine 正在 streaming 时返回 busy 错误，避免并发破坏当前运行状态。

## Record/Replay 与请求快照

record/replay 已经具备版本化 JSONL 格式：

- `RecordLine` 包含 `schema_version`、`seq`、`timestamp`、`session_id`、`turn_id`、`item`。
- `RecordItem` 覆盖 session meta/state、turn started/finished、message、query event、tool progress、permission request/response、question request/response、compaction boundary、snapshot、rollback、branch 和 legacy message。
- Assistant 消息记录可以携带 `usage` 和 `cost_usd`。

录制器采用 bounded channel，容量为 256；写入时执行 redaction 和持久化过滤；flush 后会 best-effort 更新 SQLite rollout index。

请求快照写入 `{sessions_dir}/{session_id}.requests.ndjson`。图片类请求会省略 base64 内容，只保留必要元数据，用于会话导出和 usage 关联。

## Web 与 TUI 入口

Web handlers 位于 `crates/allthecodes-web/src/handlers/sessions.rs`，当前暴露的核心能力包括：

- `GET /api/sessions`
- `POST /api/sessions/new`
- `GET /api/sessions/{id}`
- `POST /api/sessions/{id}/resume`
- `POST /api/sessions/{id}/archive`
- chat mode patch
- message branch、feedback、delete、regenerate、rollback、edit prepare
- profile 会话列表

新建冷会话时会重建 engine，保留 runtime app state，但清空 session-level permission grants。

TUI 侧会使用 `allthecodes_session::storage::list_workspace_sessions(cwd)` 读取当前工作区历史，并过滤出可用于 persistent history 的用户提示。

## 当前边界与待补齐点

- SQLite 与 JSON 仍然并存；部分消费方仍直接扫描 JSON 文件。
- usage 统计当前主要读取 JSON 会话文件，不直接以 SQLite 或 record/replay 为唯一来源。
- durable record/replay 已覆盖权限事件和工具进度，但还没有统一的 per-tool execution record，把工具输入、输出、权限结论、耗时和成本汇总在一个稳定记录中。
- 请求快照与 usage/model/provider 的关联依赖事件数量对齐；不对齐时会降级为 unknown 分组。
- session-level permission grants 会在新会话时清空，但持久规则仍来自配置，不属于会话存储本身。
