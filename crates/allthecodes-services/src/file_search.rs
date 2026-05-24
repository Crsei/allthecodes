//! File-search handler for the `SearchFiles` frontend command.
//!
//! Frontend-facing counterpart of upstream's global search dialog helper.
//! The frontend has no direct filesystem access, so the backend runs `rg`
//! or a pure-Rust fallback and returns one capped IPC response.

use std::path::Path;
use std::process::Command;

use allthecodes_ipc_protocol::{BackendMessage, FileSearchMatch};
use anyhow::{anyhow, Result};
use tracing::{debug, warn};

const DEFAULT_MAX_RESULTS: usize = 500;
const MAX_LINE_LEN: usize = 2048;

#[derive(Debug, Clone)]
pub struct FileSearchRequest {
    pub request_id: String,
    pub pattern: String,
    pub cwd: Option<String>,
    pub case_insensitive: bool,
    pub max_results: Option<usize>,
}

/// Spawn a bounded search on a blocking thread and pass the IPC response to
/// the supplied sender. The caller owns the concrete transport/sink.
pub fn dispatch_search<F>(request: FileSearchRequest, send: F)
where
    F: FnOnce(BackendMessage) + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let msg = handle_search(request);
        send(msg);
    });
}

pub fn handle_search(request: FileSearchRequest) -> BackendMessage {
    let cap = request
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .min(DEFAULT_MAX_RESULTS);
    let search_root = resolve_cwd(request.cwd.as_deref());
    debug!(
        "file_search: request_id={} pattern={:?} root={:?} cap={}",
        request.request_id, request.pattern, search_root, cap
    );

    match run_search(
        &request.pattern,
        &search_root,
        request.case_insensitive,
        cap,
    ) {
        Ok((matches, truncated)) => BackendMessage::FileSearchResult {
            request_id: request.request_id,
            matches,
            truncated,
            error: None,
        },
        Err(error) => {
            warn!("file_search: failed: {}", error);
            BackendMessage::FileSearchResult {
                request_id: request.request_id,
                matches: Vec::new(),
                truncated: false,
                error: Some(error.to_string()),
            }
        }
    }
}

fn resolve_cwd(cwd: Option<&str>) -> String {
    if let Some(explicit) = cwd {
        if !explicit.is_empty() {
            return explicit.to_string();
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string())
}

fn run_search(
    pattern: &str,
    search_root: &str,
    case_insensitive: bool,
    cap: usize,
) -> Result<(Vec<FileSearchMatch>, bool)> {
    if pattern.trim().is_empty() {
        return Ok((Vec::new(), false));
    }

    if let Some(result) = try_ripgrep(pattern, search_root, case_insensitive, cap) {
        return Ok(result);
    }

    fallback_search(pattern, search_root, case_insensitive, cap)
}

fn try_ripgrep(
    pattern: &str,
    search_root: &str,
    case_insensitive: bool,
    cap: usize,
) -> Option<(Vec<FileSearchMatch>, bool)> {
    if Command::new("rg").arg("--version").output().is_err() {
        return None;
    }

    let mut cmd = Command::new("rg");
    cmd.arg("-n").arg("--no-heading").arg("-F");
    if case_insensitive {
        cmd.arg("-i");
    }
    cmd.arg("-m").arg("10");
    cmd.arg("-e").arg(pattern);
    cmd.arg(search_root);

    let output = cmd.output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    let (matches, truncated) = parse_ripgrep_output(&stdout, search_root, cap);
    Some((matches, truncated))
}

fn parse_ripgrep_output(raw: &str, search_root: &str, cap: usize) -> (Vec<FileSearchMatch>, bool) {
    let mut matches = Vec::with_capacity(cap.min(256));
    let mut truncated = false;
    for line in raw.lines() {
        if matches.len() >= cap {
            truncated = true;
            break;
        }
        let Some(parsed) = split_rg_line(line) else {
            continue;
        };
        let rel = relativize(search_root, parsed.file);
        matches.push(FileSearchMatch {
            file: rel,
            line: parsed.line,
            text: truncate_line(parsed.text),
        });
    }
    (matches, truncated)
}

struct RgSplit<'a> {
    file: &'a str,
    line: u64,
    text: &'a str,
}

