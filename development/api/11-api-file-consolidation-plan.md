# API File Consolidation Plan

> Purpose: after the API architecture upgrade phases land, consolidate API
> functionality so business logic lives with its domain and shared
> infrastructure remains small and explicit.
>
> Scope: backend repository
> `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`.
> Frontend generated artifacts remain in the paired frontend repository and are
> not moved by this plan.
>
> Status: planning document. The dispatcher, typed serialization queues, API
> JSON-RPC WebSocket, IPC v2 runtime, and IPC v2 WebSocket described here are
> future targets unless explicitly called out as existing code.

## Goal

Do not merge the API into one large file. The target shape is:

- Business API logic is concentrated by domain in
  `crates/allthecodes-web/src/handlers/<domain>.rs`.
- Protocol DTOs are concentrated by domain in versioned protocol modules.
- Shared routing, processor, serialization, error, codegen, and transport
  infrastructure stays in dedicated infrastructure files.
- One central composition/dispatch layer remains responsible for mapping typed
  protocol requests to domain processors and for applying initialization,
  experimental-gate, serialization, and transport rules consistently.
- Temporary compatibility wrappers disappear once each domain is migrated.

This keeps feature ownership clear while avoiding the current split where a
single domain can have logic scattered across `processors.rs`,
`handler_registry.rs`, `handlers/mod.rs`, protocol `v1/mod.rs`, and generated
docs.

## Codex Reference Adjustments

This plan was checked against the local Codex app-server implementation:

- `../codex/codex-rs/app-server/src/message_processor.rs`
- `../codex/codex-rs/app-server/src/in_process.rs`
- `../codex/codex-rs/app-server/src/request_processors.rs`
- `../codex/codex-rs/app-server/src/request_serialization.rs`
- `../codex/codex-rs/app-server-client/src/lib.rs`
- `../codex/codex-rs/app-server-transport/src/transport/mod.rs`
- `../codex/codex-rs/app-server-protocol/src/protocol/common.rs`
- `../codex/codex-rs/app-server-protocol/src/protocol/v2/mod.rs`
- `../codex/AGENTS.md`

The comparison changes the plan in several places:

- Keep a central dispatcher/registry as the composition root. Codex owns
  concrete domain processors in separate files, but `MessageProcessor` still
  centralizes initialization checks, experimental gating, serialization, and the
  `ClientRequest` match.
- Treat request serialization as protocol-derived typed scope, not a loose
  string key. Codex maps `ClientRequest::serialization_scope()` to typed queue
  keys and supports both `Exclusive` and `SharedRead` access.
- Prefer v2 protocol conventions for new stable API surface: domain modules,
  `*Params`/`*Response`/`*Notification` naming, camelCase wire fields, optional
  request fields as `#[ts(optional = nullable)]`, cursor pagination for new list
  methods, and field-level experimental gating where only part of a method is
  experimental.
- Treat transports as adapters over one request lifecycle. Codex has stdio,
  Unix socket, WebSocket, in-process, and remote-control transport entry points,
  but app-server requests converge into one `MessageProcessor`. allthecodes
  should follow that principle for REST, future API JSON-RPC WebSocket, direct
  calls, and future IPC v2 commands that map to typed API requests.
- Keep legacy IPC compatibility separate from new IPC design. The existing
  `/api/ipc/ws` bridge still uses bare `FrontendMessage`/`BackendMessage` and
  should not be rewritten as an envelope transport as part of file
  consolidation. Future IPC v2 work is planned separately in
  `development/code-plan/03-ipc-template-future-application-plan.zh.md`.

Related planning docs:

- `development/code-plan/02-transport-unification-plan.zh.md`
- `development/code-plan/03-ipc-template-future-application-plan.zh.md`

## Desired File Ownership

