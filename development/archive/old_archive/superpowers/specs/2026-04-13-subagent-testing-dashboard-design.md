# Subagent 测试与可视化仪表盘设计

日期: 2026-04-13
状态: 已批准

## 概述

通过三个互补手段测试和观测 cc-rust 的 subagent（Agent tool）运行情况：

1. **Headless IPC e2e 测试** — 复用现有 `--headless` JSONL 协议测试链路，验证 subagent 端到端正确性
2. **Dashboard Companion** — 浏览器实时展示 subagent 生命周期事件，辅助本地开发与人工排查
3. **结构化 subagent 事件流** — Rust 端额外输出机器可读事件，供 dashboard 消费；现有 tracing 日志继续保留给人类调试

## 1. 整体架构

```
┌──────────────────────────────────────────────────────────────┐
│                       cc-rust 进程                            │
│                                                              │
│  QueryEngine (parent)                                        │
│    ├─ Agent Tool → spawn child QueryEngine (subagent)        │
│    ├─ tracing::debug!/info! → .logs/cc-rust.log.YYYY-MM-DD   │
│    └─ append NDJSON event → .logs/subagent-events.ndjson     │
│                                                              │
│  仅在交互式 TUI + FEATURE_SUBAGENT_DASHBOARD=1 时：           │
│    ├─ spawn companion                                         │
│    └─ open browser (可配置关闭)                               │
└─────────────────┬────────────────────────────────────────────┘
                  │ fs.watch() + stat polling fallback
                  ▼
┌──────────────────────────────────────────────────────────────┐
│            Bun Companion (ui/subagent-dashboard/)             │
│                                                              │
│  event-watcher.ts                                            │
│    ├─ watch .logs/subagent-events.ndjson                     │
│    ├─ 逐行解析 NDJSON                                         │
│    └─ 维护 agent state + recent events                        │
│                                                              │
│  server.ts (Bun.serve 127.0.0.1:19838)                       │
│    ├─ GET /           → dashboard.html                       │
│    ├─ GET /events     → SSE stream (subagent 事件)            │
│    └─ GET /api/state  → 当前快照 (JSON)                       │
│                                                              │
│  dashboard.html (单页)                                        │
│    ├─ fetch(/api/state) 初始化                                │
│    ├─ EventSource(/events) 实时渲染                           │
│    ├─ 生命周期视图                                             │
│    ├─ 事件流视图                                               │
│    └─ 活跃 subagent 列表                                       │
└──────────────────────────────────────────────────────────────┘
                  │
                  ▼ Chrome DevTools MCP
┌──────────────────────────────────────────────────────────────┐
│                  自动化验证（手动 / opt-in）                   │
│                                                              │
│  take_screenshot → 仪表盘渲染正确                             │
│  evaluate_script → 检查 DOM 状态 / 事件计数                   │
│  wait_for → 等待特定 subagent 事件出现                         │
└──────────────────────────────────────────────────────────────┘
```

### 关键设计决策

- v1 dashboard **不解析** `tracing_subscriber` 输出文本；机器消费事件统一来自 `<cwd>/.logs/subagent-events.ndjson`
- 现有 tracing 文件 `.logs/cc-rust.log.YYYY-MM-DD` 继续保留，用于人工调试，不作为 UI 数据源
- v1 **不新增** `--log-dir` / `CC_RUST_LOG_DIR`，先与当前项目内 `.logs/` 行为对齐；后续如引入统一日志目录配置，再扩展 companion
- companion 监听地址固定为 `127.0.0.1:19838`
- companion 仅在交互式 TUI 模式下启用；`--headless`、`--print`、`--daemon`、测试进程都不自动拉起 dashboard

### v1 非目标

- 不通过正则解析 tracing 文本日志中的 `target` / `line number` / free-form message
- 不展示 subagent 内部每一个 `Bash` / `Read` / `Glob` 的细粒度时间线；v1 只展示生命周期级事件
- 不支持 `run_in_background=true` 与 `isolation="worktree"` 的组合能力；该组合当前保持降级/告警语义
- 不把 Chrome DevTools MCP 校验作为默认 CI gate；它是人工探索与回归辅助

## 2. Headless IPC e2e 测试

### 测试文件

复用现有 `tests/e2e_terminal/` 测试目标，不新增独立 harness：

- `tests/e2e_terminal/helpers.rs` — 继续作为 headless spawn / send / collect helper
- `tests/e2e_terminal/subagent.rs` — 新增 subagent 专项测试模块
- `tests/e2e_terminal/main.rs` — 追加 `mod subagent;`

### 测试方式

通过 `--headless` 模式 + JSONL 协议驱动。subagent 行为是后端逻辑，主断言应落在 IPC 消息序列上，而不是 ANSI/TUI 渲染。

### API Key

live 用例复用 cc-rust 现有的 `.env` / 进程环境加载逻辑，不在测试 harness 中自行解析 `.env`。

- 需要真实模型的用例继续使用 `#[ignore]`
- 本地无有效 API key 时，开发者显式跳过 live 用例
- primary assertion 依赖 JSONL 消息；日志文件仅作为补充调试材料

