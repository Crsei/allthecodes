use crate::client::McpClient;
use crate::manager::McpManager;
use crate::transport::dispatch_response;
use crate::{JsonRpcError, JsonRpcResponse, McpConnectionState, McpServerConfig};

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};

use super::http_utils::{handle_sse_event_stream_status, redact_url_for_log};
use super::sse::{SseConnectTarget, SsePostTarget};

const HEADER_MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";
const HEADER_MCP_SESSION_ID: &str = "mcp-session-id";

async fn read_http_request(stream: &mut TcpStream) -> (String, String) {
    let mut buffer = Vec::new();
    let header_end = loop {
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }

        let mut chunk = [0_u8; 512];
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0, "connection closed before HTTP headers completed");
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = String::from_utf8(buffer[..header_end].to_vec()).unwrap();
    let body_start = header_end + 4;
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let mut chunk = vec![0_u8; content_length - body.len()];
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0, "connection closed before HTTP body completed");
        body.extend_from_slice(&chunk[..read]);
    }

    body.truncate(content_length);
    (head, String::from_utf8(body).unwrap())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn header_value(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        if header_name.eq_ignore_ascii_case(name) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &str,
) {
    let mut response = format!("HTTP/1.1 {}\r\nContent-Length: {}\r\n", status, body.len());
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    response.push_str(body);
    stream.write_all(response.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
    let _ = stream.shutdown().await;
}

#[test]
fn test_mcp_client_new() {
    let config = McpServerConfig {
        name: "test-server".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: Some(vec!["hello".to_string()]),
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let client = McpClient::new(config);
    assert_eq!(client.state, McpConnectionState::Pending);
    assert!(client.tools.is_empty());
    assert!(client.resources.is_empty());
}

#[test]
fn test_mcp_manager_new() {
    let manager = McpManager::new();
    assert!(manager.clients.is_empty());
    assert!(manager.all_tools().is_empty());
    assert!(manager.all_resources().is_empty());
}

#[test]
fn test_mcp_connect_retry_delay_uses_capped_exponential_backoff() {
    assert_eq!(crate::manager::connect_retry_delay_ms(0), 50);
    assert_eq!(crate::manager::connect_retry_delay_ms(1), 100);
    assert_eq!(crate::manager::connect_retry_delay_ms(2), 200);
    assert_eq!(crate::manager::connect_retry_delay_ms(3), 250);
    assert_eq!(crate::manager::connect_retry_delay_ms(20), 250);
}

#[tokio::test]
async fn test_mcp_manager_disconnect_server_removes_client() {
    let mut manager = McpManager::new();
    let config = McpServerConfig {
        name: "disconnect-me".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };
    manager
        .clients
        .insert("disconnect-me".to_string(), McpClient::new(config));

    assert!(manager.disconnect_server("disconnect-me").await);
    assert!(manager.clients.is_empty());
    assert!(!manager.disconnect_server("disconnect-me").await);
}

#[tokio::test]
async fn test_mcp_manager_connect_disabled_removes_existing_client() {
    let mut manager = McpManager::new();
    let existing = McpServerConfig {
        name: "disabled-server".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };
    manager
        .clients
        .insert("disabled-server".to_string(), McpClient::new(existing));

    let disabled = McpServerConfig {
        name: "disabled-server".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: Some(true),
    };

    manager.connect_server(disabled).await.unwrap();
    assert!(manager.clients.is_empty());
}

#[tokio::test]
async fn test_mcp_manager_reconnect_invalid_config_drops_stale_client() {
    let mut manager = McpManager::new();
    let existing = McpServerConfig {
        name: "reconnect-me".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };
    manager
        .clients
        .insert("reconnect-me".to_string(), McpClient::new(existing));

    let invalid = McpServerConfig {
        name: "reconnect-me".to_string(),
        transport: "stdio".to_string(),
        command: None,
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let error = manager.reconnect_server(invalid).await.unwrap_err();
    assert!(error.to_string().contains("command"));
    assert!(manager.clients.is_empty());
}

#[test]
fn test_jsonrpc_request_ids_increment() {
    let config = McpServerConfig {
        name: "test".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };
    let client = McpClient::new(config);

    let id1 = client.next_id.fetch_add(1, Ordering::SeqCst);
    let id2 = client.next_id.fetch_add(1, Ordering::SeqCst);
    assert_eq!(id1 + 1, id2);
}

#[tokio::test]
async fn test_dispatch_response_success() {
    let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let (tx, rx) = oneshot::channel();
    {
        let mut p = pending.lock().await;
        p.insert(42, tx);
    }

    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: json!(42),
        result: Some(json!({"tools": []})),
        error: None,
    };

    dispatch_response(&pending, "test", response).await;

    let result = rx.await.unwrap().unwrap();
    assert_eq!(result, json!({"tools": []}));
}

#[tokio::test]
async fn test_dispatch_response_error() {
    let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let (tx, rx) = oneshot::channel();
    {
        let mut p = pending.lock().await;
        p.insert(7, tx);
    }

    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: json!(7),
        result: None,
        error: Some(JsonRpcError {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: None,
        }),
    };

    dispatch_response(&pending, "test", response).await;

    let result = rx.await.unwrap();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid Request"));
}

#[tokio::test]
async fn test_dispatch_response_unknown_id() {
    let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: json!(99),
        result: Some(json!("ignored")),
        error: None,
    };

    // Should not panic
    dispatch_response(&pending, "test", response).await;
}