| Concern | Final owner |
|---|---|
| Domain handlers, domain processors, handler wrappers, domain tests | `crates/allthecodes-web/src/handlers/<domain>.rs` |
| Central composition and request dispatch | `crates/allthecodes-web/src/handler_registry.rs` or a future `api_dispatcher.rs` |
| Registry data structure, validation, route mounting, `/api/v2` mirror, experimental gate | `crates/allthecodes-web/src/handler_registry.rs` |
| Processor trait, generic Axum adapters, shared protocol error conversion | `crates/allthecodes-web/src/processors.rs` |
| Typed serialization scope, queue keys, access mode, and queue draining | `crates/allthecodes-web/src/serialization.rs` |
| Web shared state only | `crates/allthecodes-web/src/state.rs` |
| Protocol macro, endpoint inventory, request/response enums | `crates/allthecodes-protocol/src/{macros.rs,request.rs,response.rs}` |
| Protocol errors | `crates/allthecodes-protocol/src/error.rs` |
| Protocol DTOs | `crates/allthecodes-protocol/src/v1/<domain>.rs` for compatibility; prefer `v2/<domain>.rs` for new stable surface if the v2 mirror becomes primary |
| Codegen implementation and CLI binaries | `crates/allthecodes-protocol/src/codegen.rs` and `src/bin/*.rs` |
| Transport traits and JSON-RPC frames | `crates/allthecodes-protocol/src/transport.rs` |
| API JSON-RPC WebSocket adapter | planned: `crates/allthecodes-web/src/ws/api.rs` after dispatcher exists |
| Legacy IPC WebSocket compatibility bridge | existing: `crates/allthecodes-web/src/ws/ipc.rs`, keep bare `FrontendMessage`/`BackendMessage` wire format |
| IPC v2 protocol payload/adapters | planned: `crates/allthecodes-ipc-protocol/src/{payload.rs,legacy.rs}` or merged `allthecodes-ipc/src/protocol/` |
| Shared IPC runtime | planned: `crates/allthecodes-ipc/src/runtime/` |
| IPC v2 WebSocket adapter | planned: `crates/allthecodes-web/src/ws/ipc_v2.rs` |

## Files To Merge Or Move

### 1. Move domain processors out of `processors.rs`

`crates/allthecodes-web/src/processors.rs` should only keep:

- `Processor`
- `processor_json_handler`
- `processor_no_params_handler`
- `processor_path_handler`
- `process_processor`
- shared protocol error response conversion
- adapter tests with fake processors

Move any concrete domain processor from `processors.rs` into its domain file:

| Current concrete processor | Move to |
|---|---|
| `CapabilitiesProcessor` | `crates/allthecodes-web/src/handlers/capabilities.rs` |
| `SessionListProcessor`, `SessionDetailProcessor`, `SessionResumeProcessor`, `SessionArchiveProcessor` | `crates/allthecodes-web/src/handlers/sessions.rs` |
| future `AgentProcessor` | `crates/allthecodes-web/src/handlers/agents.rs` |
| future `PeopleProcessor` | `crates/allthecodes-web/src/handlers/people.rs` |
| future `FileProcessor` | `crates/allthecodes-web/src/handlers/files.rs` |
| future `SkillProcessor` | `crates/allthecodes-web/src/handlers/skills.rs` |
| future `PluginProcessor` | `crates/allthecodes-web/src/handlers/plugins.rs` |
| future `HookProcessor` | `crates/allthecodes-web/src/handlers/hooks.rs` |
| future `ChatProcessor` | `crates/allthecodes-web/src/handlers/chat.rs` |

### 2. Shrink domain route bulk without losing central dispatch

`crates/allthecodes-web/src/handler_registry.rs` should remain the central
composition and validation owner, equivalent to Codex `MessageProcessor` for
allthecodes' REST/Axum surface. It should keep:

- `HandlerEntry`
- `HandlerRegistry`
- `RegistryValidationError`
- `register_protocol_routes`
- `protocol_routes_handler`
- the central `ApiMethod` to domain processor/handler mapping
- initialization, experimental-gate, serialization, and `/api/v2` mirror rules
- shared route metadata helpers

Domain-specific `*_handlers()` functions may move into matching domain modules
when that materially reduces file size, but the final architecture should not
require every domain to self-register. If a function moves, the central registry
should still call it explicitly.

