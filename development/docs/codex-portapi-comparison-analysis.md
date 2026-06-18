# Codex Architecture Deep-Dive: Lessons for allthecodes

> Analysis date: 2026-06-14
> Codex codebase: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/`

---

## Table of Contents

1. [App Server Transport Architecture](#1-app-server-transport-architecture)
2. [Exec Server Architecture](#2-exec-server-architecture)
3. [Health Check / Readiness Patterns](#3-health-check--readiness-patterns)
4. [Codex API / Provider Architecture](#4-codex-api--provider-architecture)
5. [Config / Settings Resolution](#5-config--settings-resolution)
6. [Daemon / Supervisor Architecture](#6-daemon--supervisor-architecture)
7. [Cross-Cutting Observations](#7-cross-cutting-observations)

---

## 1. App Server Transport Architecture

### Key File Paths

- **`app-server-transport/src/transport/mod.rs`** -- Core enum `AppServerTransport`, `TransportEvent`, `ConnectionOrigin`, queueing logic, overload protection
- **`app-server-transport/src/transport/websocket.rs`** -- Axum-based WebSocket server with health endpoints, auth, graceful shutdown
- **`app-server-transport/src/transport/stdio.rs`** -- stdio line-based JSON-RPC transport
- **`app-server-transport/src/transport/unix_socket.rs`** -- Unix domain socket transport using WebSocket-over-UDS
- **`app-server-transport/src/transport/auth.rs`** -- WebSocket auth: capability-token and signed-JWT bearer modes
- **`app-server/src/lib.rs`** -- `run_main_with_transport_options()` wiring all transports together
- **`app-server/src/main.rs`** -- CLI entry point parsing `--listen` argument

### Multi-Transport Abstraction

Codex uses a single enum to represent all transport types:

```rust
pub enum AppServerTransport {
    Stdio,
    UnixSocket { socket_path: AbsolutePathBuf },
    WebSocket { bind_address: SocketAddr },
    Off,
}
```

The transport is selected by a single `--listen` CLI argument that accepts URL-like strings:
- `stdio://` (default) -- stdio JSON-RPC
- `unix://` or `unix://PATH` -- Unix domain socket
- `ws://IP:PORT` -- WebSocket
- `off` -- no transport (remote-control only)

All transports produce the same `TransportEvent` enum consumed by a single `MessageProcessor`. This means the entire application logic is transport-agnostic.

### TransportEvent -- The Universal Inbound Channel

```rust
pub enum TransportEvent {
    ConnectionOpened { connection_id, origin, writer, disconnect_sender },
    ConnectionClosed { connection_id },
    IncomingMessage { connection_id, message: JSONRPCMessage },
}
```

Every transport variant pushes `TransportEvent` messages into a single `mpsc::Sender<TransportEvent>` channel. The processor loop in `app-server/src/lib.rs` reads from this single channel, dispatching JSON-RPC messages regardless of origin.

### Outbound: Two-Loop Architecture

The server runs two independent tokio tasks:

1. **Processor loop** -- reads `TransportEvent` from the inbound channel, handles JSON-RPC dispatch, pushes outgoing messages as `OutgoingEnvelope` into a channel
2. **Outbound router loop** -- reads `OutgoingEnvelope` from the outbound channel, writes to per-connection writers

`OutboundControlEvent` synchronizes the two loops without shared mutable state:
```rust
enum OutboundControlEvent {
    Opened { connection_id, writer, disconnect_sender, initialized, ... },
    Closed { connection_id },
    DisconnectAll,
}
```

### WebSocket Server (Axum) Details

The websocket transport uses a standard Axum router with:
- `/readyz` and `/healthz` -- always return 200 OK
- Fallback route -- all other paths trigger WebSocket upgrade
- `Origin` header rejection middleware -- prevents browser-based CSRF
- Graceful shutdown via `CancellationToken` -- `axum::serve(...).with_graceful_shutdown(...)`
- Per-connection outbound channel capacity of 32,768 messages (vs 128 for internal channels)

```rust
let router = Router::new()
    .route("/readyz", get(health_check_handler))
    .route("/healthz", get(health_check_handler))
    .fallback(any(websocket_upgrade_handler))
    .layer(middleware::from_fn(reject_requests_with_origin_header));
```

### Unix Socket Transport

The Unix socket transport accepts TCP-like connections, then upgrades each to WebSocket using `tokio_tungstenite::accept_async`. This means the same WebSocket framing code handles both network and local connections. Key patterns:

- `prepare_control_socket_path()` -- checks for stale sockets, sets 0600 permissions
- `ControlSocketFileGuard` -- `Drop` impl removes the socket file on shutdown
- `acquire_app_server_startup_lock()` -- file-based lock using `flock` to prevent duplicate servers

### Graceful Shutdown

Implemented via `ShutdownState` in `app-server/src/lib.rs`:

1. First signal (SIGTERM, SIGINT, SIGHUP) sets `requested = true`
2. Server stops accepting new connections and waits for running assistant turns to finish
3. Second signal (or all turns finished) forces shutdown via `transport_shutdown_token.cancel()`
4. `OutboundControlEvent::DisconnectAll` closes all WebSocket connections
5. HUP signal triggers graceful-only restart (waits for turns before restarting)

### What allthecodes Can Learn

1. **Single transport abstraction enum** -- Replace allthecodes's separate gateway and IPC path logic with a unified `Transport` enum. All transports produce `TransportEvent` so the message processor is transport-agnostic.

2. **Two-loop outbound architecture** -- Separate inbound processing from outbound writing. This prevents slow WebSocket clients from blocking the message processor. allthecodes's current single-loop architecture in the gateway could block request processing on slow outgoing writes.

3. **WebSocket-over-UDS for local connections** -- Codex upgrades Unix socket connections to WebSocket protocol, reusing the same framing code. allthecodes could use this pattern for local IPC instead of raw TCP or a separate protocol.

4. **Stale socket cleanup** -- The `prepare_control_socket_path()` function checks for stale sockets before binding. allthecodes should add this to prevent "address already in use" errors after crashes.

5. **Origin-header rejection middleware** -- Simple CSRF protection for WebSocket endpoints is a one-line middleware. allthecodes's gateway should add this.

### Gaps Codex Has vs. allthecodes

- Codex's transport layer has no HTTP REST API (it is JSON-RPC only). allthecodes's HTTP API layer is more mature for RESTful usage.
- No built-in rate limiting per-connection in the transport layer itself (rate limits are handled at the API client layer in `codex-api`).
- No multiplexing within a single connection -- each connection is one JSON-RPC session.

---

## 2. Exec Server Architecture

### Key File Paths

- **`exec-server/src/server/transport.rs`** -- Transport setup (WebSocket or stdio), `ExecServerTransport` enum, Axum router with `/readyz`
- **`exec-server/src/server/handler.rs`** -- `ExecServerHandler` with initialization lifecycle, process management, file system operations
- **`exec-server/src/server/session_registry.rs`** -- Session attach/detach lifecycle with TTL-based expiration
- **`exec-server/src/server/process_handler.rs`** -- Thin wrapper over `LocalProcess`
- **`exec-server/src/local_process.rs`** -- Full process lifecycle: spawn, stream output, terminate, cleanup
- **`exec-server/src/process.rs`** -- `ExecProcess` trait, `ExecProcessEventLog` (history + broadcast)
- **`exec-server/src/server.rs`** -- Entry point `run_main()` dispatching to transport

### Architecture Overview

The exec-server is a standalone process that provides sandboxed execution over WebSocket (or stdio). Its default listen URL is `ws://127.0.0.1:0` (random port), printing the assigned port to stdout for the parent process to discover.

```
app-server <--WS--> exec-server (sandboxed child process)
```

### Transport Selection

```rust
enum ExecServerListenTransport {
    WebSocket(SocketAddr),
    Stdio,
}
```

Default: `ws://127.0.0.1:0` -- binds to a random port. The server prints `ws://...` to stdout so the parent can discover the port.

### Port Binding with `:0` (Random Port)

The key insight: `TcpListener::bind("127.0.0.1:0")` lets the OS assign a random port. After binding, `listener.local_addr()` returns the actual port. The server prints it to stdout:

```rust
let listener = TcpListener::bind(bind_address).await?;
let local_addr = listener.local_addr()?;
println!("ws://{local_addr}");  // Parent reads this from stdout
```

### Axum Router for Exec Server

```rust
let router = Router::new()
    .route("/", any(websocket_upgrade_handler))
    .route("/readyz", get(readiness_handler))
    .layer(middleware::from_fn(reject_requests_with_origin_header));
```

### Session Lifecycle

`SessionRegistry` manages session attachment and detachment:

