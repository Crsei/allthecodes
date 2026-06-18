# API 数据源分析与 `.allthecodes/` 缓存适配报告

> **日期:** 2026-06-10
> **目的:** 全面分析 allthecodes 后端所有 API 端点的数据来源，识别哪些端点可以从 `.allthecodes/` 目录中的文件直接读取，哪些需要动态计算或外部调用。

---

## 1. 总览

### 1.1 数据根目录

`data_root()` 解析优先级：
1. `$ALLTHECODES_HOME` 环境变量
2. `$CC_RUST_HOME`（已弃用）
3. `$HOME/.allthecodes`（默认）
4. 临时目录（最后兜底）

对应的**项目级数据**存储在 `{cwd}/.allthecodes/`。

### 1.2 `.allthecodes/` 完整文件布局

| 路径 | 格式 | 用途 |
|------|------|------|
| `settings.json` | JSON | 全局设置（用户级），项目级在 `{cwd}/.allthecodes/settings.json` |
| `credentials.json` | JSON | 凭据 |
| `sessions/{uuid}.json` | JSON | 会话数据 |
| `transcripts/{uuid}.ndjson` | NDJSON | 消息日志 |
| `memory/entries.json` | JSON | 记忆条目 |
| `people/*.json` | JSON（每人一个文件） | 人物定义 |
| `plugins/` | 目录 | 插件包 |
| `skills/` | 目录 | 技能包 |
| `web/state.db` | SQLite | UI 状态（偏好、主题、提示词、布局） |
| `web/workspaces.json` | JSON | 工作区元数据 |
| `web/kanban.json` | JSON | 看板数据 |
| `web/jobs.json` | JSON | 作业定义 |
| `web/job-runs.jsonl` | JSONL | 作业运行记录 |
| `web/group-chat.json` | JSON | 群聊房间数据 |
| `web/backend-services.json` | JSON | 后端服务状态 |
| `web/skills.json` | JSON | 技能 UI 元数据（启用/置顶） |
| `quick-prompts.json` | JSON | 快速提示词 |
| `search-cookies.json` | JSON | 搜索 cookies |
| `logs/YYYY/MM/YYYY-MM-DD.md` | Markdown | 守护进程日志 |
| `gateway/runs/{run_id}/` | JSON/NDJSON | Gateway 运行数据 |
| `daemon/` | JSON | 守护进程状态 |
| `launchpad-snapshots/{id}.json` | JSON | Launchpad 快照 |

---

## 2. 端点分类矩阵

### 图例

| 分类 | 含义 | 可直接从文件读取？ |
|------|------|:---:|
| ✅ **文件直接支持** | 响应直接来自单个 JSON/MD 文件，返回内容 ≈ 文件内容 | **是** |
| 🟡 **文件辅助** | 来自文件但需要后处理（合并、排序、过滤） | 部分可以 |
| 🔴 **动态/引擎状态** | 来自引擎运行时状态、内存、编译内置 | **否** |
| 🔵 **外部 API** | 向外发出 HTTP/IPC 调用 | **否** |
| ⚪ **未实现** | 返回 501 | N/A |

### 端点总表

