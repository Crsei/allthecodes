# API Architecture Upgrade Plan — Adopt Codex Patterns

> Based on analysis of `codex` (at `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex`)
> and current `allthecodes` API layouter (at `crates/allthecodes-web/src/mod.rs`).
> Original plan date: 2026-06-07
>
> Historical architecture plan: implementation status is tracked in
> [the implementation review](10-api-architecture-implementation-review.md),
> and current generated-artifact ownership/freshness requirements are in
> [the freshness plan](17-api-generated-artifact-freshness-plan.md).

## Current Situation

`allthecodes` builds its web API via a monolithic `build_router()` function in
`crates/allthecodes-web/src/mod.rs` (~520 lines) that chains ~150+ `.route()` calls
manually. Each endpoint has a dedicated handler function in a domain submodule under
`crates/allthecodes-web/src/handlers/`. The system works but has structural pain
points that grow as the API expands:

- **No single API surface definition.** There is no authoritative list of all
  endpoints, their request/response schemas, or their serialization semantics. To
  discover what the API does one must read `build_router()` and then each handler.
- **Hand-written route registration.** Every new endpoint requires edits in three
  places: a handler function, a `pub use` export in `handlers/mod.rs`, and a
  `.route(...)` call in `build_router()`. This is mechanical and error-prone.
- **No protocol versioning.** All routes are under `/api/` with no version
  namespace. Breaking changes are impossible to roll out incrementally.
- **No code generation.** TypeScript types in the frontend are manually synced.
  There is no single source of truth that generates both Rust types and TS types.
- **No request serialization control.** There is no way to declare "this operation
  must be serialized per-session" or "these requests can run concurrently". The
  only coordination is the ad-hoc `SessionOwnership` lock in `WebState`.
- **No experimental API gating.** New endpoints cannot be marked as experimental
  and guarded behind a client opt-in flag.
- **Error patterns are inconsistent.** Some handlers return `(StatusCode, Json(ApiError))`,
  some return `StatusCode`, some return `impl IntoResponse` directly. There is no
  shared error type hierarchy.

## Target Architecture (Inspired by Codex)

```
┌──────────────────────────────────────────────────────┐
│                   Transport Layer                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────┐ │
│  │  REST    │  │WebSocket │  │Unix Sock │  │ Stdio│ │
│  │ (axum)  │  │  (axum)  │  │ (tokio)  │  │      │ │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └──┬───┘ │
│       │              │             │            │      │
│       └──────────────┴─────────────┴────────────┘      │
│                              │ TransportEvent channel   │
├──────────────────────────────┼──────────────────────────┤
│               Protocol Layer │                          │
│  ┌───────────────────────────┴────────────────────┐    │
│  │          MessageProcessor / Dispatcher          │    │
│  │  (match on ClientRequest enum → sub-processor)  │    │
│  └───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┘    │
│      │   │   │   │   │   │   │   │   │   │   │         │
│      ▼   ▼   ▼   ▼   ▼   ▼   ▼   ▼   ▼   ▼   ▼        │
│  ┌────────────────────────────────────────────────┐    │
│  │           Sub-Processors (by domain)            │    │
│  │  Session │ Agent │ File │ Skill │ Config │ ...  │    │
│  └────────────────────────────────────────────────┘    │
├────────────────────────────────────────────────────────┤
│  Protocol Definitions (declared via macros)             │
│  - Single-source request/response type definitions      │
│  - Auto-generated serde, TS types, JSON Schema          │
│  - Serialization scope declarations                     │
│  - Experimental gating annotations                      │
└────────────────────────────────────────────────────────┘
```

The key insight from codex is **separating protocol definitions from transport
mechanics** — a request is the same concept whether it arrives via HTTP POST,
WebSocket JSON-RPC, or Unix socket. The protocol definition is the single source
of truth; transports are pluggable.

## Migration Phases

### Parallel Agent Execution Guide

The migration is deliberately decomposed so multiple agents can work in parallel
when their write scopes are disjoint. Assign each agent an explicit owner area,
make every agent run the narrow tests for that area, and merge through one
integrator agent that owns cross-cutting manifests, generated output, and final
workspace validation.

