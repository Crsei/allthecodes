# Gateway Lifecycle Backend Plan

> Target pages: `allthecodes-web` `/gateways` and
> `http://127.0.0.1:17321/settings?panel=gateways`
> Current symptom: `Gateway request failed` / `API endpoint not implemented`
> Backend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`
> Frontend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web`

---

## 1. Current State

The frontend Gateway manager already calls these APIs:

| Frontend caller | API |
|---|---|
| `src/lib/api.ts` | `GET /api/gateways` |
| `src/lib/api.ts` | `GET /api/gateway/status` |
| `src/lib/api.ts` | `POST /api/gateways/:id/start` |
| `src/lib/api.ts` | `POST /api/gateways/:id/stop` |

The backend web crate currently advertises `gateways=true` in
`crates/allthecodes-web/src/handlers/capabilities.rs`, but
`crates/allthecodes-web/src/mod.rs` does not register any of these gateway
routes. Requests therefore fall through to the `/api/{*path}` fallback and
return `501 API endpoint not implemented`.

There is existing gateway infrastructure in the backend:

| Module | Existing capability |
|---|---|
| `crates/allthecodes-daemon/src/gateway_client.rs` | Reads local daemon state and talks to daemon gateway HTTP routes. |
| `crates/allthecodes-daemon/src/gateway_routes.rs` | Owns `/remote-control/v1/*` daemon gateway routes. |
| `crates/allthecodes-web/src/handlers/channels.rs` | Uses `LocalGatewayClient` for adapter/capability operations. |
| `crates/allthecodes-daemon/src/process_state.rs` | Implements shell daemon `start`, `stop`, `restart`, status, and shutdown request. |

The missing piece is a web-facing lifecycle adapter matching the frontend
Gateway manager contract.

---

## 2. Scope

Implement the backend surface needed by the Gateway UI:

```http
GET  /api/gateways
GET  /api/gateway/status
POST /api/gateways/:id/start
POST /api/gateways/:id/stop
```

The first implementation should remove the 501 error and provide a truthful
single default gateway backed by the local daemon gateway.

Out of scope for MVP:

- Multiple independently configured gateway instances.
- Remote host gateway lifecycle control.
- Provider adapter configuration UI.
- Stopping individual gateway runs; that belongs to `/remote stop <run_id>` or
  future run-management APIs, not the GatewayManager lifecycle button.

---

## 3. Frontend Contract

Match the existing TypeScript types in `allthecodes-web/src/lib/types.ts`.

### 3.1 GatewayStatusResponse

```ts
interface GatewayStatusResponse {
  status: 'unknown' | 'stopped' | 'starting' | 'running' | 'error'
  port?: number | null
  profile_id?: string | null
  message?: string | null
  id?: string
  name?: string
  bind_address?: string | null
  diagnostics?: string[] | null
  log_ref?: string | null
  updated_at?: number | null
}
```

### 3.2 GatewayListResponse

```ts
interface GatewayListResponse {
  gateways: GatewaySummary[]
  active_gateway_id?: string | null
}
```

Return one summary for MVP:

```json
{
  "gateways": [
    {
      "id": "local-daemon",
      "name": "Local daemon gateway",
      "status": "running",
      "port": 17322,
      "bind_address": "127.0.0.1",
      "message": "Daemon gateway is running.",
      "diagnostics": [],
      "log_ref": "~/.allthecodes/daemon/supervisor.log",
      "updated_at": 1760000000000
    }
  ],
  "active_gateway_id": "local-daemon"
}
```

Use `profile_id` only as a cache partitioning echo for now; the local daemon
gateway is process-global.

---

## 4. Backend Design

### 4.1 New handler module

Add:

```text
crates/allthecodes-web/src/handlers/gateways.rs
```

Export it from:

```text
crates/allthecodes-web/src/handlers/mod.rs
```

The module should own:

- Query/body DTOs.
- `GatewayStatusResponse` and `GatewayListResponse`.
- Mapping from `LocalGatewayDaemonStatus` to frontend status.
- Start/stop action responses.
- Error-to-HTTP mapping shared with the existing `channels` gateway handler if
  practical.

### 4.2 Route registration

Register these routes before `/api/{*path}` in `crates/allthecodes-web/src/mod.rs`:

```rust
.route("/api/gateway/status", get(handlers::gateway_status_handler))
.route("/api/gateways", get(handlers::gateways_list_handler))
.route(
    "/api/gateways/{id}/start",
    post(handlers::gateway_start_handler),
)
.route(
    "/api/gateways/{id}/stop",
    post(handlers::gateway_stop_handler),
)
```

Keep `capabilities.rs` as `gateways=true`, but add a test proving the route is
actually registered so the capability does not drift again.

---

## 5. Status Mapping

Use `allthecodes_daemon::gateway_client::LocalGatewayClient::daemon_status()`.

