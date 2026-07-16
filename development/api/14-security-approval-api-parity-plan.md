# Security Approval API Transport Parity Plan

> Status: Implemented on 2026-07-16; complete Web transport E2E remains pending
> Priority: P0
> Scope: Web chat permission events/responses and Web IPC permission projection

## Implementation Result (2026-07-16)

Implemented in `72bce1b4` and described by the backend artifacts refreshed in
`e5163791`:

- Web chat SSE and Web IPC preserve the normalized operation and display-safe
  `SecurityDecisionDisplay` supplied by the engine;
- exact `allow` requires a matching single-use response binding, while binding
  mismatch, replay, cross-session resolution, and exact `always_allow` fail
  closed;
- exact `deny` remains available without turning the request into a reusable
  rule; and
- the dynamic permission-response route requires the privileged capability
  before pending state can be consumed.

Protocol, runtime, handler, and Web-state regression tests cover the typed
payload and one-shot response rules. The plan-specific full Web E2E that
observes the SSE event, submits the bound response, and proves modified/replayed
approval cannot execute the request has not yet been added; do not treat the
unit/integration coverage as that end-to-end evidence.

## Audit Snapshot Before Implementation

Exact workflow-injection approval is not an engine gap. The engine already:

- creates a redacted `SecurityDecisionDisplay` with `exact_approval = true`;
- offers only `Allow once` and `Deny` for the security decision;
- accepts only the normalized `allow` decision, so `always_allow` cannot create
  a reusable rule; and
- records the decision against the exact tool request and its source digests.

The implementation is in
`crates/allthecodes-engine/src/lifecycle/deps/tool_pipeline.rs`. Regression
coverage exists in that module, engine lifecycle tests, and
`crates/allthecodes/tests/agentic_workflow_injection_e2e.rs`.

The gap is a Web transport projection:

| Surface | Current behavior |
|---|---|
| Shared callback contract | `crates/allthecodes-types/src/callbacks.rs::PermissionRequestPayload` carries `operation: Option<ToolOperation>` and `security: Option<SecurityDecisionDisplay>` |
| Standard IPC callback/wire | `crates/allthecodes-ipc-protocol/src/protocol/mod.rs` and `normalized.rs` can carry both fields; `crates/allthecodes-ipc/src/client/callbacks.rs` forwards them |
| Web IPC WebSocket | `crates/allthecodes-web/src/ws/ipc.rs` calls `IpcRuntime::request_permission`, whose server-request params include `operation` but currently omit `security` |
| Web chat SSE | `crates/allthecodes-web/src/handlers/chat.rs::ChatPermissionRequestEvent` copies only tool, command, input, and options, dropping `operation` and `security` |
| Web chat response authorization | `POST /api/chat/permissions/{tool_use_id}/response` is not matched by `requires_privileged_capability`, so the normal control credential can currently reach a security-sensitive pending-request mutation |

Consequently, a client consuming `POST /api/chat` loses both structured fields,
while a client on `/api/ipc/ws` loses the exact-security decision. Neither Web
transport has full parity even though the engine made the distinction, and the
Web chat response mutation is not yet protected by the privileged capability.

## Goals

1. Give Web chat and Web IPC clients the display-safe exact-approval context
   supported by the standard IPC contract.
2. Keep the current SSE event name and legacy fields compatible.
3. Expose only display-safe security metadata.
4. Bind an exact approval response to one session, tool request, and pending
   approval instance.
5. Reject duplicate, stale, mismatched, timed-out, and replayed approvals.
6. Require the Web privileged capability before any permission response can
   resolve a pending tool request.

## Non-Goals

- Do not redesign engine taint policy or exact-approval enforcement.
- Do not add a second permission callback type.
- Do not weaken the standard IPC callback/wire contract to match either lossy
  Web projection.
- Do not persist raw prompts, remote payloads, tool output, or approval tokens.
- Do not make `always_allow` valid for an exact security request.

## Typed Event Contract

Move the handler-private Web event into
`crates/allthecodes-protocol/src/v1/chat.rs` and use that type from
`crates/allthecodes-web/src/handlers/chat.rs`.

Keep these existing fields and meanings unchanged:

```text
type = "permission_request"
session_id
tool_use_id
tool
command
input
options
```

Add optional, additive fields:

```text
operation
security
response_binding
```

- `security` is the serialized `SecurityDecisionDisplay`. Its public fields are
  limited to `sink`, `decision`, `rule_ids`, `source_labels`,
  `source_digests`, and `exact_approval`.
