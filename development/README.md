# Development Docs Index

This directory is the working index for implementation references, migration notes, plans, debug logs, UI analyses, and archived records.

## How to use this tree

- `reference/` holds active technical references and cross-project comparisons.
- `archive/` holds historical plans, debug notes, completed analyses, and frozen records.
- [README.md](README.md)
- Keep new active references in `reference/` and move closed material into the relevant `archive/` subdirectory.
- Prefer relative links so each directory stays self-contained when moved.

## `reference/`

- [reference/CRATE_DEPENDENCY_TARGETS.md](reference/CRATE_DEPENDENCY_TARGETS.md)
- [reference/CRATE_MIGRATION_GUIDE.md](reference/CRATE_MIGRATION_GUIDE.md)
- [reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md](reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md)
- [reference/CRATE_MIGRATION_TARGET_STATE.md](reference/CRATE_MIGRATION_TARGET_STATE.md)
- [reference/Codex_SDK_Features.md](reference/Codex_SDK_Features.md)
- [reference/DAEMON_OPERATIONS.md](reference/DAEMON_OPERATIONS.md)
- [reference/REMOTE_CONTROL_GATEWAY.md](reference/REMOTE_CONTROL_GATEWAY.md)
- [reference/agent-extension-installation.md](reference/agent-extension-installation.md)
- [reference/anthropic_coding.md](reference/anthropic_coding.md)
- [reference/bootstrap-ipc-daemon-relationship.md](reference/bootstrap-ipc-daemon-relationship.md)
- [reference/browser-mcp-config.md](reference/browser-mcp-config.md)
- [reference/chrome-native-host.md](reference/chrome-native-host.md)
- [reference/claude-code-bun-entrypoints-ui-structure.md](reference/claude-code-bun-entrypoints-ui-structure.md)
- [reference/computer-use-mcp-config.md](reference/computer-use-mcp-config.md)
- [reference/headless-ipc-protocol.md](reference/headless-ipc-protocol.md)
- [reference/remote-control-current-state.md](reference/remote-control-current-state.md)
- [reference/rust-vs-claude-code-bun-communication-message-mapping.md](reference/rust-vs-claude-code-bun-communication-message-mapping.md)
- [reference/snapshot-testing-guide.md](reference/snapshot-testing-guide.md)
- [reference/teammem-analysis.md](reference/teammem-analysis.md)
- [reference/tool-comparison-3projects.md](reference/tool-comparison-3projects.md)
- [reference/webui-adapters-and-cc-ipc-structure.md](reference/webui-adapters-and-cc-ipc-structure.md)

### `reference/anthropic/`

- [reference/anthropic/api/overview.md](reference/anthropic/api/overview.md)
- [reference/anthropic/changlog.md](reference/anthropic/changlog.md)
- [reference/anthropic/prompt_cache/overview.md](reference/anthropic/prompt_cache/overview.md)
- [reference/anthropic/what's_changed.md](reference/anthropic/what's_changed.md)

## `archive/`

- [archive/CLI_REFERENCE.md](archive/CLI_REFERENCE.md)
- [archive/COMMAND_REFERENCE.md](archive/COMMAND_REFERENCE.md)
- [archive/COMMAND_UI_REFERENCE.md](archive/COMMAND_UI_REFERENCE.md)
- [archive/FINAL_RELEASE_PLAN.md](archive/FINAL_RELEASE_PLAN.md)
- [archive/IMPLEMENTATION_GAPS.md](archive/IMPLEMENTATION_GAPS.md)
- [archive/KNOWN_ISSUES.md](archive/KNOWN_ISSUES.md)
- [archive/RATATUI_UI_PARITY.md](archive/RATATUI_UI_PARITY.md)
- [archive/UNUSED_CODE_REPORT.md](archive/UNUSED_CODE_REPORT.md)
- [archive/USAGE_GUIDE.md](archive/USAGE_GUIDE.md)

### `archive/agent-handoff/`

- [archive/agent-handoff/core-utilities-agent-brief.md](archive/agent-handoff/core-utilities-agent-brief.md)

### `archive/cache/`

- [archive/cache/langfuse-ccb.md](archive/cache/langfuse-ccb.md)
- [archive/cache/langfuse-improvement-plan.md](archive/cache/langfuse-improvement-plan.md)
- [archive/cache/langfuse-runtime-status.md](archive/cache/langfuse-runtime-status.md)
- [archive/cache/prompt-cache-implementation.md](archive/cache/prompt-cache-implementation.md)

### `archive/claude-code-configuration/`

- [archive/claude-code-configuration/customize-keyboard-shortcuts.md](archive/claude-code-configuration/customize-keyboard-shortcuts.md)
- [archive/claude-code-configuration/customize-status-line.md](archive/claude-code-configuration/customize-status-line.md)
- [archive/claude-code-configuration/fullscreen-rendering.md](archive/claude-code-configuration/fullscreen-rendering.md)
- [archive/claude-code-configuration/model-configuration.md](archive/claude-code-configuration/model-configuration.md)
- [archive/claude-code-configuration/output-styles.md](archive/claude-code-configuration/output-styles.md)
- [archive/claude-code-configuration/permissions.md](archive/claude-code-configuration/permissions.md)
- [archive/claude-code-configuration/sandboxing.md](archive/claude-code-configuration/sandboxing.md)
- [archive/claude-code-configuration/settings.md](archive/claude-code-configuration/settings.md)
- [archive/claude-code-configuration/speed-up-responses-with-fast-mode.md](archive/claude-code-configuration/speed-up-responses-with-fast-mode.md)
- [archive/claude-code-configuration/terminal-configuration.md](archive/claude-code-configuration/terminal-configuration.md)
- [archive/claude-code-configuration/voice-dictation.md](archive/claude-code-configuration/voice-dictation.md)