| Current registry function | Candidate helper owner |
|---|---|
| `chat_handlers()` | `handlers/chat.rs` |
| `session_handlers()` | `handlers/sessions.rs` |
| `capability_handlers()` | `handlers/capabilities.rs` |
| `chat_mode_handlers()` | `handlers/chat_modes.rs` |
| `agent_handlers()` | `handlers/agents.rs` |
| `people_handlers()` | `handlers/people.rs` |
| `hook_handlers()` | `handlers/hooks.rs` |
| `prompt_handlers()` | `handlers/prompts.rs` |
| `plugin_handlers()` | `handlers/plugins.rs` |
| `file_handlers()` | `handlers/files.rs` |
| `skill_handlers()` | `handlers/skills.rs` |
| `kanban_handlers()` | `handlers/kanban.rs` |
| `job_handlers()` | `handlers/jobs.rs` |
| `group_chat_handlers()` | `handlers/group_chat.rs` |
| `backend_service_handlers()` | `handlers/backend_services.rs` |
| all remaining domain-specific `*_handlers()` functions | matching `handlers/<domain>.rs` |

Acceptable centralized shape:

```rust
pub fn all_api_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .extend(handlers::chat::handlers())
        .extend(handlers::sessions::handlers())
        .extend(handlers::capabilities::handlers())
        // ...
}
```

Also acceptable if the match table stays more Codex-like in one file:

```rust
match request.method() {
    ApiMethod::ChatSend => handlers::chat::send(state, request).await,
    ApiMethod::SessionList => handlers::sessions::list(state, request).await,
    // ...
}
```

The hard requirement is not where each route list lives; it is that domain
business logic leaves `handler_registry.rs`, while dispatch semantics remain
central and consistent across REST, future API JSON-RPC WebSocket, direct
transports, and future IPC v2 commands where those paths overlap.

### 3. Move fallback out of capabilities

`api_fallback_handler` is not a capabilities endpoint. Move it from
`crates/allthecodes-web/src/handlers/capabilities.rs` into one of:

- preferred: `crates/allthecodes-web/src/api_errors.rs`
- acceptable: `crates/allthecodes-web/src/handler_registry.rs`

After the move, `capabilities.rs` should only own capability discovery data and
its route/processor.

### 4. Remove legacy web `ApiError`

`crates/allthecodes-web/src/handlers/mod.rs` still contains a compatibility
`ApiError` wrapper for legacy handlers. Replace handler call sites with
`allthecodes_protocol::ApiError` or small domain helper functions that return
`ApiErrorBody`.

Completion target:

- no `pub struct ApiError` in `handlers/mod.rs`
- no new ad-hoc `{ error, code }` structs
- all JSON errors include `details`
- `api_fallback_handler` uses `allthecodes_protocol::ApiError::NotImplemented`

### 5. Split protocol DTOs out of version `mod.rs`

Move session-related protocol types from
`crates/allthecodes-protocol/src/v1/mod.rs` into a dedicated file:

- preferred path: `crates/allthecodes-protocol/src/v1/sessions.rs`

Then re-export them through `v1/mod.rs`:

```rust
pub mod sessions;
pub use sessions::*;
```

Apply the same rule for any future DTOs that are added directly to `v1/mod.rs`.
`v1/mod.rs` should be a module index, not a DTO dump.

Codex keeps active app-server API development in v2 and uses `v2/mod.rs` only as
a domain module index plus re-export layer. If allthecodes' `/api/v2` mirror is
intended to become the stable public surface, add new stable DTOs under
`crates/allthecodes-protocol/src/v2/<domain>.rs` instead of expanding v1, while
leaving existing v1 DTOs available for compatibility.

When adding or moving v2 DTOs, follow the Codex-style contract rules:

- Request payloads end in `Params`, responses in `Response`, notifications in
  `Notification`.
- Wire fields are camelCase unless there is an explicit compatibility reason.
- Optional client request fields use `#[ts(optional = nullable)]`.
- New list APIs use cursor pagination by default: `cursor`, `limit`, `data`,
  and `next_cursor`.
- If only some fields are experimental, derive the local equivalent of
  `ExperimentalApi` and make request inventory inspect params, instead of
  marking the whole endpoint experimental.

