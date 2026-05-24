//! Socket paths + cross-platform listener for the Chrome native host (#5).
//!
//! The native host runs as a subprocess of Chrome. When an MCP stdio bridge
//! (our `--claude-in-chrome-mcp` mode) connects to the native host's socket
//! it forwards tool-request JSON, and the native host relays it over Chrome
//! native-messaging to the extension.
//!
//! Two transports mirror the bun reference:
//! - **Unix**: a filesystem socket per-process, path
//!   `/tmp/claude-mcp-browser-bridge-{user}/{pid}.sock`, mode 0600.
//! - **Windows**: a named pipe, `\\.\pipe\claude-mcp-browser-bridge-{user}`.
//!
//! Socket paths are cross-process discoverable so a allthecodes MCP bridge spawned
//! later can find the native host that's already running under Chrome.

use std::io;
use std::path::PathBuf;

#[cfg(unix)]
use anyhow::Context;
use anyhow::Result;

/// Maximum message size (1 MiB). Mirrors the bun reference — keeps a single
/// stray `length` header from causing a ballooning allocation.
pub const MAX_MESSAGE_SIZE: u32 = 1024 * 1024;

/// Get the username we include in socket paths. Falls back to `"default"`
/// when the OS doesn't give us one (rare — Docker, some CI envs).
pub fn socket_username() -> String {
    if cfg!(windows) {
        std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string())
    } else {
        std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "default".to_string())
    }
}

/// Socket directory for Unix (`/tmp/claude-mcp-browser-bridge-{user}`).
///
/// Not used on Windows — named pipes have no parent directory.
pub fn socket_dir() -> PathBuf {
    let user = socket_username();
    PathBuf::from(format!("/tmp/claude-mcp-browser-bridge-{}", user))
}

/// The socket path for the currently-running process.
///
/// - Unix: `{socket_dir}/{pid}.sock`
/// - Windows: `\\.\pipe\claude-mcp-browser-bridge-{user}`
///
/// Per-PID on Unix lets several allthecodes processes coexist; on Windows
/// pipes are inherently process-scoped so the user-suffixed name is
/// unique enough.
pub fn secure_socket_path() -> PathBuf {
    if cfg!(windows) {
        let user = socket_username();
        PathBuf::from(format!(r"\\.\pipe\claude-mcp-browser-bridge-{}", user))
    } else {
        let pid = std::process::id();
        socket_dir().join(format!("{}.sock", pid))
    }
}

/// Enumerate every candidate socket a allthecodes MCP bridge could connect to.
///
/// On Unix we scan the socket directory for any `*.sock` files (one per
/// running native host). On Windows there's only one named pipe path.
pub fn all_socket_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        return vec![secure_socket_path()];
    }
    let dir = socket_dir();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "sock").unwrap_or(false) {
                out.push(path);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Socket preparation (Unix)
// ---------------------------------------------------------------------------

/// Create the socket directory with 0700 permissions (Unix only). Cleans up
/// any stale `*.sock` file whose PID no longer exists. No-op on Windows.
#[cfg(unix)]
pub fn prepare_socket_dir() -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = socket_dir();
    // If the path exists but isn't a directory (stale from an older version
    // that used the same path as a socket itself), remove it first.
    if let Ok(meta) = fs::metadata(&dir) {
        if !meta.is_dir() {
            let _ = fs::remove_file(&dir);
        }
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    // Tighten perms (ignore failures — directory may already exist with
    // looser perms on some systems we can't fix).
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));

    // Clean up dead sockets.
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(pid) = stem.parse::<u32>() else {
                continue;
            };
            if !pid_is_alive(pid) {
                let _ = fs::remove_file(&path);
                tracing::debug!(%pid, "claude-in-chrome: removed stale socket");
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn prepare_socket_dir() -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    // `kill -0 PID` probes existence without sending a real signal.
    // SAFETY: libc::kill is a thin system call. A zero signal sent to a
    // nonexistent PID returns -1 and sets errno=ESRCH.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Tighten socket permissions to 0600 (Unix only). Call after the socket
/// file has been created. No-op on Windows.
#[cfg(unix)]
pub fn secure_socket_file(path: &std::path::Path) -> io::Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub fn secure_socket_file(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_contains_user_or_pid() {
        let p = secure_socket_path();
        let s = p.to_string_lossy();
        if cfg!(windows) {
            assert!(s.starts_with(r"\\.\pipe\"));
            assert!(s.contains("claude-mcp-browser-bridge"));
        } else {
            assert!(s.starts_with("/tmp/"));
            assert!(s.ends_with(".sock"));
            let pid_part = p.file_stem().unwrap().to_string_lossy();
            assert!(pid_part.parse::<u32>().is_ok());
        }
    }

    #[test]
    fn socket_username_is_non_empty() {
        assert!(!socket_username().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn prepare_socket_dir_creates_dir() {
        prepare_socket_dir().expect("prepare_socket_dir should not fail");
        let dir = socket_dir();
        assert!(dir.is_dir(), "socket dir should exist after prepare");
    }
}
