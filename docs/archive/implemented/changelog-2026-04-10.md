# 2026-04-10 开发日志

共 **80 个 commit**，涵盖 6 个主要方向。

---

## 1. KAIROS 常驻助手模式 (~30 commits)

从零搭建了完整的 KAIROS daemon 架构，分 5 个阶段推进：

- **基础设施**: Feature Gate 系统 (`FEATURE_*` 环境变量)、AppState 扩展 (kairos/brief/assistant 字段)、QuerySource 新增 ProactiveTick/WebhookEvent/ChannelNotification
- **工具 & 命令**: SleepTool (tick 休眠控制)、BriefTool (结构化输出)、`/brief` `/sleep` `/assistant` `/daemon` `/notify` `/channels` `/dream` 等命令
- **Daemon 服务**: axum HTTP server (127.0.0.1:19836)、SSE 事件流、12 个 REST 端点、tick 循环 (30s)
- **外部集成**: Webhook 签名验证 (GitHub HMAC-SHA256 + Slack)、推送通知 (Windows Toast + Webhook)、MCP Channel 通知协议、ChannelManager + allowlist
- **前端对接**: DaemonClient (HTTP/SSE)、IPC 协议扩展 (Brief/Autonomous/Notification 消息)
- **持久化**: 每日日志记忆系统 (`~/.cc-rust/logs/YYYY/MM/DD.md`)

### 关键 commits

- `b53189b` feat(kairos): add feature gate system with FEATURE_* env vars
- `db92170` feat(kairos): add ProactiveTick/WebhookEvent/ChannelNotification to QuerySource
- `ab74755` feat(kairos): extend AppState with kairos/brief/assistant fields
- `6da1e37` feat(kairos): add SleepTool for proactive tick control
- `f05dba0` feat(kairos): add BriefTool for structured output in Brief mode
- `8b285fb` docs: update WORK_STATUS, USAGE_GUIDE, CLAUDE.md with OAuth login
- `adcce90` feat(kairos): add /brief /sleep /assistant /daemon commands
- `d69647b` feat(kairos): add daemon module with DaemonState
- `db95d60` feat(kairos): add daemon HTTP server with routes and SSE
- `713bd13` feat(kairos): add --daemon entry point with HTTP server and tick loop
- `e97b297` feat(kairos): add webhook signature verification (GitHub + Slack)
- `37b0120` feat(kairos): add push notification system (Toast + webhook)
- `b50e94b` feat(kairos): add /notify /channels /dream commands
- `4ca7edf` feat(kairos): add ChannelManager with allowlist filtering
- `5a509e6` feat(kairos): extend IPC protocol with Brief/Autonomous/Notification messages
- `b9925df` feat(kairos): add MCP channel notification protocol extension
- `f8f1aa3` feat(kairos): add DaemonClient for ink-terminal → daemon HTTP/SSE
- `762fdd8` feat(kairos): add daily log memory system

---

## 2. OAuth 认证 (~10 commits)

完整实现 OAuth PKCE 登录流程：

- PKCE 工具 (code_verifier + code_challenge + state)
- OAuth 配置常量 (platform.claude.com endpoints)
- HTTP Client (token exchange + refresh + create_api_key)
- `/login-code` 两步登录流程、`/logout` 清除 OAuth 状态
- 与 `resolve_auth()` 完整集成，自动刷新过期 token

### 关键 commits

- `7b615bf` feat(auth): add PKCE utilities for OAuth login
- `abd34d0` feat(auth): add OAuth config constants and URL builder
- `4a49ee5` feat(auth): add OAuth HTTP client for token exchange and API key creation
- `eee85d7` feat(auth): extend StoredToken with scopes and oauth_method fields
- `b6d7b45` feat(auth): implement OAuth auto-refresh and browser interface stub
- `4183013` feat(auth): wire OAuth into resolve_auth() and remove old stubs
- `24de49a` feat(commands): add /login-code for two-step OAuth flow
- `26eb506` feat(commands): update /logout to clear OAuth state via oauth_logout()
- `82ca784` chore(auth): resolve all OAuth warnings

