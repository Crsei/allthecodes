# Plugin Install Backend Implementation Plan

Date: 2026-06-21

## Goal

Implement the backend/runtime side required by the Web/Desktop plugin marketplace:

- Fetch official marketplace entries from `https://allthecodes.cc/api/plugins/marketplace`.
- Install official plugin zip artifacts without user unzip/build/config steps.
- Validate zip contents and `plugin.json` before registration.
- Store installed plugin state in a versioned local registry.
- Register plugin skills and MCP servers from the manifest.
- Inject the current allthecodes account OAuth token when launching plugin MCP servers.
- Keep token material out of plugin directories, logs, registry files, and API responses.

The paired frontend plan lives in:

```text
/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web-fix-bugs/development-docs/martket/plugin-plan.md
```

## Current Backend State

Relevant existing modules:

- `crates/allthecodes-plugins/src/installation.rs`
  - Existing installer accepts a generic `source` string and supports local paths, URLs, GitHub, npm, mcpb, zip, and tgz.
  - Existing install target is under the plugin cache and writes `installed_plugins.json`.
- `crates/allthecodes-plugins/src/marketplace.rs`
  - Existing marketplace cache is seeded with the legacy `superpowers` entry.
  - `MarketplacePluginEntry` already has `id`, `version`, `download_url`, `checksum`, `homepage`, and tags.
- `crates/allthecodes-plugins/src/zip_cache.rs`
  - Existing zip extraction rejects basic path traversal through `ZipFile::enclosed_name()`.
  - It does not yet enforce file count limits, decompressed size limits, symlink rejection, or official manifest id/version matching.
- `crates/allthecodes-plugins/src/manifest.rs`
  - Existing `PluginManifest` has `name`, `version`, `skills`, and `mcp_servers`.
  - Validation does not yet reject absolute plugin MCP commands or skill paths escaping the plugin root.
- `crates/allthecodes-plugins/src/mod.rs`
  - `discover_plugin_mcp_servers_scoped()` converts manifest MCP servers into `McpServerConfig`.
  - It currently resolves relative commands but does not inject account OAuth env vars.
- `crates/allthecodes-mcp/src/client/stdio.rs`
  - Existing MCP stdio launcher applies `McpServerConfig.env` to spawned processes and logs stderr lines.
  - It does not currently redact plugin token values from stderr.
- `crates/allthecodes-web/src/handlers/plugins.rs`
  - Existing REST API exposes `GET /api/plugins`, `GET /api/plugins/marketplace`, `POST /api/plugins/install`, and `POST /api/plugins/{id}/uninstall`.
  - `POST /api/plugins/install` currently accepts `{ "source": "...", "scope": "user" }`.
- `crates/allthecodes-protocol/src/v1/plugins.rs`
  - Generated frontend contract still mirrors the legacy install request.
- `crates/allthecodes-web/src/handlers/auth.rs`
  - Existing desktop account auth can refresh and hold an `AccountAuthSession` with `access_token`.
  - Plugin launch needs an internal helper that returns an unexpired token without exposing it over the API.

## Target API Surface

Keep existing routes compatible, then add the plan-required aliases and lifecycle operations.

### Marketplace

```text
GET /api/plugins/marketplace
```

Return official entries from `https://allthecodes.cc/api/plugins/marketplace`, cached in memory and optionally on disk. Response should keep the current shape:

```json
{
  "plugins": [
    {
      "id": "eco-boost",
      "version": "0.1.0",
      "homepage": "https://allthecodes.cc/plugins/eco-boost",
      "download_url": "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip",
      "source_name": "official-allthecodes"
    }
  ],
  "sources": []
}
```

Validation:

- `download_url` host must be `allthecodes.cc` for official entries.
- `download_url` scheme must be `https`.
- `id` and `version` must be non-empty.
- Future-compatible fields: accept `sha256` and map it to the existing `checksum` field or expose both.

### Installed Plugins

```text
GET /api/plugins
GET /api/plugins/installed
```

`/api/plugins` remains the compatibility route. `/api/plugins/installed` is an alias for the frontend plan.

### Install

```text
POST /api/plugins/install
```

New official install body:

```json
{
  "id": "eco-boost",
  "version": "0.1.0",
  "download_url": "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip",
  "sha256": null
}
```

Compatibility:

- Continue accepting the legacy `{ "source": "...", "scope": "user" }` request for CLI/local installs.
- Route official installs through a new strict path, not through generic source resolution.

