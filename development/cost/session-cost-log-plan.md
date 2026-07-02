# Session Cost Log Plan

> 状态：Implemented (2026-07-03)
> 范围：session 内 API usage / cost 记录、展示、历史汇总和 Web 用量看板
> 约束：保持 allthecodes 路径隔离，持久化数据只能落在 `~/.allthecodes/` 下；不再以 Lite 缩减为边界
> 验证：cost/pricing/usage/statusline/audit targeted tests 已通过；`cargo build --workspace --release` 已通过。`cargo fmt` 和 `cargo test -p allthecodes status` 仍被既有 `crates/allthecodes/tests/pty_tui_e2e/model_flow.rs` 未闭合分隔符阻塞。

## 1. 目标

当前成本统计已经能工作，但事实源分散：单次成本写在 assistant message 上，运行时 `UsageTracking` 维护累计值，命令、TUI、Web、audit shutdown 再各自汇总。这个计划的目标是把 session cost log 明确成一条可追踪、可恢复、可验证的运行时事实链。

最终能力：

- 每次模型 API call 都有一条 durable cost/usage 记录。
- 每条 cost 记录能关联 `session_id`、submit/turn/request、assistant message、provider、model 和定价来源。
- `/cost`、`/extra-usage`、`/insights`、status line、Web usage dashboard 的总数一致。
- 旧 session 仍可读，缺失成本细节时有明确 backfill 标记。
- 未知模型、环境覆盖价格、cache tokens、reasoning tokens 都有可解释的输出。
- 崩溃或异常退出后，已完成 API call 的 cost log 仍可从 append-only 记录恢复。

非目标：

- 不接入 provider 账单 API。
- 不保证估算金额等于最终账单，只保证估算公式、定价来源和输入 token 可追溯。
- 不在 npm backend-only 发布流程中引入 Web SPA 打包。

## 2. 当前实现

### 2.1 单次成本来源

- `crates/allthecodes-types/src/message.rs`
  - `Usage` 保存 `input_tokens`、`output_tokens`、`reasoning_output_tokens`、cache read/create token。
  - `AssistantMessage` 保存 `usage: Option<Usage>` 和 `cost_usd: f64`。
- `crates/allthecodes-api/src/api/streaming.rs`
  - `StreamAccumulator` 从 stream event 聚合 usage。
  - `build_with_uuid()` 调 `crate::api::pricing::calculate_cost(model, &self.usage)`，把 `cost_usd` 写进 final assistant message。
- `crates/allthecodes-types/src/models/pricing.rs`
  - 内置模型价格表。
  - `MODEL_INPUT_PRICE` / `MODEL_OUTPUT_PRICE` 可覆盖全局输入、输出价格。
  - unknown model 返回 zero pricing。
  - cache read 按 input price `0.1x`，cache creation 按 input price `1.25x`。

### 2.2 运行时累计

- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
  - 处理 `Message::Assistant` 时把 message 推入 engine state。
  - 如果 assistant 有 usage，调用 `state.usage.add_usage(msg_usage, assistant_msg.cost_usd)`。
- `crates/allthecodes-types/src/sdk.rs`
  - `UsageTracking` 保存 session 累计 token、cost、api call count。
- `crates/allthecodes-engine/src/lifecycle/types.rs`
  - `UsageTrackingExt::add_usage()` 同步更新 engine usage 和 `PROCESS_STATE.total_cost_usd`。

### 2.3 持久化与展示

- session snapshot 保存 assistant message，所以 `usage` 和 `cost_usd` 会随 session JSON 落盘。
- `/cost` 和别名 `/usage` 从当前 messages 聚合 assistant `usage` 和 `cost_usd`。
- `/extra-usage` 做 per-message token/cost breakdown 和 top expensive calls。
- `/insights` 扫描历史 sessions 并聚合 assistant `usage` 和 `cost_usd`。
- Web `GET /api/usage` 扫描 `{data_root}/sessions` 下的 session JSON，按 assistant message 生成 usage events。
- TUI 收到 final `SdkResult` 后更新 status line payload；单条 assistant message `cost_usd > 0` 时渲染 `($x.xxxx)`。
- shutdown 时写 `SessionEnd` audit event，包含 session summary 的 `api_calls`、`input_tokens`、`output_tokens`、`cost_usd`。

## 3. 当前缺口

1. 缺少 canonical cost ledger。
   现在 cost 事实主要藏在 assistant message 和累计状态里。它们适合展示和 resume，但不是独立的 API call ledger。

