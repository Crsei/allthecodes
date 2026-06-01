# 代码拆分总览

> 更新日期: 2026-05-27
> 规则: 所有 `.rs` 文件 > 1000 行，排除 `target/` 和 `.claude/worktrees/`

---

## 已实现拆分的文件（9 个）

| # | 文件 | 原行数 | 拆分结构 | 实际状况 |
|---|------|--------|---------|---------|
| 1 | `cc-engine/src/lifecycle/deps.rs` | 3147 | → 子模块目录 (5 文件) | ✅ 已完成。修复: 移除了 dangling `mod tests;` 声明 |
| 2 | `claude-code-rs/src/ui/messages/render.rs` | 2949 | → `render/` (7 文件) | ✅ 已完成 |
| 3 | `claude-code-rs/src/app_subsystem_handlers.rs` | 2370 | → `app_subsystem_handlers/` (8 文件) | ✅ 已完成 |
| 4 | `cc-permissions/src/read_only_shell.rs` | 2320 | → `read_only_shell/commands/` (10 文件) | ✅ 已完成 |
| 5 | `cc-mcp/src/client.rs` | 2052 | → `client/` (7 文件) | ✅ 已完成 |
| 6 | `cc-permissions/src/dangerous.rs` | 1993 | → 5 子模块（有方案） | ⏳ 待执行 |
| 7 | **`cc-api/src/api/client/mod.rs`** | 1847 | → `client/{types,headers,body,model,provider,builder,messages}.rs` (7+2 子模块) | ✅ **2026-05-27 完成拆分** |
| 8 | `cc-engine/src/lifecycle/submit_message.rs` | 1668 | → `submit_message/` (5 文件) | ✅ **2026-05-27 完成拆分** |
| 9 | `cc-api/src/api/openai_compat.rs` | 1614 | → 4 子模块（有方案） | ⏳ 待执行 |
| 10 | `cc-session/src/memdir.rs` | 1612 | → `memdir/{types,index,recall,crud}.rs` + `mod.rs` | ✅ **2026-05-27 完成拆分** |
| 11 | `cc-engine/src/system_prompt.rs` | 1542 | → 4 子模块（有方案） | ⏳ 待执行 |

> **注意**: 表中 3 个拆分方案已完备但尚未执行（标注 ⏳），10 个文件已完成拆分。

---

## 待拆分文件清单（37 个）

### 优先级 B — 中等复杂度（1200–1500 行）

| # | 文件 | 行数 | 模块 | 简述 |
|---|------|------|------|------|
| 1 | `claude-code-rs/src/main.rs` | 1461 | bin | 程序入口（CLI 参数解析、生命周期 A/B/I、headless 启动） |
| 2 | `cc-tools/src/tool_search.rs` | 1440 | tools | Agent 工具搜索（类似子 Agent 的搜索工具实现） |
| 3 | `claude-code-rs/src/ui/app.rs` | 1406 | TUI | TUI 应用主逻辑（App struct、事件循环、状态管理） |
| 4 | `cc-types/src/hooks.rs` | 1353 | types | Hooks 类型定义（PreToolUse / PostToolUse / Notification 等） |
| 5 | `cc-engine/src/query/loop_helpers.rs` | 1341 | engine | 查询循环辅助函数（tool_use 处理、重试、压缩触发） |
| 6 | `cc-query/src/loop_helpers.rs` | 1337 | query | 查询循环辅助（engine/query 分离后的副本） |
| 7 | `cc-skills/src/lib.rs` | 1279 | skills | 技能系统（内置技能 + 用户自定义技能加载/执行） |
| 8 | `worktree/src/tool.rs` | 1276 | worktree | Worktree 工具实现（EnterWorktree / ExitWorktree） |
| 9 | `cc-tools/src/fs/file_read.rs` | 1275 | tools | 文件读取工具（Read tool，PDF/图片/Notebook 支持） |
| 10 | `claude-code-rs/src/ui/app/input.rs` | 1269 | TUI | TUI 输入处理（键盘事件映射、输入框、自动补全） |
| 11 | `cc-commands/src/lib.rs` | 1262 | commands | 斜杠命令系统入口（命令注册、路由、执行） |
| 12 | `cc-permissions/src/decision.rs` | 1224 | permissions | 权限决策引擎（allow/deny/ask 规则匹配） |
| 13 | `cc-api/src/api/google_provider.rs` | 1193 | api | Google Gemini API 适配层 |
| 14 | `cc-ipc-protocol/src/subsystem_events.rs` | 1172 | ipc | 子系统事件协议类型（LSP/MCP/Plugin/Skill/IDE 事件） |
| 15 | `cc-lsp-service/src/recommendation.rs` | 1148 | lsp | LSP 推荐逻辑（代码补全建议排序与过滤） |
| 16 | `cc-tools/src/fs/file_edit.rs` | 1135 | tools | 文件编辑工具（Edit tool，diff 应用） |
| 17 | `cc-commands/src/plugin_cmd.rs` | 1131 | commands | 插件命令处理（plugin install/uninstall/list 等） |
| 18 | `cc-session/src/storage.rs` | 1118 | session | 会话持久化存储（JSONL 读写、压缩、索引） |
| 19 | `cc-lsp-service/src/client.rs` | 1105 | lsp | LSP 客户端实现（进程管理、协议通信、能力协商） |
| 20 | `cc-engine/src/agent/supervisor.rs` | 1104 | engine | Agent Supervisor（子 Agent 生命周期管理） |
| 21 | `cc-commands/src/login.rs` | 1097 | commands | 登录命令（OAuth 流程、API Key 设置、认证状态管理） |
| 22 | `cc-mcp/src/auth.rs` | 1092 | mcp | MCP OAuth 认证流程（设备码/授权码流程） |

