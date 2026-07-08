# Hermes-like Persistent Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Hermes-like capability layer for allthecodes: searchable long-term session history, curated memory, learnable skills, background review, and runtime entry points without replacing the existing agent loop.

**Architecture:** Keep `QueryEngine` / tool execution as the single agent loop and add a persistent runtime layer around it. The first shippable slice is `session_search`, because it turns existing SQLite/JSON session persistence into Hermes-style warm memory that slash commands, Web, model tools, memory review, and future gateway workers can share. Later tasks layer curated memory, skill learning, background review approvals, cron/gateway routing, and worktree subagents on top of the same storage and command/tool surfaces.

**Tech Stack:** Rust workspace crates, `sqlx` SQLite, existing `allthecodes-session` JSON fallback, `allthecodes-protocol` REST/RPC definitions, `allthecodes-web` Axum handlers, `allthecodes-commands` slash commands, `allthecodes-tools` model-visible tools, `allthecodes-skills`, and existing `allthecodes-gateway` / `allthecodes-daemon` scaffolding.

## Global Constraints

- Full-build rule: do not keep Lite-era omissions when touching a Hermes-corresponding path.
- Path isolation: all persisted allthecodes data must stay under `~/.allthecodes/` or project `.allthecodes/`, never `~/.codex/`, `~/.Codex/`, or `.Codex/`.
- Do not rewrite `QueryEngine`, `ToolRouter`, or the main turn loop for this feature.
- Do not create parallel session state in Web, TUI, CLI, gateway, or cron code; shared storage APIs are the source of truth.
- Rust TUI work, when needed, is only under `crates/allthecodes/src/ui/`.
- New behavior must be covered by failing tests before production implementation.
- SQLite failures must preserve existing JSON fallback behavior.
- Search results must cap snippets and result counts; never dump complete transcripts by default.
- Approval-gated writes are required for self-improvement outputs that modify memory or skills.

---

## Current Repo Mapping

- Existing session storage lives in `crates/allthecodes-session/src/storage.rs`, `storage/file_store.rs`, `storage/sqlite_store.rs`, and `storage/serialization.rs`.
- SQLite already stores `sessions`, `session_messages`, and `session_rollouts`; JSON files remain in `~/.allthecodes/sessions/` for compatibility and fallback.
- Durable replay exists under `crates/allthecodes-session/src/record_replay/`.
- Memory directory support exists under `crates/allthecodes-session/src/memdir/`.
- Session insight memory exists under `crates/allthecodes-engine/src/services/session_memory.rs`.
- Skill registry, loader, package validation, and usage tracking exist under `crates/allthecodes-skills/`.
- Slash commands live under `crates/allthecodes-commands/src/`.
- Web/API session handlers live under `crates/allthecodes-web/src/handlers/sessions.rs`.
- Protocol endpoints are declared in `crates/allthecodes-protocol/src/request.rs` and session DTOs in `crates/allthecodes-protocol/src/v1/sessions.rs`.
- Tool implementations and registries live under `crates/allthecodes-tools/src/`.

---

### Task 1: Session Search Storage API

**Files:**
- Modify: `crates/allthecodes-session/src/storage.rs`
- Modify: `crates/allthecodes-session/src/storage/file_store.rs`
- Modify: `crates/allthecodes-session/src/storage/sqlite_store.rs`
- Create: `crates/allthecodes-session/src/storage/session_search.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSearchResult {
    pub session_id: String,
    pub message_index: usize,
    pub role: Option<String>,
    pub msg_type: String,
    pub timestamp: i64,
    pub title: String,
    pub cwd: String,
    pub workspace_key: String,
    pub workspace_name: String,
    pub snippet: String,
}

pub fn search_sessions(query: &str, limit: usize) -> anyhow::Result<Vec<SessionSearchResult>>;

pub fn search_workspace_sessions(
    cwd: &std::path::Path,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<SessionSearchResult>>;
```

- Internal SQLite helper:

```rust
#[cfg(feature = "sqlite-storage")]
pub(super) fn search_session_messages(
    query: &str,
    limit: usize,
    workspace_key_filter: Option<String>,
) -> anyhow::Result<Vec<SessionSearchResult>>;
```

**Storage design:**
- Add a plain SQLite table in a new migration:

```sql
CREATE TABLE IF NOT EXISTS session_message_search (
    session_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    role TEXT,
    msg_type TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (session_id, position),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_session_message_search_text
    ON session_message_search(text);

CREATE INDEX IF NOT EXISTS idx_session_message_search_session
    ON session_message_search(session_id, position);
```

- Add a best-effort FTS5 virtual table outside mandatory migrations:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS session_message_search_fts
USING fts5(session_id UNINDEXED, position UNINDEXED, role, msg_type UNINDEXED, text);
```

- If FTS5 creation or query fails, fall back to `LOWER(text) LIKE LOWER(?)` over `session_message_search`.
- Update `save_session_file()` and legacy import path so both `session_messages` and `session_message_search` are refreshed in the same transaction.
- `session_message_search.text` is a normalized plain-text projection of stored message JSON, not a replacement for full messages.

- [ ] **Step 1: Write the failing storage tests**

Add tests to the existing `#[cfg(test)] mod tests` in `crates/allthecodes-session/src/storage.rs`:

```rust
#[test]
#[serial_test::serial]
fn test_search_sessions_finds_saved_message_text() {
    let temp = tempdir().unwrap();
    let _g = HomeGuard::set(temp.path());

    save_session(
        "hermes-search-one",
        &[user_message("remember hermes runtime planning details")],
        "/proj",
    )
    .unwrap();

    let hits = search_sessions("hermes runtime", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, "hermes-search-one");
    assert_eq!(hits[0].message_index, 0);
    assert!(hits[0].snippet.contains("hermes runtime"));
}

#[test]
#[serial_test::serial]
fn test_search_workspace_sessions_filters_by_workspace() {
    let temp = tempdir().unwrap();
    let _g = HomeGuard::set(temp.path());
    let project = temp.path().join("project");
    let other = temp.path().join("other");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&other).unwrap();

    save_session(
        "hermes-workspace-hit",
        &[user_message("gateway cron memory")],
        project.to_str().unwrap(),
    )
    .unwrap();
    save_session(
        "hermes-workspace-miss",
        &[user_message("gateway cron memory")],
        other.to_str().unwrap(),
    )
    .unwrap();

    let hits = search_workspace_sessions(&project, "gateway", 10).unwrap();
    let ids = hits
        .iter()
        .map(|hit| hit.session_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["hermes-workspace-hit"]);
}

#[test]
#[serial_test::serial]
fn test_search_sessions_uses_json_fallback_when_sqlite_is_blocked() {
    let temp = tempdir().unwrap();
    let _g = HomeGuard::set(temp.path());
    std::fs::create_dir_all(temp.path().join("state").join("state_5.sqlite")).unwrap();

    write_fixture_session(
        "hermes-json-fallback",
        vec![user_sm(
            "json fallback searchable memory",
            "00000000-0000-0000-0000-000000000701",
        )],
        "/proj",
    )
    .unwrap();

    let hits = search_sessions("searchable memory", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session_id, "hermes-json-fallback");
}
```

- [ ] **Step 2: Run the failing tests**

Run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo test -p allthecodes-session search_sessions -- --nocapture
```

Expected: compile failure for missing `search_sessions` / `search_workspace_sessions`.

- [ ] **Step 3: Implement `session_search.rs`**

Implement:

```rust
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
const SNIPPET_CHARS: usize = 240;

pub(super) fn clamp_search_limit(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_LIMIT
    } else {
        limit.min(MAX_LIMIT)
    }
}

pub(super) fn searchable_message_text(message: &SerializableMessage) -> String {
    // user: content string and text/tool_result blocks
    // assistant: text blocks, tool_use names, tool_use JSON input summary
    // system: content
    // progress/attachment: short JSON summary only
}

