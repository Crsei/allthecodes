# 缺失功能汇总与实现状态验证

> **目的**: 基于 `docs/ui/great/` 奇偶性分析文档与并行 Agent 实际代码搜索，汇总
> Rust (ratatui/OpenTUI) 中仍缺失或未完成的 UI 功能，并验证文档评分的准确性。
>
> **生成日期**: 2026-05-18
> **验证方法**: 4 个并行 Explore Agent 搜索 `rust/crates/claude-code-rs/src/ui/` 实际代码，
> 逐项确认文件存在性、实现完整度、渲染管道接线状态

## Logo 相关（不做，已登记）

以下 Logo 相关内容确认**不做**，不在补齐范围内：

- **Logo V2 动画**（TS `LogoV2/` 18 个文件）— 动画 ASCII 艺术标志，ratatui 不支持动画
- **ASCII 艺术大字标志**（OpenTUI `WelcomeScreen.tsx` 中的 ASCII art）— 已有装饰元素，无需进一步开发
- 欢迎屏幕保持简洁信息展示（版本、模型、会话 ID、CWD），不做动画标志、扫描线效果等增强

已更新 `README.md` §Logo 相关（不做）。

---

## 1. 消息渲染系统 — 27/34 个渲染器为纯文本 stub

### 关键缺失（P0）

| 文件 | 行数 | 返回值 | `_theme` 使用 | 现状 |
|------|:----:|:------:|:------------:|------|
| `assistant_text_message.rs` | 11 | `String` | 忽略 | **P0** — 只区分空/非空文本；TS 处理 10+ 种 API 错误状态（速率限制、无效 key、信用余额、超时、过载、被封禁、权限被拒等），每种有独立的用户引导文案 |
| `assistant_tool_use_message.rs` | 9 | `String` | 忽略 | **P0** — 一行 `format!()`；TS 有 ~368 行完整状态机（进度、队列、loader、spinner）+ `HookProgressMessage` 集成 |
| `system_text_message.rs` | 19 | `String` | 忽略 | **P0** — 通用格式；TS 有 15+ 种子类型（`turn_duration`、`memory_saved`、`away_summary`、`agents_killed`、`bridge_status`、`api_error` 等），共 ~827 行 |
| `user_text_message.rs` | 12 | `String` | 忽略 | **P1** — TS 是通过标签路由 15+ 消息类型的中央调度器（~275 行）；Rust 是简单 `format!()` |
| `attachment_message.rs` | 12 | `String` | 忽略 | **P1** — TS 有 25+ 附件类型渲染器（~536 行）；Rust 只有 7 个内联变体 |

### 全部 34 个渲染器分类

| 类别 | 数量 | 文件 |
|------|:----:|------|
| 返回 `String`（纯文本，无样式） | 27 | 除 `user_tool_result_message/` 外的所有顶层 `messages/*.rs` |
| 返回 `Vec<Line>`（带样式） | 7 | `user_tool_result_message/` 内子模块 |
| 接受 `_theme` 但忽略 | 24 | 全部 27 个 `String`-returning 中的 24 个（3 个连 theme 都不接受） |
| 使用 `theme` | 7 | 全部 7 个 `Vec<Line>`-returning |

### API 错误状态严重不足

- `assistant_text_message.rs`: **0 种**错误状态处理
- `system_api_error_message.rs`: **1 种**通用状态（`format!("API error {status}: {detail}")`）
- TS 参考：**10+ 种**独立错误状态，每种带用户指导文案

### 第三方 API / OAuth 错误展示约定

使用第三方 API 或 OAuth/Bearer Token 时，后端只能接收第三方服务实际返回的
HTTP 状态、SSE error event、错误 body 或网络错误。速率限制、无效 key、信用余额、
超时、过载、被封禁、权限被拒等信息**只有在第三方响应中明确返回时才能展示**；
如果第三方只返回通用 `403 forbidden` 或空错误体，Rust 端不应推断成更细的业务原因。

在补齐完整分类 UI 前，API 报错的默认展示格式应保持稳定、可诊断：

```text
Error occurred: <具体错误信息>
```

其中 `Error occurred:` 为英文错误前缀，`<具体错误信息>` 使用后端收到的原始错误详情
或标准化后的错误文本，例如：