1. **Attach** -- Client sends `initialize` with optional `resume_session_id`. New session created or existing session resumed
2. **Detach** -- Connection drops. Session enters a detached state with a 10-second TTL
3. **Expire** -- After TTL, the session and its processes are cleaned up
4. **Reattach** -- Within TTL, the same session ID can be resumed by a new connection

```rust
const DETACHED_SESSION_TTL: Duration = Duration::from_secs(10);
```

The `SessionHandle` tracks `connection_id` (UUID v4), and `SessionRegistry` uses `Mutex<HashMap<String, Arc<SessionEntry>>>` for concurrent access.

### Process Management

`LocalProcess` manages child processes using `codex_utils_pty::spawn_pty_process()` or `spawn_pipe_process()`:

- **Process map** -- `Mutex<HashMap<ProcessId, ProcessEntry>>` with `Starting`/`Running` states
- **Output streaming** -- Each process has two output streams (stdout/stderr or pty) + an exit watcher, each running in a separate tokio task
- **Retained output** -- Bounded buffer of 1 MB per process (`RETAINED_OUTPUT_BYTES_PER_PROCESS`)
- **Event log** -- `ExecProcessEventLog` with a replay buffer + live broadcast channel for push-based consumption
- **Exit retention** -- Exited processes retained for 30 seconds (`EXITED_PROCESS_RETENTION`) so late output can still be read

### ExecProcess Trait

```rust
#[async_trait]
pub trait ExecProcess: Send + Sync {
    fn process_id(&self) -> &ProcessId;
    fn subscribe_wake(&self) -> watch::Receiver<u64>;
    fn subscribe_events(&self) -> ExecProcessEventReceiver;
    async fn read(&self, ...) -> Result<ReadResponse, ExecServerError>;
    async fn write(&self, chunk: Vec<u8>) -> Result<WriteResponse, ExecServerError>;
    async fn terminate(&self) -> Result<(), ExecServerError>;
}
```

### ExecBackend Trait (For Sandbox Pluggability)

```rust
#[async_trait]
pub trait ExecBackend: Send + Sync {
    async fn start(&self, params: ExecParams) -> Result<StartedExecProcess, ExecServerError>;
}
```

This allows different sandbox backends (local, remote, containerized) to be swapped in.

### What allthecodes Can Learn

1. **`:0` random port binding** -- allthecodes should use `:0` for ephemeral ports in sandbox agents, printing the actual port to stdout for discovery. This eliminates port conflicts.

2. **Session resume with TTL** -- allthecodes does not have session resumption. Adding a `SessionRegistry` with 10-30 second TTL would allow clients to reconnect without losing in-progress operations.

3. **Bounded output retention** -- 1 MB per process with sequential reads enables "page through output" without holding infinite buffers. allthecodes should cap agent output per process.

4. **Replay + broadcast event log** -- `ExecProcessEventLog` combines a bounded history buffer with a live broadcast channel. New subscribers get replay first, then live events. allthecodes is missing this for agent output -- consumers cannot join mid-stream and catch up.

5. **Output sequence numbers** -- Every output chunk has a sequential ID, enabling idempotent reads with `after_seq`. allthecodes should add sequence numbers to process/agent output for reliable incremental reads.

6. **Process lifecycle states** -- `Starting`/`Running` states prevent race conditions during process spawn, and `EXITED_PROCESS_RETENTION` handles the "late output after exit" edge case.

### Gaps Codex Has vs. allthecodes

- Codex's exec-server is a separate binary that must be discovered and managed. allthecodes's in-process sandboxing may be simpler for single-machine deployments.
- No container-level isolation in the exec-server itself (relies on bwrap/Linux sandbox external tools).
- The exec-server currently only supports WebSocket and stdio -- no Unix socket transport unlike the app-server.

---

## 3. Health Check / Readiness Patterns

### Key File Paths

- **`app-server-transport/src/transport/websocket.rs`** -- `/readyz` and `/healthz` in WebSocket transport
- **`exec-server/src/server/transport.rs`** -- `/readyz` in exec-server WebSocket transport
- **`exec-server/tests/health.rs`** -- Integration test for `/readyz`
- **`app-server/src/request_processors/apps_processor.rs`** -- Codex Apps readiness status

### Implementation

All health endpoints in Codex are trivially simple:

```rust
async fn health_check_handler() -> StatusCode {
    StatusCode::OK
}
```

