# 设置面板后端适配计划

> 前端设置面板执行计划→ 后端 `allthecodes` 适配需求分析
> 基于：`allthecodes-web/development-docs/UI/Settings/settings-execution-plan.md`（29 面板）
> 后端代码库：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/`

---

## 目录

1. [现状分析](#1-现状分析)
2. [后端适配总览](#2-后端适配总览)
3. [逐面板适配分析](#3-逐面板适配分析)
4. [API 变更方案](#4-api-变更方案)
5. [数据结构变更](#5-数据结构变更)
6. [依赖关系与实现顺序](#6-依赖关系与实现顺序)
7. [Phase 计划](#7-phase-计划)

---

## 1. 现状分析

### 1.1 当前后端 API 能力

```
POST /api/settings
  支持的 action: set_model, set_permission_mode, set_thinking, set_fast_mode, set_effort

GET /api/state
  返回: model, session_id, tools, permission_mode, thinking_enabled, fast_mode, effort, usage, commands

GET /api/capabilities
  返回: { chat, sessions, settings, debug, state, auth, profiles, models, providers, credentials: true
          gateways, usage, skills, memory, kanban, jobs, group_chat, files, logs, backend_services: false }
```

### 1.2 RawSettings 现有字段

配置文件 `~/.allthecodes/settings.json` 中已有的设置字段（与面板相关）：

```
model, backend, api_provider, theme, verbose, permission_mode, allowed_tools,
hooks, status_line, output_style, language, voice_enabled, editor_mode, view_mode,
thinking, default_model, fallback_model, fast_model, effort_level, fast_mode,
teammate_mode, auto_memory_enabled, system_prompt, api_key
```

### 1.3 缺口摘要

| 类别 | 前端口数 | 后端已实现 | 需新增 |
|------|---------|-----------|--------|
| 设置 API action | 29+ 面板 | 5 个 action | 大量扩展 |
| StateResponse 字段 | 10+ 面板需要 | 9 个字段 | 大幅扩展 |
| RawSettings 字段 | ~80 需要 | ~30 个 | ~50 个 |
| 独立 API 端点 | 8 面板需要 | 0 | 8+ 端点 |
| 后端数据模型 | 12 面板需要 | 有限 | 12+ 模型 |

---

## 2. 后端适配总览

### 2.1 三类适配工作

```
Type A: 扩展现有 POST /api/settings + GET /api/state
        新增 action 类型，扩展 StateResponse 字段
        影响: General, Chat, Projects, UI, Network, Speech, TTS, Web Search,
              Memory, Data, Token Savings

Type B: 新增专用 CRUD API 端点
        独立的数据模型 + RESTful CRUD
        影响: Agents(Profiles), People, Channels, Activity Recorder,
              Computer Use, Appshots, Chrome Relay, Hooks, Prompts(可选)

Type C: 利用已有 API 端点扩展
        现有端点功能完整，只需前端适配
        影响: Providers(已完整), Skills(已完整), Plugins/MCP(待规划),
              Usage(已完整), About(纯前端)
```

### 2.2 关键依赖路径

```
Phase 1 (基础架构)
  └─ 扩展 RawSettings ← 所有 Type A 面板依赖
  └─ 扩展 StateResponse ← Type A 面板依赖
  └─ 扩展 POST /api/settings handler ← Type A 面板依赖

Phase 2 (核心设置)
  └─ 新增 AgentProfile 数据模型 → /api/agents CRUD
  └─ 新增 PeopleProfile 数据模型 → /api/people CRUD
  └─ 新增 ChannelEndpointConfig 扩展 → /api/gateways 扩展
  └─ 新增 ActivityRecorderConfig → /api/activity-recorder CRUD

Phase 3 (扩展设置)
  └─ 新增 ComputerUseConfig → /api/computer-use CRUD
  └─ 新增 AppshotConfig → /api/appshots CRUD
  └─ 新增 ChromeRelayConfig → /api/chrome-relay CRUD
  └─ 新增 HookConfig 扩展 → 现有 hooks 字段增强