### `archive/code-split/`

- [archive/code-split/api-tests-split-plan.md](archive/code-split/api-tests-split-plan.md)
- [archive/code-split/dangerous-split-plan.md](archive/code-split/dangerous-split-plan.md)
- [archive/code-split/engine-loop-tests-split-plan.md](archive/code-split/engine-loop-tests-split-plan.md)
- [archive/code-split/main-rs-split-plan.md](archive/code-split/main-rs-split-plan.md)
- [archive/code-split/product-mod-split-plan.md](archive/code-split/product-mod-split-plan.md)
- [archive/code-split/query-loop-tests-split-plan.md](archive/code-split/query-loop-tests-split-plan.md)

### `archive/debug/`

- [archive/debug/crate-migration-allow-audit-2026-05-16.md](archive/debug/crate-migration-allow-audit-2026-05-16.md)
- [archive/debug/logging-architecture.md](archive/debug/logging-architecture.md)
- [archive/debug/model-default-locations.md](archive/debug/model-default-locations.md)
- [archive/debug/pty-command-surface-mcp-actions-2026-05-29.md](archive/debug/pty-command-surface-mcp-actions-2026-05-29.md)
- [archive/debug/pty_tui_e2e-log-review-2026-05-29.md](archive/debug/pty_tui_e2e-log-review-2026-05-29.md)
- [archive/debug/tui-command-init-status-fix-2026-05-22.md](archive/debug/tui-command-init-status-fix-2026-05-22.md)
- [archive/debug/tui-command-model-deepseek-fix-2026-05-23.md](archive/debug/tui-command-model-deepseek-fix-2026-05-23.md)
- [archive/debug/utils-full-build-review-issues-2026-05-19.md](archive/debug/utils-full-build-review-issues-2026-05-19.md)

### `archive/delete/`

- [archive/delete/phase2-deleted-code.md](archive/delete/phase2-deleted-code.md)
- [archive/delete/ui-core-item-allow-2026-05-20.md](archive/delete/ui-core-item-allow-2026-05-20.md)
- [archive/delete/ui-file-level-allow-2026-05-20.md](archive/delete/ui-file-level-allow-2026-05-20.md)
- [archive/delete/ui-item-allow-2026-05-20.md](archive/delete/ui-item-allow-2026-05-20.md)
- [archive/delete/ui-warning-cleanup-agents-theme.md](archive/delete/ui-warning-cleanup-agents-theme.md)

### `archive/http/`

- [archive/http/user-agent-and-custom-agents-gap.md](archive/http/user-agent-and-custom-agents-gap.md)

### `archive/mvp-optimization-plans/`

- [archive/mvp-optimization-plans/MVP-001-api-providers-plan.md](archive/mvp-optimization-plans/MVP-001-api-providers-plan.md)
- [archive/mvp-optimization-plans/MVP-007-tool-search-ranking-plan.md](archive/mvp-optimization-plans/MVP-007-tool-search-ranking-plan.md)
- [archive/mvp-optimization-plans/MVP-010-skill-system-package-plan.md](archive/mvp-optimization-plans/MVP-010-skill-system-package-plan.md)
- [archive/mvp-optimization-plans/mvp-compromise-memory.md](archive/mvp-optimization-plans/mvp-compromise-memory.md)

### `archive/old_archive/`