Both `/readyz` and `/healthz` return 200 OK immediately without any dependency checking. They exist primarily so load balancers and orchestrators have a valid endpoint.

The exec-server health test confirms:
```rust
let response = reqwest::get(format!("http://{http_base_url}/readyz")).await?;
assert_eq!(response.status(), reqwest::StatusCode::OK);
```

There is no separate `/livez` (liveness) endpoint -- both readiness and liveness use the same handler.

### Readiness in Codex Apps

In `apps_processor.rs`, readiness is determined by whether Codex Apps connectors are fully loaded, but this is an application-level concern not exposed via HTTP health endpoints.

### What allthecodes Can Learn

1. **Minimal viable health check is fine** -- Codex's `/readyz` returns OK immediately without dependency checking. For local daemon processes, this is acceptable. allthecodes should not over-engineer its health checks.

2. **Startup probe through client-side polling** -- Instead of a complex startup sequence with dependency checking, Codex's daemon polls the Unix socket with `wait_until_ready()` (50ms interval, 10s timeout). This pattern is simpler and more robust than a startup health endpoint.

3. **Separate readiness from startup** -- Codex does not differentiate. allthecodes could benefit from a three-phase approach: `/startupz` (accepting connections but not ready), `/livez` (process alive), `/readyz` (dependencies available).

### Gaps Codex Has vs. allthecodes

- No dependency checking in health endpoints (load balancers cannot distinguish "startup" from "healthy")
- No `/livez` separate from `/readyz` -- the same handler serves both
- No integration of thread or message-processor health into the HTTP health endpoint

---

## 4. Codex API / Provider Architecture

### Key File Paths

- **`codex-api/src/lib.rs`** -- Public API surface exports
- **`codex-api/src/provider.rs`** -- `Provider` struct with base URL, headers, auth, retry config
- **`codex-api/src/endpoint/responses.rs`** -- HTTP SSE-based streaming client (`ResponsesClient`)
- **`codex-api/src/endpoint/responses_websocket.rs`** -- WebSocket-based streaming client (`ResponsesWebsocketClient`)
- **`codex-api/src/sse/responses.rs`** -- SSE event parsing, `process_responses_event()`
- **`codex-api/src/endpoint/mod.rs`** -- All endpoint clients re-exported

### Provider Model

The `Provider` struct encapsulates all API endpoint configuration:

```rust
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub query_params: Option<HashMap<String, String>>,
    pub headers: HeaderMap,
    pub retry: RetryConfig,
    pub stream_idle_timeout: Duration,
}
```

Key methods:
- `url_for_path(path)` -- Constructs full URL with query parameters
- `build_request(method, path)` -- Creates a `Request` with headers, URL, compression
- `websocket_url_for_path(path)` -- Converts HTTP URL to WS/WSS URL for WebSocket connections

### Retry Configuration

```rust
pub struct RetryConfig {
    pub max_attempts: u64,
    pub base_delay: Duration,
    pub retry_429: bool,
    pub retry_5xx: bool,
    pub retry_transport: bool,
}
```

Converts to a `RetryPolicy` used by the `codex-client` transport layer for both unary and streaming calls.

### Two Streaming Paths: SSE and WebSocket

Codex provides two streaming response mechanisms:

**1. HTTP SSE (`ResponsesClient`)**:
```rust
pub async fn stream_request(&self, request, options) -> Result<ResponseStream, ApiError>
```
- Uses standard HTTP POST with `Accept: text/event-stream`
- Returns a `ResponseStream` with channel-based event delivery
- Supports compression (Zstd)
- Reads `ResponseStreamEvent` from SSE, maps to `ResponseEvent` enum

**2. WebSocket (`ResponsesWebsocketClient`)**:
```rust
pub async fn connect(&self, ...) -> Result<ResponsesWebsocketConnection, ApiError>
pub async fn stream_request(&self, request, connection_reused) -> Result<ResponseStream, ApiError>
```
- Persistent connection, reusable across multiple requests
- Returns connection metadata (reasoning support, model catalog ETag, server model)
- Supports permanent-deflate compression

Both paths produce the same `ResponseEvent` type, enabling transparent switching.

### ResponseStream -- Unified Event Stream

```rust
pub struct ResponseStream {
    pub rx_event: mpsc::Receiver<Result<ResponseEvent, ApiError>>,
    pub upstream_request_id: Option<String>,
}
```

### SSE Event Processing