```

---

## 3. 逐面板适配分析

### 3.1 General（Type A）

**现状：** 仅有 `set_model` action 与此面板部分相关

**需要新增的设置 action：**

| 设置项 | action 名称 | 值类型 | 后端改动 |
|--------|------------|--------|---------|
| Tool Model | `set_model` | string | ✅ 已有 |
| Coding Agent | `set_coding_agent` | string | 新增 action，存 `backend` 字段 |
| Language | `set_language` | string | 新增 action，存 `language` 字段 |
| App Icon | `set_app_icon` | string | 新增 action，存 settings.extra.app_icon |
| Auto Start | `set_auto_start` | boolean | 新增 action，存 settings.extra.auto_start |
| Start Minimized | `set_start_minimized` | boolean | 新增 action，存 settings.extra.start_minimized |
| Minimize to Tray | `set_minimize_to_tray` | boolean | 新增 action，存 settings.extra.minimize_to_tray |
| Close to Tray | `set_close_to_tray` | boolean | 新增 action，存 settings.extra.close_to_tray |
| Quick Chat Settings | `set_quick_chat_X` | boolean | 新增 3 个 action |
| Auto Approve | `set_auto_approve` | boolean | 新增 action |
| Analytics | `set_analytics` | boolean | 新增 action |

**StateResponse 需要新增字段：**

```rust
pub struct StateResponse {
    // 已有字段保留...
    // 新增：
    pub language: Option<String>,
    pub app_icon: Option<String>,
    pub auto_start: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub auto_approve: Option<bool>,
    pub analytics_enabled: Option<bool>,
    // ...etc (建议用 settings 子对象承载)
}
```

**建议方案：** 将 `StateResponse` 扩展为包含 `settings: HashMap<String, Value>` 字段，避免为每个小设置加独立字段。前端按 key 存取。

#### 修改文件

| 文件 | 改动 |
|------|------|
| `crates/allthecodes-config/src/settings/raw.rs` | 新增字段到 `RawSettings` |
| `crates/allthecodes-config/src/settings/effective.rs` | 新增字段到 `EffectiveSettings` |
| `crates/allthecodes-web/src/handlers/chat.rs` | 扩展 `StateResponse` 结构体 |
| `crates/allthecodes-web/src/handlers/admin.rs` | 扩展 `settings_handler` action 匹配 |
| `crates/allthecodes-engine/src/types/app_state.rs` | 考虑 `AppState` 是否需扩展 |

---

### 3.2 Chat（Type A）

**现状：** 无对应 action

**需要新增的：**

| 设置项 | action 名称 | 值类型 | 说明 |
|--------|------------|--------|------|
| Default Model | `set_default_model` | string | 已有 `default_model` 字段 |
| System Prompt | `set_system_prompt` | string | 已有 `system_prompt` 字段 |
| Context Window | `set_context_window` | number | 新增 |
| Max Messages | `set_max_messages` | number | 新增 |
| Auto Title | `set_auto_title` | boolean | 新增 |

**RawSettings 需新增：** `context_window`, `max_messages`, `auto_title`

---

### 3.3 Projects（Type A）

**现状：** 无对应 action（`set_effort` 部分相关）

**需要新增的：**

| 设置项 | action 名称 | 值类型 |
|--------|------------|--------|
| Temperature | `set_temperature` | number (0-2) |
| Max Tokens | `set_max_tokens` | number |
| Streaming | `set_streaming` | boolean |
| Show Token Usage | `set_show_token_usage` | boolean |
| Markdown | `set_markdown` | boolean |
| Single Dollar Math | `set_single_dollar_math` | boolean |
| Infographic | `set_infographic` | boolean |
| Auto Collapse | `set_auto_collapse` | boolean |
| Quick Reply | `set_quick_reply` | boolean |
| Tool Selection | `set_default_tool_selection` | string |
| Skill Selection | `set_default_skill_selection` | string |
| Sound Effects | `set_sound_effects` | boolean |
| Auto Compact | `set_auto_compact` | boolean + threshold |
| Keep Recent | `set_keep_recent` | number |
| Hashline Mode | `set_hashline_mode` | boolean |

**RawSettings 需新增：** `temperature`, `streaming`, `show_token_usage`, `markdown_rendering`, 等

**注意：** `Effort level` 已有 `effort_level` + `set_effort` action

---

### 3.4 User Interface（Type A / 纯前端）

**现状：** 部分已有（theme 已存 settings.theme），但 settings API 不支持

**分析：** UI 设置多为前端专用（localStorage），少量需持久化到后端：

| 设置项 | 存储位置 | 建议 |
|--------|---------|------|
| Theme | frontend + backend | 同步到 settings.theme |
| Font/FontSize | 纯前端 | localStorage |
| Density | 纯前端 | localStorage |
| Sidebar | 纯前端 | ✅ 已有 `useUiStore` 持久化 |
| Editor Settings | 纯前端 | localStorage |
| Terminal Settings | 纯前端 | localStorage |
| Tool Card Expand | 纯前端 | localStorage |

**后端改动：** 可选，`set_theme` action（已有 `theme` 字段）

---

### 3.5 Color Scheme（纯前端）

**分析：** 主题选择是 UI 设置的扩展。主题列表可在前端硬编码或从后端获取。

**后端改动：** 无，纯前端实现。

---

### 3.6 Providers（Type C / 已有完整 API）

**现状：** 已有完整的 `/api/providers` CRUD + `/api/models` CRUD

| 已有端点 | 用途 |
|---------|------|
| GET/POST `/api/providers` | 列表/创建 |
| PATCH/DELETE `/api/providers/{id}` | 修改/删除 |
| POST `/api/providers/{id}/models/refresh` | 刷新模型列表 |
| GET `/api/models` | 模型注册表 |
| PATCH `/api/models/{id}` | 更新模型参数 |

**前端需要做：** 参考 `reference/providers.md` 中 Azure + Codex CLI 的表单设计适配现有 API 返回类型。

**后端改动：** 若需要支持 ACP Provider 等新类型，需扩展 `ProviderKind` 枚举。

---

### 3.7 Agents（Type B / 需新增 API）

**现状：** 无 agent profile 管理 API

**需要新增的：**

```
新增数据模型:
  AgentProfile {
    id: string
    name: string
    execution_mode: string         // generalist / coder / plan / operator
    available: bool
    preferred_model: Option<string>
    mission_summary: string
    focus_areas: Vec<string>
    can_delegate_to: Vec<string>
    specialist_prompt: string
    built_in: bool
    role: string                  // Designer / Developer / ProductManager / etc.
  }

新增 API 端点:
  GET    /api/agents              → 列表
  POST   /api/agents              → 创建
  PATCH  /api/agents/{id}         → 更新
  DELETE /api/agents/{id}         → 删除
  POST   /api/agents/{id}/restore  → 恢复内置默认
  POST   /api/agents/{id}/reorder  → 排序
