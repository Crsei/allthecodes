# cc-rust 日志架构

> 最后更新: 2026-04-15

## 架构概览

双层 tracing subscriber，非阻塞写入：

| Layer | 输出 | 级别 | 格式 |
|-------|------|------|------|
| stderr | 终端 | `--verbose` → DEBUG, 默认 WARN, 支持 `RUST_LOG` | 无 target, 有 ANSI 颜色 |
| file | `~/.cc-rust/logs/cc-rust.log.YYYY-MM-DD` | DEBUG (自身 crate), WARN (第三方) | 有 target + 行号, 无 ANSI |

### 初始化

`src/main.rs` 中 `fn main()` 早期初始化，早于 CLI 解析以外的所有逻辑。

### 日志路径

统一使用绝对路径 `~/.cc-rust/logs/`（通过 `config::settings::global_claude_dir()`），不再依赖 CWD。

### 轮转与清理

- **轮转**: `tracing_appender::rolling::daily`，每日自动创建新文件
- **清理**: `cleanup_old_logs()` 在启动时删除 7 天前的 `cc-rust.log.*` 文件

### 第三方 crate 过滤

file layer 的 EnvFilter:

```
debug,reqwest=warn,hyper_util=warn,hyper=warn,h2=warn,rustls=warn,ignore=warn,globset=warn
```

## 各模块日志覆盖

### 级别约定

- **info** — 用户可见的关键生命周期事件
- **debug** — 开发调试，每个事件只记录一次
- **warn** — 异常但可恢复的情况
- **error** — 不可恢复的故障

### 启动 (`src/main.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| 版本号 | info | — |
| 工作目录切换 | info | cwd |
| settings loaded | debug | model, permission_mode, backend |
| tools registered | info | count |
| QueryEngine created | info | session |

### Query Loop (`src/query/loop_impl.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| iteration start | debug | turn |
| aborted before API call | info | — |
| tool use summary 注入 | debug | summary (截断 200 字符) |
| tools refreshed | debug | — |
| max turns reached | info | — |
| query loop finished | info | turns |

### Tool 执行管线 (`src/tools/execution/pipeline.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| tool call starting | debug | tool |
| tool call succeeded | debug | tool, duration_ms |
| tool execution failed | warn | tool, error, duration_ms |
| pre-tool hook >500ms | debug | tool, duration_ms |
| pre-tool hook error | warn | tool, error |
| post-tool hook stopped | debug | tool, message |

### Agent / Subagent (`src/tools/agent/tool_impl.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| spawning subagent | info | agent_id, description, subagent_type, model, depth, isolation |
| background agent started | info | agent_id, description |
| background agent completed | info | agent_id, duration_ms, result_len, had_error |
| no bg_agent_tx | warn | agent_id |
| background+worktree unsupported | warn | agent_id |

### Agent dispatch (`src/tools/agent/dispatch.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| subagent completed | debug | agent_id, result_len, error |

### Session 持久化 (`src/session/`)

| 事件 | 级别 | 文件 | 字段 |
|------|------|------|------|
| session saved | debug | storage.rs | session_id, messages |
| session loaded | debug | storage.rs | session_id, messages |
| sessions listed | debug | storage.rs | count |
| found last session | debug | resume.rs | session_id, messages |
| resuming session | info | resume.rs | session_id |
| session exported (saved) | info | export.rs | session_id, path |
| session exported (live) | debug | export.rs | session_id, path |

### Compact 管线 (`src/compact/pipeline.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| tool result budget applied | debug | replacement count |
| snip compact | debug | tokens freed |
| microcompact trimmed | debug | freed |
| auto compact triggered | info | estimated_tokens, model |
| compaction pipeline completed | info | before_tokens, after_tokens, messages |

### Headless IPC (`src/ipc/headless.rs`)

| 事件 | 级别 | 字段 |
|------|------|------|
| submit_prompt | debug | id |
| slash command | debug | — |
| abort requested | debug | — |
| quit requested | debug | — |

> 注: 被忽略的 stream event (ContentBlockStop, MessageDelta 等) 不再输出日志。