`process_responses_event()` handles the `ResponsesStreamEvent` type, matching on:
- `response.created` / `response.completed` / `response.failed` / `response.incomplete`
- `response.output_item.done` / `response.output_item.added`
- `response.output_text.delta`
- `response.custom_tool_call_input.delta`
- `response.reasoning_summary_text.delta` / `response.reasoning_text.delta`
- `response.metadata` (model verifications)
- `codex.rate_limits` (custom rate-limit events)

Error classification distinguishes: context_window_exceeded, insufficient_quota, rate_limit_exceeded, cyber_policy, invalid_prompt, server_overloaded, generic retryable, and streaming errors.

### What allthecodes Can Learn

1. **Unified `Provider` struct** -- Codex encapsulates all API connection parameters (base URL, headers, query params, retry, timeout) in a single `Provider` struct. allthecodes should adopt this pattern instead of spreading connection parameters across config and code.

2. **Dual SSE + WebSocket streaming** -- Both paths produce the same `ResponseEvent` type. allthecodes should abstract streaming so the consumer is agnostic to the transport.

3. **Rich error classification** -- Codex's SSE parser classifies errors into distinct types (`ContextWindowExceeded`, `QuotaExceeded`, `CyberPolicy`, etc.) with structured recovery hints. allthecodes should move beyond "got 4xx/5xx" to semantic error handling.

4. **Turn state header passthrough** -- Codex forwards a `x-codex-turn-state` header from the API response to the caller via `OnceLock`. This is useful for request correlation across retries.

5. **WebSocket probe** -- `ResponsesWebsocketProbe` allows testing a WebSocket connection without sending a request, useful for diagnostic UIs. allthecodes could use this for its connection status indicators.

### Gaps Codex Has vs. allthecodes

- Single-provider focus (OpenAI/ChatGPT API). allthecodes's multi-provider support (Claude API, Gemini, local models) is more flexible.
- No model fallback or routing between providers.
- Retry is at the transport level only, not aware of model-level retry decisions.

---

## 5. Config / Settings Resolution

### Key File Paths

- **`config/src/config_toml.rs`** -- `ConfigToml` struct: the exhaustive schema for `config.toml`
- **`config/src/profile_toml.rs`** -- `ConfigProfile`: named profile overrides
- **`config/src/overrides.rs`** -- CLI `--config` override parser with dotted-path support
- **`config/src/loader/mod.rs`** -- `load_config_layers_state()`: layer ordering, loading, merging
- **`config/src/state.rs`** -- `ConfigLayerEntry`, `ConfigLayerStack`, `ConfigLoadOptions`
- **`config/src/merge.rs`** -- `merge_toml_values()`: recursive TOML table merge
- **`config/src/lib.rs`** -- Re-exports
- **`config/src/config_requirements.rs`** -- Admin-enforced constraints (`requirements.toml`)

### Layer Architecture

Codex uses a multi-layer config system. Layers are loaded in ascending precedence:

1. **System** -- `/etc/codex/config.toml` (Unix) or `%ProgramData%\OpenAI\Codex\config.toml` (Windows)
2. **Cloud** -- Enterprise-managed cloud config bundle fragments
3. **User (base)** -- `$CODEX_HOME/config.toml`
4. **User (profile)** -- `$CODEX_HOME/<name>.config.toml` (profile-v2)
5. **Project** -- `$PWD/.codex/config.toml` (and parent directories up to project root)
6. **Runtime/CLI** -- `--config` command-line overrides
7. **Thread** -- Remote thread-scoped config from `experimental_thread_config_endpoint`

### Requirements Layer (Admin Constraints)

Separate from config layers, a `requirements.toml` system enforces admin constraints:
- System `/etc/codex/requirements.toml`
- Cloud enterprise managed requirements
- Legacy `managed_config.toml` reinterpreted as requirements
- macOS MDM managed preferences

Requirements constrain allowed values (e.g., `allowed_sandbox_modes = ["read-only", "workspace-write"]`) rather than setting values directly.

### ConfigLayerEntry

```rust
pub struct ConfigLayerEntry {
    pub name: ConfigLayerSource,
    pub config: TomlValue,
    pub version: String,
    pub disabled_reason: Option<String>,
    // ...
}
```

Layers can be **disabled** due to project trust decisions without removing them from the stack, enabling accurate reporting of why certain config was ignored.

### Merging Strategy