#[tokio::test]
async fn test_connect_stdio_missing_command() {
    let config = McpServerConfig {
        name: "bad-server".to_string(),
        transport: "stdio".to_string(),
        command: None,
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("command"));
}

#[test]
fn test_sse_connect_target_accepts_remote_https() {
    let target = SseConnectTarget::parse("https://example.com/mcp/sse?token=secret")
        .expect("https SSE target");
    match target {
        SseConnectTarget::RemoteHttps(target) => {
            assert_eq!(
                redact_url_for_log(target.url().as_str()),
                "https://example.com/mcp/sse?redacted"
            );
        }
        other => panic!("expected remote https target, got {:?}", other),
    }
}

#[test]
fn test_sse_remote_endpoint_resolves_same_origin_path() {
    let target = SseConnectTarget::parse("https://example.com/mcp/sse").expect("https SSE target");
    let post_target = target
        .resolve_endpoint("/messages?session=abc#ignored")
        .expect("same-origin endpoint");
    match post_target {
        SsePostTarget::RemoteHttps(target) => {
            assert_eq!(
                target.url().as_str(),
                "https://example.com/messages?session=abc"
            );
        }
        other => panic!("expected remote https post target, got {:?}", other),
    }
}

#[test]
fn test_sse_remote_endpoint_rejects_cross_origin() {
    let target = SseConnectTarget::parse("https://example.com/mcp/sse").expect("https SSE target");
    let err = target
        .resolve_endpoint("https://evil.example/messages")
        .unwrap_err();
    assert!(err.to_string().contains("same https origin"));
}

#[test]
fn test_sse_remote_endpoint_rejects_plain_http() {
    let target = SseConnectTarget::parse("https://example.com/mcp/sse").expect("https SSE target");
    let err = target
        .resolve_endpoint("http://example.com/messages")
        .unwrap_err();
    assert!(err.to_string().contains("https"));
}

#[test]
fn test_sse_auth_status_is_classified() {
    let err = handle_sse_event_stream_status(reqwest::StatusCode::UNAUTHORIZED, "remote-sse")
        .unwrap_err();
    assert!(crate::client::is_auth_needed_error(&err));
    assert!(err.to_string().contains("requires authentication"));
}