2. 缺少 request 级关联。
   Web dashboard 通过 request snapshots 数量和 assistant usage events 对齐 provider/model。一旦数量不一致就降级为 unknown。

3. unknown model 静默归零。
   `get_pricing()` 对 unknown model 返回 zero。用户看到 `$0.0000` 时不能区分“免费”“未知模型”“价格表缺失”。

4. 定价来源不可追踪。
   当前 `cost_usd` 只保存结果，不保存 price row、pricing source、cache multiplier、环境覆盖来源。

5. 展示面重复聚合。
   `/cost`、`/extra-usage`、`/insights`、statusline command、Web usage 都有自己的聚合逻辑，后续字段扩展容易漂移。

6. IPC usage update 字段不足。
   `BackendMessage::UsageUpdate` 只有 input/output/cost，缺少 cache、reasoning、api_calls 和“累计还是增量”的语义说明。

7. SessionEnd audit 是 summary，不是事实源。
   shutdown 成功时能得到 session summary；崩溃时没有最终 summary。已经完成的 API call 应该在 call 完成时就落 durable event。

8. 旧 session 可回放但无法区分真实记录与 backfill。
   历史 session 只能从 assistant message 推导成本，没有 request/pricing metadata。

## 4. 设计决策

### 4.1 成本事实源

采用“API call 完成时写入 cost event，assistant message 保留兼容字段”的设计。

推荐事实源：

- 首选写入现有 audit append-only log：`~/.allthecodes/runs/<session_id>/events.ndjson`。
- cost event 使用稳定 `data` schema，避免新增一套并行 writer。
- 后续如需要高频查询，可从 audit event 派生 `cost-summary.json` 或 sqlite/materialized view，但派生文件不是事实源。

保留：

- `AssistantMessage.cost_usd` 继续保留，用于兼容 session snapshot、现有 UI 和旧命令。
- `UsageTracking` 继续保留，用于运行时低成本累计和 final `SdkResult`。

### 4.2 Cost event schema

建议新增或扩展 audit event，kind 使用 `model.usage` 或 `cost.recorded`。字段放在 audit event `data` 内：

```json
{
  "schema_version": 1,
  "session_id": "session-id",
  "submit_id": "submit-id",
  "turn_id": "turn-id",
  "request_id": "request-id",
  "message_id": "assistant-message-uuid",
  "provider": "anthropic",
  "backend": "native",
  "model": "claude-sonnet-4-20250514",
  "pricing": {
    "source": "builtin",
    "matched_key": "claude-sonnet-4",
    "currency": "USD",
    "input_per_1m": 3.0,
    "output_per_1m": 15.0,
    "cache_read_multiplier": 0.1,
    "cache_creation_multiplier": 1.25
  },
  "usage": {
    "input_tokens": 1000,
    "output_tokens": 250,
    "reasoning_output_tokens": 0,
    "cache_read_input_tokens": 0,
    "cache_creation_input_tokens": 0
  },
  "cost_usd": 0.00675,
  "stop_reason": "end_turn",
  "is_retry": false,
  "attempt": 1,
  "backfilled": false
}
```

Schema notes:

- `pricing.source`: `builtin | env_override | provider_reported | unknown | backfilled`.
- `matched_key`: 定价表命中的模型 key；unknown 时为空。
- `provider_reported` 预留给未来 provider 返回 cost 的场景。
- `backfilled=true` 表示从旧 session assistant message 推导，不代表运行时真实 request event。
- `reasoning_output_tokens` 先记录但不单独计价，除非 provider pricing 明确需要。

### 4.3 聚合原则

- 运行时状态从 completed API call 累加。
- 新的历史统计优先读 cost event。
- 如果 cost event 不存在，回退读 session assistant message，并标记为 backfilled。
- 对用户可见的总数必须说明是否包含 backfilled / unknown pricing。
- 不在多个展示入口重复实现公式；共享聚合 helper。

## 5. Phase Plan

### Phase 0：基线和契约测试

目的：固定当前行为，防止改造时破坏已有展示。

工作内容：

- 给当前 `/cost`、`/extra-usage`、`/insights`、Web `usage_events_from_session()` 增加或补齐 fixture 测试。
- 覆盖 cache read/create、reasoning tokens、unknown model、env override、assistant without usage。
- 记录当前 `AssistantMessage.cost_usd` 的兼容要求：旧 session 缺失字段视为 `0.0`。
- 明确 `UsageTracking.total_cost_usd` 是 session 累计值，不是单次 delta。

退出条件：

- 成本计算和聚合的当前行为有测试保护。
- 所有测试 fixture 使用 `ALLTHECODES_HOME` 隔离临时目录。

