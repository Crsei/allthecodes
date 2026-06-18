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
| `$ROOT/teams/{sanitized_team_name}/` | Agent Teams config and mailbox state. |
| `$ROOT/tasks/{sanitized_team_name}/` | Agent Teams task lists. |
| `$ROOT/projects/{sanitized_cwd}/memory/team/` | Per-workspace Team Memory sync mirror. |

## Project-local paths

These live **inside your project directory** (not under `$ROOT`) and behave like
`.git/config` — per-repo overrides and artifacts:

| Path | Purpose |
|------|---------|
| `{cwd}/.cc-rust/settings.json` | Project-level settings. Loaded in addition to global settings. |
| `{cwd}/.cc-rust/settings.local.json` | Project-local overrides. Intended to be gitignored. |
| `{cwd}/.cc-rust/memory/` | Project-scoped memory. |
| `{cwd}/.cc-rust/skills/` | Project-scoped skills. |

These are discovered by ancestor-walk from the current working directory.

## Settings layering

`settings.json` is loaded from up to four files and merged in this order
(higher overrides lower):

| Priority | Source | Path |
|---------:|--------|------|
| 1 (lowest) | `managed` | Windows: `%ProgramData%\cc-rust\settings.json` · others: `/etc/cc-rust/managed-settings.json` · or `$CC_RUST_MANAGED_SETTINGS` |
| 2 | `user` | `$ROOT/settings.json` |
| 3 | `project` | `{cwd}/.cc-rust/settings.json` |
| 4 | `local` | `{cwd}/.cc-rust/settings.local.json` |
| 5 | `env` | `CLAUDE_*` / `CC_*` overrides |
| 6 (highest) | `cli` | `--model`, `--verbose`, `--permission-mode`, ... |

Use `/config show` to see the merged effective values; `/config sources`
prints which layer won for each key. Schema is at
[`docs/schemas/settings.schema.json`](schemas/settings.schema.json) and
generated from `src/config/settings.rs::settings_schema()` (the in-crate
test `schema_file_matches_runtime` keeps them in sync).

When `/config set` writes a key, the existing file is copied to a
timestamped `.bak` sibling first; the five most recent backups are
retained.

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
CC_RUST_HOME=/tmp/cc-rust-test ./target/release/claude-code-rs --headless
```

All persistence will land in `/tmp/cc-rust-test/`, with nothing written to the
current working directory or your real home.
