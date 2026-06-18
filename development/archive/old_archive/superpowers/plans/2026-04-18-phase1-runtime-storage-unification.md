# Phase 1 Runtime Storage Unification — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify all cc-rust runtime persistence under `CC_RUST_HOME` (or `~/.cc-rust/`), eliminating repo-directory pollution and inconsistent home-dir resolution.

**Architecture:** Introduce `src/config/paths.rs` as the single source of truth for persistence paths. All call sites that previously did `dirs::home_dir().join(".cc-rust")...` migrate to `paths::<fn>()`. `main.rs`'s `.logs/` fallback and `dashboard.rs`'s `{cwd}/.logs/` hardcode are removed. Fallback chain: `CC_RUST_HOME` → `~/.cc-rust` → `std::env::temp_dir()/cc-rust` (with one-time warn).

**Tech Stack:** Rust 2021, `dirs` crate, `tracing`, `serial_test` (dev-only, new), existing `tempfile` test infra.

**Spec reference:** [development/archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md](../specs/2026-04-18-phase1-runtime-storage-unification-design.md).

---

## File Structure

### Created
- `src/config/paths.rs` — new module, ~180 lines (path-resolution functions + tests)
- `tests/cc_rust_home_subprocess.rs` — subprocess-level smoke test (binary-only crate, no lib import possible)
- `docs/STORAGE.md` — user-facing path reference

### Modified (call-site migration only, ~3–10 lines each)
- `Cargo.toml` — add `serial_test` to `[dev-dependencies]`
- `src/config/mod.rs` — declare `pub mod paths;`
- `src/config/settings.rs` — `global_claude_dir()` becomes paths wrapper
- `src/main.rs` — remove `.logs/` fallback; init dashboard session_id
- `src/session/storage.rs` — `get_session_dir()` → paths
- `src/session/export.rs` — `get_export_dir()` → paths
- `src/session/transcript.rs` — `get_transcript_dir()` → paths
- `src/session/audit_export.rs` — `get_audit_dir()` → paths
- `src/auth/token.rs` — `token_file_path()` → paths
- `src/daemon/memory_log.rs` — `log_dir()` → paths
- `src/daemon/team_memory_proxy.rs` — team memory path → paths
- `src/services/session_memory.rs` — remove custom `home_dir()` helper
- `src/skills/mod.rs` — user skills dir → paths
- `src/dashboard.rs` — session_id injection + path change
- `README.md`, `CLAUDE.md` — doc pointers

### Modified (optional cleanup — dead fallback removal)
- `src/session/memdir.rs` — use `paths::memory_dir_global()`
- `src/plugins/mod.rs` — use `paths::plugins_dir()`
- `src/observability/sink.rs` — use `paths::runs_dir()`

---

## Task 1: Add `serial_test` dev-dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Inspect current `[dev-dependencies]` block**

Run: `grep -n -A 20 "^\[dev-dependencies\]" Cargo.toml`

- [ ] **Step 2: Add `serial_test = "3"` to dev-dependencies**

Edit `Cargo.toml` — inside the existing `[dev-dependencies]` section (starts at line 135), append a new line:

```toml
serial_test = "3"
```

- [ ] **Step 3: Verify the dependency resolves**

Run: `cargo metadata --format-version 1 > /dev/null 2>&1 && echo OK`
Expected: `OK`

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add serial_test dev-dependency for paths tests"
```

---

## Task 2: Create `src/config/paths.rs` scaffolding with `data_root()`

**Files:**
- Create: `src/config/paths.rs`
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Declare the module in `src/config/mod.rs`**

Edit `src/config/mod.rs` — add `pub mod paths;` under the existing `pub mod` list. Final block should read:

```rust
pub mod claude_md;
pub mod constants;
pub mod features;
pub mod paths;
pub mod settings;
pub mod validation;
```

- [ ] **Step 2: Create `src/config/paths.rs` with ONLY `data_root()` + env tests**

Write `src/config/paths.rs`:

```rust
//! Central registry of cc-rust runtime persistence paths.
//!
//! All runtime data — sessions, logs, credentials, transcripts — is resolved
//! through this module. The resolution chain is:
//!
//! 1. `CC_RUST_HOME` env var (trim non-empty) → use as-is.
//! 2. `dirs::home_dir().join(".cc-rust")`.
//! 3. `std::env::temp_dir().join("cc-rust")` (non-persistent, warns once).
//!
//! Functions return `PathBuf` unconditionally; they never fail. Creation of
//! the directory is the caller's responsibility.

use std::path::PathBuf;
use std::sync::Once;

static TEMP_FALLBACK_WARN: Once = Once::new();

/// Return the cc-rust data root directory.
///
/// See module-level docs for the resolution chain.
pub fn data_root() -> PathBuf {
    if let Ok(override_dir) = std::env::var("CC_RUST_HOME") {
        if !override_dir.trim().is_empty() {
            return PathBuf::from(override_dir);
        }
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".cc-rust");
    }

    let tmp = std::env::temp_dir().join("cc-rust");
    TEMP_FALLBACK_WARN.call_once(|| {
        tracing::warn!(
            path = %tmp.display(),
            "unable to resolve home directory; cc-rust data will be written to a \
             non-persistent temp location. Set CC_RUST_HOME to override."
        );
    });
    tmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial]
    fn data_root_uses_env_override() {
        let _g = EnvGuard::set("CC_RUST_HOME", "/tmp/cc-rust-test-override");
        assert_eq!(data_root(), PathBuf::from("/tmp/cc-rust-test-override"));
    }

    #[test]
    #[serial]
    fn data_root_ignores_empty_env() {
        let _g = EnvGuard::set("CC_RUST_HOME", "");
        let root = data_root();
        assert!(
            root.ends_with(".cc-rust") || root.file_name().map(|n| n == "cc-rust").unwrap_or(false),
            "expected home fallback or temp fallback, got {}",
            root.display()
        );
    }

    #[test]
    #[serial]
    fn data_root_ignores_whitespace_env() {
        let _g = EnvGuard::set("CC_RUST_HOME", "   ");
        let root = data_root();
        assert!(
            root.ends_with(".cc-rust") || root.file_name().map(|n| n == "cc-rust").unwrap_or(false),
            "expected home fallback or temp fallback, got {}",
            root.display()
        );
    }

    #[test]
    #[serial]
    fn data_root_respects_env_with_whitespace_padding() {
        let _g = EnvGuard::set("CC_RUST_HOME", " /tmp/padded ");
        assert_eq!(data_root(), PathBuf::from(" /tmp/padded "));
    }

    #[test]
    #[serial]
    fn data_root_temp_fallback_best_effort() {
        let _g1 = EnvGuard::unset("CC_RUST_HOME");
        if dirs::home_dir().is_some() {
            eprintln!("skipping: dirs::home_dir() still resolvable via OS APIs");
            return;
        }
        let root = data_root();
        assert!(
            root.starts_with(std::env::temp_dir()),
            "expected temp fallback, got {}",
            root.display()
        );
    }
}
```

- [ ] **Step 3: Run the tests — expect them to pass**

Run: `cargo test -p cc-rust --lib config::paths`
(Adjust `-p` if the crate name differs — check `Cargo.toml` `[package] name`; likely `cc-rust` but confirm.)

Expected: 5 tests pass (one may print "skipping" on hosts with a resolvable home).

- [ ] **Step 4: Build with warnings check**

Run: `cargo build --all-targets 2>&1 | tee /tmp/build.log && ! grep -E "^warning:" /tmp/build.log`
Expected: exit 0 (no warnings on the new file; ignore pre-existing warnings elsewhere — but document any new ones introduced).

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs src/config/paths.rs
git commit -m "feat(paths): add central path registry with data_root() fallback chain"
```

