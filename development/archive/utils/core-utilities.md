# Core Utilities — Owner-Aware Migration Matrix

> **Updated**: 2026-05-18 (Batch 1)
> **Scope**: `claude-code-bun/src/utils/` → Rust workspace crate ownership.
> **Constraint**: Pure helpers → `cc-utils`. Domain behavior → owner crate.

## Current State

| Crate | Modules | Status |
|---|---|---|
| `cc-utils` | 9 modules (now 9 after removing `file_state_cache`) | Small, actively used |
| `cc-teams` | ~12 modules | ~50% parity with Bun swarm/team |
| Other crates | Various | Each owns its domain utilities |

## Decisions (Batch 1)

| Asset | Decision | Rationale |
|---|---|---|
| `abort.rs` | **Keep in cc-utils** | Pure utility. `AbortController` is a reusable async cancellation primitive with no domain dependencies. Currently no external callers but maintained for future use. |
| `file_state_cache.rs` (cc-utils) | **Removed** | Exact duplicate of `cc-tools::tool::FileStateCache`. The cc-tools version (HashMap+RwLock) is canonical across all tool and engine callers. The LRU-based cc-utils version had zero external callers. |
| `hash.rs` (cc-utils) | **Added** (Batch 1) | Pure utility: Bun-compatible signed `djb2_hash` plus Rust-local `hash_content`/`hash_pair` cache-key helpers. |

## Owner-Aware Matrix

Each Bun `src/utils/` file is classified by **owner crate** and **implementation status**.

### Legend

| Status | Meaning |
|---|---|
| ✅ Implemented | Module exists with full coverage |
| 🔶 Partial | Module exists but has gaps vs Bun |
| 📋 Std Equivalent | Rust standard library or well-known crate covers this |
| 🏗️ Planned | Module identified, not yet implemented |
| ⏳ Deferred Full Build Gap | Required for Full Build parity, but scheduled for a later batch |
| 🚫 Intentional Gap | Deliberately not porting; must not be used for Full Build deferrals |

---

### `cc-utils` — Pure Helpers

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `abortController.ts` | `abort.rs` | ✅ | `AbortController`, `child()` |
| `hash.ts` | `hash.rs` (new) | ✅ | `djb2_hash`, `hash_content`, `hash_pair` |
| `fileStateCache.ts` | — | 🚫 | Replaced by `cc-tools::tool::FileStateCache` |
| `cwd.ts` | `cwd.rs` | ✅ | `get_cwd`, `set_cwd`, `resolve_path` |
| `bash/commands.ts` | `bash.rs` | ✅ | command parsing, shell escaping, heredoc detection |
| `bash/shellQuote.ts` | `bash.rs` (partial) | 🔶 | Some quoting utilities not ported |
| `messages.ts` | `messages.rs` | ✅ | `get_text_content`, `count_tool_calls`, etc. |
| `tokens.ts` | `tokens.rs` | ✅ | Token estimation, context window math |
| `shell/` shell detection | `shell.rs` | ✅ | `ShellKind`, `build_shell_env` |
| `stringUtils.ts` | — | 🏗️ | `escapeRegExp`, `capitalize`, `plural`, `safeJoinLines`, `EndTruncatingAccumulator`, `truncateToLines` |
| `uuid.ts` | — | 🏗️ | `validateUuid`, `createAgentId` (Rust has `uuid` crate; validation helper useful) |
| `words.ts` | — | 🏗️ | Word processing utilities |
| `truncate.ts` | `messages.rs` (partial) | 🔶 | `truncate_text` exists; Bun has more variants |
| `treeify.ts` | — | 🏗️ | Tree-formatted string output |
| `hyperlink.ts` | — | 🏗️ | Hyperlink/anchor utilities |
| `format.ts` | — | 🏗️ | General formatting helpers |
| `formatBriefTimestamp.ts` | — | 🏗️ | Short timestamp formatting (Rust has `chrono`) |
| `json.ts` | — | 🏗️ | JSON helpers (Rust has `serde_json`) |
| `jsonRead.ts` | — | 🏗️ | JSON reading with error handling |
| `lazySchema.ts` | — | 🏗️ | Lazy schema validation |
| `contentArray.ts` | — | 🏗️ | Content array processing |
| `semanticBoolean.ts` | — | 🏗️ | Semantic boolean parsing |
| `semanticNumber.ts` | — | 🏗️ | Semantic number parsing |
| `sleep.ts` | — | 📋 | `tokio::time::sleep` |
| `timeouts.ts` | — | 🏗️ | Timeout wrapper utilities |
| `combinedAbortSignal.ts` | — | 🏗️ | Merge multiple abort signals |
| `array.ts` | — | 📋 | Rust slices/iterators/Vec cover this |
| `set.ts` | — | 📋 | Rust `HashSet` covers this |
| `CircularBuffer.ts` | — | 📋 | `ringbuf` crate or `VecDeque` |
| `stream.ts` | — | 📋 | Rust streams / `futures` |
| `semaphore.ts` | — | 📋 | `tokio::sync::Semaphore` |
| `sequential.ts` | — | 📋 | `futures::future::join_all`, channels |
| `memoize.ts` | — | 📋 | `cached` crate or manual |
| `withResolvers.ts` | — | 📋 | `tokio::sync::oneshot` channel |
| `diff.ts` | — | 📋 | `similar` crate |
| `markdown.ts` | — | 🏗️ | Markdown processing utilities |
| `markdownConfigLoader.ts` | — | 🏗️ | Markdown config loader |
| `frontmatterParser.ts` | — | 🏗️ | Frontmatter parser |
| `textHighlighting.ts` | — | 🏗️ | Text highlighting |
| `stats.ts` | — | 🏗️ | Statistics collection helpers |
| `statsCache.ts` | — | 🏗️ | Stats caching |