### 6. Consolidate tests by domain

Move large domain-specific tests out of
`crates/allthecodes-web/src/handlers/mod.rs` into the owning domain module.

Examples:

| Test group | Move to |
|---|---|
| session archive/detail/resume tests | `handlers/sessions.rs` |
| file read/write/mutation tests | `handlers/files.rs` |
| skills list/detail/patch tests | `handlers/skills.rs` |
| kanban board/task tests | `handlers/kanban.rs` |
| jobs/cron tests | `handlers/jobs.rs` |
| group chat tests | `handlers/group_chat.rs` |
| backend services tests | `handlers/backend_services.rs` |

Keep only genuinely cross-domain handler export tests in `handlers/mod.rs`.
Protocol-level invariants, schema compatibility, and experimental-gating tests
may stay in protocol version test modules such as `v2/tests.rs`; they do not need
to be forced into every DTO domain file.

### 7. Replace SessionOwnership with typed serialization queues

The API architecture cleanup is not fully complete until `SessionOwnership` no
longer exists as a parallel concurrency mechanism.

Final ownership:

- Protocol request metadata produces a typed serialization scope, not just a
  free-form key string.
- `SerializationLayer` maps typed scope to typed queue keys and access mode.
- Access modes include at least `Exclusive` and `SharedRead`; consecutive shared
  reads for the same key may run concurrently, while writes preserve FIFO order.
- Long-lived or connection-local operations use connection-scoped queue keys when
  needed so one client does not block unrelated clients.
- `WebState` exposes serialization primitives, not `try_claim_chat`,
  `try_claim_tui`, `try_claim`, or `release_owner`.
- chat, legacy IPC, future IPC v2, direct transports, and any PTY operation that
  mutates shared session state submit work through the same serialization path
  instead of manual claim/release calls.

This should be the last consolidation step because streaming lifecycle cleanup is
more fragile than ordinary REST handlers.

### 8. Keep API transport entry points behind one dispatcher

Codex has both a JSON-RPC path and an in-process typed `ClientRequest` path, but
both delegate to the same `handle_client_request` implementation. allthecodes
should keep the same principle:

- REST/Axum handlers decode HTTP details and build typed protocol requests.
- The future API JSON-RPC WebSocket decodes frames and builds the same typed
  requests.
- Direct/in-process transports bypass JSON decoding but still call the same
  dispatch and serialization path.
- Future IPC v2 commands that are real API operations decode to the same typed
  requests; legacy `/api/ipc/ws` remains a compatibility adapter until it is
  intentionally migrated.
- Domain handlers do not reimplement transport-specific request validation,
  experimental checks, or error envelope conversion.
- PTY terminal WebSocket, MCP client transports, browser native-host transport,
  and daemon SSE replay are not forced into this API request/response dispatcher.

### 9. Keep IPC consolidation staged and compatibility-first

The current IPC WebSocket is not an ordinary REST/API transport. It is an
interactive frontend bridge that installs callbacks, streams assistant/tool
events, and resolves permission/question interactions through
`FrontendMessage`/`BackendMessage`.

Consolidation target:

- Keep `/api/ipc/ws` wire-compatible while file moves happen.
- Move reusable callback, pending interaction, event classification, and
  outbound queue logic toward shared IPC modules only after behavior is covered
  by tests.
- Do not introduce `/api/v2/ipc/ws` in this file consolidation pass. Treat it as
  a future transport adapter that depends on:
  - typed `ApiDispatcher`
  - typed serialization queues
  - IPC v2 payload/envelope definitions
  - explicit `ServerRequest` handling for permission/question flows
- When `SubmitPrompt`, `AbortQuery`, file search, completions, or subsystem
  commands gain stable `ClientRequest` DTOs, migrate their internal execution to
  the dispatcher while keeping legacy outbound `BackendMessage` compatibility.
- Keep terminal resize/control on PTY WebSocket, not IPC v2.

## Execution Phases

### Phase A: Low-risk file moves

