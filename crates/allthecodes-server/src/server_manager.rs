//! [`ServerManager`] — lifecycle management for one or two Axum servers.
//!
//! The `ServerManager` owns a root [`CancellationToken`] and provides:
//!
//! - **`run()`** — blocks on the server(s) (for simple single-server modes).
//! - **`start()`** — spawns server tasks and returns handles (for daemon mode
//!   where the caller runs background loops alongside the servers).
//!
//! Graceful shutdown is triggered via `root_cancel.cancel()`, which the
//! `with_graceful_shutdown` hooks observe on each Axum server.

use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::server_mode::ServerMode;

/// A handle to a running Axum server.
#[derive(Debug)]
pub struct ServerHandle {
    /// The address the server is actually listening on (may differ from the
    /// requested address when `:0` is used).
    pub addr: SocketAddr,
    /// Cancel token for this specific server.
    pub cancel: CancellationToken,
    /// Task running the Axum server.
    pub join_handle: JoinHandle<anyhow::Result<()>>,
    exit_observed: bool,
}

impl ServerHandle {
    /// Wait for the server task to exit and surface server or task failures.
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        if self.exit_observed {
            return Ok(());
        }
        let result = match (&mut self.join_handle).await {
            Ok(result) => result,
            Err(err) if err.is_cancelled() => Ok(()),
            Err(err) => Err(anyhow::anyhow!("server task failed: {err}")),
        };
        self.exit_observed = true;
        result
    }

    /// Abort the underlying server task.
    pub fn abort(&self) {
        self.join_handle.abort();
    }

    /// Whether the underlying server task has finished.
    pub fn is_finished(&self) -> bool {
        self.exit_observed || self.join_handle.is_finished()
    }
}

/// Manages the lifecycle of one or two Axum HTTP servers.
///
/// # Cancellation hierarchy
///
/// ```text
/// root_cancel_token (ServerManager)
///   ├── web_child_cancel      → Axum serve graceful_shutdown
///   └── daemon_child_cancel   → Axum serve graceful_shutdown
/// ```
#[derive(Debug)]
pub struct ServerManager {
    mode: ServerMode,
    root_cancel: CancellationToken,
}

impl ServerManager {
    /// Create a new `ServerManager` for the given [`ServerMode`].
    ///
    /// The root cancel token starts in the uncancelled state. Call
    /// [`ServerManager::shutdown`] or drop the manager to signal shutdown.
    pub fn new(mode: ServerMode) -> Self {
        Self {
            mode,
            root_cancel: CancellationToken::new(),
        }
    }

    /// Block on the active server(s).
    ///
    /// In `All` mode both servers run concurrently under `tokio::select!` —
    /// whichever finishes first returns and the other is cancelled.
    ///
    /// # Errors
    ///
    /// Returns an error when [`ServerMode::None`] (guard against misuse).
    pub async fn run(self, web_router: Router, daemon_router: Router) -> anyhow::Result<()> {
        self.mode.validate()?;
        match self.mode {
            ServerMode::None => {
                anyhow::bail!("ServerManager::run called with ServerMode::None");
            }
            ServerMode::Web { addr } => {
                Self::serve_with_graceful_shutdown(web_router, addr, self.root_cancel, "web").await
            }
            ServerMode::Daemon { addr } => {
                Self::serve_with_graceful_shutdown(daemon_router, addr, self.root_cancel, "daemon")
                    .await
            }
            ServerMode::All {
                web_addr,
                daemon_addr,
            } => {
                let web_bound = Self::bind_server(web_addr, "web").await?;
                let daemon_bound = Self::bind_server(daemon_addr, "daemon").await?;
                let cancel = self.root_cancel.clone();
                tokio::select! {
                    result = Self::serve_bound_with_graceful_shutdown(
                        web_router, web_bound, cancel.clone()
                    ) => result,
                    result = Self::serve_bound_with_graceful_shutdown(
                        daemon_router, daemon_bound, cancel
                    ) => result,
                }
            }
        }
    }

