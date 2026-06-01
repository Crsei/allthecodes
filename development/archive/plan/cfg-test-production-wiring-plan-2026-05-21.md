# Rust TUI `#[cfg(test)]` 生产链路补齐计划

**日期**: 2026-05-21
**范围**: `crates/claude-code-rs/src/ui/` 下所有子目录的 `#[cfg(test)]` 守卫项审计与接入
**总计**: 16 个目录，约 150+ 项 `#[cfg(test)]`，其中 ~120 项需移除守卫，~30 项保留

---

## 总体统计

| 目录 | 需移除 cfg(test) | 需保留 cfg(test) | 修改生产文件数 |
|------|------------------|------------------|---------------|
| agents | ~25 项 | 0 | 7 |
| app | ~6 项 | 1 | 6 |
| command_palette | ~3 项 | ~10 项 | 3 |
| components | ~20 项 | ~5 项 | 8 |
| diff | ~12 项 | 0 | 2 |
| hooks | ~5 项 | 0 | 5 |
| input | ~9 项 | ~8 项 | 6 |
| mcp | ~20 项 | 0 | 4 |
| memory | ~3 项 | 0 | 3 |
| messages | ~9 项 | ~10 项 | 3 |
| notifications | ~6 项 | 0 | 3 |
| overlays | ~8 项 | 1 | 4 |
| permissions | ~15 项 | 0 | 8 |
| rendering | ~14 项 | ~6 项 | 4 |
| tasks | ~17 项 | 0 | 3 |
| teams | ~3 项 | 0 | 3 |
| **合计** | **~175 项** | **~41 项** | **~70 文件** |

**7 个目录无 `#[cfg(test)]` 生产接入项**: `command_surface`, `helpers`, `lsp_recommendation`, `platform`, `runtime`, `skills`, `theme`

---

## 需要修改的生产入口文件

以下 7 个生产入口/路由文件是主要修改目标：

### 1. `tui.rs` — 主循环入口
- 初始化时调用 `detect_backend()` 检测终端通知能力
- Ctrl+C 事件处理中集成 `ExitGuard::arm()` / `confirm()` 双击退出状态机
- 后端断开时发送 `AppEvent::Shutdown`
- `CommandPalette.handle_key()` 返回 `CommandAction::Execute` 时提交 prompt

### 2. `app.rs` — App 核心
- `AppEvent::Tick` / `AppEvent::Shutdown` 在 `handle_app_event()` 中的匹配分支
- `CommandPalette` 返回 `CommandAction::Execute` 的分支处理

### 3. `app/input.rs` — 键盘/鼠标/粘贴事件分发
- Tab 键处理分支中，`completion_state.active` 为 true 时调用 `select_next()` 循环前进
- 粘贴事件处理中调用 `normalize_pasted_path()` / `pasted_image_format()`
- `Dialog` 活跃时调用 `Dialog::handle_key()`

### 4. `app/render.rs` — 渲染分发
- (各 14 个 CommandSurface 的 render 路由已在生产，无需修改入口文件本身)

### 5. `app_event.rs` — 事件类型
- `AppEvent::Tick` 解禁
- `AppEvent::Shutdown` 解禁

### 6. `command_surface/surfaces/` — 各 Slash 命令 Surface
- `agents.rs`: Agent 创建/编辑向导状态机（`agent_editor`, `color_picker`, `generate_agent`, `model_selector` 等子页面）
- `diff.rs`: `DiffDialogMode` / `DiffSource` 状态机分发
- `hooks.rs`: `hooks_config_menu` 概览页面集成
- `mcp.rs`: MCP 服务器详情→工具列表→工具详情→设置状态机
- `memory.rs`: `memory_update_notification` 渲染集成
- `tasks.rs`: 任务列表→详情状态机 + `BackendMessage` 事件处理
- `team.rs`: `team_status` 概览 + `teams_dialog` 详情页

