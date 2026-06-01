# Web Chat UI for cc-rust — Design Spec

**Date:** 2026-04-16  
**Approach:** A — Axum embedded server + independent React SPA  
**Status:** Approved

---

## 1. Overview

Add a browser-based chat interface to cc-rust as an alternative frontend alongside the existing TUI. The interface serves dual purposes: developer debugging (inspect SSE events, raw JSON, API timelines) and a production-quality chat experience that can evolve over time.

### Key Decisions

- **Backend:** Axum HTTP server embedded in the cc-rust binary, sharing the same tokio runtime and `Arc<QueryEngine>`
- **Frontend:** React 18 + TypeScript + Tailwind CSS + shadcn/ui + Lucide icons, built with Vite
- **Communication:** SSE as primary channel (`POST /api/chat`), WebSocket as optional upgrade (`/ws`, Phase 5)
- **Distribution:** Single binary with rust-embed; `cargo build --release` embeds the frontend dist

---

## 2. Architecture

### Startup Modes

```
cc-rust              → TUI mode (existing)
cc-rust --print      → Print mode (existing)
cc-rust --web        → Web mode (new)
cc-rust --web --port 3000 --no-open
```

`--web` mode: completes existing Phase A init (settings → auth → tools → AppState), starts Axum server, optionally opens the browser.

### Process Model

```
cc-rust --web
  │
  ├─ Axum Server (tokio main task)
  │    ├─ GET  /              → static files (React SPA)
  │    ├─ POST /api/chat      → SSE streaming response
  │    ├─ POST /api/abort     → abort current generation
  │    ├─ GET  /api/state     → full state snapshot
  │    ├─ POST /api/settings  → modify runtime settings
  │    ├─ POST /api/command   → execute slash command
  │    ├─ POST /api/permission_response → permission decision
  │    └─ GET  /ws            → WebSocket upgrade (Phase 5)
  │
  ├─ QueryEngine (shared Arc)
  │    └─ submit_message() → Stream<SdkMessage>
  │
  └─ Terminal log output (tracing)
```

Single process, single tokio runtime. Axum and QueryEngine share the runtime with zero cross-process overhead.

### New Module Layout

```
src/
├── web/                    ← NEW
│   ├── mod.rs              ← Axum router assembly, server startup
│   ├── handlers.rs         ← Route handlers (chat, abort, settings, command, permission)
│   ├── sse.rs              ← SdkMessage → SSE event conversion
│   ├── ws.rs               ← WebSocket handler (Phase 5)
│   ├── state.rs            ← Web-layer shared state (active connections, session ref)
│   └── static_files.rs     ← Embedded static file serving
├── main.rs                 ← Add --web branch
└── ...
```

### Relationship to Existing Code

- **No modifications** to QueryEngine, ApiClient, tools/, commands/
- `src/web/` is purely additive, references engine via `Arc<QueryEngine>`
- `main.rs` gains one new mode branch for `--web`

---

## 3. API Protocol

### `POST /api/chat` — SSE Streaming (Primary Channel)

**Request:**
```json
{
  "message": "help me write a sort function",
  "session_id": "optional-uuid",
  "attachments": []
}
```

**Response:** `Content-Type: text/event-stream`

Each SSE event maps 1:1 to an `SdkMessage` variant:

```
event: stream_event
data: {"type":"text_delta","content":"Sure"}

event: assistant
data: {"content":[{"type":"text","text":"..."},{"type":"tool_use","id":"tu_1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":320,"output_tokens":87},"cost_usd":0.002}

event: tool_result
data: {"tool_use_id":"tu_1","content":"file1.rs\nfile2.rs","is_error":false}

event: tool_use_summary
data: {"summary":"FileWrite → sort.rs","preceding_tool_use_ids":["tu_1"]}

event: usage_update
data: {"turn":2,"input_tokens":1520,"output_tokens":340,"cache_read":800,"cost_usd":0.0023,"cumulative":{"total_input":3200,"total_output":780,"total_cost_usd":0.005}}

event: permission_request
data: {"request_id":"pr_001","tool":"Bash","input":{"command":"rm -rf /tmp/old"},"risk_level":"high","reason":"Destructive command detected","options":["allow_once","allow_always","deny"]}

event: agent_spawn
data: {"agent_id":"ag_abc","description":"Explore codebase","subagent_type":"Explore","model":"claude-sonnet-4-20250514","parent_tool_use_id":"tu_5"}

event: agent_progress
data: {"agent_id":"ag_abc","status":"running","turns":3}

event: agent_complete
data: {"agent_id":"ag_abc","success":true,"result_preview":"Found 15 files...","duration_ms":8200,"usage":{"input":5000,"output":1200}}

event: api_retry
data: {"attempt":2,"max_retries":5,"retry_delay_ms":3000,"error_status":429,"error":"Rate limit exceeded"}

event: result
data: {"subtype":"Success","is_error":false,"duration_ms":4500,"num_turns":2,"total_cost_usd":0.005,"usage":{...},"permission_denials":[]}
```

