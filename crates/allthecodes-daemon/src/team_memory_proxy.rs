//! Team Memory proxy: spawns a Bun TS subprocess and forwards HTTP requests.

use std::process::Stdio;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio::process::{Child, Command};
use tracing::{error, info};

use super::state::DaemonState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HEALTH_CHECK_TIMEOUT_MS: u64 = 5000;
const HEALTH_CHECK_INTERVAL_MS: u64 = 100;

// ---------------------------------------------------------------------------
// Subprocess lifecycle
// ---------------------------------------------------------------------------

/// Spawn the Bun team-memory-server subprocess.
///
/// Returns `(child, port, secret)` on success.
pub async fn spawn_team_memory_server(
    base_port: u16,
    cwd: &std::path::Path,
) -> anyhow::Result<(Child, u16, String)> {
    let port = base_port + 1;
    let secret = uuid::Uuid::new_v4().to_string();

    // Resolve GitHub repo from git remote.
    let repo = allthecodes_utils::git::get_remote_url(cwd)
        .ok()
        .and_then(|url| allthecodes_utils::git::parse_github_repo(&url));

    // Compute team memory path: {data_root}/projects/<sanitized>/memory/team/
    let team_mem_path = crate::process_state::team_memory_dir(cwd);

    // Resolve the script path relative to the binary location.
    let exe_dir = std::env::current_exe()?
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    // Try multiple candidate paths for the TS server script.
    let candidates = [
        exe_dir.join("../ui/team-memory-server/index.ts"),
        exe_dir.join("../../ui/team-memory-server/index.ts"),
        std::path::PathBuf::from("ui/team-memory-server/index.ts"),
    ];
    let script_path = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("ui/team-memory-server/index.ts"));

    info!(
        port,
        script = %script_path.display(),
        repo = repo.as_deref().unwrap_or("none"),
        team_mem_path = %team_mem_path.display(),
        "spawning team-memory-server"
    );

    let mut cmd = Command::new("bun");
    cmd.arg("run")
        .arg(&script_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--secret")
        .arg(&secret);

    if let Some(ref repo_str) = repo {
        cmd.arg("--repo").arg(repo_str);
    }
    cmd.arg("--team-mem-path")
        .arg(team_mem_path.to_string_lossy().as_ref());

    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // Wait for health check.
    let health_url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(HEALTH_CHECK_TIMEOUT_MS);

    loop {
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "team-memory-server failed to start within {}ms",
                HEALTH_CHECK_TIMEOUT_MS
            );
        }
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(port, "team-memory-server is ready");
                break;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(HEALTH_CHECK_INTERVAL_MS)).await;
            }
        }
    }

    Ok((child, port, secret))
}

// ---------------------------------------------------------------------------
// Proxy handler
// ---------------------------------------------------------------------------

/// Proxy handler for `/api/claude_code/team_memory`.
///
/// Forwards the request to the Bun TS subprocess, transparently relaying
/// method, query string, headers (If-Match, If-None-Match), and body.
pub async fn proxy_team_memory(
    State(state): State<DaemonState>,
    method: Method,
    query: Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let port = match state.team_memory_port {
        Some(p) => p,
        None => {
            return (StatusCode::BAD_GATEWAY, "team-memory-server not available").into_response();
        }
    };
    let secret = match &state.team_memory_secret {
        Some(s) => s.clone(),
        None => {
            return (StatusCode::BAD_GATEWAY, "team-memory-server not configured").into_response();
        }
    };

    // Build query string.
    let qs: String = query
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!(
        "http://127.0.0.1:{}/api/claude_code/team_memory?{}",
        port, qs
    );

    let client = reqwest::Client::new();
    let mut req = client
        .request(method.clone(), &url)
        .header("X-Team-Memory-Secret", &secret);

    // Forward relevant headers.
    if let Some(v) = headers.get("if-match") {
        req = req.header("If-Match", v.to_str().unwrap_or(""));
    }
    if let Some(v) = headers.get("if-none-match") {
        req = req.header("If-None-Match", v.to_str().unwrap_or(""));
    }

    // Forward body for PUT.
    if method == Method::PUT {
        req = req.header("Content-Type", "application/json").body(body);
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut builder = axum::http::Response::builder().status(status);

            // Forward ETag header from TS response.
            if let Some(etag) = resp.headers().get("etag") {
                builder = builder.header("ETag", etag);
            }
            builder = builder.header("Content-Type", "application/json");

            let body_bytes = resp.bytes().await.unwrap_or_default();
            builder
                .body(axum::body::Body::from(body_bytes))
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "response build error").into_response()
                })
        }
        Err(e) => {
            error!(error = %e, "failed to proxy to team-memory-server");
            (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response()
        }
    }
}
