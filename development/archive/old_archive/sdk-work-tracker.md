# cc-rust SDK 功能对标工作追踪

> 对标: OpenAI Codex SDK (`docs/Codex_SDK_Features.md`)
> 基线日期: 2026-04-10

---

## 状态图例

| 标记 | 含义 |
|------|------|
| ✅ | 已实现且已接入 |
| ⚠️ | 代码存在但未接入/未强制 |
| 🔨 | 进行中 |
| ❌ | 未实现 |

---

## P0 — 安全加固 (不做就不能给外部用)

| # | 功能 | 状态 | Spec | 说明 |
|---|------|------|------|------|
| P0-1 | 危险命令拦截接入 Bash 执行管道 | ✅ | [design](superpowers/specs/2026-04-10-p0-security-hardening-design.md) | `execution.rs` Stage 3c.2, 8 tests |
| P0-2 | 路径边界检查接入 Write/Edit 工具 | ✅ | 同上 | `execution.rs` Stage 3c.3, 8 tests |
| P0-3 | Plan 模式写入拦截 | ✅ | 同上 | `execution.rs` Stage 3c.1, 8 tests |

## P1 — 实用性

| # | 功能 | 状态 | Spec | 说明 |
|---|------|------|------|------|
| P1-1 | Git 上下文自动注入 system prompt | ✅ | — | `system_prompt.rs` git_status_section(), porcelain-style snapshot |
| P1-2 | `--ephemeral` 临时会话 | ❌ | — | 不持久化的一次性会话 |
| P1-3 | Web 搜索缓存层 | ✅ | — | 进程级内存 TTL 缓存, 默认 5min, CC_RUST_SEARCH_CACHE_TTL 可配置 |
| P1-4 | LSP 方法实现 | ✅ | — | 9/9 方法已通过 LspClient 完整实现 |

## P2 — 生态扩展

| # | 功能 | 状态 | Spec | 说明 |
|---|------|------|------|------|
| P2-1 | MCP 服务器模式 | ❌ | — | 仅做客户端，需暴露工具给外部 MCP 客户端 |
| P2-2 | JSON-RPC v2 App-Server | ❌ | — | IDE 集成基础 (thread/start, turn/start, approval/*) |
| P2-3 | OS 级沙盒 (Windows Restricted Token) | ❌ | — | Bash 工具零隔离 |
| P2-4 | 网络访问控制 | ❌ | — | 无 `--no-network` / 代理 / 域名白名单 |
| P2-5 | 沙盒模式 (read-only / workspace-write / full-access) | ❌ | — | Codex 三档控制 |

## P3 — 功能完善

| # | 功能 | 状态 | Spec | 说明 |
|---|------|------|------|------|
| P3-1 | 会话回滚 | ❌ | — | 仅覆盖保存，无版本/快照 |
| P3-2 | Tree-sitter AST 感知编辑 | ❌ | — | 纯文本行级编辑 |
| P3-3 | API 级 JSON Schema 约束输出 | ❌ | — | StructuredOutput 是后处理 |
| P3-4 | 配置 Schema 自动生成 | ❌ | — | 硬编码验证 |
| P3-5 | Web 搜索 live/cached 模式切换 | ❌ | — | 仅 Brave API 直连 |

---

## 已完成的对标项

| 功能 | 完成日期 | 说明 |
|------|----------|------|
| P0 安全加固 (Stage 3c) | 2026-04-10 | 危险命令拦截 + 路径边界强制 + Plan 模式写入拦截，13 tests |
| P1-1 Git 上下文注入 system prompt | 2026-04-10 | git_status_section() — branch/user/status/commits porcelain snapshot |
| P1-3 Web 搜索缓存层 | 2026-04-11 | SearchCache — query|max_results|provider key, 5min default TTL, 128 entry cap |
| Headless IPC 协议 | 2026-04-10 | 补完 SlashCommand/ToolResult/Suggestions/Resize/SessionResume |
| 28 个内置工具 | baseline | Bash/Read/Write/Edit/Grep/Glob/Agent/Skill/LSP/Tasks/Web... |
| MCP 客户端 | baseline | stdio + SSE，多服务器管理 |
| 权限规则引擎 | baseline | Deny > Allow > Ask，glob 匹配 |
| 多 API 提供商 | baseline | Anthropic/OpenAI/Google/Azure |
| 多语言 SDK | baseline | TypeScript + Python |
| Hook 系统 | baseline | PreToolUse/PostToolUse/Stop |
| 技能系统 | baseline | 内置 + 用户 + 插件 |
| 上下文压缩 | baseline | compact 管道 |
| 多 Agent Teams | baseline | 实验性，feature-gated |
