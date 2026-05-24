# Claude Code Rust 重写计划

> **阶段 (2026-04-22)**：已进入**全量构建 (Full Build)**。原 Lite 缩减清单重新解读为"待补齐 TODO"，详见 [`../CLAUDE.md`](../CLAUDE.md) 顶部"当前阶段"说明。
>
> 最后更新: 2026-04-02（历史部分仍按当时状态）
> 已完成模块: [`COMPLETED_FULL.md`](archive/COMPLETED_FULL.md) (完整实现) | [`COMPLETED_SIMPLIFIED.md`](archive/COMPLETED_SIMPLIFIED.md) (进入全量构建后视作待补齐清单)
> 当前仍未补齐的注意事项、设计限制与缩减实现汇总见 [`IMPLEMENTATION_GAPS.md`](IMPLEMENTATION_GAPS.md)

## 原始项目概况

- **语言**: TypeScript/React (Ink 终端渲染)
- **运行时**: Bun
- **文件数**: ~1896 个 .ts/.tsx 文件, 35 个顶级子目录
- **代码行数**: ~225,000 行 (含 UI 框架)
- **核心架构**: Generator-based 流式查询状态机 + 工具系统 + 终端 UI

## Rust 实现现状

- **文件数**: 204 个 .rs 文件, 22 个顶级子目录
- **代码行数**: ~49,187 行 (占 TS 的 ~22%)
- **测试数**: 819 个测试
- **工具数**: 30 工具 + MCP 动态工具
- **命令数**: 74 个斜杠命令
- **目录覆盖率**: 22/35 顶级目录已存在 (~63%)

---

## 状态图例

| 标记 | 含义 |
|------|------|
| ✅ | 完整实现 |
| 🔧 | 脚手架 / 接口保留 |
| 📦 | 仅模块声明 |
| ❌ | 缺失 |

---

## 参考文档

| 文档 | 内容 |
|------|------|
| [`COMPLETED_FULL.md`](archive/COMPLETED_FULL.md) | 已完成的完整实现模块 (Phase 0-14B) |
| [`COMPLETED_SIMPLIFIED.md`](archive/COMPLETED_SIMPLIFIED.md) | 已完成但大幅简化的模块 (缩减率/缺失功能) |
| [`MODULE_SIMPLIFICATION.md`](archive/MODULE_SIMPLIFICATION.md) | TS vs Rust 全模块简化率分析 |
| [`LIFECYCLE_STATE_MACHINE.md`](archive/LIFECYCLE_STATE_MACHINE.md) | 生命周期状态机 (Phase A-I) |
| [`QUERY_ENGINE_SESSION_LIFECYCLE.md`](archive/QUERY_ENGINE_SESSION_LIFECYCLE.md) | 查询引擎会话生命周期 |
| [`TOOL_EXECUTION_STATE_MACHINE.md`](archive/TOOL_EXECUTION_STATE_MACHINE.md) | 工具执行管线 |
| [`COMPACTION_RETRY_STATE_MACHINE.md`](archive/COMPACTION_RETRY_STATE_MACHINE.md) | 压缩重试状态机 |
| [`STRUCTURE_DIFF.md`](archive/STRUCTURE_DIFF.md) | TS vs Rust 目录结构对比 |
| [`AGENT_TEAMS_SPEC.md`](AGENT_TEAMS_SPEC.md) | 多代理 Swarm 系统实现规格 |

---

## 已完成 Phase 总览

