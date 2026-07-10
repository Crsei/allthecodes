#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Integration tests for `allthecodes-server`.
//!
//! These tests start real Axum servers on `:0` (OS-assigned port) and verify
//! that they respond to HTTP requests and shut down cleanly.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{routing::get, Router};
use tokio::net::TcpListener;

const WEB_BODY: &str = "web-ok";
const DAEMON_BODY: &str = "daemon-ok";

fn web_router() -> Router {
    Router::new().route("/health", get(|| async { WEB_BODY }))
}

fn daemon_router() -> Router {
    Router::new().route("/health", get(|| async { DAEMON_BODY }))
}

fn slow_router() -> Router {
    Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            "slow-ok"
        }),
    )
}

fn client_addr(addr: SocketAddr) -> SocketAddr {
    if addr.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), addr.port())
    } else {
        addr
    }
}

/// Helper: send a GET request and return the response body string.
async fn get_body(addr: SocketAddr, path: &str) -> String {
    let addr = client_addr(addr);
    let url = format!("http://{addr}{path}");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("failed to build reqwest client");
    client
        .get(&url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url} failed: {e}"))
        .text()
        .await
        .unwrap_or_else(|e| panic!("reading body from {url} failed: {e}"))
}

/// Return a `ServerMode::Web` on `:0`.
fn web_mode() -> allthecodes_server::ServerMode {
    allthecodes_server::ServerMode::Web {
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    }
}

/// Return a `ServerMode::All` with both servers on `:0`.
fn all_mode() -> allthecodes_server::ServerMode {
    allthecodes_server::ServerMode::All {
        web_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        daemon_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    }
}

/// Return a `ServerMode::Daemon` on `:0`.
fn daemon_mode() -> allthecodes_server::ServerMode {
    allthecodes_server::ServerMode::Daemon {
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    }
}

async fn unused_loopback_addr() -> SocketAddr {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .expect("failed to bind temporary listener");
    listener.local_addr().expect("temporary listener address")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn non_loopback_web_startup_requires_explicit_control_secret() {
    let mode = allthecodes_server::ServerMode::Web {
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 17322),
    };

    let error = allthecodes_server::ServerManager::new_with_web_control_secret(mode, false)
        .expect_err("startup must fail before binding an unauthenticated public listener");

    assert!(error.to_string().contains("control secret"));
}

#[tokio::test]
async fn test_start_web_server() {
    let mode = web_mode();
    let manager = allthecodes_server::ServerManager::new(mode);

    let (web_handle, daemon_handle) = manager.start(web_router(), Router::new()).await.unwrap();
    let handle = web_handle.expect("expected web handle");

    let body = get_body(handle.addr, "/health").await;
    assert_eq!(body, WEB_BODY);

    handle.cancel.cancel();

    assert!(daemon_handle.is_none());
}

#[tokio::test]
async fn test_start_daemon_server() {
    let mode = daemon_mode();
    let manager = allthecodes_server::ServerManager::new(mode);

    let (web_handle, daemon_handle) = manager.start(Router::new(), daemon_router()).await.unwrap();
    let handle = daemon_handle.expect("expected daemon handle");

    let body = get_body(handle.addr, "/health").await;
    assert_eq!(body, DAEMON_BODY);

    handle.cancel.cancel();

    assert!(web_handle.is_none());
}

#[tokio::test]
async fn test_start_both_servers() {
    let mode = all_mode();
    let manager = allthecodes_server::ServerManager::new(mode);

    let (web_handle, daemon_handle) = manager.start(web_router(), daemon_router()).await.unwrap();
    let web = web_handle.expect("expected web handle");
    let daemon = daemon_handle.expect("expected daemon handle");

    // Ports must be different
    assert_ne!(web.addr.port(), daemon.addr.port());

    let web_body = get_body(web.addr, "/health").await;
    assert_eq!(web_body, WEB_BODY);

    let daemon_body = get_body(daemon.addr, "/health").await;
    assert_eq!(daemon_body, DAEMON_BODY);

    web.cancel.cancel();
    daemon.cancel.cancel();
}

#[tokio::test]
async fn test_all_mode_rejects_duplicate_non_zero_addr() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 17322);
    let manager = allthecodes_server::ServerManager::new(allthecodes_server::ServerMode::All {
        web_addr: addr,
        daemon_addr: addr,
    });

    let result = manager.start(web_router(), daemon_router()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_all_mode_second_bind_failure_leaves_no_web_server() {
    let web_addr = unused_loopback_addr().await;
    let occupied_daemon = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .expect("failed to bind occupied daemon listener");
    let daemon_addr = occupied_daemon
        .local_addr()
        .expect("occupied daemon listener address");

    let manager = allthecodes_server::ServerManager::new(allthecodes_server::ServerMode::All {
        web_addr,
        daemon_addr,
    });

    let result = manager.start(web_router(), daemon_router()).await;
    assert!(result.is_err());

    let rebound = TcpListener::bind(web_addr)
        .await
        .expect("web address should be reusable after failed All startup");
    drop(rebound);
    drop(occupied_daemon);
}

#[tokio::test]
async fn test_start_none_returns_no_handles() {
    let manager = allthecodes_server::ServerManager::new(allthecodes_server::ServerMode::None);
    let (web, daemon) = manager.start(Router::new(), Router::new()).await.unwrap();
    assert!(web.is_none());
    assert!(daemon.is_none());
}

#[tokio::test]
async fn test_graceful_shutdown() {
    let mode = web_mode();
    let manager = allthecodes_server::ServerManager::new(mode);
    let cancel = manager.shutdown_token();

    let (web_handle, _) = manager.start(web_router(), Router::new()).await.unwrap();
    let handle = web_handle.expect("expected web handle");

    // Verify server is running
    let body = get_body(handle.addr, "/health").await;
    assert_eq!(body, WEB_BODY);

    // Signal shutdown
    cancel.cancel();

    // After cancellation, the server should stop accepting connections.
    // We try a few times with a short timeout to let shutdown propagate.
    let url = format!("http://{}/health", client_addr(handle.addr));
    let mut disconnected = false;
    for _ in 0..10 {
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("failed to build reqwest client");
        match tokio::time::timeout(
            std::time::Duration::from_millis(200),
            client.get(&url).send(),
        )
        .await
        {
            Ok(Ok(_)) => {
                // Server still running, wait a bit more
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            _ => {
                disconnected = true;
                break;
            }
        }
    }
    assert!(disconnected, "server did not shut down after cancellation");
}

#[tokio::test]
async fn test_graceful_shutdown_drains_requests() {
    let mode = web_mode();
    let manager = allthecodes_server::ServerManager::new(mode);
    let cancel = manager.shutdown_token();

    let (web_handle, _) = manager.start(slow_router(), Router::new()).await.unwrap();
    let mut handle = web_handle.expect("expected web handle");
    let url = format!("http://{}/slow", client_addr(handle.addr));
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("failed to build reqwest client");

    let request = tokio::spawn(async move {
        client
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {url} failed: {e}"))
            .text()
            .await
            .unwrap_or_else(|e| panic!("reading body from {url} failed: {e}"))
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    cancel.cancel();

    let body = tokio::time::timeout(std::time::Duration::from_secs(2), request)
        .await
        .expect("request did not finish")
        .expect("request task panicked");
    assert_eq!(body, "slow-ok");

    handle.wait().await.unwrap();
}