Response:

```json
{
  "plugin": {
    "id": "eco-boost",
    "version": "0.1.0",
    "enabled": true,
    "status": "installed"
  },
  "install_path": "<ALLTHECODES_HOME>/plugins/eco-boost/0.1.0",
  "fresh_install": true,
  "status": "installed"
}
```

### Lifecycle Operations

Add these routes while keeping `POST /api/plugins/{id}/uninstall` working:

```text
POST /api/plugins/update
POST /api/plugins/uninstall
POST /api/plugins/enable
POST /api/plugins/disable
POST /api/plugins/restart
```

Request bodies should use explicit plugin IDs:

```json
{ "id": "eco-boost" }
```

`update` also accepts the same official marketplace fields as install. `restart` should reconnect plugin MCP servers after env injection is ready.

## Local Storage Layout

Use versioned install roots so updates do not overwrite a running plugin:

```text
<ALLTHECODES_HOME>/plugins/<plugin-id>/<version>/
<ALLTHECODES_HOME>/plugins/installed_plugins.json
```

Example:

```text
~/.allthecodes/plugins/eco-boost/0.1.0/plugin.json
~/.allthecodes/plugins/eco-boost/0.1.0/eco/target/release/mcp-cli-bridge
```

Prefer extending the existing `installed_plugins.json` v2 format rather than adding a second registry. Add fields to `PluginEntry` only with serde defaults to preserve older files:

- `enabled: bool` or continue mapping `PluginStatus::Installed`/`Disabled`.
- `root` or reuse `cache_path`.
- `download_url`.
- `source_url` or `source` metadata.
- `manifest` raw copy, if needed for re-validation/debugging.
- `official: bool`.
- `sha256`.

Do not store `ALLTHECODES_COM_ACCESS_TOKEN` or refresh tokens in any plugin registry file.

## Security Requirements

The official zip path must be stricter than the generic legacy installer.

### Download

- Only install from the marketplace entry `download_url` supplied by `allthecodes.cc`.
- Require HTTPS and host `allthecodes.cc`.
- Use `reqwest::Client` with explicit timeout.
- Enforce a max compressed download size before buffering.
- Accept `application/zip` and `application/octet-stream`; reject clear non-zip content types.
- If `sha256` is present, hash the downloaded bytes before extraction.
- Do not delete or modify the currently installed version when download fails.

Suggested limits for Phase 1:

- Compressed zip max: `128 MiB`.
- Expanded total max: `512 MiB`.
- File count max: `10_000`.
- Single file max: `256 MiB`.

### Extraction

Implement a new strict extractor in `crates/allthecodes-plugins/src/zip_cache.rs` or a new module such as `official_zip.rs`.

Reject:

- Absolute paths.
- `..` components.
- Windows drive prefixes and UNC prefixes.
- Symlinks and hard links.
- Entries that canonicalize outside the extraction temp directory.
- Zip archives with more than the allowed entry count.
- Archives whose expanded size exceeds the allowed total.

Flow:

1. Extract to `<ALLTHECODES_HOME>/plugins/.tmp/<id>-<version>-<nonce>/`.
2. Locate `plugin.json` at zip root or under a single top-level directory.
3. Validate the manifest.
4. Move the normalized plugin root to `<ALLTHECODES_HOME>/plugins/<id>/<version>/`.
5. Update `installed_plugins.json` only after the move succeeds.
6. Leave old versions in place; add garbage collection later.

### Manifest Validation

For official installs, validate more than the generic manifest validator:

- `plugin.json.name == request.id`.
- `plugin.json.version == request.version`.
- `mcp_servers[].command` must be relative and must resolve under plugin root.
- `skills[].path` must be relative and must resolve under plugin root.
- Reject empty command/path values.
- Reject manifest-declared remote update/download URLs as authoritative sources.
- Preserve manifest `args` and plugin env values, but allthecodes-owned OAuth env vars must override plugin-provided values.

On Unix, preserve executable bits from the zip for binaries. On Windows, `.exe` files are expected in official Windows artifacts.

## OAuth Injection

Plugin MCP servers need these environment variables at launch:

```text
ALLTHECODES_COM_ACCESS_TOKEN=<current access token>
ALLTHECODES_COM_BASE_URL=https://allthecodes.cc
```

Implementation options:

1. Add an internal helper in `crates/allthecodes-web/src/handlers/auth.rs` or a shared auth module:
   - returns an unexpired account session token;
   - refreshes via the stored refresh token when needed;
   - returns typed errors for signed out, refresh failed, and unavailable account auth.
