# Structured Brief ACP Wire Protocol Extension Plan

> **For agentic workers:** Use `test-driven-development` before implementation and `verification-before-completion` before claiming this task is done. Keep ACP stdout protocol-pure: only JSON-RPC frames may be written to stdout in ACP mode.

**Goal:** Preserve KAIROS structured Brief output over ACP instead of degrading `SdkMessage::BriefMessage` to plain `AgentMessage` text.

**Current limitation:** `crates/allthecodes-acp/src/updates.rs` maps `SdkMessage::BriefMessage` through `brief_message_update(...)` into `SessionUpdate::AgentMessage` with only `brief.message`. This keeps ACP clients usable, but drops `status`, `attachments`, `level`, `source_tool_name`, `tool_use_id`, `session_id`, and `timestamp`.

**Required outcome:** ACP clients that opt into structured Brief support receive a wire-level structured update. Clients without that support keep receiving the current text fallback.

## Constraints

- Do not break existing ACP v2 clients using `agent-client-protocol-schema = "=1.2.0"`.
- Do not advertise structured Brief support until the wire shape, negotiation, mapper, and tests exist.
- Keep legacy `--headless` JSONL IPC unchanged. This task is ACP-only.
- Preserve all Brief fields already carried by `allthecodes_types::brief::BriefMessagePayload`.
- Keep stdout JSON-RPC-only in ACP mode.

## Proposed Wire Shape

Implemented using ACP v2's custom `SessionUpdate::Other` extension slot, so the schema remains `agent-client-protocol-schema = "=1.2.0"` and no local fork is needed. The negotiated update discriminator is `_allthecodes_structured_brief`.

Required payload fields:

```json
{
  "sessionUpdate": "_allthecodes_structured_brief",
  "sessionId": "session-id",
  "messageId": "brief-msg-1",
  "message": "Build finished.",
  "status": "normal",
  "attachments": [],
  "level": "info",
  "sourceToolName": "Brief",
  "toolUseId": "toolu_...",
  "sourceSessionId": "allthecodes-session-id",
  "timestamp": 1783273215000
}
```

Use `sourceSessionId` for the Brief payload's internal allthecodes session id so it does not collide with ACP `sessionId`.

## Implementation Tasks

- [x] Inspect `agent-client-protocol-schema` v2 generated types and identify whether `SessionUpdate`, `ContentBlock`, or `_meta` can carry a structured allthecodes extension without forking the schema.
- [x] Define the negotiated client capability name. Suggested private capability: `allthecodes.structuredBrief`.
- [x] Add ACP runtime capability negotiation storage so each session knows whether structured Brief updates are allowed.
- [x] Add a structured Brief conversion path in `crates/allthecodes-acp/src/updates.rs`.
- [x] Keep the current `AgentMessage` text fallback when the client does not advertise `allthecodes.structuredBrief`.
- [x] Ensure structured Brief updates preserve `message`, `status`, `attachments`, `level`, `source_tool_name`, `tool_use_id`, `session_id`, and `timestamp`.
- [x] Add deterministic message ids for Brief updates, separate from normal `agent-msg-*` ids.
- [x] Update `development/acp/README.md` only after implementation is verified.

## Test Plan

- [x] Add mapper tests proving `SdkMessage::BriefMessage` emits the structured ACP update when `allthecodes.structuredBrief` is negotiated.
- [x] Add mapper tests proving old clients still receive the existing `AgentMessage` text fallback.
- [x] Add JSON serialization tests for the exact wire payload field names.
- [x] Add runtime negotiation tests for supported and unsupported clients.
- [x] Add stdio smoke coverage proving the new update remains valid JSON-RPC on stdout.

## Verification

Run with the repository Rust environment from `AGENTS.md`:

```bash
cargo test -p allthecodes-acp brief
cargo test -p allthecodes-acp --test prompt_updates
cargo test -p allthecodes-acp --test protocol_methods
cargo test -p allthecodes --test acp_stdio_smoke
cargo check -p allthecodes-acp
cargo check --workspace
```

## Exit Criteria

- Structured Brief fields survive ACP transport for opted-in clients.
- Non-opted-in clients keep the current text behavior.
- The ACP README no longer lists structured Brief as a limitation.
- No stdout protocol pollution is introduced.
