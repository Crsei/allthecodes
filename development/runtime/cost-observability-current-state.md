# 成本观测当前实现情况

日期：2026-07-02

范围：token usage、cost 计算、预算中断、状态栏、usage API、审计事件、Langfuse 适配。

## 总体状态

成本观测已经覆盖运行时累计、会话落盘、Web usage 汇总和审计事件。当前核心链路是：

1. API 层从模型响应中得到 usage。
2. pricing 模块按模型价格计算 `cost_usd`。
3. Assistant message 携带 `usage` 和 `cost_usd`。
4. `QueryEngine` 在流式处理过程中累计 `UsageTracking`。
5. TUI/status line/Web/API/审计在不同层面读取累计值或历史会话文件。

当前实现可以回答“本次运行用了多少 token/成本”和“历史会话大致用了多少 token/成本”。但它还不是完整的成本归因系统，尚未把成本精确拆到每个工具、每个 agent 或每个 runtime execution record。

## Usage 与成本字段

基础类型在 `crates/allthecodes-types/src/message.rs`。

`Usage` 当前字段：

- `input_tokens`
- `output_tokens`
- `reasoning_output_tokens`
- `cache_read_input_tokens`
- `cache_creation_input_tokens`

`AssistantMessage` 当前包含：

- `usage: Option<Usage>`
- `cost_usd: f64`

流式事件也携带 usage：

- `StreamEvent::MessageStart { usage }`
- `StreamEvent::MessageDelta { usage: Option<Usage> }`

SDK 汇总类型在 `crates/allthecodes-types/src/sdk.rs`。`UsageTracking` 会累计：

- total input tokens。
- total output tokens。
- total cache read tokens。
- total cache creation tokens。
- total cost USD。
- API call count。

`SdkResult` 会返回 total cost、usage、duration、turn 数、session id、错误和权限拒绝信息。

## 成本计算

成本计算在 `crates/allthecodes-api/src/api/pricing.rs`。

当前逻辑：

- 按 model 查 `allthecodes_types::models::get_pricing(model)`。
- 用 input/output/cache read/cache creation 计数计算成本。
- 未知 model 的成本为 0。

这意味着成本准确性依赖模型名映射和 pricing metadata 是否完整。若 provider 返回新模型名但本地价格表未覆盖，usage 仍会记录，cost 会降级为 0。

## 运行时累计与预算中断

运行时累计在 `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs` 和 `crates/allthecodes-engine/src/lifecycle/types.rs`。

当前行为：

- assistant message 到达并带 usage 时，调用 `state.usage.add_usage(msg_usage, assistant_msg.cost_usd)`。
- `UsageTrackingExt::add_usage()` 会同步累计 token、cost 和 API call count。
- 进程级状态 `PROCESS_STATE.total_cost_usd` 也会同步更新。
- `check_budget()` 会比较当前累计 `total_cost_usd` 和 `max_budget_usd`。
- 超出预算时设置 `AbortReason::MaxBudget`，结果 subtype 为 `ErrorMaxBudgetUsd`。

submit 完成时，审计事件和 `SdkResult` 都会带上累计 `cost_usd`。

## TUI 与状态栏观测

TUI 在 `crates/allthecodes/src/ui/tui/engine_events.rs` 中消费引擎结果：

- 更新当前 session cost。
- 更新 session usage totals。
- 推送给状态栏 payload。

状态栏 payload 位于 `crates/allthecodes-engine/src/status_line/payload.rs`，包含：

- input tokens。
- output tokens。
- cache read/cache creation tokens。
- total cost USD。
- API calls。
- duration。
- context status。

settings 侧还有 token savings 视图，会读取当前 live engine usage，用 cache read tokens 估算节省情况和 cache hit rate。

## 历史 Usage API

Web usage handler 位于 `crates/allthecodes-web/src/handlers/usage.rs`。

当前 API：

- `GET /api/usage?period=24h|7d|30d|90d|all&profile_id=...`

返回内容包括：

- 总 input/output/cache token。
- `total_cost_usd`。
- API call count。
- session count。
- 时间 bucket。
- by model。
- by provider。
- generated_at。
- partial/warnings。
- profile_id。

当前聚合方式是扫描 `~/.allthecodes/sessions/` 下的 JSON 会话文件：

- 只读取 `.json`。
- 跳过 `.rewind-` 和 `.archived-` 类文件。
- 对 assistant message 中的 `usage` 和 `cost_usd` 做累计。
- 尝试用 `{session_id}.requests.ndjson` 请求快照关联 model/provider。
- 如果 usage 事件数量和 request snapshot 数量无法对齐，则 model/provider 降级为 unknown。

时间 bucket 规则：

- `24h`：小时粒度。
- `7d`、`30d`、`90d`：天粒度。
- `all`：月粒度。

读取失败的文件不会中断整个统计，会作为 warning 返回。

## 审计与外部观测

审计事件类型在 `crates/allthecodes-observability/src/event.rs`。

当前 `AuditEvent` 使用稳定 envelope，覆盖：

- model request start/retry/finish/error。
- tool lifecycle。
- permission。
- session。
- submit。
- duration。
- arbitrary data。

审计写入由 `crates/allthecodes-observability/src/sink.rs` 完成：

- 写入 `~/.allthecodes/runs/<session_id>/events.ndjson`。
- 写入 `meta.json`。
- 支持 artifacts。
- 支持 redaction。
- shutdown 时 flush/sync。

`crates/allthecodes/src/shutdown.rs` 会写入 `session.end` 审计事件，其中包含 API calls、input/output tokens 和 cost USD。

Langfuse 适配位于 `crates/allthecodes-engine/src/services/langfuse/`：

- `telemetry` feature 开启时走真实 telemetry。
- 未开启时使用 no-op stub。
- convert 层会把 generation input、messages、tools、assistant output 转换为 Langfuse 类型。

## 当前边界与待补齐点

- 历史 usage API 当前主要扫描 JSON 会话文件，不直接以 SQLite 或 record/replay 作为唯一数据源。
- 未知 model 的 cost 为 0，可能低估真实花费。
- model/provider 关联依赖 request snapshot 与 usage 事件数量对齐；不对齐时会降级为 unknown。
- 成本目前按 assistant message/session 聚合，没有稳定的 per-tool、per-agent、per-subtask 成本归因。
- audit event 记录 tool/model/permission 生命周期，但尚未形成统一的 runtime execution cost ledger。
- live usage、history usage、audit usage 是多条链路，语义接近但不是同一个物理数据源。
