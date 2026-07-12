## Project Knowledge Base Organization — Implementation Plan (Tasks 1–9)

**Working tree:** `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/.worktrees/allthecodes-preview` (branch `allthecodes-preview`). All edits below operate on that worktree's files.

**Scope (confirmed with user):** Tasks 1–9 only. Skip Task 10 (docs/README.md `.cc-rust` cleanup) and Task 11 (runtime integration). The plan's own "Minimum verification after implementing Tasks 1-9" gate is the exit target. Task 11 in particular must remain a separate runtime integration plan, so it touches no Rust crates in this pass — `cargo check` is therefore not required.

**Key implementation decisions (confirmed):**
- Architecture layer entities (Task 4) use the plan's canonical English names ("Layer 1: CLI / startup", etc.) as `name`, kept under `architecture-overview.md`-derived entities. Soft Chinese fallback parsing in the doc text still produces the canonical English rows.
- Plan-task parser (Task 5) extracts inline code paths from active-plan prose (not just `**Files:**` blocks) so Task 5 Step 5's verification query returns `crates/allthecodes-engine/src/agent/tool_impl.rs`. Archive plans keep prose parsing but routes any missing path to `legacy_path_reference` rather than `missing_active_path`.
- Cargo.toml workspace parsing uses `tomllib` when available, and falls back to a minimal regex parser that handles `[workspace] members = [...]` and bare `members = ["crates/*"]` syntax. The Python module target is Python 3.10+.
- SQLite FTS5 handled with a try/except; if the runtime lacks FTS5 the loader silently switches to a `LIKE` fallback table so the script stays portable.

### Files to create/modify

- **Create** `scripts/kb/build_project_kb.py` — single-file offline KB builder. Pure stdlib (`argparse`, `sqlite3`, `json`, `re`, `hashlib`, `pathlib`, `subprocess`, `tomllib` w/ regex fallback, optional `cargo metadata` shellout).
- **Create** `scripts/kb/README.md` — documents commands, generated outputs (`.allthecodes/kb/*`), commit policy, drift categories, output format.
- **Modify** `development/README.md` — append a `development/docs/` index section listing `project-knowledge-base-organization-plan.md` (Task 1 Step 3) and later `project-knowledge-base-maintenance.md` (Task 9 Step 3). No other section changes.
- **Create** `development/docs/project-knowledge-base-maintenance.md` — the six-step Explore→Plan→Approve→Apply→Verify→Archive flow and the high-severity drift review gates (Task 9 Step 1).

**Generated under `.allthecodes/kb/` (not committed — well-known fact, `.allthecodes/` already exists in preview worktree under `development/`? actually not at repo root — confirm below):** `project-facts.sqlite`, `facts.jsonl`, `kb-report.md`, `current-status.json`, `drift.json`.

### Task-by-task execution order

Every task ends with its plan-specified verification command run from the worktree root. Failed verification blocks the next task.

1. **Task 1 — Scaffold.** Create `scripts/kb/build_project_kb.py` with the plan's CLI skeleton (`build`, `report`, `query`, `check --fail-on {none,high,medium}`). Create `scripts/kb/README.md`. Add the `development/README.md` index entry for `project-knowledge-base-organization-plan.md`. Verify: `python3 scripts/kb/build_project_kb.py --repo . build` exits non-zero with a clear "not wired yet" message.

2. **Task 2 — Document inventory.**
   - `DOCUMENT_ROOTS = ["README.md", "AGENTS.md", "docs/README.md", "docs/WORK_STATUS.md", "docs/architecture", "docs/schemas", "development"]`.
   - `SKIP_PARTS = {".git", ".allthecodes", ".worktrees", "target", "__pycache__"}`.
   - Markdown frontmatter parser reads `title`, `description`, `keywords`, falls back to first `# ` heading.
   - `classify_doc()` follows the plan's exact pseudocode, including the `path.endswith("-plan.md") or "/plan/" in path or "/planz/" in path` rule.
   - For every `.md`, scan lines for `(?:^|\s)- \[(?P<check>[ xX])\]\s+(?P<text>.+)$` (checkbox), `[text](relative.md)` (links), and inline backtick code matching `^(crates/|docs/|development/|\.allthecodes/|src/)` (code-path references).
   - Insert into `documents` table (id = sha1 of repo-relative path, raw_hash = sha1 of file bytes, body = file text), and write `facts.jsonl` rows.
   - Verify: `docs/WORK_STATUS.md` indexed as `doc_type=status, status=active`; an archive doc (e.g. `development/archive/old_archive/plan/...plan.md`) indexed as `archived`. `query --q "WORK_STATUS"` returns the file row.