#[tokio::test]
async fn test_connect_sse_loopback_initializes_over_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut sse_stream, _) = listener.accept().await.unwrap();
        let (get_head, get_body) = read_http_request(&mut sse_stream).await;
        assert!(get_head.starts_with("GET /sse?token=ok HTTP/1.1"));
        assert!(get_head.contains("Accept: text/event-stream"));
        assert!(get_head.contains("Authorization: Bearer local"));
        assert!(get_body.is_empty());
        sse_stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\nevent: endpoint\r\ndata: /messages\r\n\r\n",
            )
            .await
            .unwrap();
        sse_stream.flush().await.unwrap();

        let (mut post_stream, _) = listener.accept().await.unwrap();
        let (post_head, post_body) = read_http_request(&mut post_stream).await;
        assert!(post_head.starts_with("POST /messages HTTP/1.1"));
        assert!(post_head.contains("Authorization: Bearer local"));
        let init_request: Value = serde_json::from_str(&post_body).unwrap();
        assert_eq!(init_request["method"], "initialize");
        let request_id = init_request["id"].as_u64().unwrap();
        post_stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        post_stream.flush().await.unwrap();

        let response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "local-sse", "version": "1.0.0" }
            }
        });
        let frame = format!(
            "event: message\r\ndata: {}\r\n\r\n",
            serde_json::to_string(&response).unwrap()
        );
        sse_stream.write_all(frame.as_bytes()).await.unwrap();
        sse_stream.flush().await.unwrap();

        let (mut notify_stream, _) = listener.accept().await.unwrap();
        let (notify_head, notify_body) = read_http_request(&mut notify_stream).await;
        assert!(notify_head.starts_with("POST /messages HTTP/1.1"));
        let notification: Value = serde_json::from_str(&notify_body).unwrap();
        assert_eq!(notification["method"], "notifications/initialized");
        notify_stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        notify_stream.flush().await.unwrap();
    });

    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer local".to_string());
    let config = McpServerConfig {
        name: "sse-server".to_string(),
        transport: "sse".to_string(),
        command: None,
        args: None,
        url: Some(format!("http://127.0.0.1:{}/sse?token=ok", addr.port())),
        headers: Some(headers),
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    client.connect().await.unwrap();
    client.initialize().await.unwrap();
    assert_eq!(client.state, McpConnectionState::Connected);
    assert!(client.supports_tools());
    client.disconnect().await;

    server.await.unwrap();
}

#[tokio::test]
async fn test_connect_sse_rejects_insecure_remote_http() {
    let config = McpServerConfig {
        name: "sse-server".to_string(),
        transport: "sse".to_string(),
        command: None,
        args: None,
        url: Some("http://example.com/mcp".to_string()),
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("https"));
}

#[tokio::test]
async fn test_connect_sse_rejects_header_injection() {
    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        "Bearer ok\r\nX-Bad: yes".to_string(),
    );
    let config = McpServerConfig {
        name: "sse-server".to_string(),
        transport: "sse".to_string(),
        command: None,
        args: None,
        url: Some("https://example.com/mcp".to_string()),
        headers: Some(headers),
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("header value"));
}

#[tokio::test]
async fn test_connect_sse_rejects_reserved_header_override() {
    let mut headers = HashMap::new();
    headers.insert("Host".to_string(), "evil.example".to_string());
    let config = McpServerConfig {
        name: "sse-server".to_string(),
        transport: "sse".to_string(),
        command: None,
        args: None,
        url: Some("https://example.com/mcp".to_string()),
        headers: Some(headers),
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("managed"));
}

#[tokio::test]
async fn test_connect_sse_rejects_missing_url() {
    let config = McpServerConfig {
        name: "sse-server".to_string(),
        transport: "sse".to_string(),
        command: None,
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("url"));
}

