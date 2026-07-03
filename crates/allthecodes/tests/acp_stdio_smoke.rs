use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const REAL_MODEL_PROMPT: &str =
    include_str!("../../allthecodes-acp/tests/fixtures/acp_smoke_prompt.txt");

struct AcpProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout_rx: mpsc::Receiver<String>,
    stderr: Arc<Mutex<String>>,
    observed: Vec<Value>,
    _isolated_home: Option<tempfile::TempDir>,
}

impl AcpProcess {
    fn spawn(project: &std::path::Path, isolate_home: bool) -> Self {
        let exe = assert_cmd::cargo::cargo_bin("allthecodes");
        let mut command = Command::new(exe);
        command
            .arg("--acp")
            .arg("--cwd")
            .arg(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let isolated_home = if isolate_home {
            let temp = tempfile::tempdir().expect("temp home");
            let allthecodes_home = temp.path().join("allthecodes-home");
            let user_home = temp.path().join("user-home");
            std::fs::create_dir_all(&allthecodes_home).expect("allthecodes home");
            std::fs::create_dir_all(&user_home).expect("user home");
            command.env("ALLTHECODES_HOME", &allthecodes_home);
            command.env("HOME", &user_home);
            Some(temp)
        } else {
            None
        };

        let mut child = command.spawn().expect("spawn allthecodes --acp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let stderr_pipe = child.stderr.take().expect("child stderr");

        let (stdout_tx, stdout_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if stdout_tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let stderr = Arc::new(Mutex::new(String::new()));
        let stderr_for_thread = stderr.clone();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr_pipe).read_to_string(&mut text);
            *stderr_for_thread.lock().expect("stderr lock") = text;
        });

        Self {
            child,
            stdin: Some(stdin),
            stdout_rx,
            stderr,
            observed: Vec::new(),
            _isolated_home: isolated_home,
        }
    }

    fn send_request(&mut self, id: u64, method: &str, params: Value) {
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let stdin = self.stdin.as_mut().expect("child stdin open");
        writeln!(stdin, "{frame}").expect("write request");
        stdin.flush().expect("flush request");
    }

    fn read_response(&mut self, id: u64, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            let value = self.read_jsonrpc(remaining(deadline));
            if value.get("id") == Some(&json!(id)) {
                return value;
            }
            self.observed.push(value);
        }
    }

    fn read_jsonrpc(&mut self, timeout: Duration) -> Value {
        let line = self
            .try_read_line(timeout)
            .unwrap_or_else(|| self.timeout_panic("timed out waiting for stdout line"));
        let value: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!(
                "stdout line was not JSON: {error}; line={line:?}; stderr={}",
                self.stderr_text()
            )
        });
        assert_eq!(
            value.get("jsonrpc").and_then(Value::as_str),
            Some("2.0"),
            "stdout line was JSON but not JSON-RPC 2.0: {value:?}"
        );
        value
    }

    fn try_read_jsonrpc(&mut self, timeout: Duration) -> Option<Value> {
        let line = self.try_read_line(timeout)?;
        let value: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!(
                "stdout line was not JSON: {error}; line={line:?}; stderr={}",
                self.stderr_text()
            )
        });
        assert_eq!(
            value.get("jsonrpc").and_then(Value::as_str),
            Some("2.0"),
            "stdout line was JSON but not JSON-RPC 2.0: {value:?}"
        );
        Some(value)
    }

    fn try_read_line(&mut self, timeout: Duration) -> Option<String> {
        match self.stdout_rx.recv_timeout(timeout) {
            Ok(line) => Some(line),
            Err(RecvTimeoutError::Timeout) => None,
            Err(error) => panic!(
                "stdout channel closed while waiting for JSON-RPC frame: {error}; status={:?}; stderr={}; observed={}",
                self.child.try_wait().ok().flatten(),
                self.stderr_text(),
                observed_summary(&self.observed)
            ),
        }
    }

    fn timeout_panic(&mut self, message: &str) -> ! {
        panic!(
            "{message}; status={:?}; stderr={}; observed={}",
            self.child.try_wait().ok().flatten(),
            self.stderr_text(),
            observed_summary(&self.observed)
        )
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().expect("stderr lock").clone()
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        for _ in 0..20 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for AcpProcess {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": 2,
        "info": {
            "name": "allthecodes-acp-smoke",
            "version": "0.0.0"
        },
        "capabilities": {}
    })
}

fn new_session_params(project: &std::path::Path) -> Value {
    json!({
        "cwd": project,
        "additionalDirectories": [],
        "mcpServers": []
    })
}

fn prompt_params(session_id: &str) -> Value {
    json!({
        "sessionId": session_id,
        "prompt": [
            {
                "type": "text",
                "text": REAL_MODEL_PROMPT.trim()
            }
        ]
    })
}

fn close_params(session_id: &str) -> Value {
    json!({ "sessionId": session_id })
}

fn response_result(response: &Value) -> &Value {
    assert!(
        response.get("error").is_none(),
        "request returned error: {response:?}"
    );
    response.get("result").expect("response result")
}