### 7. `permissions/permission_request_router.rs` — 权限路由
- `bash_tool_use_options()` 路由分支
- `powershell_tool_use_options()` 路由分支
- `file_permission_dialog` 路由分支
- `Escalate` 变体在 `default_permission_options()` 和决策处理中的分支
- `PermissionRequestView::for_worker()` 在 `EngineEvent::PermissionRequest` 处理中

---

## 目录级执行计划

### 1. agents — ~25 项移除

**生产入口**: `CommandSurface::Agents` 持有 `AgentsSurface` 但向导流程未接入

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs` 8 个 `pub mod` 声明 | 接入 — Agent 向导子页面 | 移除 `#[cfg(test)]`；在 `AgentsSurface::handle_key()` 添加状态机 |
| 2 | `types.rs` 测试枚举变体/方法 | 部分接入 | 移除相关 `#[cfg(test)]` |
| 3 | `utils.rs` 3 个辅助函数 | 解禁 | 移除 `#[cfg(test)]` |
| 4 | `agent_file_utils.rs` 6 个辅助函数 | 部分接入 | 移除 `#[cfg(test)]` |
| 5 | `agents_list.rs::render()` | 接入 | 移除 `#[cfg(test)]` |
| 6 | `agents_menu.rs` 3 个方法 | 接入 | 移除 `#[cfg(test)]`；`handle_key()` 中调用 `move_next/prev` |

**修改文件**: `agents/mod.rs`, `agents/types.rs`, `agents/utils.rs`, `agents/agent_file_utils.rs`, `agents/agents_list.rs`, `agents/agents_menu.rs`, `command_surface/surfaces/agents.rs`

**验证**: `cargo test -p claude-code-rs ui::agents && cargo build --workspace --release`

---

### 2. app — ~6 项移除，1 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `app_event.rs::AppEvent::Tick` | 解禁 — 事件枚举变体 | 移除 `#[cfg(test)]` |
| 2 | `app_event.rs::AppEvent::Shutdown` | 解禁 — 优雅关闭信号 | 移除 `#[cfg(test)]`；tui.rs 后端断开时发送 |
| 3 | `input.rs::CompletionState::select_next()` | 接入 — Tab 循环前进 | 移除 `#[cfg(test)]`；Tab 分支调用 |
| 4 | `status.rs::StatusState::status_line_runner()` | 解禁 — 只读访问器 | 移除 `#[cfg(test)]` |
| 5 | `agent_navigation.rs::short_thread_id()` | 接入 — 与重复实现合并 | 移除 `#[cfg(test)]`；删除 `agent_tree_dialog.rs` 中的重复 |
| 6 | `agent_navigation.rs::render_agent_tree()` | ❌ 保留 — 仅文本调试 | 保持 `#[cfg(test)]` |

**修改文件**: `app_event.rs`, `app.rs`, `input.rs`, `status.rs`, `agent_navigation.rs`, `agent_tree_dialog.rs`

**验证**: `cargo test -p claude-code-rs ui::app && cargo build --workspace --release`

---

### 3. command_palette — ~3 项移除，~10 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `command_palette.rs::apply_command_suggestion()` | 接入 | 移除 `#[cfg(test)]` |
| 2 | `command_palette.rs::CommandAction` 枚举 | 接入 — Insert/Execute | 移除 `#[cfg(test)]`；`handle_key()` 返回供 App 处理 |
| 3 | `filter.rs::find_mid_input_slash_command()` | 解禁 | 移除 `#[cfg(test)]` |
| 4 | `tests.rs` 辅助函数 | ❌ 保留 | 保持 `#[cfg(test)]` |

**修改文件**: `command_palette/mod.rs`, `command_palette/filter.rs`, `app.rs`/`tui.rs`

**验证**: `cargo test -p claude-code-rs ui::command_palette && cargo build --workspace --release`

---