1. Move `api_fallback_handler` out of `capabilities.rs`.
2. Move `CapabilitiesProcessor` into `capabilities.rs`.
3. Move session processors into `sessions.rs` if any remain outside it.
4. Run:
   ```bash
   cargo fmt --all --check
   cargo check -p allthecodes-web
   cargo test -p allthecodes-web handler_registry::tests
   ```

### Phase B: Central dispatch cleanup

1. Keep `handler_registry.rs` as the central composition and validation owner.
2. Move domain business logic out of `handler_registry.rs` into matching
   `handlers/<domain>.rs` files.
3. Move only route-list helpers that reduce bulk; keep an explicit central
   aggregator or central `ApiMethod` dispatch match.
4. Ensure REST, future API WebSocket, future direct transports, and future IPC
   v2 commands do not each own separate business dispatch rules.
5. Keep legacy `/api/ipc/ws` behavior unchanged during this phase.
6. Run:
   ```bash
   cargo test -p allthecodes-web handler_registry::tests
   cargo check -p allthecodes-web
   ```

### Phase C: Protocol DTO cleanup

1. Move session DTOs from `v1/mod.rs` to `v1/sessions.rs`.
2. If new stable v2 surface is needed, create `v2/<domain>.rs` modules instead
   of expanding v1.
3. Ensure generated request/response unions are unchanged except import/type
   ordering if expected.
4. Regenerate artifacts:
   ```bash
   cargo run -p allthecodes-protocol --bin codegen
   cargo run -p allthecodes-protocol --bin codegen -- --check
   ```
5. Run:
   ```bash
   cargo test -p allthecodes-protocol
   ```

### Phase D: Error unification

1. Replace legacy `handlers::ApiError` call sites domain by domain.
2. Delete `handlers::ApiError`.
3. Add or update tests that assert `details` exists on error bodies.
4. Run:
   ```bash
   cargo check -p allthecodes-web
   cargo test -p allthecodes-web
   ```

### Phase E: Test relocation

1. Move tests from `handlers/mod.rs` to their domain modules.
2. Keep shared test helpers private to `handlers/mod.rs` only if they are truly
   cross-domain; otherwise duplicate small helpers locally.
3. Run:
   ```bash
   cargo test -p allthecodes-web
   ```

### Phase F: SessionOwnership retirement

1. Add protocol-derived typed serialization scopes and access modes.
2. Replace semaphore-only guards with FIFO queues that support `Exclusive` and
   batched `SharedRead` access per typed key.
3. Replace chat, legacy IPC, future IPC v2, and direct claim-release calls with
   `SerializationLayer` queue submission or scoped guards that use the same
   queue keys where they touch shared engine/session state.
4. Keep PTY byte-stream ownership separate from API dispatch, but use
   connection-scoped queue keys for any PTY operation that mutates shared
   session state.
5. Delete `SessionOwner` and `SessionOwnership`.
6. Remove `WebState` claim/release wrapper methods.
7. Add tests for FIFO ordering, different-key concurrency, shared-read batching,
   conflicting session operations, closed connection cleanup, and stream cleanup.
8. Run:
   ```bash
   cargo check -p allthecodes-web
   cargo test -p allthecodes-web
   ```

### Phase G: Transport adapter alignment

This phase only begins after the dispatcher and typed serialization queues are
usable. It should not be mixed with low-risk file moves.

1. Keep REST as the stable HTTP adapter and route it through the central
   dispatcher for migrated endpoints.
2. Add or wire the direct/in-process adapter only through the same dispatcher;
   do not let it call domain handlers through a second path.
3. If an API JSON-RPC WebSocket is added, put it in `ws/api.rs` and use
   `allthecodes_protocol::JsonRpcFrame`; do not reuse IPC or PTY wire formats.
4. Keep `/api/ipc/ws` as a legacy compatibility bridge. First extract reusable
   pending interaction, callback, queue, and adapter code; only then consider
   internal dispatcher routing for stable `ClientRequest` commands.
5. Do not create `/api/v2/ipc/ws` in this consolidation plan. That route belongs
   to the future IPC v2 plan and requires `IpcPayload`, shared IPC runtime, and
   explicit server-request handling.