---

### `cc-config` — Configuration, Paths, Environment

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `env.ts` | `cc-config` env module | ✅ | Environment variable handling |
| `envUtils.ts` | — | 🏗️ | Additional env utilities |
| `envDynamic.ts` | — | 🏗️ | Dynamic environment updates |
| `envValidation.ts` | — | 🏗️ | Environment validation |
| `systemDirectories.ts` | `cc-config` | ✅ | XDG/compat paths |
| `cachePaths.ts` | `cc-config` | ✅ | Cache directory resolution |
| `windowsPaths.ts` | — | 🏗️ | Windows path normalization |
| `subprocessEnv.ts` | — | 🏗️ | Subprocess env construction |
| `userAgent.ts` | `cc-config` | ✅ | User-Agent string |

---

### `cc-tools` — File / Process / Tool Utilities

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `file.ts` | File tools in `cc-tools` | ✅ | File operations |
| `fileRead.ts` | `cc-tools/src/fs/file_read.rs` | ✅ | |
| `fileReadCache.ts` | — | 🏗️ | Read cache layer |
| `fsOperations.ts` | File tools | ✅ | FS operations |
| `glob.ts` | — | 🏗️ | Glob matching (need `globset`/`ignore`) |
| `tempfile.ts` | — | 📋 | `tempfile` crate |
| `lockfile.ts` | — | 🏗️ | File locking |
| `findExecutable.ts` | — | 🏗️ | Executable search |
| `which.ts` | — | 📋 | `which` crate |
| `binaryCheck.ts` | — | 🏗️ | Binary file detection |
| `execFileNoThrow.ts` | Exec helpers | ✅ | Non-throwing exec |
| `execFileNoThrowPortable.ts` | — | 🏗️ | Portable variant |
| `execSyncWrapper.ts` | — | 🏗️ | Sync exec wrapper |
| `genericProcessUtils.ts` | Process utils | ✅ | Process utilities |
| `toolErrors.ts` | `cc-tools` errors | ✅ | Tool error types |
| `toolPool.ts` | — | 🏗️ | Tool pool management |
| `toolResultStorage.ts` | — | 🏗️ | Tool result storage |
| `toolSchemaCache.ts` | — | 🏗️ | Tool schema caching |
| `fileHistory.ts` | — | 🏗️ | File operation history |
| `fileOperationAnalytics.ts` | — | 🏗️ | File operation analytics |
| `shell/readOnlyCommandValidation.ts` | Permissions | ✅ | In `cc-permissions` |
| `shell/prefix.ts` | — | 🏗️ | Command prefix extraction |

---