**General rules for parallel work:**

- Use parallel agents for independent domain modules, DTO/schema expansion,
  handler migration, transport adapters, and documentation/codegen checks.
- Keep macro/runtime infrastructure in a single owner at a time. Do not let
  multiple agents edit `src/macros.rs`, route registration core, processor
  traits, or serialization middleware simultaneously.
- Split handler work by domain file. For example, one agent owns sessions, one
  owns files, one owns skills/plugins, and one owns jobs/kanban/memory.
- Split protocol DTO work by `crates/allthecodes-protocol/src/v1/<domain>.rs`.
  Agents may add endpoint entries only after the macro shape is stable.
- Use one integrator for `Cargo.toml`, `Cargo.lock`, generated TypeScript files,
  and any route registry table that aggregates all domains.
- Each parallel agent must avoid reverting unrelated edits and should report
  changed files plus tests run.

**High-level parallelization map:**

| Phase | Can run in parallel? | Safe parallel slices | Single-owner / sequencing points |
|-------|----------------------|----------------------|----------------------------------|
| 0 | Partially | `v1/<domain>.rs` DTO modules, error tests, docs inventory | Macro design, root workspace manifest, `request.rs` endpoint aggregation |
| 1 | Partially | Handler registry entries by domain, route coverage tests by domain | Router registration core and compatibility shim |
| 2 | Yes, after trait lands | One processor per domain: health/capabilities/session/files/skills/plugins/jobs | `Processor` trait, generic Axum adapter, shared middleware hooks |
| 3 | Partially | Serialization annotations by endpoint domain, concurrency tests by domain | Serialization middleware implementation and `WebState` replacement |
| 4 | Yes, after codegen contract lands | TS type snapshots, JSON schema export, route docs export, CI freshness test | Codegen binary contract and generated file ownership |
| 5 | Yes | WebSocket, Unix socket, REST/OpenAPI adapter can be separate agents | Shared `Transport` trait and `MessageProcessor` dispatch contract |

### Phase 0: Foundation — Protocol Definitions Crate (2–3 sprints)

Create `crates/allthecodes-protocol/` — a new crate that defines the entire API
surface using declarative macros, modeled on codex's `app-server-protocol`.

**Concrete deliverables:**

1. **Crate scaffold**
   ```
   crates/allthecodes-protocol/
     Cargo.toml          # serde, schemars, thiserror deps
     src/lib.rs          # re-exports macros + types
     src/request.rs      # ClientRequest enum (generated by macro)
     src/response.rs     # ClientResponse enum (generated by macro)
     src/notification.rs # ServerNotification enum (generated by macro)
     src/v1/mod.rs       # V1 parameter/response types
     src/macros.rs       # api_definitions! macro + helpers
   ```

2. **Protocol macro** (`api_definitions!`)

   Adapted from codex's `client_request_definitions!` but targeting REST
   semantics rather than JSON-RPC:

   ```rust
   api_definitions! {
       /// Session management
       SessionList => "GET /api/sessions" {
           response: v1::SessionListResponse,
       },
       SessionDetail => "GET /api/sessions/{id}" {
           params: v1::SessionDetailParams,
           response: v1::SessionDetailResponse,
           errors: [NotFound, Conflict],
       },
       SessionCreate => "POST /api/sessions" {
           params: v1::SessionCreateParams,
           response: v1::SessionCreateResponse,
           serialization: PerKey("session"),
       },
       SessionDelete => "DELETE /api/sessions/{id}" {
           params: v1::SessionDeleteParams,
           errors: [NotFound],
           serialization: PerKey("session"),
       },
       /// Agent management
       AgentList => "GET /api/agents" {
           response: v1::AgentListResponse,
       },
       AgentUpdate => "PATCH /api/agents/{name}" {
           params: v1::AgentUpdateParams,
           response: v1::AgentUpdateResponse,
           #[experimental("agent-mutations-2026-06")]
       },
       // ... all 150+ existing endpoints
   }
   ```

   The macro generates:
   - `ClientRequest` enum with `params` and `response` type info
   - `ClientResponse` enum with all response variants
   - `Method` enum with path + HTTP method mapping
   - `serialization_scope()` method for per-key serialization
   - `experimental_reason()` for gated APIs