### 4. components — ~20 项移除，~5 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `approval_overlay.rs::ApprovalKind::subject()` | 解禁 | 移除 `#[cfg(test)]` |
| 2 | `approval_overlay.rs::ApprovalChoice/ApprovalOverlay` | ❌ 保留 — 与生产 PermissionChoice 重复 | 保持 `#[cfg(test)]` |
| 3 | `bottom_pane.rs::BottomPaneView/BottomPane` | ❌ 保留 — 仅 visual_regression 使用 | 保持 `#[cfg(test)]` |
| 4 | `divider.rs::char()/padding()/title()` | 解禁 — builder | 移除 `#[cfg(test)]` |
| 5 | `tabs.rs::Tab.id` 字段 | 解禁 | 移除 `#[cfg(test)]` |
| 6 | `tabs.rs` 字段 + 方法（select_next/prev 等） | 解禁 | 移除 `#[cfg(test)]` |
| 7 | `fuzzy_match.rs::weighted_fuzzy_match()` | 解禁 | 移除 `#[cfg(test)]` |
| 8 | `search_box.rs` 5 个 builder 方法 | 解禁 — 被生产 fuzzy_picker.rs 调用 | 移除 `#[cfg(test)]` |
| 9 | `keyboard_shortcut.rs::with_bold_key()/with_color()` | 解禁 — builder | 移除 `#[cfg(test)]` |
| 10 | `keyboard_shortcut.rs::render_byline()/Byline` | 接入 — 被生产 fuzzy_picker.rs 调用 | 移除 `#[cfg(test)]` |
| 11 | `history_search_dialog.rs::loading()/query()` | 解禁 | 移除 `#[cfg(test)]` |
| 12 | `better_view_panel.rs::key_value_row()` | 解禁 | 移除 `#[cfg(test)]` |

**修改文件**: `search_box.rs`, `keyboard_shortcut.rs`, `tabs.rs`, `divider.rs`, `fuzzy_match.rs`, `history_search_dialog.rs`, `better_view_panel.rs`, `approval_overlay.rs`

**验证**: `cargo test -p claude-code-rs ui::components && cargo build --workspace --release`

---

### 5. diff — ~12 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `diff_dialog.rs` 3 个 `use` 导入 | 接入 — `CommandSurface::Diff` 存在 | 移除 `#[cfg(test)]` |
| 2 | `diff_dialog.rs` 8 个辅助函数 + `render_diff_dialog_lines()` | 接入 — 核心渲染函数 | 移除 `#[cfg(test)]`；`DiffSurface::render()` 集成 |
| 3 | `diff_dialog.rs` 条件 use 导入 | 接入 | 移除 `#[cfg(test)]` |

**修改文件**: `diff/diff_dialog.rs`, `command_surface/surfaces/diff.rs`

**验证**: `cargo test -p claude-code-rs ui::diff && cargo build --workspace --release`

---

### 6. hooks — ~5 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs::pub mod hooks_config_menu` | 接入 — `CommandSurface::Hooks` 存在 | 移除 `#[cfg(test)]`；`HooksSurface::render()` 集成 |
| 2 | `select_hook_mode.rs::HookListItem::new()` | 解禁 | 移除 `#[cfg(test)]` |
| 3 | `select_matcher_mode.rs::HookMatcher::all_tools()/for_tool()` | 解禁 | 移除 `#[cfg(test)]` |
| 4 | `select_event_mode.rs::HOOK_EVENTS` re-export | 解禁 | 移除 `#[cfg(test)]` |

**修改文件**: `hooks/mod.rs`, `hooks/select_hook_mode.rs`, `hooks/select_matcher_mode.rs`, `hooks/select_event_mode.rs`, `command_surface/surfaces/hooks.rs`

**验证**: `cargo test -p claude-code-rs ui::hooks && cargo build --workspace --release`

---

