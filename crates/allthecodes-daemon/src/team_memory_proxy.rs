//! Team Memory proxy: spawns a Bun TS subprocess and forwards HTTP requests.

use std::process::Stdio;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use tokio::process::{Child, Command};
use tracing::{error, info};

use super::state::DaemonState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROXY_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

const REQUEST_TIMEOUT_MESSAGE: &str = "team-memory-server request timed out";
const REQUEST_FAILED_MESSAGE: &str = "team-memory-server request failed";
const RESPONSE_TIMEOUT_MESSAGE: &str = "team-memory-server response timed out";
const RESPONSE_FAILED_MESSAGE: &str = "team-memory-server response failed";
const RESPONSE_TOO_LARGE_MESSAGE: &str = "team-memory-server response exceeded size limit";

#[derive(Clone, Copy)]
struct HealthCheckTiming {
    total_timeout: Duration,
    attempt_timeout: Duration,
    retry_interval: Duration,
}

const HEALTH_CHECK_TIMING: HealthCheckTiming = HealthCheckTiming {
    total_timeout: Duration::from_secs(5),
    attempt_timeout: Duration::from_millis(500),
    retry_interval: Duration::from_millis(100),
};

#[derive(Clone, Copy)]
struct ProxyTimeouts {
    request: Duration,
    body: Duration,
}

const PROXY_TIMEOUTS: ProxyTimeouts = ProxyTimeouts {
    request: Duration::from_secs(5),
    body: Duration::from_secs(5),
};

async fn wait_for_team_memory_health(
    client: &reqwest::Client,
    health_url: &str,
    mut child: Child,
    timing: HealthCheckTiming,
) -> anyhow::Result<Child> {
    let deadline = tokio::time::Instant::now() + timing.total_timeout;

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            stop_team_memory_server(child).await;
            anyhow::bail!(
                "team-memory-server failed to start within {}ms",
                timing.total_timeout.as_millis()
            );
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("team-memory-server exited during startup with status {status}");
        }

        let attempt_timeout = timing.attempt_timeout.min(deadline - now);
        match tokio::time::timeout(attempt_timeout, client.get(health_url).send()).await {
            Ok(Ok(resp)) if resp.status().is_success() => return Ok(child),
            _ => {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if !remaining.is_zero() {
                    tokio::time::sleep(timing.retry_interval.min(remaining)).await;
                }
            }
        }
    }
}

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
    let client = team_memory_http_client()?;

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

    #[cfg(unix)]
    cmd.process_group(0);

    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    // Wait for health check.
    let health_url = format!("http://127.0.0.1:{}/health", port);
    let child =
        wait_for_team_memory_health(&client, &health_url, child, HEALTH_CHECK_TIMING).await?;
    info!(port, "team-memory-server is ready");

    Ok((child, port, secret))
}