- `operation` is a display projection of the supplied `ToolOperation`. The Web
  serializer must not add `raw_output`, secret-bearing side-channel values, or
  any data not already authorized for the existing `command`/`input` event.
- `response_binding` is an opaque, random, single-use value. It is not a digest
  of raw tool input and must never be logged.

The shared `PermissionRequestPayload` remains the source object. The Web
adapter must map all fields deliberately instead of rebuilding security state
from `message`, `tool_name`, or options.

Moving the event type into `allthecodes-protocol` is not sufficient to make it
part of generated contracts. `ApiMethod::Chat` currently declares `Value` as
its response, and the generators only traverse request/response types referenced
by `API_METADATA`. Add an explicit codegen-visible stream-event reference (for
example, `ChatStreamEvent` metadata associated with `Chat`) and make
`SecurityDecisionDisplay`, or a deliberately identical protocol projection,
implement `JsonSchema`. The TypeScript, JSON Schema, and OpenAPI outputs must
therefore describe the permission event rather than silently omitting it.

## Operation Normalization

Do not assume `PermissionRequestPayload.operation` is present. The engine's
exact-security callback currently sets `operation: None`; the standard IPC
client path fills that gap with
`ToolClassifier::classify_permission(tool_name, tool_input, message, ...)`.

Extract or reuse one shared display projection that:

1. preserves a supplied operation when present;
2. applies the same classifier fallback when it is absent; and
3. copies the already-redacted security display without trying to infer it.

Both Web chat and Web IPC must use this projection so "parity" includes the
fallback classification, not just serialization of fields supplied by the
caller.

## Web IPC Permission Projection

Keep the existing `/api/ipc/ws` route and `FrontendMessage`/`BackendMessage`
wire. Update `allthecodes_ipc::runtime::IpcRuntime::request_permission` so its
`ServerRequestEnvelope.params` deliberately includes the supplied
`request.security` beside the normalized operation. The existing payload
adapter can then produce the normalized/legacy permission event without
inventing a second DTO.

Add a regression at the `IpcRuntime` boundary: an exact-security request sent
through the runtime must emerge with the same redacted
`SecurityDecisionDisplay` fields as the standard IPC client callback path.
Absence remains valid for normal permissions. Do not add chat-only
`response_binding` semantics to the IPC wire unless a separate replay audit
proves its existing request-id/pending-interaction consumption is insufficient.

## Backward Compatibility

- New event fields use Serde defaults and omission when absent. Existing
  clients can continue decoding the old shape.
- Keep SSE event name `permission_request` and the current response route:
  `POST /api/chat/permissions/{tool_use_id}/response`.
- Add this dynamic POST path to
  `crates/allthecodes-web/src/mod.rs::requires_privileged_capability`. A control
  token alone must not be able to approve, deny, or consume a pending tool
  request. This is an intentional authorization hardening; clients that submit
  permission responses must send the privileged credential.
- For the Web chat route, add optional `response_binding` to
  `allthecodes_protocol::v1::chat::ChatPermissionResponseRequest`; remove the
  duplicate handler-local request struct.
- A legacy response without a binding remains valid for a normal permission
  request.
- For `security.exact_approval = true`, `deny` remains fail-closed and may be
  accepted without a binding, but `allow` requires the matching binding.
- Reject `always_allow` for an exact request with a stable 400 error instead of
  silently presenting it as reusable approval. The engine remains the final
  enforcement layer.

## Pending Request and Replay Rules

For Web chat, replace the Web state's bare oneshot sender with a pending-entry record in
`crates/allthecodes-web/src/state.rs`. The entry owns:

```text
session_id
tool_use_id
sender
exact_approval
request_fingerprint
response_binding_hash
created_at / expiry
```

Rules:

1. Build `request_fingerprint` from a canonical, server-side representation of
   the tool name, tool-use ID, input, operation classification, and redacted
   security decision. Do not send or log the fingerprint.
2. Hash the response binding before storing it and compare in constant time.
3. Reject duplicate insertion for the same `(session_id, tool_use_id)` rather
   than overwriting a live sender.
4. Resolve by `(session_id, tool_use_id)` and verify the binding before taking
   the entry.
5. Remove the entry atomically before delivering an accepted response. A
   second response receives `409 stale_permission_response`.
6. A binding mismatch receives a stable conflict response and cannot consume
   or approve a different request. Rate-limit repeated mismatches.