- [archive/old_archive/AGENT_TEAMS_SPEC.md](archive/old_archive/AGENT_TEAMS_SPEC.md)
- [archive/old_archive/COMPACTION_RETRY_STATE_MACHINE.md](archive/old_archive/COMPACTION_RETRY_STATE_MACHINE.md)
- [archive/old_archive/COMPLETED_FULL.md](archive/old_archive/COMPLETED_FULL.md)
- [archive/old_archive/COMPLETED_SIMPLIFIED.md](archive/old_archive/COMPLETED_SIMPLIFIED.md)
- [archive/old_archive/LIFECYCLE_STATE_MACHINE.md](archive/old_archive/LIFECYCLE_STATE_MACHINE.md)
- [archive/old_archive/MIGRATION_PLAN.md](archive/old_archive/MIGRATION_PLAN.md)
- [archive/old_archive/MODULE_SIMPLIFICATION.md](archive/old_archive/MODULE_SIMPLIFICATION.md)
- [archive/old_archive/P1_EXECUTION_PLAN.md](archive/old_archive/P1_EXECUTION_PLAN.md)
- [archive/old_archive/PROMPT_MIGRATION_GUIDE.md](archive/old_archive/PROMPT_MIGRATION_GUIDE.md)
- [archive/old_archive/PYTHON_SDK_PLAN.md](archive/old_archive/PYTHON_SDK_PLAN.md)
- [archive/old_archive/QUERY_ENGINE_SESSION_LIFECYCLE.md](archive/old_archive/QUERY_ENGINE_SESSION_LIFECYCLE.md)
- [archive/old_archive/STRUCTURE_DIFF.md](archive/old_archive/STRUCTURE_DIFF.md)
- [archive/old_archive/TECH_DEBT.md](archive/old_archive/TECH_DEBT.md)
- [archive/old_archive/TOOL_EXECUTION_STATE_MACHINE.md](archive/old_archive/TOOL_EXECUTION_STATE_MACHINE.md)
- [archive/old_archive/cc-daemon-phase0-baseline-2026-05-13.md](archive/old_archive/cc-daemon-phase0-baseline-2026-05-13.md)
- [archive/old_archive/commands-00-checkpoint-2026-05-11.md](archive/old_archive/commands-00-checkpoint-2026-05-11.md)
- [archive/old_archive/completed-gap-closures-2026-05-07.md](archive/old_archive/completed-gap-closures-2026-05-07.md)
- [archive/old_archive/context-phase0-decisions-2026-05-06.md](archive/old_archive/context-phase0-decisions-2026-05-06.md)
- [archive/old_archive/context-phase1-token-count-provider-matrix-2026-05-06.md](archive/old_archive/context-phase1-token-count-provider-matrix-2026-05-06.md)
- [archive/old_archive/context-phase10-final-verification-2026-05-06.md](archive/old_archive/context-phase10-final-verification-2026-05-06.md)
- [archive/old_archive/context-phase2-memory-recall-2026-05-06.md](archive/old_archive/context-phase2-memory-recall-2026-05-06.md)
- [archive/old_archive/context-phase3-partial-compact-2026-05-06.md](archive/old_archive/context-phase3-partial-compact-2026-05-06.md)
- [archive/old_archive/context-phase4-verification-2026-05-06.md](archive/old_archive/context-phase4-verification-2026-05-06.md)
- [archive/old_archive/context-phase5-token-budget-exact-fallback-2026-05-06.md](archive/old_archive/context-phase5-token-budget-exact-fallback-2026-05-06.md)
- [archive/old_archive/context-phase6-provider-token-parity-2026-05-06.md](archive/old_archive/context-phase6-provider-token-parity-2026-05-06.md)
- [archive/old_archive/context-phase7-partial-compact-command-roundtrip-2026-05-06.md](archive/old_archive/context-phase7-partial-compact-command-roundtrip-2026-05-06.md)
- [archive/old_archive/context-phase7-request-boundary-2026-05-07.md](archive/old_archive/context-phase7-request-boundary-2026-05-07.md)
- [archive/old_archive/context-phase8-model-assisted-memory-recall-2026-05-06.md](archive/old_archive/context-phase8-model-assisted-memory-recall-2026-05-06.md)
- [archive/old_archive/context-phase9-system-prompt-metadata-contract-2026-05-06.md](archive/old_archive/context-phase9-system-prompt-metadata-contract-2026-05-06.md)
- [archive/old_archive/extensibility-phase0-baseline-scope-2026-05-06.md](archive/old_archive/extensibility-phase0-baseline-scope-2026-05-06.md)
- [archive/old_archive/extensibility-phase1-mcp-lifecycle-2026-05-06.md](archive/old_archive/extensibility-phase1-mcp-lifecycle-2026-05-06.md)
- [archive/old_archive/extensibility-phase2-remote-https-sse-2026-05-06.md](archive/old_archive/extensibility-phase2-remote-https-sse-2026-05-06.md)
- [archive/old_archive/extensibility-phase3-mcp-oauth-2026-05-06.md](archive/old_archive/extensibility-phase3-mcp-oauth-2026-05-06.md)
- [archive/old_archive/extensibility-phase4-mcp-streamable-http-2026-05-06.md](archive/old_archive/extensibility-phase4-mcp-streamable-http-2026-05-06.md)
- [archive/old_archive/extensibility-phase5-custom-agent-safety-2026-05-06.md](archive/old_archive/extensibility-phase5-custom-agent-safety-2026-05-06.md)
- [archive/old_archive/extensibility-phase6-integration-closure-2026-05-06.md](archive/old_archive/extensibility-phase6-integration-closure-2026-05-06.md)
- [archive/old_archive/ipc-00-checkpoint-2026-05-11.md](archive/old_archive/ipc-00-checkpoint-2026-05-11.md)
- [archive/old_archive/ratatui-ui-parity-omx-execution-report-2026-05-08.md](archive/old_archive/ratatui-ui-parity-omx-execution-report-2026-05-08.md)
- [archive/old_archive/resolved-known-issues-2026-05-07.md](archive/old_archive/resolved-known-issues-2026-05-07.md)
- [archive/old_archive/resolved-model-context-2026-05-07.md](archive/old_archive/resolved-model-context-2026-05-07.md)
- [archive/old_archive/sdk-work-tracker.md](archive/old_archive/sdk-work-tracker.md)
- [archive/old_archive/tools-phase0-baseline-2026-05-06.md](archive/old_archive/tools-phase0-baseline-2026-05-06.md)
- [archive/old_archive/tools-phase1-task-list-storage-2026-05-06.md](archive/old_archive/tools-phase1-task-list-storage-2026-05-06.md)
- [archive/old_archive/tools-phase2-task-list-lock-2026-05-06.md](archive/old_archive/tools-phase2-task-list-lock-2026-05-06.md)
- [archive/old_archive/tools-phase3-task-v2-schema-2026-05-06.md](archive/old_archive/tools-phase3-task-v2-schema-2026-05-06.md)
- [archive/old_archive/tools-phase4-teammate-unassign-2026-05-06.md](archive/old_archive/tools-phase4-teammate-unassign-2026-05-06.md)
- [archive/old_archive/tools-phase5-web-provider-diff-2026-05-06.md](archive/old_archive/tools-phase5-web-provider-diff-2026-05-06.md)
- [archive/old_archive/tools-phase6-final-verification-2026-05-06.md](archive/old_archive/tools-phase6-final-verification-2026-05-06.md)
- [archive/old_archive/tools-tasks-00-inventory-2026-05-11.md](archive/old_archive/tools-tasks-00-inventory-2026-05-11.md)
- [archive/old_archive/ui-00-checkpoint-2026-05-12.md](archive/old_archive/ui-00-checkpoint-2026-05-12.md)
- [archive/old_archive/workspace-crate-extraction-execution-tracker-2026-05-10.md](archive/old_archive/workspace-crate-extraction-execution-tracker-2026-05-10.md)

