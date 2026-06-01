# Remote Control Gateway Reference

Status: Phase 9 release-gate reference, 2026-05-08.

The remote-control gateway is the external control plane for cc-rust daemon runs. It normalizes remote inputs into `RemoteSource`, deterministic session keys, durable `run_id` records, gateway events, delivery records, and daemon worker commands. It is not the legacy `/api/*` daemon control API, and it is not a public multi-tenant hosted service.

## Boundaries

- Public-ish control surface: `/remote-control/v1/**`.
- Local user surface: `/remote` slash command and the TUI `RemoteSurface`.
- Compatibility surface: legacy `/webhook/github`, `/webhook/slack`, and `/webhook/generic` still exist and are routed through declarative webhook handling.
- Persistence root: `~/.cc-rust/gateway/**` through `cc-config` path isolation.
- Execution owner: the daemon assistant worker via `GatewayDaemonBridge`; `/remote` mutating operations use the loopback gateway client.
- Adapter scope: Telegram and Lark currently provide configuration status, connect/health checks, diagnostics, and test-message delivery only. Full inbound conversational remote control is a follow-up.

## Auth

Loopback daemon gateway routes use the local daemon control token:

```http
x-cc-rust-daemon-token: <local daemon token>
```

Remote token verification is implemented in `gateway::auth` for non-loopback gateway mode:

```http
x-cc-rust-remote-token: <configured remote token>
Authorization: Bearer <configured remote token>
```

Non-loopback binds must fail closed unless a remote token is configured. Origin checks are allowlist-based for browser-capable remote-control routes. Tokens, secrets, signatures, and credentials must be redacted from run metadata, `/remote` output, TUI snapshots, and diagnostics.

Stable auth and security diagnostics:

| Code | HTTP | Meaning |
| --- | ---: | --- |
| `missing_control_token` | 401 | Loopback daemon request omitted the daemon token. |
| `invalid_control_token` | 401 | Loopback daemon token did not verify. |
| `missing_remote_token` | 401 | Remote-token mode request omitted the remote token. |
| `invalid_remote_token` | 401 | Remote token did not verify. |
| `remote_token_required` | 403 | Non-loopback bind was attempted without a configured remote token. |
| `bad_origin` | 403 | Browser origin is not allowlisted. |
| `payload_too_large` | 413 | Request body exceeded the configured gateway limit. |

Error responses use:

```json
{
  "error": {
    "code": "payload_too_large",
    "message": "The remote-control gateway payload is too large.",
    "action": "Reduce the request body size and retry.",
    "context": "optional redacted context"
  }
}
```

## Source And Session Keys

Every submitted run carries a `RemoteSource`:

```json
{
  "transport": "http",
  "tenant": "local",
  "workspace": "F:/AIclassmanager/cc/rust",
  "clientId": "dashboard-1",
  "userId": "86186",
  "threadId": "default",
  "metadata": {
    "origin": "https://example.invalid"
  }
}
```

The gateway derives a deterministic session key from redacted source fields, with the default `remote:<transport>:...` prefix. Source metadata is redacted before persistence.

## Runs

Run statuses:

| Status | Meaning |
| --- | --- |
| `queued` | Durable run exists and is waiting to start. |
| `running` | Daemon worker command has been dispatched or acknowledged. |
| `waiting_approval` | Run is waiting for a run-scoped tool approval response. |
| `waiting_user` | Run is waiting for a run-scoped ask-user response. |
| `completed` | Run reached a terminal success state. |
| `failed` | Run reached a terminal failure state. |
| `cancelled` | Run was stopped. |
| `recoverable` | Daemon restarted while the run was active and durable events need operator review. |

Busy policy:

| Policy | Behavior |
| --- | --- |
| `queue` | Queue if one run is active and queue capacity remains. |
| `reject` | Return `409 busy` when the gateway is active. |
| `interrupt` | Send stop/abort command, then create the new run. |
| `steer` | Advertised as unsupported by default; returns `501 unsupported`. |

## HTTP API

### `GET /remote-control/v1/capabilities`

Returns version, auth mode, transports, busy policies, `supportsSteer`, limits, capacity, and endpoint paths.