---

## Task 3: Add partition functions to `paths.rs`

**Files:**
- Modify: `src/config/paths.rs`

- [ ] **Step 1: Append partition functions**

Insert **before** the `#[cfg(test)]` block in `src/config/paths.rs`:

```rust
use chrono::{DateTime, Local};
use std::path::Path;

// ----- Global paths (under data_root) --------------------------------------

pub fn sessions_dir() -> PathBuf {
    data_root().join("sessions")
}

pub fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

/// `{data_root}/logs/YYYY/MM/YYYY-MM-DD.md` — daemon daily log layout.
pub fn daily_log_path(now: DateTime<Local>) -> PathBuf {
    logs_dir()
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%Y-%m-%d.md").to_string())
}

pub fn credentials_path() -> PathBuf {
    data_root().join("credentials.json")
}

pub fn runs_dir(session_id: &str) -> PathBuf {
    data_root().join("runs").join(session_id)
}

pub fn exports_dir() -> PathBuf {
    data_root().join("exports")
}

pub fn audits_dir() -> PathBuf {
    data_root().join("audits")
}

pub fn transcripts_dir() -> PathBuf {
    data_root().join("transcripts")
}

pub fn memory_dir_global() -> PathBuf {
    data_root().join("memory")
}

pub fn session_insights_dir() -> PathBuf {
    data_root().join("session-insights")
}

pub fn plugins_dir() -> PathBuf {
    data_root().join("plugins")
}

pub fn skills_dir_global() -> PathBuf {
    data_root().join("skills")
}

/// `{data_root}/projects/{sanitized_cwd}/memory/team/`
pub fn team_memory_dir(cwd: &Path) -> PathBuf {
    let sanitized: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    data_root()
        .join("projects")
        .join(sanitized)
        .join("memory")
        .join("team")
}

// ----- Project-local paths (under cwd) -------------------------------------

/// `{cwd}/.cc-rust/` — project-level settings / memory / skills root.
pub fn project_cc_rust_dir(cwd: &Path) -> PathBuf {
    cwd.join(".cc-rust")
}
```

- [ ] **Step 2: Append smoke tests for partition functions**

Inside the existing `#[cfg(test)] mod tests { ... }` block in `src/config/paths.rs`, append these tests:

```rust
    #[test]
    #[serial]
    fn partition_functions_all_root_under_data_root() {
        let _g = EnvGuard::set("CC_RUST_HOME", "/tmp/cc-rust-partition-test");
        let base = PathBuf::from("/tmp/cc-rust-partition-test");
        assert_eq!(sessions_dir(), base.join("sessions"));
        assert_eq!(logs_dir(), base.join("logs"));
        assert_eq!(credentials_path(), base.join("credentials.json"));
        assert_eq!(runs_dir("abc"), base.join("runs").join("abc"));
        assert_eq!(exports_dir(), base.join("exports"));
        assert_eq!(audits_dir(), base.join("audits"));
        assert_eq!(transcripts_dir(), base.join("transcripts"));
        assert_eq!(memory_dir_global(), base.join("memory"));
        assert_eq!(session_insights_dir(), base.join("session-insights"));
        assert_eq!(plugins_dir(), base.join("plugins"));
        assert_eq!(skills_dir_global(), base.join("skills"));
    }

    #[test]
    #[serial]
    fn daily_log_path_builds_yyyy_mm_dd_layout() {
        let _g = EnvGuard::set("CC_RUST_HOME", "/tmp/cc-rust-dlp");
        let dt = Local.with_ymd_and_hms(2026, 4, 18, 10, 30, 0).unwrap();
        let p = daily_log_path(dt);
        assert!(p.ends_with("logs/2026/04/2026-04-18.md"), "got {}", p.display());
    }

    #[test]
    #[serial]
    fn team_memory_dir_sanitizes_cwd() {
        let _g = EnvGuard::set("CC_RUST_HOME", "/tmp/cc-rust-tmd");
        let cwd = Path::new("/home/user/My Project/sub");
        let p = team_memory_dir(cwd);
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(
            s.ends_with("_home_user_My_Project_sub/memory/team")
                || s.ends_with("/_home_user_My_Project_sub/memory/team"),
            "unexpected path: {}",
            s
        );
    }

    #[test]
    #[serial]
    fn project_cc_rust_dir_is_cwd_relative() {
        // Not affected by CC_RUST_HOME.
        let _g = EnvGuard::set("CC_RUST_HOME", "/tmp/ignored");
        assert_eq!(
            project_cc_rust_dir(Path::new("/foo/bar")),
            PathBuf::from("/foo/bar/.cc-rust")
        );
    }
```

Add `use chrono::TimeZone;` at the **top of the `mod tests`** block (right after `use super::*;` and `use serial_test::serial;`):

```rust
    use chrono::TimeZone;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib config::paths`