### `cc-engine` — Prompt / Message / Lifecycle

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `hooks.ts` | `cc-engine` hooks | ✅ | Hook system |
| `commandLifecycle.ts` | — | 🏗️ | Command lifecycle |
| `handlePromptSubmit.ts` | — | 🏗️ | Prompt submit handler |
| `immediateCommand.ts` | — | 🏗️ | Immediate command execution |
| `contextAnalysis.ts` | — | 🏗️ | Context window analysis |
| `contextSuggestions.ts` | — | 🏗️ | Context suggestions |
| `conversationRecovery.ts` | — | 🏗️ | Conversation recovery |
| `groupToolUses.ts` | — | 🏗️ | Tool use grouping |
| `messagePredicates.ts` | — | 🏗️ | Message predicate helpers |
| `messageQueueManager.ts` | — | 🏗️ | Message queue management |
| `messages/` | — | 🏗️ | Message mapping directory |
| `slashCommandParsing.ts` | `cc-commands` | ✅ | Slash command parsing |
| `systemPrompt.ts` | Engine prompt | ✅ | System prompt construction |
| `systemPromptType.ts` | Engine prompt | ✅ | System prompt types |
| `taskStateMessage.ts` | — | 🏗️ | Task state message |
| `taskSummary.ts` | — | 🏗️ | Task summary |
| `userPromptKeywords.ts` | — | 🏗️ | User prompt keyword detection |
| `transcriptSearch.ts` | — | 🏗️ | Transcript search |
| `statusNoticeDefinitions.tsx` | — | 🏗️ | Status notice definitions (TSX→Rust TUI) |
| `statusNoticeHelpers.ts` | — | 🏗️ | Status notice helpers |
| `billing.ts` | Engine | ✅ | Billing/token tracking |
| `extraUsage.ts` | Engine | ✅ | Extra usage tracking |
| `tokenBudget.ts` | Engine | ✅ | Token budget management |

---

### `cc-teams` — Team / Swarm / Mailbox

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `swarm/` (directory) | `cc-teams` | 🔶 | Core modules done; pane backends deferred |
| `inProcessTeammateHelpers.ts` | `helpers.rs` | ✅ | In-process teammate helpers |
| `inProcessRunner.ts` | `in_process.rs` | ✅ | In-process runner |
| `teamHelpers.ts` | `helpers.rs` | ✅ | Team file management |
| `team*.ts` | Various | 🔶 | Multiple team files |
| `mailbox.ts` | `mailbox.rs` | ✅ | Mailbox IPC |
| `reconnection.ts` | — | 🏗️ | Swarm reconnection |
| `permissionSync.ts` | — | 🏗️ | Permission sync (new `permission_sync.rs`) |
| `leaderPermissionBridge.ts` | — | 🏗️ | Leader permission bridge |
| `spawnInProcess.ts` | `in_process.rs` | ✅ | In-process spawn |
| `spawnUtils.ts` | `team_spawn.rs` | ✅ | Spawn utilities |
| `teammateInit.ts` | — | 🏗️ | Teammate initialization |
| `teammateLayoutManager.ts` | — | 🏗️ | Layout management |
| `teammateModel.ts` | `types.rs` | ✅ | Model types |
| `teammatePromptAddendum.ts` | — | 🏗️ | Prompt addendum |
| `constants.ts` | `constants.rs` | ✅ | Team constants |
| TmuxBackend.ts | — | ⏳ | Full Build gap; pane backend implementation pending |
| ITermBackend.ts | — | ⏳ | Full Build gap; pane backend implementation pending |
| InProcessBackend.ts | `in_process.rs` | ✅ | |
| PaneBackendExecutor.ts | — | ⏳ | Full Build gap; pane executor pending |
| WindowsTerminalBackend.ts | — | ⏳ | Full Build gap; pane backend implementation pending |
| registry.ts | — | ⏳ | Full Build gap; backend registry pending |
| detection.ts | — | ⏳ | Full Build gap; backend detection pending |

---

### `cc-session` / `cc-state` — Session Persistence

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `sessionActivity.ts` | `cc-session` | ✅ | Session activity tracking |
| `sessionDataUploader.ts` | — | 🏗️ | Session data upload |
| `sessionEnvironment.ts` | — | 🏗️ | Session environment |
| `sessionEnvVars.ts` | — | 🏗️ | Session env vars |
| `sessionFileAccessHooks.ts` | — | 🏗️ | Session file access hooks |
| `sessionIngressAuth.ts` | — | 🏗️ | Session ingress auth |
| `sessionRestore.ts` | — | 🏗️ | Session restoration |
| `sessionStart.ts` | — | 🏗️ | Session startup logic |
| `sessionState.ts` | — | 🏗️ | Session state management |
| `sessionStorage.ts` | — | 🏗️ | Session storage |
| `sessionStoragePortable.ts` | — | 🏗️ | Portable session storage |
| `sessionTitle.ts` | — | 🏗️ | Session title generation |
| `sessionUrl.ts` | — | 🏗️ | Session URL |
| `concurrentSessions.ts` | — | 🏗️ | Concurrent session management |
| `crossProjectResume.ts` | — | 🏗️ | Cross-project resume |
| `ccshareResume.ts` | — | 🏗️ | CC Share resume |

