# LSP Service Implementation Design

**Date:** 2026-04-10
**Scope:** Core complete — 9 operations, 6 language servers, no passive diagnostics
**Approach:** Hand-written JSON-RPC transport + `lsp-types` crate for protocol types

## 1. Problem

The LSP service layer (`src/lsp_service/mod.rs`) has 9 operations that all return `bail!("not yet implemented")`. The tool layer (`src/tools/lsp.rs`) and process lifecycle manager (`src/services/lsp_lifecycle.rs`) are fully implemented, but there is no JSON-RPC communication layer to actually talk to LSP server subprocesses.

## 2. Architecture

```
LspTool::call()                            (existing, unchanged)
    ↓
execute_lsp_operation()                    (existing, unchanged)
    ↓
lsp_service::{operation}()                 (stub → wired to LspClient)
    ↓
┌─────────────────────────────────────┐
│  LspClient                          │
│  ├─ ensure_server(language_id)      │    lazy start + initialize
│  ├─ request(method, params)         │    JSON-RPC request/response
│  ├─ notify(method, params)          │    JSON-RPC one-way notification
│  └─ ensure_file_open(uri)           │    didOpen file tracking
└───────────────┬─────────────────────┘
                ↓
┌─────────────────────────────────────┐
│  JsonRpcTransport                    │
│  ├─ send(json) → stdin              │    Content-Length encoding
│  ├─ recv() → json ← stdout          │    Content-Length decoding
│  └─ 30s timeout per operation        │
└───────────────┬─────────────────────┘
                ↓
          LSP server subprocess            (rust-analyzer, pylsp, etc.)
```

### Files changed

| File | Action | Est. lines |
|------|--------|-----------|
| `src/lsp_service/transport.rs` | **New** — JSON-RPC over stdio | ~200 |
| `src/lsp_service/client.rs` | **New** — LSP client lifecycle + requests | ~500 |
| `src/lsp_service/conversions.rs` | **New** — lsp-types → SourceLocation/SymbolInfo/HoverInfo | ~200 |
| `src/lsp_service/mod.rs` | **Modify** — wire 9 stubs to LspClient | ~200 changed |
| `Cargo.toml` | **Modify** — add `lsp-types` dependency | 1 line |
| `tests/e2e_lsp.rs` | **New** — integration tests (skip if no server) | ~150 |

### Files unchanged

- `src/tools/lsp.rs` — tool layer stays as-is
- `src/services/lsp_lifecycle.rs` — retained but not used directly (LspClient manages its own Child process because it needs to hold stdin/stdout handles)

## 3. JsonRpcTransport

Handles the LSP wire protocol: `Content-Length` header framing over stdin/stdout.

```rust
pub struct JsonRpcTransport {
    writer: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
}
```

### Methods

**`send(&mut self, message: &Value) -> Result<()>`**
1. Serialize `message` to JSON bytes
2. Write `Content-Length: {len}\r\n\r\n`
3. Write JSON bytes
4. Flush

**`recv(&mut self, timeout: Duration) -> Result<Value>`**
1. Read lines until `Content-Length: {n}` header found
2. Read blank line separator
3. Read exactly `n` bytes
4. Parse as JSON
5. Wrap in `tokio::time::timeout`

### Wire format (LSP standard)

```
Content-Length: 52\r\n
\r\n
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

### Key decisions

- Use `tokio::process::ChildStdin/ChildStdout` for async I/O (not `std::process`)
- Use `tokio::io::BufReader` for line-by-line header parsing
- Default timeout: 30s per recv (configurable per-call)
- No `Content-Type` header (optional per LSP spec, omit for simplicity)

## 4. LspClient

Manages one LSP server: startup, initialization handshake, request routing, file synchronization.

```rust
pub struct LspClient {
    transport: JsonRpcTransport,
    child: tokio::process::Child,
    next_id: u64,
    root_uri: String,
    language_id: String,
    initialized: bool,
    open_files: HashSet<String>,
    server_capabilities: Option<ServerCapabilities>,
}
```

### Lifecycle

**`start(config: &LspServerConfig, root_path: &Path) -> Result<Self>`**
1. Spawn subprocess with `tokio::process::Command` (stdin/stdout/stderr piped)
2. Create `JsonRpcTransport` from stdin/stdout
3. Send `initialize` request with:
   - `rootUri`: workspace root as `file://` URI
   - `capabilities`: client capabilities (minimal — just declare support for the 9 operations)