验证：

```bash
cargo test -p allthecodes-types models::pricing
cargo test -p allthecodes-api api::pricing api::streaming
cargo test -p allthecodes-commands cost extra_usage insights
cargo test -p allthecodes-web handlers::usage
```

### Phase 1：定价结果结构化

目的：让 `calculate_cost()` 不只返回 `f64`，还返回定价来源和计算明细。

建议改动：

- `crates/allthecodes-types/src/models/pricing.rs`
- `crates/allthecodes-api/src/api/pricing.rs`
- `crates/allthecodes-api/src/api/streaming.rs`

工作内容：

- 新增 `PricingMatch` / `CostBreakdown` 类型。
- `get_pricing()` 返回 pricing row 时携带 source 和 matched key。
- unknown model 不再只是 zero row，而是显式 `source=unknown`。
- `StreamAccumulator::build_with_uuid()` 继续写 `AssistantMessage.cost_usd`，同时保留 breakdown 给后续 cost event。

退出条件：

- unknown pricing 可以被调用方识别。
- env override 可以被记录为 `pricing.source=env_override`。
- 现有 `calculate_cost(model, usage) -> f64` 可保留为兼容 wrapper。

验证：

```bash
cargo test -p allthecodes-types pricing
cargo test -p allthecodes-api pricing streaming
```

### Phase 2：写入 request 级 cost event

目的：API call 完成时就落 append-only cost event。

建议改动：

- `crates/allthecodes-observability/src/event.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`
- `crates/allthecodes-session/src/request_snapshot.rs`

工作内容：

- 扩展 audit `EventKind`，加入 `ModelUsage` 或 `CostRecorded`。
- 在 assistant message final 化并写入 engine state 后，立即 emit cost event。
- event 关联当前 session、submit、turn、request、message UUID。
- request snapshot 与 cost event 使用同一个 request identity，避免 Web dashboard 只靠数量 zip。
- emit 失败只 warning，不阻塞对话。

退出条件：

- 每个有 usage 的 assistant API call 都对应一条 cost event。
- 本地正常退出和异常前缀都能解析已写入的 events。
- `SessionEnd` summary 继续保留，但不再是唯一成本日志。

验证：

```bash
cargo test -p allthecodes-observability
cargo test -p allthecodes-engine lifecycle::submit_message
cargo test -p allthecodes-session request_snapshot
```

### Phase 3：共享聚合层

目的：把多个展示入口从“各自扫 messages”收敛到同一套聚合 helper。

建议新增：

- `crates/allthecodes-services/src/cost_ledger.rs`

建议职责：

- `load_session_cost_events(session_id)`.
- `backfill_cost_events_from_messages(session_id, messages)`.
- `aggregate_cost_events(events)`.
- `CostSummary` 输出 total tokens、cache tokens、reasoning tokens、cost、api_calls、unknown_pricing_count、backfilled_count。

迁移入口：

- `/cost`
- `/extra-usage`
- `/insights`
- `/statusline --print-payload` 的 usage snapshot
- Web `/api/usage`
- session export / audit export 的 cost summary

退出条件：

- 当前 session 仍可从 in-memory messages 快速展示。
- 历史 session 优先读 cost events，缺失时 backfill。
- 所有入口使用同一个 summary type，字段命名一致。

验证：

```bash
cargo test -p allthecodes-services cost_ledger
cargo test -p allthecodes-commands cost extra_usage insights statusline
cargo test -p allthecodes-web handlers::usage
```

### Phase 4：UI、IPC 和 status line 字段补齐

目的：让前端和 TUI 获得完整 usage/cost 语义。

建议改动：

- `crates/allthecodes-ipc-protocol/src/protocol/mod.rs`
- `crates/allthecodes-ipc-protocol/src/normalized.rs`
- `crates/allthecodes/src/app_runtime_adapters/sdk_mapper.rs`
- `crates/allthecodes/src/ui/app/status.rs`
- `crates/allthecodes-types/src/status_line.rs`

工作内容：

- 扩展 `UsageUpdate`：加入 cache read/create、reasoning output、api_calls。
- 明确 `UsageUpdate` 是 cumulative snapshot；如需 delta，新增字段 `kind: "cumulative" | "delta"`。
- status line payload 的 `cost` 增加 unknown/backfilled 计数，或在 `context`/`cost` 中公开 warning 字段。
- TUI footer/status widget 继续使用简洁成本展示，不把详细计费文本塞进主界面。

退出条件：