```

**后端改动：**
- 新增 `AgentProfile` 数据模型（`crates/allthecodes-types/src/agents.rs`）
- 新增 `handlers/agents.rs`（axum handler）
- 注册路由到 `build_router()`
- 存储到 settings 或独立文件

---

### 3.8 Skills（Type C / 已有 API）

**现状：** 已有完整 `/api/skills` 接口

| 端点 | 用途 |
|-----|------|
| GET `/api/skills` | 技能列表 |
| GET `/api/skills/{id}` | 技能详情 |
| GET `/api/skills/{id}/files` | 技能文件 |
| PATCH `/api/skills/{id}` | 更新技能 |

**前端需要做：** 前端 `SkillsBrowser` 组件已映射这些接口。

**后端改动：** 无，或新增 `POST /api/skills/extract` 触发自动提取。

---

### 3.9 Memory（Type B / 需扩展 API）

**现状：** 已有 `GET /api/memory` + `PATCH /api/memory/{id}`，但功能有限

**需要扩展的：**

```
扩展 GET /api/memory 返回:
  {
    memories: Vec<MemoryEntry>,
    config: MemoryConfig {
      enabled: bool,
      auto_retrieve: bool,
      query_rewriting: bool,
      max_retrieved: number,
      similarity_threshold: number,
      auto_summarize: bool,
      // ...更多配置
    },
    stats: MemoryStats { total, auto_generated, manually_added }
  }

新增端点:
  POST /api/memory/sleep/run      → 触发夜间合并
  POST /api/memory/clear          → 清空所有
  POST /api/memory/export         → 导出快照

新增 action:
  set_memory_enabled
  set_auto_retrieve
  set_query_rewriting
  set_max_retrieved
  set_similarity_threshold
  set_auto_summarize
  set_nightly_consolidation
  set_sleep_time
  set_temp_ttl
  set_archive_retention
  set_memory_tool_model
  set_embedding_model
```

**后端改动方案（推荐）：** 
1. 将 MemoryConfig 存入 `RawSettings.extra.memory` 或独立字段
2. 扩展 `MemoryListResponse` 增加 `config` 和 `stats`
3. 新增专用 endpoint `/api/memory/config` 批量读写

---

### 3.10 Plugins（Type B / 需新增 API）

**现状：** 无插件管理 API

**需要新增的：**

```
新增端点:
  GET    /api/plugins              → 已安装列表
  GET    /api/plugins/marketplace  → 市场列表
  POST   /api/plugins/install      → 安装
  POST   /api/plugins/{id}/uninstall → 卸载
  POST   /api/plugins/{id}/disable → 禁用/启用
```

**后端改动：** 
- 需定义插件加载机制和 marketplace 协议
- 或可利用已有 MCP 扩展机制

---

### 3.11 MCP Servers（Type B / 需新增 API）

**现状：** 无 MCP Server 管理 API

**需要新增的：**

```
新增端点:
  GET    /api/mcp-servers              → 列表
  POST   /api/mcp-servers              → 添加
  PATCH  /api/mcp-servers/{id}         → 更新
  DELETE /api/mcp-servers/{id}         → 删除
  POST   /api/mcp-servers/{id}/restart → 重启
  GET    /api/mcp-servers/marketplace  → 市场

数据模型:
  McpServerConfig {
    id: string
    name: string
    transport: McpTransport  // Stdio | Sse
    command: Option<String>
    args: Option<Vec<String>>
    url: Option<String>
    enabled: bool
    status: ConnectionStatus
  }
```

---

### 3.12 Hooks（Type B / 需扩展）

**现状：** RawSettings 已有 `hooks: Option<HashMap<String, Value>>` 字段

**需要扩展的：**

```
扩展 POST /api/settings action:
  set_hook              → 设置单个 hook
  delete_hook           → 删除 hook
  add_hook_rule         → 添加规则
  set_hook_command      → 设置命令

或新增专用端点:
  GET    /api/hooks                 → 列表
  POST   /api/hooks                 → 创建
  PATCH  /api/hooks/{event}         → 更新
  DELETE /api/hooks/{event}         → 删除
  POST   /api/hooks/test            → 测试

数据模型:
  HookConfig {
    event: String,       // "chat.message.willSend"
    rules: Vec<HookRule>
  }
  HookRule {
    pattern: String,     // regex
    commands: Vec<String>
  }
```

---

### 3.13 Keybindings（纯前端）

**分析：** 快捷键绑定为纯前端功能，可存储 localStorage 或通过 `set_keybinding` action 同步到后端。

**后端改动：** 可选，新增 `set_keybinding` action 存储到 `settings.extra.keybindings`

---

### 3.14 People（Type B / 需新增 API）

**现状：** 无对应 API

**需要新增的：**

```
新增端点:
  GET    /api/people            → 列表
  POST   /api/people            → 创建
  PATCH  /api/people/{id}       → 更新
  DELETE /api/people/{id}       → 删除

数据模型:
  PersonProfile {
    id: string
    name: String,
    telegram_id: Option<String>,
    discord_id: Option<String>,
    discord_username: Option<String>,
    feishu_id: Option<String>,
    username: Option<String>,
    profile_content: String,  // markdown
  }