### 7. input — ~9 项移除，~8 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `vim.rs::VimMode::short_indicator()` | 解禁 — 只读访问器 | 移除 `#[cfg(test)]` |
| 2 | `vim.rs::EditorModeSetting::as_str()` | 解禁 | 移除 `#[cfg(test)]` |
| 3 | `vim.rs::VimState::editor_mode_setting()` | 解禁 — getter | 移除 `#[cfg(test)]` |
| 4 | `slack_channel_completion.rs::MAX_CACHED_QUERIES` | 解禁 — 常量 | 移除 `#[cfg(test)]` |
| 5 | `slack_channel_completion.rs::set_known_channels()` 等 | ❌ 保留 | 保持 `#[cfg(test)]` |
| 6 | `form_navigation.rs::TabbedFormState::selected_option()` | 解禁 | 移除 `#[cfg(test)]` |
| 7 | `path_completion.rs::set_include_hidden()` 等 | ❌ 保留 | 保持 `#[cfg(test)]` |
| 8 | `shell_history_completion.rs::with_paths()` | ❌ 保留 | 保持 `#[cfg(test)]` |
| 9 | `completions.rs::is_empty()/provider_count()` | 解禁 | 移除 `#[cfg(test)]` |
| 10 | `completions.rs::find_command_token_range()` | 解禁 | 移除 `#[cfg(test)]` |
| 11 | `clipboard_paste.rs::normalize_pasted_path()` | 接入 | 移除 `#[cfg(test)]`；app/input.rs 调用 |
| 12 | `clipboard_paste.rs::normalize_windows_path()` | 接入 | 移除 `#[cfg(test)]` |
| 13 | `clipboard_paste.rs::pasted_image_format()` | 接入 | 移除 `#[cfg(test)]` |

**修改文件**: `input/vim.rs`, `input/slack_channel_completion.rs`, `input/form_navigation.rs`, `input/completions.rs`, `input/clipboard_paste.rs`, `app/input.rs`

**验证**: `cargo test -p claude-code-rs ui::input && cargo build --workspace --release`

---

### 8. mcp — ~20 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs` 17 个 `pub mod` 声明 | 接入 — `CommandSurface::Mcp` 存在 | 移除 `#[cfg(test)]`；McpSurface 状态机 |
| 2 | `index.rs::McpServerKind::Agent` 变体 | 接入 | 移除 `#[cfg(test)]` |
| 3 | `utils/mod.rs::reconnect_helpers` | 接入 | 移除 `#[cfg(test)]` |

**修改文件**: `mcp/mod.rs`, `mcp/index.rs`, `mcp/utils/mod.rs`, `command_surface/surfaces/mcp.rs`

**验证**: `cargo test -p claude-code-rs ui::mcp && cargo build --workspace --release`

---

### 9. memory — ~3 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs::pub mod memory_update_notification` | 接入 — `CommandSurface::Memory` 存在 | 移除 `#[cfg(test)]`；`MemorySurface::render()` 集成 |
| 2 | `memory_file_selector.rs::MemoryFileOption::with_parent()` | 解禁 — builder | 移除 `#[cfg(test)]` |
| 3 | `memory_file_selector.rs::MemoryFileSelectorState::selected_path()` | 解禁 — getter | 移除 `#[cfg(test)]` |

**修改文件**: `memory/mod.rs`, `memory/memory_file_selector.rs`, `command_surface/surfaces/memory.rs`

**验证**: `cargo test -p claude-code-rs ui::memory && cargo build --workspace --release`

---

### 10. messages — ~9 项移除，~10 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `render.rs::build_message_render_context()` | ❌ 保留 | 保持 `#[cfg(test)]` |
| 2 | `render.rs::render_single_message()` | ❌ 保留 — 测试入口 | 保持 `#[cfg(test)]` |
| 3 | `render.rs::abbreviate_json()` | 接入 — JSON 缩写 | 移除 `#[cfg(test)]` |
| 4 | `render.rs::tool_input_summary()` | 接入 — 工具输入摘要 | 移除 `#[cfg(test)]` |
| 5 | `assistant_tool_use_message.rs::ToolUseState` 变体 | 接入 — Queued/Waiting等 | 移除 `#[cfg(test)]`；`from_tool_use()` 处理 |
| 6 | `user_tool_result_message/utils.rs::with_tool_use_result()` | ❌ 保留 | 保持 `#[cfg(test)]` |
| 7 | `user_tool_result_message/utils.rs::line_to_text()` | 解禁 — 纯函数 | 移除 `#[cfg(test)]` |
| 8 | 所有 `render_*()` 辅助 | ❌ 保留 — snapshot 测试 | 保持 `#[cfg(test)]` |

