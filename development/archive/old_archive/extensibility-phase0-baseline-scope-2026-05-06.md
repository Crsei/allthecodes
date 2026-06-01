# Extensibility Phase 0 Baseline And Scope Lock

Date: 2026-05-06
Plan: `.omx/plans/extensibility-implementation-phase-plan-2026-05-06.md`
Source map: `architecture/extensibility-implementation-map.md`

## Decision

Phase 0 is a baseline and scope-lock phase. It does not change runtime behavior. It records the existing regression coverage for the already implemented Extensibility lanes, confirms the local upstream-reference docs that later phases must use, and fixes the implementation order for the remaining MCP/custom-agent work.

Phase 1 may start with `/mcp reconnect` and IPC/runtime reconnect integration. Hooks and skills are treated as regression baselines, not active feature work.

## Baseline Coverage

The current implemented behavior is already covered by targeted tests:

| Lane | Baseline evidence |
| --- | --- |
| MCP stdio / local loopback SSE / JSON-RPC / manager lifecycle / channel notifications | `cargo test -p cc-mcp` passed: 38 tests. Coverage includes manager connect/disconnect/reconnect failure cleanup, stdio missing-command rejection, loopback SSE endpoint initialization, HTTPS rejection placeholder, insecure remote HTTP rejection, header injection rejection, JSON-RPC serialization/deserialization, discovery scope merging, and channel notification routing. |
| Skills loading / registration / invocation gates | `cargo test -p cc-skills` passed: 30 tests. Coverage includes bundled skill registration, directory reload, legacy command compatibility, frontmatter parsing, dependency ordering, version conflicts, cycle/missing dependency rejection, app-version compatibility, prompt expansion, source variants, and model invocation gates. |
| Hook permission ordering | `cargo test -p cc-permissions` passed: 101 unit tests + 1 doctest. Coverage includes deny/ask/allow ordering, hook deny overriding allow, deny/ask rules overriding hook allow, hook allow lifting default ask, hook input modification, session grants, permission modes, bash matching, path validation, and dangerous command detection. |
| Custom agent settings | `cargo test -p claude-code-rs agent_settings` passed: 20 tests. Coverage includes parsing/rendering populated frontmatter, editable source flags, name validation, path safety for editor open, project-scope upsert/reload, builtin rejection, tools wildcard normalization, permission mode roundtrip, builtins listing, and generated-agent parsing diagnostics. |
| Agent runtime/tool filtering | `cargo test -p claude-code-rs engine::agent` passed: 75 tests. Coverage includes active child tool boundaries, builtin read-only tool filtering, custom allow/deny tool specs and dedupe, background agent events, worktree isolation detection, fork params, supervisor cancellation, agent schema, model alias resolution, and SDK event conversion. |
| Skill tool runtime | `cargo test -p claude-code-rs tools::skill` passed: 7 tests. Coverage includes schema/name rendering, prompt message construction, skills listing, missing skill validation, and existing skill validation. |
| Hook runtime helpers | `cargo test -p claude-code-rs tools::hooks` passed: 20 tests. Coverage includes hook config loading, matcher behavior, JSON/plain-text hook output parsing, pre-tool deny/allow/input update, post-tool/stop/event hook behavior, and Windows pipe bug documentation. |

Additional compile gate:

- `cargo check -p claude-code-rs --message-format short` passed with no warnings after the baseline run.

## Upstream Reference Confirmation

Local upstream-reference docs are available under `architecture/extensibility/` and were inspected before starting transport/auth work:

- `architecture/extensibility/custom-agents.mdx`
- `architecture/extensibility/hooks.mdx`
- `architecture/extensibility/mcp-configuration.mdx`
- `architecture/extensibility/mcp-protocol.mdx`
- `architecture/extensibility/skills.mdx`

Later MCP transport/auth phases must use these local docs plus the TypeScript/Bun source references named by `architecture/extensibility-implementation-map.md` before changing protocol behavior.

## Implementation Order

The remaining work stays in this order:

1. Phase 1 - MCP lifecycle and reconnect integration.
2. Phase 2 - Remote HTTPS SSE transport.
3. Phase 3 - MCP OAuth and interactive auth.
4. Phase 4 - Full MCP transport matrix.
5. Phase 5 - Custom agent safety contract.
6. Phase 6 - Integration verification and documentation closure.

## Constraints

- No new dependency is approved by Phase 0. Later phases must first prove existing workspace dependencies are insufficient.
- Persistent paths must stay isolated under `~/.cc-rust/` and project `.cc-rust/`.
- Keychain service names must remain `cc-rust`.
- Hooks and skills are not reopened for feature changes unless a later phase breaks their regression baselines.
- The map currently has no evidence-backed `故意裁剪` item for Extensibility, so later phases must implement, explicitly defer with rationale, or document a real intentional crop.

## Test Gaps

- Phase 0 did not run `cargo test --workspace --lib` or `cargo build --release`; those remain Phase 6 gates.
- Phase 0 did not add new runtime tests because the existing targeted tests already cover the current implemented lanes. Phase 1 must add or update reconnect-specific command/IPC tests when behavior changes.
- Remote HTTPS SSE, OAuth, Streamable HTTP, WebSocket, and IDE transport behavior remain intentionally unimplemented until their phases.