---

### Root TUI — Terminal Rendering

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `ansiToPng.ts` | — | 🏗️ | ANSI to PNG rendering |
| `ansiToSvg.ts` | — | 🏗️ | ANSI to SVG rendering |
| `sliceAnsi.ts` | — | 🏗️ | ANSI slicing |
| `terminal.ts` | Terminal detection | ✅ | In cc-config or TUI crate |
| `claudeDesktop.ts` | — | 🏗️ | Claude Desktop integration |
| `fullscreen.ts` | — | 🏗️ | Fullscreen mode |
| `ink.tsx` | — | 🏗️ | Ink renderer (React→Rust TUI) |
| `staticRender.tsx` | — | 🏗️ | Static rendering |
| `exportRenderer.tsx` | — | 🏗️ | Export rendering |
| `highlightMatch.tsx` | — | 🏗️ | Match highlighting |
| `theme.ts` | TUI theme | ✅ | Theme support |
| `keyboardShortcuts.ts` | Keybindings | ✅ | In `cc-keybindings` |
| `hotkeys.ts` | Keybindings | ✅ | Hotkey support |
| `status.tsx` | Status bar | ✅ | In Rust TUI |
| `horizontalScroll.ts` | — | 🏗️ | Horizontal scroll |
| `terminalPanel.ts` | — | 🏗️ | Terminal panel |
| `image*.ts` | — | 🏗️ | Image rendering helpers |

---

### `cc-permissions` — Permissions & Safety

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `autoModeDenials.ts` | `cc-permissions` | ✅ | Auto-mode denial logic |
| `sandbox/` | `cc-sandbox` | ✅ | Sandbox implementation |
| `shell/readOnlyCommandValidation.ts` | Classifier | ✅ | Command risk classification |

---