pub(super) fn make_snippet(text: &str, query: &str) -> String {
    // Return a bounded snippet around the first case-insensitive match.
}

pub(super) fn json_search_results(
    query: &str,
    limit: usize,
    workspace_key_filter: Option<&str>,
    excluded_ids: &std::collections::HashSet<String>,
) -> anyhow::Result<Vec<SessionSearchResult>>;
```

- [ ] **Step 4: Wire public storage APIs**

In `storage.rs`:

```rust
mod session_search;

pub use session_search::SessionSearchResult;
pub use file_store::{search_sessions, search_workspace_sessions};
```

In `file_store.rs`:

```rust
pub fn search_sessions(query: &str, limit: usize) -> Result<Vec<SessionSearchResult>> {
    // Try sqlite_store::search_session_messages(query, limit, None).
    // On error, log and fall back to JSON search.
}

pub fn search_workspace_sessions(
    cwd: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SessionSearchResult>> {
    // Compute workspace_key(cwd), then use sqlite or JSON fallback.
}
```

- [ ] **Step 5: Wire SQLite indexing and querying**

In `sqlite_store.rs`:
- Add migration for `session_message_search`.
- Add `ensure_fts_search_table(pool)` as best-effort.
- Add `refresh_search_rows_for_session(tx, file)`.
- Call it in both `save_session_file()` and `save_session_file_to_pool()`.
- Implement `search_session_messages()` with FTS query first and LIKE fallback.
- Always filter `sessions.archived = 0`.
- Sort by `sessions.last_modified DESC, session_messages.position ASC`.

- [ ] **Step 6: Run green tests**

Run:

```bash
cargo test -p allthecodes-session search_sessions -- --nocapture
cargo test -p allthecodes-session storage::tests::test_search_workspace_sessions_filters_by_workspace -- --nocapture
```

Expected: all new tests pass.

---

### Task 2: `/session search` Slash Command

**Files:**
- Modify: `crates/allthecodes-commands/src/session.rs`

**Interfaces:**
- Consumes: `allthecodes_session::storage::search_sessions`
- Consumes: `allthecodes_session::storage::search_workspace_sessions`
- Produces slash command forms:

```text
/session search <query>
/session search all <query>
/session find <query>
/session find all <query>
```

- [ ] **Step 1: Write command tests**

Add tests to `crates/allthecodes-commands/src/session.rs`:

```rust
#[tokio::test]
#[serial_test::serial]
async fn test_session_search_workspace_outputs_hits() {
    let home = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    allthecodes_session::storage::save_session(
        "session-search-command",
        &[user_message("hermes style session search")],
        workspace.to_str().unwrap(),
    )
    .unwrap();

    let handler = SessionHandler;
    let mut ctx = test_ctx();
    ctx.cwd = workspace;

    let result = handler.execute("search hermes style", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("session-search-command"));
            assert!(text.contains("hermes style"));
            assert!(text.contains("Use /resume session-search-command"));
        }
        _ => panic!("expected Output"),
    }
}
```

If `EnvGuard` and `user_message` helpers do not exist in this module yet, copy the small equivalents from `resume.rs` tests.

- [ ] **Step 2: Run failing command test**

Run:

```bash
cargo test -p allthecodes-commands session_search -- --nocapture
```

Expected: failure because command parsing does not support `search`.

- [ ] **Step 3: Implement command parsing**

Update `SessionHandler::execute()` match:

```rust
["search", rest @ ..] | ["find", rest @ ..] => handle_search(ctx, rest),
```

Add:

```rust
fn handle_search(ctx: &CommandContext, parts: &[&str]) -> Result<CommandResult> {
    let (include_all, query_parts) = match parts {
        ["all", rest @ ..] => (true, *rest),
        _ => (false, parts),
    };
    let query = query_parts.join(" ");
    if query.trim().is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /session search <query>\n       /session search all <query>".into(),
        ));
    }
    let hits = if include_all {
        storage::search_sessions(&query, 20)?
    } else {
        storage::search_workspace_sessions(&ctx.cwd, &query, 20)?
    };
    Ok(CommandResult::Output(format_session_search_hits(&hits, include_all)))
}
```

Add result formatting with columns:

```text
Session search results (current workspace):

  Session ID                              Msg  Role       Title
  ----------                              ---  ----       -----
  session-search-command                    0  user       hermes style session search
      hermes style session search