| Phase | 名称 | 状态 | 详见 |
|-------|------|------|------|
| Phase 0 | 类型基础 | ✅ 100% (6/6) | COMPLETED_FULL.md |
| Phase 1 | 状态机骨架 | ✅ 100% (12/12) | COMPLETED_FULL.md |
| Phase 2 | 本地工具系统 | ✅ 100% (14/14) | COMPLETED_FULL.md + COMPLETED_SIMPLIFIED.md |
| Phase 3 | 权限与配置 | ✅ 100% (5/5) | COMPLETED_FULL.md |
| Phase 4 | 上下文管理 | ✅ 100% (9/9) | COMPLETED_FULL.md |
| Phase 5 | 终端 UI | ✅ 100% (8/8) | COMPLETED_SIMPLIFIED.md (94% 框架缩减) |
| Phase 6 | 会话持久化 | ✅ 100% (3/3) | COMPLETED_FULL.md |
| Phase 7 | 命令系统 | ✅ 100% (18/18) | COMPLETED_FULL.md |
| Phase 8 | 高级工具 | ✅ 100% (4/4) | COMPLETED_FULL.md |
| Phase 9 | API 客户端 | ✅ 活跃 + 📦 存根 | COMPLETED_FULL.md + COMPLETED_SIMPLIFIED.md |
| Phase 10 | 认证 | ✅ 活跃 + 📦 存根 | COMPLETED_SIMPLIFIED.md |
| Phase 11 | MCP 协议 | ✅ 100% (3/3) | COMPLETED_FULL.md |
| Phase 12 | 网络工具 | ✅ 核心完成 | COMPLETED_FULL.md |
| Phase 13 | 远程/遥测 | 📦 接口保留 | COMPLETED_SIMPLIFIED.md |
| Phase 14A | 本地补充 | ✅ 100% (10/10) | COMPLETED_FULL.md |
| Phase 14B | 命令 batch 1+2 | ✅ 100% (20/20) | COMPLETED_FULL.md |
| Phase 14B-3 | 命令 batch 3 | ✅ 100% (30/30) | 新增 20 commands + 改进 |
| Phase 14C | 缺失工具补全 | ✅ 100% (lite) | +5 tools: PowerShell, Config, REPL, StructuredOutput, SendUserMessage |
| Phase 14D | 服务补全 | ✅ 100% (lite) | +4 services: ToolUseSummary, SessionMemory, PromptSuggestion, LSP Lifecycle |

---

## 剩余工作

### Phase 14B-3: 第三批命令 (待做)

> 来源: REWRITE_PLAN 原有列表 + UNIMPLEMENTED_CHECKLIST 新增

**已规划 (原有列表):**

```
/add-dir, /agents, /color, /copy, /doctor, /feedback,
/ide, /init, /keybindings, /mcp, /plugin,
/privacy-settings, /rewind, /sandbox, /skills,
/status, /tag, /tasks, /theme, /think-back, /upgrade, /vim, /voice
```

**新增 (UNIMPLEMENTED_CHECKLIST):**

```
/commit-push-pr, /security-review, /pr-comments, /advisor,
/btw, /insights, /output-style, /extra-usage, /passes,
/reload-plugins, /statusline, /ultrareview, /assistant, /brief,
/proactive, /force-snip, /fork, /peers, /buddy, /subscribe-pr,
/torch, /ultraplan, /workflows, /install-github-app,
/install-slack-app, /rate-limit-options, /thinkback-play
```

**暂不实现 — 远程/浏览器/桌面/移动端命令:**

```
/bridge, /chrome, /desktop, /mobile, /remote-env, /web-setup,
/remote-control, /release-notes, /stickers, /terminal-setup, /usage
```

**内部/Ant-Only 命令 (不实现):**

```
/agents-platform, /ant-trace, /autofix-pr, /backfill-sessions,
/break-cache, /bridge-kick, /bughunter, /ctx-viz, /debug-tool-call,
/env, /good-claude, /init-verifiers, /issue, /mock-limits,
/oauth-refresh, /onboarding, /perf-issue, /reset-limits,
/share, /summary, /teleport, /heapdump
```

---

### Phase 14C: 缺失工具补全

**待完成工具:**

