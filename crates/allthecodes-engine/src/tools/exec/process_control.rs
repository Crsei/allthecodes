//! Shared subprocess process-tree control for shell tools.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::sync::watch;

#[cfg(unix)]
const TERMINATION_GRACE: Duration = Duration::from_millis(250);
const TERMINATION_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) enum ControlledExit {
    Exited(io::Result<ExitStatus>),
    TimedOut(io::Result<ExitStatus>),
    Cancelled(io::Result<ExitStatus>),
}

/// Configure a spawned shell as its own process group/session where the
/// platform allows it. This lets timeout/cancel logic terminate descendants
/// rather than only the immediate shell wrapper.
#[cfg(unix)]
pub(crate) fn configure_process_group(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
pub(crate) fn configure_process_group(cmd: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn configure_process_group(_cmd: &mut Command) {}

pub(crate) async fn wait_for_exit_or_termination(
    child: &mut Child,
    timeout: Duration,
    mut abort_rx: watch::Receiver<bool>,
) -> ControlledExit {
    let pid = child.id();

    if *abort_rx.borrow() {
        let _ = terminate_process_tree(pid).await;
        return ControlledExit::Cancelled(child.wait().await);
    }

    let wait_future = child.wait();
    tokio::pin!(wait_future);
    let timeout_future = tokio::time::sleep(timeout);
    tokio::pin!(timeout_future);
    let mut abort_closed = false;

    loop {
        tokio::select! {
            status = &mut wait_future => return ControlledExit::Exited(status),
            _ = &mut timeout_future => {
                let _ = terminate_process_tree(pid).await;
                let status = wait_after_termination(&mut wait_future).await;
                return ControlledExit::TimedOut(status);
            }
            changed = abort_rx.changed(), if !abort_closed => {
                match changed {
                    Ok(()) if *abort_rx.borrow() => {
                        let _ = terminate_process_tree(pid).await;
                        let status = wait_after_termination(&mut wait_future).await;
                        return ControlledExit::Cancelled(status);
                    }
                    Ok(()) => {}
                    Err(_) => abort_closed = true,
                }
            }
        }
    }
}

async fn wait_after_termination<F>(
    wait_future: &mut std::pin::Pin<&mut F>,
) -> io::Result<ExitStatus>
where
    F: std::future::Future<Output = io::Result<ExitStatus>>,
{
    match tokio::time::timeout(TERMINATION_WAIT, wait_future).await {
        Ok(status) => status,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "process did not exit after termination",
        )),
    }
}

async fn terminate_process_tree(pid: Option<u32>) -> io::Result<()> {
    let pid =
        pid.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "process has no pid"))?;
    terminate_process_tree_by_pid(pid).await
}

#[cfg(unix)]
async fn terminate_process_tree_by_pid(pid: u32) -> io::Result<()> {
    send_signal_to_process_group(pid, libc::SIGTERM)?;
    tokio::time::sleep(TERMINATION_GRACE).await;
    send_signal_to_process_group(pid, libc::SIGKILL)
}

#[cfg(unix)]
fn send_signal_to_process_group(pid: u32, signal: libc::c_int) -> io::Result<()> {
    let pgid = -(pid as libc::pid_t);
    let rc = unsafe { libc::kill(pgid, signal) };
    if rc == -1 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(err)
        }
    } else {
        Ok(())
    }
}

#[cfg(windows)]
async fn terminate_process_tree_by_pid(pid: u32) -> io::Result<()> {
    let mut cmd = Command::new("taskkill");
    cmd.arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    configure_process_group(&mut cmd);

    let _ = cmd.status().await?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
async fn terminate_process_tree_by_pid(_pid: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn termination_stops_spawned_process() {
        let mut cmd = sleeper_command();
        configure_process_group(&mut cmd);
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        let mut child = cmd.spawn().expect("spawn sleeper");
        let pid = child.id();
        terminate_process_tree(pid)
            .await
            .expect("terminate sleeper");

        let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("sleeper should exit after termination")
            .expect("wait succeeds");
        assert!(!status.success());
    }

    #[cfg(windows)]
    fn sleeper_command() -> Command {
        let mut cmd = Command::new("powershell.exe");
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg("Start-Sleep -Seconds 30");
        cmd
    }

    #[cfg(unix)]
    fn sleeper_command() -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 30");
        cmd
    }
}
