# Logs and Diagnostics Backend Plan

> Target page: `allthecodes-web` settings logs panel,
> `http://127.0.0.1:17321/settings?panel=logs`
> Current symptom: `Diagnostics snapshot unavailable: API endpoint not implemented`
> Backend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`
> Frontend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web`

---

## 1. Current State

The frontend already calls the backend APIs expected by the logs panel:

| Frontend caller | API |
|---|---|
| `src/lib/api.ts` | `GET /api/logs` |
| `src/lib/api.ts` | `GET /api/logs/export` |
| `src/lib/api.ts` | `GET /api/diagnostics/snapshot` |
| `src/lib/api.ts` | `GET /api/diagnostics/traces` |

The backend does not register these routes. In `crates/allthecodes-web/src/mod.rs`,
unregistered `/api/*` paths fall through to `api_fallback_handler`, which returns
`501` with `API endpoint not implemented`. In
`crates/allthecodes-web/src/handlers/capabilities.rs`, `logs` is still advertised
as `false`.

The visible snapshot error is therefore a backend gap, not a frontend routing
problem.

---

## 2. Scope

Implement the backend surface needed by the logs panel:

```http
GET /api/logs
GET /api/logs/export
GET /api/diagnostics/snapshot
GET /api/diagnostics/traces
```

Out of scope for the first implementation:

- Full distributed trace storage.
- Long-term log retention policies.
- Remote profile log aggregation.
- Frontend redesign.

The first implementation should remove the `501` errors and return truthful,
bounded diagnostic data from available backend sources.

---

## 3. Frontend Contract

Match the existing TypeScript contracts in `allthecodes-web/src/lib/types.ts`.

### 3.1 LogsResponse

```ts
interface LogsResponse {
  profile_id?: string | null
  entries: LogEntry[]
  next_cursor?: string | null
  truncated?: boolean
  message?: string | null
}
```

Required query parameters:

| Param | Type | Notes |
|---|---|---|
| `profile_id` | string? | Echo in response for cache partitioning. |
| `source` | string? | Filter by `backend`, `frontend`, `ipc`, `debug`, etc. |
| `level` | string? | Filter by `debug`, `info`, `warn`, `error`. |
| `category` | string? | Filter by event category. |
| `session_id` | string? | Filter when an entry has a session id. |
| `since` | number/string? | Milliseconds timestamp or parseable date. |
| `cursor` | string? | Pagination cursor. |
| `limit` | number? | Default 200, hard cap 500. |
| `search` | string? | Case-insensitive search over message and payload summary. |

### 3.2 DiagnosticsSnapshot

```ts
interface DiagnosticsSnapshot {
  profile_id?: string | null
  timestamp: number
  frontend_event_count: number
  ipc_event_count: number
  renderer_state_count: number
  captured_sse_count: number
  captured_ws_count: number
  enabled?: boolean
  events?: DiagnosticsEvent[]
  session_traces?: SessionTrace[]
  renderer_states?: RendererState[]
  metadata?: Record<string, unknown> | null
}
```

Backend-owned counters should be honest:

- `frontend_event_count`: normally `0` unless frontend uploads events later.
- `ipc_event_count`: number of recent IPC bridge events captured by backend.
- `renderer_state_count`: normally `0` unless backend has renderer data.
- `captured_sse_count`: number of recent SSE/log stream events observed.
- `captured_ws_count`: number of recent WebSocket events observed.

### 3.3 TracesResponse

```ts
interface TracesResponse {
  profile_id?: string | null
  traces: TraceSummary[]
  detail?: TraceDetail | null
}
```

Supported query parameters:

| Param | Type | Notes |
|---|---|---|
| `profile_id` | string? | Echo in response. |
| `session_id` | string? | Filter traces to a session. |
| `turn_id` | string? | Return `detail` for one turn when available. |

---

## 4. Backend Design

### 4.1 New handler module

Add:

```text
crates/allthecodes-web/src/handlers/logs.rs
```

Export it from:

```text
crates/allthecodes-web/src/handlers/mod.rs
```

The module should own:

- Request query structs.
- Response DTOs matching the frontend names and JSON field casing.
- Log source readers.
- Filtering, cursoring, and export formatting.

### 4.2 Route registration

Register routes before the `/api/{*path}` fallback in
`crates/allthecodes-web/src/mod.rs`:

```rust
.route("/api/logs", get(handlers::logs_handler))
.route("/api/logs/export", get(handlers::logs_export_handler))
.route(
    "/api/diagnostics/snapshot",
    get(handlers::diagnostics_snapshot_handler),
)
.route(
    "/api/diagnostics/traces",
    get(handlers::diagnostics_traces_handler),
)
```

### 4.3 Capabilities

Update `crates/allthecodes-web/src/handlers/capabilities.rs`:

```rust
caps.insert("logs".into(), true);
```

If diagnostics gets its own capability later, add:

```rust
caps.insert("diagnostics".into(), true);
```

The current frontend groups diagnostics requests under the `diagnostics` request
category but only uses `logs` for capability discovery.

---

## 5. Data Sources

Use a layered source strategy so the endpoint is useful immediately and can grow
without changing the frontend contract.

### 5.1 MVP sources