| # | 工具 | 预估行数 | 优先级 | 说明 |
|---|------|---------|--------|------|
| P14C.1 | ✅ SendMessage | ~280 | 高 | Agent 间通信 (含 teams/mailbox IPC) |
| P14C.6 | PowerShell | ~100 | 中 | Windows 支持 |
| P14C.7 | Sleep | ~20 | 低 | tokio::time::sleep |
| P14C.8 | SendUserMessage (Brief) | ~30 | 低 | 输出简化 |
| P14C.9 | Config | ~50 | 低 | 运行时设置修改 |
| P14C.10 | REPL | ~100 | 低 | 嵌入式 REPL |
| P14C.13 | RemoteTrigger | ~80 | 低 | 需 network |
| P14C.14 | CronCreate/Delete/List | ~120 | 低 | 需 network |
| P14C.15 | TodoWrite | 中 | — | 任务管理写入 |
| P14C.16 | SnipTool | 中 | — | 手动历史裁剪 |
| P14C.17 | StructuredOutput | 中 | — | 结构化输出 |
| P14C.18 | WebBrowser | 低 | — | 浏览器交互 (需 network) |
| P14C.19 | Workflow | 低 | — | 工作流执行 |
| P14C.20 | TerminalCapture | 低 | — | 终端截图 |
| P14C.21 | McpAuthTool | 低 | — | MCP 认证 (需 network) |
| P14C.22 | Monitor | 低 | — | 进程监控 |
| P14C.23 | ListPeers | 低 | — | 对等节点列表 |

**内部/不实现工具:**

```
✅ TeamCreate, ✅ TeamDelete (已实现), CtxInspect, OverflowTest,
VerifyPlanExecution, Tungsten, SuggestBackgroundPR,
SendUserFile, PushNotification, SubscribePR
```

---

### Phase 14D: services 子模块补全 (中优先级)

| # | 模块 | 对应 TS | 预估行数 | 说明 |
|---|------|---------|---------|------|
| P14D.1 | LSP 服务完善 | `services/lsp/` | ~300 | 真实 LSP 服务器生命周期 |
| P14D.4 | SessionMemory | `services/SessionMemory/` | ~100 | 会话记忆服务 |
| P14D.5 | 提示建议 | `services/PromptSuggestion/` | ~80 | 输入补全 |
| P14D.6 | 工具摘要 | `services/toolUseSummary/` | ~60 | 工具使用统计 |

---

### Phase 14E: 网络/远程目录 — ❌ 暂不实现

> **决策 (2026-04-01):** 远程会话、浏览器桥接、桌面端、移动端相关功能暂不实现。

| # | 模块 | 对应 TS | 文件数 | 说明 |
|---|------|---------|--------|------|
| P14E.1 | bridge | `bridge/` | 31 | 远程控制桥接 |
| P14E.2 | cli transports | `cli/transports/` | 6 | SSE, WebSocket, Worker |
| P14E.3 | ✅ teams (核心) | `teams/` | 10 | 多 Agent 协调: types/identity/mailbox/protocol/backend/in_process/runner/context/helpers/constants |
| P14E.4 | server | `server/` | 3 | 服务器模式 |
| P14E.5 | remote 扩展 | `remote/` | 4 | 云容器 (CCR) |
| P14E.6 | OAuth 流程 | `services/oauth/` | ~5 | 📦 接口已保留 |
| P14E.7 | 远程设置同步 | `services/remoteManagedSettings/` | ~5 | MDM + 同步 |
| P14E.8 | 遥测网络发送 | `services/analytics/` | ~5 | Datadog/1P 管线 |
| P14E.9 | desktop | 桌面端集成 | — | 桌面应用 |
| P14E.10 | mobile | 移动端集成 | — | 移动端 |

---

### 简化模块优先补全

> 详见 [`COMPLETED_SIMPLIFIED.md`](archive/COMPLETED_SIMPLIFIED.md) 中的"优先补全建议"

| 优先级 | 模块 | 缺失功能 | 状态 |
|--------|------|---------|------|
| ~~高~~ | ~~FileEditTool~~ | ~~fuzzy 匹配~~ | ✅ 已补全 (230→386 行) |
| ~~高~~ | ~~BashTool~~ | ~~输出截断策略~~ | ✅ 已补全 (199→430 行) |
| ~~高~~ | ~~FileReadTool~~ | ~~PDF/图片支持~~ | ✅ 已补全 (236→743 行) |
| ~~中~~ | ~~GrepTool~~ | ~~ripgrep 调用~~ | ✅ 已补全 (185→371 行) |
| ~~中~~ | ~~AgentTool~~ | ~~worktree 隔离~~ | ✅ 已补全 (322→789 行) |