---

## 3. LSP 服务 (~8 commits)

从 stub 到完整实现的 LSP 集成：

- JSON-RPC 传输层 (Content-Length framing)
- lsp-types 类型转换
- LspClient (生命周期管理 + 请求路由 + 文件同步)
- 9 个操作全部从 stub 替换为真实实现
- 集成测试验证

### 关键 commits

- `cb069c5` deps: add lsp-types 0.97 for LSP protocol types
- `efbd58b` feat(lsp): add JSON-RPC transport layer with Content-Length framing
- `5539bbe` feat(lsp): add lsp-types to internal type conversions
- `cef63c1` feat(lsp): add LSP client with lifecycle, request routing, file sync
- `2016336` feat(lsp): wire 9 operations to LspClient with lazy server startup
- `205743a` test(lsp): add integration tests verifying stubs are replaced
- `0637a58` fix(lsp): remove allow(unused) and fix all warnings

---

## 4. 后台代理 (Background Agents, ~7 commits)

实现 `run_in_background` 功能：

- 核心类型定义 (`BackgroundAgentResult`, channel 类型)
- `tokio::spawn` + mpsc channel 异步执行
- `bg_agent_tx` 穿透 ToolUseContext 和 QueryDeps
- headless 事件循环中 `select!` 多路复用接收结果
- 单元测试 + 集成测试

### 关键 commits

- `c679b32` feat(agent): add background agent core types
- `4a9f0b2` feat(agent): thread bg_agent_tx through ToolUseContext and QueryDeps
- `8624fed` feat(agent): inject background agent results at query loop turn start
- `47926a8` feat(agent): implement run_in_background via tokio::spawn + mpsc channel
- `5f72eea` feat(agent): wire background agent channel into headless event loop with select!
- `687718f` test(agent): add unit and integration tests for background agents
- `e4f9ad6` fix(agent): address code review findings for background agents

---

## 5. Hooks 系统 (~10 commits)

生命周期钩子全面实装：

- 事件钩子: SessionStart/End, UserPromptSubmit, Notification, PermissionRequest/Denied, PreCompact/PostCompact, StopFailure
- Hook 配置从 MergedConfig 注入 AppState
- 工具执行管线集成
- StreamingToolExecutor hook-aware 执行编排

### 关键 commits

- `b0879e1` feat(hooks): add hooks field to AppState, populate from MergedConfig
- `3a575e3` feat(hooks): add generic run_event_hooks() dispatcher, make HookOutput public
- `1aa5a85` feat(hooks): wire hook configs into tool execution pipeline
- `abfaa02` feat(hooks): wire stop hooks to real config, remove stubs
- `b5c6bf1` feat(hooks): fire UserPromptSubmit hook before query start
- `d966a14` feat(hooks): fire SessionStart and SessionEnd hooks
- `6d60ddd` feat(hooks): fire Notification, PermissionRequest, and PermissionDenied hooks
- `90deb0c` feat(hooks): fire PreCompact, PostCompact, StopFailure, and remaining lifecycle hooks
- `5379c6b` feat: expand StreamingToolExecutor with hook-aware execution and orchestration

---

## 6. 其他改进

- **Web 搜索**: Tavily 作为新 provider (`710c6b5`)、SearchCache 缓存 + MAX_CACHE_ENTRIES 淘汰策略 (`e4ec839`, `fac2d16`)
- **Git 上下文**: 注入 branch/user/status/commits 到系统提示词 (`831d026`)
- **IPC 增强**: terminal resize 支持、content block 重放 (`5379c6b`, `f56b043`)
- **模型定价**: pricing 系统、`/model-add` 命令、OpenAI streaming usage 修复 (`3a575e3`)
- **代码质量**: 大量 warning 清理、`allow(dead_code)` 移除、PTY 测试改进
- **文档**: KAIROS 设计文档、实现计划、架构文档更新
