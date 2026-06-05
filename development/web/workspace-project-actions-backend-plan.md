# Workspace 项目操作 — 后端适配计划

> 日期：2026-06-05
> 前端提交：`allthecodes-web@1448dfc`（`Fix chat workspace and sidebar controls`）
> 目标：补齐 Web 左侧项目分组菜单与聊天栏模型选择中需要后端真实落地的能力。

---

## 1. 背景

`allthecodes-web/development-docs/human-check.md` 提到的修复已在前端完成：

- 聊天输入栏增加可选择模型的控件。
- 左侧项目分组行移除目录与 session 数量。
- 项目行 hover 时展示新建 session 和更多菜单：
  - 置顶项目
  - 在资源管理器中打开
  - 重命名项目
  - 归档对话
  - 移除
- 默认 UI 字体改为 Cascadia Mono 优先。

其中模型选择、单个 session 新建、单个 session 归档已有后端 API 基础；项目级菜单的大部分语义目前只是前端临时状态或浏览器能力，需要后端支持后才能真正持久、跨刷新、跨进程生效。

---

## 2. 当前后端能力

| 能力 | 当前端点 | 状态 |
|------|----------|------|
| 列出 session 与 workspace metadata | `GET /api/sessions` | 已有，返回 `workspace_key`, `workspace_root`, `workspace_name` |
| 新建当前 workspace session | `POST /api/sessions/new` | 已有，只能在当前 engine cwd 新建 |
| 归档单个 session | `POST /api/sessions/{id}/archive` | 已有 |
| 列出模型 | `GET /api/models` | 已有 |
| 设置默认模型 | `POST /api/models/default` | 已有，需确认持久化 |

---

## 3. 后端缺口

### 3.1 项目元数据持久化

前端当前用 React state 保存：

- pinned project ids
- renamed project labels
- removed project ids

刷新页面或重启后会丢失。

需要后端保存 per-profile 的 workspace UI metadata。

### 3.2 在资源管理器中打开项目

前端当前尝试 `window.open(file://...)`，多数浏览器环境会阻止或无效。

需要由本机后端执行系统打开命令：

- Linux: `xdg-open <path>`
- macOS: `open <path>`
- Windows: `explorer <path>`

注意：不要在 Windows 平台编译后端；这里只是运行时行为设计。

### 3.3 在指定项目下新建 session

前端项目行的 `+` 当前复用 `POST /api/sessions/new`，只能在当前 engine cwd 新建 session。

如果用户点击的是非当前项目，应支持传入 `workspace_root` 或 `cwd`，由后端切换/重建 engine 到该 workspace 并创建新 session。

### 3.4 项目级批量归档

前端当前循环调用 `POST /api/sessions/{id}/archive`。

可用但不理想：

- 多请求，慢。
- 可能部分成功部分失败。
- 归档当前 active session 会返回冲突，前端难以给出项目级一致反馈。

建议新增批量端点。

### 3.5 默认模型持久化确认

`POST /api/models/default` 当前设置运行时 app state 与 `settings.model`。需要确认是否写入配置文件；如果不写入，重启后默认模型会恢复。

---

## 4. 建议 API

### 4.1 Workspace 元数据

```
GET /api/workspaces
PATCH /api/workspaces/{workspace_key}
```

`GET /api/workspaces` 返回：

```json
{
  "workspaces": [
    {
      "key": "repo:...",
      "root": "/path/to/project",
      "name": "allthecodes-web",
      "display_name": "Frontend",
      "pinned": true,
      "hidden": false,
      "session_count": 12,
      "last_modified": 1710000000
    }
  ]
}
```

`PATCH /api/workspaces/{workspace_key}` 请求：

```json
{
  "display_name": "Frontend",
  "pinned": true,
  "hidden": false
}
```

语义：

- `display_name`: 用户重命名项目；`null` 表示恢复默认 workspace name。
- `pinned`: 置顶项目。
- `hidden`: 移除项目；不删除真实 session，只从默认列表隐藏。

### 4.2 打开项目目录

```
POST /api/workspaces/{workspace_key}/open
```

请求：

```json
{
  "root": "/path/to/project"
}
```

响应：

```json
{
  "ok": true,
  "message": "Workspace opened"
}
```

安全约束：

- `workspace_key` 必须来自已知 session/workspace，或 `root` 必须能通过后端 workspace discovery 验证。
- 不允许任意 URL。
- 只允许本地目录路径。

### 4.3 在指定项目新建 session

```
POST /api/sessions/new
```

扩展请求体：

```json
{
  "workspace_key": "repo:...",
  "cwd": "/path/to/project"
}
```

兼容性：

- 空 body 或缺省字段时保持当前行为：在当前 engine cwd 新建 session。
- 传入 `cwd` 时，后端验证目录存在并是允许 workspace，然后用该 cwd rebuild engine。

响应仍为：

```json
{
  "session_id": "..."
}
```

