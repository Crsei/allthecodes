//! Path matching for sandbox filesystem rules.
//!
//! Supports the three prefix conventions from Claude Code sandbox spec:
//! - `/absolute/path` — absolute path
//! - `~/relative` — relative to `$HOME`
//! - `./relative` or bare `relative` — relative to the project root (or
//!   the enclosing `~/.allthecodes/` for user settings)
//!
//! We also accept the legacy `//absolute/path` form for backward
//! compatibility, matching the TS implementation.

use std::path::{Path, PathBuf};

/// Decision produced by [`PathResolver::check_write`] /
/// [`PathResolver::check_read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsDecision {
    /// Access is permitted.
    Allowed,
    /// Access is denied; `reason` identifies the matching rule.
    Denied { reason: String },
}

/// Resolve sandbox path rules relative to a workspace root and `$HOME`.
///
/// Constructed once per tool invocation with the effective settings in
/// hand, then queried per-path.
#[derive(Debug, Clone)]
pub struct PathResolver {
    workspace: PathBuf,
    home: Option<PathBuf>,
    /// Extra workspace roots (from `permissions.additionalDirectories` and
    /// `addDir` session grants).
    extra_workspaces: Vec<PathBuf>,
    allow_write: Vec<PathBuf>,
    deny_write: Vec<PathBuf>,
    allow_read: Vec<PathBuf>,
    deny_read: Vec<PathBuf>,
}