| Source | Path / owner | Use |
|---|---|---|
| Dev restart backend log | `/tmp/allthecodes-dev-restart/backend.log` | Local development backend lines. |
| Dev restart frontend log | `/tmp/allthecodes-dev-restart/frontend.log` | Optional source when requested. |
| Audit sink / runs | `~/.allthecodes/runs` when writable | Session and model-call metadata if present. |
| Engine app state | `state.engine().app_state()` | Current session id, model, cwd, settings metadata. |
| IPC/TUI websocket handlers | `crates/allthecodes-web/src/ws/{ipc,tui}.rs` | Future in-memory ring buffers for ws counters/events. |

### 5.2 Log parsing

Do not require logs to be JSON. Parse best-effort:

1. If a line is JSON, preserve it in `payload_raw`.
2. If a line starts with an RFC3339 timestamp, use it as `timestamp`.
3. Detect level from `ERROR`, `WARN`, `INFO`, `DEBUG`, or JSON fields.
4. Use the remaining text as `message`.
5. Set default fields:
   - `source = "backend"` for backend log.
   - `source = "frontend"` for frontend log.
   - `category = "log"` unless the line maps to `diagnostics`, `chat`, `settings`, etc.

### 5.3 Cursoring

Use an opaque cursor that can be decoded by the same backend version:

```text
source:file_offset_or_line_index
```

For MVP, line-index cursoring is acceptable because logs are small in local dev.
Hard cap reads to avoid loading huge files:

- Read at most the newest 10,000 lines per source.
- Return at most 500 entries.
- Set `truncated=true` when the cap is hit.

---

## 6. Diagnostics Snapshot MVP

`GET /api/diagnostics/snapshot` should return `200` even when no traces exist:

```json
{
  "profile_id": null,
  "timestamp": 1760000000000,
  "frontend_event_count": 0,
  "ipc_event_count": 0,
  "renderer_state_count": 0,
  "captured_sse_count": 0,
  "captured_ws_count": 0,
  "enabled": true,
  "events": [],
  "session_traces": [],
  "renderer_states": [],
  "metadata": {
    "cwd": "...",
    "session_id": "...",
    "model": "...",
    "log_sources": ["backend_dev_restart"]
  }
}
```

This removes the current user-facing error while accurately saying no backend
trace events have been captured yet.

---

## 7. Diagnostics Traces MVP

`GET /api/diagnostics/traces` should return summaries built from the best
available sources:

1. Prefer structured run/session trace files if present under `~/.allthecodes`.
2. Otherwise derive coarse summaries from recent log entries containing
   `session_id`, `turn_id`, `request_id`, or websocket lifecycle markers.
3. If no source exists, return an empty list with `200`.

Empty state:

```json
{
  "profile_id": null,
  "traces": [],
  "detail": null
}
```

Do not return `501` for missing trace storage.

---

## 8. Export

`GET /api/logs/export?format=jsonl` should stream or return a download body:

- `jsonl` default: one normalized `LogEntry` per line.
- `json`: `{ "entries": [...] }`.
- Reuse the same filters as `GET /api/logs`.
- Set `Content-Disposition` with a stable filename, for example
  `allthecodes-logs-YYYYMMDD-HHMMSS.jsonl`.

---

## 9. Tests

Add focused backend tests in `crates/allthecodes-web/src/handlers/logs.rs` or
the existing handler test module:

| Test | Assertion |
|---|---|
| `diagnostics_snapshot_returns_empty_snapshot_without_trace_store` | Status 200, required counters present. |
| `diagnostics_traces_returns_empty_list_without_trace_store` | Status 200, `traces=[]`. |
| `logs_handler_parses_plain_text_backend_log` | Level/timestamp/message normalized. |
| `logs_handler_filters_by_level_and_search` | Filtering happens before limit/cursor. |
| `logs_export_jsonl_uses_same_filters` | JSONL output contains only matching entries. |
| `capabilities_marks_logs_ready` | `logs=true`. |

Run:

```bash
cargo test -p allthecodes-web logs
cargo test -p allthecodes-web diagnostics
```

Then rebuild the release backend before using `npm run dev:restart` from the
frontend repository.

---

## 10. Implementation Order

1. Add `handlers/logs.rs` with DTOs and empty-success diagnostics handlers.
2. Register the four routes before the API fallback.
3. Flip `logs` capability to `true`.
4. Add log file readers and normalization.
5. Add filters, cursoring, and export.
6. Add best-effort trace summaries from app state/logs.
7. Add tests.
8. Rebuild backend and verify the settings logs panel no longer shows
   `Diagnostics snapshot unavailable: API endpoint not implemented`.

---

## 11. Acceptance Criteria

- `curl http://127.0.0.1:17322/api/diagnostics/snapshot` returns `200`.
- `curl http://127.0.0.1:17322/api/diagnostics/traces` returns `200`.
- `curl http://127.0.0.1:17322/api/logs` returns `200` with `entries`.
- `curl http://127.0.0.1:17322/api/logs/export?format=jsonl` returns a download
  body or JSONL body.
- `GET /api/capabilities` reports `logs: true`.
- `http://127.0.0.1:17321/settings?panel=logs` does not show the 501-derived
  snapshot error.

