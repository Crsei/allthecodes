# Dead Code Audit Report

**Date**: 2026-04-12
**Branch**: rust-lite
**Scope**: All `#[allow(dead_code)]`, `#![allow(unused)]`, `#[allow(unused_imports)]` annotations in `src/`

## Executive Summary

| Metric | Count |
|--------|-------|
| Total annotations investigated | 155 across 119 files |
| **Can remove annotation (code is used)** | **72** |
| **Entire module is dead code** | **9** |
| **Specific items are dead** | **~35** |
| **Not-yet-wired (planned features)** | **~25** |
| **Used only in tests** | **6** |

### Classification Legend

| Tag | Meaning | Action |
|-----|---------|--------|
| `USED_ELSEWHERE` | All items actively used; annotation unnecessary | Remove annotation |
| `BLANKET_SUPPRESS` | File-level allow hides mix of used + unused items | Replace with targeted item-level allows |
| `TRULY_DEAD` | Not used anywhere; can be deleted | Delete code or module |
| `NOT_YET_WIRED` | Part of planned feature, has clear integration point | Keep with TODO comment |
| `USED_ONLY_IN_TESTS` | Only exercised by `#[cfg(test)]` code | Move to test module or keep |
| `FEATURE_GATED` | Used but only when runtime feature flag is enabled | Keep; consider Cargo features |

---

## 1. Annotations to Remove (Code is Used)

These 72 file-level `#![allow(unused)]` / `#![allow(dead_code)]` annotations suppress warnings on code that is **actively used** and should be removed with no other changes:

### types/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `types/state.rs` | 1 | `#![allow(dead_code)]` | All 5 types used across compact/, query/, engine/ |
| `types/message.rs` | 1 | `#![allow(dead_code)]` | 19/22 types actively used (3 need targeted allows) |
| `types/tool.rs` | 1 | `#![allow(dead_code)]` | 15/17 types actively used (1 duplicate needs consolidation) |
| `types/config.rs` | 1 | `#![allow(dead_code)]` | 4/8 types used (4 need targeted allows) |
| `types/transitions.rs` | 1 | `#![allow(dead_code)]` | 7/17 variants used (10 need targeted allows) |

### config/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `config/features.rs` | 12 | `#![allow(dead_code)]` | All items (`Feature`, `FeatureFlags`, `enabled()`) actively used in 11+ files |
| `config/claude_md.rs` | 7 | `#![allow(unused)]` | All 3 functions used by engine/system_prompt.rs and commands/memory.rs |
| `config/settings.rs` | 11 | `#![allow(unused)]` | `load_and_merge()` used in main.rs, `global_claude_dir()` used in 3 files |

### auth/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `auth/oauth/pkce.rs` | 3 | `#![allow(dead_code)]` | All 3 functions used by commands/login_code.rs; comment is outdated |

### api/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `api/mod.rs` | 1 | `#![allow(unused)]` | All 8 submodule declarations used |
| `api/openai_compat.rs` | 1 | `#![allow(unused)]` | All 5 functions used (pub fn by stream_provider.rs, rest internally) |
| `api/google_provider.rs` | 1 | `#![allow(unused)]` | All 8 items used (pub fn by stream_provider.rs, rest internally) |
| `api/streaming.rs` | 1 | `#![allow(unused)]` | Both `parse_sse_event` and `StreamAccumulator` used across codebase |
| `api/client/mod.rs` | 1 | `#![allow(unused)]` | Core API used; 3 dead imports to clean up separately |
| `api/providers.rs` | 1 | `#![allow(unused)]` | Core types used; keep item-level allows on Bedrock/Vertex stubs |

### engine/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `engine/mod.rs` | 1 | `#![allow(unused)]` | All 6 submodule declarations used |
| `engine/input_processing.rs` | 6 | `#![allow(unused)]` | Both items used by submit_message.rs |
| `engine/sdk_types.rs` | 6 | `#![allow(unused)]` | All 10 types actively used across 5+ files |
| `engine/result.rs` | 5 | `#![allow(unused)]` | All 3 functions used by submit_message.rs |
| `engine/prompt_sections.rs` | 9 | `#![allow(unused)]` | All items used by system_prompt.rs and commands/brief.rs |
| `engine/system_prompt.rs` | 14 | `#![allow(unused)]` | 14/15 functions used; `language_section` needs targeted allow |