2. Add a plugin MCP env provider:
   - either in `crates/allthecodes-plugins/src/mod.rs` before building `McpServerConfig.env`;
   - or in `crates/allthecodes-mcp` as a launch-time env hook.

Preferred approach:

- Keep token retrieval in the web/runtime layer because `allthecodes-plugins` should not depend on account auth internals.
- Add a small plugin launch env hook to `allthecodes_mcp::McpServerConfig` creation path or to `McpClient::connect_stdio`.
- Tag plugin-origin MCP configs so only plugin MCP servers receive the account token.

Required behavior:

- If not logged in, plugin MCP launch fails with a clear auth error.
- If token is expired, refresh before launch.
- If refresh fails, do not launch the plugin process.
- Token value must be redacted from diagnostics, stderr logs, tracing fields, and API payloads.

## Implementation Phases

### Phase 1: Protocol and API Compatibility

Files:

- `crates/allthecodes-protocol/src/v1/plugins.rs`
- `crates/allthecodes-protocol/src/request.rs`
- `crates/allthecodes-web/src/handlers/plugins.rs`
- `crates/allthecodes-web/src/handler_registry.rs`

Tasks:

- Add official install/update request structs.
- Keep legacy install request support through an untagged enum or optional fields.
- Add `/api/plugins/installed` alias.
- Add route entries for update, uninstall, enable, disable, restart.
- Return stable `status`, `enabled`, `installed_version`, `download_url`, and `homepage` fields for frontend rendering.

Acceptance:

- Existing plugin handler tests still pass.
- New tests prove both legacy `{ source }` and official `{ id, version, download_url }` request shapes deserialize.

### Phase 2: Official Marketplace Source

Files:

- `crates/allthecodes-plugins/src/marketplace.rs`
- `crates/allthecodes-web/src/handlers/plugins.rs`

Tasks:

- Add `OFFICIAL_MARKETPLACE_URL = "https://allthecodes.cc/api/plugins/marketplace"`.
- Fetch and cache official marketplace entries.
- Validate official entry URL host and scheme.
- Include `allthecodes-bridge-cli` and `eco-boost` when the official endpoint is reachable.
- Decide fallback behavior:
  - use last cached official marketplace if refresh fails;
  - include an API diagnostic when serving stale data.

Acceptance:

- Mocked marketplace response with the two official plugins is returned by `GET /api/plugins/marketplace`.
- Entries with non-`allthecodes.cc` `download_url` are rejected or marked invalid.

### Phase 3: Strict Official Zip Installer

Files:

- `crates/allthecodes-plugins/src/installation.rs`
- `crates/allthecodes-plugins/src/zip_cache.rs` or new `official_zip.rs`
- `crates/allthecodes-plugins/src/manifest.rs`
- `crates/allthecodes-plugins/src/loader.rs`

Tasks:

- Add `install_official_plugin(request)` path.
- Download to memory or a temp file with max-size enforcement.
- Add strict zip extractor with count/size/symlink/path checks.
- Normalize root directory.
- Validate manifest id/version/path rules.
- Write versioned install root under `<ALLTHECODES_HOME>/plugins/<id>/<version>`.
- Update registry atomically after successful install.

Acceptance:

- Valid `eco-boost@0.1.0` and `allthecodes-bridge-cli@0.1.0` zips install into versioned roots.
- Existing installed version remains selected when download or validation fails.
- Malicious zip-slip archives fail before writing outside the temp directory.
- Manifest id/version mismatch fails.

### Phase 4: Enable, Disable, Uninstall, Update

Files:

- `crates/allthecodes-plugins/src/installation.rs`
- `crates/allthecodes-plugins/src/loader.rs`
- `crates/allthecodes-web/src/handlers/plugins.rs`
- `crates/allthecodes/src/app_subsystem_handlers/*` if TUI/runtime commands need parity.

Tasks:

- `enable` maps registry status to `PluginStatus::Installed`.
- `disable` maps status to `PluginStatus::Disabled`.
- `uninstall` removes registry entry; optional purge deletes the version root.
- `update` installs the new version first, then switches the registry pointer.
- Keep old version roots until a later garbage collection task.

Acceptance:

- Disabled plugins are omitted from `discover_plugin_mcp_servers_scoped()` and skill discovery.
- Uninstall removes the plugin from `GET /api/plugins/installed`.
- Update failure preserves the old installed version.

