# Rust 版 Computer Use 落地清单

## 1. 目标

在 `rust/` 中落地可用的 Computer Use 能力，使模型能够：

- 主动调用桌面控制相关工具
- 获取截图并将截图继续回喂给模型
- 在同一轮 agent loop 中完成"看屏幕 -> 决策 -> 点击/输入 -> 再看屏幕"
- 在 macOS / Windows / Linux 上逐步具备可验证能力

本清单以 Full Build 代码结构为准，不假设要一比一复制 TypeScript 版本的内部实现。

## 2. 当前结论

### 2.1 已有基础

- 已有 MCP 客户端、发现、连接、工具包装能力，可连接外部 MCP server。
  - 见 [src/main.rs](../src/main.rs)、[src/mcp/manager.rs](../src/mcp/manager.rs)、[src/mcp/tools.rs](../src/mcp/tools.rs)
- 已有 `ContentBlock::Image`、`ToolResultContent::Blocks` 类型定义，消息模型允许保存图片块。
  - 见 [src/types/message.rs](../src/types/message.rs)
- 已有权限模式、规则系统、交互式 permission callback。
  - 见 [src/types/tool.rs](../src/types/tool.rs)、[src/permissions/decision.rs](../src/permissions/decision.rs)

### 2.2 当前硬阻塞

- MCP 图片结果会在 wrapper 层被压扁成字符串 `"[Image: image/png]"`，图像数据没有进入消息历史。
  - 见 [src/mcp/tools.rs](../src/mcp/tools.rs)
- query loop 会把所有工具结果强制转成 `ToolResultContent::Text(...)`，不会保留 `Blocks([Image])`。
  - 见 [src/query/loop_impl.rs](../src/query/loop_impl.rs)
- 权限 matcher 只认识 `Bash / Read / Write / Edit / Glob`，对 Computer Use 工具没有专门规则入口。
  - 见 [src/permissions/decision.rs](../src/permissions/decision.rs)
- CLI 没有 `--computer-use-mcp` 快速路径；当前 `mcp/` 目录主要是客户端，不是内置 MCP server 宿主。
  - 见 [src/main.rs](../src/main.rs)、[src/mcp/](../src/mcp/)

## 3. 推荐路线

### 结论

推荐分两步走：

1. **先做"外置 Computer Use MCP + Rust 侧图像保真传输" MVP**
2. **再决定是否需要"内置 `--computer-use-mcp` + 原生平台 backend"**

原因：

- Rust 侧已经有 MCP client，最短路径不是重写一整套 darwin/win32/linux backend，而是先把现成 `computer-use-mcp` 接进来。
- 真正的关键门槛不是"有没有 MCP"，而是"截图能不能作为图片 tool_result 继续进入下一轮模型上下文"。
- 如果这条图像链路没打通，即使接上外部 Computer Use MCP，模型也只能收到字符串占位符，无法完成闭环。

## 4. 落地清单

### Phase 0：打通多模态工具结果链路

这是 **第一优先级**。不完成这一层，Computer Use 只能"调用工具"，不能"看见截图"。

- [x] 在 [src/mcp/tools.rs](../src/mcp/tools.rs) 中新增"结构化 MCP 结果转换"逻辑。
  - 目标：把 `ToolCallContent::Image { data, mime_type }` 转成 `ContentBlock::Image { source }`
  - 不再只通过 `format_tool_call_result()` 返回字符串
- [x] 改造 [src/types/tool.rs](../src/types/tool.rs) 的 `ToolResult`
  - 建议新增结构化字段，例如：
  - `model_content: Option<ToolResultContent>`
  - `display_preview: Option<String>`
  - 保留现有 `data: Value` 仅用于兼容和日志，不要再让它承担全部语义
- [x] 改造 [src/query/loop_impl.rs](../src/query/loop_impl.rs)
  - 目标：当工具返回图片块或混合块时，生成 `ContentBlock::ToolResult { content: ToolResultContent::Blocks(...) }`
  - 只有纯文本结果才走 `ToolResultContent::Text(...)`
- [x] 为导出模块补充图片块回归验证
  - `cargo test -p cc-session session_export` 确认图片型 tool result 导出为结构化 metadata + 可读占位，不丢失 MIME/长度信息，也不输出原始 base64
- [ ] 为 session save/restore 补充图片块回归验证
  - 确认 `Blocks([Image])` 在会话保存、恢复后仍存在