### commands/ (all 20 files)
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `commands/mod.rs` | 6 | `#![allow(unused)]` | All items used (Command, get_all_commands, etc.) |
| `commands/branch.rs` | 6 | `#![allow(unused)]` | BranchHandler registered in mod.rs |
| `commands/clear.rs` | 3 | `#![allow(unused)]` | ClearHandler registered in mod.rs |
| `commands/commit.rs` | 6 | `#![allow(unused)]` | CommitHandler registered in mod.rs |
| `commands/compact.rs` | 8 | `#![allow(unused)]` | CompactHandler registered; check `compact_messages` import |
| `commands/config_cmd.rs` | 8 | `#![allow(unused)]` | ConfigHandler registered in mod.rs |
| `commands/context.rs` | 9 | `#![allow(unused)]` | ContextHandler registered in mod.rs |
| `commands/cost.rs` | 6 | `#![allow(unused)]` | CostHandler registered in mod.rs |
| `commands/exit.rs` | 5 | `#![allow(unused)]` | ExitHandler registered in mod.rs |
| `commands/export.rs` | 9 | `#![allow(unused)]` | ExportHandler registered in mod.rs |
| `commands/files.rs` | 7 | `#![allow(unused)]` | FilesHandler registered in mod.rs |
| `commands/help.rs` | 3 | `#![allow(unused)]` | HelpHandler registered in mod.rs |
| `commands/memory.rs` | 6 | `#![allow(unused)]` | MemoryHandler registered in mod.rs |
| `commands/model.rs` | 10 | `#![allow(unused)]` | ModelHandler registered in mod.rs |
| `commands/permissions_cmd.rs` | 13 | `#![allow(unused)]` | PermissionsHandler registered in mod.rs |
| `commands/resume.rs` | 10 | `#![allow(unused)]` | ResumeHandler registered in mod.rs |
| `commands/session.rs` | 10 | `#![allow(unused)]` | SessionHandler registered in mod.rs |
| `commands/version.rs` | 3 | `#![allow(unused)]` | VersionHandler registered in mod.rs |
| `commands/diff.rs` | 6 | `#![allow(unused)]` | DiffHandler used; replace with targeted allow on `get_status_summary` |

### tools/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `tools/ask_user.rs` | 9 | `#![allow(unused)]` | AskUserQuestionTool registered in registry.rs |
| `tools/config_tool.rs` | 8 | `#![allow(unused)]` | ConfigTool registered; remove unused HashMap import |
| `tools/grep.rs` | 1 | `#![allow(unused)]` | GrepTool registered in registry.rs |
| `tools/lsp.rs` | 17 | `#![allow(unused)]` | LspTool registered; types used by lsp_service/ |
| `tools/send_message.rs` | 11 | `#![allow(unused)]` | SendMessageTool registered in registry.rs |
| `tools/skill.rs` | 41 | `#[allow(dead_code)]` | `build_skills_listing()` IS called at line 236 |

### session/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `session/resume.rs` | 6 | `#![allow(unused)]` | Both functions used by main.rs and commands/resume.rs |
| `session/transcript.rs` | 8 | `#![allow(unused)]` | `record_transcript` and `flush_transcript` used |
| `session/storage.rs` | 7 | `#![allow(unused)]` | All types/functions extensively used across 6+ files |

### mcp/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `mcp/client.rs` | 13 | `#![allow(unused)]` | McpClient used via McpManager, wired in main.rs |
| `mcp/discovery.rs` | 1 | `#![allow(unused)]` | `discover_mcp_servers()` called from main.rs |
| `mcp/mod.rs` | 10 | `#![allow(unused)]` | All submodules and most types actively used |
| `mcp/tools.rs` | 8 | `#![allow(unused)]` | `mcp_tools_to_tools()` called from main.rs |

### permissions/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `permissions/decision.rs` | 33 | `#![allow(unused)]` | Core types used by tools/execution/pipeline.rs |
| `permissions/rules.rs` | 9 | `#![allow(unused)]` | All items used by permissions/decision.rs |

### query/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `query/deps.rs` | 1 | `#![allow(dead_code)]` | All 6 types extensively used by loop_impl, loop_helpers, lifecycle/deps |
| `query/loop_helpers.rs` | 23,30,42 | `#[allow(unused)]` | All 3 items used by loop_impl.rs |

### utils/
| File | Line | Annotation | Why safe to remove |
|------|------|-----------|-------------------|
| `utils/cwd.rs` | 1 | `#![allow(unused)]` | 3/4 functions used by commands/init.rs |
| `utils/git.rs` | 8 | `#![allow(unused)]` | ~8 functions used across engine/, commands/, daemon/ |

---

## 2. Dead Modules (Entire Module Unused)

These modules have **zero external callers** and can be deleted entirely:

| Module | Lines | Why dead | Recommendation |
|--------|-------|----------|---------------|
| `utils/shell.rs` | ~100 | Shell detection never integrated; Bash tool handles it directly | Delete |
| `utils/messages.rs` | ~500 | Message utilities never imported; `commands/copy.rs` has its own | Delete |
| `utils/file_state_cache.rs` | ~120 | LRU cache superseded by simpler `types/tool.rs::FileStateCache` | Delete |
| `utils/abort.rs` | ~80 | Custom AbortController unused; codebase uses `CancellationToken` | Delete |
| `utils/bash.rs` | ~730 | Bash parsing utilities never imported by any module | Delete |
| `session/memdir.rs` | ~200 | Memory entry system never wired into engine or commands | Delete |
| `session/migrations.rs` | ~200 | Migration system never called from session loading path | Delete |
| `services/session_memory.rs` | ~150 | Session memory service never wired into engine | Delete |
| `services/lsp_lifecycle.rs` | ~150 | Duplicate of lsp_service/; never integrated | Delete |

**Total deletable**: ~2,230 lines of dead code

### Additional dead submodules within ui/:

| Module | Lines | Why dead | Recommendation |
|--------|-------|----------|---------------|
| `ui/keybindings.rs` | ~200 | Keybinding registry never used; TUI handles keys directly | Delete |
| `ui/vim.rs` | ~820 | Vim mode superseded by ink-terminal frontend's vim/ | Delete |