4. Wait for `InitializeResult`, store `server_capabilities`
5. Send `initialized` notification
6. Return ready client

**`request<P: Serialize>(&mut self, method: &str, params: P) -> Result<Value>`**
1. Assign `self.next_id`, increment
2. Build JSON-RPC request: `{"jsonrpc":"2.0","id":N,"method":"...","params":{...}}`
3. `transport.send(request)`
4. Loop `transport.recv()` until response with matching `id` (skip notifications from server)
5. If response has `error` field, return `Err` with code + message
6. Return `result` field

**`notify<P: Serialize>(&mut self, method: &str, params: P) -> Result<()>`**
1. Build JSON-RPC notification: `{"jsonrpc":"2.0","method":"...","params":{...}}` (no `id`)
2. `transport.send(notification)`

**`ensure_file_open(&mut self, uri: &str) -> Result<()>`**
1. If `uri` already in `self.open_files`, return Ok
2. Convert URI to file path, read file contents
3. Detect `language_id` from file extension
4. Send `textDocument/didOpen` with `TextDocumentItem { uri, languageId, version: 1, text }`
5. Add to `self.open_files`

**`shutdown(mut self) -> Result<()>`**
1. Send `shutdown` request, wait for response
2. Send `exit` notification
3. Wait for child process to exit (with 5s timeout, then kill)

### Server capability checks

Before sending a request, check `server_capabilities` to confirm the server supports the operation. If not supported, return a clear error message (e.g., "rust-analyzer does not support callHierarchy").

## 5. Global Server Management

```rust
use parking_lot::Mutex;
use std::sync::LazyLock;

static LSP_CLIENTS: LazyLock<tokio::sync::Mutex<HashMap<String, LspClient>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));
```

Use `tokio::sync::Mutex` (not `parking_lot`) because `LspClient` methods are async and the lock must be held across `.await` points.

**`get_or_start_client(uri: &str) -> Result<&mut LspClient>`**
1. Determine language from file extension via `config_for_file`
2. If client exists and is alive, return it
3. If client exists but process died, remove it
4. Start new client with `LspClient::start(config, root_path)`
5. Insert and return

**Root path detection**: Use the current working directory (`std::env::current_dir`) as workspace root. This matches how Claude Code operates (always from a project root).

## 6. Operation Implementations

All 9 operations follow the same pattern:

```rust
pub async fn go_to_definition(uri: &str, line: u32, character: u32) -> Result<Vec<SourceLocation>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let client = get_or_start_client(uri, &mut clients).await?;
    client.ensure_file_open(uri).await?;

    let params = lsp_types::GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.parse()? },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let response = client.request("textDocument/definition", params).await?;
    conversions::parse_definition_response(response)
}
```

### Response parsing (conversions.rs)

| LSP response | Operations | Parse to |
|-------------|-----------|---------|
| `Location \| Location[] \| LocationLink[]` | definition, implementation, references | `Vec<SourceLocation>` |
| `Hover` (MarkupContent or MarkedString) | hover | `HoverInfo` |
| `DocumentSymbol[] \| SymbolInformation[]` | documentSymbol | `Vec<SymbolInfo>` |
| `SymbolInformation[]` | workspaceSymbol | `Vec<SymbolInfo>` |
| `CallHierarchyItem[]` | prepareCallHierarchy | `Vec<SymbolInfo>` |
| `CallHierarchyIncomingCall[]` | incomingCalls | `Vec<SymbolInfo>` |
| `CallHierarchyOutgoingCall[]` | outgoingCalls | `Vec<SymbolInfo>` |