3. **Task 3 — WORK_STATUS current-state index.**
   - Parse `## 当前结论` (current conclusions), `## 活跃待办` (active work), `## 活跃文档入口` (active entrypoints), `## 历史 Deferred` (historical deferred). Each section continues until the next `## ` heading.
   - Active-work rows (the table under `## 活跃待办`) parse `scope`, `current_status`, `next_step` columns verbatim.
   - Each Markdown link under `## 活跃文档入口` becomes a `relations` row `documents('docs/WORK_STATUS.md') --indexes--> documents(linked_path)`.
   - Emit `.allthecodes/kb/current-status.json` matching the plan's exact JSON shape.
   - Verify: `query --q "Crate migration"` returns the WORK_STATUS active conclusion before archived crate-migration plans. Achieved by an "active status boosts result rank" tiebreaker in the query ranking.

4. **Task 4 — Architecture system map.**
   - Read `docs/architecture/introduction/architecture-overview.md`. Extract entities for the five architecture layers — stored with the **plan's canonical English names** ("Layer 1: CLI / startup", "Layer 2: TUI / Headless IPC / Daemon / Web", "Layer 3: QueryEngine / lifecycle / system_prompt / hooks", "Layer 4: query loop / tool runtime / MCP / sandbox", "Layer 5: API providers / streaming / fallback"). Extraction uses Chinese regex as a soft match then maps to the canonical English rows so the verification query "QueryEngine lifecycle modules" hits both the doc and the engine lib.
   - Read `crates/allthecodes-engine/src/lib.rs`, parse `pub mod name;` lines and create `code_entities(kind=module, name=name, path=file)`.
   - Read `crates/allthecodes-engine/src/query/loop_impl.rs`, parse the structured `/// Structure:` comment phaselist (setup, context preprocessing, streaming model call, post-streaming, terminal check, tool execution, attachments, continue) — create `code_entities(kind=workflow)` rows for each.
   - Create relation rows: `architecture-overview.md --describes--> allthecodes-engine`, `architecture-overview.md --describes--> crates/allthecodes-engine/src/query/loop_impl.rs`, `workflow_phase --implements--> crates/allthecodes-engine/src/query/loop_impl.rs`.
   - Verify: `query --q "QueryEngine lifecycle modules"` returns `docs/architecture/introduction/architecture-overview.md` and `crates/allthecodes-engine/src/lib.rs`.

5. **Task 5 — Plan-task ⨯ file relations.**
   - Match headings `### Task N: Title`, create one `plan_tasks` row with `sequence=N`, `title=Title`, `status` derived from checkbox state (when no checkbox lines exist, default to "active" for non-archive plans).
   - Parse `**Files:**` blocks capturing `Create / Modify / Test` bullet lines into `creates / modifies / verifies` relations with the file path as evidence.
   - Parse checkbox `- [ ] / - [x]` lines under each task and attach to the current task's `checklist_text`.
   - Prose inline code paths: for non-archive active plans, every `crates/…/file.rs`-shaped backtick reference within the task's prose bullet lines also becomes a `modifies` (or `mentions`, severity medium) relation. This is what makes Task 5 Step 5 return `crates/allthecodes-engine/src/agent/tool_impl.rs`.
   - Validate referenced paths: active-plan missing path → `missing_active_path` (severity medium); archived-plan missing path → `legacy_path_reference` (severity low). If the archived doc is indexed by WORK_STATUS as current guidance → upgrade to `missing_active_path`.
   - Verify: `query --q "fork_context live_readonly files"` returns at least `development/fork/fork-agent-context-inheritance-plan.md`, `crates/allthecodes-engine/src/agent/mod.rs`, `crates/allthecodes-engine/src/agent/tool_impl.rs`.