**修改文件**: `messages/render.rs`, `messages/assistant_tool_use_message.rs`, `messages/user_tool_result_message/utils.rs`

**验证**: `cargo test -p claude-code-rs ui::messages && cargo build --workspace --release`

---

### 11. notifications — ~6 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs` 的 `NotificationMethod/DesktopNotificationBackend/bel/osc9/detect_backend/supports_osc9` | 接入 — 终端原生桌面通知 | 移除 `#[cfg(test)]`；`run_tui` 初始化调用 `detect_backend()` |
| 2 | `in_app.rs::with_rendered()` | 解禁 — builder | 移除 `#[cfg(test)]` |
| 3 | `in_app.rs::with_invalidates()` | 解禁 — builder | 移除 `#[cfg(test)]` |
| 4 | `in_app.rs::queued_len()` | 解禁 — 只读访问器 | 移除 `#[cfg(test)]` |

**修改文件**: `notifications/mod.rs`, `notifications/in_app.rs`, `tui.rs`

**验证**: `cargo test -p claude-code-rs ui::notifications && cargo build --workspace --release`

---

### 12. overlays — ~8 项移除，1 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs::pub mod overlay_stack` | ❌ 保留 — 无需 z-index 栈 | 保持 `#[cfg(test)]` |
| 2 | `dialog.rs::ExitGuard::{arm, disarm, confirm}` | 接入 — Ctrl+C 双击退出 | 移除 `#[cfg(test)]`；tui.rs 集成 |
| 3 | `dialog.rs::DialogEvent` 枚举 | 接入 — handle_key 返回类型 | 移除 `#[cfg(test)]` |
| 4 | `dialog.rs::Dialog::handle_key()` | 接入 — Modal 键盘输入 | 移除 `#[cfg(test)]`；app/input.rs 分发 |
| 5 | `dialog.rs::Dialog::subtitle()/hide_border()/cancel_active()/input_guide()` | 解禁 — builder | 移除 `#[cfg(test)]` |

**修改文件**: `overlays/dialog.rs`, `overlays/mod.rs`, `app/input.rs`, `tui.rs`

**验证**: `cargo test -p claude-code-rs ui::overlays && cargo build --workspace --release`

---

### 13. permissions — ~15 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `PermissionDecision::Escalate` 变体 | 接入 — 权限升级选项 | 移除 `#[cfg(test)]`；选项渲染 + App 决策处理 |
| 2 | `PermissionRequestView::for_worker()` | 接入 — worker 权限 | 移除 `#[cfg(test)]`；`EngineEvent` 处理路径 |
| 3 | `PermissionDialog::new()` | 解禁 — 标准构造器 | 移除 `#[cfg(test)]` |
| 4 | `ask_user_question_permission_request/mod.rs` 4 模块 | 接入 — QuestionDialog | 移除 `#[cfg(test)]`；集成渲染/键盘 |
| 5 | `bash_permission_request/mod.rs::bash_tool_use_options` | 接入 — bash 选项 | 移除 `#[cfg(test)]`；router 调用 |
| 6 | `power_shell_permission_request/mod.rs::powershell_tool_use_options` | 接入 — PowerShell 选项 | 移除 `#[cfg(test)]`；router 调用 |
| 7 | `file_permission_dialog/mod.rs` 4 模块 | 接入 — 文件权限 | 移除 `#[cfg(test)]`；router 新增 FileDialog 路由 |
| 8 | `rules/mod.rs` 4 模块 | 接入 — /permissions CommandSurface | 移除 `#[cfg(test)]`；PermissionsSurface 规则编辑 |

**修改文件**: `utils.rs`, `dialog_overlay.rs`, `permission_request_router.rs`, `ask_user_question_permission_request/mod.rs`, `bash_permission_request/mod.rs`, `power_shell_permission_request/mod.rs`, `file_permission_dialog/mod.rs`, `rules/mod.rs`