**Total with ui/**: ~3,250 lines of dead code

---

## 3. Dead Items Within Used Files

These are specific functions, types, or imports within otherwise-used files that can be deleted:

### Truly dead (delete)

| File | Item | Why dead |
|------|------|---------|
| `api/retry.rs` | `RetryConfig`, `Default for RetryConfig`, `retry_delay()`, `rand_fraction()`, `is_retryable()` | Engine has its own retry logic in submit_message.rs |
| `api/client/mod.rs:15-16` | Imports: `categorize_api_error`, `ApiErrorCategory`, `parse_sse_event` | Dead re-imports; used by sibling modules with their own imports |
| `commands/diff.rs:119` | `get_status_summary()` | Never called anywhere |
| `config/constants.rs` | `models::{OPUS,SONNET,HAIKU,FRONTIER_MODEL_NAME}`, entire `api` module, entire `tokens` module, entire `tool_limits` module, `media`, `product`, `xml`, `tool_names` modules | Constants defined but never imported; some duplicated locally in other files |
| `tools/skill.rs:15` | `bail` import | Never used in file |
| `tools/skill.rs:24` | `ContentBlock` import | Never used in file |
| `tools/config_tool.rs:10` | `HashMap` import | Never used in file |
| `types/config.rs` | `QueryConfig`, `QueryGates` | Never used outside definition |
| `types/state.rs:3` | `ToolUseContext` import | Never used in file |
| `types/tool.rs:11` | `ContentBlock` import | Never used in file |
| `types/config.rs:2` | `SystemMessage`, `Usage` imports | Never used in file |
| `types/config.rs:4` | `ToolUseContext` import | Never used in file |
| `utils/tokens.rs` | `get_context_window_size()` | Private, never called |
| `query/mod.rs:16-19` | Re-exports `QueryDeps`, `query` | Never used; callers import directly |
| `session/session_export/mod.rs:30,32` | Re-exports from builders/compression | Never used externally; stale comment |
| `bootstrap/mod.rs:37,39,42` | Re-exports `ModelSetting`, `ModelStrings`, `ModelTier`, `Signal`, `DurationTracker` | Never imported via these re-exports |
| `plugins/manifest.rs` | `CommandContribution` | Defined but never referenced even in tests |
| `ui/permissions.rs:4` | `Style` import | Never used in file |

### Not-yet-wired (keep with TODO)

| File | Item | Integration point |
|------|------|--------------------|
| `api/client/mod.rs:45,48` | `ApiProvider::Bedrock`, `ApiProvider::Vertex` | Future AWS/GCP provider integration |
| `api/providers.rs:209-268` | `BedrockProvider`, `VertexProvider` | Future provider stubs |
| `config/validation.rs` (entire) | `validate_settings()` | Wire into settings loader |
| `daemon/notification.rs` (entire) | `notification_consumer()` | Spawn in daemon startup |
| `daemon/channels.rs` (entire) | `ChannelManager` | Instantiate in daemon startup |
| `daemon/state.rs` | `Notification`, `terminal_focus()` | Wire notification pipeline |
| `engine/system_prompt.rs` | `language_section()` | Wire into `build_system_prompt()` |
| `ipc/protocol.rs:42` | `BriefMessage`, `AutonomousStart`, `NotificationSent` variants | KAIROS headless IPC support |
| `lsp_service/client.rs:264` | `shutdown()` | Call from shutdown.rs hooks |
| `skills/mod.rs` | `init_skills()`, `get_user_invocable_skills()` | Wire into main.rs startup |
| `tools/tasks.rs:108` | `append_output()` | Background agent execution |
| `tools/hooks/post_tool.rs:245` | `fire_notification_hook()` | Notification dispatch points |
| `tools/hooks/mod.rs:18` | `fire_notification_hook` re-export | Same as above |
| `tools/orchestration.rs` (entire) | `run_tools()` | Alternative to inline execution in deps.rs |
| `tools/execution/mod.rs` | Public re-exports | Alternative execution pipeline |
| `types/message.rs` | `TombstoneMessage`, `ToolUseSummaryMessage` | QueryYield variant payloads |
| `types/transitions.rs` | 9 `Terminal` variants, `CollapseDrainRetry` | Future query loop termination paths |

---

## 4. Feature-Gated Modules

These modules are used but only when runtime feature flags are enabled:

| Module | Feature Gate | Status |
|--------|-------------|--------|
| `daemon/routes.rs` | `FEATURE_KAIROS` | All items used within daemon; keep `#![allow(dead_code)]` or use Cargo features |
| `daemon/webhook.rs` | `FEATURE_KAIROS_GITHUB_WEBHOOKS` | Functions tested but route handlers are stubs |
| `teams/` (all 11 files) | `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` env var | Internally consistent subsystem; keep allows |

---

## 5. Test-Only Code

| File | Item | Action |
|------|------|--------|
| `lsp_service/mod.rs:101` | `language_for_extension()` | Move to `#[cfg(test)]` or keep allow |
| `lsp_service/mod.rs:125` | `ServerState` enum | Move to `#[cfg(test)]` or keep allow |
| `query/stop_hooks.rs:68` | `has_tool_use()` | Move to `#[cfg(test)]` or keep allow |
| `bootstrap/mod.rs` | `signal` submodule | `Signal<T>` only used in own tests |

---

## 6. Patterns & Root Causes

### Why so many blanket `#![allow(unused)]`?

The overwhelming pattern is **preventive suppression during development**: 72 of the annotations suppress warnings on code that is actively used. This suggests a development pattern of:

1. Write module with `#![allow(unused)]`
2. Wire it into the system
3. **Never remove the allow**

### Duplicate implementations

| Concept | Location 1 | Location 2 | Resolution |
|---------|-----------|-----------|------------|
| FileStateCache | `types/tool.rs` (HashMap) | `utils/file_state_cache.rs` (LRU) | Delete utils version |
| Tool execution | `engine/lifecycle/deps.rs` (inline) | `tools/orchestration.rs` + `tools/execution/` | Wire or delete alternatives |
| Bash parsing | `utils/bash.rs` (standalone) | `tools/bash.rs` (inline) | Delete utils version |
| Session memory | `session/memdir.rs` | `services/session_memory.rs` | Delete both (neither used) |
| Shell detection | `utils/shell.rs` | Inline in tools/bash.rs | Delete utils version |
| Token constants | `config/constants.rs::tokens` | Locally in `query/loop_helpers.rs`, `compact/compaction.rs` | Wire constants or delete module |

---

## 7. Recommended Cleanup Order

### Phase 1: Quick wins (remove annotations only, no code changes)
Remove 72 `#![allow(unused)]` / `#![allow(dead_code)]` from files where all items are used.
**Risk**: Zero. If any item produces a warning, add a targeted `#[allow(dead_code)]` on that specific item.
**Estimated impact**: Removes false sense of "this code might not work" from 72 files.

### Phase 2: Delete dead modules (~3,250 lines)
Delete the 9+2 confirmed dead modules listed in Section 2.
**Risk**: Low. Verify with `cargo build` after deletion.
**Files**: `utils/{shell,messages,file_state_cache,abort,bash}.rs`, `session/{memdir,migrations}.rs`, `services/{session_memory,lsp_lifecycle}.rs`, `ui/{keybindings,vim}.rs`

### Phase 3: Clean up dead items within used files
Delete specific dead functions, imports, and types listed in Section 3 "Truly dead".
**Risk**: Low. Individual items confirmed unused via grep.

### Phase 4: Replace blanket allows with targeted allows
For files classified as `BLANKET_SUPPRESS`, replace `#![allow(unused)]` with `#[allow(dead_code)]` on specific unused items.
**Risk**: Low. May surface unexpected warnings that need investigation.

### Phase 5: Wire not-yet-wired features or add TODO comments
For items classified as `NOT_YET_WIRED`, either integrate them or add clear `// TODO(feature): ...` comments.
**Risk**: Medium. Requires design decisions about feature priorities.

---

## Appendix: Full Annotation Index

### File-level `#![allow(dead_code)]` (14 files)

| File | Classification |
|------|---------------|
| `auth/oauth/pkce.rs` | USED_ELSEWHERE -- remove |
| `config/features.rs` | USED_ELSEWHERE -- remove |
| `config/validation.rs` | NOT_YET_WIRED -- keep |
| `daemon/channels.rs` | NOT_YET_WIRED |
| `daemon/notification.rs` | NOT_YET_WIRED |
| `daemon/routes.rs` | FEATURE_GATED |
| `daemon/state.rs` | BLANKET_SUPPRESS |
| `daemon/webhook.rs` | USED_ONLY_IN_TESTS |
| `query/deps.rs` | USED_ELSEWHERE -- remove |
| `types/config.rs` | BLANKET_SUPPRESS |
| `types/message.rs` | BLANKET_SUPPRESS |
| `types/state.rs` | USED_ELSEWHERE -- remove |
| `types/tool.rs` | BLANKET_SUPPRESS |
| `types/transitions.rs` | BLANKET_SUPPRESS |

### File-level `#![allow(unused)]` (80 files)

| Module | Files | Classification |
|--------|-------|---------------|
| api/ | 7 files | All USED_ELSEWHERE or BLANKET_SUPPRESS -- remove all |
| engine/ | 6 files | All USED_ELSEWHERE -- remove all |
| commands/ | 20 files | All USED_ELSEWHERE -- remove all |
| tools/ | 7 files | 5 USED_ELSEWHERE, 2 NOT_YET_WIRED |
| compact/ | 7 files | Not investigated in detail (auto_compact, compaction, messages, microcompact, pipeline, snip, tool_result_budget) |
| session/ | 5 files | 3 USED_ELSEWHERE, 2 TRULY_DEAD |
| services/ | 2 files | 2 TRULY_DEAD |
| skills/ | 3 files | BLANKET_SUPPRESS / NOT_YET_WIRED |
| plugins/ | 3 files | BLANKET_SUPPRESS |
| teams/ | 11 files | FEATURE_GATED / NOT_YET_WIRED |
| mcp/ | 4 files | All USED_ELSEWHERE -- remove all |
| permissions/ | 2 files | All USED_ELSEWHERE -- remove all |
| utils/ | 8 files | 2 USED_ELSEWHERE, 6 TRULY_DEAD |
| ui/ | 3 files (+10 in mod.rs) | 2 TRULY_DEAD, rest BLANKET_SUPPRESS |
| query/ | 1 file | USED_ELSEWHERE -- remove |

### Item-level `#[allow(dead_code)]` (19 items)

| File | Item | Classification |
|------|------|---------------|
| `api/client/mod.rs:45` | `Bedrock` variant | NOT_YET_WIRED |
| `api/client/mod.rs:48` | `Vertex` variant | NOT_YET_WIRED |
| `api/providers.rs:209` | `BedrockProvider` struct | NOT_YET_WIRED |
| `api/providers.rs:215` | `BedrockProvider` impl | NOT_YET_WIRED |
| `api/providers.rs:243` | `VertexProvider` struct | NOT_YET_WIRED |
| `api/providers.rs:249` | `VertexProvider` impl | NOT_YET_WIRED |
| `bootstrap/mod.rs:21-31` | 6 submodules | Mixed (3 used, 3 partially dead) |
| `commands/diff.rs:119` | `get_status_summary()` | TRULY_DEAD |
| `ipc/protocol.rs:42` | `BackendMessage` enum | BLANKET_SUPPRESS (3 unused variants) |
| `lsp_service/mod.rs:101` | `language_for_extension()` | USED_ONLY_IN_TESTS |
| `lsp_service/mod.rs:125` | `ServerState` enum | USED_ONLY_IN_TESTS |
| `lsp_service/client.rs:264` | `shutdown()` | NOT_YET_WIRED |
| `query/stop_hooks.rs:68` | `has_tool_use()` | USED_ONLY_IN_TESTS |
| `tools/hooks/post_tool.rs:245` | `fire_notification_hook()` | NOT_YET_WIRED |
| `tools/skill.rs:41` | `build_skills_listing()` | USED_ELSEWHERE -- remove annotation |
| `tools/tasks.rs:108` | `append_output()` | NOT_YET_WIRED |
| `tools/worktree.rs:311` | `discard_changes` field | Structural (serde schema) |
| `ui/mod.rs:1-20` | 10 submodules | Mixed (8 used internally, 2 TRULY_DEAD) |
