use crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
/// Export a pre-rendered transcript body to a temp file and open it in
/// `$VISUAL` / `$EDITOR`. Returns the path on success. The TUI exits
/// alternate screen while the editor runs so the user can scroll freely,
/// and re-enters before returning to the main loop.
///
/// Falls back to "just write the file" when no editor env var is set.
pub(super) async fn export_to_editor(
    body: &str,
    mouse_capture_enabled: bool,
) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write as _;
    let mut path = std::env::temp_dir();
    let stem = format!(
        "allthecodes-transcript-{}.md",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    path.push(stem);
    {
        let mut f = std::fs::File::create(&path)?;
        f.write_all(body.as_bytes())?;
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok();
    if let Some(ed) = editor.filter(|s| !s.trim().is_empty()) {
        let parts = shell_words::split(&ed)
            .map_err(|e| anyhow::anyhow!("could not parse editor command '{}': {}", ed, e))?;
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("editor command is empty"))?;

        // Leave the alternate screen so the editor can paint over a real
        // terminal. Re-entering on the way out is handled by the caller
        // via the dirty flag + next render.
        if mouse_capture_enabled {
            let _ = execute!(
                std::io::stdout(),
                DisableFocusChange,
                DisableMouseCapture,
                LeaveAlternateScreen,
                cursor::Show
            );
        } else {
            let _ = execute!(
                std::io::stdout(),
                DisableFocusChange,
                LeaveAlternateScreen,
                cursor::Show
            );
        }
        let _ = terminal::disable_raw_mode();

        let status = tokio::process::Command::new(program)
            .args(args)
            .arg(&path)
            .status()
            .await;

        // Always re-arm the terminal, even on editor failure.
        let _ = terminal::enable_raw_mode();
        if mouse_capture_enabled {
            let _ = execute!(
                std::io::stdout(),
                EnterAlternateScreen,
                EnableFocusChange,
                EnableMouseCapture
            );
        } else {
            let _ = execute!(std::io::stdout(), EnterAlternateScreen, EnableFocusChange);
        }

        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                return Err(anyhow::anyhow!(
                    "{} exited with status {}",
                    ed,
                    s.code().unwrap_or(-1)
                ));
            }
            Err(e) => return Err(anyhow::anyhow!("could not launch '{}': {}", ed, e)),
        }
    }
    Ok(path)
}

pub(super) async fn open_reference_in_editor(
    reference: &str,
    cwd: &str,
    mouse_capture_enabled: bool,
) -> anyhow::Result<std::path::PathBuf> {
    let (path_text, line) = parse_path_reference(reference);
    let path = std::path::PathBuf::from(path_text);
    let path = if path.is_absolute() {
        path
    } else {
        std::path::Path::new(cwd).join(path)
    };
    let path = path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("could not resolve {}: {}", path.display(), e))?;
    if !path.is_file() {
        anyhow::bail!("{} is not a file", path.display());
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Set $VISUAL or $EDITOR to open {}", path.display()))?;

    let parts = shell_words::split(&editor)
        .map_err(|e| anyhow::anyhow!("could not parse editor command '{}': {}", editor, e))?;
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("editor command is empty"))?;

    if mouse_capture_enabled {
        let _ = execute!(
            std::io::stdout(),
            DisableFocusChange,
            DisableMouseCapture,
            LeaveAlternateScreen,
            cursor::Show
        );
    } else {
        let _ = execute!(
            std::io::stdout(),
            DisableFocusChange,
            LeaveAlternateScreen,
            cursor::Show
        );
    }
    let _ = terminal::disable_raw_mode();
    let mut command = tokio::process::Command::new(program);
    command.args(args);
    append_editor_location_args(&mut command, program, &path, line);
    let status = command.status().await;

    let _ = terminal::enable_raw_mode();
    if mouse_capture_enabled {
        let _ = execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            EnableFocusChange,
            EnableMouseCapture
        );
    } else {
        let _ = execute!(std::io::stdout(), EnterAlternateScreen, EnableFocusChange);
    }

    match status {
        Ok(s) if s.success() => Ok(path),
        Ok(s) => Err(anyhow::anyhow!(
            "{} exited with status {}",
            editor,
            s.code().unwrap_or(-1)
        )),
        Err(e) => Err(anyhow::anyhow!("could not launch '{}': {}", editor, e)),
    }
}

fn parse_path_reference(reference: &str) -> (&str, Option<u32>) {
    let raw = reference
        .strip_prefix("path=")
        .or_else(|| reference.strip_prefix("file="))
        .unwrap_or(reference)
        .trim();
    if let Some((path, line)) = raw.rsplit_once(':') {
        if let Ok(line) = line.parse::<u32>() {
            return (path, Some(line));
        }
    }
    (raw, None)
}

fn append_editor_location_args(
    command: &mut tokio::process::Command,
    program: &str,
    path: &std::path::Path,
    line: Option<u32>,
) {
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    match line {
        Some(line) if matches!(name.as_str(), "code" | "code-insiders" | "codium") => {
            command.arg("-g").arg(format!("{}:{line}", path.display()));
        }
        Some(line)
            if name.contains("vim")
                || matches!(name.as_str(), "vi" | "nvim" | "nano" | "emacs") =>
        {
            command.arg(format!("+{line}")).arg(path);
        }
        _ => {
            command.arg(path);
        }
    }
}