3. **Error type hierarchy**

   Replace the ad-hoc `ApiError` tuple with a structured error enum:

   ```rust
   pub enum ApiError {
       NotFound { entity: &'static str, id: String },
       Conflict { reason: String },
       Validation { field: String, message: String },
       EngineBusy,
       Experimental(String),  // experimental API not opted-in
       Internal(Box<dyn Error + Send + Sync>),
   }
   ```

   Each variant knows its HTTP status code:
   ```rust
   impl ApiError {
       pub fn status_code(&self) -> StatusCode { ... }
       pub fn into_response(self) -> (StatusCode, Json<ApiErrorBody>) { ... }
   }
   ```

**Test:** `cargo test -p allthecodes-protocol` — all macro-expanded types roundtrip
through serde; error variants produce correct status codes.

**Parallel agent opportunities:**

- Agent A owns macro/runtime foundation:
  `crates/allthecodes-protocol/src/macros.rs`, `request.rs`, `response.rs`,
  `notification.rs`, and the first 5 endpoint sample.
- Agent B owns structured errors:
  `crates/allthecodes-protocol/src/error.rs` and error mapping tests.
- Agent C owns protocol DTO expansion after Agent A stabilizes the macro:
  create domain modules under `crates/allthecodes-protocol/src/v1/`.
- Agent D owns documentation and endpoint inventory:
  audit existing `allthecodes-web` routes and produce a domain-by-domain endpoint
  checklist, without editing macro/runtime files.

Agent A must land first if the macro syntax is still changing. Agents B and D can
start immediately. Agent C should wait until the generated enum shape and
serialization metadata format are stable.

**Risk:** Macro design is iterative. Prototype `api_definitions!` with 5 endpoints
first, then expand to cover the full surface.

---

### Phase 1: Route Generation from Protocol Definitions (2 sprints)

Replace hand-written routes in `build_router()` with generated routes driven by
the protocol definitions.

**Concrete deliverables:**

1. **Router generation macro or function**

   ```rust
   // crates/allthecodes-web/src/mod.rs
   pub fn build_router(state: WebState) -> Router {
       let mut router = Router::new();

       // Auto-register all API endpoints from protocol definitions
       router = router.register_api::<v1::Api>(handlers::v1());

       // Keep manually-registered non-API routes
       router = router
           .route("/healthz", get(health_handler))
           .fallback(static_files::static_handler)
           .layer(TraceLayer::new_for_http())
           .layer(CorsLayer::permissive())
           .with_state(state);

       router
   }
   ```

   The `register_api::<Api>()` function iterates over all defined endpoints
   and calls `.route()` for each one, wired to the appropriate handler from
   a registry map.

2. **Handler Registry**

   Each handler module returns a `HandlerMap` that maps `ClientRequest` variants
   to handler functions:

   ```rust
   // crates/allthecodes-web/src/handlers/sessions.rs
   pub fn handlers() -> HandlerRegistry {
       HandlerRegistry::new()
           .handle(ClientRequest::SessionList, session_list_handler)
           .handle(ClientRequest::SessionDetail, session_detail_handler)
           .handle(ClientRequest::SessionCreate, session_create_handler)
           // ...
   }
   ```

   The registry validates at startup that every `ClientRequest` variant has
   exactly one handler assigned (or explicitly marked `unimplemented`).

3. **Backward compatibility shim**

   During migration, keep existing hand-written routes alongside generated ones
   so endpoints can be migrated incrementally. A `route_exists()` check prevents
   double-registration.

**Test:** Compare the output of `build_router()` before and after migration for
identical endpoint coverage. Route registration tests in `handlers/mod.rs`
continue to pass.

**Parallel agent opportunities:**