Expected: all 9 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/config/paths.rs
git commit -m "feat(paths): add partition functions for all persistence categories"
```

---

## Task 4: Make `global_claude_dir()` a `paths::data_root()` wrapper

**Files:**
- Modify: `src/config/settings.rs` (lines 96-106)

- [ ] **Step 1: Read current `global_claude_dir()`**

Open `src/config/settings.rs` — locate `pub fn global_claude_dir() -> Result<PathBuf>` at line 96.

- [ ] **Step 2: Replace implementation to delegate to `paths::data_root()`**

Replace lines 96-106 (entire `global_claude_dir` fn) with:

```rust
/// Return the path to the global cc-rust settings directory.
///
/// Historically this could fail if the home directory was unresolvable; the
/// unified [`crate::config::paths::data_root`] now never fails (it falls back
/// to a temp dir with a one-time warn), so `Result` is preserved only for
/// source compatibility with existing `?`-using callers.
pub fn global_claude_dir() -> Result<PathBuf> {
    Ok(crate::config::paths::data_root())
}
```

- [ ] **Step 3: Ensure `PathBuf` / `Result` imports still compile**

Run: `cargo check --lib`
Expected: no errors.

- [ ] **Step 4: Run settings unit tests**

Run: `cargo test --lib config::settings`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/config/settings.rs
git commit -m "refactor(settings): delegate global_claude_dir to paths::data_root"
```

---

## Task 5: Migrate `main.rs` tracing logger — remove `.logs/` fallback

**Files:**
- Modify: `src/main.rs` (lines 254-263)

- [ ] **Step 1: Locate the block**

Open `src/main.rs` at line 254. The block currently reads:

```rust
    let preferred_log_dir = crate::config::settings::global_claude_dir()
        .map(|d| d.join("logs"))
        .unwrap_or_else(|_| std::path::PathBuf::from(".logs"));
    let log_dir = if std::fs::create_dir_all(&preferred_log_dir).is_ok() {
        preferred_log_dir
    } else {
        let fallback = std::path::PathBuf::from(".logs");
        let _ = std::fs::create_dir_all(&fallback);
        fallback
    };
```

- [ ] **Step 2: Replace with paths-based resolution (no repo fallback)**

Replace those lines with:

```rust
    let log_dir = crate::config::paths::logs_dir();
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "warning: failed to create log directory {}: {}. File logging disabled.",
            log_dir.display(),
            e
        );
    }
```

**Note:** If `create_dir_all` fails, the subsequent `tracing_appender::rolling::daily` call will also fail on first write, but that failure is swallowed by `non_blocking` (events just drop). This is acceptable — previously we hid the problem under a repo-local fallback; now we surface it via stderr and accept degraded logging.

- [ ] **Step 3: Build and run a smoke check**

Run: `cargo build --release 2>&1 | tail -20`
Expected: success, no new warnings.

Run (manually, on a machine where you're comfortable): `CC_RUST_HOME=/tmp/cc-rust-manual ./target/release/cc-rust --version`
Expected: `/tmp/cc-rust-manual/logs/` exists afterwards; no `.logs/` in cwd.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix(main): remove .logs/ repo-dir fallback in tracing init"
```

---

## Task 6: Migrate `src/session/storage.rs`

**Files:**
- Modify: `src/session/storage.rs` (lines 65-69)

- [ ] **Step 1: Replace `get_session_dir()`**

Replace lines 65-69:

```rust
/// Return the base directory for session storage (`~/.cc-rust/sessions/`).
pub fn get_session_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".cc-rust").join("sessions")
}
```

with:

```rust
/// Return the base directory for session storage. Resolves through
/// [`crate::config::paths::sessions_dir`].
pub fn get_session_dir() -> PathBuf {
    crate::config::paths::sessions_dir()
}
```

- [ ] **Step 2: Verify existing tests still pass**

Run: `cargo test --lib session::storage`
Expected: all existing tests pass unchanged.

- [ ] **Step 3: Commit**

```bash
git add src/session/storage.rs
git commit -m "refactor(session/storage): route through paths::sessions_dir"
```

---

## Task 7: Migrate `src/session/export.rs`

**Files:**
- Modify: `src/session/export.rs` (lines 97-100)

- [ ] **Step 1: Replace `get_export_dir()`**

Replace:

```rust
fn get_export_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".cc-rust").join("exports")
}
```

with:

```rust
fn get_export_dir() -> PathBuf {
    crate::config::paths::exports_dir()
}
```

- [ ] **Step 2: Build + run existing tests**

Run: `cargo test --lib session::export`
Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add src/session/export.rs
git commit -m "refactor(session/export): route through paths::exports_dir"
```

---

## Task 8: Migrate `src/session/transcript.rs`

**Files:**
- Modify: `src/session/transcript.rs` (lines 41-44)

- [ ] **Step 1: Replace `get_transcript_dir()`**

Replace:

```rust
fn get_transcript_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".cc-rust").join("transcripts")
}
```

with:

```rust
fn get_transcript_dir() -> PathBuf {
    crate::config::paths::transcripts_dir()
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib session::transcript`
Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add src/session/transcript.rs
git commit -m "refactor(session/transcript): route through paths::transcripts_dir"
```

---

## Task 9: Migrate `src/session/audit_export.rs`

**Files:**
- Modify: `src/session/audit_export.rs` (lines 504-507)

- [ ] **Step 1: Replace `get_audit_dir()`**

Replace:

```rust
fn get_audit_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".cc-rust").join("audits")
}
```

with:

```rust
fn get_audit_dir() -> PathBuf {
    crate::config::paths::audits_dir()
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib session::audit_export`
Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add src/session/audit_export.rs
git commit -m "refactor(session/audit_export): route through paths::audits_dir"
```

---

## Task 10: Migrate `src/auth/token.rs`

**Files:**
- Modify: `src/auth/token.rs` (lines 7-13)

- [ ] **Step 1: Replace `token_file_path()`**

Replace:

```rust
/// Token storage file path: `~/.cc-rust/credentials.json`
pub fn token_file_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".cc-rust")
        .join("credentials.json")
}
```

with:

```rust
/// Token storage file path: `{data_root}/credentials.json`
pub fn token_file_path() -> std::path::PathBuf {
    crate::config::paths::credentials_path()
}
```

- [ ] **Step 2: Run auth tests**

Run: `cargo test --lib auth::token`
Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add src/auth/token.rs
git commit -m "refactor(auth/token): route through paths::credentials_path"
```

---

## Task 11: Migrate `src/daemon/memory_log.rs`

**Files:**
- Modify: `src/daemon/memory_log.rs` (lines 1-22)

- [ ] **Step 1: Delete internal `log_dir()` and update `today_log_path()`**

Replace lines 1-22:

```rust
//! Daily append-only log system for KAIROS perpetual sessions.
//! Logs stored at ~/.cc-rust/logs/YYYY/MM/YYYY-MM-DD.md

use std::path::PathBuf;

use chrono::Local;
use tracing::{debug, error};

fn log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-rust")
        .join("logs")
}