6. Run:
   ```bash
   cargo test -p allthecodes-web --lib
   cargo test -p allthecodes-protocol
   cargo test -p allthecodes-ipc-protocol
   cargo test -p allthecodes-ipc-transport
   ```

## Acceptance Criteria

- `handler_registry.rs` or `api_dispatcher.rs` remains the central composition
  and dispatch owner; it no longer contains domain business logic.
- Domain route-list helpers may live with domain handlers, but central startup
  validation and dispatch semantics remain explicit in one place.
- `processors.rs` contains processor infrastructure only; no concrete domain
  processor remains there.
- `capabilities.rs` no longer owns fallback behavior.
- `handlers/mod.rs` no longer owns a legacy `ApiError` type or large
  domain-specific test suites.
- Version `mod.rs` files are module indexes and re-export layers; domain DTOs
  live in `v1/<domain>.rs` or `v2/<domain>.rs`.
- New stable protocol work follows v2 naming, camelCase, optional-field,
  pagination, and experimental-gate conventions.
- `SessionOwnership` and manual `try_claim_*`/`release_owner` APIs are removed.
- `SerializationLayer` uses typed queue keys and `Exclusive`/`SharedRead` access
  modes.
- REST, future API JSON-RPC WebSocket, direct transports, and future IPC v2
  commands share one typed dispatch and serialization path where they overlap.
- Legacy `/api/ipc/ws` remains wire-compatible while reusable IPC internals move
  toward shared modules.
- `/api/v2/ipc/ws` is not introduced by this consolidation plan; it remains a
  future IPC v2 implementation target.
- Generated frontend API artifacts pass freshness checks.
- These commands pass:
  ```bash
  cargo fmt --all --check
  cargo check -p allthecodes-protocol -p allthecodes-web
  cargo test -p allthecodes-protocol
  cargo test -p allthecodes-web handler_registry::tests
  cargo run -p allthecodes-protocol --bin codegen -- --check
  ```

## Non-goals

- Do not collapse all handlers into one file.
- Do not create a new web server crate.
- Do not move frontend generated files into the backend repository.
- Do not change `/api/ipc/ws` wire format.
- Do not introduce `/api/v2/ipc/ws` as part of file consolidation.
- Do not force PTY terminal WebSocket, MCP client transports, browser
  native-host transport, or daemon SSE replay into the API dispatcher.
- Do not mix this consolidation with unrelated clippy cleanup.

## Implementation Review (2026-06-07)

> Reviewed against the current codebase to check whether the plan's prescriptions
> match reality and to identify drift, stale assumptions, and unstarted phases.

### Phase A — Completed (not yet marked in the plan)

The following Phase A steps are already done in the current worktree but the plan
still lists them as future work:

1. `api_fallback_handler` moved from `capabilities.rs` → `api_errors.rs`. ✅
2. `CapabilitiesProcessor` lives in `capabilities.rs`, not `processors.rs`. ✅
3. Session processors (`SessionListProcessor`, `SessionDetailProcessor`,
   `SessionResumeProcessor`, `SessionArchiveProcessor`) live in `sessions.rs`. ✅
4. `processors.rs` contains processor infrastructure only: `Processor` trait,
   adapter functions (`processor_json_handler`, `processor_no_params_handler`,
   `processor_path_handler`, `process_processor`), and fake-processor tests.
   No concrete domain processor remains there. ✅
5. `v1/sessions.rs` DTO split completed; `v1/mod.rs` is a module index. ✅

**Action**: mark Phase A steps as done and update the document timeline, or
re-title Phase A as "completed groundwork" so new contributors don't re-do it.

### Phase B — `*_handlers()` functions still live in `handler_registry.rs`

The plan section 2 says domain-specific `*_handlers()` functions may move into
matching `handlers/<domain>.rs` modules. Currently all 25+ of them
(`chat_handlers()`, `session_handlers()`, `capability_handlers()`, etc.) remain
in `handler_registry.rs` (lines 140–812, ~670 lines of routing boilerplate).

Only `all_api_handlers()` aggregation + dispatch semantics should stay central.
When a new domain is added, the pattern of appending yet another `*_handlers()`
function to `handler_registry.rs` reinforces the old layout.