/// Terminate the team-memory subprocess tree and bound process reaping.
pub async fn stop_team_memory_server(mut child: Child) {
    let terminator = child.id().map(|pid| {
        tokio::task::spawn_blocking(move || crate::process_state::terminate_process_tree(pid, None))
    });

    if tokio::time::timeout(SHUTDOWN_TIMEOUT, child.wait())
        .await
        .is_err()
    {
        error!("team-memory-server did not exit within shutdown timeout");
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
    }

    if let Some(terminator) = terminator {
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, terminator).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(err))) => {
                error!(error = %err, "failed to terminate team-memory-server process tree");
            }
            Ok(Err(err)) => {
                error!(error = %err, "team-memory-server terminator task failed");
            }
            Err(_) => {
                error!("team-memory-server terminator exceeded shutdown timeout");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Proxy handler
// ---------------------------------------------------------------------------

/// Proxy handler for `/api/claude_code/team_memory`.
///
/// Forwards the request to the Bun TS subprocess, transparently relaying
/// method, query string, headers (If-Match, If-None-Match), and body.
pub async fn proxy_team_memory(
    state: State<DaemonState>,
    method: Method,
    query: Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_team_memory_with_timeouts(state, method, query, headers, body, PROXY_TIMEOUTS).await
}

async fn proxy_team_memory_with_timeouts(
    State(state): State<DaemonState>,
    method: Method,
    query: Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
    timeouts: ProxyTimeouts,
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

    let client = match team_memory_http_client() {
        Ok(client) => client,
        Err(_error) => {
            error!("failed to initialize team-memory HTTP client");
            return proxy_error(StatusCode::BAD_GATEWAY, REQUEST_FAILED_MESSAGE);
        }
    };
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

    let resp = match tokio::time::timeout(timeouts.request, req.send()).await {
        Err(_) => return proxy_error(StatusCode::GATEWAY_TIMEOUT, REQUEST_TIMEOUT_MESSAGE),
        Ok(Err(error)) if error.is_timeout() => {
            return proxy_error(StatusCode::GATEWAY_TIMEOUT, REQUEST_TIMEOUT_MESSAGE);
        }
        Ok(Err(_error)) => {
            error!("team-memory proxy request failed");
            return proxy_error(StatusCode::BAD_GATEWAY, REQUEST_FAILED_MESSAGE);
        }
        Ok(Ok(resp)) => resp,
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let etag = resp.headers().get("etag").cloned();
    let body_bytes = match tokio::time::timeout(
        timeouts.body,
        read_limited_response_body(resp, MAX_PROXY_RESPONSE_BYTES),
    )
    .await
    {
        Err(_) => return proxy_error(StatusCode::GATEWAY_TIMEOUT, RESPONSE_TIMEOUT_MESSAGE),
        Ok(Err(LimitedBodyError::TooLarge)) => {
            return proxy_error(StatusCode::BAD_GATEWAY, RESPONSE_TOO_LARGE_MESSAGE);
        }
        Ok(Err(LimitedBodyError::Transport)) => {
            error!("team-memory proxy response body failed");
            return proxy_error(StatusCode::BAD_GATEWAY, RESPONSE_FAILED_MESSAGE);
        }
        Ok(Ok(body)) => body,
    };

    let mut builder = axum::http::Response::builder().status(status);
    if let Some(etag) = etag {
        builder = builder.header("ETag", etag);
    }
    builder = builder.header("Content-Type", "application/json");
    builder
        .body(axum::body::Body::from(body_bytes))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build error").into_response()
        })
}

fn team_memory_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        // The peer is fixed to loopback; never forward its shared secret through an environment proxy.
        .no_proxy()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .build()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitedBodyError {
    TooLarge,
    Transport,
}

