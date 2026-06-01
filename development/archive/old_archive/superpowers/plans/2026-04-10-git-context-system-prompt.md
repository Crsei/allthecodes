# Git Context Injection into System Prompt — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inject git repository context (branch, user, status, recent commits) into the system prompt so the LLM has repo awareness at conversation start.

**Architecture:** Add a `git_status_section(cwd)` function to `system_prompt.rs` that calls existing `utils/git.rs` helpers, formats the output in porcelain style, and registers as a cached dynamic section after `env_info_simple`.

**Tech Stack:** `git2` crate (already a dependency), existing `utils/git.rs` helpers.

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/engine/system_prompt.rs` | Add `git_status_section()` + register in dynamic sections |

No new files needed. All git helpers already exist in `src/utils/git.rs`.

---

### Task 1: Add `git_status_section()` function and wire it into the prompt

**Files:**
- Modify: `src/engine/system_prompt.rs:192-322` (add function + register section)

- [ ] **Step 1: Write tests for git_status_section**

Add these tests at the bottom of the existing `#[cfg(test)] mod tests` block in `system_prompt.rs`:

```rust
#[test]
fn test_git_status_section_in_git_repo() {
    // Use the actual cc-rust repo as test input
    let cwd = env!("CARGO_MANIFEST_DIR");
    let result = git_status_section(cwd);
    assert!(result.is_some(), "should produce output in a git repo");
    let text = result.unwrap();
    assert!(text.contains("gitStatus:"));
    assert!(text.contains("Current branch:"));
    assert!(text.contains("Recent commits:"));
}

#[test]
fn test_git_status_section_not_git_repo() {
    let dir = std::env::temp_dir().join(format!("no_git_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let result = git_status_section(dir.to_str().unwrap());
    assert!(result.is_none(), "should return None for non-git dir");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_git_status_section_contains_main_branch() {
    let cwd = env!("CARGO_MANIFEST_DIR");
    let result = git_status_section(cwd);
    if let Some(text) = result {
        assert!(
            text.contains("Main branch"),
            "should contain main branch info"
        );
    }
}

#[test]
fn test_git_status_section_limits_commits() {
    let cwd = env!("CARGO_MANIFEST_DIR");
    if let Some(text) = git_status_section(cwd) {
        let commit_lines: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.contains("Recent commits:"))
            .skip(1)
            .filter(|l| !l.is_empty())
            .collect();
        assert!(
            commit_lines.len() <= 10,
            "should have at most 10 commit lines, got {}",
            commit_lines.len()
        );
    }
}

#[test]
fn test_build_system_prompt_includes_git_status() {
    let cwd = env!("CARGO_MANIFEST_DIR");
    let (parts, _, _) =
        build_system_prompt(None, None, &[], "claude-sonnet-4-20250514", cwd);
    let joined = parts.join("\n");
    assert!(
        joined.contains("gitStatus:"),
        "system prompt should include git status section"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib engine::system_prompt::tests -- --nocapture 2>&1 | tail -20`

Expected: FAIL — `git_status_section` not found.

- [ ] **Step 3: Implement `git_status_section()`**

Add this function in `system_prompt.rs` after `env_info_section()` (after line 239, before `language_section()`):

```rust
/// Build a git status snapshot for the system prompt.
///
/// Returns `None` if the directory is not a git repo or if any git
/// operation fails (fail-open: never block prompt construction).
///
/// Corresponds to TS: `gitStatus` section in system prompt.
fn git_status_section(cwd: &str) -> Option<String> {
    use crate::utils::git;

    let path = Path::new(cwd);
    if !git::is_git_repo(path) {
        return None;
    }

    let branch = git::current_branch(path).ok()?;
    let default_br = git::default_branch(path).unwrap_or_else(|_| "main".into());

    // Git user name via git2 config
    let git_user = git::open_repo(path)
        .ok()
        .and_then(|repo| repo.config().ok())
        .and_then(|cfg| cfg.get_string("user.name").ok())
        .unwrap_or_default();

    // Status (porcelain-style, capped at 20 files)
    let status_text = match git::get_status(path) {
        Ok(status) => {
            let mut lines = Vec::new();
            for f in &status.staged {
                let prefix = match f.status {
                    git::FileStatusKind::Deleted => "D ",
                    git::FileStatusKind::Renamed => "R ",
                    git::FileStatusKind::StagedAndModified => "MM",
                    _ => "M ",
                };
                lines.push(format!("{} {}", prefix, f.path));
            }
            for f in &status.unstaged {
                let prefix = match f.status {
                    git::FileStatusKind::Deleted => " D",
                    git::FileStatusKind::Renamed => " R",
                    _ => " M",
                };
                lines.push(format!("{} {}", prefix, f.path));
            }
            for f in &status.untracked {
                lines.push(format!("?? {}", f.path));
            }
            if lines.is_empty() {
                String::new()
            } else {
                let total = lines.len();
                let mut out: Vec<String> = lines.into_iter().take(20).collect();
                if total > 20 {
                    out.push(format!("... and {} more files", total - 20));
                }
                format!("\nStatus:\n{}", out.join("\n"))
            }
        }
        Err(_) => String::new(),
    };

    // Recent commits (up to 10)
    let commits_text = match git::get_log(path, 10) {
        Ok(log) if !log.is_empty() => {
            let lines: Vec<String> = log
                .iter()
                .map(|e| format!("{} {}", e.short_sha, e.summary))
                .collect();
            format!("\nRecent commits:\n{}", lines.join("\n"))
        }
        _ => String::new(),
    };

    Some(format!(
        "gitStatus: This is the git status at the start of the conversation. \
         Note that this status is a snapshot in time, and will not update during the conversation.\n\
         \n\
         Current branch: {branch}\n\
         \n\
         Main branch (you will usually use this for PRs): {default_br}\n\
         \n\
         Git user: {git_user}\
         {status_text}\
         {commits_text}",
    ))
}
```