### 测试用例矩阵

| # | 用例 | 输入 | 断言 |
|---|------|------|------|
| 1 | 同步 subagent 成功 | 触发 Agent tool 的 prompt | 收到 `tool_use(name="Agent")` → `tool_result(is_error=false)` |
| 2 | 同步 subagent 返回结果 | 简单任务 prompt | `tool_result.output` 含预期内容 |
| 3 | 背景 subagent 完成通知 | `run_in_background: true` | 收到 `background_agent_complete(had_error=false)` |
| 4 | 递归深度限制 | 嵌套 agent 超过 5 层 | `tool_result.output` 含 recursion depth limit 错误 |
| 5 | subagent abort | 发送 prompt 后立即 `abort_query` | 不 hang，随后可继续提交下一轮 prompt |
| 6 | worktree 隔离 | `isolation: "worktree"` | `tool_result.output` 含 worktree 保留或清理信息 |
| 7 | background+worktree 当前降级 | `run_in_background: true, isolation: "worktree"` | 行为不崩溃，且结果/日志体现“降级为普通 cwd” |

### Harness 复用

不新增 `HeadlessHarness` 结构体，直接复用现有 helper：

```rust
spawn_headless(...)
read_line_json(...)
send_msg(...)
collect_until(...)
```

这样可以保持与现有 `e2e_terminal`、`permission`、`tool_display`、`usage` 用例一致的测试风格。

### 事件日志验证

每个 live 测试可在结束后附加读取 `.logs/subagent-events.ndjson` 做补充校验，但它不是 primary assertion。主断言仍是 JSONL IPC 消息序列。

## 3. 事件提取与消费

### 事件来源

v1 区分两类输出：

- **人类调试日志**：`<cwd>/.logs/cc-rust.log.YYYY-MM-DD`
- **dashboard 事件流**：`<cwd>/.logs/subagent-events.ndjson`

dashboard 只消费后者，不依赖用户 hooks，也不依赖 tracing 文本格式。

### Rust 侧写入职责

以下状态变化需要显式追加一条 NDJSON 事件：

- subagent spawn
- subagent complete
- subagent error
- background subagent complete
- worktree fallback / kept / cleaned
- 与 dashboard 直接相关的 warning

不要求 v1 覆盖 subagent 内部的每一个工具调用。

### 事件格式

```typescript
type SubagentEvent = {
  ts: string;
  kind:
    | "spawn"
    | "complete"
    | "error"
    | "background_complete"
    | "worktree_kept"
    | "worktree_cleaned"
    | "warning";
  agent_id: string;
  parent_agent_id?: string;
  description?: string;
  model?: string;
  depth: number;
  background: boolean;
  payload?: Record<string, unknown>;
};
```

NDJSON 示例：

```json
{"ts":"2026-04-13T22:35:39.100Z","kind":"spawn","agent_id":"a1b2","description":"search files","model":"sonnet","depth":1,"background":false}
{"ts":"2026-04-13T22:35:43.000Z","kind":"complete","agent_id":"a1b2","depth":1,"background":false,"payload":{"duration_ms":3800}}
{"ts":"2026-04-13T22:35:44.200Z","kind":"warning","agent_id":"d4e5","depth":1,"background":true,"payload":{"message":"background + worktree not yet combined; using normal cwd"}}
```

### 消费策略 (`event-watcher.ts`)

- 主路径：`fs.watch()` 监听 `subagent-events.ndjson`
- Windows 兜底：定期 `stat()` 轮询，避免 append 事件漏触发
- 维护读取 offset，增量读取新追加内容
- 检测文件 size 缩小时重置 offset（truncate / recreate）
- 启动时回放最近 N 条事件（建议 200 条）作为初始状态
- 遇到坏行时跳过并记录 server 侧 warning，不让整个 dashboard 崩溃

## 4. 仪表盘 UI

### 三栏布局

```
┌──────────────────────────────────────────────────────────┐
│  cc-rust Subagent Dashboard           ● Connected  00:42 │
├──────────────┬───────────────────────────────────────────┤
│ Active       │ Lifecycle                                 │
│ Agents       │                                           │
│              │ agent-a1b2  spawn ─────────── complete    │
│ ● a1b2       │ agent-d4e5  spawn ───── running...        │
│   sonnet     │                                           │
│   3.8s       │ badges: background / worktree / warning   │
│              │                                           │
│ ◐ d4e5       │                                           │
│   opus       │                                           │
│   running    │                                           │
├──────────────┴───────────────────────────────────────────┤
│ Event Stream (newest first)                              │
│                                                          │
│ 22:35:44 WARN  d4e5 warning background+worktree fallback │
│ 22:35:43 INFO  a1b2 complete duration=3800ms             │
│ 22:35:39 INFO  a1b2 spawn model=sonnet                   │
│ 22:35:39 INFO  d4e5 spawn model=opus background=yes      │
└──────────────────────────────────────────────────────────┘
```

### 面板定义