```bash
curl -H "x-cc-rust-daemon-token: $TOKEN" \
  http://127.0.0.1:19836/remote-control/v1/capabilities
```

### `POST /remote-control/v1/runs`

Creates or resolves a durable run.

```bash
curl -X POST http://127.0.0.1:19836/remote-control/v1/runs \
  -H "x-cc-rust-daemon-token: $TOKEN" \
  -H "content-type: application/json" \
  -H "idempotency-key: provider-delivery-id" \
  -d '{
    "prompt": "Summarize the repository status.",
    "source": {
      "transport": "http",
      "tenant": "local",
      "workspace": "F:/AIclassmanager/cc/rust",
      "clientId": "dashboard",
      "userId": "86186",
      "threadId": "default"
    },
    "policy": {
      "busy": "queue",
      "permissionMode": "ask",
      "delivery": ["origin", "local"]
    }
  }'
```

Success:

```json
{
  "runId": "run_0123456789abcdef0123",
  "sessionKey": "remote:http:...",
  "status": "queued",
  "action": "queued",
  "eventsUrl": "/remote-control/v1/runs/run_0123456789abcdef0123/events"
}
```

Common errors: `invalid_json` 400, `payload_too_large` 413, `busy` 409, `queue_full` 429, `unsupported` 501, auth/origin errors above. Duplicate idempotency returns the existing run with HTTP 200.

### `GET /remote-control/v1/runs/{run_id}`

Returns durable run metadata and redacted request.

```bash
curl -H "x-cc-rust-daemon-token: $TOKEN" \
  http://127.0.0.1:19836/remote-control/v1/runs/run_0123456789abcdef0123
```

Common errors: `invalid_run_id` 400, `run_not_found` 404, auth/origin errors.

### `GET /remote-control/v1/runs/{run_id}/events`

Returns durable event replay from `events.ndjson`.

```bash
curl -H "x-cc-rust-daemon-token: $TOKEN" \
  http://127.0.0.1:19836/remote-control/v1/runs/run_0123456789abcdef0123/events
```

Common errors: `invalid_run_id` 400, `run_not_found` 404, `replay_unavailable` 503, auth/origin errors.

### `POST /remote-control/v1/runs/{run_id}/stop`

Requests run cancellation through the daemon worker bridge.

```bash
curl -X POST -H "x-cc-rust-daemon-token: $TOKEN" \
  http://127.0.0.1:19836/remote-control/v1/runs/run_0123456789abcdef0123/stop
```

Common errors: `invalid_run_id` 400, `run_not_found` 404, `run_already_terminal` 409, auth/origin errors.

### `POST /remote-control/v1/runs/{run_id}/approval`

Answers a pending tool approval for the specific run.

```bash
curl -X POST http://127.0.0.1:19836/remote-control/v1/runs/run_0123456789abcdef0123/approval \
  -H "x-cc-rust-daemon-token: $TOKEN" \
  -H "content-type: application/json" \
  -d '{"toolUseId":"tool-1","approved":true,"reason":"approved remotely"}'
```

Common errors: `approval_id_missing` 400, `stale_response` 409, run/auth errors.

### `POST /remote-control/v1/runs/{run_id}/ask-user`

Answers a pending ask-user prompt for the specific run.

```bash
curl -X POST http://127.0.0.1:19836/remote-control/v1/runs/run_0123456789abcdef0123/ask-user \
  -H "x-cc-rust-daemon-token: $TOKEN" \
  -H "content-type: application/json" \
  -d '{"questionId":"question-1","response":"Use option A."}'
```

Common errors: `question_id_missing` 400, `stale_response` 409, run/auth errors.

### `GET /remote-control/v1/adapters`

Lists provider-neutral Telegram and Lark adapter statuses.

```bash
curl -H "x-cc-rust-daemon-token: $TOKEN" \
  http://127.0.0.1:19836/remote-control/v1/adapters
```

States: `unconfigured`, `connected`, `connected_outbound_only`, `blocked`, `failed`.

### `POST /remote-control/v1/adapters/{provider}/connect`

Checks adapter credentials and returns a provider-neutral status.

```bash
curl -X POST -H "x-cc-rust-daemon-token: $TOKEN" \
  http://127.0.0.1:19836/remote-control/v1/adapters/telegram/connect
```