### `cc-observability` — Telemetry & Debug

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `telemetry/` | — | 🏗️ | Telemetry (partial in cc-observability) |
| `telemetryAttributes.ts` | — | 🏗️ | Telemetry attributes |
| `errors.ts` | Error types | ✅ | In cc-types/cc-engine |
| `errorLogSink.ts` | — | 🏗️ | Error log sink |
| `debug.ts` | Tracing | ✅ | `tracing` crate based |
| `debugFilter.ts` | — | 🏗️ | Debug filtering |
| `warningHandler.ts` | — | 🏗️ | Warning handling |
| `sentry.ts` | — | 🚫 | Not ported (cc-rust doesn't use Sentry) |
| `fpsTracker.ts` | — | 🏗️ | FPS tracking |
| `startupProfiler.ts` | — | 🏗️ | Startup profiling |
| `headlessProfiler.ts` | — | 🏗️ | Headless profiling |
| `heapDumpService.ts` | — | 🚫 | Not applicable in Rust |
| `eventLoopStallDetector.ts` | — | 🚫 | Not applicable (tokio runtime) |

---

### `cc-mcp` — MCP Protocol

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `mcpInstructionsDelta.ts` | — | 🏗️ | MCP instruction delta |
| `mcpOutputStorage.ts` | — | 🏗️ | MCP output storage |
| `mcpValidation.ts` | — | 🏗️ | MCP validation |
| `mcpWebSocketTransport.ts` | — | 🏗️ | MCP WebSocket transport |

---

### `cc-commands` — CLI / Slash Commands

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `cliArgs.ts` | `claude-code-rs` args | ✅ | CLI argument parsing |
| `cliHighlight.ts` | TUI | ✅ | CLI highlighting |
| `cliLaunch.ts` | Main | ✅ | CLI launch logic |
| `exampleCommands.ts` | — | 🏗️ | Example commands |

---

### `cc-daemon` / `gateway` — Background Services

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `lanBeacon.ts` | Gateway | ✅ | LAN beacon |
| `managedEnv*.ts` | Gateway | ✅ | Managed environment |

---

### `cc-sandbox` — Sandbox

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `sandbox/` | `cc-sandbox` | ✅ | Full sandbox implementation |

---

### `cc-tasks` — Task System

| Bun file | Rust equivalent | Status | Notes |
|---|---|---|---|
| `task/` | `cc-tasks` | ✅ | Task system |
| `generatedFiles.ts` | — | 🏗️ | Generated file management |

---

### Deferred Full Build Gaps — Still Need Migration

These files are not implemented in this batch, but they remain required for
Full Build parity unless a future PR explicitly documents a true Intentional
decision.

| Bun file | Remaining work |
|---|---|
| `TmuxBackend.ts` | Implement tmux pane create/show/hide/kill/send-command/rebalance behavior |
| `ITermBackend.ts` | Implement iTerm2 setup and pane control or document platform-specific unsupported behavior |
| `WindowsTerminalBackend.ts` | Implement Windows Terminal backend or document platform-specific unsupported behavior |
| `PaneBackendExecutor.ts` | Add pane executor abstraction for terminal-backed teammates |
| `registry.ts` | Add backend registry and mode resolution |
| `detection.ts` | Add backend/platform detection |

---

### Intentional Gaps — Not Ported

These Bun files will **not** be ported because the functionality is absent or provided differently in Rust:

| Bun file | Why not ported |
|---|---|
| `sentry.ts` | cc-rust does not use Sentry; observability via `tracing` |
| `heapDumpService.ts` | Rust memory model doesn't require JS-style heap dumps |
| `eventLoopStallDetector.ts` | tokio runtime handles this differently |
| `deepLink/**` | Deep link handling depends on platform-specific mechanisms |
| `desktopDeepLink.ts` | Same as above |
| `dxt/**` | DXT tooling not applicable |
| `secureStorage/**` | cc-auth uses keyring; different approach |
| `shell/readOnlyCommandValidation.ts` | Already handled by `cc-permissions` classifier |

---

## Batch 1 Fix Notes And Remaining Work

This section records the cleanup made after the first code review of the
`core-utilities` worktree.

Resolved:
- `djb2_hash` now mirrors Bun `djb2Hash()` semantics: UTF-16 code units,
  signed 32-bit wrapping arithmetic, and regression values for `abc`, `xyz`,
  and `hello`.
- `hash_content` and `hash_pair` are documented as Rust-local cache-key helpers,
  not Bun-compatible `hashContent()` / `hashPair()` replacements.
- `cc-utils` package metadata no longer claims file-state ownership.
- The stale `lru` dependency was removed from `cc-utils` after deleting the
  unused LRU-backed `file_state_cache.rs`.
- Team pane backends moved from `Intentional Gap` to `Deferred Full Build Gap`.

Remaining:
- If cross-runtime hash parity is required for `hashContent()` or `hashPair()`,
  add explicit stable string-returning helpers instead of reusing
  `hash_content` / `hash_pair`.
- `cc-tools::tool::FileStateCache::hash_content` still owns file edit/read
  change detection; it intentionally remains local to `cc-tools` until a real
  shared caller appears.
- Full Team/Swarm parity still requires pane backend architecture, backend
  registry/detection, and concrete tmux/iTerm2/Windows Terminal implementations.

## Dependency Note: file_state_cache.rs

`cc-utils` previously had its own `file_state_cache.rs` with an LRU-backed `FileStateCache`. This was a **duplicate** of the canonical `cc-tools::tool::FileStateCache` (HashMap+RwLock). All workspace callers use `cc-tools::tool::FileStateCache`:

- `cc-tools::tool::FileStateCache` — 15+ call sites across `cc-tools`, `cc-engine`, `cc-teams`, `cc-plugins`, `worktree`
- `cc-utils::file_state_cache::FileStateCache` — 0 external callers

**Decision**: Removed `cc-utils` version entirely. Callers should use `cc_tools::tool::FileStateCache`.

---

## Migration Priority (Remaining)

| Priority | Area | Owner | Effort |
|---|---|---|---|
| P0 | String utilities (`stringUtils.ts`) | `cc-utils` | Small |
| P0 | UUID helpers (`uuid.ts`) | `cc-utils` | Small |
| P1 | Tool schema cache | `cc-tools` | Medium |
| P1 | File history | `cc-tools` | Small |
| P2 | Session persistence | `cc-session` | Large |
| P2 | Telemetry | `cc-observability` | Medium |
| P3 | MCP utilities | `cc-mcp` | Medium |
| P3 | Terminal rendering | `crates/claude-code-rs/src/ui/` | Large |