### 优先级 C — 较小但仍超阈值（1000–1100 行）

| # | 文件 | 行数 | 模块 | 简述 |
|---|------|------|------|------|
| 23 | `cc-lsp-service/src/mod.rs` | 1085 | lsp | LSP 服务主模块（语言服务生命周期管理） |
| 24 | `cc-daemon/src/process_state.rs` | 1070 | daemon | 进程状态管理（Daemon 状态机） |
| 25 | `claude-code-rs/src/ui/app/render.rs` | 1063 | TUI | TUI App 渲染逻辑 |
| 26 | `cc-services/src/agent_definitions/mod.rs` | 1061 | services | Agent 定义与注册 |
| 27 | `cc-shell-command/src/heredoc.rs` | 1051 | shell | Heredoc 解析（`<<EOF` 语法处理） |
| 28 | `cc-tools/src/plan_mode.rs` | 1047 | tools | Plan Mode 工具（EnterPlanMode / ExitPlanMode） |
| 29 | `claude-code-rs/src/ui/messages/attachment_message.rs` | 1029 | TUI | 附件消息渲染 |
| 30 | `cc-commands/src/schedule.rs` | 1015 | commands | 定时命令（CronCreate/Delete/List） |
| 31 | `cc-teams/src/runner.rs` | 1014 | teams | Team Runner（Team Memory 代理运行时） |
| 32 | `claude-code-rs/src/ui/permissions/permission_request_router.rs` | 1012 | TUI | 权限请求路由（TUI 中权限弹窗分发） |

### 测试文件（> 1000 行，优先级低）

| # | 文件 | 行数 | 模块 | 简述 |
|---|------|------|------|------|
| 33 | `cc-api/src/api/client/tests.rs` | 2769 | api | API 客户端单元测试 |
| 34 | `cc-engine/src/query/loop_tests.rs` | 2451 | engine | 查询循环测试 |
| 35 | `cc-query/src/loop_tests.rs` | 2384 | query | 查询循环测试（engine/query 分离副本） |
| 36 | `cc-engine/src/lifecycle/deps/tests/mod.rs` | 1314 | engine | deps 模块测试（已含在 deps 拆分计划内） |
| 37 | `claude-code-rs/src/ui/app/tests.rs` | 1002 | TUI | TUI App 测试 |

---

## 统计

| 分类 | 文件数 | 总行数 |
|------|--------|--------|
| 已实现拆分 | 6 | 14,868 |
| 有计划待执行 | 5 | 8,429 |
| 待拆分 B（1200–1500 行） | 22 | 27,787 |
| 待拆分 C（1000–1100 行） | 10 | 10,534 |
| 测试文件 | 5 | 10,019 |
| **合计** | **48** | **71,637** |

---

## 更新日志

| 日期 | 变更 |
|------|------|
| 2026-05-27 | `submit_message.rs` 拆分完成（`mod.rs` + 4 个职责子模块），保持 `QueryEngine::submit_message` 对外入口不变 |
| 2026-05-27 | `client/mod.rs` 拆分完成（7 子模块）；修复 `deps` 丢失 `tests.rs` 的编译错误；更新状态表反映实际进度 |
| 2026-05-23 | 初始版本，记录 11 个拆分计划 |