| 端点 | 方法 | 处理器文件 | 数据源 | 分类 | 从 `.allthecodes/` 直接读取的建议 |
|------|------|-----------|--------|:----:|------|
| **核心系统** |
| `/api/state` | GET | `chat.rs` | Engine 运行时状态：当前模型、会话 ID、工具列表、usage、settings_map、system_prompt | 🔴 | 不可缓存。状态随每次推理变化 |
| `/api/capabilities` | GET | `capabilities.rs` | 编译内置的 `HashMap<String, bool>`，全为 `true` | 🔴 | 无对应文件。可在前端硬编码 |
| `/api/healthz` | GET | `health.rs` | WebUiStore SQLite 健康检查 | 🔴 | 无需缓存 |
| `/api/-/routes` | GET | `handler_registry.rs` | 运行时注册表 | 🔴 | 调试用 |
| **会话** |
| `/api/sessions` | GET | `sessions.rs` | `{data_root}/sessions/{uuid}.json` | 🟡 | 需扫描会话目录，合并元数据 |
| `/api/sessions/{id}` | GET | `sessions.rs` | `{data_root}/sessions/{id}.json` | ✅ | 可直接读取 `sessions/{id}.json` |
| **聊天/推理** |
| `/api/chat` | POST | `chat.rs` | Engine submit_message（AI 推理） | 🔵 | N/A — 写操作 |
| `/api/abort` | POST | `chat.rs` | Engine abort | 🔵 | N/A — 写操作 |
| `/api/system-prompt` | GET | `chat.rs` | Engine 系统提示词（动态构建） | 🔴 | 不可缓存 |
| `/api/coding-agents/status` | GET | `chat.rs` | Engine 代理状态 | 🔴 | 引擎运行时状态 |
| **代理** |
| `/api/agents` | GET | `agents.rs` | `{data_root}/agents/*.md` + `{cwd}/.allthecodes/agents/*.md` + 内置 + 插件多源合并 | 🟡 | 需要跨目录合并和 source-rank 排序逻辑 |
| `/api/agents/{name}` | GET | `agents.rs` | 同上，按 name 过滤 | 🟡 | 同上 |
| **人物** |
| `/api/people` | GET | `people.rs` | `{data_root}/people/*.json` | ✅ | **可直接读取** `people/` 目录下所有 JSON 文件 |
| `/api/people/{id}` | GET | `people.rs` | `{data_root}/people/{id}.json` | ✅ | **可直接读取** `people/{id}.json` |
| **钩子** |
| `/api/hooks` | GET | `hooks.rs` | `{data_root}/settings.json` 的 `hooks` 字段 | ✅ | **可直接读取** `settings.json` 的 `hooks` 字段 |
| `/api/hooks/{event}` | GET | `hooks.rs` | 同上，按 event 过滤 | ✅ | 同上 |
| **快速提示词** |
| `/api/prompts` | GET | `prompts.rs` | `{data_root}/quick-prompts.json` | ✅ | **可直接读取** `quick-prompts.json` |
| `/api/prompts/{id}` | GET | `prompts.rs` | 同上，按 id 过滤 | ✅ | 同上 |
| **记忆** |
| `/api/memory` | GET | `memory.rs` | `{data_root}/memory/entries.json` | ✅ | **可直接读取** `memory/entries.json` |
| `/api/memory/{id}` | PATCH | `memory.rs` | 同上，按 id 更新 | ✅ | 同上 |
| `/api/memory/config` | GET | `memory.rs` | Engine 设置 map + 文件系统 memory 统计 | 🟡 | 混合数据 |
| **MCP 服务器** |
| `/api/mcp-servers` | GET | `mcp_servers.rs` | `{data_root}/settings.json` + `{cwd}/.allthecodes/settings.json` + 插件/IDE 作用域 | 🟡 | 需合并多源并按优先级排序 |
| `/api/mcp-servers/marketplace` | GET | `mcp_servers.rs` | 编译内置空列表 | 🔴 | 直接空列表 |
| `/api/mcp-servers/{name}` | GET | `mcp_servers.rs` | 同上，按 name 过滤 | 🟡 | 同上 |
| **插件** |
| `/api/plugins` | GET | `plugins.rs` | 插件安装目录 + 编译内置市场索引 | 🟡 | 插件清单来自目录 + 运行时诊断 |
| `/api/plugins/marketplace` | GET | `plugins.rs` | 编译内置全局市场索引 | 🔴 | 运行时可忽略 |
| **技能** |
| `/api/skills` | GET | `skills.rs` | `{data_root}/skills/` + `{cwd}/.allthecodes/skills/` + 内置 + 插件，合并 `web/skills.json` 元数据 | 🟡 | 多源合并 + 文件扫描 |
| `/api/skills/{id}` | GET | `skills.rs` | 同上，按 id 过滤 | 🟡 | 同上 |
| `/api/skills/{id}/files` | GET | `skills.rb` | 技能目录文件系统 | 🟡 | 需枚举目录 |
| **看板** |
| `/api/kanban/boards` | GET | `kanban.rs` | `{data_root}/web/kanban.json` | ✅ | **可直接读取** `web/kanban.json` |
| `/api/kanban/boards/{id}` | GET | `kanban.rs` | 同上，按 board id 过滤 | ✅ | 同上 |
| `/api/kanban/tasks` (POST) | POST | `kanban.rs` | 写入看板存储 | ✅ | 写操作 |
| **作业/Cron** |
| `/api/jobs` | GET | `jobs.rs` | `{data_root}/web/jobs.json` | ✅ | **可直接读取** `web/jobs.json` |
| `/api/jobs/{id}` | GET/PATCH/DELETE | `jobs.rs` | 同上，按 id 操作 | ✅ | 同上 |
| `/api/cron/history` | GET | `jobs.rs` | `{data_root}/web/job-runs.jsonl` | ✅ | **可直接读取** `web/job-runs.jsonl` |
| **群聊** |
| `/api/group-chat/rooms` | GET | `group_chat.rs` | `{data_root}/web/group-chat.json` | ✅ | **可直接读取** `web/group-chat.json` |
| `/api/group-chat/rooms/{id}` | GET | `group_chat.rs` | 同上，按 id 过滤 | ✅ | 同上 |
| **工作区** |
| `/api/workspaces` | GET | `workspaces.rs` | `{data_root}/web/workspaces.json` + 扫描 `{data_root}/sessions/` | 🟡 | metadata 可直接读，session 计数需扫描 |
| `/api/workspaces/{key}` | PATCH | `workspaces.rs` | 同上 | 🟡 | 写操作 |
| **个人资料** |
| `/api/profiles` | GET | `profiles.rs` | `{data_root}/settings.json` 的 `auth_profiles` 字段 | ✅ | **可直接读取** `settings.json` 的 `auth_profiles` |
| `/api/profiles/{id}` | GET | `profiles.rs` | 同上 | ✅ | 同上 |
| `/api/profiles/{id}/export` | GET | `profiles.rs` | 同上，构建导出包 | 🟡 | 同上 |
| **提供商** |
| `/api/providers` | GET | `providers.rs` | `{data_root}/settings.json` + 编译内置 provider presets | 🟡 | settings 部分可读取，presets 是编译内置 |
| **模型** |
| `/api/models` | GET | `models.rs` | settings + 引擎内存状态（当前模型选择） | 🔴 | 依赖于运行时的模型选择 |
| **凭据** |
| `/api/credentials` | GET | `credentials.rs` | Auth 子系统（keychain） | 🔵 | 不可从文件读取 |
| **渠道** |
| `/api/channels` | GET | `channels.rs` | 守护进程状态文件 + HTTP 到本地 gateway | 🔵 | 实时守护进程状态 |
| `/api/channels/capabilities` | GET | `channels.rs` | 同上 | 🔵 | 同上 |
| **Gateway** |
| `/api/gateway/status` | GET | `gateways.rs` | 守护进程状态文件（pid、健康 URL） | 🔵 | 实时守护进程状态 |
| `/api/gateways` | GET | `gateways.rs` | 硬编码 + 状态响应 | 🔴 | 硬编码返回一个 gateway |
| **后端服务** |
| `/api/backend-services` | GET | `backend_services.rs` | 大部分硬编码状态 + 文件系统备份列表 | 🟡 | 多数为合成状态 |
| **日志/诊断** |
| `/api/logs` | GET | `logs.rs` | `{data_root}/logs/YYYY/MM/` | ✅ | **可直接读取** log 目录 |
| `/api/diagnostics/snapshot` | GET | - | 引擎 + OS 状态 | 🔴 | 不可缓存 |
| `/api/diagnostics/traces` | GET | - | Engine tracing | 🔴 | 不可缓存 |
| **文件操作** |
| `/api/files/tree` | GET | `files.rs` | 文件系统（cwd） | 🟡 | 实时文件系统 |
| `/api/files/read` | GET | `files.rs` | 文件系统（cwd） | 🟡 | 实时文件读取 |
| `/api/files/stat` | GET | `files.rs` | 文件系统（cwd） | 🟡 | 实时文件状态 |
| `/api/files/write/download/upload/...` | PUT/POST | `files.rs` | 文件系统读写 | 🟡 | 文件操作 |
| **Web UI 状态（SQLite）** |
| `/api/web/preferences` | GET/PUT | `web_state_routes.rs` | SQLite: `{data_root}/web/state.db` | ✅ | **可直接读取** SQLite |
| `/api/web/themes` | GET/POST | `web_state_routes.rs` | 同上 | ✅ | 同上 |
| `/api/web/themes/{id}` | PUT/DELETE | `web_state_routes.rs` | 同上 | ✅ | 同上 |
| `/api/web/prompts` | GET/POST | `web_state_routes.rs` | 同上 | ✅ | 同上 |
| `/api/web/prompts/{id}` | PUT/DELETE | `web_state_routes.rs` | 同上 | ✅ | 同上 |
| `/api/web/layouts` | GET/POST | `web_state_routes.rs` | 同上 | ✅ | 同上 |
| `/api/web/layouts/{id}` | PUT/DELETE | `web_state_routes.rs` | 同上 | ✅ | 同上 |
| **聊天模式** |
| `/api/chat-modes` | GET | `chat_modes.rs` | `{data_root}/settings.json` + 内存中插件/技能/MCP 注册表 | 🟡 | 复杂多源合并 |
| `/api/chat-modes/resources` | GET | `chat_modes.rs` | 同上 | 🟡 | 同上 |
| **OAuth/认证** |
| `/api/auth/status` | GET | `auth.rs` | Auth token 状态 | 🔵 | 外部认证 |
| `/api/auth/login` | POST | `auth.rs` | Auth 服务器 | 🔵 | 外部认证 |
| `/api/auth/refresh` | POST | `auth.rs` | Auth 服务器 | 🔵 | 外部认证 |
| **用量** |
| `/api/token-savings` | GET | `settings_phase1.rs` | 内存中 usage 计数器 | 🔴 | 运行时累加数据 |
| `/api/usage` | GET | `usage.rs` | Engine usage | 🔴 | 运行时数据 |
| **未实现 (501)** |
| `/api/hooks/test` | POST | `hooks.rs` | N/A | ⚪ | 未实现 |
| `/api/computer-use/test` | POST | - | N/A | ⚪ | 未实现 |
| `/api/appshots/capture` | POST | - | N/A | ⚪ | 未实现 |
| `/api/chrome-relay/launch` | POST | - | N/A | ⚪ | 未实现 |