- [ ] **Step 4: Register `git_status_section` as a dynamic section**

In `build_system_prompt()`, modify the `dynamic_sections` vec (around line 310) to add the git status section after `env_info_simple`. Change from:

```rust
        let dynamic_sections = vec![
            cached_section("env_info_simple", move || {
                Some(env_info_section(&model_owned, &cwd_owned))
            }),
            cached_section("summarize_tool_results", || {
```

To:

```rust
        let cwd_for_git = cwd.to_string();
        let dynamic_sections = vec![
            cached_section("env_info_simple", move || {
                Some(env_info_section(&model_owned, &cwd_owned))
            }),
            cached_section("git_status", move || {
                git_status_section(&cwd_for_git)
            }),
            cached_section("summarize_tool_results", || {
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib engine::system_prompt::tests -- --nocapture`

Expected: All tests PASS including the 5 new ones.

- [ ] **Step 6: Build release and check for warnings**

Run: `cargo build --release 2>&1 | grep warning`

Expected: No new warnings from `system_prompt.rs`. Fix any that appear.

- [ ] **Step 7: Commit**

```bash
git add src/engine/system_prompt.rs
git commit -m "feat: inject git context (branch, user, status, commits) into system prompt

Adds git_status_section() that calls utils/git.rs helpers to build a
porcelain-style snapshot. Registered as cached dynamic section after
env_info_simple. Limits: 20 status files, 10 recent commits.
Fail-open: returns None for non-git dirs or on any git error."
```

### Task 2: Update SDK work tracker

**Files:**
- Modify: `docs/sdk-work-tracker.md:31`

- [ ] **Step 1: Update P1-1 status to completed**

Change line 31 from:

```markdown
| P1-1 | Git 上下文自动注入 system prompt | ❌ | — | `utils/git.rs` 有 branch/sha/status，需注入 system_prompt |
```

To:

```markdown
| P1-1 | Git 上下文自动注入 system prompt | ✅ | — | `system_prompt.rs` git_status_section(), porcelain-style snapshot |
```

Also update P1-4 (LSP) from ⚠️ to ✅ since it was found to be already completed:

Change line 34 from:

```markdown
| P1-4 | LSP 方法实现 | ⚠️ | — | 框架存在，6 个方法全部 `unimplemented!()` |
```

To:

```markdown
| P1-4 | LSP 方法实现 | ✅ | — | 9/9 方法已通过 LspClient 完整实现 |
```

- [ ] **Step 2: Commit tracker update**

```bash
git add docs/sdk-work-tracker.md
git commit -m "docs: update P1-1 (git context) and P1-4 (LSP) status to completed"
```

### Task 3: Remove unused variable warning in env_info_section

**Files:**
- Modify: `src/engine/system_prompt.rs:208,210`

The `env_info_section()` function has two unused variables (`os_version` on line 208, `date` on line 210) that were never wired into the output format. Clean them up.

- [ ] **Step 1: Remove the two unused lines**

Delete these lines from `env_info_section()`:

```rust
    let os_version = std::env::consts::OS;

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
```

- [ ] **Step 2: Build and verify no warnings**

Run: `cargo build --release 2>&1 | grep warning`

Expected: Zero warnings from system_prompt.rs.

- [ ] **Step 3: Commit**

```bash
git add src/engine/system_prompt.rs
git commit -m "fix: remove unused os_version and date variables from env_info_section"
```
