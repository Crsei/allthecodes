# Unified Discovery Search API Plan

> Status: Implemented on 2026-07-16
> Audit date: 2026-07-16
> Runtime source: `crates/allthecodes-tools/src/discovery_search.rs`

## Implementation Result (2026-07-16)

Implemented in `53254712` and included in the backend artifacts refreshed by
`f9dc6d76`:

- one typed, read-only operation reuses the MCP/plugin discovery providers and
  existing scorer rather than maintaining a Web index;
- the trusted workspace comes from the bound engine, not request parameters;
- provider failure, panic, timeout, and budget truncation produce stable typed
  partial results without hiding successful providers;
- default projection removes contribution detail, tool schemas, credentials,
  host paths, remote URLs, and unsafe follow-up actions; and
- the endpoint performs no mutation and does not replace mention autocomplete.

Targeted protocol, tools, and Web discovery tests pass, including Web discovery
7/7 and plugin search 2/2. Mention autocomplete remains unchanged; no claim is
made that this change added a separate autocomplete regression suite.

## Goal

Expose one bounded, typed, read-only API for discovering MCP capabilities,
plugins, and their summarized skill/tool contributions. The API must reuse the
existing discovery providers and ranking behavior rather than implement a second
search index in `allthecodes-web`.

The first version is intentionally local-only and preview-oriented. It returns
enough information to explain why a result matched and what safe follow-up the
client may offer, but it does not execute the follow-up action or return callable
tool schemas.

## Not Mention Autocomplete

`GET /api/mentions/autocomplete` remains a prompt-composer helper. It searches
sessions, workspace files, and skills, then returns compact chip targets with
`kind`, `name`, `target`, and `description`.

Unified discovery has a different contract:

- It searches installed runtime providers, initially MCP and plugins.
- It uses the scoring and identifier-token matching in `discovery_search.rs`.
- It returns match reasons, provider status, capability summaries, and a safe
  next action.
- It never scans workspace files or sessions.
- It is an explicit search request, not prefix completion on every keystroke.

The two endpoints must not call each other or merge response DTOs.

## Route

```http
GET /api/discovery/search
```

Example:

```http
GET /api/discovery/search?q=github+pull+request&provider=all&mcp_scope=runtime&limit=20&include_summaries=false
```

Register the route through `allthecodes-protocol` and `HandlerRegistry`; do not
add a standalone route directly to `build_router()`.

## Typed Request DTO

Add `crates/allthecodes-protocol/src/v1/discovery.rs` with transport-only DTOs.
Business search types and ranking remain owned by `allthecodes-tools`.

```rust
pub struct DiscoverySearchQuery {
    pub q: String,
    pub provider: DiscoveryProviderSelector,
    pub mcp_scope: McpDiscoveryScope,
    pub plugin_source: PluginDiscoverySource,
    pub kind: Option<DiscoveryResultKindDto>,
    pub limit: Option<u16>,
    pub include_summaries: Option<bool>,
}

pub enum DiscoveryProviderSelector {
    All,
    Mcp,
    Plugin,
}

pub enum McpDiscoveryScope {
    All,
    User,
    Project,
    Runtime,
}

pub enum PluginDiscoverySource {
    All,
    Installed,
    Active,
    MarketplaceCache,
}
```

Defaults:

- `provider=all`
- `mcp_scope=all`
- `plugin_source=all`
- `limit=20`
- `include_summaries=false`

Reject an empty query, a query longer than 256 UTF-8 characters, unknown enum
values, and `limit=0`. Clamp neither silently nor differently by provider; reject
limits above 100 with `400 invalid_limit`.

## Typed Response DTO

```rust
pub struct DiscoverySearchResponse {
    pub query: String,
    pub results: Vec<DiscoverySearchItem>,
    pub providers: Vec<DiscoveryProviderState>,
    pub partial: bool,
    pub truncated: bool,
    pub returned: usize,
}

pub struct DiscoveryProviderState {
    pub provider: DiscoveryProviderKind,
    pub status: DiscoveryProviderStatus,
    pub returned: usize,
    pub error: Option<DiscoveryProviderError>,
}

pub enum DiscoveryProviderStatus {
    Ok,
    Unavailable,
    Failed,
    TimedOut,
    Truncated,
}

pub struct DiscoveryProviderError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
```

`DiscoverySearchItem` should be a typed, transport-safe projection of the
existing `DiscoverySearchResult`. It may contain:

- kind, name, stable id, display name, provider/source, and server name;
- version, description, `when_to_use`, invocation flags, and status summary;
- capability, tool, and skill summaries when allowed by the request;
- `match_reasons` from the existing scorer;
- a sanitized `next_action` containing label and command text;
- discovery signal and the existing inert remote-discovery state.

Do not expose the numeric internal score. Preserve the existing deterministic
ordering: descending score, then name, then id. A response-local `rank` may be
derived after sorting if the frontend needs one.

## Reuse Boundary

The web handler must not copy `score_item`, `text_score`, identifier tokenization,
source filtering, result redaction, or next-action construction.