- IPC consumer 不再需要猜测 usage update 是累计还是增量。
- status line script 能拿到完整 cost summary。
- 旧前端字段兼容，新增字段为 additive。

验证：

```bash
cargo test -p allthecodes-ipc-protocol
cargo test -p allthecodes --lib ui::tui ui::app
```

### Phase 5：历史数据迁移与导出

目的：让旧 session 成本可继续统计，并明确质量等级。

工作内容：

- 为旧 session 提供 lazy backfill：读取 session assistant message 生成 `backfilled=true` cost events。
- 不默认改写旧 session 文件，避免大规模 churn。
- Web usage dashboard 对 backfilled / unknown pricing 给出 warnings。
- audit export 优先读 cost events，缺失时 backfill 并在 export metadata 中标明。

退出条件：

- 旧 session 在 `/insights` 和 Web dashboard 中仍有成本统计。
- 用户能区分真实 runtime cost event 与 backfilled estimate。
- 不读取、打印、提交任何凭据或 token。

验证：

```bash
cargo test -p allthecodes-session audit_export session_export
cargo test -p allthecodes-web handlers::usage
```

### Phase 6：端到端验证与 release gate

目的：确认真实对话路径、命令、Web 后端和 shutdown audit 都一致。

场景：

- 单次普通回答。
- tool-use 多轮回答。
- API retry 后成功。
- unknown model。
- env pricing override。
- crash-like early stop 后读取 partial events。
- 旧 session backfill。

验收标准：

- 对同一 session，cost event aggregation、engine `UsageTracking`、`/cost`、Web `/api/usage` total 一致。
- unknown pricing 不再静默表现为普通 `$0.0000`。
- `~/.allthecodes/runs/<session_id>/events.ndjson` 中每条 cost event 都能独立 parse。
- `cargo build --workspace --release` 无 warning。

验证：

```bash
cargo build --workspace --release
cargo test --workspace cost usage pricing --all-targets
```

如果 targeted test 名称不足以覆盖，应使用对应 crate 的完整测试命令替代。

## 6. 风险与处理

| 风险 | 影响 | 处理 |
| --- | --- | --- |
| 新 cost event 与 assistant `cost_usd` 不一致 | 用户看到不同总额 | Phase 0 先固定测试，Phase 3 使用共享聚合 |
| unknown model 变为 warning 后噪声过多 | 用户界面干扰 | UI 只显示简短 indicator，详细说明放 `/cost --details` 或 Web warning |
| audit event 写入失败 | 丢失 canonical ledger | 保留 assistant message fallback，并在 tracing 中 warning |
| 旧 session 无 request metadata | provider/model breakdown 降级 | 标记 `backfilled=true` 和 `request=unknown` |
| pricing table 过期 | 成本估算偏差 | event 保存 pricing source/matched key，后续可重算或解释 |
| 多入口迁移造成破坏 | 命令/Web 行为回归 | 先建共享 helper，再逐个入口替换 |

## 7. 需要改动的主要文件

- `crates/allthecodes-types/src/message.rs`
- `crates/allthecodes-types/src/models/pricing.rs`
- `crates/allthecodes-types/src/sdk.rs`
- `crates/allthecodes-api/src/api/pricing.rs`
- `crates/allthecodes-api/src/api/streaming.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`
- `crates/allthecodes-observability/src/event.rs`
- `crates/allthecodes-observability/src/context.rs`
- `crates/allthecodes-session/src/request_snapshot.rs`
- `crates/allthecodes-session/src/audit_export.rs`
- `crates/allthecodes-services/src/session_analytics.rs`
- `crates/allthecodes-services/src/cost_ledger.rs`（新增）
- `crates/allthecodes-commands/src/cost.rs`
- `crates/allthecodes-commands/src/extra_usage.rs`
- `crates/allthecodes-commands/src/insights.rs`
- `crates/allthecodes-commands/src/statusline_cmd.rs`
- `crates/allthecodes-web/src/handlers/usage.rs`
- `crates/allthecodes-ipc-protocol/src/protocol/mod.rs`
- `crates/allthecodes-ipc-protocol/src/normalized.rs`
- `crates/allthecodes/src/app_runtime_adapters/sdk_mapper.rs`
- `crates/allthecodes/src/ui/app/status.rs`

## 8. 完成定义

- cost log 有 request 级 durable event，不只依赖 session end summary。
- 所有用户可见入口复用同一套 cost summary 聚合语义。
- 旧数据可读且质量标记清晰。
- unknown pricing、env override、cache token 成本都有测试。
- release build 无 warning。