Use /resume session-search-command to load a result.
```

- [ ] **Step 4: Run green command tests**

Run:

```bash
cargo test -p allthecodes-commands session_search -- --nocapture
```

Expected: command tests pass.

---

### Task 3: Web/API Session Search

**Files:**
- Modify: `crates/allthecodes-protocol/src/v1/sessions.rs`
- Modify: `crates/allthecodes-protocol/src/request.rs`
- Modify: `crates/allthecodes-web/src/handlers/sessions.rs`
- Modify: `crates/allthecodes-web/src/handlers/sessions_tests.rs`

**Interfaces:**
- Protocol endpoint:

```text
GET /api/sessions/search
```

- DTOs:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SessionSearchParams {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SessionSearchResponse {
    pub query: String,
    pub hits: Vec<SessionSearchHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SessionSearchHit {
    pub session_id: String,
    pub message_index: usize,
    pub role: Option<String>,
    pub msg_type: String,
    pub timestamp: i64,
    pub title: String,
    pub cwd: String,
    pub workspace_key: String,
    pub workspace_name: String,
    pub snippet: String,
}
```

- Add request entry:

```rust
SessionSearch => "GET /api/sessions/search" {
    params: v1::SessionSearchParams,
    response: v1::SessionSearchResponse,
},
```

- [ ] **Step 1: Write protocol tests**

Extend existing protocol request tests so endpoint metadata includes:

```rust
(ApiMethod::SessionSearch, "GET", "/api/sessions/search")
```

Expected failing result before implementation: unknown `SessionSearch`.

- [ ] **Step 2: Write Web handler test**

In `sessions_tests.rs`, add a test that:
- sets `ALLTHECODES_HOME` to a temp dir,
- saves one matching and one non-matching session,
- calls the processor or handler for `SessionSearch`,
- asserts that the response contains only the matching hit.

Use the existing test support style in `sessions_tests.rs`; do not create a second test harness.

- [ ] **Step 3: Implement processor and route**

In `sessions.rs`:

```rust
#[derive(Clone)]
pub struct SessionSearchProcessor {
    state: WebState,
}

impl From<WebState> for SessionSearchProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionSearchProcessor {
    type Request = SessionSearchParams;
    type Response = SessionSearchResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.search"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        // query required and non-empty
        // workspace_key optional: if provided, filter by key using storage API
        // if workspace_key omitted, search all sessions
    }
}
```

Add to `handlers()`:

```rust
.handle(ApiMethod::SessionSearch, get(session_search_handler))
```

Add:

```rust
pub async fn session_search_handler(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<SessionSearchParams>,
) -> Response {
    rest_processor_response::<SessionSearchProcessor>(state, ApiMethod::SessionSearch, params).await
}
```

- [ ] **Step 4: Run protocol and Web tests**

Run:

```bash
cargo test -p allthecodes-protocol session_search -- --nocapture
cargo test -p allthecodes-web session_search -- --nocapture
```

Expected: tests pass.

---

### Task 4: Model-visible `SessionSearch` Tool

**Files:**
- Modify: `crates/allthecodes-tools/Cargo.toml`
- Modify: `crates/allthecodes-tools/src/lib.rs`
- Create: `crates/allthecodes-tools/src/session_search.rs`
- Modify the existing tool registry file that aggregates `allthecodes_tools::tools()`; locate it with `rg "fn tools\\(" crates/allthecodes-tools crates/allthecodes-engine`.

**Interfaces:**
- Tool name: `SessionSearch`
- JSON schema:

```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "Search text to find in saved allthecodes sessions."
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 20,
      "description": "Maximum number of hits to return."
    },
    "scope": {
      "type": "string",
      "enum": ["workspace", "all"],
      "description": "Search only the current workspace by default, or all saved sessions."
    }
  },
  "required": ["query"]
}
```

- [ ] **Step 1: Write tool tests**

Create tests in `session_search.rs`:

```rust
#[tokio::test]
#[serial_test::serial]
async fn session_search_tool_returns_bounded_hits() {
    // temp ALLTHECODES_HOME
    // save a session with a known phrase
    // execute tool with {"query":"known phrase","limit":5}
    // assert output includes session_id, snippet, and resume instruction
}

#[tokio::test]
async fn session_search_tool_rejects_empty_query() {
    // execute with {"query":"   "}
    // assert error mentions query is required
}
```

- [ ] **Step 2: Run failing tool tests**

Run:

```bash
cargo test -p allthecodes-tools session_search_tool -- --nocapture
```

Expected: missing module/tool failures.

- [ ] **Step 3: Implement `SessionSearchTool`**

Implement the same `Tool` trait style used by `SleepTool`, `WorkflowTool`, or `ViewImageTool`.

Behavior:
- default `scope = "workspace"`;
- limit clamped to `1..=20`;
- use `ctx.cwd` when available;
- call `allthecodes_session::storage::search_workspace_sessions` or `search_sessions`;
- return compact markdown/text with one hit per result;
- never include complete message JSON.

- [ ] **Step 4: Register the tool**

Add the tool to the central tool list so the model can call it through the normal `ToolRouter`. Do not add a special gateway executor.

- [ ] **Step 5: Run green tool tests**

Run:

```bash
cargo test -p allthecodes-tools session_search_tool -- --nocapture
cargo check -p allthecodes-tools
```

Expected: tests and check pass.

---

### Task 5: Curated Memory Layer Alignment

**Files:**
- Modify: `crates/allthecodes-session/src/memdir/types.rs`
- Modify: `crates/allthecodes-session/src/memdir/crud.rs`
- Modify: `crates/allthecodes-session/src/memdir/recall.rs`
- Modify: `crates/allthecodes-engine/src/prompt_sections.rs`
- Modify: existing memory command files under `crates/allthecodes-commands/src/memory.rs`

**Interfaces:**
- Keep existing memory files under allthecodes paths.
- Add a bounded curated-memory profile:

```rust
pub enum CuratedMemoryTarget {
    User,
    Project,
    Reference,
    Feedback,
}

pub struct CuratedMemoryWrite {
    pub target: CuratedMemoryTarget,
    pub key: String,
    pub value: String,
    pub source_session_id: Option<String>,
    pub approval_id: Option<String>,
}
```

- [ ] **Step 1: Write tests for bounded memory output**

Tests must prove:
- `USER.md` / global user memory does not exceed configured max bytes.
- project memory path uses `.allthecodes/`, not `.codex/` or `.Codex/`.
- prompt context uses a frozen snapshot per session startup and does not mutate mid-turn.

- [ ] **Step 2: Implement bounded curated memory helpers**

Use existing `memdir` JSON memory as the durable store, and generate markdown entrypoints as bounded indexes.

- [ ] **Step 3: Add slash command affordances**

Add command forms:

```text
/memory pending
/memory approve <id>
/memory reject <id>
/memory search <query>
```