```

**存储建议：** 存放在 `~/.allthecodes/people/` 目录下，每人一个 JSON/MD 文件。

---

### 3.15 Channels（Type B / 需扩展现有 API）

**现状：** 已有 `/api/gateways`，但仅支持 Telegram 和 Lark

**需要扩展的：**

```
扩展 GatewayConfig 支持:
  Discord:  bot_token, allowed_servers, allowed_channels, mention_only
  WeChat:   QR code auth state

扩展已有端点:
  PATCH /api/gateways/{id}  ← 扩展支持更多 adapter 类型
  POST  /api/gateways/{id}/test  ← 发送测试消息

新增端点:
  POST /api/gateways/{id}/discord/setup  → Discord 设置引导
  GET  /api/gateways/wechat/qrcode       → 微信二维码
  POST /api/gateways/wechat/status       → 微信连接状态

数据模型扩展:
  GatewayConfig {
    // 已有: lark, telegram
    // 新增:
    discord: Option<DiscordConfig>,
    wechat: Option<WeChatConfig>,
    workspace_bindings: Vec<WorkspaceBinding>,
  }
```

**后端改动（推荐方案）：**
1. 扩展 `config.rs` 中的 `GatewayAdaptersConfig` 增加 discord 和 wechat
2. 新增 `adapters/discord.rs` 和 `adapters/wechat.rs`（或仅建 stub）
3. 扩展 gateway web handlers

---

### 3.16 Usage（Type C / 已有 API）

**现状：** 已有 `GET /api/usage?period=`，返回 `UsageDashboardResponse`

**前端需要做：** 适配 `UsageDashboard` 组件渲染图表

**后端改动：** 无，或扩展 `UsageDashboardResponse` 增加更多统计维度。

---

### 3.17 Speech（Type A / 需新增）

**分析：** 语音识别输入——Whisper 模型管理

**需要新增的：**

```
新增 action:
  set_speech_enabled          → boolean
  set_speech_model            → string (active model id)
  set_speech_language         → string
  post_download_model         → { model_id: string }

新增端点（推荐，因涉及文件下载）:
  GET    /api/speech/models           → 可用/已下载模型列表
  POST   /api/speech/models/download  → 触发下载
  DELETE /api/speech/models/{id}      → 删除已下载模型
  GET    /api/speech/models/{id}/progress → 下载进度

数据模型:
  WhisperModel {
    id: string,          // "large-v3-turbo"
    name: string,
    size_mb: number,
    description: string,
    downloaded: bool,
    download_progress: Option<number>,
    recommended: bool,
  }
```

**后端改动（可选方案）：** 若 Whisper 推理在后端进程，需管理模型文件。若在前端（浏览器/Electron），则纯前端实现。

---

### 3.18 Text-to-Speech（Type A / 需扩展）

**现状：** 无对应 action

**需要新增的：**

```
新增 action:
  set_tts_provider         → string ("elevenlabs" | ...)
  set_tts_api_key          → string (敏感字段，需加密存储)
  set_tts_voice            → string
  set_tts_voice_custom_id  → string (optional)
  set_tts_model            → string

新增端点（可选）:
  POST /api/tts/test       → 发送测试语音
  GET  /api/tts/voices     → 获取可用声音列表

RawSettings 需新增:
  tts_provider, tts_api_key, tts_voice, tts_voice_custom_id, tts_model
```

---

### 3.19 Web Search（Type A + B）

**分析：** 部分设置（选择搜索引擎）可走 Type A，部分（cookie 管理）需 Type B

```
新增 action:
  set_search_engine         → string ("google" | "xiaohongshu")

新增端点:
  POST /api/search/cookies/export  → 导出 cookie
  POST /api/search/cookies/import  → 导入 cookie
  POST /api/search/cookies/clear   → 清除 cookie
  POST /api/search/webfetch/login  → 打开浏览器登录
```

---

### 3.20 Network（Type A）

**现状：** 无对应 action

```
新增 action:
  set_proxy_enabled        → boolean
  set_proxy_url            → string
  set_prefer_ipv4          → boolean
  set_request_timeout      → number
  set_retry_attempts       → number
  set_user_agent           → string

RawSettings 需新增:
  proxy_enabled, proxy_url, prefer_ipv4, request_timeout,
  retry_attempts, custom_user_agent
```

---

### 3.21 Computer Use（Type B）

**分析：** macOS 桌面自动化辅助，涉及系统状态检查

```
新增端点:
  GET  /api/computer-use/status   → 检查 helper/permissions 状态
  POST /api/computer-use/permissions/accessibility → 触发 AX 权限弹窗
  POST /api/computer-use/permissions/screen-recording → 触发录屏权限
  POST /api/computer-use/test     → 测试配置

新增 action:
  set_computer_use_strict_mode     → boolean
  set_computer_use_action_log      → boolean
  set_computer_use_pip             → boolean
  set_computer_use_approved_app    → [app_id] (增删)
```

---

### 3.22 Appshots（Type A + B）

```
新增 action:
  set_appshot_hotkey         → string (key combo)
  set_appshot_destination    → string ("current-thread" | "new-thread")

新增端点:
  POST /api/appshots/capture → 触发抓取测试
  GET  /api/appshots/status  → 检查权限状态
```

---

### 3.23 Activity Recorder（Type B）

```
新增端点:
  GET  /api/activity-recorder/status    → 录制状态/统计
  POST /api/activity-recorder/clear     → 清空所有数据
  GET  /api/activity-recorder/sessions  → 最近会话列表

