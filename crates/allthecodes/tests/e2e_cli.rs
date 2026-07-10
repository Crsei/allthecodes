#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;
use serial_test::serial;

const ASSISTANT_WORKER_ID: &str = "assistant-session-1";
const BRIDGE_WORKER_ID: &str = "bridge-sync-1";
const PROACTIVE_WORKER_ID: &str = "proactive-1";
const SCHEDULER_WORKER_ID: &str = "scheduler-1";

struct IsolatedDaemon {
    exe: PathBuf,
    root: tempfile::TempDir,
    project_dir: PathBuf,
    allthecodes_home: PathBuf,
    user_home: PathBuf,
    codex_home: PathBuf,
    port: u16,
    started: bool,
}

impl IsolatedDaemon {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("isolated daemon root");
        let project_dir = root.path().join("project");
        let allthecodes_home = root.path().join("allthecodes-home");
        let user_home = root.path().join("user-home");
        let codex_home = root.path().join("codex-home");
        for dir in [&project_dir, &allthecodes_home, &user_home, &codex_home] {
            std::fs::create_dir_all(dir).expect("create isolated dir");
        }

        Self {
            exe: assert_cmd::cargo::cargo_bin("allthecodes"),
            root,
            project_dir,
            allthecodes_home,
            user_home,
            codex_home,
            port: unused_port(),
            started: false,
        }
    }

    fn base_command(&self) -> Command {
        let mut command = Command::new(&self.exe);
        command
            .current_dir(&self.project_dir)
            .env("FEATURE_KAIROS", "1")
            .env("ALLTHECODES_HOME", &self.allthecodes_home)
            .env("HOME", &self.user_home)
            .env("CODEX_HOME", &self.codex_home)
            .env("XDG_CONFIG_HOME", self.root.path().join("xdg-config"))
            .env("XDG_DATA_HOME", self.root.path().join("xdg-data"))
            .env("XDG_CACHE_HOME", self.root.path().join("xdg-cache"))
            .env("NO_COLOR", "1")
            .env("ANTHROPIC_API_KEY", "sk-ant-api03-e2e-offline-key")
            .env("ANTHROPIC_BASE_URL", "http://127.0.0.1:9")
            .env("ANTHROPIC_MODEL", "e2e-offline-model")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("ANTHROPIC_BEDROCK_BASE_URL")
            .env_remove("ANTHROPIC_DEFAULT_SOTA_MODEL")
            .env_remove("ANTHROPIC_DEFAULT_MOTA_MODEL")
            .env_remove("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .env_remove("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_BASE_URL")
            .env_remove("OPENAI_MODEL")
            .env_remove("OPENAI_CODEX_AUTH_TOKEN")
            .env_remove("OPENAI_CODEX_BASE_URL")
            .env_remove("OPENAI_CODEX_MODEL")
            .env_remove("AZURE_API_KEY")
            .env_remove("AZURE_BASE_URL")
            .env_remove("AZURE_MODEL")
            .env_remove("GOOGLE_API_KEY")
            .env_remove("GOOGLE_APPLICATION_CREDENTIALS")
            .env_remove("ALLTHECODES_VERTEX_ACCESS_TOKEN")
            .env_remove("GOOGLE_OAUTH_ACCESS_TOKEN")
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_SECRET_ACCESS_KEY")
            .env_remove("AWS_SESSION_TOKEN")
            .env_remove("AWS_BEARER_TOKEN_BEDROCK")
            .env_remove("ALLTHECODES_USE_VERTEX")
            .env_remove("ALLTHECODES_USE_BEDROCK")
            .env_remove("ALLTHECODES_USE_FOUNDRY")
            .env_remove("HTTPS_PROXY")
            .env_remove("https_proxy")
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = self.base_command();
        command.args(args);
        command.output().expect("run allthecodes command")
    }

    fn start(&mut self) -> Output {
        let mut command = self.base_command();
        let output = command
            .arg("--port")
            .arg(self.port.to_string())
            .arg("daemon")
            .arg("start")
            .output()
            .expect("start daemon");
        if output.status.success() {
            self.started = true;
        }
        output
    }

    fn stop(&mut self) -> Output {
        let output = self.run(&["daemon", "stop"]);
        self.started = false;
        output
    }

    fn get_json(&self, path: &str) -> Result<Value, String> {
        let token_path = self
            .allthecodes_home
            .join("daemon")
            .join("control-token.json");
        let token_file = std::fs::read_to_string(&token_path)
            .map_err(|error| format!("failed to read {}: {error}", token_path.display()))?;
        let token: Value = serde_json::from_str(&token_file)
            .map_err(|error| format!("failed to parse {}: {error}", token_path.display()))?;
        let token = token["token"]
            .as_str()
            .ok_or_else(|| format!("missing token in {}", token_path.display()))?;
        http_get_json(self.port, path, token)
    }

    fn wait_for_status<F>(&self, mut accept: F) -> Value
    where
        F: FnMut(&Value) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut last = String::new();
        while Instant::now() < deadline {
            match self.get_json("/api/status") {
                Ok(value) if accept(&value) => return value,
                Ok(value) => last = serde_json::to_string_pretty(&value).unwrap_or_default(),
                Err(error) => last = error,
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timed out waiting for daemon status; last={last}");
    }

    fn wait_for_command_done(&self, command_id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut last = String::new();
        while Instant::now() < deadline {
            let output = self.run(&["daemon", "command", command_id]);
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                match serde_json::from_str::<Value>(&stdout) {
                    Ok(command) => {
                        if matches!(
                            command.get("status").and_then(Value::as_str),
                            Some("handled" | "failed")
                        ) {
                            return command;
                        }
                        last = serde_json::to_string_pretty(&command).unwrap_or_default();
                    }
                    Err(error) => last = format!("{error}; stdout={stdout}"),
                }
            } else {
                last = output_text(&output);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timed out waiting for command {command_id}; last={last}");
    }
}

impl Drop for IsolatedDaemon {
    fn drop(&mut self) {
        if self.started {
            let _ = self.stop();
        }
    }
}

#[test]
#[serial]
fn daemon_cli_reports_stopped_without_state() {
    let daemon = IsolatedDaemon::new();

    let output = daemon.run(&["daemon", "status"]);

    assert_success(&output);
    assert_stdout_contains(&output, "daemon status: stopped");
}

#[test]
#[serial]
fn kairos_daemon_start_status_submit_sleep_and_stop_smoke() {
    let mut daemon = IsolatedDaemon::new();

    let start = daemon.start();
    assert_success(&start);
    assert_stdout_contains(&start, "daemon started:");

    let status = daemon.wait_for_status(|status| {
        status.get("supervisor_status").and_then(Value::as_str) == Some("running")
            && expected_worker_ids().is_subset(&worker_ids(status))
    });
    assert_eq!(status["kairos_active"], true);
    assert_eq!(status["proactive"], true);
    assert_eq!(status["automation_state"]["status"], "standby");
    assert_eq!(status["automation_state"]["query_running"], false);
    assert_eq!(status["automation_state"]["pending_input"], false);
    assert_path_under(&status["command_root"], &daemon.allthecodes_home);
    assert_path_under(&status["assistant_event_log"], &daemon.allthecodes_home);

    let cli_status = daemon.run(&["daemon", "status"]);
    assert_success(&cli_status);
    for expected in [
        "daemon status: running",
        ASSISTANT_WORKER_ID,
        BRIDGE_WORKER_ID,
        PROACTIVE_WORKER_ID,
        SCHEDULER_WORKER_ID,
    ] {
        assert_stdout_contains(&cli_status, expected);
    }

    let history = daemon.get_json("/api/history").expect("history json");
    assert!(
        history["history"].is_array(),
        "history field missing: {history}"
    );
    assert!(
        history["history_snapshots"].is_array(),
        "history_snapshots field missing: {history}"
    );
    assert!(
        history["daemon_events"].is_array(),
        "daemon_events field missing: {history}"
    );

    let submit = daemon.run(&["daemon", "submit", "hello from no-model e2e"]);
    assert_success(&submit);
    assert_stdout_contains(&submit, "daemon command queued:");
    let command_id = queued_command_id(&submit);
    let command = daemon.wait_for_command_done(&command_id);
    assert_eq!(command["target_worker_id"], ASSISTANT_WORKER_ID);
    assert_eq!(command["kind"], "submit");
    assert!(
        matches!(
            command.get("status").and_then(Value::as_str),
            Some("handled" | "failed")
        ),
        "submit command was not handled by worker: {command}"
    );

    let sleep = daemon.run(&["daemon", "sleep", "60", "e2e maintenance"]);
    assert_success(&sleep);
    assert_stdout_contains(&sleep, "daemon sleeping until");
    let sleeping = daemon.wait_for_status(|status| {
        status["automation_state"]["status"] == "sleeping"
            && status["daemon_sleep_reason"] == "e2e maintenance"
    });
    assert_eq!(sleeping["sleeping"], true);
    assert!(sleeping["daemon_sleep_until"].is_string());

    let stop = daemon.stop();
    assert_success(&stop);
    assert_stdout_contains(&stop, "daemon stopped");

    let stopped = daemon.run(&["daemon", "status"]);
    assert_success(&stopped);
    assert_stdout_contains(&stopped, "daemon status: stopped");
}

fn unused_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind random port");
    listener.local_addr().expect("local addr").port()
}

fn http_get_json(port: u16, path: &str, token: &str) -> Result<Value, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         x-allthecodes-daemon-token: {token}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| error.to_string())?;

    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .map_err(|error| error.to_string())?;
    let (headers, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("malformed HTTP response: {raw}"))?;
    if !headers.starts_with("HTTP/1.1 200") && !headers.starts_with("HTTP/1.0 200") {
        return Err(format!("unexpected HTTP response: {headers}; body={body}"));
    }

    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked_body(body)?
    } else {
        body.to_string()
    };
    serde_json::from_str(body.trim()).map_err(|error| format!("{error}; body={body}"))
}

