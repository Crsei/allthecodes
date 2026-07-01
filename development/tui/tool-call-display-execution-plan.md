# Tool Call Display Execution Plan

> Date: 2026-06-22
> Scope: `allthecodes-web` chat transcript, tool call drawer, composer-adjacent status surfaces.
> Goal: reduce low-signal tool-call noise such as `TaskCreate`, `TaskList`, `TaskUpdate`, `TaskGet`, `TodoWrite`, and system-status updates while keeping full auditability in the tools panel.

---

## 1. Problem

The current frontend renders most `tool_use` messages as chat-visible cards. This makes state-management tools look as important as user-facing work:

- task tools appear as repeated cards even when their useful effect is the task list state;
- system/status events can appear in the conversation instead of a footer/status surface;
- repeated successful tool calls are only partially collapsed by count, not by semantic display policy;
- the side panel and transcript share too much presentation logic.

Claude Code's TUI uses a stricter model:

- each tool decides whether its use/result renders in transcript;
- task/todo tools often return `null` for tool-use UI and surface state in a dedicated task panel;
- status information lives in a footer/status line, not as normal chat content;
- complete tool details remain inspectable outside the main transcript.

This plan applies the same product rule to the web frontend.

---

## 2. Current Frontend Entry Points

| Area | File | Current role |
|------|------|--------------|
| SDK normalization | `src/lib/message-normalizer.ts` | Converts wire `tool_use` / `tool_result` blocks into `ToolUseMessageVM` / `ToolResultMessageVM` and decorates status. |
| Tool record extraction | `src/lib/tool-calls.ts` | Builds `ToolCallRecord[]` for drawer and dropdown renderers. |
| Chat renderer switch | `src/components/chat/MessageItem.tsx` | Routes `tool_use` to `ToolCallDropdown` or `ToolUse`, and `tool_result` to `ToolResult`. |
| Inline tool card | `src/components/chat/renderers/ToolUse.tsx` | Displays a full card with tool name, status, input preview, and actions. |
| Inline dropdown | `src/components/chat/renderers/ToolCallDropdown.tsx` | Displays input/result details for one tool call. |
| Batch collapse | `src/components/chat/renderers/ToolCallBatch.tsx` + `src/lib/tool-call-batches.ts` | Collapses extra successful tool calls after a threshold. |
| Right sidebar | `src/components/workspace/ToolCallsPanel.tsx` | Lists all extracted tool calls with search, filter, jump, copy. |
| Footer/status | `src/components/chat/StatusFooter.tsx` | Existing place to surface session/status metadata. |

Existing tests that will need updates:

- `tests/shell.spec.ts` tool-use rendering and collapse assertions.
- `tests/unit/message-normalizer.test.ts`.
- `tests/unit/tool-call-batches.test.ts`.
- `tests/unit/tool-calls.test.ts`.
- visual smoke fixture `tool_use`.

---

## 3. Target Display Model

Introduce a centralized display policy. The policy should classify every tool call into one transcript behavior and one auxiliary surface.

| Policy | Transcript behavior | Auxiliary surface | Examples |
|--------|---------------------|-------------------|----------|
| `hidden_state` | Do not render normal tool-use/result card unless error. | Task/todo/status surface plus ToolCallsPanel audit. | `TaskCreate`, `TaskGet`, `TaskList`, `TaskUpdate`, `TodoWrite`. |
| `status_footer` | Do not render normal tool card. | `StatusFooter` or a compact status strip. | `SystemStatus`, `system_status`, status-line style events. |
| `compact_inline` | Render one-line summary by default; details behind disclosure. | ToolCallsPanel keeps full details. | `Read`, `Grep`, `Glob`, `WebSearch`, `WebFetch`, MCP read/search tools. |
| `expanded_inline` | Render visible dropdown/card because user needs to inspect it. | ToolCallsPanel mirrors it. | `Bash`, `Edit`, `Write`, `ApplyPatch`, permission-sensitive tools. |
| `error_inline` | Always render in transcript. | ToolCallsPanel mirrors it. | Any failed hidden/compact tool. |