新增 action（大量）:
  set_snapshot_debounce, set_heartbeat_interval, set_idle_threshold,
  set_typing_pause, set_visual_change_poll, set_jpeg_quality,
  set_vision_ocr, set_keyboard_monitor, set_ocr_languages,
  set_ocr_interval, set_sensitive_apps, set_max_storage,
  set_output_directory
```

---

### 3.24 Chrome Relay（Type B）

```
新增端点:
  GET  /api/chrome-relay/status    → 连接/标签页状态
  POST /api/chrome-relay/launch    → 启动 Chrome + 扩展
  POST /api/chrome-relay/token/regenerate → 重新生成 token

新增 action:
  set_chrome_relay_auth_token      → string
```

---

### 3.25 Permissions（纯前端）

**分析：** macOS 系统权限信息页面，纯展示 + 引导按钮。

**后端改动：** 无。

---

### 3.26 Data（Type A + B）

```
新增 action:
  set_cloud_sync_enabled     → boolean
  set_cloud_sync_path        → string

新增端点:
  POST /api/data/export      → 导出数据
  POST /api/data/import      → 导入数据
  POST /api/data/sync/refresh → 同步刷新
```

---

### 3.27 Prompts（Type B）

```
新增端点:
  GET    /api/prompts         → 斜杠命令列表
  POST   /api/prompts         → 创建
  PATCH  /api/prompts/{id}    → 更新
  DELETE /api/prompts/{id}    → 删除

数据模型:
  QuickPrompt {
    id: string,
    name: string,          // 触发词（不含 /）
    content: string,       // 提示词内容
    description: Option<string>,
  }
```

---

### 3.28 About（纯前端 / Type C）

**分析：** 应用版本、更新检查、反馈链接

**后端改动：** 
- 可在 `GET /api/state` 中增加 `version` 和 `update_status` 字段
- 新增 `POST /api/check-update` 端点检查更新

---

### 3.29 Token Savings（Type A）

```
新增 action:
  set_token_savings_tracking → boolean

新增端点:
  GET /api/token-savings     → 节省统计
    返回: { total_saved_tokens, total_saved_cost, cache_hit_rate,
            by_model: [{ model, cache_reads, tokens_saved, cost_saved }] }
```

---

## 4. API 变更方案

### 4.1 方案 A：扩展 POST /api/settings

**推荐方案。** 保持 `/api/settings` 端点，但扩展 action 支持为任意 JSON 路径写入。

#### 输入格式

```json
// 当前格式
{ "action": "set_model", "value": "gpt-5.3-codex", "profile_id": null }

// 扩展格式 —— 支持路径式写入
{ "action": "set", "path": "general.language", "value": "zh-CN" }
{ "action": "set", "path": "network.proxy_enabled", "value": true }
{ "action": "set", "path": "speech.enabled", "value": true }

// 或保持简单，继续用命名 action:
{ "action": "set_language", "value": "zh-CN" }
```

#### 后端结构扩展

```rust
// 将所有设置组织到 settings.extra 子对象中
impl RawSettings {
    pub fn set_setting(&mut self, path: &str, value: Value) -> Result<()> {
        // 按路径写 settings.extra 中的嵌套字段
        // "speech.enabled" → extra["speech"]["enabled"]
    }
    
    pub fn get_setting(&self, path: &str) -> Option<&Value> {
        // 从 settings.extra 按路径读取
    }
}
```

**建议：** 保持现有命名 action（`set_model` 等），新增 `set_ext` action 处理所有扩展设置：

```rust
// admin.rs
"set_ext" => {
    let path = value.get("path").and_then(|v| v.as_str()).ok_or(...)?;
    let val = value.get("value").ok_or(...)?;
    settings.set_ext_setting(path, val);
}
```

**StateResponse 扩展：**

```rust
pub struct StateResponse {
    // 保留现有字段
    pub model: String,
    pub session_id: String,
    pub tools: Vec<String>,
    pub permission_mode: String,
    pub thinking_enabled: Option<bool>,
    pub fast_mode: bool,
    pub effort: Option<String>,
    pub usage: UsageResponse,
    pub commands: Vec<CommandInfo>,
    
    // 新增：所有扩展设置
    pub settings: HashMap<String, Value>,
    pub version: String,
}
```

### 4.2 方案 B：按域新增独立端点

**推荐作为方案 A 的补充。** 对有复杂 CRUD 需求的域，新增独立端点。

```
通用设置:      POST /api/settings (扩展现有)
Agent 配置:    GET/POST/PATCH/DELETE /api/agents
People 配置:   GET/POST/PATCH/DELETE /api/people
MCP Servers:   GET/POST/PATCH/DELETE /api/mcp-servers
Hooks 配置:    GET/POST/PATCH/DELETE /api/hooks
Slash Prompts: GET/POST/PATCH/DELETE /api/prompts
插件管理:      GET/POST/DELETE /api/plugins/...
Activity:      GET/POST /api/activity-recorder/...
Computer Use:  GET/POST /api/computer-use/...
Appshots:      GET/POST /api/appshots/...
Chrome Relay:  GET/POST /api/chrome-relay/...
```

---

## 5. 数据结构变更

### 5.1 RawSettings 扩展

```rust
pub struct RawSettings {
    // === 现有字段（保留） ===
    pub model: Option<String>,
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub theme: Option<String>,
    pub verbose: Option<bool>,
    pub permission_mode: Option<String>,
    pub hooks: Option<HashMap<String, Value>>,
    pub language: Option<String>,
    pub voice_enabled: Option<bool>,
    pub thinking: Option<Value>,
    pub default_model: Option<String>,
    pub effort_level: Option<String>,
    pub fast_mode: Option<bool>,
    pub auto_memory_enabled: Option<bool>,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
    