`merge_toml_values()` recursively merges overlay into base at the table level:
- Tables merge recursively
- Scalar values from overlay replace base
- Key aliases are normalized during merge
- Network domain keys are case-normalized

### CLI Overrides

`build_cli_overrides_layer()` parses `--config key=value` pairs into a TOML structure using dotted-path notation (e.g., `model=claude-3`, `features.apps=true`).

### Project-Local Config Deny List

Certain sensitive settings are stripped from project-local config:
```rust
const PROJECT_LOCAL_CONFIG_DENYLIST: &[&str] = &[
    "openai_base_url",
    "chatgpt_base_url",
    "model_provider",
    "model_providers",
    "notify",
    "profile",
    "profiles",
    "otel",
    // ...
];
```

### What allthecodes Can Learn

1. **Layered config with explicit precedence** -- Codex's layer system is its strongest architectural pattern. allthecodes currently has a flat config. Adding layers (system -> user -> project -> CLI -> runtime) would enable: enterprise deployment without editing user files, per-project settings, and ephemeral runtime overrides.

2. **ConfigLayerStack with disabled layers** -- Disabled layers remain in the stack with a `disabled_reason`. This enables accurate diagnostics ("your project config was ignored because the directory is not trusted"). allthecodes should keep rejected config visible for reporting.

3. **Separate "requirements" from "config"** -- Admin-enforced constraints should be a separate concept from user config. allthecodes could add a `requirements.toml` for immutable constraints separate from `config.toml` for preferences.

4. **Path resolution at load time** -- `resolve_relative_paths_in_config_toml()` resolves `AbsolutePathBuf` fields against the config file's directory at load time, enabling correct merging of layers from different directories. allthecodes should resolve paths eagerly to prevent "relative path from wrong base" bugs.

5. **TOML merge semantics** -- The recursive table merge in `merge_toml_values()` is the right approach for config merging. allthecodes's current replace-semantics would lose partial overrides.

6. **Profile support** -- Named profiles that override only a subset of fields. allthecodes could support `--profile development` / `--profile production` patterns.

### Gaps Codex Has vs. allthecodes

- Config schema is tightly coupled to Codex-specific fields (model providers, approval policies, sandboxing). allthecodes's generic config model may be more flexible.
- Schema validation is done through `#[schemars(deny_unknown_fields)]` which is compile-time only.
- No hot-reload of config (config is loaded at startup and per-request by `ConfigManager`).
- Thread-scoped config is experimental and limited.

---

## 6. Daemon / Supervisor Architecture

### Key File Paths

- **`app-server-daemon/src/lib.rs`** -- `Daemon` struct, lifecycle management (start/stop/restart/version), remote control
- **`app-server-daemon/src/backend/mod.rs`** -- `BackendKind` enum, `BackendPaths`, `PidBackend` factory
- **`app-server-daemon/src/backend/pid.rs`** -- `PidBackend`: PID file management, process start/stop/terminate, stderr log capture
- **`app-server-daemon/src/client.rs`** -- Unix socket client to probe running app-server
- **`app-server-daemon/src/update_loop.rs`** -- Update watcher loop
- **`app-server-daemon/src/managed_install.rs`** -- Managed Codex binary install path
- **`app-server-daemon/src/settings.rs`** -- `DaemonSettings` (remote_control_enabled)
- **`app-server-daemon/src/remote_control_client.rs`** -- Enables remote control on a running app-server

### Process Lifecycle Management

The `Daemon` struct manages the app-server as a child process:

**Operations:**
- `start()` -- Probes existing socket, checks PID file, manages binary, spawns child, waits for ready
- `stop()` -- Sends SIGTERM, waits up to 60s grace period, sends SIGKILL if still running
- `restart()` -- Stop + start sequence with "if version changed" optimization
- `version()` -- Probe running server for its version

**PID Backend:**

```rust
struct PidBackend {
    codex_bin: PathBuf,
    pid_file: PathBuf,
    lock_file: PathBuf,
    command_kind: PidCommandKind,
}
```

The PID backend uses:
- **PID file** with JSON-serialized record (`pid` + `process_start_time`)
- **Reservation lock** (`pid.lock` file with `flock`) to coordinate startup between processes
- **Stderr log** (`stderr.log` file) for capturing child process stderr
- **Start time verification** (`ps -p <pid> -o lstart=`) to detect PID reuse

### Process Start (Unix)