- Agent A owns the generated route registration core:
  `crates/allthecodes-web/src/mod.rs` route integration and
  `crates/allthecodes-web/src/handler_registry.rs`.
- Agent B owns session/capabilities registry wiring and route coverage tests.
- Agent C owns files/skills/plugins registry wiring and route coverage tests.
- Agent D owns jobs/kanban/memory/backend-services registry wiring and route
  coverage tests.
- Agent E owns the debug route inventory endpoint `GET /api/-/routes`.

Only Agent A should change the router core and compatibility shim. Domain agents
should limit edits to their handler modules and registry entries assigned by the
integrator, then report any missing protocol definitions instead of changing the
macro core.

**Risk:** The route generation layer may make it harder to see what routes exist
at a glance. Mitigate by adding a debug endpoint `GET /api/-/routes` that lists
all registered routes at runtime.

---

### Phase 2: Processor Pattern (3–4 sprints)

Convert handler functions from free functions into processor structs with shared
infrastructure, modeled on codex's sub-processor pattern.

**Current pattern (free functions):**

```rust
pub async fn session_detail_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let engine = state.engine();
    let session = engine.load_session(&id).await;
    match session {
        Ok(s) => Json(s).into_response(),
        Err(_) => session_not_found_response(&id),
    }
}
```

**Target pattern (processor struct):**

```rust
#[derive(Clone)]
pub struct SessionProcessor {
    state: WebState,
}

impl SessionProcessor {
    pub fn new(state: WebState) -> Self { Self { state } }

    pub async fn detail(&self, id: String, _profile_id: Option<String>)
        -> Result<SessionDetailResponse, ApiError>
    {
        let engine = self.state.engine();
        let session = engine.load_session(&id).await
            .map_err(|_| ApiError::NotFound { entity: "session", id })?;
        Ok(SessionDetailResponse {
            id: session.id,
            title: session.title,
            created_at: session.created_at,
            messages: session.messages,
        })
    }
}
```

**Concrete deliverables:**

1. **Processor trait and derive macro**

   ```rust
   pub trait Processor: Clone + Send + Sync + 'static {
       type Request: Into<ClientRequest>;
       type Response: Serialize;
       type Error: Into<ApiError>;

       fn handler_name() -> &'static str;
       async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error>;
   }
   ```

2. **Conversion layer** — adapt `Processor::handle()` return to Axum response:

   ```rust
   // A generic wrapper that lets any Processor be used as an Axum handler
   pub async fn processor_handler<P, E>(
       State(processor): State<Arc<P>>,
       Json(params): Json<E>,
   ) -> impl IntoResponse
   where
       P: Processor<Request = E>,
       E: DeserializeOwned + Into<ClientRequest>,
       P::Error: Into<ApiError>,
   {
       match processor.handle(params).await {
           Ok(resp) => Json(resp).into_response(),
           Err(err) => err.into().into_response(),
       }
   }
   ```

3. **Shared middleware / pre-processing** — injectable into all processor calls:
   - Request validation (field presence, bounds checking)
   - Profile-aware context injection
   - Request serialization (per-key queuing)
   - Experimental gating check
   - Structured logging / tracing spans

**Migration order** (lowest risk → highest value):

| Priority | Processor | Rationale |
|----------|-----------|-----------|
| 1 | `HealthProcessor` | Simplest; health endpoints have no state |
| 2 | `CapabilitiesProcessor` | Read-only, heavily used by frontend |
| 3 | `SessionProcessor` | Core domain, validates processor pattern under load |
| 4 | `AgentProcessor`, `PeopleProcessor` | CRUD pattern, easy to generalize |
| 5 | `FileProcessor` | Complex validation, tests error handling |
| 6 | `SkillProcessor`, `PluginProcessor`, `HookProcessor` | Domain-specific logic |
| 7 | `ChatProcessor` | Streaming, most complex — done last |

**Parallel agent opportunities:**

- Agent A owns processor infrastructure:
  `Processor` trait, generic Axum adapter, shared error conversion, and tests for
  the adapter with a fake processor.