fn response_auth_methods(response: &Value) -> Vec<Value> {
    response
        .pointer("/result/authMethods")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn response_session_id(response: &Value) -> String {
    response_result(response)
        .get("sessionId")
        .and_then(Value::as_str)
        .expect("sessionId")
        .to_string()
}

fn update_payload(value: &Value) -> Option<&Value> {
    if value.get("method").and_then(Value::as_str) == Some("session/update") {
        value.pointer("/params/update")
    } else {
        None
    }
}

fn agent_text(update: &Value) -> Option<&str> {
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("agent_message_chunk") => update.pointer("/content/text").and_then(Value::as_str),
        Some("agent_message") => update.pointer("/content/0/text").and_then(Value::as_str),
        _ => None,
    }
}

fn is_idle(update: &Value) -> bool {
    update.get("sessionUpdate").and_then(Value::as_str) == Some("state_update")
        && update.get("state").and_then(Value::as_str) == Some("idle")
}

fn remaining(deadline: Instant) -> Duration {
    deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_else(|| Duration::from_millis(1))
}

fn real_model_timeout() -> Duration {
    std::env::var("ALLTHECODES_ACP_SMOKE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(120))
}

fn observed_summary(observed: &[Value]) -> String {
    if observed.is_empty() {
        return "[]".to_string();
    }

    let start = observed.len().saturating_sub(8);
    let entries = observed[start..]
        .iter()
        .map(message_summary)
        .collect::<Vec<_>>()
        .join(", ");
    if start == 0 {
        format!("[{entries}]")
    } else {
        format!("[... {} earlier, {entries}]", start)
    }
}

fn message_summary(value: &Value) -> String {
    if let Some(id) = value.get("id") {
        if value.get("error").is_some() {
            return format!("response id={id} error");
        }
        return format!("response id={id}");
    }

    match value.get("method").and_then(Value::as_str) {
        Some("session/update") => {
            let update = value.pointer("/params/update");
            let kind = update
                .and_then(|update| update.get("sessionUpdate"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let state = update
                .and_then(|update| update.get("state"))
                .and_then(Value::as_str)
                .map(|state| format!(" state={state}"))
                .unwrap_or_default();
            format!("session/update {kind}{state}")
        }
        Some(method) => format!("notification {method}"),
        None => "unknown JSON-RPC frame".to_string(),
    }
}

#[test]
fn acp_stdio_stdout_contains_only_jsonrpc_frames() {
    let project = tempfile::tempdir().expect("project tempdir");
    let mut acp = AcpProcess::spawn(project.path(), true);

    acp.send_request(1, "initialize", initialize_params());
    let initialize = acp.read_response(1, Duration::from_secs(30));
    response_result(&initialize);

    acp.send_request(2, "session/new", new_session_params(project.path()));
    let new_session = acp.read_response(2, Duration::from_secs(30));
    let session_id = response_session_id(&new_session);

    acp.send_request(3, "session/close", close_params(&session_id));
    let close = acp.read_response(3, Duration::from_secs(30));
    response_result(&close);

    acp.shutdown();
}

#[test]
#[ignore = "requires configured allthecodes credentials, provider access, and network"]
fn acp_stdio_real_model_prompt_smoke() {
    let project = tempfile::tempdir().expect("project tempdir");
    let mut acp = AcpProcess::spawn(project.path(), false);

    acp.send_request(1, "initialize", initialize_params());
    let initialize = acp.read_response(1, Duration::from_secs(60));
    response_result(&initialize);
    let auth_methods = response_auth_methods(&initialize);
    assert!(
        auth_methods.is_empty(),
        "real model smoke requires usable allthecodes credentials; initialize advertised auth methods: {auth_methods:?}. Authenticate with allthecodes /login or set a supported API credential before running this smoke."
    );

    acp.send_request(2, "session/new", new_session_params(project.path()));
    let new_session = acp.read_response(2, Duration::from_secs(60));
    let session_id = response_session_id(&new_session);

    acp.send_request(3, "session/prompt", prompt_params(&session_id));
    let prompt = acp.read_response(3, Duration::from_secs(60));
    response_result(&prompt);

    let timeout = real_model_timeout();
    let deadline = Instant::now() + timeout;
    let mut saw_model_text = false;
    let mut saw_idle = false;
    while Instant::now() < deadline {
        let Some(value) = acp.try_read_jsonrpc(remaining(deadline)) else {
            break;
        };
        if let Some(update) = update_payload(&value) {
            if agent_text(update).is_some_and(|text| !text.trim().is_empty()) {
                saw_model_text = true;
            }
            if is_idle(update) {
                saw_idle = true;
                if saw_model_text {
                    break;
                }
            }
        }
        acp.observed.push(value);
    }

    assert!(
        saw_model_text,
        "real model smoke did not observe non-empty model output within {:?}; verify allthecodes provider network access, backend config, and model availability. stderr={}; observed={}",
        timeout,
        acp.stderr_text(),
        observed_summary(&acp.observed)
    );
    assert!(
        saw_idle,
        "real model smoke did not observe final idle update within {:?}; verify allthecodes provider network access, backend config, and model availability. stderr={}; observed={}",
        timeout,
        acp.stderr_text(),
        observed_summary(&acp.observed)
    );

    acp.send_request(4, "session/list", json!({ "cwd": project.path() }));
    let list = acp.read_response(4, Duration::from_secs(30));
    response_result(&list);

    acp.send_request(5, "session/close", close_params(&session_id));
    let close = acp.read_response(5, Duration::from_secs(30));
    response_result(&close);

    acp.shutdown();
}
