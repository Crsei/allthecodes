# Bash/Shell Parity Migration Plan

Date: 2026-05-18

Scope:
- Source gap note: `docs/utils/bash-shell.md`
- Bun reference: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/utils/bash/`
- Bun reference: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/utils/shell/`
- Rust target repo: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust`
- Ownership boundary: `development/reference/CRATE_DEPENDENCY_TARGETS.md`

## Executive Summary

`docs/utils/bash-shell.md` is directionally correct that Rust still lacks the
large Bun Bash/Shell safety surface, but the current code is more advanced than
that note says in a few critical paths.

Already active in Rust:
- `crates/cc-utils/src/bash.rs` is used by `cc-permissions`, `cc-sandbox`, and
  `cc-engine` for shell splitting, quoting checks, heredoc validation, timeout
  handling, command names, and simple prefixes.
- `crates/cc-utils/src/shell.rs` is used by Bash and PowerShell execution for
  default shell detection and process environment setup.
- `crates/cc-permissions/src/bash_matcher.rs` is the active `Bash(...)`
  permission matcher. It handles compound commands, common process wrappers,
  `bash -c`, exact/glob/prefix rules, and denies on later command segments.
- `crates/cc-sandbox/src/policy.rs` has strict `allowedCommands` argv-prefix
  matching and rejects redirects, substitutions, and backticks for pre-approval.
- `crates/cc-sandbox/src/runner.rs` has shell preflight checks for network policy
  and common filesystem writes before Bash/PowerShell execution.
- `crates/cc-engine/src/tools/exec/bash.rs` and `powershell.rs` are the runtime
  tool implementations, including validation, streaming, timeout/cancel process
  tree handling, sandbox wrapping, and git operation metadata.
- `crates/cc-engine/src/tools/exec/powershell_parser.rs` already uses the native
  PowerShell parser and metadata for a fail-closed execution gate.

Still missing or only partial:
- No Rust equivalent for Bun's full Bash AST parser and fail-closed Bash AST
  security analyzer.
- No Rust equivalent for Bun's complete read-only command validation maps for
  `git`, `gh`, `docker`, `rg`, `pyright`, and external commands.
- No `ShellProvider` abstraction equivalent; Bash/PowerShell command assembly is
  embedded directly in the tool implementations.
- No `ShellSnapshot` equivalent for `rg`/`bfs`/`ugrep` argv0 shell functions and
  session shell initialization.
- No `rearrangePipeCommand()` equivalent.
- Prefix extraction is static and small; there is no Bun-style Haiku fallback,
  subcommand extractor, Fig/spec registry, or spec-based prefix depth.
- Heredoc validation is active, but Bun's extraction/restoration pipeline is not
  ported.

This plan treats the missing safety/parsing work as Full Build parity work, not
as a Lite-era intentional crop. If a later implementation decides not to port a
specific Bun behavior, it must be marked intentional in docs.

## Owner Model

Do not move all Bash/Shell logic into `cc-utils`. Use owner crates:

| Area | Rust owner | Rule |
| --- | --- | --- |
| Pure lexical helpers, shell quoting, heredoc text utilities | `cc-utils` only if dependency-free and reusable | Keep no runtime state here. |
| Bash AST parser, normalized command model, shell provider DTOs | new `cc-shell-command` or internal module promoted to that crate | This is the likely primary owner. |
| Permission rule matching, read-only command classification, Auto/Plan decisions | `cc-permissions`, possibly with `cc-shell-command` parser contract | No dependency on `cc-engine` or root. |
| Sandbox command preflight and OS runner wrapping | `cc-sandbox` | Consume parsed command DTOs, do not own parser policy. |
| Bash/PowerShell tool runtime | `cc-engine/src/tools/exec/*` | Consume provider/parser/policy crates; keep runtime execution here. |
| Tool schemas and prompts | `cc-tools/src/exec/*` | Metadata only. |
| TUI permission display | `crates/claude-code-rs/src/ui/permissions/*` | UI only; no policy decisions. |

Preferred new crate:
- `cc-shell-command`

Why:
- `development/reference/CRATE_DEPENDENCY_TARGETS.md` already lists `cc-shell-command`
  as the natural owner for shell parsing, display, risk summary, and escalation
  adapters.
- It avoids bloating `cc-utils`.
- It gives `cc-permissions`, `cc-sandbox`, and `cc-engine` one shared parsed
  command contract instead of three separate shell splitters.

Acceptable first step:
- Start as `crates/cc-shell-command/` with only parser DTOs and Bash helpers.
- Move existing pure helpers from `cc-utils::bash` only after call sites are
  ready. Do not leave permanent compatibility wrappers.

## Current Rust Usage Matrix

| Rust surface | Current use | Status |
| --- | --- | --- |
| `cc_utils::bash::parse_command` | `cc-sandbox` network/policy/runner, `cc-permissions` matcher/rules, `cc-engine` Bash validation | Active but simple `shell-words` parse only. |
| `cc_utils::bash::split_compound_command` | network policy, permission matcher, accept-edits Bash check, Bash dangerous command loop | Active but not a full AST splitter. |
| `cc_utils::bash::extract_command_name` | permission prefix matcher, Bash user-facing name | Active. |
| `cc_utils::bash::extract_command_prefixes` | Bash tool deny-prefix check | Active, static, not Bun LLM/spec parity. |
| `cc_utils::bash::validate_heredocs` | Bash tool input validation | Active partial parity. |
| `cc_utils::bash::should_add_stdin_redirect` | Bash tool execution | Active partial parity. |
| `cc_utils::bash::rewrite_windows_null_redirect` | Bash tool execution for POSIX shells | Active. |
| `cc_utils::shell::detect_default_shell` | Bash tool shell selection | Active but not provider abstraction. |
| `cc_utils::shell::build_shell_env` | Bash and PowerShell execution | Active. |
| `cc_permissions::bash_matcher` | `Bash(...)` allow/ask/deny rules | Active. |
| `cc_sandbox::policy::allowedCommands` | Sandbox workspace auto-approval | Active but not read-only command classification. |
| `cc_sandbox::runner::preflight_shell_command` | Bash/PowerShell before execution | Active but heuristic write-target extraction. |
| `cc_engine::tools::exec::powershell_parser` | PowerShell tool validation | Active and stronger than the old gap doc describes. |

## Bun To Rust Gap Matrix

| Bun capability | Bun source | Rust counterpart | Current Rust status | Needed action |
| --- | --- | --- | --- | --- |
| Full Bash parser | `bash/bashParser.ts`, `bash/parser.ts` | `cc-utils::bash::parse_command`, simple splitters | Partial and active; no AST, node budget, timeout, byte offsets, compound structures | Add `cc-shell-command` Bash AST parser using `tree-sitter-bash` or a proven crate. |
| Fail-closed Bash AST safety analysis | `bash/ast.ts`, `bash/treeSitterAnalysis.ts`, `bash/ParsedCommand.ts` | regex dangerous checks, `bash_matcher`, sandbox preflight | Partial; no node allowlist, variable scope, command substitution tracking, semantic checks | Port analyzer to normalized Rust DTOs and consume from permissions/sandbox/exec validation. |
| Traditional shell-quote parser helpers | `bash/commands.ts`, `bash/shellQuote.ts`, `bash/shellQuoting.ts` | `cc-utils::bash`, duplicate splitters in sandbox/permissions | Partial and used | Consolidate into shared parser facade; keep only fallback helpers after AST path lands. |
| Heredoc extraction/restoration | `bash/heredoc.ts` | `validate_heredocs`, `contains_heredoc`, stdin redirect guard | Partial validation only | Port extraction/restoration and nested/quoted/dash variants; keep execution validation active. |
| Pipe command rearrangement | `bash/bashPipeCommand.ts` | none | Missing | Port after provider abstraction; add tests for eval/stdin redirect compatibility. |
| Fig/spec registry | `bash/registry.ts`, `bash/specs/*`, `shell/specPrefix.ts` | none | Missing | Add optional static spec loader or embedded subset; use for prefix suggestions and permissions UX. |
| Static command prefix extraction | `bash/prefix.ts` | `extract_command_prefixes` | Partial and active | Replace static list with spec-aware extractor while preserving current simple behavior. |
| Haiku LLM prefix extraction | `shell/prefix.ts` | none | Missing | Add later behind model adapter, cache, and fail-closed fallback to static/spec extractor. |
| ShellProvider interface | `shell/shellProvider.ts` | no trait; direct command assembly in tools | Missing | Introduce provider trait/DTO used by Bash/PowerShell tools and hooks. |
| BashProvider behavior | `shell/bashProvider.ts` | `detect_default_shell`, direct `Command::new(shell)` | Partial | Add provider assembly: extglob disable, pipe rearrange, snapshot sourcing, cwd update contract. |
| PowerShellProvider behavior | `shell/powershellProvider.ts` | direct `-Command`; native parser exists | Partial | Add encoded command path, temp/sandbox dir behavior, provider contract; preserve native parser gate. |
| ShellSnapshot | `bash/ShellSnapshot.ts` | none | Missing | Port session snapshot under `~/.cc-rust`, with path isolation and search-tool argv0 functions. |
| Read-only command validation | `shell/readOnlyCommandValidation.ts` | `Tool::is_read_only=false`, Plan mode blocks Bash/PowerShell; sandbox allowedCommands is separate | Missing as read-only classifier | Port maps and flag validator into `cc-permissions`/`cc-shell-command`; wire Bash/PowerShell input-dependent `is_read_only`. |
| UNC/path vulnerability checks | `readOnlyCommandValidation.ts` | path validation exists for file tools; no shell read-only UNC check | Missing for shell commands | Port `containsVulnerableUncPath` equivalent and use in read-only classifier. |
| Output limits | `shell/outputLimits.ts` | `cc-tools::exec::truncate_output`, tool max result size | Mostly present | Verify numeric parity; adjust docs/tests if intentional. |
| Default shell resolution and PowerShell detection | `resolveDefaultShell.ts`, `powershellDetection.ts` | `cc-utils::shell::detect_default_shell`, `PowerShellTool::is_enabled` | Partial | Fold into provider layer and test per platform. |
| Shell tool utilities | `shell/shellToolUtils.ts` | scattered | Unknown/partial | Audit during provider phase; port only runtime-used behavior. |

## Execution Plan

### P0: Baseline And Contract Freeze

Write scope:
- `development/archive/plan/bash-shell-parity-migration-plan-2026-05-18.md`
- optional follow-up edit to `docs/utils/bash-shell.md`
- optional new `docs/debug/bash-shell-current-state-2026-05-18.md`

Tasks:
- Keep this matrix current against actual Rust call paths, not only file names.
- Mark `docs/utils/bash-shell.md` statements that are stale:
  - PowerShell parser/security is no longer "simple provider missing only".
  - Heredoc validation is already active.
  - `allowedCommands` strict matching exists, but it is not read-only validation.
- Decide whether first implementation creates `cc-shell-command` immediately or
  starts as an internal module and promotes later. Preferred: create crate.

Exit criteria:
- Current docs distinguish "active partial implementation" from "missing".
- The owner for parser DTOs is explicit before code moves begin.

### P1: Add Shared Shell Command Contract

Write scope:
- new `crates/cc-shell-command/Cargo.toml`
- new `crates/cc-shell-command/src/lib.rs`
- new `crates/cc-shell-command/src/model.rs`
- new `crates/cc-shell-command/src/fallback.rs`
- workspace `Cargo.toml`

Tasks:
- Define stable DTOs:
  - `ShellDialect`
  - `ParsedShellCommand`
  - `ShellSegment`
  - `SimpleCommand`
  - `Redirection`
  - `Heredoc`
  - `ParseDiagnostic`
  - `ParseMode::{Permissive, FailClosedSecurity}`
- Move or wrap current fallback behavior behind DTO-producing APIs:
  - parse argv
  - split segments
  - command name
  - command prefix
  - stdin redirect detection
  - heredoc validation
- Do not change runtime behavior yet; only add shared facade and tests.

Exit criteria:
- `cargo test -p cc-shell-command` covers existing `cc-utils::bash` behavior.
- No runtime crate depends on root or UI.
- Existing callers can still compile unchanged.

### P2: Port Bash AST Parser And Security Analyzer

Write scope:
- `crates/cc-shell-command/src/bash_ast.rs`
- `crates/cc-shell-command/src/bash_security.rs`
- `crates/cc-shell-command/src/heredoc.rs`
- `crates/cc-shell-command/src/tree_sitter_analysis.rs`
- `Cargo.toml` dependency entries for `tree-sitter-bash` or equivalent

Tasks:
- Integrate a real Bash parser. Prefer `tree-sitter-bash` unless a better Rust
  crate already provides the needed AST and byte offsets.
- Reproduce Bun safety properties:
  - parse timeout and node budget
  - byte offset tracking
  - fail-closed unknown node handling
  - explicit allowlist for safe node kinds
  - compound command/list/pipeline structure
  - file redirects and heredoc redirects
  - command substitutions and placeholders
  - variable assignment/scope resolution where needed for safety
  - semantic checks for high-risk shell constructs
- Keep fallback parser only for display and low-risk contexts; security mode must
  fail closed on parse failure, budget overrun, unsupported nodes, or dynamic
  constructs.

Exit criteria:
- AST tests cover representative Bun cases: pipelines, lists, function defs,
  subshells, process substitutions, command substitutions, heredocs, quoted
  strings, arithmetic, redirects, and unsupported nodes.
- Security analyzer exposes typed decisions instead of strings only.
- No execution path switches to the AST parser until P3 integration tests exist.

### P3: Replace Duplicate Runtime Splitters With Shared Parser Facade

Write scope:
- `crates/cc-utils/src/bash.rs`
- `crates/cc-permissions/src/bash_matcher.rs`
- `crates/cc-permissions/src/rules.rs`
- `crates/cc-sandbox/src/network.rs`
- `crates/cc-sandbox/src/policy.rs`
- `crates/cc-sandbox/src/runner.rs`
- `crates/cc-engine/src/tools/exec/bash.rs`

Tasks:
- Rewire current call sites to use `cc-shell-command` DTOs.
- Keep permission behavior stable while replacing the parser substrate.
- Remove duplicate segment splitters from `cc-sandbox::policy` and
  `cc-sandbox::runner` once the shared parser covers those cases.
- Add adapter helpers where needed rather than adding cross-runtime dependencies.
- Keep `cc-utils` pure and shrink it after callers move.

Exit criteria:
- Current tests for `cc-utils`, `cc-permissions`, `cc-sandbox`, and Bash tool
  behavior still pass.
- Deny rules still catch later compound/pipeline segments.
- Sandbox `allowedCommands` remains fail-closed for redirects, substitutions,
  backticks, and unsupported syntax.

### P4: Port Read-Only Command Validation

Write scope:
- `crates/cc-permissions/src/read_only_shell.rs`
- `crates/cc-permissions/src/lib.rs`
- `crates/cc-engine/src/tools/exec/bash.rs`
- `crates/cc-engine/src/tools/exec/powershell.rs`
- optional shared flag parser module in `cc-shell-command`

Tasks:
- Port Bun's read-only command model:
  - `FlagArgType`
  - `ExternalCommandConfig`
  - `GIT_READ_ONLY_COMMANDS`
  - `GH_READ_ONLY_COMMANDS`
  - `DOCKER_READ_ONLY_COMMANDS`
  - `RIPGREP_READ_ONLY_COMMANDS`
  - `PYRIGHT_READ_ONLY_COMMANDS`
  - `EXTERNAL_READONLY_COMMANDS`
  - flag validation and callback support
  - UNC/path vulnerability detection
- Implement `is_shell_command_read_only(command, dialect, cwd, parser)` with a
  fail-closed result type:
  - `ReadOnly`
  - `NotReadOnly(reason)`
  - `Unsupported(reason)`
  - `ParseFailed(reason)`
- Wire input-dependent `Tool::is_read_only` for Bash and PowerShell.
- Ensure Plan mode can allow truly read-only shell commands without allowing
  writes or dynamic command execution.
- Keep sandbox `allowedCommands` separate from read-only classification.

Exit criteria:
- Plan mode permits safe examples such as `git status`, `git diff`,
  `git log`, `gh pr view`, `rg pattern`, `ls`, `cat`, and blocks `git clean`,
  `git checkout`, `gh pr merge`, redirects, shell substitutions, and path
  vulnerability cases.
- Tests prove read-only classification does not override deny/ask rules.
- Agent Explore/read-only flows can use shell commands only when this classifier
  returns `ReadOnly`.

### P5: Add ShellProvider Layer

Write scope:
- `crates/cc-shell-command/src/provider.rs`
- `crates/cc-shell-command/src/bash_provider.rs`
- `crates/cc-shell-command/src/powershell_provider.rs`
- `crates/cc-engine/src/tools/exec/bash.rs`
- `crates/cc-engine/src/tools/exec/powershell.rs`
- `crates/cc-utils/src/shell.rs`

Tasks:
- Define provider contract:
  - provider type/dialect
  - executable and argv builder
  - display command
  - environment setup
  - stdin redirect policy
  - command normalization
  - optional cwd update contract
  - optional sandbox temp dir behavior
- Move shell detection and argv construction out of direct tool code.
- Bash provider:
  - source snapshot when present
  - disable extglob where upstream does
  - apply pipe command rearrangement when needed
  - preserve current streaming/sandbox behavior in `cc-engine`
- PowerShell provider:
  - build encoded command args using UTF-16LE Base64
  - preserve existing native parser validation before execution
  - add temp/sandbox dir behavior if required by platform support

Exit criteria:
- Bash/PowerShell tools only orchestrate validation, permissions, sandbox, spawn,
  streaming, and result conversion.
- Provider tests cover Unix, Windows, Git Bash, PowerShell Core, and unavailable
  PowerShell cases with platform gates.

### P6: Port ShellSnapshot And Search Tool Integrations

Write scope:
- `crates/cc-shell-command/src/snapshot.rs`
- `crates/cc-config/src/paths.rs` if a new path helper is needed
- `crates/cc-engine/src/tools/exec/bash.rs`
- docs under `docs/utils/` or `docs/debug/`

Tasks:
- Port snapshot creation under `~/.cc-rust`, never `~/.claude`.
- Add shell function generation for search tools:
  - `rg`
  - `bfs`
  - `ugrep`
  - find/grep compatibility helpers where upstream uses them
- Preserve path isolation from AGENTS.md:
  - global data root: `~/.cc-rust`
  - no writes to `~/.Codex` or `~/.claude`
- Decide lifecycle:
  - create once per session
  - refresh when shell path or config changes
  - cleanup stale snapshots

Exit criteria:
- Bash provider can source a cc-rust snapshot.
- Snapshot paths are isolated and tested.
- Search tool wrappers are covered by unit tests and one shell smoke test.

### P7: Port Heredoc Extraction/Restoration And Pipe Rearrangement

Write scope:
- `crates/cc-shell-command/src/heredoc.rs`
- `crates/cc-shell-command/src/pipe.rs`
- `crates/cc-engine/src/tools/exec/bash.rs`

Tasks:
- Port Bun heredoc extraction/restoration:
  - unquoted delimiter
  - single/double quoted delimiter
  - `<<-` tab stripping
  - multiple heredocs
  - nested command contexts
  - placeholder salt generation
- Port `rearrangePipeCommand()`:
  - pipe operator detection
  - environment assignment preservation
  - control structure guard
  - eval stdin redirect quoting
  - continuation line joining
- Integrate only after provider layer is stable.

Exit criteria:
- Existing heredoc validation tests still pass.
- New tests cover extraction/restoration round trips.
- Pipe commands that upstream rearranges produce equivalent argv/provider output.

### P8: Port Prefix Extraction UX

Write scope:
- `crates/cc-shell-command/src/prefix.rs`
- `crates/cc-shell-command/src/spec_prefix.rs`
- `crates/cc-shell-command/src/spec_registry.rs`
- `crates/cc-permissions`
- `crates/cc-tools/src/plan_mode.rs`

Tasks:
- Port static prefix extraction from Bun `bash/prefix.ts`.
- Add spec-aware prefix extraction:
  - known subcommand depth
  - flags that consume arguments
  - fallback to command-only prefix
- Add optional embedded command spec subset for high-value tools:
  - `git`
  - `gh`
  - `cargo`
  - `npm`/`pnpm`/`yarn`
  - `docker`
  - `kubectl`
- Later add Haiku/LLM prefix extractor behind an adapter:
  - cache by command string
  - use static/spec result on failure
  - never allow LLM output to weaken deny/read-only classification

Exit criteria:
- Permission rule suggestions become more precise without changing deny
  semantics.
- Plan-mode allowed prompt generation still strips dangerous broad shell rules.
- LLM prefix extractor, if added, is advisory only.

### P9: Cross-Crate Integration And E2E

Write scope:
- `crates/cc-engine/src/tools/exec/*`
- `crates/cc-permissions`
- `crates/cc-sandbox`
- `crates/cc-shell-command`
- `crates/claude-code-rs/tests/*`
- `development/archive/IMPLEMENTATION_GAPS.md`
- `development/archive/COMPLETED_FULL.md`

Tasks:
- Add focused integration tests:
  - Plan mode read-only Bash/PowerShell decisions
  - deny/ask/allow rule precedence with AST parsed commands
  - sandbox `allowedCommands` still fail-closed
  - read-only classifier does not permit redirects/substitutions
  - shell snapshot path isolation
  - PowerShell provider encoded command with parser gate
  - Bash provider snapshot + pipe rearrangement
- Add e2e cases where existing harness supports them:
  - safe read-only shell in Plan mode
  - blocked write shell in Plan mode
  - sandbox workspace allowed command
  - denied dangerous command
- Move completed lines from gap docs to `development/archive/COMPLETED_FULL.md`.

Exit criteria:
- Targeted crates pass.
- `cargo build --workspace --release` passes with only known non-blocking npm
  warning if npm remains absent.
- Docs no longer list implemented behavior as missing.

## Parallel Team Work Plan

The work can be split across teams after P1 contract lands. Before P1, avoid
parallel code edits because all lanes need the same DTO boundary.

Team A: Parser and analyzer
- Write scope: `crates/cc-shell-command/src/bash_ast.rs`,
  `bash_security.rs`, `tree_sitter_analysis.rs`, parser tests.
- Deliverable: fail-closed Bash AST and safety result DTOs.

Team B: Read-only and permissions
- Write scope: `crates/cc-permissions/src/read_only_shell.rs`,
  `rules.rs`, `decision.rs` tests, Bash/PowerShell `is_read_only` wiring.
- Deliverable: Bun read-only maps and Plan/Explore shell behavior.

Team C: Runtime providers
- Write scope: provider modules in `cc-shell-command`,
  `cc-engine/src/tools/exec/bash.rs`, `powershell.rs`, `cc-utils/src/shell.rs`.
- Deliverable: ShellProvider abstraction, Bash/PowerShell argv construction,
  encoded PowerShell command path.

Team D: Snapshot, heredoc, pipe
- Write scope: `snapshot.rs`, `heredoc.rs`, `pipe.rs`, provider integration.
- Deliverable: snapshot file lifecycle, search wrappers, heredoc round trip,
  pipe rearrangement.

Team E: Prefix/spec UX and docs
- Write scope: prefix/spec modules, `cc-tools/src/plan_mode.rs`, docs.
- Deliverable: spec-aware prefix suggestions and documentation closeout.

Non-overlap rule:
- Teams must not edit the same Rust module concurrently except through an agreed
  adapter interface from P1.
- Parser DTO changes are owned by Team A; other teams request additions rather
  than inventing local DTOs.
- Permission behavior changes are owned by Team B; runtime teams should not add
  ad hoc permission checks outside existing security/permission boundaries.

## Validation Commands

Use the repo-local Rust toolchain:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```

Recommended gates by phase:

| Phase | Commands |
| --- | --- |
| P1 | `cargo test -p cc-shell-command`; `cargo check -p cc-utils -p cc-permissions -p cc-sandbox -p cc-engine` |
| P2 | `cargo test -p cc-shell-command bash`; parser corpus tests |
| P3 | `cargo test -p cc-permissions bash_matcher`; `cargo test -p cc-sandbox allowed_command preflight_shell_command`; `cargo test -p cc-engine bash` |
| P4 | `cargo test -p cc-permissions read_only_shell`; `cargo test -p cc-engine plan read_only bash powershell` |
| P5 | `cargo test -p cc-shell-command provider`; `cargo test -p cc-engine bash powershell` |
| P6/P7 | `cargo test -p cc-shell-command snapshot heredoc pipe`; targeted shell smoke tests |
| Closeout | `cargo build --workspace --release` |

Known machine caveats:
- `npm` may be absent, so the web-ui dependency install warning is non-blocking
  for Rust release builds.
- Run tests that write home/session data with `CC_RUST_HOME` or a temp home if
  they otherwise fall back to read-only default paths.

## Documentation Closeout

After implementation:
- Update `docs/utils/bash-shell.md` from "missing/partial" to runtime-verified
  status.
- Update `development/archive/IMPLEMENTATION_GAPS.md` only for remaining gaps or intentional
  crops.
- Move completed rows to `development/archive/COMPLETED_FULL.md`.
- If any Bun behavior is intentionally not ported, add it to
  `development/archive/IMPLEMENTATION_GAPS.md` section 6 with owner, date, reason, and review
  trigger.
