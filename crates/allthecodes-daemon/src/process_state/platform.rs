use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

#[cfg(windows)]
use anyhow::Context;
use anyhow::Result;

#[cfg(windows)]
pub(super) fn configure_detached(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(not(windows))]
pub(super) fn configure_detached(_cmd: &mut Command) {}

#[cfg(unix)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.contains(&format!("\"{pid}\"")) || line.contains(&pid.to_string()))
}

#[cfg(unix)]
pub(crate) fn terminate_process_tree(pid: u32) -> Result<()> {
    let pid = pid as libc::pid_t;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(500));
    if process_is_alive(pid as u32) {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn terminate_process_tree(pid: u32) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .context("failed to run taskkill")?;
    if !status.success() {
        anyhow::bail!("taskkill failed with status {status}");
    }
    Ok(())
}