### MCP (`src/mcp/`)

| 事件 | 级别 | 文件 | 字段 |
|------|------|------|------|
| stdio server spawn | info | client.rs | command, args |
| MCP initialized | info | client.rs | protocol, server_info |
| MCP disconnecting | info | client.rs | — |
| tool listed | info | client.rs | — |
| tool call error | warn | client.rs | — |
| server startup error | warn | manager.rs | — |

### Worktree (`src/tools/worktree.rs`, `src/tools/agent/worktree.rs`)

覆盖率优秀 (90%)，创建/删除/保留/清理全有 info/debug/warn。

### Agent Teams (`src/teams/`)

覆盖率良好 (75%)，spawn/shutdown/cleanup/error 全有日志。

### Daemon / KAIROS (`src/daemon/`)

server 启动、routes 处理、SSE、tick、notification 均有日志。

## 日志分析技巧

```bash
# 查看今天的日志
tail -f ~/.cc-rust/logs/cc-rust.log.$(date +%Y-%m-%d)

# 按 session 过滤
grep "session=" ~/.cc-rust/logs/cc-rust.log.2026-04-15

# 查看 tool 执行耗时
grep "tool call succeeded" ~/.cc-rust/logs/cc-rust.log.2026-04-15

# 查看 background agent 完成情况
grep "background agent completed" ~/.cc-rust/logs/cc-rust.log.2026-04-15

# 查看 compaction 效果
grep "compaction pipeline completed" ~/.cc-rust/logs/cc-rust.log.2026-04-15
```

## 2026-04-15 改动记录

### Phase 1: 降噪

1. **删除 "ignoring stream event" debug log** — `src/ipc/headless.rs`
   - 被忽略的 ContentBlockStop/MessageDelta 事件不再写入日志
   - 减少每次 API 响应 4+ 条无用行

2. **file layer 屏蔽第三方 crate 噪音** — `src/main.rs`
   - reqwest/hyper_util/hyper/h2/rustls/ignore/globset 只输出 warn+
   - 消除连接建立/池化/gitignore 解析等 debug 噪音

3. **settings loaded 只输出关键字段** — `src/main.rs`
   - 从完整 `MergedConfig` Debug 序列化改为只输出 model/permission_mode/backend
   - 每次启动节省数百字符

4. **tool_use_summary 截断到 200 字符** — `src/query/loop_impl.rs`
   - 使用 `utils::messages::truncate_text()` 避免工具输出 dump 进日志

### Phase 2: 路径统一 + 清理

5. **log 目录改为绝对路径 `~/.cc-rust/logs/`** — `src/main.rs`
   - 通过 `global_claude_dir().join("logs")` 获取路径
   - fallback 到 `.logs/` (当 home 目录不可用时)
   - 日志不再分散到各 CWD 下

6. **启动时清理 7 天前旧日志** — `src/main.rs`
   - `cleanup_old_logs()` 函数，只匹配 `cc-rust.log.*` 文件
   - 基于文件修改时间判断

### Phase 3: 补充关键模块日志

7. **Session 持久化** — `src/session/{storage,resume,export}.rs`
   - 从零覆盖提升到关键操作全覆盖 (save/load/list/resume/export)

8. **Background Agent** — `src/tools/agent/tool_impl.rs`
   - spawn 闭包内增加 started/completed info 日志

9. **Tool 执行管线** — `src/tools/execution/pipeline.rs`
   - 增加 tool call starting (debug) 和 succeeded (debug + duration_ms)

10. **Compact 管线** — `src/compact/pipeline.rs`
    - 增加 compaction completed info 日志 (含 before/after token 对比)

11. **Query Loop** — `src/query/loop_impl.rs`
    - `query loop finished` 从 debug 升级为 info

### 附带修复

12. **codex_exec.rs 编译错误** — `src/engine/codex_exec.rs`
    - `Vec<str>` → `Vec<String>` 显式类型注解
    - `impl Iterator<Item = &'_ str>` → 命名生命周期 `&'a str`