```text
Error occurred: Provider openrouter error (HTTP 429): rate limit exceeded
Error occurred: API error provider=anthropic status=403 type=permission_error: forbidden
Error occurred: stream idle timeout after 300000ms
```

---

## 2. 设计系统组件 — 16 个全部缺失

| 组件 | Rust ratatui | Rust OpenTUI | TS design-system | 现状 |
|------|:----------:|:----------:|:--------------:|------|
| Dialog（通用） | 缺失 | 缺失 | 完整 | **缺失** — 仅有 PermissionDialog (357行) 且非通用 |
| Pane（通用） | 缺失 | 缺失 | 完整 | **缺失** — 仅有 BetterViewPanel (187行) 领域专用 |
| Divider | 缺失 | 缺失 | 完整 | **缺失** — 手动 `"\u{2500}".repeat()` |
| ThemedBox | 缺失 | 缺失 | 完整 | **缺失** — 直接 `theme.warning` 字段访问 |
| ThemedText | 缺失 | 缺失 | 完整 | **缺失** |
| ThemeProvider | 缺失 | 缺失 | 完整 | **缺失** — 无运行时主题切换 |
| KeyboardShortcutHint | **存在** | 缺失 | 完整 | ShortcutHint (42行) 已集成渲染管道 |
| ListItem | 缺失 | 缺失 | 完整 | **缺失** |
| ProgressBar | **存在** | 部分 | 完整 | `render_progress_bar()` (53行) 字符串函数 |
| StatusIcon | **存在** | 部分 | 完整 | 简单枚举 (33行)，纯文本标签 |
| Tabs | **存在** | 部分 | 完整 | 最小函数 (28行)，无状态/键盘 |
| FuzzyPicker（UI） | 缺失（算法有）| 缺失 | 完整 | 仅有 `fuzzy_match` 算法 (123行) |
| Byline | 缺失 | 缺失 | 完整 | **缺失** |
| LoadingState | 缺失 | 缺失 | 完整 | **缺失** |
| Ratchet | 缺失 | 缺失 | 完整 | **缺失** |
| color（主题色解析）| 缺失 | 缺失 | 完整 | **缺失** |

**证据：** `components/` 目录中 00 个可复用设计系统 primitive。所有颜色硬编码。

> 补齐计划：[plans/plan-02-design-system.md](plans/plan-02-design-system.md) — 包含 16 个组件的分步实施步骤、依赖图和执行优先级。总工作量 17-21 天，分 3 个冲刺。

---

## 3. 输入/编辑器 — 多行和高亮缺失

> **补齐计划**: 见 `docs/ui/great/plans/plan-03-input-editor.md`（6 项缺失功能，共 ~2000 行估算工作量）

| 功能 | 行数 | 现状 |
|------|:----:|------|
| PromptInput 单行文本输入 | 473 | **完整** — 支持光标/粘贴/CTRL快捷键/水平滚动 |
| **多行输入** | — | **缺失** — PromptInput 文档明确定义为 "single-line"，Enter/Shift+Enter 均为提交 |
| **文本高亮**（彩虹色、@提及、/命令、token 预算） | — | **缺失** — 所有输入以 `Span::raw` 纯文本渲染 |
| **撤销/重做** | — | **缺失** — `VimAction::Undo` 映射到 `AppAction::None`（无操作）；全局搜索 `undo_history`/`undo_stack` 零结果 |
| **图片粘贴** | — | **stub** — `input/clipboard_paste.rs` 代码完整但从未被事件处理器调用 |
| Vim 模式状态机 | 916 | **完整** — 已通过 `apply_vim_action()` 方法连接到 PromptInput |
| 历史搜索（Ctrl+R） | 516 | **完整** — 模糊搜索 + 预览 + 导航 |
| 命令面板 | 245 | **完整** — 已与输入同步集成 |
| 文本粘贴 | 内联 | **完整** — bracketed paste + 超大粘贴截断提示 |
| ChatComposer | 126 | **stub** — 纯状态模型，无 widget 实现 |
| 输入建议（自动补全） | — | **部分** — 命令面板处理 `/` 命令；无 inline autocomplete |

---

## 4. 权限系统 — 模块完整但 17 个为死代码