pub fn today_log_path() -> PathBuf {
    let now = Local::now();
    log_dir()
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%Y-%m-%d.md").to_string())
}
```

with:

```rust
//! Daily append-only log system for KAIROS perpetual sessions.
//! Logs stored at `{data_root}/logs/YYYY/MM/YYYY-MM-DD.md`.

use std::path::PathBuf;

use chrono::Local;
use tracing::{debug, error};

pub fn today_log_path() -> PathBuf {
    crate::config::paths::daily_log_path(Local::now())
}
```

- [ ] **Step 2: Check for unused imports**

Run: `cargo build --lib 2>&1 | grep "daemon/memory_log"`
Expected: no warnings. If `PathBuf` is no longer used directly inside this file, remove `use std::path::PathBuf;` — but `today_log_path() -> PathBuf` still references it as a return type, so keep it.

- [ ] **Step 3: Run daemon tests**

Run: `cargo test --lib daemon::memory_log`
Expected: pass.

- [ ] **Step 4: Commit**

```bash
git add src/daemon/memory_log.rs
git commit -m "refactor(daemon/memory_log): route through paths::daily_log_path"
```

---

## Task 12: Migrate `src/daemon/team_memory_proxy.rs`

**Files:**
- Modify: `src/daemon/team_memory_proxy.rs` (lines 41-61)

- [ ] **Step 1: Replace inline team memory path construction**

Replace lines 41-61 (the `let team_mem_path = { ... };` block) with:

```rust
    // Compute team memory path: {data_root}/projects/<sanitized>/memory/team/
    let team_mem_path = crate::config::paths::team_memory_dir(cwd);
```

- [ ] **Step 2: Build to verify nothing else referenced the old locals**

Run: `cargo check --lib`
Expected: no errors.

- [ ] **Step 3: Run daemon tests**

Run: `cargo test --lib daemon::team_memory_proxy`
Expected: pass (or "no tests to run" if none exist).

- [ ] **Step 4: Commit**

```bash
git add src/daemon/team_memory_proxy.rs
git commit -m "refactor(daemon/team_memory_proxy): route through paths::team_memory_dir"
```

---

## Task 13: Migrate `src/services/session_memory.rs` — remove custom home fallback

**Files:**
- Modify: `src/services/session_memory.rs` (lines 33-57)

- [ ] **Step 1: Replace `SessionMemoryConfig::default()` and delete `home_dir()` helper**

Replace lines 33-57:

```rust
impl Default for SessionMemoryConfig {
    fn default() -> Self {
        let home = home_dir();
        SessionMemoryConfig {
            enabled: true,
            memory_dir: home.join(".cc-rust").join("session-insights"),
            max_entries: 50,
            min_messages_before_extract: 5,
        }
    }
}

/// Cross-platform home directory resolution.
fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        // Fallback: try environment variables
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
        } else if let Ok(profile) = std::env::var("USERPROFILE") {
            PathBuf::from(profile)
        } else {
            PathBuf::from(".")
        }
    })
}
```

with:

```rust
impl Default for SessionMemoryConfig {
    fn default() -> Self {
        SessionMemoryConfig {
            enabled: true,
            memory_dir: crate::config::paths::session_insights_dir(),
            max_entries: 50,
            min_messages_before_extract: 5,
        }
    }
}
```

- [ ] **Step 2: Remove now-unused imports**

If `PathBuf` is no longer used elsewhere in the file, remove `use std::path::PathBuf;`. Check:

Run: `grep -n "PathBuf" src/services/session_memory.rs`
If all matches are inside the removed block, drop the import.

- [ ] **Step 3: Build — watch for warnings**

Run: `cargo build --lib 2>&1 | grep "session_memory"`
Expected: no new warnings.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib services::session_memory`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/services/session_memory.rs
git commit -m "refactor(services/session_memory): route through paths, drop custom home fallback"
```

---

## Task 14: Migrate `src/skills/mod.rs` — user skills dir

**Files:**
- Modify: `src/skills/mod.rs` (lines 232-242)

- [ ] **Step 1: Replace user skills discovery block**

Locate the block that starts at line 232:

```rust
    // 2. Load user skills from ~/.cc-rust/skills/
    if let Some(home) = dirs::home_dir() {
        let user_skills_dir = home.join(".cc-rust").join("skills");
        if user_skills_dir.is_dir() {
            let skills = loader::load_skills_from_dir(&user_skills_dir, SkillSource::User);
            for skill in skills {
                register_skill(skill);
            }
        }
    }
```

Replace with:

```rust
    // 2. Load user skills from {data_root}/skills/
    let user_skills_dir = crate::config::paths::skills_dir_global();
    if user_skills_dir.is_dir() {
        let skills = loader::load_skills_from_dir(&user_skills_dir, SkillSource::User);
        for skill in skills {
            register_skill(skill);
        }
    }
```

**Do not modify** the subsequent block for project skills (`if let Some(proj) = project_dir { ... }`) — that stays as-is since project-level `.cc-rust/skills/` is intentionally cwd-relative.

- [ ] **Step 2: Build + run tests**

Run: `cargo test --lib skills`
Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add src/skills/mod.rs
git commit -m "refactor(skills): route user skills through paths::skills_dir_global"
```

---

## Task 15: Migrate `src/dashboard.rs` — session_id injection + runs path

**Goal:** Move subagent event log from `{cwd}/.logs/subagent-events.ndjson` to `{data_root}/runs/{session_id}/subagent-events.ndjson`.

**Approach:** Add a process-global `OnceLock<String>` for session_id, initialized once during main-bootstrap. `event_log_path()` reads it and builds via `paths::runs_dir()`. If session_id is not yet set, events are dropped + `debug!`-logged.

**Files:**
- Modify: `src/dashboard.rs`
- Modify: `src/main.rs` (call to initialize session_id)

### Sub-task 15a: Add `init_session_id` / `event_log_path` refactor in `dashboard.rs`

- [ ] **Step 1: Add `SESSION_ID` `OnceLock` and setter**

Edit `src/dashboard.rs`. Under the existing `use std::sync::{LazyLock, Mutex};` import at line 5, extend to also import `OnceLock`:

```rust
use std::sync::{LazyLock, Mutex, OnceLock};
```

Under the existing `static EVENT_LOG_LOCK` at line 13, add:

```rust
static SESSION_ID: OnceLock<String> = OnceLock::new();

/// Initialize the dashboard's session_id. Called once during main bootstrap
/// (after QueryEngine is created). Subsequent calls are no-ops.
pub fn init_session_id(id: &str) {
    let _ = SESSION_ID.set(id.to_string());
}
```

