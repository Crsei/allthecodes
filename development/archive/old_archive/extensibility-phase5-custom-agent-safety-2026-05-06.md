# Extensibility Phase 5 - Custom Agent Safety Contract

Date: 2026-05-06

## Decision

Custom agent definitions now have an explicit runtime safety contract instead
of relying on incidental tool filtering:

- child `QueryEngine` instances inherit the parent `ToolPermissionContext`,
  including allow/deny/ask/session rules;
- user and project agent `permissionMode` applies only from the default parent
  permission mode;
- parent `auto`, `bypass`, `acceptEdits`, `plan`, and `dontAsk` modes remain
  authoritative for children;
- plugin agent `permissionMode` is ignored, matching the upstream rule that
  plugin-contributed agents cannot use `hooks`, `mcpServers`, or
  `permissionMode`;
- `tools` and `disallowedTools` are normalized before persistence;
- `disallowedTools` wins over `tools`, and `disallowedTools: *` denies every
  tool;
- wildcard tool specs such as `mcp__server__*` match namespaced tools by
  prefix;
- `isolation` is normalized to `worktree`, and editable definitions reject
  unknown isolation modes;
- `maxTurns: 0` is rejected for editable definitions.

## Implementation Notes

The child engine config now carries an inherited permission context through
`AgentContext`, and `QueryEngine::new` installs that context into child
`AppState`. Agent definition defaults now apply to model, background execution,
worktree isolation, teammate mode, and `maxTurns`.

When worktree isolation changes the child cwd, definition lookup falls back to
the parent process cwd so project custom-agent policy is not lost merely because
`.cc-rust/agents` is not present in the temporary worktree.

The current phase intentionally does not activate per-agent `skills`, `hooks`,
or `mcpServers` runtime expansion. Those fields continue to parse and
round-trip. Future activation must pass through the same inherited permission
context and tool allow/deny filtering.

## Verification

- `cargo test -p claude-code-rs agent_settings`
- `cargo test -p claude-code-rs child_tool_boundary_tests`
- `cargo test -p claude-code-rs test_query_engine_inherits_agent_permission_context`
- `cargo test -p claude-code-rs engine::agent`
- `cargo check -p cc-engine --message-format short`
- `cargo check -p claude-code-rs --message-format short`

## Sources

- Official custom subagents documentation: https://docs.anthropic.com/en/docs/claude-code/sub-agents
