use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

#[cfg(windows)]
use anyhow::Context;
use anyhow::Result;

use super::types::ProcessIdentityStatus;

#[cfg(windows)]
pub(super) fn configure_detached(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(all(not(unix), not(windows)))]
pub(super) fn configure_detached(_cmd: &mut Command) {}

#[cfg(unix)]
pub(super) fn configure_detached(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

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
pub(crate) fn process_start_key(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        linux_process_start_key(pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        unix_ps_process_start_key(pid)
    }
}

#[cfg(target_os = "linux")]
fn linux_process_start_key(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(") ")?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let start_time = fields.get(19)?;
    Some(format!("linux:{start_time}"))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn unix_ps_process_start_key(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then(|| format!("ps:{value}"))
}

#[cfg(windows)]
pub(crate) fn process_start_key(pid: u32) -> Option<String> {
    let script =
        format!("(Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\").CreationDate");
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then(|| format!("cim:{value}"))
}

pub(crate) fn process_matches_record(
    pid: u32,
    expected_start_key: Option<&str>,
) -> ProcessIdentityStatus {
    if !process_is_alive(pid) {
        return ProcessIdentityStatus::Dead;
    }
    let Some(expected) = expected_start_key.filter(|value| !value.trim().is_empty()) else {
        return ProcessIdentityStatus::Unrecorded;
    };
    let Some(actual) = process_start_key(pid) else {
        return ProcessIdentityStatus::Unknown;
    };
    if actual == expected {
        ProcessIdentityStatus::Matched
    } else {
        ProcessIdentityStatus::Mismatched {
            expected: expected.to_string(),
            actual: Some(actual),
        }
    }
}

#[cfg(unix)]
pub(crate) fn send_soft_terminate(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    ensure_process_identity(pid, expected_start_key)?;
    ensure_group_leader(pid)?;
    signal_process_group(pid, libc::SIGTERM)
}

#[cfg(unix)]
pub(crate) fn send_force_kill(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    ensure_process_identity(pid, expected_start_key)?;
    ensure_group_leader(pid)?;
    signal_process_group(pid, libc::SIGKILL)
}

#[cfg(windows)]
pub(crate) fn send_soft_terminate(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    ensure_process_identity(pid, expected_start_key)?;
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
        .context("failed to run taskkill")?;
    if !status.success() {
        anyhow::bail!("taskkill failed with status {status}");
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn send_force_kill(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    ensure_process_identity(pid, expected_start_key)?;
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .context("failed to run taskkill")?;
    if !status.success() {
        anyhow::bail!("taskkill /F failed with status {status}");
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn terminate_process_tree(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    ensure_process_identity(pid, expected_start_key)?;
    ensure_group_leader(pid)?;
    signal_process_group(pid, libc::SIGTERM)?;
    std::thread::sleep(Duration::from_millis(500));
    if process_group_is_alive(pid) {
        signal_process_group(pid, libc::SIGKILL)?;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn terminate_process_tree(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    send_soft_terminate(pid, expected_start_key)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    if process_matches_record(pid, expected_start_key).is_current_process_record() {
        send_force_kill(pid, expected_start_key)?;
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_group_leader(pid: u32) -> Result<()> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid != pid as libc::pid_t {
        anyhow::bail!("refusing to terminate pid {pid} as a process group: pgid={pgid}");
    }
    Ok(())
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: libc::c_int) -> Result<()> {
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), signal) };
    if rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(unix)]
fn process_group_is_alive(pid: u32) -> bool {
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn ensure_process_identity(pid: u32, expected_start_key: Option<&str>) -> Result<()> {
    match process_matches_record(pid, expected_start_key) {
        ProcessIdentityStatus::Dead => anyhow::bail!("process {pid} is not running"),
        ProcessIdentityStatus::Mismatched { expected, actual } => anyhow::bail!(
            "refusing to terminate pid {pid}: process identity mismatch expected={} actual={}",
            expected,
            actual.as_deref().unwrap_or("unknown")
        ),
        ProcessIdentityStatus::Matched
        | ProcessIdentityStatus::Unknown
        | ProcessIdentityStatus::Unrecorded => Ok(()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    #[test]
    fn terminate_process_tree_kills_the_worker_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let child_pid_path = temp.path().join("child.pid");
        let script = format!(
            "trap '' TERM; sleep 60 & echo $! > '{}'; wait",
            child_pid_path.display()
        );
        let mut command = Command::new("sh");
        command.arg("-c").arg(script).process_group(0);
        let mut leader = command.spawn().unwrap();
        let leader_pid = leader.id();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !child_pid_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let child_pid = std::fs::read_to_string(&child_pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let start_key = process_start_key(leader_pid).unwrap();

        terminate_process_tree(leader_pid, Some(&start_key)).unwrap();
        let _ = leader.wait();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while process_is_alive(child_pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(!process_is_alive(child_pid));
    }

    #[test]
    fn terminate_process_tree_rejects_a_mismatched_pid_identity() {
        let mut command = Command::new("sleep");
        command.arg("60").process_group(0);
        let mut child = command.spawn().unwrap();

        let error = terminate_process_tree(child.id(), Some("linux:not-this-process"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("identity mismatch"));
        assert!(process_is_alive(child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn force_kill_fallback_kills_the_daemon_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let child_pid_path = temp.path().join("child.pid");
        let script = format!("sleep 60 & echo $! > '{}'; wait", child_pid_path.display());
        let mut command = Command::new("sh");
        command.arg("-c").arg(script).process_group(0);
        let mut leader = command.spawn().unwrap();
        let leader_pid = leader.id();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !child_pid_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let child_pid = std::fs::read_to_string(&child_pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let start_key = process_start_key(leader_pid).unwrap();

        send_force_kill(leader_pid, Some(&start_key)).unwrap();
        let _ = leader.wait();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while process_is_alive(child_pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(!process_is_alive(child_pid));
    }
}