- [ ] **Step 2: Rewrite `event_log_path()`**

Replace the existing `pub fn event_log_path() -> PathBuf` (lines 105-108):

```rust
pub fn event_log_path() -> PathBuf {
    let base = resolve_base_dir();
    event_log_path_for_base(&base)
}
```

with:

```rust
/// Return the subagent event log path. Resolves to
/// `{data_root}/runs/{session_id}/subagent-events.ndjson` once the session_id
/// has been initialized via [`init_session_id`]. Before that, returns a
/// sentinel path; callers should check via [`event_log_ready`].
pub fn event_log_path() -> Option<PathBuf> {
    SESSION_ID
        .get()
        .map(|id| crate::config::paths::runs_dir(id).join("subagent-events.ndjson"))
}

pub fn event_log_ready() -> bool {
    SESSION_ID.get().is_some()
}
```

- [ ] **Step 3: Update `append_event()` to handle `None`**

Find the existing `fn append_event(event: &SubagentEvent) -> Result<()>` (around line 138) and replace:

```rust
fn append_event(event: &SubagentEvent) -> Result<()> {
    let path = event_log_path();
    append_event_to_path(&path, event)
}
```

with:

```rust
fn append_event(event: &SubagentEvent) -> Result<()> {
    match event_log_path() {
        Some(path) => append_event_to_path(&path, event),
        None => {
            debug!(
                kind = %event.kind,
                agent_id = %event.agent_id,
                "subagent event dropped: session_id not initialized yet"
            );
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Update `DashboardConfig::default()` — use sentinel if session not yet set**

Replace `DashboardConfig::default()` (lines 26-36):

```rust
impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            event_log_path: event_log_path(),
            auto_open_browser: std::env::var("FEATURE_SUBAGENT_DASHBOARD_OPEN")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
                .unwrap_or(false),
        }
    }
}
```

with:

```rust
impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            event_log_path: event_log_path()
                .expect("DashboardConfig::default() called before init_session_id"),
            auto_open_browser: std::env::var("FEATURE_SUBAGENT_DASHBOARD_OPEN")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
                .unwrap_or(false),
        }
    }
}
```

**Rationale for `expect`**: The dashboard is spawned after `init_session_id` — see Task 15b. If this panics in practice, it means `main.rs` initialization order is broken, which is a programming error worth surfacing.

- [ ] **Step 5: Delete now-unused `resolve_base_dir()` + `event_log_path_for_base()` and their usages**

Remove these two functions (lines 168-178):

```rust
fn event_log_path_for_base(base: &Path) -> PathBuf {
    base.join(".logs").join("subagent-events.ndjson")
}