### `archive/old_archive/code-split/`

- [archive/old_archive/code-split/client-mod-refactor-plan.md](archive/old_archive/code-split/client-mod-refactor-plan.md)
- [archive/old_archive/code-split/dangerous-refactor-plan.md](archive/old_archive/code-split/dangerous-refactor-plan.md)
- [archive/old_archive/code-split/memdir-refactor-plan.md](archive/old_archive/code-split/memdir-refactor-plan.md)
- [archive/old_archive/code-split/openai_compat-refactor-plan.md](archive/old_archive/code-split/openai_compat-refactor-plan.md)
- [archive/old_archive/code-split/readme.md](archive/old_archive/code-split/readme.md)
- [archive/old_archive/code-split/submit_message-refactor-plan.md](archive/old_archive/code-split/submit_message-refactor-plan.md)
- [archive/old_archive/code-split/system_prompt-refactor-plan.md](archive/old_archive/code-split/system_prompt-refactor-plan.md)

### `archive/old_archive/implemented/`

- [archive/old_archive/implemented/changelog-2026-04-10.md](archive/old_archive/implemented/changelog-2026-04-10.md)
- [archive/old_archive/implemented/changelog-2026-04-15-codex-oauth-login.md](archive/old_archive/implemented/changelog-2026-04-15-codex-oauth-login.md)
- [archive/old_archive/implemented/codex-agent.md](archive/old_archive/implemented/codex-agent.md)
- [archive/old_archive/implemented/daily-report-2026-04-11.md](archive/old_archive/implemented/daily-report-2026-04-11.md)
- [archive/old_archive/implemented/daily-report-2026-04-15.md](archive/old_archive/implemented/daily-report-2026-04-15.md)
- [archive/old_archive/implemented/ink-terminal-vs-opentui.md](archive/old_archive/implemented/ink-terminal-vs-opentui.md)
- [archive/old_archive/implemented/ui-parity-implementation-note-2026-05-02.md](archive/old_archive/implemented/ui-parity-implementation-note-2026-05-02.md)
- [archive/old_archive/implemented/ui-skeleton-surfaces-2026-05-03.md](archive/old_archive/implemented/ui-skeleton-surfaces-2026-05-03.md)

### `archive/old_archive/issues/`

- [archive/old_archive/issues/2026-04-19-acp-agent-protocol.md](archive/old_archive/issues/2026-04-19-acp-agent-protocol.md)
- [archive/old_archive/issues/2026-04-20-architecture-docs-rewrite.md](archive/old_archive/issues/2026-04-20-architecture-docs-rewrite.md)
- [archive/old_archive/issues/2026-04-20-tui-codex-refactor.md](archive/old_archive/issues/2026-04-20-tui-codex-refactor.md)
- [archive/old_archive/issues/2026-04-21-frontend-refactor-notes.md](archive/old_archive/issues/2026-04-21-frontend-refactor-notes.md)
- [archive/old_archive/issues/2026-05-07-code-review-findings.md](archive/old_archive/issues/2026-05-07-code-review-findings.md)
- [archive/old_archive/issues/codex-review-review1.md](archive/old_archive/issues/codex-review-review1.md)

### `archive/old_archive/plan/`

- [archive/old_archive/plan/crate-migration-phase-0-inventory-2026-05-14.md](archive/old_archive/plan/crate-migration-phase-0-inventory-2026-05-14.md)

### `archive/old_archive/superpowers/plans/`