Default fallback:

- unknown successful tools use `compact_inline`;
- unknown failed tools use `error_inline`;
- unknown running tools use `compact_inline` so the user can see ongoing work.

---

## 4. Canonical Tool Identity

Add a single canonicalization layer to avoid duplicated entries caused by case or alias differences.

```ts
type ToolDisplayPolicy = {
  canonicalName: string
  family: 'task' | 'status' | 'file' | 'shell' | 'search' | 'web' | 'mcp' | 'agent' | 'other'
  transcript: 'hidden' | 'compact' | 'expanded'
  result: 'hidden' | 'compact' | 'expanded'
  surface: 'none' | 'task_panel' | 'status_footer' | 'tool_panel'
  showOnError: true
}
```

Implementation file:

- `src/lib/tool-display-policy.ts`

Rules:

- normalize matching with `trim().toLowerCase()`;
- preserve canonical display casing such as `TaskCreate`, `Bash`, `WebSearch`;
- map known aliases and casing variants to one canonical key;
- MCP names keep their original server/tool display, but are matched through normalized prefixes such as `mcp__`.

Initial known state tools:

```ts
TaskCreate
TaskGet
TaskList
TaskUpdate
TodoWrite
```

Initial known status tools/events:

```ts
SystemStatus
system_status
StatusLine
status_line
```

---

## 5. Implementation Phases

### Phase 1: Policy Module

Add `src/lib/tool-display-policy.ts` with:

- `normalizeToolName(name: string): string`;
- `canonicalToolName(name: string): string`;
- `getToolDisplayPolicy(name: string): ToolDisplayPolicy`;
- `shouldRenderToolUseInTranscript(call): boolean`;
- `shouldRenderToolResultInTranscript(call, result): boolean`.

Tests:

- case-only duplicates map to one canonical name;
- task tools return `hidden_state`;
- status tools return `status_footer`;
- errors override hidden behavior and render inline;
- unknown tools fall back to compact display.

### Phase 2: Transcript Filtering Without Data Loss

Do not remove messages from `chat-store`. Build transcript-visible messages as a derived view before rendering.

Likely touch points:

- `src/components/chat/MessageList.tsx`
- `src/components/chat/MessageItem.tsx`
- `src/lib/chat/renderer-state.ts`
- `src/lib/tool-call-batches.ts`

Rules:

- hidden successful state tools do not render `ToolUse`, `ToolCallDropdown`, or `ToolResult` in the chat transcript;
- failed state tools render as `error_inline`;
- running state tools can update the task/status surface but should not create a card unless no surface data exists;
- `ToolCallsPanel` still receives all calls from raw messages.

### Phase 3: Task/Todo Surface

Create a compact task surface instead of tool-call cards.

Candidate files:

- `src/components/chat/TaskStatusPanel.tsx`
- `src/lib/task-tool-state.ts`
- `src/components/chat/StatusFooter.tsx` if the compact panel belongs near the composer.

Behavior:

- infer task rows from `TaskCreate`, `TaskUpdate`, `TaskList`, and `TaskGet` inputs/results when structured data is available;
- fall back to compact text if only model-facing result text exists;
- show pending/in-progress/completed icons in a vertical list;
- keep completed rows briefly visible, then collapse when the list is long;
- never use nested cards for the task list.

Acceptance:

- `TaskCreate` creates/updates the task panel without a chat card;
- `TaskUpdate` changes row status without a chat card;
- `TaskList` refreshes the panel without dumping a tool-result block;
- task-tool failures still render inline.

### Phase 4: System Status Surface

Route status tools/events to a footer/status area.

Likely touch points:

- `src/components/chat/StatusFooter.tsx`
- `src/components/chat/ChatInputAuxiliary.tsx`
- `src/lib/message-normalizer.ts`

Behavior:

- `SystemStatus` and status-line equivalents should update footer text/badges;
- status updates should not appear as normal tool cards;
- stale status should clear or dim after a defined interval;
- critical status errors can still render as system/error notices.