```rust
command.args(["app-server", "--listen", "unix://"]);
command.stdin(Stdio::null());
command.stdout(Stdio::null());
command.stderr(Stdio::from(stderr_log));
unsafe { command.pre_exec(|| { libc::setsid(); Ok(()) }); }
```

The child is spawned in a new session (`setsid()`) to detach from the parent's process group.

### Grace Period and Force Kill

```
SIGTERM -> wait 60s (STOP_GRACE_PERIOD) -> SIGKILL -> wait 10s -> timeout error
```

### Startup Lock

Two lock mechanisms:
1. **Operation lock** (`daemon.lock`) -- Serializes daemon operations (start/stop/restart). 75s timeout.
2. **Reservation lock** (`pid.lock`) -- Prevents concurrent PID file writes.

### Startup Lock (App-Server Side)

In `app-server-transport/src/transport/unix_socket.rs`, the app-server itself acquires a **startup lock** using `flock` on a lock file. This prevents two app-server instances from running simultaneously:

```rust
pub async fn acquire_app_server_startup_lock(startup_lock_path) -> IoResult<AppServerStartupLock>
```

The lock is released when `AppServerStartupLock` is dropped.

### Update Loop

The daemon includes a `pid_update_loop` that watches for new Codex binary versions and triggers restarts. This is a separate managed process (`codex app-server daemon pid-update-loop`) that checks version and reexecs if the binary changed.

### What allthecodes Can Learn

1. **PID file with process start time** -- Simple but reliable daemon management without a supervisor. allthecodes currently has no process lifecycle management for long-running agents.

2. **Stderr capture to file** -- Redirect child stderr to a file, with tail reading for diagnostics. allthecodes should capture agent stderr to files for debugging.

3. **Graceful shutdown with grace period** -- SIGTERM, wait 60s, then SIGKILL. allthecodes's process management should implement this pattern instead of immediate SIGKILL.

4. **Version-aware restart** -- `RestartMode::IfVersionChanged` avoids unnecessary restarts. allthecodes's agent updates should be version-aware.

5. **Operation lock** -- File-based lock to serialize lifecycle operations. allthecodes should use file locks to prevent concurrent daemon operations.

6. **Socket readiness polling** -- `wait_until_ready()` polls with 50ms intervals and 10s timeout. allthecodes should use this pattern instead of fixed sleep timers.

### Gaps Codex Has vs. allthecodes

- Daemon management is Unix-only (no Windows support for PID-based management)
- No container orchestration integration (Kubernetes, Docker)
- Single-process management only -- no process pool or worker scaling
- The daemon manages only the app-server, not child sandbox processes

---

## 7. Cross-Cutting Observations

### Patterns that Could Significantly Improve allthecodes

1. **Transport abstraction** -- Copy the `AppServerTransport` enum + `TransportEvent` pattern to make allthecodes backend transport-agnostic (stdio for local dev, WebSocket for remote, Unix socket for daemon).

2. **Layered config** -- The `ConfigLayerStack` pattern is the single most impactful improvement allthecodes could adopt. System-level defaults + user config + project config + CLI overrides + runtime overrides, with disabled-layer diagnostics.

3. **Output event log with replay** -- Codex's `ExecProcessEventLog` (bounded history + live broadcast) solves the "new subscriber cannot catch up" problem. allthecodes should use this for agent output streams.

4. **`:0` port binding** -- Let the OS assign ports for ephemeral agents. Print the port to stdout for discovery.

5. **Session resume with TTL** -- Add session resumption support so clients can reconnect without data loss.

6. **PID-based daemon management** -- Simple, reliable process lifecycle without external dependencies.

### Patterns allthecodes Already Handles Better

1. **Multi-provider model routing** -- Codex is fundamentally single-provider (OpenAI/ChatGPT API). allthecodes has genuine multi-provider support that could route between Claude, GPT, Gemini, and local models.

2. **RESTful HTTP API** -- Codex is JSON-RPC only. allthecodes's HTTP REST API is more standard for web integration.

3. **Pluggable transport** -- Codex's exec-server is a separate binary. allthecodes's Gateway pattern with in-process routing could be more deployable.

4. **WebSocket-native vs. SSE** -- allthecodes WebSocket layer may handle persistent connections more elegantly than Codex's SSE-first approach.

5. **Cross-platform support** -- Codex's daemon is Unix-only. allthecodes should maintain cross-platform support for lifecycle management.
