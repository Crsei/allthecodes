use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::process::{Child, Command};

const TERMINATION_GRACE: Duration = Duration::from_millis(250);
const TERMINATION_WAIT: Duration = Duration::from_secs(5);

#[cfg(unix)]
pub(crate) fn configure_process_group(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(not(unix))]
pub(crate) fn configure_process_group(_command: &mut Command) {}

pub(crate) async fn terminate_child_tree(child: &mut Child) -> io::Result<()> {
    terminate_child_tree_with(child, terminate_tree).await
}

async fn terminate_child_tree_with<F, Fut>(child: &mut Child, terminate: F) -> io::Result<()>
where
    F: FnOnce(u32) -> Fut,
    Fut: Future<Output = io::Result<()>>,
{
    let pid = child
        .id()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "process has no pid"))?;
    let termination_result = terminate(pid).await;
    let wait_result = tokio::time::timeout(TERMINATION_WAIT, child.wait())
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "process did not exit after termination",
            )
        })?
        .map(|_| ());

    match (termination_result, wait_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(termination_error), Ok(())) => Err(termination_error),
        (Ok(()), Err(wait_error)) => Err(wait_error),
        (Err(termination_error), Err(wait_error)) => Err(io::Error::new(
            termination_error.kind(),
            format!("{termination_error}; direct child wait/reap also failed: {wait_error}"),
        )),
    }
}

#[cfg(unix)]
async fn terminate_tree(pid: u32) -> io::Result<()> {
    signal_group(pid, libc::SIGTERM)?;
    tokio::time::sleep(TERMINATION_GRACE).await;
    signal_group(pid, libc::SIGKILL)
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: libc::c_int) -> io::Result<()> {
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), signal) };
    if rc == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(windows)]
async fn terminate_tree(pid: u32) -> io::Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("taskkill failed with {status}")))
    }
}

#[cfg(not(any(unix, windows)))]
async fn terminate_tree(_pid: u32) -> io::Result<()> {
    Ok(())
}

pub(crate) fn force_terminate_tree(pid: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        signal_group(pid, libc::SIGKILL)
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("taskkill failed with {status}")))
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(())
    }
}

fn terminate_child_tree_blocking(child: &mut Child) -> io::Result<()> {
    let pid = child
        .id()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "process has no pid"))?;
    let termination_result = force_terminate_tree(pid);
    let deadline = std::time::Instant::now() + TERMINATION_WAIT;
    let wait_result = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Ok(()),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "process did not exit after termination",
                ));
            }
            Err(error) => break Err(error),
        }
    };

    termination_result.and(wait_result)
}

pub(crate) fn terminate_child_tree_without_runtime(child: Child) -> io::Result<()> {
    let child = Arc::new(Mutex::new(Some(child)));
    let background_child = Arc::clone(&child);
    match std::thread::Builder::new()
        .name("mcp-drop-reaper".to_string())
        .spawn(move || {
            let Some(mut child) = background_child
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            else {
                return;
            };
            let _ = terminate_child_tree_blocking(&mut child);
        }) {
        Ok(_) => Ok(()),
        Err(spawn_error) => {
            let mut child = child
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
                .ok_or_else(|| io::Error::other("MCP drop reaper lost child ownership"))?;
            terminate_child_tree_blocking(&mut child).map_err(|reap_error| {
                io::Error::other(format!(
                    "failed to spawn MCP drop reaper ({spawn_error}); fallback reap failed: {reap_error}"
                ))
            })
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[tokio::test]
    async fn disconnect_termination_stops_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!("(sleep 2; touch '{}') & wait", marker.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        terminate_child_tree(&mut child).await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "grandchild survived MCP disconnect");
    }

    #[tokio::test]
    async fn signaling_error_still_reaps_direct_child() {
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let error = terminate_child_tree_with(&mut child, |_| async {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected process-tree signaling failure",
            ))
        })
        .await
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert!(
            rc == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
            "signal failure returned without reaping direct pid {pid}"
        );
    }
}