Stream terminates with exactly one `event: result`.

### `POST /api/abort`

```json
{ "session_id": "uuid" }
```

Sets QueryEngine's `abort_flag` (existing `AtomicBool`).

### `GET /api/state` — Full State Snapshot

```json
{
  "model": "claude-sonnet-4-20250514",
  "permissions": {
    "mode": "default",
    "always_allow": { "project": ["Bash(npm *)","FileRead"] },
    "always_deny": { "project": ["Bash(rm -rf *)"] },
    "always_ask": {}
  },
  "thinking_enabled": true,
  "fast_mode": false,
  "effort": "high",
  "tools": ["Bash","FileRead","FileWrite","FileEdit","Glob","Grep","Agent",...],
  "session_id": "abc-123",
  "commands": [
    {"name":"help","aliases":["h","?"],"description":"Show available commands"},
    ...
  ]
}
```

### `POST /api/settings` — Modify Runtime Settings

```json
{ "action": "set_model", "value": "claude-opus-4-6-20250414" }
{ "action": "set_permission_mode", "value": "auto" }
{ "action": "add_allow_rule", "tool": "Bash(git *)", "source": "web" }
{ "action": "remove_deny_rule", "tool": "Bash(rm -rf *)", "source": "project" }
{ "action": "set_thinking", "value": true }
{ "action": "set_fast_mode", "value": false }
{ "action": "set_effort", "value": "low" }
```

### `POST /api/command` — Execute Slash Command

```json
{ "command": "compact", "args": "" }
```

Response:
```json
{
  "type": "output",
  "content": "Compacted conversation...",
  "messages": []
}
```

When `type` is `"query"`, the messages are auto-injected into the engine and a new SSE stream begins.

### `POST /api/permission_response`

```json
{ "request_id": "pr_001", "decision": "allow_once" }
```

Resolves the oneshot channel in the engine; SSE stream resumes.

---

## 4. SdkMessage Data Availability

### Already Available

| Data | Source | Fields |
|------|--------|--------|
| Tool calls | `AssistantMessage.content[ToolUse]` | id, name, input (full JSON) |
| Tool results | `ContentBlock::ToolResult` | tool_use_id, content, is_error |
| Per-call usage | `AssistantMessage.usage` | input_tokens, output_tokens, cache_read, cache_creation |
| Per-call cost | `AssistantMessage.cost_usd` | USD |
| Cumulative usage | `SdkResult.usage` (UsageTracking) | total_input/output/cache tokens, total_cost_usd, api_call_count |
| Thinking | `ContentBlock::Thinking` | thinking text, signature |
| Tool summary | `SdkToolUseSummary` | summary text, tool_use_ids |
| API retry | `SdkApiRetry` | attempt, max_retries, delay, error |
| Result | `SdkResult` | subtype, duration, turns, cost, errors, permission_denials |

### Requires Extension

| Data | Change Needed |
|------|---------------|
| Subagent lifecycle | Add `agent_spawn`, `agent_progress`, `agent_complete` SSE events. Add optional progress callback channel in `src/tools/agent.rs`. |
| Per-turn usage push | Add `usage_update` SSE event, emitted after each assistant message. |
| Permission request | Add `permission_request` SSE event + oneshot response channel in web handler. |
| `parent_tool_use_id` | Currently hardcoded to `None`; populate when running inside an Agent tool call. |

---

## 5. Frontend Architecture

### Tech Stack

| Library | Purpose |
|---------|---------|
| React 18 + TypeScript | UI framework |
| Tailwind CSS | Utility-first styling |
| shadcn/ui | Component library (copy-paste, fully customizable) |
| Lucide React | Icons |
| zustand | State management |
| react-markdown + remark-gfm | Markdown rendering |
| shiki | Code syntax highlighting (VS Code engine) |
| @tanstack/react-virtual | Virtual scrolling for long conversations |

### Project Structure