---

## 3. 可以直接从 `.allthecodes/` 文件读取的端点

以下端点的响应数据可以直接从文件系统读取，无需 API 调用。前端可以在本地读取这些文件来加速加载。

### 3.1 一级候选 — 零后处理，文件内容直出

| 端点 | 文件路径 | 注意事项 |
|------|---------|----------|
| `GET /api/prompts` | `{data_root}/quick-prompts.json` | 仅有按 name/id 排序 |
| `GET /api/memory` | `{data_root}/memory/entries.json` | 仅有按 updated_at 倒序排序 |
| `GET /api/kanban/boards` | `{data_root}/web/kanban.json` | 仅有列排序 + schema 版本迁移 |
| `GET /api/jobs` | `{data_root}/web/jobs.json` | 仅有按 profile_id 过滤 + 排序 |
| `GET /api/cron/history` | `{data_root}/web/job-runs.jsonl` | 仅有按 profile_id 过滤 + 排序 |
| `GET /api/group-chat/rooms` | `{data_root}/web/group-chat.json` | 仅有过滤已归档 + 排序 |
| `GET /api/people` | `{data_root}/people/*.json` | 每人一个 JSON，仅有按 name/id 排序 |
| `GET /api/people/{id}` | `{data_root}/people/{id}.json` | 直接读取单个文件 |
| `GET /api/sessions/{id}` | `{data_root}/sessions/{id}.json` | 直接读取单个文件 |
| `GET /api/logs` | `{data_root}/logs/YYYY/MM/` | 直接列出日志目录 |