async fn read_limited_response_body(
    response: reqwest::Response,
    limit: usize,
) -> Result<Bytes, LimitedBodyError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(LimitedBodyError::TooLarge);
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| LimitedBodyError::Transport)?;
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or(LimitedBodyError::TooLarge)?;
        if next_len > limit {
            return Err(LimitedBodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

fn proxy_error(status: StatusCode, message: &'static str) -> Response {
    (status, message).into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use allthecodes_config::features::FeatureFlags;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use axum::body::{to_bytes, Bytes};
    use axum::extract::{Query, State};
    use axum::http::{HeaderMap, Method, StatusCode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    #[cfg(unix)]
    use tokio::process::Command;

    use super::{proxy_team_memory_with_timeouts, DaemonState, ProxyTimeouts};
    #[cfg(unix)]
    use super::{team_memory_http_client, wait_for_team_memory_health, HealthCheckTiming};

    const TEST_COMPLETION_DEADLINE: Duration = Duration::from_secs(1);
    const EXPECTED_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
    const TEST_PROXY_TIMEOUTS: ProxyTimeouts = ProxyTimeouts {
        request: Duration::from_millis(200),
        body: Duration::from_millis(200),
    };

    enum PeerResponse {
        NeverSendHeaders,
        SlowBody,
        OversizedBody,
        OversizedChunkedBody,
    }

    fn proxy_state(port: u16, secret: &str) -> DaemonState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: ".".to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verification_policy: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }));
        let mut state = DaemonState::new(engine, Arc::new(FeatureFlags::all_disabled()), port);
        state.team_memory_port = Some(port);
        state.team_memory_secret = Some(secret.to_string());
        state
    }

    async fn spawn_peer(response: PeerResponse) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request_headers(&mut stream).await;
            match response {
                PeerResponse::NeverSendHeaders => {
                    std::future::pending::<()>().await;
                }
                PeerResponse::SlowBody => {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{",
                        )
                        .await
                        .unwrap();
                    stream.flush().await.unwrap();
                    std::future::pending::<()>().await;
                }
                PeerResponse::OversizedBody => {
                    let body_len = EXPECTED_MAX_RESPONSE_BYTES + 1;
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {body_len}\r\n\r\n"
                    );
                    stream.write_all(headers.as_bytes()).await.unwrap();
                    let body = vec![b'x'; body_len];
                    let _ = stream.write_all(&body).await;
                }
                PeerResponse::OversizedChunkedBody => {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n",
                        )
                        .await
                        .unwrap();
                    let chunk = vec![b'x'; 64 * 1024];
                    let chunk_header = format!("{:x}\r\n", chunk.len());
                    for _ in 0..=(EXPECTED_MAX_RESPONSE_BYTES / chunk.len()) {
                        if stream.write_all(chunk_header.as_bytes()).await.is_err()
                            || stream.write_all(&chunk).await.is_err()
                            || stream.write_all(b"\r\n").await.is_err()
                        {
                            break;
                        }
                    }
                    let _ = stream.write_all(b"0\r\n\r\n").await;
                }
            }
        });
        (port, task)
    }

    async fn read_request_headers(stream: &mut TcpStream) {
        let mut received = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !received.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.unwrap();
            if count == 0 {
                break;
            }
            received.extend_from_slice(&buffer[..count]);
        }
    }

    async fn call_proxy(port: u16, secret: &str) -> super::Response {
        proxy_team_memory_with_timeouts(
            State(proxy_state(port, secret)),
            Method::GET,
            Query(HashMap::new()),
            HeaderMap::new(),
            Bytes::new(),
            TEST_PROXY_TIMEOUTS,
        )
        .await
    }

    async fn response_text(response: super::Response) -> String {
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn proxy_times_out_when_peer_never_sends_headers() {
        let secret = "must-not-appear-in-errors";
        let (port, peer) = spawn_peer(PeerResponse::NeverSendHeaders).await;

        let response = tokio::time::timeout(TEST_COMPLETION_DEADLINE, call_proxy(port, secret))
            .await
            .expect("proxy must bound a peer that never sends response headers");
        peer.abort();

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = response_text(response).await;
        assert_eq!(body, "team-memory-server request timed out");
        assert!(!body.contains(secret));
    }

    #[tokio::test]
    async fn proxy_times_out_when_response_body_stalls() {
        let secret = "must-not-appear-in-errors";
        let (port, peer) = spawn_peer(PeerResponse::SlowBody).await;

        let response = tokio::time::timeout(TEST_COMPLETION_DEADLINE, call_proxy(port, secret))
            .await
            .expect("proxy must bound a stalled response body");
        peer.abort();

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = response_text(response).await;
        assert_eq!(body, "team-memory-server response timed out");
        assert!(!body.contains(secret));
    }

    #[tokio::test]
    async fn proxy_rejects_response_body_over_limit() {
        let secret = "must-not-appear-in-errors";
        let (port, peer) = spawn_peer(PeerResponse::OversizedBody).await;

        let response = tokio::time::timeout(TEST_COMPLETION_DEADLINE, call_proxy(port, secret))
            .await
            .expect("proxy must reject an oversized response without hanging");
        peer.abort();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = response_text(response).await;
        assert_eq!(body, "team-memory-server response exceeded size limit");
        assert!(!body.contains(secret));
    }

    #[tokio::test]
    async fn proxy_rejects_chunked_response_body_over_limit() {
        let secret = "must-not-appear-in-errors";
        let (port, peer) = spawn_peer(PeerResponse::OversizedChunkedBody).await;

        let response = tokio::time::timeout(TEST_COMPLETION_DEADLINE, call_proxy(port, secret))
            .await
            .expect("proxy must reject an oversized chunked response without hanging");
        peer.abort();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = response_text(response).await;
        assert_eq!(body, "team-memory-server response exceeded size limit");
        assert!(!body.contains(secret));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn health_deadline_stops_and_reaps_child_when_peer_stalls() {
        let (port, peer) = spawn_peer(PeerResponse::NeverSendHeaders).await;
        let client = team_memory_http_client().unwrap();
        let health_url = format!("http://127.0.0.1:{port}/health");
        let timing = HealthCheckTiming {
            total_timeout: Duration::from_millis(150),
            attempt_timeout: Duration::from_millis(40),
            retry_interval: Duration::from_millis(10),
        };

        let mut command = Command::new("sleep");
        command.arg("60").process_group(0).kill_on_drop(true);
        let child = command.spawn().unwrap();
        let child_pid = child.id().unwrap();

        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            wait_for_team_memory_health(&client, &health_url, child, timing),
        )
        .await
        .expect("health deadline and child cleanup must be bounded");
        peer.abort();

        let error = match outcome {
            Err(error) => error,
            Ok(mut child) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                panic!("stalled health peer unexpectedly became ready");
            }
        };
        assert!(error.to_string().contains("failed to start within 150ms"));
        assert!(
            !crate::process_state::process_is_alive(child_pid),
            "health deadline returned before child {child_pid} was reaped"
        );
    }
}