    // === 新增字段 ===
    // General
    pub coding_agent: Option<String>,           // "auto" | "claude-code" | "alma" | "acp"
    pub app_icon: Option<String>,               // "default" | "alt"
    pub auto_start: Option<bool>,
    pub start_minimized: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub quick_chat_hide_on_blur: Option<bool>,
    pub quick_chat_inject_screen: Option<bool>,
    pub quick_chat_ambient: Option<bool>,
    pub auto_approve_tools: Option<bool>,
    pub analytics_enabled: Option<bool>,
    
    // Chat
    pub context_window: Option<u64>,
    pub max_messages: Option<u64>,
    pub auto_title: Option<bool>,
    
    // Projects / Response Settings
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub streaming: Option<bool>,
    pub show_token_usage: Option<bool>,
    pub markdown_rendering: Option<bool>,
    pub single_dollar_math: Option<bool>,
    pub infographic: Option<bool>,
    pub auto_collapse_reasoning: Option<bool>,
    pub quick_reply_suggestions: Option<bool>,
    pub default_tool_selection: Option<String>,  // "auto" | "all" | "none"
    pub default_skill_selection: Option<String>,
    pub sound_effects: Option<bool>,
    pub auto_compact: Option<bool>,
    pub compact_threshold: Option<u8>,
    pub keep_recent_messages: Option<u8>,
    pub hashline_mode: Option<bool>,
    
    // Network
    pub proxy_enabled: Option<bool>,
    pub proxy_url: Option<String>,
    pub prefer_ipv4: Option<bool>,
    pub request_timeout: Option<u64>,
    pub retry_attempts: Option<u8>,
    pub custom_user_agent: Option<String>,
    
    // Speech
    pub speech_enabled: Option<bool>,
    pub speech_active_model: Option<String>,
    pub speech_language: Option<String>,
    
    // TTS
    pub tts_provider: Option<String>,
    pub tts_api_key: Option<String>,
    pub tts_voice: Option<String>,
    pub tts_voice_custom_id: Option<String>,
    pub tts_model: Option<String>,
    
    // Web Search
    pub search_engine: Option<String>,

    // Memory (deeper)
    pub memory_auto_retrieve: Option<bool>,
    pub memory_query_rewriting: Option<bool>,
    pub memory_max_retrieved: Option<u8>,
    pub memory_similarity_threshold: Option<u8>,
    pub memory_auto_summarize: Option<bool>,
    pub memory_nightly: Option<bool>,
    pub memory_sleep_time: Option<String>,
    pub memory_temp_ttl: Option<u32>,
    pub memory_archive_retention: Option<u32>,
    pub memory_tool_model: Option<String>,
    pub memory_embedding_model: Option<String>,
    
    // Computer Use
    pub computer_use_strict_mode: Option<bool>,
    pub computer_use_action_log: Option<bool>,
    pub computer_use_pip: Option<bool>,
    pub computer_use_approved_apps: Option<Vec<String>>,
    
    // Appshots
    pub appshot_hotkey: Option<String>,
    pub appshot_destination: Option<String>,
    
    // Chrome Relay
    pub chrome_relay_auth_token: Option<String>,
    
    // Data / Sync
    pub cloud_sync_enabled: Option<bool>,
    pub cloud_sync_path: Option<String>,
    
    // Token Savings
    pub token_savings_tracking: Option<bool>,
    
    // extra (已有，承载未映射字段)
    pub extra: HashMap<String, Value>,
}
```

### 5.2 新增独立数据模型文件

```
crates/allthecodes-types/src/
├── agents.rs          → AgentProfile, AgentCrewConfig
├── people.rs          → PersonProfile
├── mcp_servers.rs     → McpServerConfig
├── plugins.rs         → PluginInfo
├── prompts.rs         → QuickPrompt
├── activity.rs        → ActivityRecorderConfig
├── computer_use.rs    → ComputerUseConfig
├── appshots.rs        → AppshotConfig
├── chrome_relay.rs    → ChromeRelayConfig
├── speech_model.rs    → WhisperModel (if managed by backend)
├── token_savings.rs   → TokenSavingsStats
```

### 5.3 StateResponse 扩展（后台）

```rust
pub struct StateResponse {
    // ... 保留现有字段 ...

    // 新增：版本和构建信息
    pub version: String,             // "0.0.809"
    pub build_info: Option<String>,
    
    // 新增：UI 初始加载需要的所有设置（合并）
    pub settings_map: HashMap<String, Value>,
    
    // 新增：capability 快照（避免额外调用）
    pub capabilities: HashMap<String, bool>,
}
```

### 5.4 前端 TypeScript 类型扩展

对应后端的 `settings_map`，前端定义：

```typescript
interface UISettings {
  // General
  language?: string
  app_icon?: string
  auto_start?: boolean
  minimize_to_tray?: boolean
  close_to_tray?: boolean
  quick_chat_hide_on_blur?: boolean
  quick_chat_inject_screen?: boolean
  quick_chat_ambient?: boolean
  auto_approve_tools?: boolean
  analytics_enabled?: boolean
  
  // Chat
  context_window?: number
  max_messages?: number
  auto_title?: boolean
  