### Phase 5: OAuth Injection and MCP Restart

Files:

- `crates/allthecodes-web/src/handlers/auth.rs`
- `crates/allthecodes-plugins/src/mod.rs`
- `crates/allthecodes-mcp/src/client/stdio.rs`
- `crates/allthecodes/src/app_runtime_adapters/mod.rs`

Tasks:

- Add internal account token helper with refresh.
- Mark plugin-contributed MCP configs or pass plugin id through scoped discovery.
- Inject `ALLTHECODES_COM_ACCESS_TOKEN` and `ALLTHECODES_COM_BASE_URL`.
- Ensure allthecodes-owned env vars override manifest env values.
- Redact token in MCP stderr/debug logging.
- Implement `POST /api/plugins/restart` by reconnecting affected MCP servers.

Acceptance:

- Not logged in: plugin MCP server is not launched and UI gets a clear auth failure.
- Logged in: plugin MCP server process receives the two env vars.
- Expired token with valid refresh: refresh succeeds before launch.
- Token string never appears in logs, diagnostics, registry, or API responses.

### Phase 6: Hash and Rollback Hardening

Files:

- `crates/allthecodes-plugins/src/marketplace.rs`
- `crates/allthecodes-plugins/src/installation.rs`
- `crates/allthecodes-plugins/src/zip_cache.rs`

Tasks:

- Accept `sha256` from marketplace entries.
- Reject downloads whose SHA-256 does not match.
- For update: start new MCP server after registry switch; if launch fails, roll registry back to old version and reconnect old server.
- Add stale-version garbage collection policy separately.

Acceptance:

- Hash mismatch fails and preserves the old install.
- New-version launch failure rolls back to the previous version.

## Test Plan

### Unit Tests

Add or extend:

- `crates/allthecodes-plugins/src/zip_cache.rs`
  - rejects `../` and absolute paths;
  - rejects Windows prefixes;
  - rejects symlink entries;
  - enforces file count and decompressed size limits.
- `crates/allthecodes-plugins/src/manifest.rs`
  - rejects absolute `mcp_servers[].command`;
  - rejects `skills[].path` outside root;
  - accepts root and single-top-level `plugin.json` layouts.
- `crates/allthecodes-plugins/src/installation.rs`
  - official id/version mismatch fails;
  - failed install preserves previous registry;
  - valid official zip writes versioned root and registry entry.
- `crates/allthecodes-plugins/src/marketplace.rs`
  - parses official marketplace JSON;
  - rejects non-official download hosts.

### Web Handler Tests

Extend `crates/allthecodes-web/src/handlers/plugins_tests.rs`:

- `GET /api/plugins/marketplace` returns mocked official entries.
- `POST /api/plugins/install` accepts official install body.
- Legacy `{ source, scope }` install still works.
- `enable`, `disable`, `uninstall`, `update`, and `restart` return typed errors for missing plugin IDs.

### Integration / Manual

From repository root:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-plugins
cargo test -p allthecodes-web plugins
cargo build --workspace --release
```

Manual end-to-end after frontend is wired:

1. Start backend on `127.0.0.1:17322`.
2. Start frontend on `127.0.0.1:17321`.
3. Log in through account auth.
4. Install `eco-boost`.
5. Install `allthecodes-bridge-cli`.
6. Confirm both appear in `GET /api/plugins/installed`.
7. Restart client and confirm installed state persists.
8. Disable one plugin and confirm its MCP server is no longer discovered.
9. Re-enable and restart it.

## Open Decisions

- Whether official marketplace replaces the legacy built-in `superpowers` entry or both appear with different source labels.
- Whether to expose `sha256` or normalize to existing `checksum`.
- Exact max zip and expanded size limits.
- How many old versions to retain.
- Whether plugin MCP env injection belongs in `allthecodes-web` runtime glue or `allthecodes-mcp` launch code.
- Whether official marketplace should be cached on disk for offline startup.

## First Implementation Slice

Recommended first PR:

1. Add protocol structs and route aliases.
2. Fetch/mock official marketplace entries.
3. Add official install request deserialization but return `501 not_implemented` for strict install until Phase 3.
4. Update frontend against the stable request/response shape.

Recommended second PR:

1. Implement strict zip download/extract/manifest validation.
2. Install the two current official zips into versioned roots.
3. Add registry persistence tests.

Recommended third PR:

1. Add account token injection.
2. Add MCP restart/update rollback behavior.
3. Add redaction tests.