Do not let automatic review write directly to memory without approval unless an explicit config flag is set.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-session memdir -- --nocapture
cargo test -p allthecodes-commands memory -- --nocapture
```

Expected: memory tests pass with allthecodes path isolation.

---

### Task 6: Learnable Skills Management

**Files:**
- Modify: `crates/allthecodes-skills/src/lib.rs`
- Modify: `crates/allthecodes-skills/src/loader.rs`
- Modify: `crates/allthecodes-commands/src/skills_cmd.rs`
- Create: `crates/allthecodes-commands/src/learn.rs`
- Modify: `crates/allthecodes-commands/src/lib.rs`

**Interfaces:**
- Add slash command:

```text
/learn conversation <instruction>
/learn session <session_id> <instruction>
/learn docs <path> <instruction>
```

- Add skill management command forms:

```text
/skills pending
/skills diff <id>
/skills approve <id>
/skills reject <id>
```

- Add pending proposal type:

```rust
pub struct SkillProposal {
    pub id: String,
    pub action: SkillProposalAction,
    pub skill_name: String,
    pub source_session_id: Option<String>,
    pub proposed_path: std::path::PathBuf,
    pub markdown: String,
    pub created_at: String,
}
```

- [ ] **Step 1: Write tests for proposal staging**

Tests must prove:
- `/learn` creates a proposal file, not an active skill.
- approval writes `SKILL.md` under `~/.allthecodes/skills/` or `.allthecodes/skills/`.
- rejected proposals do not alter active registry.

- [ ] **Step 2: Implement proposal persistence**

Use:

```text
~/.allthecodes/skill_proposals/<proposal_id>.json
```

or project:

```text
.allthecodes/skill_proposals/<proposal_id>.json
```

- [ ] **Step 3: Implement learn prompt assembly**

The learn command should gather:
- selected session snippets via `session_search`,
- current session transcript summary,
- relevant repo docs path content if provided,
- existing skill registry names to avoid duplicates.

The first implementation can generate a draft markdown proposal using local heuristics. Model-assisted generation can be added later through the same proposal path.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-skills learn -- --nocapture
cargo test -p allthecodes-commands learn -- --nocapture
```

Expected: proposal lifecycle tests pass.

---

### Task 7: Background Review and Approval Queue