- Agent B owns `HealthProcessor` and `CapabilitiesProcessor`.
- Agent C owns `SessionProcessor` and session-specific tests.
- Agent D owns `AgentProcessor` and `PeopleProcessor`.
- Agent E owns `FileProcessor`.
- Agent F owns `SkillProcessor`, `PluginProcessor`, and `HookProcessor`.
- Agent G owns `ChatProcessor` only after the non-streaming processor adapter is
  stable.

Agents B-F can work in parallel after Agent A lands the trait and adapter. Avoid
parallel edits to shared handler exports by letting the integrator update
`handlers/mod.rs` and any central registry after reviewing each domain patch.

---

### Phase 3: Serialization Scoping and Concurrency Control (1 sprint)

Add explicit serialization scope declarations to the protocol definitions and
enforce them in a middleware layer.

**Codex pattern used:**

```rust
SessionCreate => "POST /api/sessions" {
    serialization: PerKey("session"),  // serialize per session ID
    // ...
},
SessionList => "GET /api/sessions" {
    serialization: Concurrent,  // can run concurrently
    // ...
},
```

**Concrete deliverables:**

1. **SerializationScope enum**

   ```rust
   pub enum SerializationScope {
       Concurrent,               // no serialization
       PerKey(&'static str),     // serialize per dynamic key
       PerProcess,               // global serialization
       PerConnection,            // serialize per WebSocket/connection
   }
   ```

2. **SerializationMiddleware** — wraps handler execution with a per-key semaphore:

   ```rust
   pub struct SerializationLayer {
       queues: Arc<dashmap::DashMap<String, Arc<tokio::sync::Semaphore>>>,
   }

   impl SerializationLayer {
       pub async fn run_scoped<K: Into<String>, F, Fut, R>(
           &self, scope: &SerializationScope, key: K, f: F
       ) -> R
       where F: FnOnce() -> Fut + Send, Fut: Future<Output = R>, R: Send
       {
           match scope {
               SerializationScope::Concurrent => f().await,
               SerializationScope::PerKey(prefix) => {
                   let sem_key = format!("{}:{}", prefix, key.into());
                   let sem = self.queues.entry(sem_key.clone())
                       .or_insert_with(|| Arc::new(Semaphore::new(1)))
                       .value().clone();
                   let _permit = sem.acquire().await.unwrap();
                   f().await
               }
               // ...
           }
       }
   }
   ```

3. **Replace `SessionOwnership`** — the current ad-hoc claim/release lock in
   `WebState` is replaced by the generic serialization layer. Session-scoped
   requests automatically serialize per `session:{id}`.

**Parallel agent opportunities:**

- Agent A owns `crates/allthecodes-web/src/serialization.rs` and the generic
  serialization middleware tests.
- Agent B owns protocol annotations for session/chat endpoints and tests that
  per-session keys are extracted correctly.
- Agent C owns protocol annotations for file/job/kanban mutation endpoints.
- Agent D owns the `WebState` replacement path and removal of `SessionOwnership`.

Agent A should land the middleware API before Agents B-D integrate against it.
Agents B and C can independently add endpoint metadata once the enum values are
defined. Agent D should be last because it removes the old runtime lock.

---

### Phase 4: Code Generation Pipeline (2–3 sprints)

Add code generation from protocol definitions to eliminate manual type syncing
between Rust and TypeScript.

**Concrete deliverables:**

1. **TypeScript type generation** — a `codegen` binary or build script

   ```rust
   // crates/allthecodes-protocol/codegen/src/main.rs
   fn main() {
       let types = generate_typescript_types::<v1::Api>();
       std::fs::write(
           "../../../allthecodes-web/src/lib/generated/api-types.ts",
           types,
       ).unwrap();
   }
   ```

   Output:
   ```typescript
   // auto-generated from Rust protocol definitions
   export type V1ApiRequest =
     | { method: "GET /api/sessions"; params?: never }
     | { method: "GET /api/sessions/{id}"; params: { id: string } }
     | { method: "POST /api/sessions"; params: SessionCreateParams }
     // ...
   export type V1ApiResponse =
     | { method: "GET /api/sessions"; data: SessionListResponse }
     | { method: "GET /api/sessions/{id}"; data: SessionDetailResponse }
     // ...
   ```