**Suggestion**: pick 2–3 high-churn domains (Session, Files, Skills) as a
Phase B pilot. Move their `*_handlers()` functions and update
`handler_registry.rs::all_api_handlers()` to call the domain module. Consolidate
the rest after the pattern is proven.

```rust
// handler_registry.rs — central aggregator only
pub fn all_api_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .extend(handlers::chat::handlers())
        .extend(handlers::sessions::handlers())
        // ...
}
```

### Phase C — Protocol DTO cleanup is done

`v1/sessions.rs` exists and `v1/mod.rs` re-exports it. No v2 modules have been
created yet, but the plan allows adding them when new stable surface is needed.
No drift detected.

### Phase D — Error unification is partial

**What's done**: the legacy `pub struct ApiError` was removed from
`handlers/mod.rs`. It is now re-exported from `api_errors.rs`:

```rust
// api_errors.rs
pub type ApiError = ApiErrorBody;
```

**What remains**: approximately 20 error construction sites in `sessions.rs`
still use the `Json(ApiError { error, code, details })` inline pattern rather
than `ProtocolApiError` enum variants (`ProtocolApiError::NotFound`,
`ProtocolApiError::Conflict`, etc.). The inline pattern works correctly but
creates a split personality: some domains use protocol enums, others hand-roll
bodies.

This is not functionally broken (the `ApiErrorBody` shape is the same either
way), but it makes systematic validation of error output harder.

**Items to address before closing Phase D**:

- Decide whether the target is exclusively `ProtocolApiError` variants or whether
  `ApiErrorBody` helper constructors in `api_errors.rs` are acceptable.
- If the latter, add helper constructors and migrate inliners.
- Add a test that asserts `details` is present (non-empty object) on every
  error body — the plan mentions this but no such test exists yet.

### Phase E — Test relocation not started (highest-severity gap)

`handlers/mod.rs` contains a single `#[cfg(test)]` block spanning lines 130–2721
(**~2600 lines of domain-specific tests**). All of the following test groups
named in the plan still live there:

| Listed test group | Location | Lines |
|---|---|---|
| Session archive/detail/resume | `mod.rs` | 582–631 |
| Files read/write/mutation | `mod.rs` | 1598–2273 |
| Skills list/detail/patch | `mod.rs` | 2279–2720 |
| Kanban board/task | `mod.rs` | (in `mod.rs` tests) |
| Jobs/cron | `mod.rs` | (in `mod.rs` tests) |
| Group chat | `mod.rs` | (in `mod.rs` tests) |
| Backend services | `mod.rs` | (in `mod.rs` tests) |

The corresponding domain modules (`handlers/files.rs`, `handlers/skills.rs`,
`handlers/kanban.rs`, etc.) exist and are substantial, but their tests remain in
the parent module. This directly contradicts the plan's ownership goal: "domain
business logic lives with its domain." A 2600-line test module in `mod.rs`
discourages contributors from adding tests alongside new handler code and makes
`mod.rs` the path of least resistance for piling on more tests.

**Suggested approach**:

1. Move `#[cfg(test)]` tests into `crate::handlers::<domain>::tests` one domain
   at a time. Each move is a mechanical copy-and-grep:
   - Move the test function and any domain-specific helpers.
   - Adjust import paths (`crate::handlers::foo::FooRequest` →
     `super::FooRequest`).
   - Keep truly shared test infrastructure (`EnvGuard`, `make_web_state`,
     `response_json`, `temp_home`) in `mod.rs` as a `pub(crate)` helper module
     or keep them re-exported via the wildcard.
2. Small domains (Kanban, Jobs/Cron, Group Chat, Backend Services) can be moved
   in a single commit. Files and Skills are larger and should be moved
   individually.
3. After each move, run `cargo test -p allthecodes-web` and confirm the same
   test count.

After all moves, `handlers/mod.rs` tests shrink to ~20 lines of shared helpers
plus the genuinely cross-domain protocol/route-registration tests.

### Phase F — `SessionOwnership` still active