Extend `allthecodes-tools` with a typed provider-collection entry point that
returns one outcome per provider while preserving the existing `McpSearch` and
`PluginSearch` tool behavior. Provider collection must accept a trusted
`DiscoveryContext` (or equivalent) containing the server-selected workspace;
the current zero-argument provider closure is not sufficient for project-scoped
discovery. Existing infallible provider closures can be adapted to successful
outcomes. Missing providers, provider panics, and future fallible providers must
become typed provider states instead of aborting results from other providers.

The handler then:

1. Validates and normalizes the typed query.
2. Collects only the requested providers.
3. Applies the existing per-provider redaction rules.
4. Merges successful result sets.
5. Reuses the existing scorer for one final deterministic ranking.
6. Truncates once at the request-wide limit.

## Trusted Workspace Context

At request start, snapshot the active engine and derive the workspace from
`WebState::engine().cwd()`. Pass that trusted path through the discovery facade
to every provider that resolves project-scoped MCP/plugin state. The query DTO
must not accept `cwd`, a workspace key, or a filesystem path.

The installed MCP provider currently calls `std::env::current_dir()` when it
collects configured project rows. Replace that process-global lookup with the
workspace in `DiscoveryContext`; changing the shell/daemon process directory
must not change a request's results. If provider results are cached later, the
canonical workspace identity and relevant configuration revision are mandatory
cache-key inputs so one project cannot observe another project's discovery
rows.

## Partial Provider Errors

A valid search returns `200` even when one requested provider is unavailable,
fails, or times out. The response sets `partial=true` and records the provider
error without dropping successful results from other providers.

If every requested provider is unavailable or failed, return `200` with an empty
result list and typed provider states. Provider availability is discovery data,
not an HTTP transport failure. Use `500` only when the aggregator itself cannot
construct a valid response.

Never return provider panic payloads or backtraces. Map them to a stable code
such as `provider_failed` and a bounded public message.

## Safety and Redaction

- The endpoint is read-only and concurrency-safe; it cannot install, enable,
  invoke, or mutate a discovered item.
- `include_summaries=false` is the safe default. Reuse current behavior that
  clears tool/skill summaries and additionally removes plugin contribution
  arrays (`skills`, `tools`, and `mcp_servers`).
- Never return callable tool input schemas, auth material, environment values,
  headers, plugin configuration bodies, executable paths, or provider panic
  text.
- Sanitize `next_action.command`. Allow only known discovery/navigation forms;
  it is display text, never a command executed by the endpoint.
- Preserve `remote_url_todo` and `remote_source` placeholders. Do not introduce
  a `remote_url` field until remote URL discovery has its own reviewed contract.
- Bound every string and contribution list in the response. Mark the response or
  provider as truncated rather than emitting an unbounded payload.

## Runtime Budgets

- Maximum query length: 256 characters.
- Maximum returned results: 100; default 20.
- Maximum collected candidates: 5,000 per provider.
- Maximum contribution summaries: 50 per result before final response trimming.
- Maximum serialized response: 256 KiB.
- Maximum wait per provider: 500 ms, with a request-wide deadline of 750 ms.
- Run synchronous provider collection through a bounded blocking pool and a
  semaphore. A timed-out blocking task must not cause unbounded follow-up tasks.

Budget exhaustion yields a partial/truncated response, not a different ranking
algorithm.

## Implementation Files

- `crates/allthecodes-protocol/src/v1/discovery.rs`
- `crates/allthecodes-protocol/src/v1/mod.rs`
- `crates/allthecodes-protocol/src/request.rs`
- `crates/allthecodes-tools/src/discovery_search.rs`
- `crates/allthecodes-web/src/handlers/discovery.rs`
- `crates/allthecodes-web/src/handlers/mod.rs`
- `crates/allthecodes-web/src/handler_registry.rs`
- generated API artifacts covered by
  `17-api-generated-artifact-freshness-plan.md`

## Tests

Add tests for:

- typed query defaults, enum validation, empty/oversized query, and limit bounds;
- exact reuse of scorer ordering and `match_reasons`;
- deterministic tie-breaking by name and id;
- safe `next_action` projection without execution;
- `include_summaries=false` redaction for MCP and plugin results;
- no remote URL, credentials, schemas, configuration, or absolute executable
  paths in serialized results;
- MCP success plus plugin failure returns `200`, successful results, and
  `partial=true`;
- missing, panicking, timed-out, and truncated providers produce stable typed
  states;
- engine workspace A and B return only their own project-scoped providers even
  when the process current directory points at a third workspace;
- a request cannot override its trusted workspace through query parameters,
  provider options, or cached results;
- total result, candidate, summary, time, and response-byte budgets;
- route registration and protocol REST/JSON-RPC serialization parity;
- `/api/mentions/autocomplete` retains its existing session/file/skill chip
  contract and is not backed by discovery search.

Run the narrow gates:

```bash
cargo test -p allthecodes-tools discovery_search
cargo test -p allthecodes-web discovery
cargo test -p allthecodes-protocol discovery
```

## Acceptance

- One request can search MCP and plugin discovery providers with deterministic
  ranking and bounded output.
- Results contain match reasons and safe next actions from the existing runtime
  model.
- One provider can fail without suppressing another provider's results.
- Default responses redact contribution detail and never expose secrets or tool
  schemas.
- The endpoint performs no mutations and does not replace mention autocomplete.
- Protocol, handler registry, generated docs, schemas, OpenAPI, and TypeScript
  artifacts describe the same typed contract.