| Daemon status | Frontend status | Response notes |
|---|---|---|
| `Running { pid, base_url, health_url }` | `running` | Parse port from `base_url` or `health_url`; include pid and health URL in diagnostics/metadata if added. |
| `Stale { pid }` | `error` | Message should say daemon state is stale for the pid. |
| `Stopped` | `stopped` | Message should tell user the daemon is not running. |
| Read error | `error` | Return `200` for list/status if possible, with diagnostics; reserve 5xx for handler failures. |

The Gateway UI expects a status payload more than an exception. For `GET
/api/gateways` and `GET /api/gateway/status`, prefer `200` with
`status="error"` over surfacing a request failure unless the backend cannot
serialize a response.

---

## 6. Start and Stop Semantics

### 6.1 Stop

`POST /api/gateways/local-daemon/stop` should request daemon shutdown.

Preferred implementation:

- Add a small public helper in `allthecodes-daemon` if needed, wrapping
  `process_state::request_shutdown("web gateway stop")`.
- After requesting shutdown, return a fresh status snapshot:
  - `status="stopped"` if it already stopped.
  - `status="starting"` or `status="running"` with a shutdown-request message
    if still alive during grace period.

Do not call `LocalGatewayClient::stop_run`; gateway lifecycle stop is daemon
shutdown, not run cancellation.

### 6.2 Start

Starting the daemon from the web server is higher risk because the current
shell implementation in `process_state.rs`:

- Spawns the current executable with `--daemon`.
- Requires `FEATURE_KAIROS=1`.
- Writes logs under the daemon directory.
- Must not be attempted on Windows for backend build/runtime workflows.

MVP options:

1. **Safe MVP:** return `409` or `501` with a structured diagnostic saying start
   must be run from shell:
   `FEATURE_KAIROS=1 allthecodes daemon start --port <port>`.
2. **Managed start follow-up:** expose a public `start_daemon_supervisor`
   function in `allthecodes-daemon::process_state`, guarded by:
   - non-Windows only,
   - explicit feature/env check,
   - fixed loopback bind,
   - no token leakage,
   - log path in response.

Recommended first implementation: Safe MVP. It avoids web-triggered process
spawning while still removing the route-level `API endpoint not implemented`
error. Add managed start only after a separate review.

---

## 7. Error Shape

For action failures, return existing `ApiError` shape:

```json
{
  "error": "The daemon is not running.",
  "code": "daemon_stopped"
}
```

Recommended HTTP mapping:

| Code | HTTP |
|---|---|
| `invalid_gateway_id` | 404 |
| `daemon_stopped` on stop | 409 or 200 with stopped status |
| `daemon_stale` | 409 |
| `daemon_state_unavailable` | 503 |
| `gateway_start_requires_shell` | 409 |
| Unknown gateway diagnostic | 502 |

Accepted gateway ids for MVP:

- `local-daemon`
- `default` as compatibility alias from frontend normalization

Reject other ids with `404 invalid_gateway_id`.

---

## 8. Tests

Add focused tests under the web crate handler tests:

| Test | Assertion |
|---|---|
| `gateway_status_returns_stopped_when_daemon_state_absent` | `GET /api/gateway/status` returns `200`, `status=stopped`. |
| `gateways_list_returns_default_gateway` | `GET /api/gateways` returns `active_gateway_id=local-daemon`. |
| `gateway_start_route_does_not_fall_through` | `POST /api/gateways/local-daemon/start` returns structured diagnostic, not fallback 501. |
| `gateway_stop_route_rejects_unknown_id` | Unknown id returns `404 invalid_gateway_id`. |
| `capabilities_gateways_true_has_registered_routes` | Capability and route registration stay aligned. |

Run:

```bash
cargo test -p allthecodes-web gateways
```

Then rebuild the release backend before using `npm run dev:restart` from the
frontend repository.

---

## 9. Implementation Order

1. Add `handlers/gateways.rs` with DTOs and daemon-status mapping.
2. Register `/api/gateway/status` and `/api/gateways`.
3. Register start/stop routes and return structured responses instead of 501.
4. Implement safe stop via daemon shutdown request.
5. Keep start as a safe structured shell instruction unless managed spawning is
   explicitly approved.
6. Add tests.
7. Verify `GET /api/capabilities` still reports `gateways=true`.
8. Verify `/gateways` and `/settings?panel=gateways` no longer show
   `Gateway request failed: API endpoint not implemented`.

---

## 10. Acceptance Criteria

- `curl http://127.0.0.1:17322/api/gateway/status` returns `200`.
- `curl http://127.0.0.1:17322/api/gateways` returns `200` and a default gateway.
- `POST /api/gateways/local-daemon/start` returns a structured gateway response
  or diagnostic, not the fallback `API endpoint not implemented`.
- `POST /api/gateways/local-daemon/stop` returns a structured gateway response
  or diagnostic, not the fallback `API endpoint not implemented`.
- The Gateway page displays a gateway status card instead of an ErrorState
  caused by route fallback.