Provider can be `telegram` or `lark`. Common errors: `adapter_unsupported` 400, `telegram_http_failed`, `lark_http_failed`, provider-specific blocked/failed diagnostics.

### `POST /remote-control/v1/adapters/{provider}/test-message`

Sends a bounded test message to an allowlisted target.

```bash
curl -X POST http://127.0.0.1:19836/remote-control/v1/adapters/lark/test-message \
  -H "x-cc-rust-daemon-token: $TOKEN" \
  -H "content-type: application/json" \
  -d '{"target":"ops-room","text":"cc-rust remote gateway test"}'
```

Common errors: `telegram_target_missing` 400, `lark_target_missing` 400, `telegram_target_blocked` 403, `lark_target_blocked` 403, transport/provider diagnostics.

### `POST /remote-control/v1/webhooks/{route_id}`

Declarative webhook ingress is mounted by the daemon webhook router. Built-in route ids are `github`, `slack`, and `generic`; other ids use generic handling.

Secrets:

- `CC_RUST_WEBHOOK_<ROUTE_ID>_SECRET`
- `CC_RUST_GITHUB_WEBHOOK_SECRET` or `GITHUB_WEBHOOK_SECRET`
- `CC_RUST_SLACK_WEBHOOK_SECRET` or `SLACK_SIGNING_SECRET`
- `CC_RUST_GENERIC_WEBHOOK_SECRET`

GitHub example:

```bash
curl -X POST http://127.0.0.1:19836/remote-control/v1/webhooks/github \
  -H "x-github-event: pull_request" \
  -H "x-github-delivery: delivery-7" \
  -H "x-hub-signature-256: sha256=<hex-hmac>" \
  -H "content-type: application/json" \
  -d '{"action":"opened","repository":{"full_name":"owner/repo"}}'
```

Responses are JSON objects with `status` values such as `received`, `ignored`, or `error`. Common errors: `webhook_disabled` 403, `missing_signature` 403, `bad_hmac` 403, `payload_too_large` 413, `invalid_json` 400.

## Delivery Targets

Run policy `delivery` accepts:

| Target | Meaning |
| --- | --- |
| `origin` | Caller/origin delivery path; currently skipped by durable router when no origin sink is attached. |
| `local` | Append local delivery record to `delivery.ndjson`. |
| `callback:<url>` | Deliver to HTTPS public callback or loopback HTTP callback. |
| `channel:<provider>:<target>[:thread]` | Deliver through a configured channel adapter. |
| `sse:<client-id>` | Explicit SSE subscriber target placeholder. |

Callback delivery blocks non-loopback HTTP, private IPs, loopback HTTPS hosts, link-local addresses, and metadata hosts. Failures are recorded as delivery diagnostics and do not rewrite terminal run status.

## Recovery Semantics

On daemon gateway route startup, `GatewayRunner::recover_on_startup` inspects durable runs:

- `queued` runs are retained and receive `run_requeued_after_restart`.
- `running` runs without terminal events become `recoverable` and receive `run_recovered_after_restart`.
- `waiting_approval` and `waiting_user` runs are retained with a pending-response recovery event and expiry marker.
- Terminal runs remain terminal.

Session locks include owner pid and heartbeat. Stale lock takeover writes a `session_lock_recovered` event.

## Local `/remote` And TUI

`/remote` commands:

```text
/remote status
/remote adapters
/remote connect <telegram|lark>
/remote test-message <telegram|lark> <target> [text]
/remote runs [--limit N]
/remote show <run_id>
/remote events <run_id> [--limit N]
/remote stop <run_id>
/remote doctor
```

`/remote` with no arguments opens the TUI `RemoteSurface`; `/remote status` and other argument forms continue through the text command handler. The TUI status widget can show `remote=off|ok|attention|error` style snapshots without network I/O in the render path.

## Current Non-Goals

- Public hosted remote-control SaaS.
- Multi-tenant public gateway service.
- Full WebSocket bidirectional parity.
- Full Telegram/Lark inbound conversational control.
- Remote desktop or GUI control.
- Real mid-turn `steer`; capabilities report `supportsSteer=false`.
- Replacing headless JSONL IPC or the existing Web UI.
