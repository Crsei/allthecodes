use std::io;
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
    let pid = child
        .id()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "process has no pid"))?;
    terminate_tree(pid).await?;
    tokio::time::timeout(TERMINATION_WAIT, child.wait())
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "process did not exit after termination",
            )
        })??;
    Ok(())
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
}