- [x] 为 headless/TUI 增加最小可见性
  - `crates/cc-ipc-protocol/src/protocol.rs`：`ToolResult` 消息增加可选 `content_blocks` 和 `ToolResultContentInfo` 类型
  - `crates/claude-code-rs/src/ui/messages.rs`：图片型 tool result 渲染为 `[image: mime_type]`
  - `crates/cc-ipc/src/headless.rs`：`extract_tool_result_output()` 正确提取图片占位符

**Phase 0 验收标准**

- fake MCP server 返回图片时，query loop 生成的工具结果是 `Blocks([Image])`，不是字符串占位符
- 下一轮模型请求中能带上图片内容块
- 会话保存/恢复后图片块仍存在

### Phase 1：接通外置 Computer Use MCP MVP

这一阶段不追求内置 server，对标"先可用"。

- [x] 增加一份 Rust 侧 Computer Use MCP 接入文档
  - 推荐新增 `development/reference/computer-use-mcp-config.md`
  - 内容包括 `.cc-rust/settings.json` 中 `mcpServers.computer-use` 的配置示例
- [x] 约定保留 server 名称 `computer-use`
  - 便于权限、日志、UI 文案和后续内置化保持一致
- [x] 为 `mcp__computer-use__*` 工具加一层识别和分组
  - 可先通过名称前缀处理，不必一开始就做复杂 capability 协商
- [x] 做一条端到端 smoke test
  - `screenshot` 工具返回图片
  - `left_click` / `type_text` / `key` / `scroll` 等至少有一组能成功 round-trip

**建议的 MVP 截止线**

- 单平台先跑通 Windows 或 macOS
- 模型可以：
  - 调 `mcp__computer-use__screenshot`
  - 收到真实图片结果
  - 再决定调用 `click/type/key`

### Phase 2：权限模型补齐

Computer Use 不能直接复用普通 Bash/File 的权限语义，需要单独加一层"桌面控制授权"。

- [x] 扩展 [src/permissions/decision.rs](../src/permissions/decision.rs) 的 matcher
  - 支持 `mcp__computer-use__screenshot`
  - 支持 `mcp__computer-use__left_click`
  - 支持 `mcp__computer-use__type_text`
  - 支持 `mcp__computer-use__key`
  - 支持 `mcp__computer-use__scroll`
  - 支持未来原生工具名
- [x] 为 Computer Use 定义单独的危险等级
  - 截图：中风险
  - 点击/输入/快捷键：高风险
  - 打开应用、切换窗口、剪贴板读写：高风险
- [x] 在 [src/types/tool.rs](../src/types/tool.rs) / permission context 中增加"会话级授权"概念
  - 避免每次点击都弹权限框
  - 建议首次调用 `request_access` 后缓存到 session 级
- [x] 新增 OS 能力检查模块
  - 建议路径：`crates/cc-computer-use/src/host_adapter.rs`
  - macOS：检查 Accessibility / Screen Recording
  - Windows：先允许，但要检测必要 API/PowerShell 可用性
  - Linux：检查 `xdotool` / `scrot` / `xclip` / `wmctrl`
- [x] 权限 UI 文案要单独写
  - 不能只显示"允许调用工具"
  - 应明确说明"允许读取屏幕并控制键盘鼠标"

**Phase 2 验收标准**

- 首次 Computer Use 调用会触发明确授权
- 同会话内后续调用不重复骚扰
- 权限拒绝后能稳定回退，不进入死循环

### Phase 3：内置 `--computer-use-mcp` 快速路径（可选）

只有在外置 MCP MVP 稳定后，才建议做这一层。

原因：

- 当前 Rust 项目已有 MCP client，但没有 TypeScript 那种现成的"内置 MCP server 快速路径"
- 这一步会把范围从"接入能力"扩大到"宿主能力 + server 生命周期管理"

- [x] 在 [src/main.rs](../src/main.rs) 增加 `--computer-use` fast path
  - 改用原生 Tool trait 注册而非 MCP server 进程, 性能更优
- [x] 新增 `src/computer_use/setup.rs`
  - `register_cu_tools()` 返回 10 个原生 CU 工具
- [x] 决定是否要支持与 TS 同名工具集合
  - 使用 `mcp__computer-use__` 前缀, 与外置 MCP 版本工具名兼容

**是否必须做**

- 如果目标只是"Rust CLI 可以使用 Computer Use"，这一步不是必须
- 如果目标是"与 TS 版功能入口一致"，这一步是必须

### Phase 4：原生平台 backend（可选，成本最高）

只有在以下场景才建议做：

