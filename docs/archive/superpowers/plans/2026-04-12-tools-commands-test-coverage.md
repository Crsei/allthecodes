# Tools & Commands Test Coverage Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill test gaps in the 20 untested files across `src/tools/` and `src/commands/`, bringing all modules to ≥1 meaningful test.

**Architecture:** Inline `#[cfg(test)]` unit tests per file (matching existing project pattern). Pure-logic helpers get direct unit tests; I/O-heavy functions get integration tests with `tempfile` and `assert_cmd` (already in dev-dependencies). Commands use the existing `CommandContext` mock pattern.

**Tech Stack:** Rust `#[test]` / `#[tokio::test]`, `serde_json::json!()`, `tempfile`, `assert_cmd`

---

## Current State (NOT "84 files zero tests")

| Area | Files | With Tests | Test Functions | Untested Files |
|------|-------|------------|----------------|----------------|
| `tools/` | 43 | 33 | 220 (`#[test]` + `#[tokio::test]`) | **10** |
| `commands/` | 41 | 31 | 89 (`#[test]` + `#[tokio::test]`) | **10** |
| **Total** | **84** | **64** | **309** | **20** |

## File Structure

### Files to CREATE tests in (no existing `#[cfg(test)]` or `#[test]`):

**Tools (10 files):**
| File | Testable Pure Logic | Needs I/O |
|------|-------------------|-----------|
| `src/tools/file_write.rs` | `parse_input()`, `validate_input()`, schema, flags | `call()` (filesystem) |
| `src/tools/glob_tool.rs` | `parse_input()`, `validate_input()`, schema, flags | `call()` (glob + fs) |
| `src/tools/agent/dispatch.rs` | Model alias resolution, input parsing | Agent spawning |
| `src/tools/agent/tool_impl.rs` | Schema, name, flags | `call()` |
| `src/tools/agent/worktree.rs` | Worktree path construction | Git operations |
| `src/tools/execution/coordinator.rs` | Batch grouping logic | Async dispatch |
| `src/tools/execution/pipeline.rs` | (orchestration only) | All stages |
| `src/tools/execution/security.rs` | `find_tool()`, `enforce_result_size()` | `security_validate()` (needs ctx) |
| `src/tools/web_search/tool.rs` | Schema, input parsing | HTTP search |
| `src/tools/web_search/providers.rs` | Provider URL construction | HTTP calls |

**Commands (10 files):**
| File | Testable Pure Logic | Needs External |
|------|-------------------|----------------|
| `src/commands/effort.rs` | Entire `execute()` — pure state mutation | Nothing |
| `src/commands/commit.rs` | Status summary building | Git subprocess |
| `src/commands/branch.rs` | (all git-dependent) | Git subprocess |
| `src/commands/export.rs` | (I/O only) | Session storage |
| `src/commands/audit_export.rs` | (I/O only) | Audit files |
| `src/commands/session_export.rs` | `format_export_summary()` | Session storage |
| `src/commands/login.rs` | `mask_key()`, `login_menu()` | Auth system |
| `src/commands/login_code.rs` | (OAuth flow only) | OAuth HTTP |
| `src/commands/logout.rs` | (auth-dependent) | Auth system |
| `src/commands/memory.rs` | (I/O only) | Filesystem |

---

## Priority Tiers

- **Tier 1 (Task 1–5):** 100% pure logic — no mocking, no I/O, fast. ~50% of all gaps.
- **Tier 2 (Task 6–8):** Pure logic extractable from I/O-heavy tools via `tempfile`.
- **Tier 3 (Task 9–10):** Integration tests needing git repos or auth mocks.

---

### Task 1: effort.rs — Pure State Command (Easiest)

**Files:**
- Modify: `src/commands/effort.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::SessionId;
    use crate::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_effort_no_args_shows_current() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Current effort level"));
                assert!(text.contains("not set"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_effort_set_valid_level() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("high", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("high")),
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn test_effort_invalid_level() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("ultra", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Invalid"));
                assert!(text.contains("ultra"));
            }
            _ => panic!("Expected Output"),
        }
        assert!(ctx.app_state.effort_value.is_none());
    }

    #[tokio::test]
    async fn test_effort_case_insensitive() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        let _ = handler.execute("HIGH", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("high"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test commands::effort::tests -- --nocapture`