fn split_rg_line(line: &str) -> Option<RgSplit<'_>> {
    let bytes = line.as_bytes();
    for start in (0..bytes.len()).rev() {
        if bytes[start] != b':' {
            continue;
        }
        let rest = &line[start + 1..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let digits_len = digits.len();
        if rest.as_bytes().get(digits_len).copied() != Some(b':') {
            continue;
        }
        let line_num = digits.parse::<u64>().ok()?;
        let file = &line[..start];
        let text = &rest[digits_len + 1..];
        if file.is_empty() {
            continue;
        }
        return Some(RgSplit {
            file,
            line: line_num,
            text,
        });
    }
    None
}

fn relativize(search_root: &str, file: &str) -> String {
    let root_path = Path::new(search_root);
    let file_path = Path::new(file);
    match file_path.strip_prefix(root_path) {
        Ok(stripped) => stripped.to_string_lossy().replace('\\', "/"),
        Err(_) => file.replace('\\', "/"),
    }
}

fn truncate_line(line: &str) -> String {
    if line.len() <= MAX_LINE_LEN {
        return line.to_string();
    }
    let mut out = line.chars().take(MAX_LINE_LEN).collect::<String>();
    out.push_str(" …");
    out
}

fn fallback_search(
    pattern: &str,
    search_root: &str,
    case_insensitive: bool,
    cap: usize,
) -> Result<(Vec<FileSearchMatch>, bool)> {
    use ignore::WalkBuilder;
    use regex::{Regex, RegexBuilder};

    let literal = regex::escape(pattern);
    let regex: Regex = RegexBuilder::new(&literal)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| anyhow!(e))?;

    let mut walker = WalkBuilder::new(search_root);
    walker.follow_links(false).hidden(true);
    let mut matches = Vec::with_capacity(cap.min(256));
    let mut truncated = false;

    for entry in walker.build().flatten() {
        if matches.len() >= cap {
            truncated = true;
            break;
        }
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let Ok(data) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let mut found_in_file = 0usize;
        for (i, line) in data.lines().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            if matches.len() >= cap {
                truncated = true;
                break;
            }
            matches.push(FileSearchMatch {
                file: relativize(search_root, &entry.path().to_string_lossy()),
                line: (i as u64) + 1,
                text: truncate_line(line),
            });
            found_in_file += 1;
            if found_in_file >= 10 {
                break;
            }
        }
    }

    Ok((matches, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_rg_line_on_linux_path() {
        let parsed = split_rg_line("src/main.rs:42:fn main() {").expect("parsed");
        assert_eq!(parsed.file, "src/main.rs");
        assert_eq!(parsed.line, 42);
        assert_eq!(parsed.text, "fn main() {");
    }

    #[test]
    fn splits_rg_line_on_windows_drive() {
        let parsed = split_rg_line("C:\\repo\\file.rs:7:hello").expect("parsed");
        assert_eq!(parsed.file, "C:\\repo\\file.rs");
        assert_eq!(parsed.line, 7);
        assert_eq!(parsed.text, "hello");
    }

    #[test]
    fn truncate_caps_long_lines() {
        let long = "a".repeat(MAX_LINE_LEN * 2);
        let trimmed = truncate_line(&long);
        assert!(trimmed.len() <= MAX_LINE_LEN + 4);
        assert!(trimmed.ends_with('…'));
    }

    #[test]
    fn empty_pattern_returns_empty_result_message() {
        let msg = handle_search(FileSearchRequest {
            request_id: "search-1".into(),
            pattern: " ".into(),
            cwd: None,
            case_insensitive: true,
            max_results: Some(10),
        });

        assert!(matches!(
            msg,
            BackendMessage::FileSearchResult {
                request_id,
                matches,
                truncated: false,
                error: None,
            } if request_id == "search-1" && matches.is_empty()
        ));
    }
}
