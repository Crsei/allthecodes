//! Clipboard text copy support for TUI actions such as `/copy`.
//!
//! The upstream Codex TUI uses `arboard` for native clipboard access and falls
//! back to OSC 52 or WSL-specific handling when the process-local clipboard is
//! the wrong target. This port keeps the same policy shape without introducing
//! a new clipboard dependency: local desktop copies go through the platform's
//! standard clipboard command, SSH sessions use OSC 52, and WSL Linux shells use
//! `powershell.exe` to reach the Windows clipboard.

#[cfg(not(target_os = "android"))]
use base64::Engine as _;
#[cfg(all(not(target_os = "android"), unix))]
use std::fs::OpenOptions;
#[cfg(all(not(target_os = "android"), windows))]
use std::io::stdout;
#[cfg(not(target_os = "android"))]
use std::io::Write;
#[cfg(not(target_os = "android"))]
use std::process::{Command, Stdio};

#[cfg(all(not(target_os = "android"), target_os = "linux"))]
use super::clipboard_paste::is_probably_wsl;

/// Copies user-visible text into the most appropriate clipboard for this TUI.
///
/// In SSH sessions the copy is emitted as an OSC 52 sequence so the user's
/// terminal can proxy it to the client clipboard. Local sessions use common
/// platform clipboard commands: `Set-Clipboard` on Windows, `pbcopy` on macOS,
/// and Wayland/X11 helpers on Linux. Linux under WSL falls back to
/// `powershell.exe`, which is usually the only reliable path to the Windows
/// clipboard from a WSL terminal.
#[cfg(not(target_os = "android"))]
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    if std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some() {
        return copy_via_osc52(text);
    }

    copy_text_to_local_clipboard(text)
}

#[cfg(all(not(target_os = "android"), target_os = "windows"))]
fn copy_text_to_local_clipboard(text: &str) -> Result<(), String> {
    copy_via_powershell_clipboard("powershell.exe", text).or_else(|first| {
        copy_via_powershell_clipboard("pwsh", text)
            .map_err(|second| format!("{first}; PowerShell Core fallback failed: {second}"))
    })
}

#[cfg(all(not(target_os = "android"), target_os = "macos"))]
fn copy_text_to_local_clipboard(text: &str) -> Result<(), String> {
    write_to_command("pbcopy", &[], text).map_err(|err| format!("clipboard unavailable: {err}"))
}

#[cfg(all(not(target_os = "android"), target_os = "linux"))]
fn copy_text_to_local_clipboard(text: &str) -> Result<(), String> {
    if is_probably_wsl() {
        return copy_via_powershell_clipboard("powershell.exe", text);
    }

    let candidates: [(&str, &[&str]); 3] = [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    let mut errors = Vec::new();

    for (command, args) in candidates {
        match write_to_command(command, args, text) {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(format!("{command}: {err}")),
        }
    }

    Err(format!(
        "clipboard unavailable: no Linux clipboard command succeeded ({})",
        errors.join("; ")
    ))
}

#[cfg(all(
    not(target_os = "android"),
    not(any(target_os = "windows", target_os = "macos", target_os = "linux"))
))]
fn copy_text_to_local_clipboard(_text: &str) -> Result<(), String> {
    Err("clipboard unavailable: unsupported platform".to_string())
}

/// Writes text through OSC 52 so the controlling terminal owns the copy.
#[cfg(not(target_os = "android"))]
fn copy_via_osc52(text: &str) -> Result<(), String> {
    let sequence = osc52_sequence(text, std::env::var_os("TMUX").is_some());

    #[cfg(unix)]
    {
        let mut tty = OpenOptions::new()
            .write(true)
            .open("/dev/tty")
            .map_err(|e| {
                format!("clipboard unavailable: failed to open /dev/tty for OSC 52 copy: {e}")
            })?;
        tty.write_all(sequence.as_bytes()).map_err(|e| {
            format!("clipboard unavailable: failed to write OSC 52 escape sequence: {e}")
        })?;
        tty.flush().map_err(|e| {
            format!("clipboard unavailable: failed to flush OSC 52 escape sequence: {e}")
        })?;
    }

    #[cfg(windows)]
    {
        let mut out = stdout();
        out.write_all(sequence.as_bytes()).map_err(|e| {
            format!("clipboard unavailable: failed to write OSC 52 escape sequence: {e}")
        })?;
        out.flush().map_err(|e| {
            format!("clipboard unavailable: failed to flush OSC 52 escape sequence: {e}")
        })?;
    }

    Ok(())
}

#[cfg(all(
    not(target_os = "android"),
    any(target_os = "windows", target_os = "linux")
))]
fn copy_via_powershell_clipboard(command: &str, text: &str) -> Result<(), String> {
    write_to_command(
        command,
        &[
            "-NoProfile",
            "-Command",
            "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; \
             $ErrorActionPreference = 'Stop'; \
             $text = [Console]::In.ReadToEnd(); \
             Set-Clipboard -Value $text",
        ],
        text,
    )
    .map_err(|err| format!("clipboard unavailable: {err}"))
}

#[cfg(not(target_os = "android"))]
fn write_to_command(command: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {command}: {e}"))?;

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("failed to open {command} stdin"));
    };

    if let Err(err) = stdin.write_all(text.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("failed to write to {command}: {err}"));
    }

    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for {command}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!("{command} exited with status {}", output.status))
    } else {
        Err(format!("{command} failed: {stderr}"))
    }
}

/// Encodes text as an OSC 52 clipboard sequence.
#[cfg(not(target_os = "android"))]
fn osc52_sequence(text: &str, tmux: bool) -> String {
    let payload = base64::engine::general_purpose::STANDARD.encode(text);
    if tmux {
        format!("\x1bPtmux;\x1b\x1b]52;c;{payload}\x07\x1b\\")
    } else {
        format!("\x1b]52;c;{payload}\x07")
    }
}

/// Reports that clipboard text copy is unavailable on Android builds.
#[cfg(target_os = "android")]
pub fn copy_text_to_clipboard(_text: &str) -> Result<(), String> {
    Err("clipboard text copy is unsupported on Android".into())
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;

    #[test]
    fn osc52_sequence_encodes_text_for_terminal_clipboard() {
        assert_eq!(osc52_sequence("hello", false), "\u{1b}]52;c;aGVsbG8=\u{7}");
    }

    #[test]
    fn osc52_sequence_wraps_tmux_passthrough() {
        assert_eq!(
            osc52_sequence("hello", true),
            "\u{1b}Ptmux;\u{1b}\u{1b}]52;c;aGVsbG8=\u{7}\u{1b}\\"
        );
    }
}