- 不想依赖外部 `computer-use-mcp`
- 希望把平台差异完全收回 Rust 仓库
- 需要比现成 MCP 更细粒度的性能/权限控制

建议新增模块：

- [x] `crates/cc-computer-use/src/lib.rs`
- [x] `crates/cc-computer-use/src/input/mod.rs` (含 darwin/win32/linux)
- [x] `crates/cc-computer-use/src/screenshot/mod.rs` (含 darwin/win32/linux)
- [x] `crates/cc-computer-use/src/lock.rs` — 并发锁
- [x] `crates/cc-computer-use/src/esc_hotkey.rs` — 退出热键
- [x] `crates/cc-computer-use/src/app_names.rs` — 应用名映射
- [x] `crates/cc-computer-use/src/host_adapter.rs` — 主机适配器
- [x] `crates/cc-computer-use/src/drain_run_loop.rs` — 事件循环 drain
- [x] `crates/cc-computer-use/src/executor.rs` — 执行器编排
- [x] `crates/cc-computer-use/src/input_loader.rs` — 输入加载器
- [x] `crates/cc-computer-use/src/swift_loader.rs` — Swift 加载器
- [x] `crates/cc-computer-use/src/win32/mod.rs`
- [x] `crates/cc-computer-use/src/win32/com_word.rs` — Word COM 自动化
- [x] `crates/cc-computer-use/src/win32/com_excel.rs` — Excel COM 自动化
- [x] `crates/cc-computer-use/src/win32/ui_automation.rs` — UI 自动化
- [x] `crates/cc-computer-use/src/win32/virtual_cursor.rs` — 虚拟光标
- [x] `crates/cc-computer-use/src/win32/window_border.rs` — 窗口边框
- [x] `crates/cc-computer-use/src/win32/shared.rs` — Win32 共享工具
- [x] `crates/cc-computer-use/src/win32/input_indicator.rs` — 输入指示器

平台实现情况：

- macOS
  - 输入：`cliclick` / `osascript` ✅
  - 截图：`screencapture` ✅
  - 权限：TCC (host_adapter + swift_loader) ✅
  - 事件 drain: CGEvent via Swift ✅
- Windows
  - 输入：PowerShell + WinAPI ✅
  - 截图：PowerShell + System.Drawing ✅
  - 应用管理：PowerShell + WinAPI (shared.rs) ✅
  - COM 自动化 (Word/Excel) ✅
  - UI 自动化 (UIAutomationClient) ✅
  - 虚拟光标 ✅
  - 窗口边框管理 ✅
  - 输入指示器 ✅
- Linux
  - 输入：`xdotool` ✅
  - 截图：`scrot` (X11) / `grim` (Wayland) ✅
  - 窗口管理：`xdotool` / `wmctrl` ✅

### Phase 5：会话、导出、前端体验补齐

- [x] `crates/cc-session/src/session_export/*`
  - 图片型 tool result 导出为结构化 metadata + 可读占位，不输出空白或 `[object Object]`
- [ ] `crates/cc-ipc-protocol/src/protocol.rs`
  - `ToolResult` 增加可选 `content_blocks`
  - headless 前端可以直接展示图片结果
- [ ] [src/ui/messages.rs](../src/ui/messages.rs)
  - 把 Computer Use 工具调用做更清晰的呈现
  - 区分截图、点击、输入、权限请求、失败重试
- [ ] 如果有独立前端 `ui/`
  - 增加图片型 tool result 的展示占位
  - 至少能在历史中看出"本轮看到了哪张图"

### Phase 6：测试清单

- [ ] 单元测试：MCP `image` 结果转换为 `ContentBlock::Image`
- [ ] 单元测试：query loop 对结构化 tool result 不再强制字符串化
- [x] 单元测试：session export 对图片块的结构化占位输出
- [ ] 单元测试：session storage 对图片块的保存与恢复
- [ ] 集成测试：fake MCP server 返回 screenshot 图片，模型收到图片块
- [ ] 集成测试：权限拒绝 / 允许 / session grant 三种路径
- [ ] 平台 smoke test：
  - macOS：截图 + 权限检测
  - Windows：截图 + 点击
  - Linux：截图 + `xdotool` 检测

## 5. 文件级改动建议

### 必改

- [src/mcp/tools.rs](../src/mcp/tools.rs)
- [src/types/tool.rs](../src/types/tool.rs)
- [src/query/loop_impl.rs](../src/query/loop_impl.rs)
- [src/permissions/decision.rs](../src/permissions/decision.rs)