impl PathResolver {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            home: dirs::home_dir(),
            extra_workspaces: Vec::new(),
            allow_write: Vec::new(),
            deny_write: Vec::new(),
            allow_read: Vec::new(),
            deny_read: Vec::new(),
        }
    }

    pub fn with_home(mut self, home: Option<PathBuf>) -> Self {
        self.home = home;
        self
    }

    pub fn add_extra_workspace(&mut self, dir: PathBuf) {
        self.extra_workspaces.push(dir);
    }

    pub fn add_allow_write(&mut self, raw: &str) {
        if let Some(p) = self.resolve_raw(raw) {
            self.allow_write.push(p);
        }
    }
    pub fn add_deny_write(&mut self, raw: &str) {
        if let Some(p) = self.resolve_raw(raw) {
            self.deny_write.push(p);
        }
    }
    pub fn add_allow_read(&mut self, raw: &str) {
        if let Some(p) = self.resolve_raw(raw) {
            self.allow_read.push(p);
        }
    }
    pub fn add_deny_read(&mut self, raw: &str) {
        if let Some(p) = self.resolve_raw(raw) {
            self.deny_read.push(p);
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn allow_write_paths(&self) -> &[PathBuf] {
        &self.allow_write
    }
    pub fn deny_write_paths(&self) -> &[PathBuf] {
        &self.deny_write
    }
    pub fn allow_read_paths(&self) -> &[PathBuf] {
        &self.allow_read
    }
    pub fn deny_read_paths(&self) -> &[PathBuf] {
        &self.deny_read
    }
    pub fn extra_workspaces(&self) -> &[PathBuf] {
        &self.extra_workspaces
    }

    /// Resolve a raw path-pattern string into an absolute path.
    ///
    /// Returns `None` only if the path is empty or has no resolvable root
    /// (e.g. `~/...` when `$HOME` is unknown). Unknown forms default to
    /// workspace-relative, matching the spec.
    fn resolve_raw(&self, raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        // `~/...` — relative to home
        if let Some(rest) = trimmed.strip_prefix("~/") {
            return self.home.as_ref().map(|h| h.join(rest));
        }
        if trimmed == "~" {
            return self.home.clone();
        }

        // `//abs` — legacy absolute form
        if let Some(rest) = trimmed.strip_prefix("//") {
            return Some(PathBuf::from("/").join(rest));
        }

        // Absolute path (unix `/...` or windows drive like `C:\`)
        let p = PathBuf::from(trimmed);
        if p.is_absolute() {
            return Some(p);
        }

        // `./...` or bare — project-relative
        let stripped = trimmed.strip_prefix("./").unwrap_or(trimmed);
        Some(self.workspace.join(stripped))
    }

    /// Check whether a write to `path` is allowed under the current policy.
    ///
    /// Order of evaluation (first match wins):
    /// 1. `denyWrite` → denied
    /// 2. `allowWrite` → allowed (overrides workspace check)
    /// 3. Inside workspace or extra_workspaces → allowed
    /// 4. Otherwise → denied ("outside workspace")
    ///
    /// Works on logical paths only; does NOT touch the filesystem.
    pub fn check_write(&self, path: &Path) -> FsDecision {
        let normalized = normalize(path);
        for deny in &self.deny_write {
            if is_within(&normalized, deny) {
                return FsDecision::Denied {
                    reason: format!("denyWrite {}", deny.display()),
                };
            }
        }
        for allow in &self.allow_write {
            if is_within(&normalized, allow) {
                return FsDecision::Allowed;
            }
        }
        if is_within(&normalized, &self.workspace) {
            return FsDecision::Allowed;
        }
        for extra in &self.extra_workspaces {
            if is_within(&normalized, extra) {
                return FsDecision::Allowed;
            }
        }
        FsDecision::Denied {
            reason: format!("outside workspace {}", self.workspace.display()),
        }
    }

    /// Check whether a read of `path` is allowed.
    ///
    /// Order of evaluation:
    /// 1. `allowRead` → allowed (overrides `denyRead` by spec)
    /// 2. `denyRead` → denied
    /// 3. Otherwise → allowed (default)
    pub fn check_read(&self, path: &Path) -> FsDecision {
        let normalized = normalize(path);
        for allow in &self.allow_read {
            if is_within(&normalized, allow) {
                return FsDecision::Allowed;
            }
        }
        for deny in &self.deny_read {
            if is_within(&normalized, deny) {
                return FsDecision::Denied {
                    reason: format!("denyRead {}", deny.display()),
                };
            }
        }
        FsDecision::Allowed
    }
}

/// Lexical normalization — resolves `.` and `..` without touching the
/// filesystem. Symlinks are NOT resolved, matching the spec's behavior of
/// path matching.
fn normalize(path: &Path) -> PathBuf {
    let mut out = Vec::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                if !out.is_empty() {
                    out.pop();
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out.iter().collect()
}

fn is_within(path: &Path, dir: &Path) -> bool {
    let dir_norm = normalize(dir);
    path.starts_with(&dir_norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> PathResolver {
        PathResolver::new(PathBuf::from("/proj")).with_home(Some(PathBuf::from("/home/alice")))
    }

    #[test]
    fn resolve_absolute() {
        let r = resolver();
        assert_eq!(r.resolve_raw("/tmp/x"), Some(PathBuf::from("/tmp/x")));
        assert_eq!(r.resolve_raw("//tmp/x"), Some(PathBuf::from("/tmp/x")));
    }

    #[test]
    fn resolve_home() {
        let r = resolver();
        assert_eq!(
            r.resolve_raw("~/.kube"),
            Some(PathBuf::from("/home/alice/.kube"))
        );
    }

    #[test]
    fn resolve_project_relative() {
        let r = resolver();
        assert_eq!(r.resolve_raw("./out"), Some(PathBuf::from("/proj/out")));
        assert_eq!(r.resolve_raw("out"), Some(PathBuf::from("/proj/out")));
        assert_eq!(r.resolve_raw("."), Some(PathBuf::from("/proj")));
    }

    #[test]
    fn check_write_workspace_allowed() {
        let r = resolver();
        assert_eq!(
            r.check_write(Path::new("/proj/src/main.rs")),
            FsDecision::Allowed
        );
    }

    #[test]
    fn check_write_outside_denied() {
        let r = resolver();
        let d = r.check_write(Path::new("/etc/passwd"));
        assert!(matches!(d, FsDecision::Denied { .. }));
    }

    #[test]
    fn allow_write_overrides_outside() {
        let mut r = resolver();
        r.add_allow_write("/tmp/build");
        assert_eq!(
            r.check_write(Path::new("/tmp/build/x")),
            FsDecision::Allowed
        );
        let d = r.check_write(Path::new("/tmp/other/x"));
        assert!(matches!(d, FsDecision::Denied { .. }));
    }

    #[test]
    fn deny_write_overrides_workspace() {
        let mut r = resolver();
        r.add_deny_write("/proj/sensitive");
        let d = r.check_write(Path::new("/proj/sensitive/a.rs"));
        assert!(matches!(d, FsDecision::Denied { .. }));
    }

    #[test]
    fn deny_read_default_denies() {
        let mut r = resolver();
        r.add_deny_read("~/");
        let d = r.check_read(Path::new("/home/alice/secrets"));
        assert!(matches!(d, FsDecision::Denied { .. }));
    }

    #[test]
    fn allow_read_reopens_deny_read() {
        let mut r = resolver();
        r.add_deny_read("~/");
        r.add_allow_read("."); // = /proj, but also re-allow something inside ~
        r.add_allow_read("~/public");
        assert_eq!(
            r.check_read(Path::new("/home/alice/public/readme.md")),
            FsDecision::Allowed
        );
        // Still denied outside the re-allow
        let d = r.check_read(Path::new("/home/alice/secrets/key"));
        assert!(matches!(d, FsDecision::Denied { .. }));
    }

    #[test]
    fn normalize_handles_parent_dirs() {
        assert_eq!(normalize(Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("/a/./b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn extra_workspace_allows_writes() {
        let mut r = resolver();
        r.add_extra_workspace(PathBuf::from("/shared/libs"));
        assert_eq!(
            r.check_write(Path::new("/shared/libs/utils.rs")),
            FsDecision::Allowed
        );
    }
}