fn decode_chunked_body(mut body: &str) -> Result<String, String> {
    let mut decoded = String::new();
    loop {
        let (len_line, after_len) = body
            .split_once("\r\n")
            .ok_or_else(|| format!("malformed chunked body: {body:?}"))?;
        let len = usize::from_str_radix(len_line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|error| format!("invalid chunk length {len_line:?}: {error}"))?;
        if len == 0 {
            return Ok(decoded);
        }
        if after_len.len() < len + 2 {
            return Err(format!("chunk body shorter than declared length {len}"));
        }
        decoded.push_str(&after_len[..len]);
        body = &after_len[len + 2..];
    }
}

fn expected_worker_ids() -> BTreeSet<String> {
    [
        ASSISTANT_WORKER_ID,
        BRIDGE_WORKER_ID,
        PROACTIVE_WORKER_ID,
        SCHEDULER_WORKER_ID,
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}

fn worker_ids(status: &Value) -> BTreeSet<String> {
    let Some(workers) = status["workers"].as_array() else {
        return BTreeSet::new();
    };
    workers
        .iter()
        .filter_map(|worker| worker["worker_id"].as_str())
        .map(ToOwned::to_owned)
        .collect()
}

fn queued_command_id(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .find_map(|part| part.strip_prefix("id="))
        .expect("queued command id")
        .to_string()
}

fn assert_path_under(value: &Value, root: &Path) {
    let path = value.as_str().expect("path string");
    assert!(
        Path::new(path).starts_with(root),
        "expected path {path} to be under {}",
        root.display()
    );
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed: {}",
        output_text(output)
    );
}

fn assert_stdout_contains(output: &Output, needle: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(needle),
        "stdout did not contain {needle:?}: {}",
        output_text(output)
    );
}

fn output_text(output: &Output) -> String {
    format!(
        "status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
