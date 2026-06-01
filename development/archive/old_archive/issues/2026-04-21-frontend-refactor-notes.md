# Frontend Refactor Notes

Date: 2026-04-21 (original) · updated 2026-04-22 (Issue 07 landing)
Scope: `ui/`

## Purpose

This note records how the newly imported upstream-style frontend files should be handled during the staged refactor of the current OpenTUI Lite frontend.

## Migration status (2026-04-22)

The staged migration plan covered by issues #90 → #96 has shipped.
The adopted architecture is described in
[ui/docs/ui-frontend-architecture.md](/F:/AIclassmanager/cc/rust/ui/docs/ui-frontend-architecture.md)
and deferred upstream panels — with explicit protocol blockers — are
enumerated in
[ui/docs/ui-panels-deferred.md](/F:/AIclassmanager/cc/rust/ui/docs/ui-panels-deferred.md).

Active-frontend additions that landed:

- `ui/src/view-model/` + `ui/src/adapters/` — normalized cross-slice
  types + IPC mappers (Issue 01 / #90).
- `ui/src/components/FilePathLink.tsx`, `OrderedList.tsx`,
  `PressEnterToContinue.tsx`, `TagTabs.tsx`,
  `ValidationErrorsList.tsx`, `string-width.ts` — primitive re-hosts
  (Issue 02 / #91).
- `ui/src/components/messages/` — per-type message leaves.
  `MessageBubble` is a thin dispatcher (Issue 03 / #92).
- `ui/src/components/permissions/` — category-aware dialog family;
  `ui/src/components/StructuredDiff/` — pure hunk parser + renderer
  (Issue 04 / #93).
- `ui/src/components/panels/` — per-subsystem cards consumed by
  `SubsystemStatus.tsx` and `TeamPanel.tsx` (Issue 05 / #94).
- `ui/src/components/PromptInput/` — composer submodules;
  `InputPrompt.tsx` composes them (Issue 06 / #95).
- `ui/src/components/StatusLine/` — built-in statusline derived from
  the store plus optional user-configured `status_line_update`
  overlay (Issue 07 / #96).

Removed interim scaffolding:

- Empty placeholder barrels `ui/src/components/mcp/index.ts`,
  `shell/index.ts`, `teams/index.ts` — the real work landed under
  `ui/src/components/panels/` (Issue 05) and `messages/` (Issue 03).
- `ui/src/components/PermissionDialog.tsx` — monolithic dialog,
  replaced by `permissions/PermissionRequestDialog`.
- `ui/src/components/Header.tsx` — replaced by `StatusLine/`.
- `ui/src/components/ThinkingBlock.tsx`, `ToolActivity.tsx`,
  `ToolGroup.tsx` — replaced by / renamed into `messages/`.
- `ui/src/components/input-prompt-{hooks,keys,utils}.ts` — relocated
  into `PromptInput/` (git-rename preserves blame).

Classification summary against the original A / B / C lists below:

- **Class A → adopted**: `FilePathLink`, `PressEnterToContinue`,
  `TagTabs`, `ValidationErrorsList`; the Lite `Spinner.tsx` already
  covers the `spinner/` slot.
- **Class B → adopted (partially)**: `StructuredDiff.tsx`,
  `StructuredDiffList.tsx` (local pure parser + renderer), all
  `permissions/` category variants, `mcp/`, `LspRecommendation/`,
  `teams/`, `shell/` adopted as operational cards under
  `components/panels/`. Richer MCP elicitation, LSP recommendation,
  and shell-progress paths stay deferred — see
  `ui-panels-deferred.md` for the protocol blockers.
  `FileEditToolDiff.tsx` / `FileEditToolUpdatedMessage.tsx` are
  adopted at the "show diff alongside tool activity" level via
  `messages/FileEditToolPreview`.
- **Class C → intentionally skipped**: `BaseTextInput.tsx`,
  `VimTextInput.tsx`, `TextInput.tsx`, `VirtualMessageList.tsx`,
  `ModelPicker.tsx`, `ThinkingToggle.tsx`, full `PromptInput/**`
  runtime. The Lite `InputPrompt.tsx` keeps the OpenTUI keyboard
  path and uses focused `PromptInput/` submodules for decomposition
  rather than porting the upstream runtime wholesale.

The A / B / C tables below stay for historical reference — they
describe the source set we inherited, not the current active source
tree.

The active frontend remains:
- [ui/src/main.tsx](/F:/AIclassmanager/cc/rust/ui/src/main.tsx:1)
- [ui/src/components/App.tsx](/F:/AIclassmanager/cc/rust/ui/src/components/App.tsx:62)

The imported sample tree has been relocated to:
- [ui/examples/upstream-patterns](/F:/AIclassmanager/cc/rust/ui/examples/upstream-patterns)

The sample tree keeps the original relative structure under `ui/`, so files that were previously under `ui/src/components/**` now live under `ui/examples/upstream-patterns/src/components/**`, and likewise for `screens/`, `dialogLaunchers.tsx`, and `interactiveHelpers.tsx`.

## Relocation Rules

Moved to sample tree:
- 381 retained paths from the untracked upstream-style import set

Excluded and removed from the active tree:
- stub-only directories marked `Auto-generated stub/type stub — replace with real implementation`
- empty file `ui/src/components/App2.tsx`
- obvious conflicting backup file `ui/src/components/Spinner copy.tsx`
- a small set of single-file decompilation leftovers and type-only placeholders

High-confidence excluded directories:
- `ui/src/screens/src/`
- `ui/src/components/LogoV2/src/`
- `ui/src/components/agents/src/`
- `ui/src/components/agents/new-agent-creation/wizard-steps/src/`
- `ui/src/components/helpv2/src/`
- `ui/src/components/hooks/src/`
- `ui/src/components/mcp/src/`
- `ui/src/components/messages/src/`
- `ui/src/components/messages/UserToolResultMessage/src/`
- `ui/src/components/permissions/ExitPlanModePermissionRequest/src/`
- `ui/src/components/permissions/FileEditPermissionRequest/src/`
- `ui/src/components/permissions/FilePermissionDialog/src/`
- `ui/src/components/permissions/rules/src/`
- `ui/src/components/permissions/SedEditPermissionRequest/src/`
- `ui/src/components/permissions/SkillPermissionRequest/src/`
- `ui/src/components/permissions/src/`
- `ui/src/components/PromptInput/src/`
- `ui/src/components/Settings/src/`
- `ui/src/components/StructuredDiff/src/`
- `ui/src/components/tasks/src/`
- `ui/src/components/TrustDialog/src/`

High-confidence excluded single files:
- `ui/src/components/App2.tsx`
- `ui/src/components/Spinner copy.tsx`
- `ui/src/components/agents/SnapshotUpdateDialog.ts`
- `ui/src/components/agents/new-agent-creation/types.ts`
- `ui/src/components/mcp/types.ts`
- `ui/src/components/messages/SnipBoundaryMessage.ts`
- `ui/src/components/messages/UserCrossSessionMessage.ts`
- `ui/src/components/messages/UserForkBoilerplateMessage.ts`
- `ui/src/components/messages/UserGitHubWebhookMessage.ts`
- `ui/src/components/spinner/types.ts`
- `ui/src/components/ui/option.ts`
- `ui/src/components/wizard/types.ts`

## Main-Entry Constraints

These files must be treated as non-main implementation entrypoints or sample-runtime-only references:
- `ui/examples/upstream-patterns/src/dialogLaunchers.tsx`
- `ui/examples/upstream-patterns/src/interactiveHelpers.tsx`
- `ui/examples/upstream-patterns/src/screens/Doctor.tsx`
- `ui/examples/upstream-patterns/src/screens/REPL.tsx`
- `ui/examples/upstream-patterns/src/screens/ResumeConversation.tsx`
- everything that originally lived under `ui/src/screens/src/**`
- `ui/examples/upstream-patterns/src/components/LogSelector.tsx`
- `ui/examples/upstream-patterns/src/components/SessionPreview.tsx`
- `ui/examples/upstream-patterns/src/components/Messages.tsx`
- `ui/examples/upstream-patterns/src/components/MessageRow.tsx`
- `ui/examples/upstream-patterns/src/components/CompactSummary.tsx`

Reason:
- they belong to an upstream Ink/runtime-heavy REPL stack, not the current OpenTUI Lite entry chain
- they should inform design and migration decisions, but should not be wired into the active frontend as-is

## Components Classification

The following classification is based on the original logical paths under `ui/src/components/**`. The actual preserved sample files now live under `ui/examples/upstream-patterns/src/components/**`.

### A. Keep As Design-Pattern Reference

Directories:
- `ui/src/components/customselect/`
- `ui/src/components/design-system/`
- `ui/src/components/highlighted-code/`
- `ui/src/components/spinner/`
- `ui/src/components/ui/`
- `ui/src/components/wizard/`

Root files:
- `ui/src/components/BuiltinStatusLine.tsx`
- `ui/src/components/ClickableImageRef.tsx`
- `ui/src/components/ConfigurableShortcutHint.tsx`
- `ui/src/components/CtrlOToExpand.tsx`
- `ui/src/components/FilePathLink.tsx`
- `ui/src/components/HighlightedCode.tsx`
- `ui/src/components/InterruptedByUser.tsx`
- `ui/src/components/MarkdownTable.tsx`
- `ui/src/components/MessageResponse.tsx`
- `ui/src/components/MessageTimestamp.tsx`
- `ui/src/components/PrBadge.tsx`
- `ui/src/components/PressEnterToContinue.tsx`
- `ui/src/components/SearchBox.tsx`
- `ui/src/components/TagTabs.tsx`
- `ui/src/components/ValidationErrorsList.tsx`

Notes:
- these are mostly reusable atoms, selector patterns, tree/list interactions, or general display helpers
- they are useful as structural reference even before any direct migration

### B. Adapt To OpenTUI Lite State/Protocol Before Reuse

Directories:
- `ui/src/components/LspRecommendation/`
- `ui/src/components/mcp/`
- `ui/src/components/permissions/`
- `ui/src/components/shell/`
- `ui/src/components/skills/`
- `ui/src/components/StructuredDiff/`
- `ui/src/components/teams/`

Root files:
- `ui/src/components/AgentProgressLine.tsx`
- `ui/src/components/DiagnosticsDisplay.tsx`
- `ui/src/components/FallbackToolUseErrorMessage.tsx`
- `ui/src/components/FallbackToolUseRejectedMessage.tsx`
- `ui/src/components/FileEditToolDiff.tsx`
- `ui/src/components/FileEditToolUpdatedMessage.tsx`
- `ui/src/components/FileEditToolUseRejectedMessage.tsx`
- `ui/src/components/MCPServerApprovalDialog.tsx`
- `ui/src/components/MCPServerDialogCopy.tsx`
- `ui/src/components/MCPServerMultiselectDialog.tsx`
- `ui/src/components/NotebookEditToolUseRejectedMessage.tsx`
- `ui/src/components/StructuredDiff.tsx`
- `ui/src/components/StructuredDiffList.tsx`
- `ui/src/components/ToolUseLoader.tsx`

Notes:
- these align with current Lite protocol surfaces such as permissions, tool result rendering, LSP/MCP/team state
- reuse should happen only through an adapter that targets the current OpenTUI Lite `ipc/protocol.ts` and `store/`
- do not import them directly into the current frontend without removing upstream Ink/state dependencies

### C. Sample Runtime Only, Do Not Directly Promote Into Main Implementation

Directories:
- `ui/src/components/agents/`
- `ui/src/components/diff/`
- `ui/src/components/helpv2/`
- `ui/src/components/hooks/`
- `ui/src/components/LogoV2/`
- `ui/src/components/ManagedSettingsSecurityDialog/`
- `ui/src/components/memory/`
- `ui/src/components/messages/`
- `ui/src/components/PromptInput/`
- `ui/src/components/sandbox/`
- `ui/src/components/Settings/`
- `ui/src/components/tasks/`
- `ui/src/components/TrustDialog/`

Remote / desktop / upstream workflow roots:
- `ui/src/components/ApproveApiKey.tsx`
- `ui/src/components/AutoModeOptInDialog.tsx`
- `ui/src/components/BridgeDialog.tsx`
- `ui/src/components/ChannelDowngradeDialog.tsx`
- `ui/src/components/ClaudeInChromeOnboarding.tsx`
- `ui/src/components/ClaudeMdExternalIncludesDialog.tsx`
- `ui/src/components/ConsoleOAuthFlow.tsx`
- `ui/src/components/IdeAutoConnectDialog.tsx`
- `ui/src/components/IdeOnboardingDialog.tsx`
- `ui/src/components/IdeStatusIndicator.tsx`
- `ui/src/components/NativeAutoUpdater.tsx`
- `ui/src/components/Onboarding.tsx`
- `ui/src/components/PackageManagerAutoUpdater.tsx`
- `ui/src/components/RemoteCallout.tsx`
- `ui/src/components/RemoteEnvironmentDialog.tsx`
- `ui/src/components/ResumeTask.tsx`
- `ui/src/components/ShowInIDEPrompt.tsx`
- `ui/src/components/SkillImprovementSurvey.tsx`
- `ui/src/components/TeleportError.tsx`
- `ui/src/components/TeleportProgress.tsx`
- `ui/src/components/TeleportRepoMismatchDialog.tsx`
- `ui/src/components/TeleportResumeWrapper.tsx`
- `ui/src/components/TeleportStash.tsx`
- `ui/src/components/WorktreeExitDialog.tsx`

Input / message / runtime replacement roots:
- `ui/src/components/BaseTextInput.tsx`
- `ui/src/components/TextInput.tsx`
- `ui/src/components/VimTextInput.tsx`
- `ui/src/components/VirtualMessageList.tsx`
- `ui/src/components/Message.tsx`
- `ui/src/components/MessageModel.tsx`
- `ui/src/components/MessageRow.tsx`
- `ui/src/components/Messages.tsx`
- `ui/src/components/MessageSelector.tsx`
- `ui/src/components/messageActions.tsx`
- `ui/src/components/OffscreenFreeze.tsx`
- `ui/src/components/StatusLine.tsx`
- `ui/src/components/StatusNotices.tsx`
- `ui/src/components/TaskListV2.tsx`
- `ui/src/components/TeammateViewHeader.tsx`
- `ui/src/components/WorkflowMultiselectDialog.tsx`

Experimental / settings / upstream shell roots:
- `ui/src/components/BashModeProgress.tsx`
- `ui/src/components/BypassPermissionsModeDialog.tsx`
- `ui/src/components/CompactSummary.tsx`
- `ui/src/components/ContextSuggestions.tsx`
- `ui/src/components/ContextVisualization.tsx`
- `ui/src/components/CoordinatorAgentStatus.tsx`
- `ui/src/components/CostThresholdDialog.tsx`
- `ui/src/components/DevBar.tsx`
- `ui/src/components/DevChannelsDialog.tsx`
- `ui/src/components/EffortCallout.tsx`
- `ui/src/components/EffortIndicator.ts`
- `ui/src/components/ExitFlow.tsx`
- `ui/src/components/ExportDialog.tsx`
- `ui/src/components/FullscreenLayout.tsx`
- `ui/src/components/GlobalSearchDialog.tsx`
- `ui/src/components/HistorySearchDialog.tsx`
- `ui/src/components/IdleReturnDialog.tsx`
- `ui/src/components/InvalidConfigDialog.tsx`
- `ui/src/components/InvalidSettingsDialog.tsx`
- `ui/src/components/KeybindingWarnings.tsx`
- `ui/src/components/LanguagePicker.tsx`
- `ui/src/components/LogSelector.tsx`
- `ui/src/components/Markdown.tsx`
- `ui/src/components/MCPServerDesktopImportDialog.tsx`
- `ui/src/components/MemoryUsageIndicator.tsx`
- `ui/src/components/ModelPicker.tsx`
- `ui/src/components/OutputStylePicker.tsx`
- `ui/src/components/QuickOpenDialog.tsx`
- `ui/src/components/SandboxViolationExpandedView.tsx`
- `ui/src/components/ScrollKeybindingHandler.tsx`
- `ui/src/components/SentryErrorBoundary.ts`
- `ui/src/components/SessionBackgroundHint.tsx`
- `ui/src/components/SessionPreview.tsx`
- `ui/src/components/Stats.tsx`
- `ui/src/components/ThemePicker.tsx`
- `ui/src/components/ThinkingToggle.tsx`
- `ui/src/components/TokenWarning.tsx`
- `ui/src/components/UndercoverAutoCallout.tsx`

Notes:
- this class represents an upstream runtime stack, not a small reusable component slice
- these files are valuable for architecture study, interaction patterns, and staged extraction
- they should remain in the sample tree until specific subsets are deliberately reimplemented for OpenTUI Lite

## Execution Cautions

1. Do not import sample-tree files directly back into the active `ui/src/main.tsx -> ui/src/components/App.tsx` path unless they have first been adapted to the current Lite protocol and state model.
2. Treat `ui/examples/upstream-patterns/src/screens/**`, `dialogLaunchers.tsx`, and `interactiveHelpers.tsx` as legacy sample runtime references, not as alternate entrypoints waiting to be enabled.
3. For class A, prefer extracting layout ideas, naming, and component boundaries rather than copying upstream Ink-specific code verbatim.
4. For class B, introduce a Lite adapter layer first. Adapt from current `ui/src/ipc/protocol.ts` and `ui/src/store/*`, then re-host the component on OpenTUI primitives.
5. For class C, study the flow and state machine, then re-implement the minimum needed behavior in Lite form. Do not port the runtime wholesale.
6. The sample tree is intentionally incomplete. Excluded stubs were removed, so the sample should be read as a reference corpus, not as a standalone runnable frontend.
7. `placeholder.invalid` URLs and PowerShell mojibake in terminal output are not enough reason to drop a file. Exclusion should stay evidence-based.
8. Keep the current production path authoritative until a replacement slice has compile proof and behavior verification.