**验证**: `cargo test -p claude-code-rs ui::permissions && cargo build --workspace --release`

---

### 14. rendering — ~14 项移除，~6 项保留

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `theme.rs::ThemeKind` 枚举 | ❌ 保留 — 测试主题 | 保持 `#[cfg(test)]` |
| 2 | `theme.rs::progress_fill/progress_empty` 字段 | 解禁 — Theme 生产结构体 | 移除 `#[cfg(test)]`；`from_design_colors()` 初始化 |
| 3 | `theme.rs::Theme::new()/Theme::named()` | ❌ 保留 — 测试构造器 | 保持 `#[cfg(test)]` |
| 4 | `theme.rs::light_theme()/tron_theme()` | ❌ 保留 | 保持 `#[cfg(test)]` |
| 5 | `syntax_highlight.rs::supports_language()` 桩 | ❌ 保留 | 保持 `#[cfg(test)]` |
| 6 | `tool_activity.rs::ToolState` 4 变体 | 解禁 — 被 render_tool_activity.rs 引用 | 移除 `#[cfg(test)]` |
| 7 | `tool_activity.rs::ToolState::label()` | 解禁 | 移除 `#[cfg(test)]` |
| 8 | `tool_activity.rs::ToolActivity::new()` | 解禁 — 被生产调用 | 移除 `#[cfg(test)]` |
| 9 | `tool_activity.rs::render_grouped_activity()` | 解禁 — 被生产调用 | 移除 `#[cfg(test)]` |
| 10 | `tool_activity.rs::format_elapsed()` | 解禁 | 移除 `#[cfg(test)]` |
| 11 | `tool_activity.rs::compact_line()/line_to_plain()` | 解禁 — 被生产调用 | 移除 `#[cfg(test)]` |
| 12 | `virtual_scroll.rs` 5 个 getter | 解禁 — 基础查询 | 移除 `#[cfg(test)]` |

**修改文件**: `rendering/theme.rs`, `theme/mod.rs`, `rendering/tool_activity.rs`, `rendering/virtual_scroll.rs`

**验证**: `cargo test -p claude-code-rs ui::rendering && cargo build --workspace --release`

---

### 15. tasks — ~17 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs` 13 个 `pub mod` 声明 | 接入 — `CommandSurface::Tasks` 存在 | 移除 `#[cfg(test)]`；TasksSurface 状态机 |
| 2 | `mod.rs::TaskStatus::new()` | 接入 — 构造器 | 移除 `#[cfg(test)]` |
| 3 | `task_status_utils.rs` 4 辅助 + 3 use | 接入 — 进度条/标题 | 移除 `#[cfg(test)]` |

**修改文件**: `tasks/mod.rs`, `tasks/task_status_utils.rs`, `command_surface/surfaces/tasks.rs`

**验证**: `cargo test -p claude-code-rs ui::tasks && cargo build --workspace --release`

---

### 16. teams — ~3 项移除

**表格**:
| # | 项 | 生产入口判断 | 操作 |
|---|---|---|---|
| 1 | `mod.rs::pub mod team_status` | 接入 — `CommandSurface::Team` 存在 | 移除 `#[cfg(test)]`；`TeamSurface::render()` 集成 |
| 2 | `teams_dialog.rs::render_teams_dialog()` | 接入 — 团队对话框 | 移除 `#[cfg(test)]`；`TeamSurface::handle_key()` 切换 |

**修改文件**: `teams/mod.rs`, `teams/teams_dialog.rs`, `command_surface/surfaces/team.rs`

**验证**: `cargo test -p claude-code-rs ui::teams && cargo build --workspace --release`

---

## 生产入口文件汇总

### 核心入口（4 文件）