| 发现 | 详情 |
|------|------|
| 权限子模块总数 | 20+ 个工具专用请求组件 |
| `#[allow(dead_code)]` 文件 | **17 个** `permissions/*/mod.rs` 文件 |
| 总 `#[allow(dead_code)]` 实例 | **42 处** |
| 渲染方式 | 全部基于字符串（非 ratatui widget），仅 snapshot 测试 |
| 唯一接线的对话框 | `dialog_overlay.rs` 中的 `PermissionDialog`（ratatui widget） |
| BypassPermissionsModeDialog | **缺失** |
| 权限反馈输入 | **缺失** |
| IDE diff 配置显示 | **缺失** |

**证据：** `permissions.rs` 模块索引列出 27 个子模块，大部分仅用于 `#[cfg(test)]`。

---

## 5. 终端内通知系统 — 1/5

| 功能 | 现状 |
|------|------|
| 桌面通知（BEL/OSC9 后端） | **完整** — 自动终端检测 |
| **通知队列**（优先级/超时/分类） | **缺失** — 无 `NotificationQueue` 结构 |
| **专用通知布局区域** | **缺失** — 状态栏只有底部栏，无通知横幅插槽 |
| **Toast/Banner 组件** | **缺失** |
| **IDE 状态指示器** | **缺失** |
| **内存使用指示器** | **缺失** |
| **Token 警告** | **缺失** |
| **速率限制警告 UI** | **缺失** |
| **自动更新器通知** | **缺失** |
| **外部编辑器提示** | **缺失** |

**对比：** TS `Notifications.tsx` 有完整的基于优先级的队列，12+ 个通知钩子。

**补齐计划：** `docs/ui/great/plans/plan-04-notification.md`（估算 ~750 行新代码）

---

## 6. Agent 导航 — 死代码未接入

| 功能 | 行数 | 现状 |
|------|:----:|------|
| `AgentNavigationState` 数据模型 | ~140 | **实现但死代码** — `app.rs:2` `#[allow(dead_code)]`，不在 App 结构体中 |
| `render_agent_tree()` | 内联 | **实现但从未被调用** — 无引用处 |
| Agent CRUD（列表/详情/编辑器/向导） | 25+ 文件 | **完整** — 存在于 `agents/` 目录 |
| Agent 导航页脚 | 存在 | **完整** — `agents/agent_navigation_footer.rs` |
| **Agent 树面板集成到渲染管道** | — | **缺失** — 渲染管线中无 agent 树/线程状态 |
| **协调器/队友状态面板** | — | **缺失** |
| **渲染中使用 AgentNavigationState** | — | **零引用** |

**补齐计划：** `docs/ui/great/plans/plan-05-agent-navigation.md`（估算 ~610 行新代码，主要工作是接线和样式化）

---

## 7. 语法高亮 — 0/5（最大单一渲染差距）

| 功能 | 现状 |
|------|------|
| `syntect` crate | **声明为 optional 依赖，零使用** |
| `tree-sitter` crate | **声明为 optional 依赖，零使用** |
| 代码块语言检测 | **缺失** — `Tag::CodeBlock(_)` 中用 `_` 丢弃 `info_string` |
| 令牌级着色 | **缺失** — 所有代码块使用统一 `theme.code` 样式 |
| 语言选择器 UI | **缺失** |
| 无高亮回退 | 无 — TS 有 WASM Shiki 懒加载 + plaintext 回退 |

**补齐计划：** `docs/ui/great/plans/plan-06-syntax-highlighting.md`（估算 ~600 行新代码，`syntect` feature 门控）

---

## 8. 死代码清单 — 18 个文件

| 路径 | `#[allow(dead_code)]` 数 |
|------|:-----------------------:|
| `permissions/rules/mod.rs` | 8 |
| `permissions/ask_user_question_permission_request/mod.rs` | 7 |
| `permissions/file_permission_dialog/mod.rs` | 5 |
| `permissions/bash_permission_request/mod.rs` | 2 |
| `permissions/file_edit_permission_request/mod.rs` | 2 |
| `permissions/file_write_permission_request/mod.rs` | 2 |
| `permissions/notebook_edit_permission_request/mod.rs` | 2 |
| `permissions/power_shell_permission_request/mod.rs` | 2 |
| `permissions/computer_use_approval/mod.rs` | 1 |
| `permissions/enter_plan_mode_permission_request/mod.rs` | 1 |
| `permissions/exit_plan_mode_permission_request/mod.rs` | 1 |
| `permissions/filesystem_permission_request/mod.rs` | 1 |
| `permissions/monitor_permission_request/mod.rs` | 1 |
| `permissions/review_artifact_permission_request/mod.rs` | 1 |
| `permissions/sed_edit_permission_request/mod.rs` | 1 |
| `permissions/skill_permission_request/mod.rs` | 1 |
| `permissions/web_fetch_permission_request/mod.rs` | 1 |
| `user_tool_result_message/mod.rs` | 8（每子模块一行）|
| `app/agent_navigation.rs` | 模块级 |