fn resolve_base_dir() -> PathBuf {
    let cwd = crate::bootstrap::state::original_cwd();
    if !cwd.as_os_str().is_empty() {
        return cwd;
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
```

Also update / delete the corresponding test `fn event_log_path_for_base_appends_logs_file()` at line 271 (since it tests the removed fn). Replace it with a new test:

```rust
    #[test]
    #[serial_test::serial]
    fn event_log_path_returns_none_before_init() {
        // Reset by using a separate process or a test-only reset hook.
        // Since OnceLock has no reset, we only test the Some-case after init.
        // Here we verify that if already initialized, path structure is correct.
        let _ = SESSION_ID.set("test-session".to_string());
        let path = event_log_path().expect("should resolve after init");
        let s = path.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("/runs/test-session/subagent-events.ndjson"), "got {}", s);
    }
```

**Note on the test:** `OnceLock::set` fails silently if already set; if this test runs after another has set `SESSION_ID`, it uses the previously-set value. That's OK for the path-structure assertion as long as the session_id used in the assertion matches. For robustness, capture the actually-set value:

```rust
    #[test]
    #[serial_test::serial]
    fn event_log_path_points_into_runs_dir() {
        // OnceLock can only be set once per process — tolerate prior sets.
        let _ = SESSION_ID.set("test-session".to_string());
        let path = event_log_path().expect("session_id should be set by now");
        let s = path.to_string_lossy().replace('\\', "/");
        assert!(
            s.contains("/runs/"),
            "expected path under runs/, got {}",
            s
        );
        assert!(
            s.ends_with("/subagent-events.ndjson"),
            "expected subagent-events.ndjson filename, got {}",
            s
        );
    }
```

- [ ] **Step 6: Update / remove other tests that depended on removed helpers**

Read the full `#[cfg(test)]` mod tests in `src/dashboard.rs` (starts around line 242). For any test still referencing `event_log_path_for_base`, delete it. For `append_event_to_path_writes_valid_ndjson` (line 280), it takes a `path` parameter directly, so it should still work — verify by building.

- [ ] **Step 7: Remove unused imports**

Run: `cargo build --lib 2>&1 | grep "dashboard"`
If `Path` is no longer used directly (only `PathBuf`), adjust. Likely `use std::path::{Path, PathBuf};` can become `use std::path::PathBuf;` — verify with the compiler.

- [ ] **Step 8: Build**

Run: `cargo build --lib`
Expected: no errors, no new warnings.

- [ ] **Step 9: Run dashboard tests**

Run: `cargo test --lib dashboard`
Expected: pass.

### Sub-task 15b: Call `dashboard::init_session_id()` from `main.rs`

- [ ] **Step 1: Locate `engine.session_id` first use in main**

Run: `grep -n "engine.session_id" src/main.rs | head -5`
Expected: first occurrence near line 588 (`info!(session = %engine.session_id, "QueryEngine created");`).

- [ ] **Step 2: Insert `init_session_id` call**

Right after the `info!(session = %engine.session_id, "QueryEngine created");` line (line 588), add:

```rust
    crate::dashboard::init_session_id(engine.session_id.as_str());
```

This ensures session_id is globally available **before** the dashboard companion is spawned at line 786 and before any tool use emits subagent events.

- [ ] **Step 3: Build**

Run: `cargo build --release`
Expected: success.

- [ ] **Step 4: Manual smoke test**

Run (manually): `FEATURE_SUBAGENT_DASHBOARD=1 CC_RUST_HOME=/tmp/cc-rust-dash-smoke ./target/release/cc-rust --headless` — send one `ping` IPC message and exit.

Verify:
```bash
ls -la /tmp/cc-rust-dash-smoke/runs/*/subagent-events.ndjson 2>/dev/null || echo "no events (OK if no subagent ran)"
ls -la ./.logs/ 2>/dev/null && echo "FAIL: .logs/ created" || echo "OK: no .logs/ in cwd"
```

Expected: `OK: no .logs/ in cwd`.

- [ ] **Step 5: Commit**

```bash
git add src/dashboard.rs src/main.rs
git commit -m "feat(dashboard): move subagent events to \$ROOT/runs/{session_id}/ and inject session_id"
```

---

## Task 16: Optional cleanup — remove redundant `.unwrap_or_else` fallbacks

**Goal:** Three call sites still carry dead fallback branches (`global_claude_dir().unwrap_or_else(|_| PathBuf::from(...))`). Since `global_claude_dir()` is now infallible via the wrapper, these fallbacks are dead code. Simplify them.

**Files:**
- Modify: `src/session/memdir.rs`
- Modify: `src/plugins/mod.rs`
- Modify: `src/observability/sink.rs`

- [ ] **Step 1: `src/session/memdir.rs`**

Find the `MemoryScope::Global` branch (around line 57-58):

```rust
        MemoryScope::Global => {
            let global = settings::global_claude_dir()?;
            Ok(global.join("memory"))
        }
```

Replace with:

```rust
        MemoryScope::Global => Ok(crate::config::paths::memory_dir_global()),
```

- [ ] **Step 2: `src/plugins/mod.rs`**

Find `plugins_dir()` (line 136-138):

```rust
pub fn plugins_dir() -> PathBuf {
    crate::config::settings::global_claude_dir()
        .unwrap_or_else(|_| PathBuf::from(".").join(".cc-rust"))
        .join("plugins")
}
```

Replace with:

```rust
pub fn plugins_dir() -> PathBuf {
    crate::config::paths::plugins_dir()
}
```

**Caveat:** This introduces a name collision — both `plugins::plugins_dir` and `paths::plugins_dir` exist. The replacement is only valid if the fully-qualified `crate::config::paths::plugins_dir()` unambiguously resolves. In Rust this works; compiler will resolve via full path. Verify build.

- [ ] **Step 3: `src/observability/sink.rs`**

Find the runs_dir construction around line 120-125:

```rust
        let global_dir = crate::config::settings::global_claude_dir()
            .unwrap_or_else(|_| PathBuf::from(".cc-rust"));
        let runs_dir = global_dir.join("runs").join(session_id);
```

Replace with:

```rust
        let runs_dir = crate::config::paths::runs_dir(session_id);
```

- [ ] **Step 4: Build**

Run: `cargo build --all-targets`
Expected: no errors, no new warnings.

- [ ] **Step 5: Run affected tests**

Run: `cargo test --lib session::memdir plugins observability::sink`
Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add src/session/memdir.rs src/plugins/mod.rs src/observability/sink.rs
git commit -m "refactor: drop dead global_claude_dir fallbacks now that resolution is infallible"
```

---

## Task 17: Subprocess-based integration smoke test

**Goal:** End-to-end assertion that running the actual binary with `CC_RUST_HOME` set redirects data there and keeps cwd clean.

**Why not a `cargo test --test` lib-import test:** `claude-code-rs` is a **binary-only** crate (no `src/lib.rs`); integration tests in `tests/` cannot `use claude_code_rs::config::paths`. Rather than introduce a lib/bin split (out of scope for this PR), we verify the property by spawning the compiled binary and inspecting side effects.

**Files:**
- Create: `tests/cc_rust_home_subprocess.rs`

- [ ] **Step 1: Confirm the binary exists**

Run: `cargo build --release 2>&1 | tail -3`
Expected: produces `./target/release/claude-code-rs` (or `.exe` on Windows).

- [ ] **Step 2: Write the test**

Create `tests/cc_rust_home_subprocess.rs`:

```rust
//! Subprocess-level integration smoke for Phase 1 storage unification.
//!
//! Verifies that running the actual binary with `CC_RUST_HOME=<tempdir>`
//! causes runtime state (logs, sessions) to appear under that tempdir
//! and NOT inside the test's working directory.

use serial_test::serial;
use std::process::Command;
use tempfile::TempDir;

fn binary_path() -> std::path::PathBuf {
    // Built by `cargo build` before `cargo test` runs; CARGO_BIN_EXE_ is a
    // Cargo-provided env var for bin targets referenced from integration tests.
    // See: https://doc.rust-lang.org/cargo/reference/environment-variables.html
    let p = env!("CARGO_BIN_EXE_claude-code-rs");
    std::path::PathBuf::from(p)
}

#[test]
#[serial]
fn cc_rust_home_redirects_logs_and_leaves_cwd_clean() {
    let tmp = TempDir::new().expect("tempdir");
    let cwd = TempDir::new().expect("cwd tempdir");

    // Run `--version` as the minimum invocation that still initializes
    // tracing / logs directory. If `--version` is a pre-init fast path,
    // swap to a lightweight command that goes through main init.
    let status = Command::new(binary_path())
        .arg("--version")
        .env("CC_RUST_HOME", tmp.path())
        .current_dir(cwd.path())
        .status()
        .expect("spawn cc-rust");
    assert!(status.success(), "binary exited non-zero");

    // Assert no `.logs/` or `logs/` appeared in the test cwd.
    let cwd_logs_hidden = cwd.path().join(".logs");
    let cwd_logs_visible = cwd.path().join("logs");
    assert!(
        !cwd_logs_hidden.exists(),
        "cwd polluted: {} exists",
        cwd_logs_hidden.display()
    );
    assert!(
        !cwd_logs_visible.exists(),
        "cwd polluted: {} exists",
        cwd_logs_visible.display()
    );
}
```

**Caveat on `--version`:** The current `src/main.rs` (lines 242-246) has a fast path for `--version` that returns *before* tracing init. That means this test only confirms "fast path does not pollute"; it does NOT exercise the tracing init codepath changed in Task 5. Options if stronger coverage is desired:

1. Accept the limited coverage — the manual acceptance steps in Task 20 cover the main init path, and the unit tests in Task 2/3 prove the paths resolution itself.
2. Add a non-trivial invocation (e.g. headless with stdin closed so it exits quickly) — more robust but depends on headless protocol details.

For this plan, use option 1 (limited but honest coverage). If the reviewer wants stronger coverage, follow-up PR can add a dedicated `--check-paths` diagnostic subcommand.

- [ ] **Step 3: Run the test**

Run: `cargo test --test cc_rust_home_subprocess`
Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add tests/cc_rust_home_subprocess.rs
git commit -m "test: subprocess smoke verifying CC_RUST_HOME redirects and cwd stays clean"
```

---

## Task 18: Write `docs/STORAGE.md`

**Files:**
- Create: `docs/STORAGE.md`

- [ ] **Step 1: Write the file**

Create `docs/STORAGE.md`:

```markdown
# cc-rust Storage Paths

All runtime persistence for cc-rust lives under a single **data root**. This
document describes exactly where each kind of data is written.

## Data root resolution

The data root is resolved in this order:

1. **`CC_RUST_HOME`** environment variable — if set and non-empty (after trim),
   used as-is. Leading/trailing whitespace in the value is preserved.
2. **`~/.cc-rust/`** — the platform's home directory with `.cc-rust/` appended.
   On Unix: via `$HOME` or `getpwuid_r`; on Windows: via `SHGetKnownFolderPath`.
3. **`$TMP/cc-rust/`** — `std::env::temp_dir()`, used only when the home
   directory cannot be resolved. cc-rust logs a one-time warning when this
   happens. Data in this location is **not persistent** across reboots on
   most systems.

## Global paths (under data root)

Let `$ROOT` denote the resolved data root.

| Path | Purpose |
|------|---------|
| `$ROOT/settings.json` | Global settings (merged with per-project `.cc-rust/settings.json`). |
| `$ROOT/sessions/` | Session JSON files, one per session id. |
| `$ROOT/logs/` | Process-level tracing logs (daily-rolling `cc-rust.log.YYYY-MM-DD`). |
| `$ROOT/logs/YYYY/MM/YYYY-MM-DD.md` | KAIROS daemon's daily markdown log. |
| `$ROOT/credentials.json` | OAuth tokens (sensitive). |
| `$ROOT/runs/{session_id}/events.ndjson` | Audit sink event stream per session. |
| `$ROOT/runs/{session_id}/meta.json` | Audit sink metadata per session. |
| `$ROOT/runs/{session_id}/subagent-events.ndjson` | Subagent dashboard event log. |
| `$ROOT/runs/{session_id}/artifacts/` | Audit-sink artifact storage. |
| `$ROOT/exports/` | Markdown session exports. |
| `$ROOT/audits/` | JSON audit record files. |
| `$ROOT/transcripts/` | NDJSON session transcripts. |
| `$ROOT/memory/` | Global memory entries. |
| `$ROOT/session-insights/` | Session insight extracts. |
| `$ROOT/plugins/` | Installed plugin metadata + marketplace cache. |
| `$ROOT/skills/` | User-installed skills. |
| `$ROOT/projects/{sanitized_cwd}/memory/team/` | Per-workspace Team Memory sync mirror. |

## Project-local paths

These live **inside your project directory** (not under `$ROOT`) and behave like
`.git/config` — per-repo overrides and artifacts:

| Path | Purpose |
|------|---------|
| `{cwd}/.cc-rust/settings.json` | Project-level settings. Loaded in addition to global settings. |
| `{cwd}/.cc-rust/memory/` | Project-scoped memory. |
| `{cwd}/.cc-rust/skills/` | Project-scoped skills. |

These are discovered by ancestor-walk from the current working directory.

## Platform notes

- **Linux / macOS:** `dirs::home_dir()` uses `$HOME` first, falling back to
  `getpwuid_r`. `std::env::temp_dir()` is typically `/tmp` on Linux, `/var/folders/...` on macOS.
- **Windows:** `dirs::home_dir()` uses `SHGetKnownFolderPath(FOLDERID_Profile)`;
  it does **not** honor `HOME` / `USERPROFILE` as overrides (though those vars
  influence `SHGetKnownFolderPath` internally). `std::env::temp_dir()` is
  typically `%TEMP%` (e.g. `C:\Users\<you>\AppData\Local\Temp`).
- **`CC_RUST_HOME` is always honored** regardless of platform and regardless of
  whether `dirs::home_dir()` would succeed.

## Migrating from older layouts

If you have data from a pre-Phase-1 cc-rust installation in unexpected places:

- **Repo-local `.logs/` or `logs/`**: no longer written. Safe to delete
  (but check contents first if you depended on them).
- **Repo-local `.cc-rust/sessions/`**: if this exists inside a project dir,
  it's a leftover — move its contents to `~/.cc-rust/sessions/`:

  ```bash
  mv ./.cc-rust/sessions/* ~/.cc-rust/sessions/
  rmdir ./.cc-rust/sessions
  ```

- **Old dashboard event log at `{cwd}/.logs/subagent-events.ndjson`**: migrated
  to per-session `$ROOT/runs/{session_id}/subagent-events.ndjson`. The old file
  can be deleted or archived.

cc-rust does **not** perform automatic migration. Handle old data manually.

## Testing overrides

For integration tests or ephemeral runs, set `CC_RUST_HOME` to an isolated
directory:

```bash
CC_RUST_HOME=/tmp/cc-rust-test ./target/release/cc-rust --headless
```

All persistence will land in `/tmp/cc-rust-test/`, with nothing written to the
current working directory or your real home.
```

- [ ] **Step 2: Lint the file (optional markdown check)**

If `prettier` or similar is configured, run:

Run: `(cd .. && npx prettier --check rust/docs/STORAGE.md 2>/dev/null) || true`
(If prettier isn't set up, skip.)

- [ ] **Step 3: Commit**

```bash
git add docs/STORAGE.md
git commit -m "docs: add STORAGE.md — canonical runtime path reference"
```

---

## Task 19: Update `README.md` and `CLAUDE.md`

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Find the right spot in `README.md`**

Run: `grep -n -i "storage\|path\|\.cc-rust\|CC_RUST_HOME" README.md | head -10`

Look for an existing "Configuration" / "Installation" / "Getting Started" section. If none, add a new subsection near the top.

- [ ] **Step 2: Add a one-liner pointer in `README.md`**

In the most relevant section (e.g. a "Configuration" heading), insert:

```markdown
### Storage paths

cc-rust writes all runtime data (sessions, logs, credentials, ...) under a
single data root — `$CC_RUST_HOME` if set, otherwise `~/.cc-rust/`. See
[docs/STORAGE.md](docs/STORAGE.md) for the complete layout.
```

If no natural location exists, append it as a new section at the end under `## Configuration` or similar.

- [ ] **Step 3: Update `CLAUDE.md` "Path Isolation" section**

Open `CLAUDE.md`. Find the "Path Isolation" heading (it already exists with a table comparing cc-rust vs Claude Code paths). Below the existing table, add:

```markdown
### `CC_RUST_HOME` override

To place all runtime data somewhere other than `~/.cc-rust/`, set the
`CC_RUST_HOME` environment variable. See [docs/STORAGE.md](docs/STORAGE.md)
for the canonical path reference and fallback behavior.
```

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: reference STORAGE.md from README and CLAUDE.md"
```

---

## Task 20: Final acceptance — build, tests, manual smoke

**Files:** none modified — verification only.

- [ ] **Step 1: Full build with warning gate**

Run: `cargo build --release --all-targets 2>&1 | tee /tmp/phase1-build.log`

Inspect for any **new** warnings introduced by this PR:

Run: `grep -E "^(warning|error)" /tmp/phase1-build.log`

Expected: no `error` lines. If warnings appear that weren't in baseline, fix them before final commit (per CLAUDE.md: "每次写完代码，编译过后查看有没有 warning，解决 warning").

- [ ] **Step 2: Run the entire unit test suite**

Run: `cargo test --lib --all-features 2>&1 | tail -20`
Expected: all pass.

- [ ] **Step 3: Run the integration tests**

Run: `cargo test --test '*' 2>&1 | tail -20`
Expected: all pass, including the new `cc_rust_home_subprocess`.

- [ ] **Step 4: Manual smoke — default `CC_RUST_HOME` path**

Before running: ensure `~/.cc-rust/` is in a known state (note current contents) and the worktree cwd is clean (`git status --porcelain` empty).

Run: `./target/release/cc-rust --version && ./target/release/cc-rust --headless < /dev/null > /dev/null 2>&1 &`
After exit: check

```bash
ls ~/.cc-rust/logs/ 2>/dev/null | head -5
ls . | grep -E "^(\.logs|logs)$" && echo "FAIL: repo has logs/" || echo "OK: no repo-level logs added"
```

Expected: `OK: no repo-level logs added` (pre-existing `logs/.gitkeep` is expected, but no new `.logs/` directory).

- [ ] **Step 5: Manual smoke — `CC_RUST_HOME` override**

```bash
CC_RUST_HOME=/tmp/cc-rust-final-smoke ./target/release/cc-rust --headless < /dev/null > /dev/null 2>&1 &
ls /tmp/cc-rust-final-smoke/
```

Expected: directory exists with `logs/` (and possibly `sessions/`, etc.) subdirectories.

- [ ] **Step 6: Manual smoke — dashboard event log (if FEATURE_SUBAGENT_DASHBOARD is expected to work)**

Optional; skip if `bun` is not installed locally:

```bash
FEATURE_SUBAGENT_DASHBOARD=1 CC_RUST_HOME=/tmp/cc-rust-dash-final ./target/release/cc-rust --headless < /dev/null > /dev/null 2>&1 &
# (send one ping via IPC and exit — mechanics depend on headless protocol)
find /tmp/cc-rust-dash-final/runs -name 'subagent-events.ndjson' 2>/dev/null
```

Expected: if any subagent ran, an `.ndjson` file exists under `runs/{session_id}/`.

- [ ] **Step 7: Push & open PR**

Push the branch and open a PR against `rust-lite`:

```bash
git push -u origin claude/relaxed-moser-134b1a
gh pr create --base rust-lite --title "Phase 1: Unify runtime storage under CC_RUST_HOME" --body "$(cat <<'EOF'
## Summary

Implements Phase 1 of [GitHub issue #1](https://github.com/crsei/cc-rust/issues/1) — Web UI overhaul.

- New `src/config/paths.rs` module centralizes all runtime path resolution.
- ~9 call sites migrated from hardcoded `dirs::home_dir().join(".cc-rust")` to `paths::` functions.
- Dashboard subagent events now write to `\$ROOT/runs/{session_id}/subagent-events.ndjson` (was `{cwd}/.logs/`).
- `main.rs` no longer falls back to repo-local `.logs/` when home isn't writable; uses `std::env::temp_dir()` fallback instead.
- Spec: [development/archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md](../specs/2026-04-18-phase1-runtime-storage-unification-design.md).
- User-facing doc: [docs/STORAGE.md](docs/STORAGE.md).

## Test plan

- [x] `cargo test --lib` passes
- [x] `cargo test --test cc_rust_home_subprocess` passes (subprocess smoke)
- [x] `cargo build --release` — no new warnings
- [x] Manual: `./cc-rust --headless` creates no repo-level `.logs/`
- [x] Manual: `CC_RUST_HOME=/tmp/x ./cc-rust --headless` writes all data to `/tmp/x/`
- [ ] Reviewer: confirm behavior on Windows (path separators, temp fallback)

## Out of scope (future PRs)

- Phase 2: Session API + project/workspace grouping.
- Phase 3-5 of issue #1.
- Automatic migration of legacy data.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

**Do not push without explicit user approval** — stop here and report results to the user first.

- [ ] **Step 8: Report**

Tell the user:
- All tasks completed
- Link to the built release binary (if applicable)
- Summary of manual smoke results
- Any residual warnings or known-skipped tests (e.g. temp fallback test that prints "skipping")

---

## Self-Review Notes

**Spec coverage check:** every acceptance criterion in spec §11 maps to a task:
- `src/config/paths.rs` exists + tests → Task 2, 3
- §4 call sites migrated → Tasks 5–15
- Dashboard subagent path changed → Task 15
- `.logs/` fallback removed → Task 5
- `data_root()` temp fallback + warn → Task 2
- Integration smoke (subprocess) → Task 17
- `docs/STORAGE.md` + README + CLAUDE.md → Tasks 18, 19
- `cargo build --release` no warnings → Task 20 step 1
- Manual verification → Task 20 steps 4–6

**Type consistency:** `data_root() -> PathBuf` (never fails). All partition helpers return `PathBuf`. `event_log_path() -> Option<PathBuf>` (may be `None` before `init_session_id`). `init_session_id(&str) -> ()`. `global_claude_dir() -> Result<PathBuf>` preserved for source compatibility via a `Ok(...)` wrapper.

**Known pragmas:**
1. `claude-code-rs` is a binary-only crate (no `src/lib.rs`); Task 17's integration test is therefore subprocess-based rather than library-linked. Stronger end-to-end coverage is deferred to either a future lib/bin split or a dedicated `--check-paths` diagnostic subcommand.
2. Task 15a's `event_log_path()` test can't reset `OnceLock`; it tests post-init state only. The `None` branch is covered by the type system (callers must handle) and by the `debug!` log in `append_event`.
3. `dirs::home_dir()` on Windows ignores env vars; the `data_root_temp_fallback_best_effort` test prints "skipping" rather than failing when host OS can resolve home. True temp-fallback verification is manual (Task 20 step 6 variant).
