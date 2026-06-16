use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub const DEFAULT_READY_POLL_INTERVAL: Duration = Duration::from_millis(50);
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessProbeResult {
    pub attempts: u32,
    pub elapsed: Duration,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessWaitError {
    pub ready_url: String,
    pub attempts: u32,
    pub elapsed: Duration,
    pub last_error: Option<String>,
}

impl fmt::Display for ReadinessWaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "daemon readiness probe failed for {} after {:?} (attempts={}",
            self.ready_url, self.elapsed, self.attempts
        )?;
        if let Some(last_error) = &self.last_error {
            write!(f, ", last_error={last_error}")?;
        }
        write!(f, ")")
    }
}

impl std::error::Error for ReadinessWaitError {}

pub fn ready_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/readyz")
}

pub fn wait_for_ready(port: u16) -> Result<ReadinessProbeResult, ReadinessWaitError> {
    wait_for_ready_with(port, DEFAULT_READY_TIMEOUT, DEFAULT_READY_POLL_INTERVAL)
}

pub fn wait_for_ready_with(
    port: u16,
    timeout: Duration,
    interval: Duration,
) -> Result<ReadinessProbeResult, ReadinessWaitError> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut attempts = 0u32;

    loop {
        attempts = attempts.saturating_add(1);
        let remaining = deadline.saturating_duration_since(Instant::now());
        let attempt_timeout = bounded_attempt_timeout(remaining);
        match probe_ready(port, attempt_timeout) {
            Ok(()) => {
                return Ok(ReadinessProbeResult {
                    attempts,
                    elapsed: started.elapsed(),
                    last_error: None,
                });
            }
            Err(error) => {
                let last_error = Some(error);
                if Instant::now() >= deadline {
                    return Err(ReadinessWaitError {
                        ready_url: ready_url(port),
                        attempts,
                        elapsed: started.elapsed(),
                        last_error,
                    });
                }

                let sleep_for = interval.min(deadline.saturating_duration_since(Instant::now()));
                if !sleep_for.is_zero() {
                    std::thread::sleep(sleep_for);
                }
            }
        }
    }
}

pub fn probe_ready(port: u16, timeout: Duration) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = timeout.max(Duration::from_millis(1));
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;

    let request =
        format!("GET /readyz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut buffer = [0u8; 1024];
    let bytes = stream
        .read(&mut buffer)
        .map_err(|error| error.to_string())?;
    if bytes == 0 {
        return Err("empty readiness response".to_string());
    }
    let response = String::from_utf8_lossy(&buffer[..bytes]);
    let status_line = response
        .lines()
        .next()
        .ok_or_else(|| "missing readiness status line".to_string())?;
    if status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        Err(format!("readiness returned {status_line}"))
    }
}

fn bounded_attempt_timeout(remaining: Duration) -> Duration {
    if remaining.is_zero() {
        Duration::from_millis(1)
    } else {
        remaining.min(DEFAULT_ATTEMPT_TIMEOUT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;

    fn spawn_http_status(status: &'static str, response_count: usize) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        thread::spawn(move || {
            for stream in listener.incoming().take(response_count) {
                let mut stream = stream.expect("stream");
                let mut request = [0u8; 512];
                let _ = stream.read(&mut request);
                let body = "{}";
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).expect("write");
            }
        });
        port
    }

    #[test]
    fn readiness_probe_accepts_200() {
        let port = spawn_http_status("200 OK", 1);
        probe_ready(port, Duration::from_secs(1)).expect("ready");
    }

    #[test]
    fn readiness_probe_rejects_non_200() {
        let port = spawn_http_status("503 Service Unavailable", 1);
        let error = probe_ready(port, Duration::from_secs(1)).expect_err("not ready");
        assert!(error.contains("503 Service Unavailable"));
    }

    #[test]
    fn readiness_wait_reports_timeout_and_last_error() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);

        let error = wait_for_ready_with(port, Duration::from_millis(40), Duration::from_millis(5))
            .expect_err("timeout");
        assert!(error.attempts > 0);
        assert!(error.last_error.is_some());
        assert!(error.to_string().contains(&ready_url(port)));
    }
}
