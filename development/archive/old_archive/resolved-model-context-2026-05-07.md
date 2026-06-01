# Resolved Model And Context Issues - 2026-05-07

| Historical ID | Title | Closure |
| --- | --- | --- |
| BUILD-001 | `plugins/tools.rs` partial move compile blocker | Verified closed by `cargo test -p claude-code-rs plugin_tool`; no active compile blocker remains on this issue. |
| MODEL-001 | Bedrock `claude-opus-4-7` mapped to an invalid versioned ID | Fixed by mapping to the current Bedrock inference ID `us.anthropic.claude-opus-4-7` and covering it with model-mapping tests. |
| MODEL-002 | Removed `opus` / `sonnet` / `haiku` aliases had no explicit path | Closed by rejecting removed aliases at user/config/agent entry points and documenting `SOTA` / `MOTA` / `FOTA` replacements. |
| DOC-001 | `/model` and configuration docs described stale aliases | Closed by updating command and configuration docs to the `SOTA` / `MOTA` / `FOTA` alias policy. |
| CONTEXT-001 | Auto-compact threshold double-counted local token savings | Fixed by comparing the post-pipeline estimate directly and keeping freed-token counters diagnostic-only. |
| CONTEXT-002 | Exact-count preflight omitted system prompt and tools | Fixed by shaping exact-count requests from the same turn boundary as the final provider request. |
