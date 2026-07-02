# Runtime 当前状态 TDD 参考文档

> 来源文档:
> - `development/runtime/tool-discovery-current-state.md`
> - `development/runtime/session-management-current-state.md`
> - `development/runtime/permission-governance-current-state.md`
> - `development/runtime/cost-observability-current-state.md`
>
> 日期: 2026-07-03

这些文档描述了**已实现状态**，不是待办计划。本文件列出每条链路当前的已知边界/缺口，以便在后续过程中新增或补强测试。

---

## Tool Discovery

### 当前状态: 分层注册体系

### 已知边界

| 边界 | 测试建议 |
|------|----------|
| ToolSearch 来源/类别推断依赖启发式规则 | 验证 `mcp__` 前缀和描述文本映射是否正确 |
| 重名工具 first wins | 测试后注册来源不会覆盖已注册的工具 |
| Deferred discovered state 是内存 + compact metadata | 测试 compact 后 discovered names 是否恢复 |
| MCP 连接失败不阻塞启动 | 测试失败后工具不进入可执行集，错误暴露到状态面 |

### 建议补充测试

```rust
#[test]
fn tool_registry_deduplicates_by_name() { ... }
#[test]
fn tool_search_identifies_mcp_tools() { ... }
#[test]
fn deferred_discovery_survives_compaction() { ... }
#[test]
fn mcp_connection_failure_graceful() { ... }
```

---

## Session Management

### 当前状态: 运行时闭环

### 已知边界

| 边界 | 测试建议 |
|------|----------|
| SQLite + JSON 并存，部分消费方仍直接扫 JSON | 测试 SQLite 与 JSON 投影一致性 |
| Usage 主要扫描 JSON 文件，不以 SQLite/record-replay 为唯一源 | 测试 SQLite/record-replay 与 JSON 的 usage 汇总一致 |
| 请求快照与 usage 事件数量不对齐时降级为 unknown | 测试不对齐时的降级行为 |
| session-level grants 新会话清空 | 测试新会话不留旧 grants |

### 建议补充测试

```rust
#[test]
fn session_messages_match_across_json_and_sqlite() { ... }
#[test]
fn usage_summary_json_vs_sqlite() { ... }
#[test]
fn request_snapshot_count_mismatch_graceful() { ... }
#[test]
fn new_session_clears_session_grants() { ... }
```

---

## Permission Governance

### 当前状态: 中心化决策引擎

### 已知边界

| 边界 | 测试建议 |
|------|----------|
| Bypass 强允许路径跳过常规校验 | 测试 Bypass 下执行限制是否全部跳过 |
| Auto classifier 可能不可用 | 测试 fallback 行为 |
| Sandbox allowed command 不覆盖 deny/ask | 测试 deny 规则优先于 sandbox allowed |
| record/replay 有 permission request/response 但 tool execution record 无内嵌结论 | 已在 execution record 计划中覆盖 |
| 新会话清空 session-level grants | 测试清空行为 |

### 建议补充测试

```rust
#[test]
fn bypass_skips_all_security_checks() { ... }
#[test]
fn auto_classifier_unavailable_fallback() { ... }
#[test]
fn sandbox_allowed_does_not_override_deny() { ... }
```

---

## Cost Observability

### 当前状态: 覆盖运行中 + 历史

### 已知边界

| 边界 | 测试建议 |
|------|----------|
| 历史 usage API 扫描 JSON 文件 | 测试 JSON vs DB/record-replay 汇总一致性 |
| 未知 model cost = 0 | 测试 unknown 模型的零 cost + 标记是否明确 |
| Model/provider 依赖 request snapshot 对齐 | 测试不对齐的降级 |
| 无 per-tool/per-agent 成本归因 | 工具级成本不在当前目标内 |
| Live usage, history usage, audit usage 是三条链路 | 测试三者语义一致（输入相同时输出一致） |

### 建议补充测试

```rust
#[test]
fn cost_api_live_vs_history_vs_audit_consistent() {
    // 对同一 mock session → 三条链路输出一致
}
```

---

## 运行命令

```bash
cargo test -p allthecodes-tools registry
cargo test -p allthecodes-session
cargo test -p allthecodes-permissions
cargo test -p allthecodes-web handlers::usage
cargo build --workspace --release
```