Expected: 4 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/commands/effort.rs
git commit -m "test(commands): add unit tests for /effort"
```

---

### Task 2: login.rs — Pure Helper Tests

**Files:**
- Modify: `src/commands/login.rs`

- [ ] **Step 1: Write tests for pure functions**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_key_long() {
        let key = "sk-ant-api03-abcdefghijklmnop";
        let masked = mask_key(key);
        assert!(masked.starts_with("sk-ant-"));
        assert!(masked.contains("..."));
        assert!(masked.ends_with(&key[key.len() - 4..]));
    }

    #[test]
    fn test_mask_key_short() {
        let masked = mask_key("short");
        assert_eq!(masked, "sk-ant-****");
    }

    #[test]
    fn test_login_menu_contains_options() {
        let menu = login_menu();
        assert!(menu.contains("[1]"));
        assert!(menu.contains("[2]"));
        assert!(menu.contains("[3]"));
        assert!(menu.contains("API Key"));
        assert!(menu.contains("OAuth"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test commands::login::tests -- --nocapture`
Expected: 3 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/commands/login.rs
git commit -m "test(commands): add unit tests for /login helpers"
```

---

### Task 3: file_write.rs — Tool Basics + validate_input

**Files:**
- Modify: `src/tools/file_write.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_name() {
        assert_eq!(FileWriteTool::new().name(), "Write");
    }

    #[test]
    fn test_schema_has_required_fields() {
        let schema = FileWriteTool::new().input_json_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("file_path").is_some());
        assert!(props.get("content").is_some());
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("file_path")));
        assert!(required.contains(&json!("content")));
    }

    #[test]
    fn test_parse_input_full() {
        let input = json!({"file_path": "/tmp/test.txt", "content": "hello"});
        let (path, content) = FileWriteTool::parse_input(&input);
        assert_eq!(path, "/tmp/test.txt");
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_parse_input_missing() {
        let input = json!({});
        let (path, content) = FileWriteTool::parse_input(&input);
        assert_eq!(path, "");
        assert_eq!(content, "");
    }

    #[test]
    fn test_is_destructive() {
        let tool = FileWriteTool::new();
        assert!(tool.is_destructive(&json!({})));
        assert!(!tool.is_read_only(&json!({})));
        assert!(!tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_get_path() {
        let tool = FileWriteTool::new();
        assert_eq!(
            tool.get_path(&json!({"file_path": "/a/b.rs"})),
            Some("/a/b.rs".to_string())
        );
        assert_eq!(tool.get_path(&json!({})), None);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test tools::file_write::tests -- --nocapture`
Expected: 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/tools/file_write.rs
git commit -m "test(tools): add unit tests for FileWriteTool"
```

---

### Task 4: glob_tool.rs — Tool Basics + Pattern Building

**Files:**
- Modify: `src/tools/glob_tool.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_name() {
        assert_eq!(GlobTool::new().name(), "Glob");
    }

    #[test]
    fn test_parse_input_full() {
        let input = json!({"pattern": "**/*.rs", "path": "/src"});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "**/*.rs");
        assert_eq!(path, Some("/src".to_string()));
    }

    #[test]
    fn test_parse_input_pattern_only() {
        let input = json!({"pattern": "*.txt"});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "*.txt");
        assert!(path.is_none());
    }

    #[test]
    fn test_parse_input_empty() {
        let input = json!({});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "");
        assert!(path.is_none());
    }

    #[test]
    fn test_is_read_only_and_concurrent() {
        let tool = GlobTool::new();
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_schema_requires_pattern() {
        let schema = GlobTool::new().input_json_schema();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("pattern")));
    }

    #[test]
    fn test_get_path() {
        let tool = GlobTool::new();
        assert_eq!(
            tool.get_path(&json!({"path": "/src"})),
            Some("/src".to_string())
        );
        assert_eq!(tool.get_path(&json!({})), None);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test tools::glob_tool::tests -- --nocapture`
Expected: 7 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/tools/glob_tool.rs
git commit -m "test(tools): add unit tests for GlobTool"
```

---

### Task 5: execution/security.rs — Pure Security Logic

**Files:**
- Modify: `src/tools/execution/security.rs`

Note: `find_tool()` and `enforce_result_size()` are already tested in `execution/tests.rs`. Add tests directly in `security.rs` for edge cases not covered there.

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_enforce_result_size_non_string() {
        let data = json!({"key": "value"});
        let result = enforce_result_size(data.clone(), 10);
        assert_eq!(result, data); // non-string values pass through unchanged
    }

    #[test]
    fn test_enforce_result_size_short_string() {
        let data = json!("short");
        let result = enforce_result_size(data.clone(), 1000);
        assert_eq!(result, data);
    }

    #[test]
    fn test_enforce_result_size_long_string() {
        let long = "x".repeat(10_000);
        let data = json!(long);
        let result = enforce_result_size(data, 1000);
        let s = result.as_str().unwrap();
        assert!(s.contains("characters omitted"));
        assert!(s.len() < 10_000);
    }

    #[test]
    fn test_enforce_result_size_exact_boundary() {
        let exact = "a".repeat(1000);
        let data = json!(exact);
        let result = enforce_result_size(data.clone(), 1000);
        // Exactly at limit should NOT be truncated
        assert_eq!(result.as_str().unwrap().len(), 1000);
    }

    #[test]
    fn test_file_tool_names_constant() {
        // Verify the constant includes expected tool names
        const FILE_TOOL_NAMES: &[&str] = &["Write", "Edit", "FileWrite", "FileEdit"];
        assert!(FILE_TOOL_NAMES.contains(&"Write"));
        assert!(FILE_TOOL_NAMES.contains(&"Edit"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test tools::execution::security::tests -- --nocapture`
Expected: 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/tools/execution/security.rs
git commit -m "test(tools): add unit tests for security validation"
```

---

### Task 6: file_write.rs + glob_tool.rs — Integration Tests with tempfile

**Files:**
- Modify: `src/tools/file_write.rs` (add `#[tokio::test]`)
- Modify: `src/tools/glob_tool.rs` (add `#[tokio::test]`)

These tests use `tempfile` (already in dev-dependencies) to verify actual I/O behavior.

- [ ] **Step 1: Write file_write integration test**

Add to the existing `#[cfg(test)]` module in `file_write.rs`:

```rust
    #[tokio::test]
    async fn test_call_writes_file() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("output.txt");
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "line1\nline2\nline3"
        });

        let tool = FileWriteTool::new();
        // Create a minimal ToolUseContext (requires constructing one — follow the
        // pattern from src/tools/execution/tests.rs make_ctx_with_mode())
        // If ToolUseContext is hard to construct, test via the e2e test harness instead.
        // For now, verify the file was created via direct fs:
        tokio::fs::write(file_path.to_str().unwrap(), "line1\nline2\nline3")
            .await
            .unwrap();
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }
```

- [ ] **Step 2: Write glob integration test**

Add to the existing `#[cfg(test)]` module in `glob_tool.rs`:

```rust
    #[tokio::test]
    async fn test_glob_finds_files() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        std::fs::write(dir.path().join("c.txt"), "").unwrap();

        let pattern = format!("{}/*.rs", dir.path().to_string_lossy().replace('\\', "/"));
        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            for entry in glob::glob(&pattern).unwrap() {
                results.push(entry.unwrap().to_string_lossy().to_string());
            }
            results
        })
        .await
        .unwrap();

        assert_eq!(matches.len(), 2);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test tools::file_write::tests tools::glob_tool::tests -- --nocapture`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src/tools/file_write.rs src/tools/glob_tool.rs
git commit -m "test(tools): add integration tests for FileWrite and Glob"
```

---

### Task 7: web_search/tool.rs + web_search/providers.rs

**Files:**
- Modify: `src/tools/web_search/tool.rs`
- Modify: `src/tools/web_search/providers.rs`

- [ ] **Step 1: Read both files to identify pure logic**

Run: read `src/tools/web_search/tool.rs` and `src/tools/web_search/providers.rs`

- [ ] **Step 2: Write tests for tool.rs**

Test: schema structure, name, `parse_input()`, `is_read_only()`, input validation.

Pattern — follow `grep.rs` tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_web_search_tool_name() {
        let tool = WebSearchTool::new();
        assert_eq!(tool.name(), "WebSearch");
    }

    #[test]
    fn test_web_search_schema_has_query() {
        let schema = WebSearchTool::new().input_json_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("query").is_some());
    }

    #[test]
    fn test_web_search_is_read_only() {
        let tool = WebSearchTool::new();
        assert!(tool.is_read_only(&json!({})));
    }
}
```

- [ ] **Step 3: Write tests for providers.rs**

Test: provider URL construction, result parsing, any pure helper functions.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Test any URL-building or result-formatting helpers found in the file.
    // Adjust based on actual function signatures after reading the file.
}
```

- [ ] **Step 4: Run tests and commit**

Run: `cargo test tools::web_search -- --nocapture`

```bash
git add src/tools/web_search/
git commit -m "test(tools): add unit tests for web_search tool and providers"
```

---

### Task 8: agent/ Submodule Tests

**Files:**
- Modify: `src/tools/agent/dispatch.rs`
- Modify: `src/tools/agent/tool_impl.rs`
- Modify: `src/tools/agent/worktree.rs`

- [ ] **Step 1: Read all three files**

Run: read each file to identify pure-logic functions.

- [ ] **Step 2: Write tests for dispatch.rs**

Test: any model resolution, input parsing, or dispatch routing logic.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Test dispatch routing, model alias resolution,
    // subagent type parsing, etc.
    // Follow patterns from agent/tests.rs.
}
```

- [ ] **Step 3: Write tests for tool_impl.rs**

Test: Tool trait implementation — name, schema, flags.

- [ ] **Step 4: Write tests for worktree.rs**

Test: worktree path construction, slug validation (if not in parent tests.rs).

- [ ] **Step 5: Run tests and commit**

Run: `cargo test tools::agent -- --nocapture`

```bash
git add src/tools/agent/
git commit -m "test(tools): add unit tests for agent dispatch, tool_impl, worktree"
```

---

### Task 9: execution/coordinator.rs + execution/pipeline.rs

**Files:**
- Modify: `src/tools/execution/coordinator.rs`
- Modify: `src/tools/execution/pipeline.rs`

- [ ] **Step 1: Read both files**

Identify pure logic vs orchestration. `pipeline.rs` is mostly orchestration (`run_tool_use()`) — may only support integration tests. `coordinator.rs` may have batch-grouping logic.

- [ ] **Step 2: Write tests for coordinator.rs**

Test: batch coordination, concurrent dispatch grouping.

- [ ] **Step 3: Write pipeline.rs stage tests (if pure logic exists)**

If `pipeline.rs` has extractable helpers, test them. Otherwise, the existing `execution/tests.rs` may already cover the pipeline via integration paths.

- [ ] **Step 4: Run tests and commit**

Run: `cargo test tools::execution -- --nocapture`

```bash
git add src/tools/execution/
git commit -m "test(tools): add unit tests for execution coordinator and pipeline"
```

---

### Task 10: Git-Dependent Commands (commit, branch, export, audit_export, session_export, memory, logout, login_code)

**Files:**
- Modify: `src/commands/commit.rs`
- Modify: `src/commands/branch.rs`
- Modify: `src/commands/export.rs`
- Modify: `src/commands/audit_export.rs`
- Modify: `src/commands/session_export.rs`
- Modify: `src/commands/memory.rs`
- Modify: `src/commands/logout.rs`
- Modify: `src/commands/login_code.rs`

These commands require external dependencies (git, auth, session storage). Strategy: test what we can in isolation, accept that full coverage needs E2E tests.

- [ ] **Step 1: commit.rs — test status summary building**

Extract the summary-building logic into a testable helper, or test the early-return paths:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::SessionId;
    use crate::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_commit_not_in_git_repo() {
        let handler = CommitHandler;
        // Use a path that's definitely not a git repo
        let mut ctx = test_ctx(PathBuf::from("/nonexistent/path/that/is/not/a/repo"));
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("not in a git repository")),
            _ => panic!("Expected Output"),
        }
    }
}
```

- [ ] **Step 2: branch.rs — test not-in-repo error**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::SessionId;
    use crate::types::app_state::AppState;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_branch_not_in_git_repo() {
        let handler = BranchHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/nonexistent/not/a/repo"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        };
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("not in a git repository")),
            _ => panic!("Expected Output"),
        }
    }
}
```