2. **JSON Schema export** — for documentation and runtime validation:

   ```bash
   cargo run -p allthecodes-protocol --features codegen --bin schema-export > docs/api/schema.json
   ```

3. **Route list export** — markdown documentation:

   ```bash
   cargo run -p allthecodes-protocol --features codegen --bin route-doc > docs/api/routes.md
   ```

4. **Build script integration** — auto-regenerate TS types on `cargo build`:

   ```toml
   # crates/allthecodes-protocol/Cargo.toml
   [package]
   links = "allthecodes-protocol"
   build = "build.rs"
   ```

5. **Validation test** — CI step that checks the generated TS file is up to date:

   ```bash
   cargo test --test check_ts_types_up_to_date
   ```

**Parallel agent opportunities:**

- Agent A owns the codegen contract and binary crate layout.
- Agent B owns TypeScript type generation and frontend import migration.
- Agent C owns JSON Schema export.
- Agent D owns route markdown documentation export.
- Agent E owns the CI freshness test and developer workflow docs.

Agents B-D can proceed in parallel after Agent A defines the shared protocol
introspection API. One integrator must own generated files to avoid merge
conflicts, especially `allthecodes-web/src/lib/generated/api-types.ts` and route docs.

---

### Phase 5: Transport Abstraction (optional, 2–3 sprints)

Add non-HTTP transport modes for low-latency and local-IPC scenarios.

Not all transports need to be implemented — start with one (WebSocket for
streaming chat) and add others on demand.

1. **Transport trait**

   ```rust
   #[async_trait]
   pub trait Transport: Send + Sync {
       async fn send_request(&self, request: ClientRequest) -> Result<ClientResponse, TransportError>;
       async fn send_notification(&self, notification: ServerNotification) -> Result<(), TransportError>;
       fn supports_streaming(&self) -> bool;
   }
   ```

2. **WebSocket transport** — replaces the current ad-hoc `/api/tui/ws` and
   `/api/ipc/ws` with a unified JSON-RPC-style WebSocket:

   ```rust
   impl Transport for WebSocketTransport {
       // JSON-RPC framing over WebSocket
       // Same protocol definitions as REST transport
   }
   ```

   The WebSocket handler uses the same `MessageProcessor` + sub-processor
   dispatch as the REST handler, so there is no duplicated business logic.

3. **Unix socket transport** — for CLI subprocess mode:

   ```rust
   impl Transport for UnixSocketTransport {
       // JSON-RPC over Unix domain socket
   }
   ```

4. **REST compatibility layer** — generates OpenAPI 3.0 spec from protocol
   definitions for REST clients:

   ```rust
   // Auto-generate OpenAPI paths from protocol definitions
   let openapi = generate_openapi::<v1::Api>();
   ```

**Parallel agent opportunities:**

- Agent A owns the shared `Transport` trait and `MessageProcessor` dispatch
  contract.
- Agent B owns WebSocket transport migration.
- Agent C owns Unix socket transport.
- Agent D owns REST/OpenAPI compatibility output.
- Agent E owns cross-transport protocol conformance tests.

Agents B-D can work in parallel after Agent A lands the trait and framing
contract. Agent E should run after at least two transports exist so it can verify
that identical `ClientRequest` values produce compatible behavior.

---

## Dependencies and Risk Matrix

| Phase | Dependencies | Risk | Mitigation |
|-------|-------------|------|------------|
| 0 | None | Macro design is hard to get right | Start with 5 endpoints, iterate |
| 1 | Phase 0 | Generated routes hide endpoint visibility | Add `/-/routes` debug endpoint |
| 2 | Phase 0, 1 | Large refactor of existing handlers | Migrate incrementally, keep old code |
| 3 | Phase 2 | Per-key semaphores could cause deadlocks | Add timeout + deadlock detection |
| 4 | Phase 0 | Codegen output must match frontend expectations | Pin generated types in CI test |
| 5 | Phase 0, 2 | Duplicate business logic risk | Use same MessageProcessor for all transports |