  // Projects
  temperature?: number
  max_tokens?: number
  streaming?: boolean
  show_token_usage?: boolean
  markdown_rendering?: boolean
  
  // Network
  proxy_enabled?: boolean
  proxy_url?: string
  request_timeout?: number
  retry_attempts?: number
  
  // Memory
  memory_auto_retrieve?: boolean
  memory_max_retrieved?: number
  memory_similarity_threshold?: number
  // ...更多
  
  [key: string]: unknown  // 允许扩展
}
```

---

## 6. 依赖关系与实现顺序

```
                    ┌──────────────────────────────┐
                    │  M0: RawSettings 扩展         │
                    │  + set_ext action             │
                    │  + StateResponse.settings_map │
                    └──────────┬───────────────────┘
                               │
             ┌─────────────────┼──────────────────┐
             ▼                 ▼                    ▼
    ┌────────────────┐ ┌────────────────┐ ┌──────────────────┐
    │ M1a: General   │ │ M1b: Chat      │ │ M1c: Network     │
    │ M1d: Projects  │ │ M1e: WebSearch │ │ M1f: TTS/Speech  │
    │ M1g: Memory    │ │ M1h: UI(前端)  │ │ M1i: About       │
    └────────────────┘ └────────────────┘ └──────────────────┘
                               │
             ┌─────────────────┼──────────────────┐
             ▼                 ▼                    ▼
    ┌────────────────┐ ┌────────────────┐ ┌──────────────────┐
    │ M2a: Agents    │ │ M2b: People    │ │ M2c: Hooks       │
    │ (新增 CRUD)    │ │ (新增 CRUD)    │ │ (扩展 hooks)     │
    └────────────────┘ └────────────────┘ └──────────────────┘
                               │
             ┌─────────────────┼──────────────────┐
             ▼                 ▼                    ▼
    ┌────────────────┐ ┌────────────────┐ ┌──────────────────┐
    │ M3a: Channels  │ │ M3b: MCP Srv  │ │ M3c: Plugins     │
    │ (扩展 gateway) │ │ (新增 CRUD)   │ │ (新增 API)       │
    └────────────────┘ └────────────────┘ └──────────────────┘
                               │
             ┌─────────────────┼──────────────────┐
             ▼                 ▼                    ▼
    ┌────────────────┐ ┌────────────────┐ ┌──────────────────┐
    │ M4a: Activity  │ │ M4b: CompUse  │ │ M4c: ChromeRl   │
    │     Recorder   │ │     Appshots  │ │     Prompts      │
    └────────────────┘ └────────────────┘ └──────────────────┘
```

---

## 7. Phase 计划

### Phase 0 — 基础架构（2-3 天）

**目标：** 扩展后端设置框架，使 20+ Type A 面板可并行开发

| 任务 | 文件 | 说明 |
|------|------|------|
| 扩展 RawSettings | `crates/allthecodes-config/src/settings/raw.rs` | 新增所有通用设置字段 |
| 扩展 EffectiveSettings | `crates/allthecodes-config/src/settings/effective.rs` | 映射新字段 |
| 扩展 SettingsJson | `crates/allthecodes-config/src/runtime_settings.rs` | 暴露新字段 |
| 实现 `set_ext` action | `crates/allthecodes-web/src/handlers/admin.rs` | 路径式设置写入 |
| 扩展 StateResponse | `crates/allthecodes-web/src/handlers/chat.rs` | 增加 `settings_map` + `version` |
| 前端类型扩展 | `allthecodes-web/src/lib/types.ts` | `UISettings` 接口 |
| 前端 settings store 扩展 | `allthecodes-web/src/store/settings-store.ts` | 解析 `settings_map` |

### Phase 1 — 核心设置（3-5 天）

**目标：** Type A 面板全部可用

| 面板 | 新 action 数 | 前端工作量 | 后端工作量 |
|------|-------------|-----------|-----------|
| General | 10+ | 中 | 小（利用 Phase 0） |
| Chat | 4 | 小 | 小 |
| Projects | 14 | 中 | 小 |
| UI | 0（纯前端） | 中 | 无 |
| Color Scheme | 0 | 小 | 无 |
| Network | 6 | 小 | 小 |
| Speech | 3 + 下载端点 | 中 | 中 |
| TTS | 5 | 小 | 小 |
| Web Search | 1 + cookie 端点 | 中 | 中 |
| Memory | 14 | 大 | 中（扩展已有 API） |
| About | 0 | 小 | 小（加 version 字段） |
| Token Savings | 1 | 中 | 中 |
| Data | 2 | 小 | 中 |

### Phase 2 — 数据模型设置（3-5 天）

**目标：** Type B 面板新增独立 CRUD

| 任务 | 新端点 | 后端新增文件 | 工作量 |
|------|--------|------------|--------|
| Agents CRUD | 5 | `types/agents.rs`, `handlers/agents.rs` | 大 |
| People CRUD | 4 | `types/people.rs`, `handlers/people.rs` | 中 |
| Hooks 扩展 | 5 | `handlers/hooks.rs` | 中 |
| Prompts CRUD | 4 | `types/prompts.rs`, `handlers/prompts.rs` | 中 |
| Keybindings | 0（前端） | - | 小 |

### Phase 3 — 扩展集成设置（3-5 天）

**目标：** 系统级和第三方集成面板

| 任务 | 新端点 | 工作量 |
|------|--------|--------|
| Channels 扩展 | 扩展现有 gateway API | 大 |
| MCP Servers CRUD | 4 | 中 |
| Plugins API | 4 | 大 |
| Computer Use | 4 | 中 |
| Appshots | 3 | 小 |
| Activity Recorder | 3 + 多个 action | 大 |
| Chrome Relay | 3 | 小 |
| Permissions | 0（前端） | 小 |

---

## 附录 A：前后端文件映射

```
Panel                    → Frontend Component              → Backend Handler
──────────────────────────────────────────────────────────────────────────────
General                  → GeneralPanel                    → admin.rs (扩展)
Chat                     → ChatPanel                       → admin.rs (扩展)
Projects                 → ProjectsPanel                   → admin.rs (扩展)
User Interface           → UiPanel                         → (纯前端)
Color Scheme             → ColorSchemePanel                → (纯前端)
Providers                → ProvidersPanel                  → providers.rs ✅
Agents                   → AgentsPanel                     → agents.rs (新增)
Skills                   → SkillsPanel                     → skills.rs ✅
Memory                   → MemoryPanel                     → memory.rs (扩展)
Plugins                  → PluginsPanel                    → plugins.rs (新增)
MCP Servers              → McpServersPanel                 → mcp_servers.rs (新增)
Hooks                    → HooksPanel                      → hooks.rs (新增)
Keybindings              → KeybindingsPanel                → (纯前端)
People                   → PeoplePanel                     → people.rs (新增)
Channels                 → ChannelsPanel                   → gateways.rs (扩展)
Usage                    → UsagePanel                      → usage.rs ✅
Speech                   → SpeechPanel                     → admin.rs + speech models
Text-to-Speech           → TtsPanel                        → admin.rs
Web Search               → WebSearchPanel                  → admin.rs + search cookies
Network                  → NetworkPanel                    → admin.rs
Computer Use             → ComputerUsePanel                → computer_use.rs (新增)
Appshots                 → AppshotsPanel                   → appshots.rs (新增)
Activity Recorder        → ActivityRecorderPanel           → activity_recorder.rs (新增)
Chrome Relay             → ChromeRelayPanel                → chrome_relay.rs (新增)
Permissions              → PermissionsPanel                → (纯前端)
Data                     → DataPanel                       → admin.rs + data endpoints
Prompts                  → PromptsPanel                    → prompts.rs (新增)
About                    → AboutPanel                      → (version 字段)
Token Savings            → TokenSavingsPanel               → token_savings.rs (新增)
```

---

## 附录 B：路由注册模板

在 `crates/allthecodes-web/src/mod.rs` 中新增路由：

```rust
// === Phase 1: 已有扩展（无需新路由）===
// GET /api/state ← 已存在，扩展 StateResponse
// POST /api/settings ← 已存在，扩展 action