**URI → file path conversion**: Strip `file://` prefix, URL-decode, normalize path separators for the platform.

**SymbolKind mapping**: Convert `lsp_types::SymbolKind` numeric values to human-readable strings ("function", "struct", "method", etc.).

**Hover content extraction**: Handle both `MarkupContent` (preferred) and legacy `MarkedString` formats. Extract the string value, preserving markdown formatting.

**DocumentSymbol hierarchy**: `DocumentSymbol` has a `children` field — map recursively to `SymbolInfo.children`. `SymbolInformation` (flat) has no children.

## 7. Error Handling

| Scenario | Handling |
|----------|---------|
| Server command not found (e.g., rust-analyzer not installed) | `bail!("LSP server '{}' not found in PATH — install it first", cmd)` |
| Server crashes after start | Mark dead, auto-restart on next call |
| Initialize timeout (30s) | Kill process, `bail!("LSP server failed to initialize within 30s")` |
| Request timeout (30s) | Return error, keep process alive (may be indexing) |
| JSON-RPC error response | Convert to `anyhow::Error` with code + message |
| Unsupported operation (server capabilities) | `bail!("{} does not support {}", server, operation)` |
| Malformed response | `bail!("Unexpected LSP response format")` |
| File read error (for didOpen) | Propagate as-is |

## 8. Supported Language Servers

Reuse existing configs from `default_server_configs()`:

| Language | Server | Args |
|----------|--------|------|
| Rust | `rust-analyzer` | (none) |
| TypeScript/JS | `typescript-language-server` | `--stdio` |
| Python | `pylsp` | (none) |
| Go | `gopls` | (none) |
| C/C++ | `clangd` | (none) |
| Java | `jdtls` | (none) |

All servers use stdio transport (stdin/stdout). No TCP/socket support needed.

## 9. Testing

### Unit tests (in each module)

**transport.rs:**
- `Content-Length` header encoding/decoding
- JSON body serialization roundtrip
- Malformed header handling (missing Content-Length, bad number)
- Empty body handling

**client.rs:**
- `initialize` request construction (verify JSON structure)
- `didOpen` notification construction
- Request ID auto-increment
- Server capability checking logic

**conversions.rs:**
- `Location` → `SourceLocation` conversion
- `LocationLink` → `SourceLocation` conversion
- `Hover` with `MarkupContent` → `HoverInfo`
- `Hover` with `MarkedString` → `HoverInfo`
- `DocumentSymbol` (nested) → `Vec<SymbolInfo>` (recursive)
- `SymbolInformation` (flat) → `Vec<SymbolInfo>`
- `CallHierarchyItem` → `SymbolInfo`
- `SymbolKind` → string mapping
- URI → file path conversion (Unix + Windows)

### Integration tests (tests/e2e_lsp.rs)

Guard all tests with:
```rust
fn has_rust_analyzer() -> bool {
    which::which("rust-analyzer").is_ok()
}
```

Tests (skip if server not available):
- Start rust-analyzer, send initialize, verify capabilities
- goToDefinition on a known Rust file
- hover on a known symbol
- documentSymbol listing
- Shutdown + cleanup

## 10. Non-goals (explicit exclusions)

- **Passive diagnostics** (`textDocument/publishDiagnostics`): Not in scope. Server notifications are read from the transport but silently discarded.
- **didChange / incremental sync**: Files are opened read-only. No editing-while-LSP-running support.
- **Plugin integration**: LSP server configs come from hardcoded defaults only, not from plugin manifests.
- **Multiple workspace roots**: One root per client (CWD).
- **TCP/socket transport**: Stdio only.
- **Server restart UI**: Auto-restart is silent. No user-facing notification.