### 4.4 批量归档项目对话

```
POST /api/workspaces/{workspace_key}/sessions/archive
```

请求：

```json
{
  "include_active": false
}
```

响应：

```json
{
  "ok": true,
  "archived": ["session-a", "session-b"],
  "skipped": [
    {
      "session_id": "session-active",
      "reason": "active_session"
    }
  ],
  "failed": []
}
```

建议默认跳过 active session，不强制归档当前正在使用的会话。

### 4.5 默认模型持久化

如果当前 `models_set_default_handler` 未写入配置，应调整为同时更新持久配置。

可选方案：

- 复用 settings action 的持久化路径，写入 `default_model` 或 `model`。
- 或让 `/api/models/default` 调用同一层配置保存 helper，避免两个入口语义不同。

---

## 5. 数据结构建议

新增配置字段，可放入 profile-scoped settings 或单独 workspace metadata 文件。

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceUiMetadata {
    pub workspace_key: String,
    pub display_name: Option<String>,
    pub pinned: bool,
    pub hidden: bool,
    pub updated_at: i64,
}
```

推荐存储：

```
~/.allthecodes/web/workspaces.json
```

如果 profile 已有独立配置作用域，则使用 profile scoped 路径，避免不同 profile 的项目显示状态互相污染。

---

## 6. 实现位置

| 任务 | 建议文件 |
|------|----------|
| workspace API handler | `crates/allthecodes-web/src/handlers/workspaces.rs` |
| route 注册 | `crates/allthecodes-web/src/mod.rs` |
| response/request 类型 | `workspaces.rs` 内部，后续可迁移到 types crate |
| metadata 存储 helper | `crates/allthecodes-web/src/workspace_metadata.rs` 或 session/storage 相关模块 |
| session new body 扩展 | `crates/allthecodes-web/src/handlers/sessions.rs` |
| model default 持久化 | `crates/allthecodes-web/src/handlers/models.rs` 与 settings/admin helper |

---

## 7. Phase 计划

### Phase 1：只补真实后端能力，不改复杂工作流

1. 新增 workspace metadata 存取 helper。
2. 新增 `GET /api/workspaces`。
3. 新增 `PATCH /api/workspaces/{workspace_key}`。
4. 前端改用后端 metadata 替代本地 `pinnedProjectIds`, `renamedProjects`, `removedProjectIds`。

验收：

- 置顶、重命名、移除刷新后仍保留。
- `GET /api/sessions` 和 `GET /api/workspaces` 的 workspace key 对齐。

### Phase 2：系统资源管理器打开

1. 新增 `POST /api/workspaces/{workspace_key}/open`。
2. 实现 Linux/macOS/Windows runtime command 分支。
3. 加路径校验和错误返回。
4. 前端从 `window.open(file://...)` 改为调用 API。

验收：

- Linux 下可打开目录。
- 非已知 workspace path 被拒绝。

### Phase 3：指定项目新建 session

1. 扩展 `POST /api/sessions/new` request body。
2. 后端支持 `cwd`/`workspace_key`。
3. 前端项目行 `+` 传入对应 workspace。

验收：

- 点击非当前项目的 `+` 后，新 session 的 `cwd/workspace_root` 是该项目。
- streaming 或 TUI ownership 冲突仍返回现有冲突语义。

### Phase 4：批量归档项目对话

1. 新增 `POST /api/workspaces/{workspace_key}/sessions/archive`。
2. 后端一次性列出该 workspace session 并归档。
3. 返回 archived/skipped/failed 明细。
4. 前端替换循环归档逻辑。

验收：

- 项目下多个 inactive session 一次归档。
- active session 默认 skipped。
- 前端能显示失败或跳过数量。

### Phase 5：默认模型持久化校验

1. 写测试确认 `/api/models/default` 重启后仍生效。
2. 如果失败，接入 settings 持久化保存。

验收：

- 设置默认模型后重启后端，`GET /api/state` 和 `GET /api/models` 仍返回该模型。

---

## 8. 测试建议

后端：

```bash
cargo test -p allthecodes-web workspace
cargo test -p allthecodes-web sessions
cargo test -p allthecodes-web models
```

前端联调：

```bash
npm run typecheck
npm run build
npm run dev:check
```

手动检查：

- 左侧项目置顶/重命名/移除后刷新页面。
- 打开资源管理器按钮。
- 在非当前项目下新建 session。
- 批量归档项目对话。
- 默认模型设置后重启后端仍生效。

---

## 9. 风险与注意事项

- 不要删除真实 workspace 或 session 文件；“移除项目”只应隐藏。
- 打开资源管理器是本机副作用，必须校验路径。
- 按项目新建 session 会改变 active engine，需要沿用现有 streaming/TUI ownership 冲突保护。
- workspace metadata 必须与 session storage 的 `workspace_key` 算法一致。
- 前端当前已能展示菜单；后端适配应保持 API 向后兼容，避免破坏已提交前端。