## Success Criteria

1. **Single source of truth.** Adding a new endpoint means editing exactly one
   macro entry (and one handler). Route registration, TypeScript types, JSON
   Schema, and documentation are auto-generated.

2. **Consistent error handling.** Every endpoint returns errors in the same
   format through the `ApiError` type hierarchy. No ad-hoc `(StatusCode, Json(...))`
   tuples outside of the conversion layer.

3. **Versioned API.** All new endpoints use `/api/v2/` prefix. Breaking changes
   to V1 are documented and deprecated via the experimental gating mechanism.

4. **Serialization scoping.** Race conditions between session-scoped operations
   (chat, session mutations) are prevented by the serialization layer, not by
   ad-hoc `SessionOwnership` claims.

5. **Generated TypeScript types.** Frontend imports from
   `src/lib/generated/api-types.ts` are auto-generated and verified in CI. PRs that change
   Rust API types without regenerating TS types fail CI.

6. **All existing tests continue to pass** at every phase. No regression in API
   behavior.

## Non-Goals

- **Not a rewrite.** The goal is to add infrastructure that makes future API
  growth cheaper and safer. Existing endpoints are migrated incrementally.
- **Allthecodes-web remains the primary web crate.** The protocol crate adds
  definitions, not a new HTTP server. The axum server in `allthecodes-web` is
  retained.
- **No mandatory WebSocket migration.** REST stays as the primary transport.
  WebSocket is additive for streaming use cases.
- **No premature optimization.** Implement the macro, processor, and codegen
  layers first. Only add WebSocket/Unix socket transports if there is a proven
  latency or UX need.

## Appendix: Key Codex Files Referenced

| Codex File | Purpose | What to Adapt |
|-----------|---------|---------------|
| `app-server-protocol/src/protocol/common.rs` | `client_request_definitions!` macro | `api_definitions!` macro for REST |
| `app-server-protocol/src/protocol/v2/*.rs` | V2 params/response types | `v1/` type modules |
| `app-server/src/message_processor.rs` | Request dispatch to sub-processors | `HandlerRegistry` and dispatch |
| `app-server/src/request_processors/*.rs` | Sub-processor implementations | Processor struct pattern |
| `app-server-transport/src/transport/websocket.rs` | Axum WS transport | Optional Phase 5 |
| `codex-api/src/endpoint/responses.rs` | Client-side HTTP client | Not directly applicable |

## Current File Inventory (for reference)

Files to be modified:

- `crates/allthecodes-web/src/mod.rs` — remove hand-written routes, use generated registry
- `crates/allthecodes-web/src/handlers/mod.rs` — re-export processor handler wrappers
- `crates/allthecodes-web/src/handlers/*.rs` — each converted to processor struct
- `crates/allthecodes-web/src/state.rs` — remove SessionOwnership, add serialization layer

Files to be created:

- `crates/allthecodes-protocol/Cargo.toml`
- `crates/allthecodes-protocol/src/lib.rs`
- `crates/allthecodes-protocol/src/macros.rs`
- `crates/allthecodes-protocol/src/request.rs`
- `crates/allthecodes-protocol/src/response.rs`
- `crates/allthecodes-protocol/src/error.rs`
- `crates/allthecodes-protocol/src/v1/mod.rs`
- `crates/allthecodes-protocol/src/v1/session.rs`
- `crates/allthecodes-protocol/src/v1/agent.rs`
- `crates/allthecodes-protocol/src/v1/file.rs`
- `crates/allthecodes-protocol/src/v1/skill.rs`
- (one v1 file per domain module)
- `crates/allthecodes-protocol/codegen/src/main.rs` — TS type generator
- `crates/allthecodes-web/src/handler_registry.rs` — handler map + validation
- `crates/allthecodes-web/src/serialization.rs` — serialization scope layer