**Files:**
- Create: `crates/allthecodes-engine/src/services/background_review.rs`
- Modify: `crates/allthecodes-engine/src/services/mod.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- Modify: `crates/allthecodes-session/src/record_replay/types.rs`
- Modify: `crates/allthecodes-commands/src/memory.rs`
- Modify: `crates/allthecodes-commands/src/skills_cmd.rs`

**Interfaces:**
- New review output:

```rust
pub struct BackgroundReviewProposal {
    pub id: String,
    pub source_session_id: String,
    pub kind: BackgroundReviewProposalKind,
    pub summary: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub enum BackgroundReviewProposalKind {
    MemoryAdd,
    MemoryReplace,
    SkillCreate,
    SkillPatch,
    WorkflowWarning,
}
```

- [ ] **Step 1: Write trigger tests**

Tests must prove:
- review is not triggered before the configured turn threshold;
- review proposal files are written after threshold;
- approval queue survives process restart;
- no proposal mutates memory or skills before approval.

- [ ] **Step 2: Implement proposal queue**

Use:

```text
~/.allthecodes/review_proposals/<proposal_id>.json
```

Each proposal references `session_id` and replay sequence boundaries when available.

- [ ] **Step 3: Hook after-turn review**

Trigger after turn finish, not inside the active model stream. The hook input should include:
- recent messages,
- tool calls / errors,
- verification commands from replay or tool summaries when available,
- session_search hits for similar prior tasks,
- current memory and relevant skills.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-engine background_review -- --nocapture
cargo test -p allthecodes-commands "memory|skills" -- --nocapture
```

Expected: proposal lifecycle tests pass.

---

### Task 8: Gateway, Cron, and Long-running Runtime Shell

**Files:**
- Modify: `crates/allthecodes-gateway/src/`
- Modify: `crates/allthecodes-daemon/src/`
- Modify: `crates/allthecodes-tasks/src/`
- Modify: `crates/allthecodes-protocol/src/request.rs`
- Modify: `crates/allthecodes-protocol/src/v1/tasks.rs`
- Modify: `crates/allthecodes-web/src/handlers/tasks.rs`

**Interfaces:**
- Add first-class scheduled agent task:

```rust
pub struct ScheduledAgentTask {
    pub id: String,
    pub prompt: String,
    pub cwd: String,
    pub schedule: ScheduleSpec,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
}
```

- Runtime path:

```text
cron tick
  -> allthecodes-gateway task registry
  -> same QueryEngine / ToolRouter execution path
  -> session storage + record_replay + session_search index
  -> Web/TUI/CLI notification adapters
```

- [ ] **Step 1: Write scheduler tests**

Tests must prove:
- due tasks are selected once;
- disabled tasks are ignored;
- task execution creates or resumes a session;
- output is persisted in session storage and searchable.

- [ ] **Step 2: Implement task registry persistence**

Use allthecodes data dir:

```text
~/.allthecodes/scheduled_tasks/tasks.json
```

- [ ] **Step 3: Route execution through existing engine**

Do not add a separate shell executor. Scheduled tasks must submit prompts through the same agent runtime used by CLI/Web sessions.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-gateway scheduled -- --nocapture
cargo test -p allthecodes-tasks scheduled -- --nocapture
```

Expected: scheduled runtime tests pass.

---

### Task 9: Worktree Subagents

**Files:**
- Modify: `crates/allthecodes-tools/src/tasks/mod.rs`
- Modify: `crates/allthecodes-worktree/src/`
- Modify: `crates/allthecodes-engine/src/lifecycle/`
- Modify: `crates/allthecodes-session/src/fork.rs`
- Modify: `crates/allthecodes-web/src/handlers/tasks.rs`

**Interfaces:**
- Add model-visible tool:

```text
DelegateTask
```

- Input:

```rust
pub struct DelegateTaskInput {
    pub role: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub worktree: Option<String>,
    pub max_turns: Option<usize>,
    pub verification_policy: Option<String>,
}
```

- Output:

```rust
pub struct DelegateTaskOutput {
    pub task_id: String,
    pub child_session_id: String,
    pub status: String,
    pub worktree_path: Option<String>,
}
```

- [ ] **Step 1: Write subagent tests**

Tests must prove:
- child sessions get parent lineage;
- child agents write to isolated worktree when requested;
- parent can list/wait/resume child task;
- child session content is searchable through `session_search`.

- [ ] **Step 2: Implement child session creation**

Use existing `fork` and worktree crates. Do not invent a second session format.

- [ ] **Step 3: Implement task lifecycle**

Task status values:

```text
queued -> running -> completed | failed | cancelled
```

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-worktree delegate -- --nocapture
cargo test -p allthecodes-tools delegate -- --nocapture
```

Expected: delegation lifecycle tests pass.

---

### Task 10: Documentation and Full Verification

**Files:**
- Modify: `docs/WORK_STATUS.md`
- Modify: `docs/IMPLEMENTATION_GAPS.md`
- Modify: `docs/KNOWN_ISSUES.md` only if user-visible limitations remain
- Create: `docs/archive/implemented/hermes-runtime.md`

**Documentation requirements:**
- Document that Hermes-like runtime is additive around `QueryEngine`.
- Document `session_search` as warm memory.
- Document memory and skill proposal approval gates.
- Document path isolation.
- Document intentionally deferred platform adapters if Telegram/Slack/etc. are not implemented.

- [ ] **Step 1: Update docs after each implemented milestone**

Move completed items from gap docs into implemented docs only after tests pass.

- [ ] **Step 2: Run targeted verification**

Run the relevant targeted test commands from each task.

- [ ] **Step 3: Run full verification before final commit**

Run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo build --workspace --release
```

Expected: release build exits 0 and no new warnings are introduced.

---

## Delivery Order

1. `session_search` storage API.
2. `/session search` slash command.
3. Web/API search endpoint.
4. model-visible `SessionSearch` tool.
5. curated memory bounded write/read alignment.
6. `/learn` and skill proposal lifecycle.
7. background review proposal queue.
8. gateway cron tasks.
9. worktree subagents.
10. documentation and full release verification.

This order gives allthecodes a usable Hermes-like memory substrate first, then exposes it to humans and the model, then adds self-improvement and long-running runtime behavior on top.
