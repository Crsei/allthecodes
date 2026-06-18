# TUI 设置：有字段但未应用的状态

> 分析日期：2026-06-17
> 数据流路径：`RawSettings → EffectiveSettings → SettingsJson (AppState.settings) → TUI App 提取个别字段`

以下字段虽然经过 `full_init.rs` 存储到了 `SettingsJson`，但 **TUI 的渲染循环和逻辑层从未读取它们**。

> 2026-06-17 更新：`view_mode`、`spinner_tips`、`auto_compact`、`compact_threshold`、
> `keep_recent_messages` 已在 Rust TUI / compact runtime 中接线；`sound_effects`、
> `terminal_progress_bar_enabled`、`advisor_model`、`auto_memory_enabled`、
> `claude_in_chrome_default_enabled` 已确认有运行时消费点，不再列为未使用字段。

---

## 1. Electron 桌面端遗留字段（Rust TUI 完全不适用）

这些是 Electron 桌面应用（Claude Code 等）的 legacy 设置，Rust TUI 没有对应的实现概念。

| 字段 | 类型 | 定义位置 | 最后出现位置 |
|------|------|----------|-------------|
| `app_icon` | `Option<String>` | `runtime_settings.rs:55` | `full_init.rs:576` |
| `auto_start` | `Option<bool>` | `runtime_settings.rs:56` | `full_init.rs:577` |
| `start_minimized` | `Option<bool>` | `runtime_settings.rs:57` | `full_init.rs:578` |
| `minimize_to_tray` | `Option<bool>` | `runtime_settings.rs:58` | `full_init.rs:579` |
| `close_to_tray` | `Option<bool>` | `runtime_settings.rs:59` | `full_init.rs:580` |
| `quick_chat_hide_on_blur` | `Option<bool>` | `runtime_settings.rs:60` | `full_init.rs:581` |
| `quick_chat_inject_screen` | `Option<bool>` | `runtime_settings.rs:61` | `full_init.rs:582` |
| `quick_chat_ambient` | `Option<bool>` | `runtime_settings.rs:62` | `full_init.rs:583` |

## 2. Web UI 独占设置（TUI 从未引用）

`settings_map()` 中有输出，`admin.rs` 中有 handler，但 Rust TUI 的渲染层和功能层不检查它们。

| 字段 | 类型 | 定义位置 | 备注 |
|------|------|----------|------|
| `auto_title` | `Option<bool>` | `runtime_settings.rs:83` | 仅 web admin handler |
| `show_token_usage` | `Option<bool>` | `runtime_settings.rs:87` | 仅 web admin handler |
| `markdown_rendering` | `Option<bool>` | `runtime_settings.rs:88` | TUI rendering 模块不读此值 |
| `single_dollar_math` | `Option<bool>` | `runtime_settings.rs:89` | 同上 |
| `infographic` | `Option<bool>` | `runtime_settings.rs:90` | 同上 |
| `auto_collapse_reasoning` | `Option<bool>` | `runtime_settings.rs:91` | 同上 |
| `quick_reply_suggestions` | `Option<bool>` | `runtime_settings.rs:92` | 同上 |
| `default_tool_selection` | `Option<String>` | `runtime_settings.rs:93` | 仅 web admin handler |
| `default_skill_selection` | `Option<String>` | `runtime_settings.rs:94` | 仅 web admin handler |
| `hashline_mode` | `Option<bool>` | `runtime_settings.rs:99` | 仅 web admin handler |

## 3. Memory 调参字段——reserved，等待完整 memory runtime wiring

```rust
// raw.rs:130-133
/// Auto-memory toggle: when true, memories captured during a session
/// are surfaced by /memory and injected into the prompt via
/// build_memory_context_with. Default is None (off). The capture
/// hook itself is not yet wired up — only the state is persisted.
pub auto_memory_enabled: Option<bool>,
```

`auto_memory_enabled` 已由 `/memory` 与相关 UI 消费；以下调参字段仍是 reserved，
schema 与 config diagnostics 会明确提示当前无 runtime effect：

| 字段 | 类型 |
|------|------|
| `memory_auto_retrieve` | `Option<bool>` |
| `memory_query_rewriting` | `Option<bool>` |
| `memory_max_retrieved` | `Option<u8>` |
| `memory_similarity_threshold` | `Option<u8>` |
| `memory_auto_summarize` | `Option<bool>` |
| `memory_nightly` | `Option<bool>` |
| `memory_sleep_time` | `Option<String>` |
| `memory_temp_ttl` | `Option<u32>` |
| `memory_archive_retention` | `Option<u32>` |
| `memory_tool_model` | `Option<String>` |
| `memory_embedding_model` | `Option<String>` |

## 4. 其他零散只存不用的字段

| 字段 | 定义位置 | 说明 |
|------|----------|------|
| `auto_approve_tools` | `runtime_settings.rs:63` | 故意不接入权限系统；诊断提示改用 `permissionMode` / `permissions.defaultMode` |
| `analytics_enabled` | `runtime_settings.rs:64` | TUI 无分析跟踪 |
| `teammate_mode` | `runtime_settings.rs:110` | 无队友模式实现 |

---

## 优先级排序

| 优先级 | 字段组 | 原因 |
|--------|--------|------|
| 🟢 P2 | Electron 遗留 8 字段 | 不影响功能，但迷惑用户；可考虑标记 deprecation 或从 schema 移除 |
| 🔵 P3 | Memory 调参 11 字段 | reserved；等 memory capture/retrieval 方案完整后统一启用 |
| 🔵 P3 | Web 独占 12 字段 | 设计上就是 web-only，合理 |
| 🔵 P3 | `auto_approve_tools` / `analytics_enabled` / `teammate_mode` | 功能未实现、故意不接线或暂不适用 |

---

## 相关文件索引

- 设置定义（运行时投影）：`crates/allthecodes-config/src/runtime_settings.rs`
- 设置原始 schema：`crates/allthecodes-config/src/settings/raw.rs`
- 设置有效值：`crates/allthecodes-config/src/settings/effective.rs`
- 设置类型结构体：`crates/allthecodes-config/src/settings/types.rs`
- 启动时注入设置：`crates/allthecodes/src/full_init.rs`（555-654 行附近）
- TUI App 状态结构体：`crates/allthecodes/src/ui/app.rs`
- TUI 状态栏消费：`crates/allthecodes/src/ui/app/status.rs`
- Web admin handler：`crates/allthecodes-web/src/handlers/admin.rs`