### 高概率会改

- `crates/cc-ipc-protocol/src/protocol.rs`
- `crates/claude-code-rs/src/ui/messages.rs`
- `crates/cc-session/src/storage.rs`
- `crates/cc-session/src/session_export/*`

### 可后置

- [src/main.rs](../src/main.rs) 的 `--computer-use-mcp` fast path
- `src/computer_use/*` 原生 backend 全家桶

## 6. 建议执行顺序

### 里程碑 A：图像 tool_result 打通

- [x] 改 `ToolResult` 结构
- [x] 改 `mcp/tools.rs`
- [x] 改 `query/loop_impl.rs`
- [x] 加 fake MCP image 测试

### 里程碑 B：外置 MCP MVP

- [x] 写 `computer-use` MCP 配置文档
- [x] 跑通 `screenshot -> image tool_result -> next turn`
- [x] 跑通一个输入动作

### 里程碑 C：权限与 UX

- [x] session-level request_access
- [x] 权限 matcher / dangerous 分类
- [x] TUI / headless 显示优化

### 里程碑 D：内置化或原生化

- [x] `--computer-use` CLI flag (原生 Tool trait 注册)
- [x] 动态注册 (`setup::register_cu_tools()`)
- [x] 原生平台 backend (Windows/macOS/Linux, 10 个工具)
- [x] 并发锁 (lock.rs)
- [x] 退出热键 (esc_hotkey.rs)
- [x] 应用名映射 (app_names.rs)
- [x] 主机适配器 (host_adapter.rs)
- [x] 事件循环 drain (drain_run_loop.rs)
- [x] 执行器编排 (executor.rs)
- [x] 输入加载器 & Swift 加载器
- [x] Win32 COM 自动化 (Word/Excel)
- [x] Win32 UI 自动化
- [x] 虚拟光标
- [x] 窗口边框 & 输入指示器

## 7. 不建议一开始做的事

- 不要一上来重写完整 darwin/win32/linux backend
- 不要先做复杂前端渲染，再补模型图像链路
- 不要把 MCP 图片继续压成文本占位后再做"Computer Use 验证"
- 不要把权限复用成普通 Bash 权限，这会导致风险边界不清

## 8. MVP 定义

满足以下条件即可认为 Rust 版 Computer Use 进入 MVP：

- 模型能调用 `computer-use` 相关 MCP 工具
- `screenshot` 返回的图片能作为图片块进入下一轮上下文
- 至少一个平台上能完成：
  - 截图
  - 点击
  - 输入
  - 再截图
- 首次调用有明确授权提示
- 会话保存/恢复不会破坏图片型 tool result

## 9. 一句话决策

**先补"图片型 tool_result 贯通"再接外置 Computer Use MCP；内置 `--computer-use-mcp` 和原生平台 backend 放在第二阶段。**

---

## 完成状态（2026-05-21）

**判定：⚠️ 部分完成（约 48/60 项完成，~12 项待办）**

已完成的主要里程碑：
- **里程碑 A（图像 tool_result 打通）**：全部 4 项 ✅ — `ToolResult` 结构改造、`mcp/tools.rs` 改造、`query/loop_impl.rs` 改造、fake MCP image 测试
- **里程碑 B（外置 MCP MVP）**：全部 3 项 ✅ — MCP 配置文档、screenshot→image→next turn、一个输入动作
- **里程碑 C（权限与 UX）**：全部 3 项 ✅ — session-level request_access、权限 matcher/危险分类、TUI/headless 显示优化
- **里程碑 D（内置化/原生化）**：全部 15 项 ✅ — `--computer-use` CLI flag、动态注册、原生平台 backend（Windows/macOS/Linux）、并发锁、退出热键、应用名映射、主机适配器、事件循环 drain、执行器编排、输入/Swift 加载器、Win32 COM/UI 自动化、虚拟光标、窗口边框/输入指示器

Phase 0~5 核心实现已大部分完成。

仍未完成（~12 项）：
- **Phase 0**：session save/restore 图片块回归验证（1 项 ⬜）
- **Phase 5（会话/导出/前端）**：`cc-ipc-protocol` `ToolResult` 增加 `content_blocks`、`ui/messages.rs` Computer Use 工具调用更清晰呈现（2 项 ⬜）
- **Phase 6（测试清单）**：单元测试 3 项 ⬜、集成测试 3 项 ⬜、平台 smoke test 3 项 ⬜（共 ~9 项 ⬜）