- [archive/old_archive/superpowers/plans/2026-04-10-background-agents.md](archive/old_archive/superpowers/plans/2026-04-10-background-agents.md)
- [archive/old_archive/superpowers/plans/2026-04-10-git-context-system-prompt.md](archive/old_archive/superpowers/plans/2026-04-10-git-context-system-prompt.md)
- [archive/old_archive/superpowers/plans/2026-04-10-hooks-system.md](archive/old_archive/superpowers/plans/2026-04-10-hooks-system.md)
- [archive/old_archive/superpowers/plans/2026-04-10-lsp-service-implementation.md](archive/old_archive/superpowers/plans/2026-04-10-lsp-service-implementation.md)
- [archive/old_archive/superpowers/plans/2026-04-10-web-search-cache.md](archive/old_archive/superpowers/plans/2026-04-10-web-search-cache.md)
- [archive/old_archive/superpowers/plans/2026-04-11-kairos-implementation.md](archive/old_archive/superpowers/plans/2026-04-11-kairos-implementation.md)
- [archive/old_archive/superpowers/plans/2026-04-11-oauth-login.md](archive/old_archive/superpowers/plans/2026-04-11-oauth-login.md)
- [archive/old_archive/superpowers/plans/2026-04-11-team-memory.md](archive/old_archive/superpowers/plans/2026-04-11-team-memory.md)
- [archive/old_archive/superpowers/plans/2026-04-15-agent-ipc-extensions.md](archive/old_archive/superpowers/plans/2026-04-15-agent-ipc-extensions.md)
- [archive/old_archive/superpowers/plans/2026-04-15-ipc-subsystem-extensions.md](archive/old_archive/superpowers/plans/2026-04-15-ipc-subsystem-extensions.md)
- [archive/old_archive/superpowers/plans/2026-04-18-phase1-runtime-storage-unification.md](archive/old_archive/superpowers/plans/2026-04-18-phase1-runtime-storage-unification.md)

### `archive/old_archive/superpowers/specs/`

- [archive/old_archive/superpowers/specs/2026-04-10-lsp-service-implementation-design.md](archive/old_archive/superpowers/specs/2026-04-10-lsp-service-implementation-design.md)
- [archive/old_archive/superpowers/specs/2026-04-10-p0-security-hardening-design.md](archive/old_archive/superpowers/specs/2026-04-10-p0-security-hardening-design.md)
- [archive/old_archive/superpowers/specs/2026-04-11-kairos-design.md](archive/old_archive/superpowers/specs/2026-04-11-kairos-design.md)
- [archive/old_archive/superpowers/specs/2026-04-11-oauth-login-design.md](archive/old_archive/superpowers/specs/2026-04-11-oauth-login-design.md)
- [archive/old_archive/superpowers/specs/2026-04-11-team-memory-design.md](archive/old_archive/superpowers/specs/2026-04-11-team-memory-design.md)
- [archive/old_archive/superpowers/specs/2026-04-13-subagent-testing-dashboard-design.md](archive/old_archive/superpowers/specs/2026-04-13-subagent-testing-dashboard-design.md)
- [archive/old_archive/superpowers/specs/2026-04-15-agent-ipc-extensions-design.md](archive/old_archive/superpowers/specs/2026-04-15-agent-ipc-extensions-design.md)
- [archive/old_archive/superpowers/specs/2026-04-15-codex-cli-credential-fallback-design.md](archive/old_archive/superpowers/specs/2026-04-15-codex-cli-credential-fallback-design.md)
- [archive/old_archive/superpowers/specs/2026-04-15-ink-ui-experiment-design.md](archive/old_archive/superpowers/specs/2026-04-15-ink-ui-experiment-design.md)
- [archive/old_archive/superpowers/specs/2026-04-15-ipc-subsystem-extensions-design.md](archive/old_archive/superpowers/specs/2026-04-15-ipc-subsystem-extensions-design.md)
- [archive/old_archive/superpowers/specs/2026-04-16-web-chat-ui-design.md](archive/old_archive/superpowers/specs/2026-04-16-web-chat-ui-design.md)
- [archive/old_archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md](archive/old_archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md)

### `archive/plan/`