6. **Task 6 — Code fact extractors.**
   - Run `cargo metadata --format-version=1 --no-deps` (subprocess, env-injected `CARGO_HOME` / `RUSTUP_HOME` / `PATH` per `AGENTS.md` Cargo section). Parse `packages[].name` starting with `allthecodes-` → `code_entities(kind=crate)`.
   - Walk `crates/**/src/**/*.rs` with `rg --files crates` (subprocess) then a simple line scanner for `^pub (struct|enum|trait|fn|mod) <name>`. Store the exact declaration line as `signature`.
   - Register `Tool`, `ToolUseContext`, `ToolResult` from `crates/allthecodes-tools/src/tool.rs` (regex against the existing `pub trait Tool`, `pub struct ToolUseContext`, `pub struct ToolResult` lines we confirmed).
   - Register tool facts from `crates/allthecodes-tools/src/registry.rs` `Arc::new(<X>) as _` push sites — covers `SessionSearchTool`, `ToolSearchTool`, `BriefTool`, `ConfigTool`, `SystemStatusTool`, `AskUserQuestionTool`, `StructedOutputTool`, `SendUserMessageTool`, `EnterPlanModeTool`, `ExitPlanModeTool`. Each becomes `code_entities(kind=tool, path=registry.rs, name=ToolName)` plus a `modifies` relation `registry.rs --modifies-- <tool_module_path>` using the matching `use` import line as evidence.
   - From `crates/allthecodes-config/src/paths.rs`, register `code_entities(kind=config_path)` for: `ALLTHECODES_HOME`, `~/.allthecodes/`, `.allthecodes/`, `.allthecodes/settings.json` (via `keybindings_path` etc.), `.allthecodes/skills/`, `.allthecodes/plan.md`, `.allthecodes/plan-workflow.json`. Sourced from the file's `pub fn …` lines (we confirmed all exist in the grep above).
   - Verify: `query --q "ToolResult model_content display_preview"` → `crates/allthecodes-tools/src/tool.rs`; `query --q "ALLTHECODES_HOME project plan file"` → `crates/allthecodes-config/src/paths.rs`.

7. **Task 7 — Drift detection.**
   - Categories implemented exactly: `broken_markdown_link`, `missing_active_path`, `legacy_path_reference`, `path_isolation_name_drift`, `archive_used_as_current_fact`, `status_conflict`, `duplicate_current_entry`.
   - Markdown link resolution: relative `.md` / `.png` / image links resolved against the source file's directory; missing → `broken_markdown_link` severity high for active docs, low for archive.
   - Inline code path check for active docs: each `crates|docs|development/…` path with no wildcard must exist on disk. Missing → `missing_active_path` severity medium. Wildcards (`*`, `**`) skip.
   - Path-isolation name drift: flag active docs mentioning `.cc-rust`, `~/.cc-rust`, `.Codex`, `~/.Codex` (regex search) **unless** a same-line context word (`历史`, `historical`, `archive`, `archived`, `legacy`, `曾用`, `original`) appears nearby → downgrade to `legacy_path_reference` severity low. Otherwise emit `path_isolation_name_drift` severity high with the exact YAML-shaped record the plan specifies. The first expected hit is `docs/README.md`.
   - Archive leakage: if archived doc is cited by active doc without a context word, emit `archive_used_as_current_fact` severity high.
   - Status conflict: a checkbox in an active plan marked done but `docs/WORK_STATUS.md` lists it as not landed → `status_conflict` severity medium.
   - Duplicate current entry: WORK_STATUS `## 活跃文档入口` listing the same document twice → `duplicate_current_entry` severity low.
   - All findings persisted to `.allthecodes/kb/drift.json` and printed in `.allthecodes/kb/kb-report.md` grouped by severity then category. Each finding carries `evidence` (file:line citations) and `suggested_action`.
   - Verify: `report` writes `.allthecodes/kb/kb-report.md` and lists findings grouped by severity/category.