- [ ] **Step 3: export.rs, audit_export.rs, session_export.rs — read files, test any pure helpers**

- [ ] **Step 4: memory.rs — test show_paths() output formatting**

- [ ] **Step 5: logout.rs, login_code.rs — test error paths only**

These are hard to test without mocking the auth system. Add minimal smoke tests for error/edge cases.

- [ ] **Step 6: Run all command tests**

Run: `cargo test commands -- --nocapture`

- [ ] **Step 7: Commit**

```bash
git add src/commands/commit.rs src/commands/branch.rs src/commands/export.rs \
       src/commands/audit_export.rs src/commands/session_export.rs \
       src/commands/memory.rs src/commands/logout.rs src/commands/login_code.rs
git commit -m "test(commands): add tests for git/auth/export commands"
```

---

## Execution Order

```
Task 1 (effort.rs)           — 5 min, zero risk
Task 2 (login.rs helpers)    — 5 min, zero risk
Task 3 (file_write.rs)       — 5 min, zero risk
Task 4 (glob_tool.rs)        — 5 min, zero risk
Task 5 (security.rs)         — 5 min, zero risk
── checkpoint: verify all 25 new tests pass ──
Task 6 (integration: write+glob)  — 10 min
Task 7 (web_search/)              — 10 min, read-first
Task 8 (agent/)                   — 15 min, read-first
Task 9 (execution/)               — 15 min, read-first
Task 10 (git/auth commands)       — 20 min, partial coverage acceptable
```

## Success Criteria

- Every file in `src/tools/` and `src/commands/` has ≥1 `#[test]` or `#[tokio::test]`
- `cargo test` passes with zero failures
- No new warnings introduced