7. Timeout, SSE delivery failure, stream cancellation, engine replacement, and
   abort all remove the pending entry and resolve deny where possible.
8. Never allow a response for one session to resolve the same tool-use ID in
   another session.

The existing remove-on-resolve behavior is a useful base, but the new record is
required so exact and normal requests can be handled differently without
trusting client-supplied flags.

## Implementation Tasks

### Task 1: Add the protocol DTO

- Add the typed permission SSE event and optional response binding in
  `crates/allthecodes-protocol/src/v1/chat.rs`.
- Reuse `SecurityDecisionDisplay`; do not introduce an independently evolving
  security struct. Add the schema support needed for that referenced type.
- Extend API/codegen metadata so chat stream event variants are traversed even
  though the HTTP streaming response is currently declared as `Value`.
- Add Serde and schema tests for old and new payload shapes.

### Task 2: Preserve the payload in both Web adapters

- Update `crates/allthecodes-web/src/handlers/chat.rs` to project the normalized
  operation and supplied `security` into the typed event.
- Apply the Web display-redaction rule before serialization.
- Generate a response binding only for the pending request instance.
- Update `crates/allthecodes-ipc/src/runtime.rs::request_permission` to include
  normalized operation plus `request.security` in the server-request params
  used by `/api/ipc/ws`.
- Reuse the existing IPC payload/normalized adapters; do not create a second
  WebSocket-only permission event.

### Task 3: Bind and consume responses once

- Replace the sender-only map in `crates/allthecodes-web/src/state.rs` with the
  pending-entry record.
- Classify `POST /api/chat/permissions/{tool_use_id}/response` as a privileged
  dynamic route before the handler reads or consumes pending state.
- Update `chat_permission_response_handler` to validate exact-request decision
  rules and the response binding.
- Preserve the existing session/tool-use lookup and stable stale-response
  conflict code.

### Task 4: Update generated contracts

Regenerate, and assert that the typed permission stream event and
`SecurityDecisionDisplay` are present:

```text
docs/api/routes.md
docs/api/schema.json
docs/api/openapi.json
allthecodes-web/src/lib/generated/api-types.ts
allthecodes-web/src/lib/generated/api-routes.ts
allthecodes-web/src/lib/generated/api-schema.json
```

## Required Tests

### Protocol

- Legacy event JSON without the new fields still deserializes.
- `operation`, redacted `security`, and `exact_approval` round-trip.
- An exact request with `operation=None` receives the same fallback
  classification as standard IPC.
- `response_binding` is optional in the response DTO.

### Web handler/state

- Normal permission events preserve current fields and behavior.
- A missing/invalid control token returns 401 and a missing/invalid privileged
  capability returns 403; neither case consumes the pending request.
- An authorized normal or exact response reaches handler validation only after
  both Web authentication layers succeed.
- Exact events include `security.exact_approval = true` and operation display.
- Raw taint payloads, raw output, and the stored binding hash never appear.
- A matching exact `allow` resolves once.
- Missing or wrong binding cannot allow an exact request.
- Exact `always_allow` is rejected; exact `deny` remains fail-closed.
- Cross-session responses, duplicate insertions, timeout responses, and a
  second response are rejected.
- SSE send failure and aborted streams remove pending state.

### Web IPC runtime

- A normal request without security remains backward compatible.
- An exact request round-trips `sink`, decision, rule/source metadata, digests,
  and `exact_approval=true` through `IpcRuntime::request_permission`.
- An absent operation uses the shared fallback classifier rather than remaining
  absent or diverging from standard IPC.
- The serialized WebSocket event contains no raw taint payload or secret.
- Existing IPC pending request-id consumption still rejects stale/duplicate
  responses.

### End to end

- Extend `agentic_workflow_injection_e2e` or add a Web-focused equivalent that
  observes the SSE event, submits the bound response, and proves modified or
  replayed approval cannot execute the request.
- Confirm standard IPC remains unchanged and `/api/ipc/ws` now preserves the
  same display-safe `security` field instead of omitting it.

## Acceptance Criteria

- Web chat, Web IPC, and standard IPC communicate the same display-safe
  exact-approval semantics.
- Old clients continue decoding normal permission prompts; response clients
  must adopt the privileged credential required by the hardened mutation route.
- Exact approval is visibly single-use and cannot be promoted to a reusable
  allow rule.
- No response can cross session/request boundaries or be replayed.
- Security metadata remains redacted and generated API artifacts match source.