| 面板 | 内容 | 更新方式 |
|------|------|----------|
| Active Agents（左侧） | 活跃 subagent 列表，ID、model、背景任务标记、耗时 | `/api/state` 初始化 + SSE 增量更新 |
| Lifecycle（右上） | 生命周期条目：spawn → running → complete/error | 基于结构化事件聚合，不展示 tool-level 节点 |
| Event Stream（底部） | 最近事件流，最新在上 | SSE 逐条追加 |

### 技术实现

- 纯 HTML + CSS + vanilla JS，无额外框架
- 首屏先请求 `GET /api/state`，再建立 `new EventSource("/events")`
- 前端状态模型：`Map<agent_id, AgentState>`
- v1 优先保证信息密度和可读性，不追求复杂动画

### SSE 事件格式

```text
event: snapshot
data: {"agents":[...],"recent_events":[...]}

event: subagent
data: {"ts":"...","kind":"spawn","agent_id":"a1b2","depth":1,"background":false}

event: ping
data: {}
```

## 5. Chrome DevTools MCP 自动化验证

### 测试流程

```text
1. 单独启动 companion dashboard (Bun, 127.0.0.1:19838)
2. 启动 cc-rust --headless，发送触发 subagent 的 prompt
3. Chrome DevTools MCP:
   - new_page → navigate_page(http://127.0.0.1:19838)
   - wait_for(".agent-row")
   - evaluate_script → 断言 DOM 状态
   - take_screenshot → .logs/screenshots/
   - close_page
4. 停止 cc-rust + companion
```

### 验证点矩阵

| # | 验证内容 | DevTools 操作 | 断言 |
|---|----------|---------------|------|
| 1 | SSE 连接建立 | `evaluate_script` 查询 `.status` 文本 | 含 `Connected` |
| 2 | Agent 行渲染 | `wait_for(".agent-row")` + 计数 | `>= 1` |
| 3 | 生命周期条目 | `evaluate_script` 查询 `.lifecycle-row` | `>= 1` |
| 4 | 事件流行数 | `evaluate_script` 查询 `.event-row` | `>= 3` |
| 5 | 完成或告警标记 | `wait_for(".agent-row.completed, .agent-row.warning")` | 元素存在 |
| 6 | 截图存档 | `take_screenshot` | 文件写入 |

### 执行方式

独立脚本 `tests/dashboard_verify.ts`（Bun）。它属于手动 / opt-in 回归工具，不作为默认 CI gate。

## 6. 生命周期管理

### Companion 进程管理

新增 `src/dashboard.rs`：

```rust
pub struct DashboardConfig {
    pub port: u16,
    pub event_log_path: std::path::PathBuf,
    pub auto_open_browser: bool,
}

pub struct DashboardCompanion {
    child: std::process::Child,
}

impl DashboardCompanion {
    pub fn spawn(config: DashboardConfig) -> Result<Self>;
    pub fn kill(&mut self);
}

impl Drop for DashboardCompanion {
    fn drop(&mut self) { self.kill(); }
}
```

### 启动条件

- `FEATURE_SUBAGENT_DASHBOARD=1`
- 当前为交互式 TUI 会话
- 非 `--headless`
- 非 `--print`
- 非 `--daemon`
- `bun` 不在 PATH 时仅 `warn` 并跳过，不阻塞 cc-rust 主流程

### 关闭保障

- `Drop` trait 兜底
- `shutdown::register_cleanup()` 在 Ctrl+C / panic 时触发
- companion server 自身检测父进程退出后自行退出
- HTTP server 仅绑定 `127.0.0.1`

### 新增文件结构

```text
rust/
├── src/dashboard.rs                     ← Rust 端 companion 生命周期管理
├── ui/package.json                      ← 新增 dashboard 启动脚本
├── ui/subagent-dashboard/
│   ├── server.ts                        ← HTTP + SSE
│   ├── event-watcher.ts                 ← NDJSON 监听 + 聚合
│   └── dashboard.html                   ← 单页 dashboard
└── tests/dashboard_verify.ts            ← DevTools MCP 验证脚本
```

`ui/subagent-dashboard/` 复用 `ui/` 根 Bun 环境，不单独新增 `package.json`。

## 7. 测试执行顺序

```text
Phase 1: Headless IPC e2e 测试 (Rust)
  cargo test --test e2e_terminal subagent -- --ignored
  -> 验证 subagent 端到端正确性
  -> 这是默认的功能回归主链路

Phase 2: 仪表盘开发 + 手动探索
  交互式启动 cc-rust (FEATURE_SUBAGENT_DASHBOARD=1)
  -> companion 自动启动
  -> 浏览器打开 dashboard
  -> 在会话中触发 subagent
  -> 观察生命周期与事件流

Phase 3: Chrome DevTools 自动化验证
  bun run tests/dashboard_verify.ts
  -> 截图 + DOM 断言
  -> 发现问题后回到 Phase 1/2 调整
```

### 回归策略

- 必须先保证 Phase 1 稳定，再开发 dashboard
- Phase 3 只在本地或专门回归任务中执行
- 若后续需要把 dashboard 纳入 CI，应先把 companion 启动、端口探测、截图路径全部做成 deterministic