`SessionOwner`, `SessionOwnership`, `try_claim_owner`, and `release_owner` all
still exist in `serialization.rs` (lines 14–90). `WebState` wrapper methods
(`try_claim_chat`, `try_claim_tui`, `try_claim`, `release_owner`) remain in
`state.rs` (lines 62–80). Call sites across `chat.rs`, `ws/ipc.rs`,
`sessions.rs`, and `admin.rs` still use them. The plan correctly identifies this
as the last step, but the gap between current state and target is wide.

**Current serialization architecture**:

```
SerializationLayer
├── run_scoped()         — per-key semaphore for request-level serialization
│   └── Semaphore(1)     — always mutex, no SharedRead batching
└── SessionOwnership     — parallel claim/release for long-lived connections
    ├── try_claim_owner()
    └── release_owner()
```

**Target per plan**:
```
SerializationLayer
├── run_scoped()         — typed scope → FIFO queue
│   ├── Exclusive        — mutex for write operations
│   └── SharedRead       — concurrent reads for same key
└── (no separate ownership layer — long-lived connections use
     PerConnection-scoped queue keys)
```

The plan's `SerializationScope` enum already exists (`Concurrent`, `PerProcess`,
`PerConnection`, `PerKey`) but the queue implementation (`Semaphore::new(1)`)
treats all scopes as exclusive. No `SharedRead` path exists. No FIFO queue
replaces the semaphore.

**Suggestion**: before starting Phase F, add typed access modes to the
`SerializationScope` or create a separate `AccessMode` enum:
```rust
pub enum AccessMode {
    Exclusive,
    SharedRead,
}
```
Then extend `run_scoped()` to create a `tokio::sync::RwLock` (or equivalent)
per queue key instead of a `Semaphore(1)`, so shared reads proceed concurrently
while exclusive writes wait for all readers to finish.

### Cross-cutting observations

#### 1. Plan-to-code alignment on SerializationScope

The plan's Phase F describes typed serialization scopes as future work, but
`SerializationScope` (defined in `crates/allthecodes-protocol/src/macros.rs`
lines 137–160) and the `Processor` trait's `serialization_scope()` method
already exist in `processors.rs`. The *macro-generated* `ClientRequest` enum
already derives a `serialization_scope()` per operation:

```
SerializationScope::Concurrent   — most read-only endpoints
SerializationScope::PerProcess   — settings apply, command run
SerializationScope::PerConnection — chat, abort
SerializationScope::PerKey       — session operations (keyed by session id)
```

But at runtime `SerializationLayer` in `serialization.rs` maps all of these to
the same `Semaphore(1)` guard, so the protocol-level distinction is inert. The
infrastructure for typed scopes exists at the protocol layer; only the runtime
serialization layer needs upgrading.

#### 2. Code paths not forced into the dispatcher (correct)

PTY WebSocket (`ws/terminal.rs`), MCP client transports, browser native-host
transport, and daemon SSE replay all remain outside the API dispatcher. The plan
says this is intentional — no drift.

#### 3. Legacy IPC WebSocket bridge untouched (correct)

`/api/ipc/ws` (in `ws/ipc.rs`) still uses bare `FrontendMessage`/
`BackendMessage` wire format. It has not been rewritten. The plan's "keep
compatibility" rule is followed.

### Summary of recommended next actions

| Priority | Action | Effort | Risk |
|---|---|---|---|
| High | Update Phase A status to completed | trivial | prevents confusion |
| High | Begin Phase E test relocation (Files domain first, then Skills, then smaller domains) | 2–3 small PRs | low — mechanical, easy to verify |
| Medium | Phase B pilot: move `session_handlers()`, `file_handlers()`, `skill_handlers()` to domain modules | 1 PR | low — the pattern is well-understood |
| Medium | Phase D: decide error strategy, add helpers, migrate inliners | 1 PR | low |
| Low | Phase F: add `AccessMode::SharedRead` to `SerializationScope` | 1 PR | medium — touches streaming lifecycle |
| Low | Phase F: replace semaphore with FIFO queue | separate PR after access modes | medium — concurrency correctness |
| Low | Phase F: remove `SessionOwnership` and `try_claim_*` | final Phase F PR | high — most fragile step |