8. **Task 8 — Exact + structured + FTS query.**
   - `--mode exact`: equality search across `documents.path`, `documents.title`, `code_entities.name`, `code_entities.path`, `relations.evidence`. Single-row cap, returns full snippet.
   - `--mode structured`: query matches a plan → also return its tasks and modifies/creates/verifies file paths; query matches a file → also return plans that `modifies` / `creates` that file. Capped at the `--limit` value (default 10).
   - `--mode fts`: populate `documents_fts` (fts5 when available, else LIKE fallback on `documents.body`). Snippets capped at 320 characters around the first match.
   - `--mode all` (default for plain `query --q`): run exact first, structured next, FTS last; merge and dedupe by entity id, rank active docs above archived ones, status source-of-truth above ordinary active.
   - Output format (terminal, not JSON): the exact format from the plan, with `[document]` / `[code_entity]` / `[plan_task]` / `[relation]` rows, snippet bounded to one paragraph.
   - Verify: run each of the five target questions from Task 8 Step 5; each returns ≥1 source path, a source type, and a bounded snippet.

9. **Task 9 — Maintenance workflow + report policy.**
   - Create `development/docs/project-knowledge-base-maintenance.md` with the six-step Explore→Plan→Approve→Apply→Verify→Archive flow and the explicit high-severity review gates (broken active doc link, active doc references missing code path, active doc contradicts allthecodes path isolation, active doc treats archive plan as current without saying so).
   - Update `scripts/kb/README.md` with the maintainer workflow link, the report sections list (Summary / Current Status Index / New Or Changed Documents / Active Plans / Drift Findings / Archive References / Suggested Documentation Updates), and the PR vs local output policy.
   - `check` command implementation: build the index, run drift detection, exit non-zero (exit code 1) if any high-severity finding exists when `--fail-on=high` (default), or non-zero when any medium-or-above when `--fail-on=medium`. `--fail-on=none` always exits 0 if build succeeds.
   - `kb-report.md` section order set exactly as the plan lists.
   - Append the maintenance doc to the `development/README.md` index entry added in Task 1.
   - Verify: `check --fail-on high` exits non-zero (because `.cc-rust` drift in docs/README.md is high severity). This is **expected** per the plan — the cleanup happens in Task 10 which we are explicitly skipping. The script's exit code documents the existence of detected drift; we are not in this pass expected to fix docs/README.md.

### Acceptance gate (run from worktree root before declaring done)

```bash
cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/.worktrees/allthecodes-preview
python3 scripts/kb/build_project_kb.py --repo . build
python3 scripts/kb/build_project_kb.py --repo . report
python3 scripts/kb/build_project_kb.py --repo . query --q "WORK_STATUS current status"
python3 scripts/kb/build_project_kb.py --repo . query --q "ToolResult model_content"
python3 scripts/kb/build_project_kb.py --repo . check --fail-on high   # exits non-zero by design
```

Plus the per-task verification commands in Tasks 2–8.

All five must run and produce the expected outputs. `check` is documented as exiting non-zero because Task 10 is intentionally out of scope. The script is gitignored-safe: I'll verify `.allthecodes/kb/**` is not picked up by `git status` (`.allthecodes/` is already gitignored per `AGENTS.md` path isolation rules; if it isn't, I will add a one-line `.gitignore` entry for `.allthecodes/kb/`).

### Non-goals (explicit)

- No edit to `crates/**` Rust sources (no `cargo check` cycle needed).
- No edit to `docs/README.md` content (Task 10 deferred).
- No `SessionSearch`/`registry` runtime integration (Task 11 deferred).
- No semantic/vector retrieval (per plan's Global Constraints).

### Committing

Per `AGENTS.md` commit policy: stage only the four created/modified paths explicitly:

```bash
git add -A -- scripts/kb/build_project_kb.py scripts/kb/README.md development/docs/project-knowledge-base-maintenance.md
git add -A -- development/README.md
git commit -m "Add project knowledge-base offline builder (Tasks 1-9)"
```

Will not push without explicit user instruction. Will not commit any generated `.allthecodes/kb/**` artifact.