// === Phase 2-3: 新路由 ===
.route("/api/agents", get(agents_list_handler).post(agents_create_handler))
.route("/api/agents/{id}", get(agents_detail_handler)
    .patch(agents_update_handler)
    .delete(agents_delete_handler))
.route("/api/agents/{id}/restore", post(agents_restore_handler))

.route("/api/people", get(people_list_handler).post(people_create_handler))
.route("/api/people/{id}", get(people_detail_handler)
    .patch(people_update_handler)
    .delete(people_delete_handler))

.route("/api/hooks", get(hooks_list_handler).post(hooks_create_handler))
.route("/api/hooks/{event}", patch(hooks_update_handler).delete(hooks_delete_handler))
.route("/api/hooks/test", post(hooks_test_handler))

.route("/api/prompts", get(prompts_list_handler).post(prompts_create_handler))
.route("/api/prompts/{id}", patch(prompts_update_handler).delete(prompts_delete_handler))

.route("/api/mcp-servers", get(mcp_servers_list_handler).post(mcp_servers_create_handler))
.route("/api/mcp-servers/{id}", patch(mcp_servers_update_handler)
    .delete(mcp_servers_delete_handler))
.route("/api/mcp-servers/marketplace", get(mcp_servers_marketplace_handler))

.route("/api/plugins", get(plugins_list_handler))
.route("/api/plugins/marketplace", get(plugins_marketplace_handler))
.route("/api/plugins/install", post(plugins_install_handler))
.route("/api/plugins/{id}/uninstall", post(plugins_uninstall_handler))

.route("/api/speech/models", get(speech_models_handler))
.route("/api/speech/models/download", post(speech_model_download_handler))

.route("/api/activity-recorder/status", get(activity_status_handler))
.route("/api/activity-recorder/clear", post(activity_clear_handler))
.route("/api/activity-recorder/sessions", get(activity_sessions_handler))

.route("/api/computer-use/status", get(computer_use_status_handler))
.route("/api/appshots/capture", post(appshots_capture_handler))
.route("/api/chrome-relay/status", get(chrome_relay_status_handler))
.route("/api/chrome-relay/token/regenerate", post(chrome_relay_token_handler))

.route("/api/data/export", post(data_export_handler))
.route("/api/data/import", post(data_import_handler))
```

---

> **总结：** 29 个设置面板中，约 12 个可直接用现有或略扩展的 API（Type A + C），
> 其余 17 个需新增独立端点和数据模型（Type B）。
> Phase 0 基础设置框架是最大杠杆——Rust struct 扩展 + `set_ext` action + `settings_map` response，
> 完成后后续 20+ 面板的前端开发可并行推进。
