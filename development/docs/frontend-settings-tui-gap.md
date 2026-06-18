# 前端 Settings 与 Rust TUI 生效差距记录

> 记录日期：2026-06-14
> 范围：`allthecodes-web` 前端设置面板、`allthecodes` 后端 `/api/settings` 持久化、Rust TUI 运行时消费路径
> 结论口径：只有被 Rust TUI 启动流程、命令同步、请求构建或工具运行时读取并产生行为变化的字段，才算“已应用到 TUI”。仅写入 `settings.json`、进入 `AppState.settings` 或返回给 Web `settings_map` 不算已应用。

---

## 1. 关键代码入口

| 方向 | 文件 | 作用 |
|---|---|---|
| 前端设置读取/提交 | `allthecodes-web/src/components/settings/**` | 各设置面板调用 `update(action, value)` |
| REST 设置入口 | `crates/allthecodes-web/src/handlers/admin.rs` | `/api/settings` action 映射、校验、持久化 |
| settings runtime projection | `crates/allthecodes-config/src/runtime_settings.rs` | `SettingsJson` 和 Web `settings_map()` |
| on-disk settings shape | `crates/allthecodes-config/src/settings/raw.rs` | `RawSettings` 一等字段和 extra passthrough |
| TUI 启动同步 | `crates/allthecodes/src/ui/tui.rs` | TUI 从 `AppState.settings` 初始化少量 UI/voice/status 字段 |
| TUI 命令后同步 | `crates/allthecodes/src/ui/tui/commands.rs` | `/config`、`/model` 等命令执行后同步到 `App` |
| 系统提示构建 | `crates/allthecodes-engine/src/lifecycle/submit_message/system_prompt_build.rs` | TUI/engine 提交时实际读取 `language`、`output_style`、`auto_memory_enabled` |
| 模型请求准备 | `crates/allthecodes-engine/src/query/turn_context.rs` | 实际进入模型调用的 settings 字段 |

---

## 2. 已应用到 Rust TUI 的设置

这些字段已经会影响 Rust TUI 或 TUI 驱动的 engine 行为：

| 设置 | 来源/前端 action | TUI 生效方式 |
|---|---|---|
| `model` | `set_model` | 更新主循环模型、模型选择、请求模型 |
| `permission_mode` / `permissions` | `set_permission_mode`、权限配置 | 工具权限模式、权限对话、sandbox/auto/bypass/plan 逻辑 |
| `sandbox` | `/config` / sandbox 命令 | Bash/PowerShell 等工具执行安全策略 |
| `thinking` | `set_thinking` | 请求构建中的 extended thinking 开关 |
| `effort_level` / `model_reasoning_effort` / `output_config.effort` | `set_effort` 等 | 请求 effort、TUI config surface 显示 |
| `fast_mode` | `set_fast_mode` | runtime fast mode 状态和状态栏/config surface |
| `theme` | `set_theme` | `App::set_theme_setting()` 更新 Rust TUI 主题 |
| `language` | `set_language` | 系统提示语言、voice STT language fallback |
| `output_style` | `/config` | 系统提示输出风格和 status line payload |
| `editor_mode` | `/config` | TUI Vim input 模式 |
| `voice_enabled` | `/voice` / config | TUI voice UI 状态；当前 backend 仍是 null voice controller |
| `status_line` | `/statusline` / config | TUI scriptable status line |
| `spinner_tips` | config | `/config show` 和配置 surface；实际 TUI spinner 消费需另查 |
| `keybindings` | Keybindings API/config | TUI keybinding registry |
| `auto_memory_enabled` | Memory `enabled` / `/memory auto` | 提交请求时是否注入 memory context |
| `web_search_provider`、`web_search_tavily_api_key`、`web_search_brave_api_key` | Network & Web 面板 | WebSearch 工具 provider/API key 选择 |

注意：`context_window` 不是直接使用前端 Chat Settings 的 `context_window` 字段。当前 `/context` 等路径主要通过 `model_capabilities[model].context_window` 或模型名推断窗口大小。

---

## 3. 前端已有但未应用到 Rust TUI 的设置

### 3.1 Chat Settings

这些字段可由前端写入后端，但没有进入 Rust TUI 主请求路径或渲染行为：

| 设置 | 前端 action | 当前状态 |
|---|---|---|
| `system_prompt` | `set_system_prompt` | 仅保存到 `AppState.settings.system_prompt`；提交请求仍使用 CLI `custom_system_prompt` / `append_system_prompt` |
| `context_window` | `set_context_window` | 未用于 TUI context window 计算 |
| `max_messages` | `set_max_messages` | 未用于 TUI 会话截断或请求构建 |
| `auto_title` | `set_auto_title` | TUI 无标题生成消费点 |
| `temperature` | `set_temperature` | 未传入模型调用参数 |
| `max_tokens` | `set_max_tokens` | 未传入 TUI 主模型调用；请求使用其他 max output token override 路径 |
| `streaming` | `set_streaming` | TUI streaming 是 engine/TUI 固有流程，不读取该设置 |
| `show_token_usage` | `set_show_token_usage` | Rust TUI 不读取该 Web 显示偏好 |
| `markdown_rendering` | `set_markdown_rendering` | TUI markdown 渲染不读取该开关 |
| `single_dollar_math` | `set_single_dollar_math` | TUI markdown/math 渲染不读取该开关 |
| `infographic` | `set_infographic_visualization` | TUI 无对应渲染路径 |
| `auto_collapse_reasoning` | `set_auto_collapse_reasoning` | TUI reasoning 展示不读取该开关 |
| `quick_reply_suggestions` | `set_quick_reply` | TUI 无 quick reply UI |
| `default_tool_selection` | `set_default_tool_selection` | TUI 请求工具选择不读取该默认值 |
| `default_skill_selection` | `set_default_skill_selection` | TUI skill 选择不读取该默认值 |
| `sound_effects` | `set_sound_effects` | TUI 无声音效果消费 |
| `auto_compact` | `set_auto_compact` | TUI/engine autocompact 当前使用独立 pipeline 触发，不读取该设置 |
| `compact_threshold` | `set_compact_threshold` | 未接入 autocompact 阈值 |
| `keep_recent_messages` | `set_keep_recent_messages` | 未接入 autocompact 保留窗口 |
| `hashline_mode` | `set_hashline_mode` | 未接入 TUI 编辑/文件改动模式 |