### 3.2 二级候选 — 简单后处理（过滤/提取字段）

| 端点 | 文件路径 | 后处理 |
|------|---------|--------|
| `GET /api/profiles` | `{data_root}/settings.json` → `auth_profiles` | 从 settings.json 提取 `auth_profiles` map |
| `GET /api/hooks` | `{data_root}/settings.json` → `hooks` | 从 settings.json 提取 `hooks` 字段，标准化为 Array |
| `GET /api/workspaces` | `{data_root}/web/workspaces.json` + `{data_root}/sessions/` | metadata 可直接读，session 计数需扫描目录 |

### 3.3 三级候选 — 多源合并

| 端点 | 来源 | 合并逻辑 |
|------|------|---------|
| `GET /api/agents` | `{data_root}/agents/*.md` + `{cwd}/.allthecodes/agents/*.md` + 内置 + 插件 | 多源合并后按 source-rank 去重取最高 |
| `GET /api/mcp-servers` | `{data_root}/settings.json` + `{cwd}/.allthecodes/settings.json` | 多源 + 插件/IDE 作用域合并，反向优先级 |
| `GET /api/skills` | 多个 skill 目录 + `web/skills.json` | 目录扫描 + 元数据合并 |
| `GET /api/providers` | `settings.json` + 编译内置 presets | 用户定义 providers + 内置 presets 合并 |
| `GET /api/chat-modes` | `settings.json` + 内存插件/技能/MCP 注册表 | 内置 bundle + 用户自定义 + 资源索引 |

