# Remote Control Gateway Current State

Updated: 2026-05-17

This document records the current release boundary for the remote-control gateway. The gateway control plane, daemon route wiring, local `/remote` surface, and outbound Telegram/Lark adapter control path now exist; inbound channel conversations and scheduled remote triggers remain intentional follow-up work.

## Current Runtime Surfaces

| Surface | Current role | Remote-control boundary |
| --- | --- | --- |
| `crates/gateway` | Durable remote-control control plane. | Owns remote source/session/run models, gateway HTTP API, auth policy, adapter registry, durable run events, delivery, recovery, redaction, and outbound adapter transports. It must not depend on `claude-code-rs` internals. |
| `crates/claude-code-rs/src/daemon` | Existing background HTTP/SSE service and worker/supervisor host. | Execution host only. It bridges gateway commands/events to the existing worker protocol and `QueryEngine`; it must not absorb remote-control policy, adapter logic, or durable run-store ownership. |
| `crates/claude-code-rs/src/ipc` | Existing local headless JSONL bridge for a frontend process. | Local UI bridge only. Gateway must not reuse `ipc::headless::run_headless`, must not expose `FrontendMessage` as a remote API, and must not model Telegram/Lark events as IPC messages. Shared protocol types should move to `cc-types` or a small protocol crate if needed later. |
| `/api/*` daemon routes | Existing local daemon control API for submit/status/history/permission and related operations. | Not a public remote-control API. These routes remain local/daemon compatibility endpoints and must not be documented or treated as the stable external control surface. |
| `/remote-control/v1/**` | Stable external gateway API for capabilities, runs, events, approval/ask-user, webhooks, and adapter status/connect/test-message. | Loopback calls use the daemon control token. Non-loopback mode must fail closed without a configured remote token and origin policy. |
| `/remote` slash command | Local user operation surface implemented in `claude-code-rs`, backed by the loopback gateway API/local client. | Mutating actions go through the gateway client and must not write worker command files directly. |
| TUI `RemoteSurface` and status indicator | Local discoverability and status UI implemented in `claude-code-rs`. | Render paths use cached snapshots and must not perform blocking network IO. |

## Existing Daemon Baseline

- The daemon starts behind the `FEATURE_KAIROS=1` gate and defaults to loopback binding.
- Current daemon HTTP routes include `/api/*`, `/webhook/*`, `/events`, and `/health`.
- Gateway daemon routes mount `/remote-control/v1/**` for capabilities, runs, events, approval/ask-user, webhooks, and adapter operations.
- Mutating daemon routes already require the local daemon control token, but this token and route family are not sufficient to make `/api/*` safe as a public remote-control API.
- Gateway run events are persisted as durable gateway events; daemon SSE remains a local event convenience and is not the public remote-control event contract.

## Boundary Decisions

- `crates/gateway` is the control plane. It owns transport-neutral remote identity, run identity, busy policy, persistent run state, remote API shape, adapter status, delivery, and security diagnostics.
- The daemon is the execution host. It owns process supervision, worker command/event files, and the bridge from gateway commands to `QueryEngine`.
- `ipc` is the local headless UI bridge. It remains separate from gateway transport and recovery semantics.
- `/remote` and TUI surfaces are local user entry points. They belong in `claude-code-rs` and call the gateway through a local client; they do not belong in `crates/gateway`.
- `/api/*` is not the public remote-control API. The stable external surface is `/remote-control/v1/**`.
- Telegram and Lark are first-wave gateway adapters. Their first release path is limited to outbound HTTP connection checks, provider-neutral status diagnostics, and allowlisted test-message sending. Inbound conversation control and full remote session parity are follow-up work.

## Current Gaps To Preserve

- Telegram/Lark inbound channel sessions are intentionally out of this release slice.
- Scheduled remote triggers and full remote session parity are intentionally out of this release slice.
- Real mid-turn `steer` is still unsupported; capabilities must report that clearly until `QueryEngine` has explicit injection semantics.
- Public hosted gateway and multi-tenant SaaS operation are non-goals for this release.
- Real third-party adapter credentials are not required for tests; fake HTTP server tests cover outbound transport behavior without leaking secrets.

## Verification Anchor

Use this document with:

```bash
rg -n "remote-control|Telegram|Lark|ipc|/remote|gateway" docs/reference docs/IMPLEMENTATION_GAPS.md docs/plan
```