### 3.2 Appearance

`theme` 已应用。以下字段主要是 Web UI 或 Web terminal 偏好；在 `RawSettings` 中也不是一等字段，而是通过 extra passthrough 保存，Rust TUI 不读取：

- `font_family`
- `font_size`
- `density`
- `sidebar_width`
- `sidebar_mode`
- `line_numbers`
- `word_wrap`
- `minimap`
- `use_system_caret`
- `tool_card_expand`
- `terminal_font`
- `terminal_font_size`
- `persist_terminals`
- `blink_cursor`

### 3.3 General / Desktop

这些字段会持久化到 settings，但 Rust TUI 不读取：

- `app_icon`
- `auto_start`
- `start_minimized`
- `minimize_to_tray`
- `close_to_tray`
- `quick_chat_hide_on_blur`
- `quick_chat_inject_screen`
- `quick_chat_ambient`
- `analytics_enabled`

`auto_approve_tools` 也属于未接入项：虽然前端 General 面板可写入，但目前没有看到 TUI permission callback 或 permission context 使用它自动批准工具请求。

### 3.4 Network & Web

已应用：

- `web_search_provider`
- `web_search_tavily_api_key`
- `web_search_brave_api_key`

未应用到 Rust TUI / WebFetch：

- `proxy_enabled`
- `proxy_url`
- `prefer_ipv4`
- `request_timeout`
- `retry_attempts`
- `custom_user_agent`
- `search_engine`

当前 WebFetch 代理路径读取环境变量代理设置，而不是这些 settings 字段。

### 3.5 Voice

Rust TUI 当前读取的是 `voice_enabled` 和通用 `language`；Voice 面板写入的下列字段未应用到 TUI voice backend：

- `speech_enabled`
- `speech_active_model`
- `speech_language`
- `tts_provider`
- `tts_api_key`
- `tts_voice`
- `tts_voice_custom_id`
- `tts_model`

当前 TUI 初始化 voice controller 时仍使用 null audio/STT backend，因此这些设置即使持久化也不会改变 TUI 语音行为。

### 3.6 Memory

已应用：

- `enabled` / `auto_memory_enabled`：控制 TUI 提交时是否注入 memory context。

未应用或仅配置态：

- `auto_retrieve`
- `query_rewriting`
- `max_retrieved`
- `similarity_threshold`
- `auto_summarize`
- `nightly` / `sleep_enabled`
- `sleep_time`
- `temp_ttl_days`
- `archive_retention_days`
- `memory_tool_model`
- `embedding_model`

当前注入路径中 `format_memory_context_for_workspace_excluding_session(5, ...)` 的数量是硬编码 `5`，没有使用 `memory_max_retrieved`。

### 3.7 Data / Token Savings

这些设置未应用到 Rust TUI：

- `cloud_sync_enabled`
- `cloud_sync_path`
- `token_savings_tracking`

---

## 4. 主要差距总结

1. `/api/settings` 的 action 映射已经覆盖大量前端设置，但这只解决“保存”和“Web 回显”，不代表 TUI 生效。
2. `SettingsJson` 已包含很多字段，但 Rust TUI 只在 `tui.rs` / `tui/commands.rs` 中显式同步了少数字段到 `App`。
3. 模型请求路径实际读取的 settings 很少，主要是 model、thinking、effort/output_config、model capabilities、advisor model、language/output_style、auto memory。
4. 前端 Appearance 和 Desktop 类设置大多是 Web/桌面 shell 语义，不应默认要求 Rust TUI 消费。
5. 最值得优先补齐的“看起来应该影响 TUI 但目前没有”的字段是：
   - `system_prompt`
   - `max_tokens`
   - `temperature`
   - `auto_compact` / `compact_threshold` / `keep_recent_messages`
   - `auto_approve_tools`
   - Memory retrieval 相关字段
   - Network proxy/request timeout/user agent 字段

---

## 5. 建议后续处理

按语义拆分，而不是把所有 Web 设置强行接入 TUI：

1. **请求语义类**：`system_prompt`、`temperature`、`max_tokens` 应进入 `QueryEngineConfig` 或 submit-time request settings。
2. **压缩策略类**：`auto_compact`、`compact_threshold`、`keep_recent_messages` 应进入 autocompact pipeline config。
3. **权限策略类**：决定是否支持 `auto_approve_tools`，若支持应接入 TUI permission callback，并明确安全提示。
4. **记忆检索类**：把 `memory_max_retrieved`、`memory_auto_retrieve`、`memory_query_rewriting` 等接入 memory context resolver。
5. **网络工具类**：将 proxy/timeout/user-agent settings 下沉到 WebFetch/WebSearch client builder。
6. **纯 Web UI 类**：Appearance、desktop tray、quick chat 等保持 Web/桌面 shell 专用，不列为 Rust TUI 缺口。