### Phase 5: Compact Inline Tools

Update visible tool calls so successful low-risk tools are concise.

Likely touch points:

- `src/components/chat/renderers/ToolCallDropdown.tsx`
- `src/components/chat/renderers/ToolUse.tsx`
- `src/lib/tool-input-markdown.ts`
- `src/lib/tool-calls.ts`

Behavior:

- `Read`, `Grep`, `Glob`, `WebSearch`, `WebFetch`, read-only MCP tools render a one-line summary by default;
- details remain available through disclosure and the right sidebar;
- `Bash`, `Edit`, `Write`, `ApplyPatch`, permission-sensitive calls remain visible and inspectable;
- repeated successful compact calls can be grouped by tool family and turn.

### Phase 6: ToolCallsPanel Audit Improvements

The side panel remains the complete audit trail.

Update `src/components/workspace/ToolCallsPanel.tsx`:

- include hidden state/status tools;
- add a display-policy badge such as `hidden from chat`, `status`, `task`, `inline`;
- add filters for tool family and visibility;
- canonicalize duplicate names so `TaskCreate` and `taskcreate` do not appear as separate tool families;
- keep jump-to-message behavior even when the transcript item is hidden by jumping to nearest visible sibling or opening the panel detail.

### Phase 7: Tests And Visual Checks

Unit tests:

- `tests/unit/tool-display-policy.test.ts`;
- update `tests/unit/tool-call-batches.test.ts`;
- update `tests/unit/tool-calls.test.ts`;
- update `tests/unit/message-normalizer.test.ts` if display hints are added to VM types.

E2E / visual:

- update `tests/shell.spec.ts` assertions for hidden task cards;
- add fixture where `TaskCreate` + `TaskUpdate` produce a task panel;
- add fixture where `SystemStatus` updates footer instead of transcript;
- run `npm run test:unit`;
- run `npm run typecheck`;
- run `npm run build`;
- run focused Playwright for `tool_use` fixture and tools panel.

---

## 6. Data Model Changes

Prefer adding derived fields instead of changing backend contracts.

Candidate additions to `ToolUseMessageVM`:

```ts
displayPolicy?: ToolDisplayPolicy
canonicalToolName?: string
```

Candidate additions to `ToolCallRecord`:

```ts
canonicalToolName: string
family: ToolDisplayPolicy['family']
transcriptVisibility: ToolDisplayPolicy['transcript']
surface: ToolDisplayPolicy['surface']
```

If backend later emits explicit display hints, treat them as overrides only after validation.

---

## 7. Acceptance Criteria

- Successful `TaskCreate`, `TaskGet`, `TaskList`, `TaskUpdate`, and `TodoWrite` do not render as normal chat tool cards.
- Task state is visible through a compact task panel or status surface.
- System/status tools update footer/status UI instead of becoming chat cards.
- Any failed tool call remains visible in the transcript.
- `ToolCallsPanel` still lists every tool call, including hidden-from-chat calls.
- Case-only or alias-only tool name differences do not create duplicate tool groups.
- Existing Bash/Edit/Write debugging workflows remain inspectable.
- Mobile and narrow desktop layouts do not overlap text, buttons, or badges.

---

## 8. Rollout Strategy

1. Ship the policy module and tests behind pure derived behavior.
2. Hide only task/status successful calls first.
3. Add task/status surfaces.
4. Compact read/search tools after task/status behavior is stable.
5. Update visual fixtures and docs.

Rollback is straightforward if filtering is implemented as a derived render step: disable `shouldRenderToolUseInTranscript` checks and the raw messages remain intact.

---

## 9. Open Questions

- Do we already receive structured task state from the backend, or must the frontend infer it from tool input/result text for the first pass?
- Should hidden calls count toward the collapsed tool-call summary in the transcript, or only in the ToolCallsPanel?
- Should user settings expose a "show all tool calls" transcript mode for debugging?
- Should status footer updates persist per session or clear on turn completion?