### 3.4 特殊：Web UI 状态（SQLite）

`/api/web/preferences`、`/api/web/themes`、`/api/web/prompts`、`/api/web/layouts` 这些端点目前写在 `{data_root}/web/state.db` SQLite 文件中。前端可以直接读取该 SQLite 数据库，但：
- 前端目前已有 `localStorage`（Zustand persist）作为本地缓存
- `/api/web/preferences` 在每次页面加载时都会调用 `hydrateFromRemote()`
- 建议：可以让前端在页面加载时直接从 `localStorage` 读取，仅当版本/时间戳不匹配时才调用 API

---

## 4. 不可从 `.allthecodes/` 文件读取的端点

### 4.1 依赖引擎运行时状态（🔴）

这些端点返回的数据依赖于引擎的内存状态、当前会话、当前模型选择等：

| 端点 | 原因 |
|------|------|
| `GET /api/state` | 包含当前模型、会话 ID、工具列表、usage、settings_map、system_prompt — 全部从引擎运行时获取 |
| `GET /api/capabilities` | 编译内置的静态 map，无对应文件 |
| `GET /api/system-prompt` | 引擎动态构建 |
| `GET /api/coding-agents/status` | 引擎运行时代理状态 |
| `GET /api/token-savings` | 内存中累加的 token 计数器 |
| `GET /api/usage` | 运行时 usage 数据 |
| `GET /api/models` | 合并 settings + 当前模型选择 |
| `GET /api/diagnostics/snapshot` | 引擎 + OS 状态快照 |
| `GET /api/plugins/marketplace` | 编译内置市场索引 |
| `GET /api/gateways` | 硬编码返回单个 gateway |
| `GET /api/backend-services` | 大部分为合成的状态信息 |

### 4.2 依赖外部 IPC/网络（🔵）

| 端点 | 原因 |
|------|------|
| `POST /api/chat` | AI 推理调用 |
| `GET /api/channels` | 需要向本地 gateway 发 HTTP 请求获取适配器状态 |
| `GET /api/channels/capabilities` | 同上 |
| `GET /api/gateway/status` | 守护进程状态文件（pid/health URL）实时变化 |
| `GET /api/credentials` | 读取 keychain/系统凭据 |
| `GET /api/auth/status` | OAuth token 状态 |
| `POST /api/auth/login` | OAuth 登录 |
| `POST /api/auth/refresh` | OAuth token 刷新 |