```
web-ui/
├── package.json
├── vite.config.ts           ← proxy /api → localhost:3001
├── tsconfig.json
├── index.html
├── src/
│   ├── main.tsx
│   ├── App.tsx              ← Layout shell
│   ├── lib/
│   │   ├── api.ts           ← SSE/WS client, fetch wrappers
│   │   ├── types.ts         ← SdkMessage mirror types
│   │   └── store.ts         ← zustand store
│   ├── components/
│   │   ├── chat/
│   │   │   ├── ChatPanel.tsx
│   │   │   ├── MessageList.tsx
│   │   │   ├── MessageBubble.tsx
│   │   │   ├── UserMessage.tsx
│   │   │   ├── AssistantMessage.tsx
│   │   │   ├── StreamingText.tsx
│   │   │   ├── InputBar.tsx
│   │   │   └── ThinkingBlock.tsx
│   │   ├── tools/
│   │   │   ├── ToolCallCard.tsx
│   │   │   ├── BashOutput.tsx
│   │   │   ├── FileChangeCard.tsx
│   │   │   └── AgentCard.tsx
│   │   ├── sidebar/
│   │   │   ├── Sidebar.tsx
│   │   │   ├── ModelSelector.tsx
│   │   │   ├── PermissionPanel.tsx
│   │   │   ├── CommandPalette.tsx
│   │   │   └── UsagePanel.tsx
│   │   ├── dialogs/
│   │   │   └── PermissionDialog.tsx
│   │   └── debug/
│   │       ├── DebugPanel.tsx
│   │       ├── RawEventLog.tsx
│   │       ├── MessageInspector.tsx
│   │       └── ApiCallTimeline.tsx
│   └── hooks/
│       ├── useChat.ts
│       ├── useSettings.ts
│       └── useCommands.ts
```

### Zustand Store Shape

```typescript
interface ChatStore {
  messages: Message[]
  isStreaming: boolean
  streamingContent: string
  sessionId: string | null
  model: string
  permissionMode: 'default' | 'auto' | 'bypass'
  permissionRules: PermissionRules
  thinkingEnabled: boolean
  fastMode: boolean
  effort: string
  tools: string[]
  commands: CommandInfo[]
  usage: UsageTracking
  pendingPermission: PermissionRequest | null
  rawEvents: SseEvent[]
  debugPanelOpen: boolean
  // Actions
  sendMessage: (text: string) => Promise<void>
  abort: () => void
  executeCommand: (name: string, args: string) => Promise<void>
  updateSetting: (action: string, value: any) => Promise<void>
  respondPermission: (requestId: string, decision: string) => void
}
```

### Component Rendering Dispatch

```
SdkMessage type           → Component
──────────────────────────────────────
assistant (text block)    → AssistantMessage (react-markdown + shiki)
assistant (thinking)      → ThinkingBlock (collapsible)
assistant (tool_use)      → ToolCallCard (expandable)
tool_result               → Embedded in ToolCallCard
stream_event              → StreamingText (appends to current AssistantMessage)
agent_spawn/complete      → AgentCard (with progress indicator)
permission_request        → PermissionDialog (modal, blocks stream)
api_retry                 → Inline notification bar
usage_update              → UsagePanel sidebar update
result                    → End-of-turn marker, restore input
```

### Tool-Specific Rendering

- `Bash` → terminal-style output (BashOutput)
- `FileWrite` / `FileEdit` → diff view (FileChangeCard)
- `Agent` → lifecycle card (AgentCard)
- All others → generic JSON viewer

---

## 6. Build Pipeline

### Development Mode

Two terminals:

```bash
# Terminal 1: Rust backend
cargo watch -x 'run -- --web --port 3001'

# Terminal 2: Frontend HMR
cd web-ui && npm run dev   # Vite on :5173, proxies /api → :3001
```

### Production Build

```bash
cargo build --release
# build.rs auto-runs: npm run build → web-ui/dist/
# rust-embed embeds dist/ into binary
```

### build.rs Logic

1. Check if `web-ui/dist/` exists and is fresh
2. If missing or `FORCE_WEB_BUILD` set → run `npm run build`
3. If npm not available → warn and skip (TUI still works)
4. `rust-embed` embeds `web-ui/dist/` at compile time

### Static File Serving

```rust
#[derive(Embed)]
#[folder = "web-ui/dist"]
struct WebAssets;
```

Axum handler serves from embedded assets with SPA fallback (all non-file paths → `index.html`).

### No-Node Graceful Degradation

If Node/npm not available at build time, the binary compiles without web assets. `cc-rust --web` prints an error message with instructions. TUI mode is unaffected.

