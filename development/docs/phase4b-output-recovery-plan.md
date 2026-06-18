# Phase 4b：Agent / Worker / Gateway 输出恢复计划

> 计划日期：2026-06-17
> 依赖：Phase 3 event log/outbound router，Phase 4 terminal/task `OutputEvent` 基础类型

## 目标

把 gateway run events、daemon worker NDJSON、agent supervisor output 三条输出链路统一到共享 `OutputEvent` / `OutputReadBatch`。本阶段是 breaking migration：旧 gateway `/events` 不再返回 `RunEvent` 列表。

## Public API / Wire Shape

- `GET /remote-control/v1/runs/{run_id}/events?after_seq=&limit_bytes=`
  - 返回 `OutputReadBatch`：
    - `events: OutputEvent[]`
    - `next_seq`
    - `truncated`
    - `first_available_seq`
    - `state`
  - 只表示可重放输出。

- `GET /remote-control/v1/runs/{run_id}/timeline?after_sequence=&limit=`
  - 返回非输出 timeline：
    - run created/status changed
    - approval requested
    - ask-user requested
    - diagnostic / delivery failure
    - session lock recovery
    - custom control events
  - timeline 保留 typed event 语义，用于远程控制 UI 和命令行展示。

- Agent IPC:
  - `AgentCommand::QueryAgentOutput` 增加 `after_seq` 和 `limit_bytes`。
  - 新增 `AgentEvent::OutputBatch { agent_id, task_id, output }`。
  - 不再用 `SystemInfo` 拼接整段输出。

## 存储与迁移

- gateway run 目录新增：
  - `output.events.ndjson`：逐行 `OutputEvent`
  - `timeline.ndjson`：typed timeline event
- 保留读取旧 `events.ndjson` 的 lazy import：
  - `AssistantDelta { text }` 转 `OutputEvent { stream: stdout, chunk: text, process_or_run_id: run_id }`
  - 状态、审批、诊断、custom 事件转 timeline
  - import 后写入新文件；旧文件不删除
- daemon worker 新增 worker output NDJSON，行格式为 `OutputEvent`。
- 原 `DaemonEvent` 只保留 command lifecycle / diagnostic timeline 用途，不再承载助手输出 chunk。
- agent supervisor 继续复用 task store 的 `.output.events.ndjson`，IPC 只读取 `TaskStore::read_output_events`。

## Implementation Notes

- 复用 `allthecodes-types::output::{OutputEvent, OutputReadBatch, OutputLifecycleState, OutputStream}`。
- gateway store 新增 output read/append helper 和 timeline read/append helper。
- `gateway_run_events` 中助手 delta 写 output，状态/控制事件写 timeline。
- `remote_cmd` 的 `/remote events` 展示改为输出 batch；新增或扩展命令展示 timeline，避免丢失审批/诊断信息。
- daemon SSE replay 如果包含 worker 输出，应从 output batch 生成 daemon output SSE event；command lifecycle 继续走 timeline-shaped SSE。

## Tests

- `cargo test -p allthecodes-gateway`
  - 新 run 创建 timeline created 事件。
  - assistant delta 写入 output batch。
  - 旧 `events.ndjson` lazy import 分流 output/timeline。
  - bounded output 超限后 `truncated=true`。
- `cargo test -p allthecodes-daemon protocol gateway_bridge`
  - worker submit stream 写 `OutputEvent`。
  - command ack/failed 不写 output。
  - gateway status/approval 事件可从 timeline 读取。
- `cargo test -p allthecodes-ipc`
  - `QueryAgentOutput` 返回 `AgentEvent::OutputBatch`。
- `cargo test -p allthecodes-commands remote`
  - remote output/timeline render 不依赖旧 `RunEvent`。

## 执行记录

Phase 4b 首轮已落地 gateway / daemon bridge / agent IPC / remote 命令的核心 contract：

- `GatewayStore` 新增 `output.events.ndjson` 与 `timeline.ndjson`，旧 `events.ndjson` 仅作 lazy import 来源，不再作为新 `/events` wire shape。
- `RunEventKind::AssistantDelta` 与 daemon `stream_delta` 文本写入 `OutputEvent`；状态、审批、诊断、custom control 事件写入 timeline。
- `GET /remote-control/v1/runs/{run_id}/events?after_seq=&limit_bytes=` breaking 改为直接返回 `OutputReadBatch`。
- 新增 `GET /remote-control/v1/runs/{run_id}/timeline?after_sequence=&limit=` 返回 `{ "events": RunEvent[] }`。
- `LocalGatewayClient` 与 `/remote events` 改为读取 output batch；新增 `/remote timeline` 展示非输出事件。
- `AgentCommand::QueryAgentOutput` 增加 `after_seq` / `limit_bytes`，优先返回 `AgentEvent::OutputBatch { agent_id, task_id, output }`。

验证记录：

| 命令/核对项 | 结果 |
|---|---|
| `cargo check -p allthecodes-gateway -p allthecodes-ipc` | 通过 |
| `cargo check -p allthecodes-daemon -p allthecodes-commands -p allthecodes --bin allthecodes` | 通过 |
| `cargo test -p allthecodes-gateway` | 通过：33 个 unit tests、0 个 doctests |
| `cargo test -p allthecodes-ipc agent_handlers` | 通过：2 个匹配测试 |
| `cargo test -p allthecodes-commands remote` | 通过：4 个匹配测试 |
| `cargo test -p allthecodes-daemon gateway_bridge` | 通过：2 个匹配测试 |
| `cargo check -p allthecodes --bin allthecodes` | 通过 |
| `cargo build --workspace` | 通过 |
| `cargo clippy --workspace --lib --bins` | 通过；输出既有 `allthecodes-config/src/validation.rs` type-complexity warning |

剩余复验：

- daemon SSE replay 是否要把 worker output batch 转为专门 output SSE event 仍需在后续切片中接入。