---

## 统计总览

```
完成状态:

  Phase 0-8    核心本地      ██████████ 100%  (已完成, 见 COMPLETED_*.md)
  Phase 9-11   API/认证/MCP  ██████████ 100%  (活跃路径完成)
  Phase 12     网络工具      ████████░░  80%  (LSP 服务待完善)
  Phase 13     远程/遥测     ░░░░░░░░░░   0%  (暂不实现)
  Phase 14B    命令 (lite)   ██████████ 100%  (26 命令, 含 extra-usage + rate-limit-options)
  Phase 14C    工具 (lite)   ██████████ 100%  (13 工具, +5: PowerShell/Config/REPL/StructuredOutput/SendUserMessage)
  Phase 14D    服务 (lite)   ██████████ 100%  (+4: ToolUseSummary/SessionMemory/PromptSuggestion/LSP Lifecycle)
  Phase 14E    网络/远程      ░░░░░░░░░░   0%  (暂不实现)

  Lite 版文件总数: 122 .rs 文件
  Lite 版代码行数: ~31,554 行 (占 TS ~225K 的 14%)
  Lite 版测试数量: 537 个 (偶尔 1-2 个 env/global 竞态)
  Lite 版命令覆盖: 26 个斜杠命令
  Lite 版工具覆盖: 13 个工具
  Lite 版服务覆盖: 4 个服务 (tool_use_summary, session_memory, prompt_suggestion, lsp_lifecycle)
```

---

## 目录结构映射

| TS 原始位置 | Rust 新位置 | 变化类型 |
|---|---|---|
| `services/api/` | `api/` | 提升为顶级 |
| `services/analytics/` | `analytics/` | 提升为顶级 |
| `services/compact/` | `compact/` | 提升为顶级 |
| `services/mcp/` | `mcp/` | 提升为顶级 |
| `services/oauth/` + `utils/secureStorage/` | `auth/` | 合并提升 |
| `utils/permissions/` | `permissions/` | 提升为顶级 |
| `utils/settings/` + config 相关 | `config/` | 合并提升 |
| `components/` + `ink/` | `ui/` | 合并简化 |
| `memdir/` + 会话相关 | `session/` | 合并重命名 |
| `QueryEngine.ts` + `query.ts` | `engine/` | 提取为模块 |
| `state/` | `types/app_state.rs` | 合入 types |
| `entrypoints/cli.tsx` + `main.tsx` | `main.rs` | 合并 |

## Rust 与 TypeScript 关键映射

| TypeScript 概念 | Rust 对应 | 所在文件 |
|----------------|-----------|---------|
| `AsyncGenerator<T>` | `impl Stream<Item = T>` | query/loop_impl.rs |
| `interface Tool` | `trait Tool` | types/tool.rs |
| `type Message = A \| B \| C` | `enum Message { A(..), B(..), C(..) }` | types/message.rs |
| `ToolUseContext` (大对象) | `struct ToolUseContext` (Arc 共享) | types/tool.rs |
| `DeepImmutable<AppState>` | `Arc<RwLock<AppState>>` | engine/lifecycle.rs |
| `AbortController` | `tokio::sync::watch<bool>` | utils/abort.rs |
| `feature('FLAG')` | `#[cfg(feature = "flag")]` | Cargo.toml |
| `z.infer<Schema>` (Zod) | `#[derive(Deserialize)]` struct | 各工具模块 |
| React/Ink (UI) | ratatui + crossterm | ui/ |

## 开发原则

1. **本地优先**: core 状态机可完全离线运行
2. **可测试**: 774 测试覆盖所有核心路径，QueryDeps trait 允许完整 mock
3. **增量构建**: 每个 Phase 可独立编译和测试
4. **Feature gates**: 网络功能通过 Cargo features 按需启用
5. **Generator → Stream**: `async_stream::stream!` 宏实现 TypeScript 的 yield 语义
6. **扁平化模块**: Rust 版本将 TS 深层嵌套提升为顶级模块