#[tokio::test]
async fn test_streamable_http_loopback_initializes_and_lists_tools() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut init_stream, _) = listener.accept().await.unwrap();
        let (init_head, init_body) = read_http_request(&mut init_stream).await;
        let init_request: Value =
            serde_json::from_str(&init_body).unwrap_or_else(|_| json!({"id": 1}));
        let init_id = init_request["id"].as_u64().unwrap();
        let init_response = json!({
            "jsonrpc": "2.0",
            "id": init_id,
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "stream-http", "version": "1.0.0" }
            }
        });
        write_http_response(
            &mut init_stream,
            "200 OK",
            &[
                ("Content-Type", "application/json"),
                ("MCP-Session-Id", "session-1"),
            ],
            &serde_json::to_string(&init_response).unwrap(),
        )
        .await;
        assert!(init_head.starts_with("POST /mcp HTTP/1.1"));
        assert!(header_value(&init_head, "accept")
            .unwrap()
            .contains("application/json"));
        assert_eq!(
            header_value(&init_head, HEADER_MCP_PROTOCOL_VERSION).unwrap(),
            "2025-11-25"
        );
        assert_eq!(init_request["method"], "initialize");
        assert_eq!(init_request["params"]["protocolVersion"], "2025-11-25");

        let (mut initialized_stream, _) = listener.accept().await.unwrap();
        let (initialized_head, initialized_body) = read_http_request(&mut initialized_stream).await;
        write_http_response(&mut initialized_stream, "202 Accepted", &[], "").await;
        assert_eq!(
            header_value(&initialized_head, HEADER_MCP_SESSION_ID).unwrap(),
            "session-1"
        );
        let initialized_notification: Value = serde_json::from_str(&initialized_body).unwrap();
        assert_eq!(
            initialized_notification["method"],
            "notifications/initialized"
        );

        let (mut get_stream, _) = listener.accept().await.unwrap();
        let (get_head, get_body) = read_http_request(&mut get_stream).await;
        write_http_response(&mut get_stream, "405 Method Not Allowed", &[], "").await;
        assert!(get_head.starts_with("GET /mcp HTTP/1.1"));
        assert_eq!(
            header_value(&get_head, HEADER_MCP_SESSION_ID).unwrap(),
            "session-1"
        );
        assert_eq!(
            header_value(&get_head, "accept").unwrap(),
            "text/event-stream"
        );
        assert!(get_body.is_empty());

        let (mut tools_stream, _) = listener.accept().await.unwrap();
        let (tools_head, tools_body) = read_http_request(&mut tools_stream).await;
        let tools_request: Value =
            serde_json::from_str(&tools_body).unwrap_or_else(|_| json!({"id": 2}));
        let tools_id = tools_request["id"].as_u64().unwrap();
        let tools_response = json!({
            "jsonrpc": "2.0",
            "id": tools_id,
            "result": {
                "tools": [{
                    "name": "search",
                    "description": "search things",
                    "inputSchema": {"type": "object"}
                }]
            }
        });
        let channel_notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/claude/channel",
            "params": {
                "content": "ready",
                "meta": {"transport": "streamable-http"}
            }
        });
        let body = format!(
            "event: message\r\ndata: {}\r\n\r\nevent: message\r\ndata: {}\r\n\r\n",
            serde_json::to_string(&channel_notification).unwrap(),
            serde_json::to_string(&tools_response).unwrap()
        );
        write_http_response(
            &mut tools_stream,
            "200 OK",
            &[("Content-Type", "text/event-stream")],
            &body,
        )
        .await;
        assert!(tools_head.starts_with("POST /mcp HTTP/1.1"));
        assert_eq!(
            header_value(&tools_head, HEADER_MCP_SESSION_ID).unwrap(),
            "session-1"
        );
        assert_eq!(tools_request["method"], "tools/list");
    });

    let config = McpServerConfig {
        name: "stream-http".to_string(),
        transport: "streamable-http".to_string(),
        command: None,
        args: None,
        url: Some(format!("http://127.0.0.1:{}/mcp", addr.port())),
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    client.connect().await.unwrap();
    client.initialize().await.unwrap();
    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search");
    assert_eq!(tools[0].server_name, "stream-http");
    client.disconnect().await;

    server.await.unwrap();
}

#[tokio::test]
async fn test_connect_streamable_http_rejects_insecure_remote_http() {
    let config = McpServerConfig {
        name: "stream-http".to_string(),
        transport: "streamable-http".to_string(),
        command: None,
        args: None,
        url: Some("http://example.com/mcp".to_string()),
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("https"));
}

#[tokio::test]
async fn test_connect_streamable_http_rejects_reserved_header_override() {
    let mut headers = HashMap::new();
    headers.insert("Accept".to_string(), "text/plain".to_string());
    let config = McpServerConfig {
        name: "stream-http".to_string(),
        transport: "streamable-http".to_string(),
        command: None,
        args: None,
        url: Some("https://example.com/mcp".to_string()),
        headers: Some(headers),
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.connect().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("managed"));
}

#[tokio::test]
async fn test_disconnect_idempotent() {
    let config = McpServerConfig {
        name: "test".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    client.disconnect().await;
    assert_eq!(client.state, McpConnectionState::Disconnected);

    client.disconnect().await;
    assert_eq!(client.state, McpConnectionState::Disconnected);
}

#[tokio::test]
async fn test_list_tools_not_connected() {
    let config = McpServerConfig {
        name: "test".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let mut client = McpClient::new(config);
    let result = client.list_tools().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not connected"));
}

#[tokio::test]
async fn test_call_tool_not_connected() {
    let config = McpServerConfig {
        name: "test".to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    };

    let client = McpClient::new(config);
    let result = client.call_tool("test", json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mcp_manager_connect_all_invalid_server() {
    let mut manager = McpManager::new();
    let configs = vec![McpServerConfig {
        name: "nonexistent".to_string(),
        transport: "stdio".to_string(),
        command: Some("this-command-does-not-exist-at-all-12345".to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    }];

    let result = manager.connect_all(configs).await;
    assert!(result.is_ok());
    assert!(manager.clients.is_empty());
}
