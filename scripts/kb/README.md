# scripts/kb — Project Knowledge Base Builder

`build_project_kb.py` builds an offline, read-only project fact index for the
allthecodes repository. It turns Markdown plans, status documents,
architecture notes, and Rust source files into a SQLite-backed searchable
fact layer under `.allthecodes/kb/`.

> The KB is a **project fact layer**, not just Markdown chunking. Plans,
> status documents, code, and historical archives are normalized facts with
> explicit relations and drift signals.

## Quick start

Run from the repository root (no extra dependencies beyond Python 3.10+
stdlib and `rg` / `cargo` for code-fact extraction):

```bash
python3 scripts/kb/build_project_kb.py --repo . build
python3 scripts/kb/build_project_kb.py --repo . report
python3 scripts/kb/build_project_kb.py --repo . query --q "WORK_STATUS current status"
python3 scripts/kb/build_project_kb.py --repo . check --fail-on high
```

## Commands

| Command | Description |
|---------|-------------|
| `build` | Rebuild the SQLite index (`project-facts.sqlite`), `facts.jsonl`, `current-status.json`, and `drift.json` under `.allthecodes/kb/`. |
| `report` | Regenerate `kb-report.md` from the current index. Does not rebuild. |
| `query` | Search the index. Defaults to `--mode all`. See Query modes below. |
| `check` | Build, then exit non-zero when drift at or above `--fail-on` exists. Defaults to `high`. Choices: `none`, `high`, `medium`. |

### Query modes

| `--mode` | Behavior |
|---------|----------|
| `exact` | Equality search across `documents.path`, `documents.title`, `code_entities.name`, `code_entities.path`, and `relations.evidence`. |
| `structured` | For each matched plan, return its tasks and modified/created/verified files. For each matched file, return plans that modify or create it. |
| `fts` | SQLite FTS5 (or LIKE fallback) over document bodies. Snippets capped at 320 characters. |
| `all` (default) | exact first, structured next, FTS last; merge and dedupe by entity id. Active documents rank above archived ones; the WORK_STATUS source-of-truth row boosts active conclusions above archived plans. |

### Output format (terminal)

```
1. [document] docs/WORK_STATUS.md
   title: allthecodes 工作状态总览
   status: active
   snippet: ...

2. [code_entity] crates/allthecodes-tools/src/tool.rs::ToolResult
   kind: struct
   relation: defined_in
```

Results are capped by `--limit` (default 10). Snippets are bounded to one
paragraph; no full plan documents or transcripts are dumped by default.

## Generated outputs (`.allthecodes/kb/`)

| File | Purpose |
|------|---------|
| `project-facts.sqlite` | SQLite database: `documents`, `code_entities`, `plan_tasks`, `relations`, `drift_findings`, plus FTS5 `documents_fts` (or LIKE fallback `documents_search`). |
| `facts.jsonl` | One JSON object per row in `documents` / `code_entities` / `plan_tasks` / `relations` / `drift_findings`, useful for diffing. |
| `kb-report.md` | Human-readable drift report grouped by severity then category. Sections: Summary / Current Status Index / New Or Changed Documents / Active Plans / Drift Findings / Archive References / Suggested Documentation Updates. |
| `current-status.json` | Snapshot of `docs/WORK_STATUS.md`'s current conclusions, active work, active entrypoints, and historical deferred policy. |
| `drift.json` | All drift findings as JSON rows. |

## Commit policy

- **Generated `.allthecodes/kb/**` files are NOT committed.** Per allthecodes
  path isolation rules, `.allthecodes/` is already gitignored. Do not commit
  any SQLite / JSONL / report artifact unless a later task explicitly changes
  this policy.
- Committed source for the KB lives at:
  - `scripts/kb/build_project_kb.py`
  - `scripts/kb/README.md`
  - `development/docs/project-knowledge-base-organization-plan.md`
  - `development/docs/project-knowledge-base-maintenance.md`
- Do **not** embed the sibling `../allthecodes-web` repository or pass
  `--features web-ui` when building for npm — those rules are independent of
  this KB builder.

## Drift categories

| Category | Severity (default) | Meaning |
|----------|--------------------|---------|
| `broken_markdown_link` | high (active), low (archive) | Relative Markdown link target does not resolve. |
| `missing_active_path` | medium | Active document references a `crates/…`, `docs/…`, or `development/…` path that does not exist on disk. |
| `legacy_path_reference` | low | An archive document uses legacy paths (`src/main.rs`, `src/mcp/tools.rs`, `crates/cc-ipc-protocol`) — historical fact, not a fix. |
| `path_isolation_name_drift` | high | Active document uses `.cc-rust`, `~/.cc-rust`, `.Codex`, `~/.Codex` without an explicit historical / archive / legacy context word. |
| `archive_used_as_current_fact` | high | An active document cites an archived document as current guidance without naming it as historical / archived. |
| `status_conflict` | medium | An active plan checkbox is marked done but `docs/WORK_STATUS.md` has not landed it. |
| `duplicate_current_entry` | low | `docs/WORK_STATUS.md · ## 活跃文档入口` lists the same document twice. |

Each emitted finding carries an `evidence` field (concrete file:line
citations) and a `suggested_action` field. Anonymous "maybe stale" warnings are
never emitted.

## Path isolation

The KB stores path facts under allthecodes naming only — `ALLTHECODES_HOME`,
`~/.allthecodes/`, project `.allthecodes/`, keychain service `allthecodes`.
Legacy references to `.cc-rust`, `.Codex`, `cc-rust`, `codex` etc. are flagged
through drift detection rather than silently indexed.

## Maintenance workflow

See [`development/docs/project-knowledge-base-maintenance.md`](../../development/docs/project-knowledge-base-maintenance.md)
for the six-step Explore → Plan → Approve → Apply → Verify → Archive flow and
the high-severity drift review gates.