### 4.3 WebSocket 端点

| 端点 | 用途 |
|------|------|
| `/api/terminal/sessions/{id}/ws` | 终端会话 WebSocket |
| `/api/tui/ws` | TUI WebSocket |
| `/api/ipc/ws` | IPC 桥接 WebSocket（FrontendMessage/BackendMessage） |
| `/api/rpc/ws` | JSON-RPC WebSocket |

---

## 5. 前端当前加载模式

### 5.1 每次页面加载必调用的端点

| 端点 | 触发时机 | 调用者 |
|------|---------|--------|
| `GET /api/capabilities` | AppShell mount | AppShell.tsx `discoverCapabilities()` |
| `GET /api/profiles` | AppShell mount | AppShell.tsx `fetchProfiles()` |
| `GET /api/web/preferences` | AppShell mount | `hydrateFromRemote()` |
| `GET /api/state` | ChatWorkspace + ChatInput mount | `refreshSettings()` |
| `GET /api/sessions` | Sidebar mount | `fetchSessions()` |
| `GET /api/workspaces` | Sidebar mount | `refreshWorkspaces()` |

### 5.2 懒加载端点

| 端点 | 触发条件 |
|------|---------|
| `GET /api/providers` | 打开 Models/Providers 页面 |
| `GET /api/models` | 打开 Models/Providers 页面 |
| `GET /api/credentials` | 打开 Models/Providers 页面 |
| `GET /api/skills` | 打开 Skills 页面或 SkillsPopover |
| `GET /api/memory` | 打开 Memory 设置面板 |
| `GET /api/kanban/boards` | 打开 Kanban 页面 |
| `GET /api/jobs` | 打开 Jobs 页面 |
| `GET /api/group-chat/rooms` | 打开 Group Chat 页面 |
| `GET /api/gateway/status` | 打开 Gateways 页面 |
| `GET /api/usage` | 打开 Usage 页面 |
| `GET /api/backend-services` | 打开 Backend Services 页面 |
| `GET /api/logs` | 打开 Logs 页面 |
| `GET /api/chat-modes` | 打开 Chat Modes 设置 |

---

## 6. 缓存策略建议

### 6.1 高优先级 — 直接文件读取

以下端点的数据完全来自文件系统，前端可以直接读取 `.allthecodes/` 下的文件来替代 API 调用：

| 端点 | 文件路径 | 建议策略 |
|------|---------|---------|
| `/api/prompts` | `quick-prompts.json` | 前端直接读取文件 |
| `/api/memory` | `memory/entries.json` | 前端直接读取文件 + 写入时同步 |
| `/api/kanban/boards` | `web/kanban.json` | 前端直接读取文件 |
| `/api/jobs` | `web/jobs.json` | 前端直接读取文件 |
| `/api/group-chat/rooms` | `web/group-chat.json` | 前端直接读取文件 |
| `/api/people` | `people/*.json` | 前端直接读取目录 |
| `/api/hooks` | `settings.json` → `hooks` | 前端直接读取 settings.json |
| `/api/profiles` | `settings.json` → `auth_profiles` | 前端直接读取 settings.json |

### 6.2 中优先级 — 文件读取 + 轻量后处理

| 端点 | 建议策略 |
|------|---------|
| `/api/workspaces` | 读取 `web/workspaces.json`，session 计数可做懒加载或估算 |
| `/api/agents` | 读取 `{data_root}/agents/*.md` + `{cwd}/.allthecodes/agents/*.md`，按 source-rank 合并 |

### 6.3 低优先级 — 多源合并复杂

| 端点 | 原因 |
|------|------|
| `/api/chat-modes` | 合并 settings + 内存注册表 |
| `/api/mcp-servers` | 合并多 settings 文件 + 插件 |
| `/api/skills` | 多目录扫描 + 元数据合并 |
| `/api/providers` | 合并 settings + 编译内置 |

### 6.4 不可缓存

所有 🔴/🔵 分类的端点都不适合文件缓存。

### 6.5 特殊建议