**代码冗余：**
- `metadata.rs`（175行）与 `render.rs`（1145行）**13 个函数完全重复**
- `metadata.rs` 中 **0 个**独有函数 — 完全冗余

---

## 9. 完整度评分对比（文档 vs 实际代码）

| 子系统 | Great 文档评分 | 代码验证结论 | 差距 |
|--------|:------------:|:----------:|:----:|
| 消息渲染 | 2.1/5 | 27/34 stub（79%），确认 2.1/5 | 一致 |
| UI 组件（设计系统） | 1.8/5 | 00 个 primitive，确认 1.8/5 | 一致 |
| 渲染子系统 | 3.2/5 | 语法高亮 0/5, Markdown 2/5 | 一致 |
| 权限与输入 | 2.7/5 | 多行缺失, 20+ 变体死代码 | 一致（输入更低 ~2/5） |
| 应用壳 | 3.5/5 | 通知 1/5, Agent 导航死代码 | 一致 |

**文档评分总体准确。** 主要验证发现：输入子系统实际评分更低（`~2/5` 而非 `2.7/5`），因为图片粘贴 stub 从未接线。

---

## 10. 优先级建议

| 优先级 | 领域 | 工作量 | 影响 | 策略 | 补齐计划 |
|--------|------|:------:|:----:|------|:--------:|
| **P0** | 语法高亮（0/5） | 大 | **最大** — 代码是核心内容 | 启用已声明的 `syntect`，添加语言检测 | `plan-06-syntax-highlighting.md` |
| **P0** | API 错误状态（0种） | 中 | **关键** — 用户看到无帮助错误 | 将 10+ 种错误状态添加到 `assistant_text_message.rs` | `plan-01-message-rendering.md` |
| **P0** | Assistant 工具状态机（1/5） | 中 | **关键** — 用户看不到进度 | 扩展 `assistant_tool_use_message.rs` | `plan-01-message-rendering.md` |
| **P0** | System 消息子类型（1/5） | 大 | **关键** — 15+ 种子类型缺失 | 按子类型分派 | `plan-01-message-rendering.md` |
| **P1** | 通知队列（1/5） | 中 | 高 — 系统状态反馈 | 新建 `NotificationQueue` + 布局插槽 | `plan-04-notification.md` |
| **P1** | 多行输入（2/5） | 大 | 高 — 用户无法多行编辑 | 用 `TextArea` 替换单行模型 | — |
| **P1** | Agent 导航接线 | 小 | 高 — 代码已写好 | 移除 `#[allow(dead_code)]`，接入渲染 | `plan-05-agent-navigation.md` |
| **P2** | 死代码清理（18 文件） | 小 | 中 — 维护卫生 | 逐个移除 `#[allow(dead_code)]`，详见 [plans/plan-08-dead-code-cleanup.md](plans/plan-08-dead-code-cleanup.md) | — |
| **P2** | `metadata.rs` 去重 | 极小 | 低 | 删除冗余 175 行 | — |
| **P2** | 权限系统接线（17 死代码模块） | 大 | 关键 | PermissionRequestRouter + 反馈 + IDE diff + Bypass 对话框，详见 [plans/plan-07-permissions-wiring.md](plans/plan-07-permissions-wiring.md) | — |
| **P2** | 设计系统 primitive | 大 | 高 — 复用基础 | 新建 Dialog/Pane/Divider 等（详见 [plans/plan-02-design-system.md](plans/plan-02-design-system.md)） | — |
| **P3** | 其余 22 个 1 星渲染器 | 小 | 低 | 逐个添加样式 | — |