### New Cargo Dependencies

```toml
axum = { version = "0.8", features = ["ws"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }
rust-embed = "8"
mime_guess = "2"
```

5 new crates, all mainstream tokio ecosystem. No conflicts with existing deps.

### Vite Config

```typescript
// web-ui/vite.config.ts
export default defineConfig({
  plugins: [react()],
  resolve: { alias: { '@': path.resolve(__dirname, './src') } },
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:3001',
      '/ws': { target: 'ws://127.0.0.1:3001', ws: true },
    },
  },
  build: { outDir: 'dist', emptyOutDir: true },
})
```

---

## 7. Error Handling

### Error Layers

| Layer | Example | Handling |
|-------|---------|----------|
| L1 Connection | SSE disconnect, WS drop | Frontend auto-reconnect with exponential backoff |
| L2 API | LLM rate limit, auth expired | Engine internal retry + `api_retry` SSE event |
| L3 Tool | Bash timeout, file not found | `ToolResult.is_error`, rendered in ToolCallCard |
| L4 Business | Permission denied, budget exhausted | `SdkResult` with error subtype |

### SSE Reconnection Strategy

| Scenario | Detection | Recovery |
|----------|-----------|----------|
| Backend restart (cargo watch) | fetch failure | Poll `/api/state` until available, toast notification |
| Network flap | SSE read timeout | Exponential backoff: 1s → 2s → 4s, cap 30s |
| Mid-stream disconnect | No `event: result` received | Mark message as interrupted with ⚠️ |
| Browser tab sleep | `visibilitychange` event | Call `/api/state` to sync on wake |

### Concurrency Protection

One active chat request per session. Second request receives `409 engine_busy`. Frontend disables send button during streaming; backend provides defense-in-depth.

### Unified Error Response Format

```json
{
  "error": "description",
  "code": "auth_expired | engine_busy | invalid_request | internal_error",
  "details": {}
}
```

---

## 8. Phased Delivery

### Phase 1: Skeleton (Minimum Viable)

**Goal:** Browser sends a message, sees streaming reply.

**Backend:** `src/web/{mod,handlers,sse,state}.rs`, `main.rs --web` flag, Cargo deps.  
**Frontend:** Vite scaffold, InputBar, MessageList, StreamingText, useChat hook, types.ts.  
**Done when:** Type "hello" → see streaming response → `result` event → input restored.

### Phase 2: Tool Calls + Permissions

**Goal:** Tool execution visible, permission flow works.

**Backend:** `tool_result` event, `POST /api/abort`, permission request/response mechanism.  
**Frontend:** ToolCallCard, BashOutput, FileChangeCard, PermissionDialog, ThinkingBlock, abort button.  
**Done when:** "List current directory" → Bash card + result → permission dialog interaction works.

### Phase 3: Settings + Commands

**Goal:** Full control panel in sidebar.

**Backend:** `GET /api/state`, `POST /api/settings`, `POST /api/command`, `usage_update` event.  
**Frontend:** Sidebar, ModelSelector, PermissionPanel, CommandPalette, UsagePanel, zustand full integration.  
**Done when:** Switch model, change permission mode, execute `/compact`, see real-time usage.

### Phase 4: Subagent + Debug Panel

**Goal:** Agent lifecycle visible, raw debugging available.

**Backend:** agent.rs progress callback, `agent_spawn/progress/complete` events.  
**Frontend:** AgentCard, DebugPanel (RawEventLog, MessageInspector, ApiCallTimeline), Ctrl+Shift+D toggle.  
**Done when:** Complex task → Agent card with progress → Debug panel shows all raw events + timeline.

### Phase 5: Production Polish

**Goal:** Single-binary distribution, robust UX.

**Backend:** build.rs auto-build, rust-embed, `--no-open`, graceful shutdown, no-Node degradation.  
**Frontend:** Virtual scrolling, code highlighting, theme toggle, responsive layout, reconnection.  
**Optional:** WebSocket channel, multi-session support.  
**Done when:** `cargo build --release` → single binary → `cc-rust --web` → full experience → long conversations stay smooth.

### Dependencies

```
Phase 1 (skeleton)
    │
    ├──→ Phase 2 (tools + permissions)
    │        │
    │        ├──→ Phase 3 (settings + commands)  ← independent
    │        │
    │        └──→ Phase 4 (subagent + debug)     ← independent
    │
    └──────────→ Phase 5 (production) ← after 2,3,4 complete
```

Phases 2, 3, 4 are independent of each other and can be interleaved.