| 文件 | 修改内容 | 关联目录 |
|------|----------|----------|
| `crates/claude-code-rs/src/ui/tui.rs` | `detect_backend()`, `ExitGuard`, `AppEvent::Shutdown` 发送, CommandAction::Execute 处理 | notifications, overlays, app, command_palette |
| `crates/claude-code-rs/src/ui/app.rs` | `AppEvent::Tick/Shutdown` 匹配，CommandAction::Execute 处理 | app, command_palette |
| `crates/claude-code-rs/src/ui/app/input.rs` | `select_next()` Tab 循环，`normalize_pasted_path()` / `pasted_image_format()`, `Dialog::handle_key()` 分发 | input, overlays |
| `crates/claude-code-rs/src/ui/app/app_event.rs` | `AppEvent::Tick`, `AppEvent::Shutdown` 解禁 | app |

### CommandSurface 路由（7 文件）

| 文件 | 修改内容 | 关联目录 |
|------|----------|----------|
| `command_surface/surfaces/agents.rs` | Agent 向导状态机（8 子页面） | agents |
| `command_surface/surfaces/diff.rs` | DiffDialogMode/DiffSource 状态机 | diff |
| `command_surface/surfaces/hooks.rs` | hooks_config_menu 概览 | hooks |
| `command_surface/surfaces/mcp.rs` | MCP 子页面状态机（详情/工具/设置） | mcp |
| `command_surface/surfaces/memory.rs` | memory_update_notification | memory |
| `command_surface/surfaces/tasks.rs` | 任务列表→详情状态机 | tasks |
| `command_surface/surfaces/team.rs` | team_status + teams_dialog | teams |

### 权限路由（1 文件）

| 文件 | 修改内容 | 关联目录 |
|------|----------|----------|
| `permissions/permission_request_router.rs` | bash/PowerShell/file_dialog 路由，Escalate 变体，for_worker() | permissions |

### 各子目录文件（36 文件）

| 目录 | 文件 |
|------|------|
| agents/ | `mod.rs`, `types.rs`, `utils.rs`, `agent_file_utils.rs`, `agents_list.rs`, `agents_menu.rs` |
| app/ | `status.rs`, `agent_navigation.rs`, `agent_tree_dialog.rs` |
| command_palette/ | `mod.rs`, `filter.rs` |
| components/ | `search_box.rs`, `keyboard_shortcut.rs`, `tabs.rs`, `divider.rs`, `fuzzy_match.rs`, `history_search_dialog.rs`, `better_view_panel.rs`, `approval_overlay.rs` |
| diff/ | `diff_dialog.rs` |
| hooks/ | `mod.rs`, `select_hook_mode.rs`, `select_matcher_mode.rs`, `select_event_mode.rs` |
| input/ | `vim.rs`, `slack_channel_completion.rs`, `form_navigation.rs`, `completions.rs`, `clipboard_paste.rs` |
| mcp/ | `mod.rs`, `index.rs`, `utils/mod.rs` |
| memory/ | `mod.rs`, `memory_file_selector.rs` |
| messages/ | `render.rs`, `assistant_tool_use_message.rs`, `user_tool_result_message/utils.rs` |
| notifications/ | `mod.rs`, `in_app.rs` |
| overlays/ | `dialog.rs` |
| permissions/ | `utils.rs`, `dialog_overlay.rs`, `ask_user_question_permission_request/mod.rs`, `bash_permission_request/mod.rs`, `power_shell_permission_request/mod.rs`, `file_permission_dialog/mod.rs`, `rules/mod.rs` |
| rendering/ | `theme.rs`, `tool_activity.rs`, `virtual_scroll.rs` |
| tasks/ | `mod.rs`, `task_status_utils.rs` |
| teams/ | `mod.rs`, `teams_dialog.rs` |
| theme/ | `mod.rs` |

---

## 执行顺序建议

### Phase 1: 解禁安全项（无生产逻辑变更）
> 仅移除 `#[cfg(test)]`，不需要修改生产入口文件。可在不依赖其他任务的情况下并行完成。