- [archive/plan/2026-05-17-mcp-skill-plugin-real-project-test-plan.md](archive/plan/2026-05-17-mcp-skill-plugin-real-project-test-plan.md)
- [archive/plan/allthecodes-engine-optimization-plan-2026-05-29.md](archive/plan/allthecodes-engine-optimization-plan-2026-05-29.md)
- [archive/plan/allthecodes-rename-migration-plan-2026-05-24.md](archive/plan/allthecodes-rename-migration-plan-2026-05-24.md)
- [archive/plan/anthropic-api-coding-compat-risk-plan-2026-05-17.md](archive/plan/anthropic-api-coding-compat-risk-plan-2026-05-17.md)
- [archive/plan/bash-shell-parity-migration-plan-2026-05-18.md](archive/plan/bash-shell-parity-migration-plan-2026-05-18.md)
- [archive/plan/bun-docs-documentation-plan.md](archive/plan/bun-docs-documentation-plan.md)
- [archive/plan/cc-daemon-migration-plan-2026-05-13.md](archive/plan/cc-daemon-migration-plan-2026-05-13.md)
- [archive/plan/cc-engine-migration-plan-2026-05-13.md](archive/plan/cc-engine-migration-plan-2026-05-13.md)
- [archive/plan/cfg-test-production-wiring-plan-2026-05-21.md](archive/plan/cfg-test-production-wiring-plan-2026-05-21.md)
- [archive/plan/claude-code-bun-gap-plan.md](archive/plan/claude-code-bun-gap-plan.md)
- [archive/plan/command-e2e-test-plan-00-overview.md](archive/plan/command-e2e-test-plan-00-overview.md)
- [archive/plan/command-e2e-test-plan-01-core-info.md](archive/plan/command-e2e-test-plan-01-core-info.md)
- [archive/plan/command-e2e-test-plan-02-session-context.md](archive/plan/command-e2e-test-plan-02-session-context.md)
- [archive/plan/command-e2e-test-plan-03-auth-model.md](archive/plan/command-e2e-test-plan-03-auth-model.md)
- [archive/plan/command-e2e-test-plan-04-git.md](archive/plan/command-e2e-test-plan-04-git.md)
- [archive/plan/command-e2e-test-plan-05-permissions-sandbox.md](archive/plan/command-e2e-test-plan-05-permissions-sandbox.md)
- [archive/plan/command-e2e-test-plan-06-mcp-plugin.md](archive/plan/command-e2e-test-plan-06-mcp-plugin.md)
- [archive/plan/command-e2e-test-plan-07-agent-team.md](archive/plan/command-e2e-test-plan-07-agent-team.md)
- [archive/plan/command-e2e-test-plan-08-kairos-feature-gated.md](archive/plan/command-e2e-test-plan-08-kairos-feature-gated.md)
- [archive/plan/command-e2e-test-plan-09-memory-skills-hooks.md](archive/plan/command-e2e-test-plan-09-memory-skills-hooks.md)
- [archive/plan/command-e2e-test-plan-10-query-review.md](archive/plan/command-e2e-test-plan-10-query-review.md)
- [archive/plan/command-e2e-test-plan-11-alias-batch.md](archive/plan/command-e2e-test-plan-11-alias-batch.md)
- [archive/plan/command-settings-ui-coverage-audit-2026-05-08.md](archive/plan/command-settings-ui-coverage-audit-2026-05-08.md)
- [archive/plan/computer-use-implementation-checklist.md](archive/plan/computer-use-implementation-checklist.md)
- [archive/plan/core-utilities-migration-plan.md](archive/plan/core-utilities-migration-plan.md)
- [archive/plan/crate-migration-phase-plan-2026-05-14.md](archive/plan/crate-migration-phase-plan-2026-05-14.md)
- [archive/plan/daemon-usability-plan.md](archive/plan/daemon-usability-plan.md)
- [archive/plan/gap-analysis-vs-claude-code-2026-03-27-2026-05-29.md](archive/plan/gap-analysis-vs-claude-code-2026-03-27-2026-05-29.md)
- [archive/plan/generic-selectable-command-surface-plan-2026-05-08.md](archive/plan/generic-selectable-command-surface-plan-2026-05-08.md)
- [archive/plan/ipc-refactor-plan.md](archive/plan/ipc-refactor-plan.md)
- [archive/plan/missing-tools-adaptation-plan-2026-05-28.md](archive/plan/missing-tools-adaptation-plan-2026-05-28.md)
- [archive/plan/p1-defensive-fail-fast-execution-plan-2026-05-07.md](archive/plan/p1-defensive-fail-fast-execution-plan-2026-05-07.md)
- [archive/plan/plugin-ui-port-to-rust-plan-2026-05-08.md](archive/plan/plugin-ui-port-to-rust-plan-2026-05-08.md)
- [archive/plan/ratatui-ui-parity-omx-execution-plan-2026-05-08.md](archive/plan/ratatui-ui-parity-omx-execution-plan-2026-05-08.md)
- [archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md](archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md)
- [archive/plan/real-memoryfile-skill-mcp-plugin-lsp-agent-teams-e2e-plan-2026-05-24.md](archive/plan/real-memoryfile-skill-mcp-plugin-lsp-agent-teams-e2e-plan-2026-05-24.md)
- [archive/plan/remote-channel-phase1-telegram-lark-plan-2026-05-08.md](archive/plan/remote-channel-phase1-telegram-lark-plan-2026-05-08.md)
- [archive/plan/remote-control-gateway-execution-plan-2026-05-08.md](archive/plan/remote-control-gateway-execution-plan-2026-05-08.md)
- [archive/plan/remote-control-gateway-implementation-report-2026-05-08.md](archive/plan/remote-control-gateway-implementation-report-2026-05-08.md)
- [archive/plan/remote-control-gateway-omx-execution-plan-2026-05-08.md](archive/plan/remote-control-gateway-omx-execution-plan-2026-05-08.md)
- [archive/plan/root-src-library-migration-plan-2026-05-15.md](archive/plan/root-src-library-migration-plan-2026-05-15.md)
- [archive/plan/session-export-implementation-guide.md](archive/plan/session-export-implementation-guide.md)
- [archive/plan/traceable-logging-plan.md](archive/plan/traceable-logging-plan.md)
- [archive/plan/ui-test-target-dead-code-subagent-plan-2026-05-20.md](archive/plan/ui-test-target-dead-code-subagent-plan-2026-05-20.md)
- [archive/plan/utils-full-build-parallel-development-plan-2026-05-19.md](archive/plan/utils-full-build-parallel-development-plan-2026-05-19.md)
- [archive/plan/workspace-all-targets-warning-budget-plan-2026-05-20.md](archive/plan/workspace-all-targets-warning-budget-plan-2026-05-20.md)
- [archive/plan/workspace-decycle-plan-2026-05-14.md](archive/plan/workspace-decycle-plan-2026-05-14.md)