    /// Start server(s) in background tasks and return handles.
    ///
    /// The caller is responsible for driving their own event loop and
    /// calling [`Self::shutdown`] when done.  Use [`Self::run`] for the
    /// simpler case where the servers are the only foreground tasks.
    pub async fn start(
        &self,
        web_router: Router,
        daemon_router: Router,
    ) -> anyhow::Result<(Option<ServerHandle>, Option<ServerHandle>)> {
        self.mode.validate()?;
        match self.mode {
            ServerMode::None => Ok((None, None)),
            ServerMode::Web { addr } => {
                let handle =
                    Self::start_server(web_router, addr, self.root_cancel.child_token(), "web")
                        .await?;
                Ok((Some(handle), None))
            }
            ServerMode::Daemon { addr } => {
                let handle = Self::start_server(
                    daemon_router,
                    addr,
                    self.root_cancel.child_token(),
                    "daemon",
                )
                .await?;
                Ok((None, Some(handle)))
            }
            ServerMode::All {
                web_addr,
                daemon_addr,
            } => {
                let web_bound = Self::bind_server(web_addr, "web").await?;
                let daemon_bound = Self::bind_server(daemon_addr, "daemon").await?;
                let web_handle =
                    Self::start_bound_server(web_router, web_bound, self.root_cancel.child_token());
                let daemon_handle = Self::start_bound_server(
                    daemon_router,
                    daemon_bound,
                    self.root_cancel.child_token(),
                );
                Ok((Some(web_handle), Some(daemon_handle)))
            }
        }
    }

    /// Signal graceful shutdown to all servers.
    pub fn shutdown(&self) {
        self.root_cancel.cancel();
    }

    /// Get a clone of the root cancellation token.
    ///
    /// Useful when the caller wants to listen for shutdown in their own
    /// `select!` loop alongside the server handles.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.root_cancel.clone()
    }

    /// Reference to the current [`ServerMode`].
    pub fn mode(&self) -> &ServerMode {
        &self.mode
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Run a single server in the foreground with graceful shutdown.
    async fn serve_with_graceful_shutdown(
        router: Router,
        addr: SocketAddr,
        cancel: CancellationToken,
        label: &'static str,
    ) -> anyhow::Result<()> {
        let bound = Self::bind_server(addr, label).await?;
        Self::serve_bound_with_graceful_shutdown(router, bound, cancel).await
    }

    async fn serve_bound_with_graceful_shutdown(
        router: Router,
        bound: BoundServer,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let label = bound.label;
        let addr = bound.addr;
        let listener = bound.listener;
        tracing::info!("{label} listening on http://{addr}");
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                cancel.cancelled().await;
            })
            .await?;
        Ok(())
    }

    /// Spawn a single server in a background task, returning a handle.
    async fn start_server(
        router: Router,
        addr: SocketAddr,
        child_cancel: CancellationToken,
        label: &'static str,
    ) -> anyhow::Result<ServerHandle> {
        let bound = Self::bind_server(addr, label).await?;
        Ok(Self::start_bound_server(router, bound, child_cancel))
    }

    async fn bind_server(addr: SocketAddr, label: &'static str) -> anyhow::Result<BoundServer> {
        let listener = TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        Ok(BoundServer {
            listener,
            addr: actual_addr,
            label,
        })
    }

    fn start_bound_server(
        router: Router,
        bound: BoundServer,
        child_cancel: CancellationToken,
    ) -> ServerHandle {
        let cancel = child_cancel.clone();
        let actual_addr = bound.addr;
        let label = bound.label;
        let listener = bound.listener;

        let join_handle = tokio::spawn(async move {
            tracing::info!("{label} listening on http://{actual_addr}");
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    cancel.cancelled().await;
                })
                .await
                .map_err(anyhow::Error::from)
        });

        ServerHandle {
            addr: actual_addr,
            cancel: child_cancel,
            join_handle,
            exit_observed: false,
        }
    }
}

struct BoundServer {
    listener: TcpListener,
    addr: SocketAddr,
    label: &'static str,
}