- `agents/utils.rs`, `agents/agent_file_utils.rs`
- `components/` 中 builder/getter 方法（divider, tabs, fuzzy_match, search_box, keyboard_shortcut, history_search_dialog, better_view_panel）
- `input/` 中解禁项（vim, slack_channel_completion, form_navigation, completions）
- `memory/memory_file_selector.rs`
- `messages/` 中解禁项（abbreviate_json, tool_input_summary, line_to_text）
- `notifications/in_app.rs`
- `overlays/dialog.rs` builder 方法
- `rendering/` 中解禁项（progress_fill, tool_activity 部分, virtual_scroll）
- `app/status.rs`, `app/agent_navigation.rs`, `app/app_event.rs`

验证: `cargo build --workspace --release`

### Phase 2: CommandSurface 状态机接入
> 需要同时修改子目录文件和 `command_surface/surfaces/` 中的路由文件。按 Surface 独立进行。

- `agents/` + `command_surface/surfaces/agents.rs`
- `diff/` + `command_surface/surfaces/diff.rs`
- `hooks/` + `command_surface/surfaces/hooks.rs`
- `mcp/` + `command_surface/surfaces/mcp.rs`
- `memory/` + `command_surface/surfaces/memory.rs`
- `tasks/` + `command_surface/surfaces/tasks.rs`
- `teams/` + `command_surface/surfaces/team.rs`

每对独立验证: `cargo build --workspace --release`

### Phase 3: 生产入口增强
> 修改 tui.rs、app/input.rs 等核心入口文件。

- `tui.rs`: notification backend detection + ExitGuard + AppEvent::Shutdown
- `app/input.rs`: Tab select_next + clipboard normalize + Dialog::handle_key
- `app.rs`: AppEvent::Tick/Shutdown + CommandAction::Execute

验证: `cargo build --workspace --release`

### Phase 4: 权限系统增强
> 修改 PermissionRequestRouter 和权限子页面。

- `permission_request_router.rs`: 新增路由分支
- `permissions/` 各子模块: bash, powershell, file_dialog, rules, ask_user_question

验证: `cargo build --workspace --release`

### Phase 5: 全量验证
```
cargo test -p claude-code-rs ui::agents
cargo test -p claude-code-rs ui::app
cargo test -p claude-code-rs ui::command_palette
cargo test -p claude-code-rs ui::components
cargo test -p claude-code-rs ui::diff
cargo test -p claude-code-rs ui::hooks
cargo test -p claude-code-rs ui::input
cargo test -p claude-code-rs ui::mcp
cargo test -p claude-code-rs ui::memory
cargo test -p claude-code-rs ui::messages
cargo test -p claude-code-rs ui::notifications
cargo test -p claude-code-rs ui::overlays
cargo test -p claude-code-rs ui::permissions
cargo test -p claude-code-rs ui::rendering
cargo test -p claude-code-rs ui::tasks
cargo test -p claude-code-rs ui::teams
cargo build --workspace --release
```

---

## 附录：已知编译 Bug（需在 Phase 1 优先修复）

以下项当前 `#[cfg(test)]` 但在非测试生产代码中被无条件引用，**必须先解禁**否则非测试构建失败：

| # | 文件 | 被谁引用 | 风险 |
|---|------|----------|------|
| 1 | `components/search_box.rs`: `focused()`, `terminal_focused()`, `cursor_offset()` | `components/fuzzy_picker.rs`（生产 `pub mod`） | 编译失败 |
| 2 | `components/keyboard_shortcut.rs::render_byline()` | `components/fuzzy_picker.rs` L288 | 编译失败 |
| 3 | `rendering/tool_activity.rs::ToolState` 4 变体 | `tasks/render_tool_activity.rs`（非测试模块） | 编译失败 |
| 4 | `rendering/tool_activity.rs::ToolActivity::new()` | `tasks/render_tool_activity.rs` L17 | 编译失败 |
| 5 | `rendering/tool_activity.rs::render_grouped_activity()` | `tasks/render_tool_activity.rs` L5 | 编译失败 |
| 6 | `rendering/theme.rs::progress_fill/progress_empty` | `progress_bar.rs::render_styled_progress_bar()` | 编译失败 |