### `archive/planz/`

- [archive/planz/cohesive-optimization-plan-2026-05-30.json](archive/planz/cohesive-optimization-plan-2026-05-30.json)
- [archive/planz/comprehensive-gap-analysis-2026-05-31.md](archive/planz/comprehensive-gap-analysis-2026-05-31.md)
- [archive/planz/langfuse-improvement-code-review-2026-06-01.md](archive/planz/langfuse-improvement-code-review-2026-06-01.md)
- [archive/planz/phase5-missing-tools-optimization-plan-2026-05-30.md](archive/planz/phase5-missing-tools-optimization-plan-2026-05-30.md)

### `archive/scripts/`

- [archive/scripts/README.md](archive/scripts/README.md)
- [archive/scripts/better-view-ui-panels-omx-tasks-2026-05-11.txt](archive/scripts/better-view-ui-panels-omx-tasks-2026-05-11.txt)
- [archive/scripts/non-workspace-unfinished-standard-task-2026-05-11.md](archive/scripts/non-workspace-unfinished-standard-task-2026-05-11.md)
- [archive/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt](archive/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt)
- [archive/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt](archive/scripts/workspace-crate-extraction-omx-tasks-2026-05-10.txt)

### `archive/scripts/achieve/`

- [archive/scripts/achieve/p1-defensive-fail-fast-after-phase1-tasks.txt](archive/scripts/achieve/p1-defensive-fail-fast-after-phase1-tasks.txt)
- [archive/scripts/achieve/ratatui-ui-parity-omx-tasks.txt](archive/scripts/achieve/ratatui-ui-parity-omx-tasks.txt)
- [archive/scripts/achieve/remote-control-gateway-omx-tasks-2026-05-08.txt](archive/scripts/achieve/remote-control-gateway-omx-tasks-2026-05-08.txt)
- [archive/scripts/achieve/workspace-crate-extraction-supervisor-smoke-task.txt](archive/scripts/achieve/workspace-crate-extraction-supervisor-smoke-task.txt)

### `archive/superpowers/plans/`

- [archive/superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md](archive/superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md)
- [archive/superpowers/plans/2026-04-11-team-memory-sync.md](archive/superpowers/plans/2026-04-11-team-memory-sync.md)
- [archive/superpowers/plans/2026-04-12-tools-commands-test-coverage.md](archive/superpowers/plans/2026-04-12-tools-commands-test-coverage.md)

### `archive/superpowers/specs/`

- [archive/superpowers/specs/2026-04-11-team-memory-sync-design.md](archive/superpowers/specs/2026-04-11-team-memory-sync-design.md)
- [archive/superpowers/specs/2026-04-20-workspace-split-design.md](archive/superpowers/specs/2026-04-20-workspace-split-design.md)

### `archive/testing/`

- [archive/testing/docker-test-guide.md](archive/testing/docker-test-guide.md)
- [archive/testing/e2e_list.md](archive/testing/e2e_list.md)
- [archive/testing/pty-e2e-test.md](archive/testing/pty-e2e-test.md)
- [archive/testing/pty-test-language-comparison.md](archive/testing/pty-test-language-comparison.md)
- [archive/testing/rust-e2e-test.md](archive/testing/rust-e2e-test.md)

### `archive/tmp/`

- [archive/tmp/JSON_USAGE_ANALYSIS.md](archive/tmp/JSON_USAGE_ANALYSIS.md)
- [archive/tmp/REWRITE_PLAN.md](archive/tmp/REWRITE_PLAN.md)
- [archive/tmp/STORAGE.md](archive/tmp/STORAGE.md)
- [archive/tmp/cc-rust-overview.md](archive/tmp/cc-rust-overview.md)
- [archive/tmp/claude-code-rs-refactor-audit-2026-05-07.md](archive/tmp/claude-code-rs-refactor-audit-2026-05-07.md)
- [archive/tmp/cloud-providers.md](archive/tmp/cloud-providers.md)
- [archive/tmp/codex-backend.md](archive/tmp/codex-backend.md)
- [archive/tmp/dead-code-audit.md](archive/tmp/dead-code-audit.md)
- [archive/tmp/ui-parity-update-plan.md](archive/tmp/ui-parity-update-plan.md)
- [archive/tmp/workspace-split-measurements.md](archive/tmp/workspace-split-measurements.md)

### `archive/ui/`

- [archive/ui/claude-code-bun-ui-structure.md](archive/ui/claude-code-bun-ui-structure.md)
- [archive/ui/claude-code-rs-ui-structure.md](archive/ui/claude-code-rs-ui-structure.md)
- [archive/ui/codex-rs-tui-structure.md](archive/ui/codex-rs-tui-structure.md)
- [archive/ui/truncation-summary.md](archive/ui/truncation-summary.md)

### `archive/ui/better-view/`

- [archive/ui/better-view/README.md](archive/ui/better-view/README.md)
- [archive/ui/better-view/approval-panels.md](archive/ui/better-view/approval-panels.md)
- [archive/ui/better-view/external-flows.md](archive/ui/better-view/external-flows.md)
- [archive/ui/better-view/selector-surfaces.md](archive/ui/better-view/selector-surfaces.md)
- [archive/ui/better-view/settings-panels.md](archive/ui/better-view/settings-panels.md)
- [archive/ui/better-view/wizard-flows.md](archive/ui/better-view/wizard-flows.md)