1. **`/api/capabilities`**：该端点的响应完全静态（所有 capability = true）。建议在前端直接硬编码，完全跳过 API 调用
2. **`/api/web/preferences`**：前端已通过 Zustand persist 在 `localStorage` 缓存了 UI 偏好。建议在页面加载时优先使用 `localStorage`，仅在检测到版本/时间戳变更时再同步后端
3. **`/api/state`**：这是被调用最频繁的动态端点之一。虽然不可缓存，但可以通过减少不必要的重调用（如 profile 切换后的重复调用）来优化

---

## 7. 完整数据流图

```
┌──────────────────────────────────────────────────────────┐
│                    前端 (Next.js)                         │
│  ┌────────────────────────────────────────────────────┐  │
│  │ Zustand Stores (localStorage)                      │  │
│  │  - uiStore (preferences, theme, layout)            │  │
│  │  - sessionStore (sessions)                         │  │
│  │  - settingsStore (state, profiles, capabilities)   │  │
│  │  - ...                                             │  │
│  └────────────────────────────────────────────────────┘  │
│            │ API 调用           │ 直接文件读取 (未来)     │
└────────────┼───────────────────┼────────────────────────┘
             │                   │
┌────────────┴───────────────────┴────────────────────────┐
│                    后端 (Rust)                          │
│  ┌──────────────────┐   ┌───────────────────────────┐  │
│  │ Engine (内存)    │   │ File System (.allthecodes/)│  │
│  │  - app_state     │   │  settings.json            │  │
│  │  - session_state │   │  people/*.json            │  │
│  │  - usage         │   │  sessions/*.json          │  │
│  │  - model         │   │  memory/entries.json      │  │
│  │  - ...           │   │  web/kanban.json          │  │
│  └──────────────────┘   │  web/jobs.json            │  │
│         │               │  quick-prompts.json       │  │
│         ▼               │  ...                      │  │
│  ┌──────────────────────────────────────────────┐     │
│  │ Web Handlers (API 端点)                      │     │
│  │  - /api/state       ─── Engine               │     │
│  │  - /api/people      ─── File (people/*.json) │     │
│  │  - /api/kanban      ─── File (web/kanban.json)│    │
│  │  - /api/chat        ─── Engine (AI)          │     │
│  │  - /api/channels    ─── HTTP (gateway)       │     │
│  │  - ...                                       │     │
│  └──────────────────────────────────────────────┘     │
└───────────────────────────────────────────────────────┘
```

---

## 8. 总结

### 可以直接从文件读取（✅ 标记）

1. **`GET /api/prompts`** → `quick-prompts.json`
2. **`GET /api/memory`** → `memory/entries.json`
3. **`GET /api/kanban/boards`** → `web/kanban.json`
4. **`GET /api/jobs`** → `web/jobs.json`
5. **`GET /api/cron/history`** → `web/job-runs.jsonl`
6. **`GET /api/group-chat/rooms`** → `web/group-chat.json`
7. **`GET /api/people`** → `people/*.json`
8. **`GET /api/hooks`** → `settings.json` → `hooks`
9. **`GET /api/profiles`** → `settings.json` → `auth_profiles`
10. **`GET /api/sessions/{id}`** → `sessions/{id}.json`
11. **`GET /api/logs`** → `logs/YYYY/MM/`
12. **`GET /api/web/preferences`** → `web/state.db` (SQLite, 前端已有 localStorage)

### 不可从文件读取（必须 API 调用）

1. 所有 POST/PUT/PATCH/DELETE 写操作
2. AI 推理端点 (`/api/chat`, `/api/abort`)
3. 引擎运行时状态 (`/api/state`, `/api/system-prompt`, `/api/token-savings`, `/api/usage`)
4. 外部 IPC/网络端点 (`/api/channels`, `/api/gateway/*`, `/api/credentials`, `/api/auth/*`)
5. 实时诊断 (`/api/diagnostics/*`)
6. 文件操作 (`/api/files/*`, 实时文件系统)
7. 模型选择 (`/api/models`, 依赖引擎状态)
8. WebSocket 端点
