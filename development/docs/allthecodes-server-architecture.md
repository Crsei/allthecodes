# allthecodes-server Architecture

`allthecodes-server` owns the transport-neutral lifecycle for the HTTP servers
used by allthecodes. Route construction remains in the owning crates:
`allthecodes-web` builds the Web UI/API router and `allthecodes-daemon` builds
the daemon router.

## Listen URL and Server Mode

The hidden `--listen` flag is parsed into `ListenUrl` and then mapped to
`ServerMode`.

- `web://127.0.0.1:17322` starts only the Web UI/API server.
- `daemon://127.0.0.1:19836` starts only the daemon API server.
- `all://web=127.0.0.1:17322,daemon=127.0.0.1:19836` starts both servers in
  one process.
- `stdio://` and `off` map to no HTTP server.

When `--listen` is absent, legacy flags remain compatible: `--web` maps to Web
mode, `--daemon` maps to Daemon mode, and `--web --daemon` maps to All mode.

If `--listen` is present but invalid, startup fails immediately. It does not
fall back to `--web`, `--daemon`, `--web-port`, or `--port`.

All mode requires distinct Web and Daemon addresses when the port is non-zero.
For example, both explicit
`all://web=127.0.0.1:17322,daemon=127.0.0.1:17322` and legacy
`--web --daemon --web-port 17322 --port 17322` fail before binding. `:0` is
allowed for both addresses because the OS assigns distinct actual ports after
binding.

## ServerManager

`ServerManager` binds Axum servers, owns a root `CancellationToken`, and returns
`ServerHandle` values for background server tasks. Each handle exposes the
actual bound address, its child cancellation token, and the task join handle so
callers can observe unexpected server exits.

`start()` is used by the main binary so Web, Daemon, and All modes share the
same shutdown path. `run()` remains available for simple foreground server
ownership.

In All mode, both listeners are bound before either Axum server is spawned or
served. If the second bind fails, the first listener is dropped without leaving
a partially running server behind.

## Shutdown Semantics

The main binary waits for Ctrl-C, SIGTERM on Unix, explicit cancellation, or a
server task exit. Once shutdown starts, the root cancellation token is cancelled,
Axum stops accepting new connections, and active requests are allowed to drain.

The grace period is 30 seconds. If a server does not exit inside that window, the
task is aborted and a warning is logged. If a server exits with an error before
shutdown is requested, the root token is cancelled so sibling servers drain and
the original error is returned after cleanup.

Daemon mode preserves daemon cleanup ordering: server shutdown is coordinated
first, then worker termination and daemon process-state cleanup run.

Gateway routes remain daemon-token protected under this lifecycle. The canonical
header is `x-allthecodes-daemon-token`; legacy `x-cc-rust-daemon-token` remains
accepted for compatibility.

## Verification

Run these checks from the repository root with the project Rust toolchain in
`PATH`:

```bash
cargo test -p allthecodes-server
cargo test -p allthecodes-gateway
cargo check -p allthecodes --bin allthecodes
cargo build --workspace
cargo clippy --workspace --lib --bins
```

The server integration tests bind to OS-assigned ports and connect via loopback
addresses. They cover Web-only, Daemon-only, All mode, no-server mode, shutdown
stop behavior, in-flight request draining, duplicate All address validation,
and the partial-start regression where the second All bind fails.