### `archive/ui/commands/`

- [archive/ui/commands/README.md](archive/ui/commands/README.md)
- [archive/ui/commands/approval-snapshots.md](archive/ui/commands/approval-snapshots.md)
- [archive/ui/commands/command-surfaces.md](archive/ui/commands/command-surfaces.md)
- [archive/ui/commands/text-and-external.md](archive/ui/commands/text-and-external.md)

### `archive/ui/great/`

- [archive/ui/great/README.md](archive/ui/great/README.md)
- [archive/ui/great/01_message_rendering_comparison.md](archive/ui/great/01_message_rendering_comparison.md)
- [archive/ui/great/02_component_parity_comparison.md](archive/ui/great/02_component_parity_comparison.md)
- [archive/ui/great/03_rendering_subsystem_comparison.md](archive/ui/great/03_rendering_subsystem_comparison.md)
- [archive/ui/great/04_permission_input_comparison.md](archive/ui/great/04_permission_input_comparison.md)
- [archive/ui/great/05_app_shell_comparison.md](archive/ui/great/05_app_shell_comparison.md)
- [archive/ui/great/06_missing_features_summary.md](archive/ui/great/06_missing_features_summary.md)

### `archive/ui/great/plans/`

- [archive/ui/great/plans/plan-01-message-rendering.md](archive/ui/great/plans/plan-01-message-rendering.md)
- [archive/ui/great/plans/plan-02-design-system.md](archive/ui/great/plans/plan-02-design-system.md)
- [archive/ui/great/plans/plan-03-input-editor.md](archive/ui/great/plans/plan-03-input-editor.md)
- [archive/ui/great/plans/plan-04-notification.md](archive/ui/great/plans/plan-04-notification.md)
- [archive/ui/great/plans/plan-05-agent-navigation.md](archive/ui/great/plans/plan-05-agent-navigation.md)
- [archive/ui/great/plans/plan-06-syntax-highlighting.md](archive/ui/great/plans/plan-06-syntax-highlighting.md)
- [archive/ui/great/plans/plan-07-permissions-wiring.md](archive/ui/great/plans/plan-07-permissions-wiring.md)
- [archive/ui/great/plans/plan-08-dead-code-cleanup.md](archive/ui/great/plans/plan-08-dead-code-cleanup.md)

### `archive/ui/show-off/`

- [archive/ui/show-off/data-type-visual-distinction.md](archive/ui/show-off/data-type-visual-distinction.md)
- [archive/ui/show-off/tui-display-issues.md](archive/ui/show-off/tui-display-issues.md)

### `archive/ui/show-off/data-type-visual-distinction/`

- [archive/ui/show-off/data-type-visual-distinction/01-core-data-types.md](archive/ui/show-off/data-type-visual-distinction/01-core-data-types.md)
- [archive/ui/show-off/data-type-visual-distinction/02-six-layer-distinction.md](archive/ui/show-off/data-type-visual-distinction/02-six-layer-distinction.md)
- [archive/ui/show-off/data-type-visual-distinction/03-theme-system.md](archive/ui/show-off/data-type-visual-distinction/03-theme-system.md)
- [archive/ui/show-off/data-type-visual-distinction/04-aggregation-optimizations.md](archive/ui/show-off/data-type-visual-distinction/04-aggregation-optimizations.md)
- [archive/ui/show-off/data-type-visual-distinction/05-rendering-pipeline.md](archive/ui/show-off/data-type-visual-distinction/05-rendering-pipeline.md)
- [archive/ui/show-off/data-type-visual-distinction/06-key-files-index.md](archive/ui/show-off/data-type-visual-distinction/06-key-files-index.md)
- [archive/ui/show-off/data-type-visual-distinction/07-distinction-strategy-summary.md](archive/ui/show-off/data-type-visual-distinction/07-distinction-strategy-summary.md)

### `archive/utils/`

- [archive/utils/bash-shell.md](archive/utils/bash-shell.md)
- [archive/utils/computer-use.md](archive/utils/computer-use.md)
- [archive/utils/core-utilities.md](archive/utils/core-utilities.md)
- [archive/utils/hooks.md](archive/utils/hooks.md)
- [archive/utils/misc.md](archive/utils/misc.md)
- [archive/utils/overview.md](archive/utils/overview.md)
- [archive/utils/permissions-classifier.md](archive/utils/permissions-classifier.md)
- [archive/utils/plugins-marketplace.md](archive/utils/plugins-marketplace.md)
- [archive/utils/settings-mdm.md](archive/utils/settings-mdm.md)
- [archive/utils/suggestions-input.md](archive/utils/suggestions-input.md)
- [archive/utils/teams-swarm.md](archive/utils/teams-swarm.md)
- [archive/utils/telemetry-observability.md](archive/utils/telemetry-observability.md)
- [archive/utils/tui-panel-output-classification.md](archive/utils/tui-panel-output-classification.md)

### `archive/web/`

- [archive/web/nextjs-react-xterm-web-plan-2026-05-27.md](archive/web/nextjs-react-xterm-web-plan-2026-05-27.md)
