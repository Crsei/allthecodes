# IPC Subsystem Extensions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the Headless IPC protocol to expose LSP/MCP/Plugin/Skills subsystem state, events, diagnostics, and lifecycle management to the frontend and Agent.

**Architecture:** Add per-subsystem event/command enums to `BackendMessage`/`FrontendMessage`, a broadcast-based `SubsystemEventBus` for push events, command handlers for lifecycle operations, and a `SystemStatus` tool for Agent observability. Each subsystem emits events through a shared bus; `headless.rs` subscribes and forwards to the frontend.

**Tech Stack:** Rust, serde, tokio::sync::broadcast, async-trait

**Spec:** [`../specs/2026-04-15-ipc-subsystem-extensions-design.md`](../specs/2026-04-15-ipc-subsystem-extensions-design.md)

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/ipc/subsystem_types.rs` | Shared data types (LspDiagnostic, McpToolInfo, PluginInfo, SkillInfo, etc.) |
| Create | `src/ipc/subsystem_events.rs` | SubsystemEvent enum, SubsystemEventBus, per-subsystem event enums |
| Create | `src/ipc/subsystem_handlers.rs` | FrontendMessage command handlers + status snapshot builder |
| Create | `src/tools/system_status.rs` | SystemStatus tool for Agent observability |
| Modify | `src/ipc/mod.rs` | Declare new submodules |
| Modify | `src/ipc/protocol.rs` | Add BackendMessage/FrontendMessage variants |
| Modify | `src/ipc/headless.rs` | Wire event bus subscription + command dispatch |
| Modify | `src/lsp_service/mod.rs` | Add event sender + emit state changes |
| Modify | `src/lsp_service/client.rs` | Capture diagnostics from notifications |
| Modify | `src/mcp/mod.rs` | Add event sender static |
| Modify | `src/mcp/client.rs` | Emit connect/disconnect/discovery events |
| Modify | `src/mcp/manager.rs` | Emit events during connect_all |
| Modify | `src/plugins/mod.rs` | Emit register/unregister events |
| Modify | `src/skills/mod.rs` | Emit load/register events |
| Modify | `src/tools/mod.rs` | Declare system_status module |
| Modify | `src/tools/registry.rs` | Register SystemStatusTool |
| Modify | `src/engine/system_prompt.rs` | Inject subsystem status reminder |

---

## Task 1: Shared Data Types

**Files:**
- Create: `src/ipc/subsystem_types.rs`

- [ ] **Step 1: Write tests for serde round-trip**

```rust
// src/ipc/subsystem_types.rs — append at bottom

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_diagnostic_serializes_correctly() {
        let diag = LspDiagnostic {
            range: DiagnosticRange {
                start_line: 42,
                start_character: 5,
                end_line: 42,
                end_character: 15,
            },
            severity: "error".to_string(),
            message: "cannot find value `foo`".to_string(),
            source: Some("rust-analyzer".to_string()),
            code: Some("E0425".to_string()),
        };
        let json = serde_json::to_value(&diag).unwrap();
        assert_eq!(json["severity"], "error");
        assert_eq!(json["range"]["start_line"], 42);
        assert!(json.get("source").is_some());
    }

    #[test]
    fn lsp_server_info_omits_none_error() {
        let info = LspServerInfo {
            language_id: "rust".to_string(),
            state: "running".to_string(),
            extensions: vec!["rs".to_string()],
            open_files_count: 3,
            error: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("error").is_none());
    }

    #[test]
    fn mcp_server_status_info_serializes() {
        let info = McpServerStatusInfo {
            name: "context7".to_string(),
            state: "connected".to_string(),
            transport: "stdio".to_string(),
            tools_count: 2,
            resources_count: 0,
            server_info: Some(McpServerInfoBrief {
                name: "context7".to_string(),
                version: "1.0.0".to_string(),
            }),
            instructions: None,
            error: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["tools_count"], 2);
        assert!(json.get("instructions").is_none());
    }

    #[test]
    fn plugin_info_serializes() {
        let info = PluginInfo {
            id: "claude-mem@official".to_string(),
            name: "claude-mem".to_string(),
            version: "1.2.0".to_string(),
            status: "installed".to_string(),
            contributed_tools: vec![],
            contributed_skills: vec!["mem-search".to_string()],
            contributed_mcp_servers: vec![],
            error: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["contributed_skills"][0], "mem-search");
    }

    #[test]
    fn skill_info_serializes() {
        let info = SkillInfo {
            name: "simplify".to_string(),
            source: "bundled".to_string(),
            description: "Review changed code".to_string(),
            user_invocable: true,
            model_invocable: true,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["source"], "bundled");
        assert_eq!(json["user_invocable"], true);
    }

    #[test]
    fn subsystem_status_snapshot_serializes() {
        let snapshot = SubsystemStatusSnapshot {
            lsp: vec![],
            mcp: vec![],
            plugins: vec![],
            skills: vec![],
            timestamp: 1744675200,
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["timestamp"], 1744675200);
        assert!(json["lsp"].as_array().unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ipc::subsystem_types -- --nocapture 2>&1 | head -20`
Expected: Compilation error — module doesn't exist yet

- [ ] **Step 3: Write the data types**

```rust
// src/ipc/subsystem_types.rs

//! Shared data types for subsystem IPC messages.
//!
//! These types are used by both `BackendMessage` events and the `SystemStatus` tool.
//! They are serializable for JSON transport and designed for cross-mode reuse
//! (headless + future daemon mode).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// LSP types
// ---------------------------------------------------------------------------

/// A single LSP diagnostic entry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LspDiagnostic {
    pub range: DiagnosticRange,
    /// "error" | "warning" | "info" | "hint"
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// 1-based line/character range for a diagnostic.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiagnosticRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// Summary info for one LSP server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LspServerInfo {
    pub language_id: String,
    /// "not_started"|"starting"|"running"|"stopped"|"error"
    pub state: String,
    pub extensions: Vec<String>,
    pub open_files_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// MCP types
// ---------------------------------------------------------------------------

/// Brief info about a single MCP tool.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Brief info about a single MCP resource.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpResourceInfo {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Summary info for one MCP server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpServerStatusInfo {
    pub name: String,
    /// "pending"|"connected"|"disconnected"|"error"
    pub state: String,
    /// "stdio"|"sse"
    pub transport: String,
    pub tools_count: usize,
    pub resources_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<McpServerInfoBrief>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Name + version for an MCP server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpServerInfoBrief {
    pub name: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// Plugin types
// ---------------------------------------------------------------------------

/// Summary info for one plugin.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    /// "not_installed"|"installed"|"disabled"|"error"
    pub status: String,
    pub contributed_tools: Vec<String>,
    pub contributed_skills: Vec<String>,
    pub contributed_mcp_servers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Skill types
// ---------------------------------------------------------------------------

/// Summary info for one skill.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    /// "bundled"|"user"|"project"|"plugin"|"mcp"
    pub source: String,
    pub description: String,
    pub user_invocable: bool,
    pub model_invocable: bool,
}

// ---------------------------------------------------------------------------
// Aggregated snapshot
// ---------------------------------------------------------------------------

/// Full status snapshot across all subsystems.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubsystemStatusSnapshot {
    pub lsp: Vec<LspServerInfo>,
    pub mcp: Vec<McpServerStatusInfo>,
    pub plugins: Vec<PluginInfo>,
    pub skills: Vec<SkillInfo>,
    pub timestamp: i64,
}
```

- [ ] **Step 4: Add module to `src/ipc/mod.rs`**

Add `pub mod subsystem_types;` to `src/ipc/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib ipc::subsystem_types -- --nocapture`
Expected: All 6 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/ipc/subsystem_types.rs src/ipc/mod.rs
git commit -m "feat(ipc): add shared subsystem data types for IPC extensions"
```

---

## Task 2: Subsystem Event Enums + Event Bus

**Files:**
- Create: `src/ipc/subsystem_events.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Write tests for event enum serialization and event bus**

```rust
// src/ipc/subsystem_events.rs — append at bottom

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::subsystem_types::*;

    #[test]
    fn lsp_event_server_state_changed_serializes() {
        let event = LspEvent::ServerStateChanged {
            language_id: "rust".to_string(),
            state: "running".to_string(),
            error: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "server_state_changed");
        assert_eq!(json["language_id"], "rust");
    }

    #[test]
    fn lsp_event_diagnostics_published_serializes() {
        let event = LspEvent::DiagnosticsPublished {
            uri: "file:///src/main.rs".to_string(),
            diagnostics: vec![LspDiagnostic {
                range: DiagnosticRange {
                    start_line: 1,
                    start_character: 1,
                    end_line: 1,
                    end_character: 10,
                },
                severity: "error".to_string(),
                message: "test".to_string(),
                source: None,
                code: None,
            }],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "diagnostics_published");
        assert_eq!(json["diagnostics"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn mcp_event_tools_discovered_serializes() {
        let event = McpEvent::ToolsDiscovered {
            server_name: "ctx7".to_string(),
            tools: vec![McpToolInfo {
                name: "query".to_string(),
                description: Some("Query docs".to_string()),
            }],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "tools_discovered");
    }

    #[test]
    fn plugin_event_status_changed_serializes() {
        let event = PluginEvent::StatusChanged {
            plugin_id: "p1".to_string(),
            name: "Plugin One".to_string(),
            status: "installed".to_string(),
            error: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "status_changed");
    }

    #[test]
    fn skill_event_skills_loaded_serializes() {
        let event = SkillEvent::SkillsLoaded { count: 5 };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "skills_loaded");
        assert_eq!(json["count"], 5);
    }

    #[test]
    fn lsp_command_deserializes() {
        let json = r#"{"kind":"start_server","language_id":"rust"}"#;
        let cmd: LspCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, LspCommand::StartServer { .. }));
    }

    #[test]
    fn mcp_command_deserializes() {
        let json = r#"{"kind":"query_status"}"#;
        let cmd: McpCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, McpCommand::QueryStatus));
    }

    #[test]
    fn skill_command_reload_deserializes() {
        let json = r#"{"kind":"reload"}"#;
        let cmd: SkillCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, SkillCommand::Reload));
    }

    #[test]
    fn event_bus_send_receive() {
        let bus = SubsystemEventBus::new();
        let mut rx = bus.subscribe();
        let tx = bus.sender();

        tx.send(SubsystemEvent::Skill(SkillEvent::SkillsLoaded { count: 3 }))
            .unwrap();

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            SubsystemEvent::Skill(SkillEvent::SkillsLoaded { count: 3 })
        ));
    }

    #[test]
    fn event_bus_no_subscriber_does_not_panic() {
        let bus = SubsystemEventBus::new();
        let tx = bus.sender();
        // No subscriber — send should not panic (returns Err, which we ignore)
        let _ = tx.send(SubsystemEvent::Skill(SkillEvent::SkillsLoaded { count: 0 }));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ipc::subsystem_events -- --nocapture 2>&1 | head -20`
Expected: Compilation error — module doesn't exist yet

- [ ] **Step 3: Write event enums, command enums, and event bus**

```rust
// src/ipc/subsystem_events.rs

//! Subsystem event and command enums + broadcast event bus.
//!
//! Each subsystem (LSP, MCP, Plugin, Skill) has:
//! - An event enum (Serialize) for backend → frontend push
//! - A command enum (Deserialize) for frontend → backend requests
//!
//! The [`SubsystemEventBus`] distributes events from any subsystem to
//! subscribers (headless event loop, future daemon SSE).

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use super::subsystem_types::*;

// ---------------------------------------------------------------------------
// Events (Backend → Frontend)
// ---------------------------------------------------------------------------

/// LSP subsystem events.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspEvent {
    ServerStateChanged {
        language_id: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    DiagnosticsPublished {
        uri: String,
        diagnostics: Vec<LspDiagnostic>,
    },
    ServerList {
        servers: Vec<LspServerInfo>,
    },
}

/// MCP subsystem events.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpEvent {
    ServerStateChanged {
        server_name: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ToolsDiscovered {
        server_name: String,
        tools: Vec<McpToolInfo>,
    },
    ResourcesDiscovered {
        server_name: String,
        resources: Vec<McpResourceInfo>,
    },
    ChannelNotification {
        server_name: String,
        content: String,
        meta: serde_json::Value,
    },
    ServerList {
        servers: Vec<McpServerStatusInfo>,
    },
}

/// Plugin subsystem events.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginEvent {
    StatusChanged {
        plugin_id: String,
        name: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    PluginList {
        plugins: Vec<PluginInfo>,
    },
}

/// Skill subsystem events.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillEvent {
    SkillsLoaded {
        count: usize,
    },
    SkillList {
        skills: Vec<SkillInfo>,
    },
}

/// Unified subsystem event (sent through the event bus).
#[derive(Debug, Clone)]
pub enum SubsystemEvent {
    Lsp(LspEvent),
    Mcp(McpEvent),
    Plugin(PluginEvent),
    Skill(SkillEvent),
}

// ---------------------------------------------------------------------------
// Commands (Frontend → Backend)
// ---------------------------------------------------------------------------

/// LSP lifecycle commands.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspCommand {
    StartServer { language_id: String },
    StopServer { language_id: String },
    RestartServer { language_id: String },
    QueryStatus,
}

/// MCP lifecycle commands.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpCommand {
    ConnectServer { server_name: String },
    DisconnectServer { server_name: String },
    ReconnectServer { server_name: String },
    QueryStatus,
}

/// Plugin lifecycle commands.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginCommand {
    Enable { plugin_id: String },
    Disable { plugin_id: String },
    QueryStatus,
}

/// Skill management commands.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillCommand {
    Reload,
    QueryStatus,
}

// ---------------------------------------------------------------------------
// Event Bus
// ---------------------------------------------------------------------------

/// Broadcast-based event bus for subsystem events.
///
/// Multiple subscribers can receive the same events. Slow subscribers
/// that fall behind the buffer (256 events) will miss old events — they
/// can recover via QueryStatus commands.
pub struct SubsystemEventBus {
    tx: broadcast::Sender<SubsystemEvent>,
}

impl SubsystemEventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    /// Get a clone of the sender for injecting into subsystems.
    pub fn sender(&self) -> broadcast::Sender<SubsystemEvent> {
        self.tx.clone()
    }

    /// Subscribe to receive events.
    pub fn subscribe(&self) -> broadcast::Receiver<SubsystemEvent> {
        self.tx.subscribe()
    }
}
```

- [ ] **Step 4: Add module to `src/ipc/mod.rs`**

Add `pub mod subsystem_events;` to `src/ipc/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib ipc::subsystem_events -- --nocapture`
Expected: All 10 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/ipc/subsystem_events.rs src/ipc/mod.rs
git commit -m "feat(ipc): add subsystem event/command enums and event bus"
```

---

## Task 3: Extend Protocol — BackendMessage + FrontendMessage

**Files:**
- Modify: `src/ipc/protocol.rs`

- [ ] **Step 1: Write test for new BackendMessage variants**

Add to `src/ipc/protocol.rs` `mod tests`:

```rust
#[test]
fn backend_lsp_event_serializes() {
    use crate::ipc::subsystem_events::LspEvent;
    let msg = BackendMessage::LspEvent {
        event: LspEvent::ServerStateChanged {
            language_id: "rust".to_string(),
            state: "running".to_string(),
            error: None,
        },
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "lsp_event");
    assert_eq!(json["event"]["kind"], "server_state_changed");
}

#[test]
fn backend_subsystem_status_serializes() {
    use crate::ipc::subsystem_types::SubsystemStatusSnapshot;
    let msg = BackendMessage::SubsystemStatus {
        status: SubsystemStatusSnapshot {
            lsp: vec![],
            mcp: vec![],
            plugins: vec![],
            skills: vec![],
            timestamp: 100,
        },
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "subsystem_status");
    assert_eq!(json["status"]["timestamp"], 100);
}

#[test]
fn frontend_lsp_command_deserializes() {
    let json = r#"{"type":"lsp_command","command":{"kind":"start_server","language_id":"rust"}}"#;
    let msg: FrontendMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, FrontendMessage::LspCommand { .. }));
}

#[test]
fn frontend_query_subsystem_status_deserializes() {
    let json = r#"{"type":"query_subsystem_status"}"#;
    let msg: FrontendMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, FrontendMessage::QuerySubsystemStatus));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ipc::protocol -- --nocapture 2>&1 | head -20`
Expected: Compilation error — `LspEvent` variant not found on `BackendMessage`

- [ ] **Step 3: Add new variants to BackendMessage**

In `src/ipc/protocol.rs`, add to the `BackendMessage` enum (before the closing `}`):

```rust
/// LSP subsystem event.
LspEvent {
    event: super::subsystem_events::LspEvent,
},

/// MCP subsystem event.
McpEvent {
    event: super::subsystem_events::McpEvent,
},

/// Plugin subsystem event.
PluginEvent {
    event: super::subsystem_events::PluginEvent,
},

/// Skill subsystem event.
SkillEvent {
    event: super::subsystem_events::SkillEvent,
},

/// Aggregated subsystem status snapshot.
SubsystemStatus {
    status: super::subsystem_types::SubsystemStatusSnapshot,
},
```

- [ ] **Step 4: Add new variants to FrontendMessage**

In `src/ipc/protocol.rs`, add to the `FrontendMessage` enum:

```rust
/// LSP lifecycle command.
LspCommand {
    command: super::subsystem_events::LspCommand,
},

/// MCP lifecycle command.
McpCommand {
    command: super::subsystem_events::McpCommand,
},

/// Plugin lifecycle command.
PluginCommand {
    command: super::subsystem_events::PluginCommand,
},

/// Skill management command.
SkillCommand {
    command: super::subsystem_events::SkillCommand,
},

/// Query all subsystem statuses.
QuerySubsystemStatus,
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib ipc::protocol -- --nocapture`
Expected: All tests PASS (existing + 4 new)

- [ ] **Step 6: Commit**

```bash
git add src/ipc/protocol.rs
git commit -m "feat(ipc): add subsystem event/command variants to BackendMessage/FrontendMessage"
```

---

## Task 4: Subsystem Command Handlers + Status Snapshot Builder

**Files:**
- Create: `src/ipc/subsystem_handlers.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Write tests for status snapshot builder**

```rust
// src/ipc/subsystem_handlers.rs — append at bottom

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_lsp_server_info_list_returns_configured_servers() {
        let infos = build_lsp_server_info_list();
        // Default configs include at least rust, typescript, python, go, c, java
        assert!(infos.len() >= 6);
        let rust = infos.iter().find(|i| i.language_id == "rust");
        assert!(rust.is_some());
        // No server is started, so state should be "not_started"
        assert_eq!(rust.unwrap().state, "not_started");
    }

    #[test]
    fn build_skill_info_list_returns_skills() {
        // Register a test skill, then check the listing
        use crate::skills;
        skills::clear_skills();
        skills::register_skill(crate::skills::SkillDefinition {
            name: "test-skill".to_string(),
            source: crate::skills::SkillSource::Bundled,
            base_dir: None,
            frontmatter: crate::skills::SkillFrontmatter {
                description: "A test".to_string(),
                user_invocable: true,
                ..Default::default()
            },
            prompt_body: String::new(),
        });

        let infos = build_skill_info_list();
        assert!(!infos.is_empty());
        let test = infos.iter().find(|s| s.name == "test-skill");
        assert!(test.is_some());
        assert_eq!(test.unwrap().source, "bundled");
        assert!(test.unwrap().user_invocable);

        skills::clear_skills();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ipc::subsystem_handlers -- --nocapture 2>&1 | head -20`
Expected: Compilation error — module doesn't exist

- [ ] **Step 3: Write command handlers and snapshot builders**

```rust
// src/ipc/subsystem_handlers.rs

//! Command handlers for subsystem lifecycle operations and status queries.
//!
//! Each handler receives a deserialized command enum and performs the
//! corresponding action. Status queries respond by sending BackendMessages
//! directly to the frontend.

use tracing::{debug, warn};

use crate::ipc::protocol::{send_to_frontend, BackendMessage};
use crate::ipc::subsystem_events::*;
use crate::ipc::subsystem_types::*;

// ---------------------------------------------------------------------------
// LSP command handler
// ---------------------------------------------------------------------------

pub async fn handle_lsp_command(cmd: LspCommand) {
    match cmd {
        LspCommand::StartServer { language_id } => {
            debug!(language_id = %language_id, "IPC: LSP start_server requested");
            // Construct a fake URI to trigger get_or_start_client.
            // Find an extension for this language to build the URI.
            let configs = crate::lsp_service::default_server_configs();
            let ext = configs
                .iter()
                .find(|c| c.language_id == language_id)
                .and_then(|c| c.extensions.first())
                .cloned();
            if let Some(ext) = ext {
                let cwd = std::env::current_dir().unwrap_or_default();
                let dummy_path = cwd.join(format!("__probe__.{}", ext));
                let uri = format!("file:///{}", dummy_path.to_string_lossy().replace('\\', "/"));
                let mut clients = crate::lsp_service::LSP_CLIENTS.lock().await;
                match crate::lsp_service::get_or_start_client_pub(&uri, &mut clients).await {
                    Ok(_) => debug!("IPC: LSP server started for {}", language_id),
                    Err(e) => {
                        warn!("IPC: failed to start LSP server {}: {}", language_id, e);
                        let _ = send_to_frontend(&BackendMessage::LspEvent {
                            event: LspEvent::ServerStateChanged {
                                language_id,
                                state: "error".to_string(),
                                error: Some(e.to_string()),
                            },
                        });
                    }
                }
            } else {
                warn!("IPC: unknown language_id for LSP: {}", language_id);
            }
        }
        LspCommand::StopServer { language_id } => {
            debug!(language_id = %language_id, "IPC: LSP stop_server requested");
            let mut clients = crate::lsp_service::LSP_CLIENTS.lock().await;
            if let Some(mut client) = clients.remove(&language_id) {
                client.shutdown().await;
            }
        }
        LspCommand::RestartServer { language_id } => {
            debug!(language_id = %language_id, "IPC: LSP restart_server requested");
            // Stop then start
            {
                let mut clients = crate::lsp_service::LSP_CLIENTS.lock().await;
                if let Some(mut client) = clients.remove(&language_id) {
                    client.shutdown().await;
                }
            }
            // Re-trigger start via recursive call
            handle_lsp_command(LspCommand::StartServer { language_id }).await;
        }
        LspCommand::QueryStatus => {
            let servers = build_lsp_server_info_list();
            let _ = send_to_frontend(&BackendMessage::LspEvent {
                event: LspEvent::ServerList { servers },
            });
        }
    }
}

// ---------------------------------------------------------------------------
// MCP command handler
// ---------------------------------------------------------------------------

pub async fn handle_mcp_command(cmd: McpCommand) {
    match cmd {
        McpCommand::ConnectServer { server_name } => {
            debug!(server = %server_name, "IPC: MCP connect requested");
            // MCP connect requires access to the McpManager which is held by the engine.
            // For now, send a system info message advising to use /mcp command.
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!("Use `/mcp connect {}` to connect MCP servers.", server_name),
                level: "info".to_string(),
            });
        }
        McpCommand::DisconnectServer { server_name } => {
            debug!(server = %server_name, "IPC: MCP disconnect requested");
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!(
                    "Use `/mcp disconnect {}` to disconnect MCP servers.",
                    server_name
                ),
                level: "info".to_string(),
            });
        }
        McpCommand::ReconnectServer { server_name } => {
            debug!(server = %server_name, "IPC: MCP reconnect requested");
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!(
                    "Use `/mcp reconnect {}` to reconnect MCP servers.",
                    server_name
                ),
                level: "info".to_string(),
            });
        }
        McpCommand::QueryStatus => {
            let servers = build_mcp_server_info_list();
            let _ = send_to_frontend(&BackendMessage::McpEvent {
                event: McpEvent::ServerList { servers },
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin command handler
// ---------------------------------------------------------------------------

pub async fn handle_plugin_command(cmd: PluginCommand) {
    match cmd {
        PluginCommand::Enable { plugin_id } => {
            debug!(plugin = %plugin_id, "IPC: plugin enable requested");
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!("Use `/plugin enable {}` to enable plugins.", plugin_id),
                level: "info".to_string(),
            });
        }
        PluginCommand::Disable { plugin_id } => {
            debug!(plugin = %plugin_id, "IPC: plugin disable requested");
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!("Use `/plugin disable {}` to disable plugins.", plugin_id),
                level: "info".to_string(),
            });
        }
        PluginCommand::QueryStatus => {
            let plugins = build_plugin_info_list();
            let _ = send_to_frontend(&BackendMessage::PluginEvent {
                event: PluginEvent::PluginList { plugins },
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Skill command handler
// ---------------------------------------------------------------------------

pub async fn handle_skill_command(cmd: SkillCommand) {
    match cmd {
        SkillCommand::Reload => {
            debug!("IPC: skill reload requested");
            crate::skills::clear_skills();
            let cwd = std::env::current_dir().ok();
            crate::skills::init_skills(cwd.as_deref());
            let count = crate::skills::get_all_skills().len();
            let _ = send_to_frontend(&BackendMessage::SkillEvent {
                event: SkillEvent::SkillsLoaded { count },
            });
        }
        SkillCommand::QueryStatus => {
            let skills = build_skill_info_list();
            let _ = send_to_frontend(&BackendMessage::SkillEvent {
                event: SkillEvent::SkillList { skills },
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Status snapshot builders
// ---------------------------------------------------------------------------

/// Build LSP server info list from default configs + running state.
pub fn build_lsp_server_info_list() -> Vec<LspServerInfo> {
    let configs = crate::lsp_service::default_server_configs();
    // We can't await the tokio Mutex here in a sync fn, so report config-level info only.
    // For running state, callers should use the async version or check via event bus.
    configs
        .into_iter()
        .map(|c| LspServerInfo {
            language_id: c.language_id,
            state: "not_started".to_string(), // default; updated by event bus
            extensions: c.extensions,
            open_files_count: 0,
            error: None,
        })
        .collect()
}

/// Build MCP server info from discovery (config-level, not live state).
pub fn build_mcp_server_info_list() -> Vec<McpServerStatusInfo> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let configs = crate::mcp::discovery::discover_mcp_servers(&cwd).unwrap_or_default();
    configs
        .into_iter()
        .map(|c| {
            let transport = c.transport.clone();
            McpServerStatusInfo {
                name: c.name.clone(),
                state: "pending".to_string(),
                transport,
                tools_count: 0,
                resources_count: 0,
                server_info: None,
                instructions: None,
                error: None,
            }
        })
        .collect()
}

/// Build plugin info from the plugin registry.
pub fn build_plugin_info_list() -> Vec<PluginInfo> {
    crate::plugins::get_all_plugins()
        .into_iter()
        .map(|p| {
            let status = match &p.status {
                crate::plugins::PluginStatus::NotInstalled => "not_installed",
                crate::plugins::PluginStatus::Installed => "installed",
                crate::plugins::PluginStatus::Disabled => "disabled",
                crate::plugins::PluginStatus::Error(_) => "error",
            };
            let error = match &p.status {
                crate::plugins::PluginStatus::Error(e) => Some(e.clone()),
                _ => None,
            };
            PluginInfo {
                id: p.id,
                name: p.name,
                version: p.version,
                status: status.to_string(),
                contributed_tools: p.tools,
                contributed_skills: p.skills,
                contributed_mcp_servers: p.mcp_servers,
                error,
            }
        })
        .collect()
}

/// Build skill info from the skill registry.
pub fn build_skill_info_list() -> Vec<SkillInfo> {
    crate::skills::get_all_skills()
        .into_iter()
        .map(|s| {
            let source = match &s.source {
                crate::skills::SkillSource::Bundled => "bundled",
                crate::skills::SkillSource::User => "user",
                crate::skills::SkillSource::Project => "project",
                crate::skills::SkillSource::Plugin(_) => "plugin",
                crate::skills::SkillSource::Mcp(_) => "mcp",
            };
            SkillInfo {
                name: s.display_name().to_string(),
                source: source.to_string(),
                description: s.frontmatter.description.clone(),
                user_invocable: s.is_user_invocable(),
                model_invocable: s.is_model_invocable(),
            }
        })
        .collect()
}

/// Build a full aggregated snapshot across all subsystems.
pub fn build_subsystem_status_snapshot() -> SubsystemStatusSnapshot {
    SubsystemStatusSnapshot {
        lsp: build_lsp_server_info_list(),
        mcp: build_mcp_server_info_list(),
        plugins: build_plugin_info_list(),
        skills: build_skill_info_list(),
        timestamp: chrono::Utc::now().timestamp(),
    }
}
```

- [ ] **Step 4: Make `get_or_start_client` publicly accessible**

In `src/lsp_service/mod.rs`, add a public wrapper (after the existing `get_or_start_client`):

```rust
/// Public wrapper for IPC command handlers.
pub async fn get_or_start_client_pub(
    uri: &str,
    clients: &mut HashMap<String, client::LspClient>,
) -> Result<String> {
    get_or_start_client(uri, clients).await
}
```

Also make `LSP_CLIENTS` public:

```rust
pub static LSP_CLIENTS: LazyLock<tokio::sync::Mutex<HashMap<String, client::LspClient>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));
```

- [ ] **Step 5: Add module to `src/ipc/mod.rs`**

Add `pub mod subsystem_handlers;` to `src/ipc/mod.rs`.

- [ ] **Step 6: Run tests**

Run: `cargo test --lib ipc::subsystem_handlers -- --nocapture`
Expected: All tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/ipc/subsystem_handlers.rs src/ipc/mod.rs src/lsp_service/mod.rs
git commit -m "feat(ipc): add subsystem command handlers and status snapshot builders"
```

---

## Task 5: Wire Headless Event Loop

**Files:**
- Modify: `src/ipc/headless.rs`

- [ ] **Step 1: Add event bus imports and initialization**

At the top of `src/ipc/headless.rs`, add to the imports:

```rust
use super::subsystem_events::SubsystemEventBus;
use super::subsystem_handlers;
```

- [ ] **Step 2: Create event bus in `run_headless()` and inject senders**

In `run_headless()`, after the background agent channel setup (`let (bg_tx, mut bg_rx) = ...`) and before the Ready message, add:

```rust
// ── 1c. Subsystem event bus setup ────────────────────────────
let event_bus = SubsystemEventBus::new();
let mut event_rx = event_bus.subscribe();

// Inject senders into subsystems
crate::lsp_service::set_event_sender(event_bus.sender());
crate::mcp::set_event_sender(event_bus.sender());
crate::plugins::set_event_sender(event_bus.sender());
crate::skills::set_event_sender(event_bus.sender());
```

- [ ] **Step 3: Add select branch for subsystem events**

In the main `loop { tokio::select! { ... } }`, add a third branch after the background agent branch:

```rust
// ── Branch 3: Subsystem events ─────────────────────────
Ok(event) = event_rx.recv() => {
    let msg = match event {
        crate::ipc::subsystem_events::SubsystemEvent::Lsp(e) => {
            BackendMessage::LspEvent { event: e }
        }
        crate::ipc::subsystem_events::SubsystemEvent::Mcp(e) => {
            BackendMessage::McpEvent { event: e }
        }
        crate::ipc::subsystem_events::SubsystemEvent::Plugin(e) => {
            BackendMessage::PluginEvent { event: e }
        }
        crate::ipc::subsystem_events::SubsystemEvent::Skill(e) => {
            BackendMessage::SkillEvent { event: e }
        }
    };
    let _ = send_to_frontend(&msg);
}
```

- [ ] **Step 4: Add FrontendMessage command dispatch**

In the `match msg { ... }` block for `FrontendMessage`, add new arms before `FrontendMessage::Quit`:

```rust
FrontendMessage::LspCommand { command } => {
    debug!("headless: LSP command");
    subsystem_handlers::handle_lsp_command(command).await;
}
FrontendMessage::McpCommand { command } => {
    debug!("headless: MCP command");
    subsystem_handlers::handle_mcp_command(command).await;
}
FrontendMessage::PluginCommand { command } => {
    debug!("headless: Plugin command");
    subsystem_handlers::handle_plugin_command(command).await;
}
FrontendMessage::SkillCommand { command } => {
    debug!("headless: Skill command");
    subsystem_handlers::handle_skill_command(command).await;
}
FrontendMessage::QuerySubsystemStatus => {
    debug!("headless: subsystem status query");
    let status = subsystem_handlers::build_subsystem_status_snapshot();
    let _ = send_to_frontend(&BackendMessage::SubsystemStatus { status });
}
```

- [ ] **Step 5: Build and fix any compilation errors**

Run: `cargo build 2>&1 | head -40`
Expected: Compiles (the `set_event_sender` functions don't exist yet, but this step compiles the IPC side; the missing functions are added in Tasks 6-9)

Note: If `set_event_sender` doesn't exist yet, temporarily wrap the injection calls in comments. They will be uncommented in Tasks 6-9.

- [ ] **Step 6: Commit**

```bash
git add src/ipc/headless.rs
git commit -m "feat(ipc): wire subsystem event bus and command dispatch in headless loop"
```

---

## Task 6: LSP Event Emission

**Files:**
- Modify: `src/lsp_service/mod.rs`
- Modify: `src/lsp_service/client.rs`

- [ ] **Step 1: Add event sender static and emit helper to `lsp_service/mod.rs`**

Add after the `LSP_CLIENTS` static:

```rust
use parking_lot::Mutex as SyncMutex;

/// Event sender for subsystem events (injected by headless event loop).
static EVENT_TX: LazyLock<SyncMutex<Option<tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>>>> =
    LazyLock::new(|| SyncMutex::new(None));

/// Inject the event sender from the headless event loop.
pub fn set_event_sender(
    tx: tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>,
) {
    *EVENT_TX.lock() = Some(tx);
}

/// Emit a subsystem event (no-op if no sender is set).
pub(crate) fn emit_event(event: crate::ipc::subsystem_events::SubsystemEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);
    }
}
```

- [ ] **Step 2: Emit ServerStateChanged in `get_or_start_client`**

In `get_or_start_client()`, after a successful start (line ~172):

```rust
// After: clients.insert(lang.clone(), new_client);
emit_event(crate::ipc::subsystem_events::SubsystemEvent::Lsp(
    crate::ipc::subsystem_events::LspEvent::ServerStateChanged {
        language_id: lang.clone(),
        state: "running".to_string(),
        error: None,
    },
));
```

After the server death detection (line ~165-166):

```rust
// After: tracing::warn!(language = %lang, "LSP server died, will restart");
emit_event(crate::ipc::subsystem_events::SubsystemEvent::Lsp(
    crate::ipc::subsystem_events::LspEvent::ServerStateChanged {
        language_id: lang.clone(),
        state: "stopped".to_string(),
        error: Some("server process died".to_string()),
    },
));
```

- [ ] **Step 3: Capture diagnostics in `client.rs` request loop**

In `src/lsp_service/client.rs`, in the `request()` method, replace the notification skip block (lines ~170-179):

```rust
None => {
    let method_str = msg.get("method").and_then(|m| m.as_str()).unwrap_or("?");
    match method_str {
        "textDocument/publishDiagnostics" => {
            if let Some(params) = msg.get("params") {
                let event = parse_diagnostics_notification(params);
                crate::lsp_service::emit_event(
                    crate::ipc::subsystem_events::SubsystemEvent::Lsp(event),
                );
            }
        }
        _ => {
            debug!(method = method_str, "skipping server notification");
        }
    }
    continue;
}
```

- [ ] **Step 4: Add diagnostics parser in `client.rs`**

Add at the bottom of `client.rs` (before tests):

```rust
/// Parse a `textDocument/publishDiagnostics` notification into an LspEvent.
fn parse_diagnostics_notification(params: &serde_json::Value) -> crate::ipc::subsystem_events::LspEvent {
    use crate::ipc::subsystem_types::{DiagnosticRange, LspDiagnostic};

    let uri = params["uri"].as_str().unwrap_or_default().to_string();
    let diagnostics = params["diagnostics"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|val| {
                    let range = val.get("range")?;
                    Some(LspDiagnostic {
                        range: DiagnosticRange {
                            start_line: range["start"]["line"].as_u64()? as u32 + 1,
                            start_character: range["start"]["character"].as_u64()? as u32 + 1,
                            end_line: range["end"]["line"].as_u64()? as u32 + 1,
                            end_character: range["end"]["character"].as_u64()? as u32 + 1,
                        },
                        severity: match val["severity"].as_u64() {
                            Some(1) => "error",
                            Some(2) => "warning",
                            Some(3) => "info",
                            Some(4) => "hint",
                            _ => "unknown",
                        }
                        .to_string(),
                        message: val["message"].as_str()?.to_string(),
                        source: val["source"].as_str().map(|s| s.to_string()),
                        code: val.get("code").and_then(|c| {
                            c.as_str()
                                .map(|s| s.to_string())
                                .or_else(|| c.as_u64().map(|n| n.to_string()))
                        }),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    crate::ipc::subsystem_events::LspEvent::DiagnosticsPublished { uri, diagnostics }
}
```

- [ ] **Step 5: Add test for diagnostics parser**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diagnostics_notification_parses_valid_params() {
        let params = serde_json::json!({
            "uri": "file:///src/main.rs",
            "diagnostics": [{
                "range": {
                    "start": {"line": 10, "character": 4},
                    "end": {"line": 10, "character": 12}
                },
                "severity": 1,
                "message": "unused variable",
                "source": "rust-analyzer",
                "code": "E0599"
            }]
        });
        let event = parse_diagnostics_notification(&params);
        match event {
            crate::ipc::subsystem_events::LspEvent::DiagnosticsPublished { uri, diagnostics } => {
                assert_eq!(uri, "file:///src/main.rs");
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].severity, "error");
                assert_eq!(diagnostics[0].range.start_line, 11); // 0-based → 1-based
                assert_eq!(diagnostics[0].code.as_deref(), Some("E0599"));
            }
            _ => panic!("expected DiagnosticsPublished"),
        }
    }

    #[test]
    fn parse_diagnostics_notification_handles_empty() {
        let params = serde_json::json!({
            "uri": "file:///empty.rs",
            "diagnostics": []
        });
        let event = parse_diagnostics_notification(&params);
        match event {
            crate::ipc::subsystem_events::LspEvent::DiagnosticsPublished { diagnostics, .. } => {
                assert!(diagnostics.is_empty());
            }
            _ => panic!("expected DiagnosticsPublished"),
        }
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib lsp_service -- --nocapture`
Expected: All tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/lsp_service/mod.rs src/lsp_service/client.rs
git commit -m "feat(lsp): emit subsystem events for state changes and diagnostics"
```

---

## Task 7: MCP Event Emission

**Files:**
- Modify: `src/mcp/mod.rs`
- Modify: `src/mcp/client.rs`
- Modify: `src/mcp/manager.rs`

- [ ] **Step 1: Add event sender static to `mcp/mod.rs`**

Add after the existing constants:

```rust
use std::sync::LazyLock;
use parking_lot::Mutex as SyncMutex;

/// Event sender for subsystem events.
static EVENT_TX: LazyLock<SyncMutex<Option<tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>>>> =
    LazyLock::new(|| SyncMutex::new(None));

/// Inject the event sender from the headless event loop.
pub fn set_event_sender(
    tx: tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>,
) {
    *EVENT_TX.lock() = Some(tx);
}

/// Emit a subsystem event.
pub(crate) fn emit_event(event: crate::ipc::subsystem_events::SubsystemEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);
    }
}
```

- [ ] **Step 2: Emit events in `mcp/client.rs`**

In `connect()`, after setting `self.state = McpConnectionState::Connected`:

```rust
super::emit_event(crate::ipc::subsystem_events::SubsystemEvent::Mcp(
    crate::ipc::subsystem_events::McpEvent::ServerStateChanged {
        server_name: self.config.name.clone(),
        state: "connected".to_string(),
        error: None,
    },
));
```

In `connect()`, in the error path (if connect fails):

```rust
super::emit_event(crate::ipc::subsystem_events::SubsystemEvent::Mcp(
    crate::ipc::subsystem_events::McpEvent::ServerStateChanged {
        server_name: self.config.name.clone(),
        state: "error".to_string(),
        error: Some(e.to_string()),
    },
));
```

In `disconnect()`, after setting `self.state = McpConnectionState::Disconnected`:

```rust
super::emit_event(crate::ipc::subsystem_events::SubsystemEvent::Mcp(
    crate::ipc::subsystem_events::McpEvent::ServerStateChanged {
        server_name: self.config.name.clone(),
        state: "disconnected".to_string(),
        error: None,
    },
));
```

In `list_tools()`, after updating `self.tools`:

```rust
super::emit_event(crate::ipc::subsystem_events::SubsystemEvent::Mcp(
    crate::ipc::subsystem_events::McpEvent::ToolsDiscovered {
        server_name: self.config.name.clone(),
        tools: self
            .tools
            .iter()
            .map(|t| crate::ipc::subsystem_types::McpToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
            })
            .collect(),
    },
));
```

In `list_resources()`, after updating `self.resources`:

```rust
super::emit_event(crate::ipc::subsystem_events::SubsystemEvent::Mcp(
    crate::ipc::subsystem_events::McpEvent::ResourcesDiscovered {
        server_name: self.config.name.clone(),
        resources: self
            .resources
            .iter()
            .map(|r| crate::ipc::subsystem_types::McpResourceInfo {
                uri: r.uri.clone(),
                name: r.name.clone(),
                mime_type: r.mime_type.clone(),
            })
            .collect(),
    },
));
```

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/mcp/mod.rs src/mcp/client.rs
git commit -m "feat(mcp): emit subsystem events for connection state and discovery"
```

---

## Task 8: Plugin + Skill Event Emission

**Files:**
- Modify: `src/plugins/mod.rs`
- Modify: `src/skills/mod.rs`

- [ ] **Step 1: Add event sender to `plugins/mod.rs`**

Add the same static pattern:

```rust
use parking_lot::Mutex as SyncMutex;

static EVENT_TX: LazyLock<SyncMutex<Option<tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>>>> =
    LazyLock::new(|| SyncMutex::new(None));

pub fn set_event_sender(
    tx: tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>,
) {
    *EVENT_TX.lock() = Some(tx);
}

fn emit_event(event: crate::ipc::subsystem_events::SubsystemEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);
    }
}
```

- [ ] **Step 2: Emit in `register_plugin()` and `unregister_plugin()`**

In `register_plugin()`, after the HashMap insert:

```rust
let status_str = match &plugin.status {
    PluginStatus::NotInstalled => "not_installed",
    PluginStatus::Installed => "installed",
    PluginStatus::Disabled => "disabled",
    PluginStatus::Error(_) => "error",
};
emit_event(crate::ipc::subsystem_events::SubsystemEvent::Plugin(
    crate::ipc::subsystem_events::PluginEvent::StatusChanged {
        plugin_id: plugin.id.clone(),
        name: plugin.name.clone(),
        status: status_str.to_string(),
        error: match &plugin.status {
            PluginStatus::Error(e) => Some(e.clone()),
            _ => None,
        },
    },
));
```

In `unregister_plugin()`, after removing:

```rust
if let Some(ref removed) = result {
    emit_event(crate::ipc::subsystem_events::SubsystemEvent::Plugin(
        crate::ipc::subsystem_events::PluginEvent::StatusChanged {
            plugin_id: removed.id.clone(),
            name: removed.name.clone(),
            status: "not_installed".to_string(),
            error: None,
        },
    ));
}
```

- [ ] **Step 3: Add event sender to `skills/mod.rs`**

Same static pattern:

```rust
use parking_lot::Mutex as SyncMutex;

static EVENT_TX: LazyLock<SyncMutex<Option<tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>>>> =
    LazyLock::new(|| SyncMutex::new(None));

pub fn set_event_sender(
    tx: tokio::sync::broadcast::Sender<crate::ipc::subsystem_events::SubsystemEvent>,
) {
    *EVENT_TX.lock() = Some(tx);
}

fn emit_event(event: crate::ipc::subsystem_events::SubsystemEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);
    }
}
```

- [ ] **Step 4: Emit in `init_skills()`**

At the end of `init_skills()`, after all skills are registered:

```rust
let count = get_all_skills().len();
emit_event(crate::ipc::subsystem_events::SubsystemEvent::Skill(
    crate::ipc::subsystem_events::SkillEvent::SkillsLoaded { count },
));
```

- [ ] **Step 5: Build and verify**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully

- [ ] **Step 6: Commit**

```bash
git add src/plugins/mod.rs src/skills/mod.rs
git commit -m "feat(plugins,skills): emit subsystem events on state changes"
```

---

## Task 9: SystemStatus Tool

**Files:**
- Create: `src/tools/system_status.rs`
- Modify: `src/tools/mod.rs`
- Modify: `src/tools/registry.rs`

- [ ] **Step 1: Write test for SystemStatus tool**

```rust
// src/tools/system_status.rs — append at bottom

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_status_tool_name() {
        let tool = SystemStatusTool;
        assert_eq!(tool.name(), "SystemStatus");
    }

    #[test]
    fn system_status_tool_is_read_only() {
        let tool = SystemStatusTool;
        assert!(tool.is_read_only(&serde_json::json!({})));
    }

    #[test]
    fn system_status_tool_schema_has_subsystem_property() {
        let tool = SystemStatusTool;
        let schema = tool.input_json_schema();
        assert!(schema["properties"]["subsystem"].is_object());
    }

    #[test]
    fn format_status_output_all_returns_all_sections() {
        let output = format_status_output("all");
        assert!(output.contains("## LSP Servers"));
        assert!(output.contains("## MCP Servers"));
        assert!(output.contains("## Plugins"));
        assert!(output.contains("## Skills"));
    }

    #[test]
    fn format_status_output_lsp_only() {
        let output = format_status_output("lsp");
        assert!(output.contains("## LSP Servers"));
        assert!(!output.contains("## MCP Servers"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib tools::system_status -- --nocapture 2>&1 | head -20`
Expected: Compilation error

- [ ] **Step 3: Write the SystemStatus tool**

```rust
// src/tools/system_status.rs

//! SystemStatus tool — lets the Agent query subsystem status.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::ipc::subsystem_handlers;
use crate::types::message::AssistantMessage;
use crate::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};

pub struct SystemStatusTool;

#[async_trait]
impl Tool for SystemStatusTool {
    fn name(&self) -> &str {
        "SystemStatus"
    }

    async fn description(&self, _input: &Value) -> String {
        "Query the current status of subsystems (LSP, MCP, plugins, skills).".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subsystem": {
                    "type": "string",
                    "enum": ["lsp", "mcp", "plugins", "skills", "all"],
                    "description": "Which subsystem to query. Defaults to 'all'."
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let subsystem = input["subsystem"].as_str().unwrap_or("all");
        let output = format_status_output(subsystem);

        Ok(ToolResult {
            data: json!({ "status": output }),
            model_content: None,
            display_preview: Some(output.clone()),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Use SystemStatus to check the current status of LSP servers, MCP servers, plugins, and skills. \
         Query a specific subsystem with the `subsystem` parameter, or use \"all\" for a full overview."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "SystemStatus".to_string()
    }
}

/// Format a human-readable status output for the given subsystem.
pub fn format_status_output(subsystem: &str) -> String {
    let mut parts = Vec::new();

    if subsystem == "all" || subsystem == "lsp" {
        let servers = subsystem_handlers::build_lsp_server_info_list();
        let mut section = String::from("## LSP Servers\n");
        if servers.is_empty() {
            section.push_str("No LSP servers configured.\n");
        } else {
            for s in &servers {
                section.push_str(&format!("- {}: {} (extensions: {})\n",
                    s.language_id, s.state, s.extensions.join(", ")));
            }
        }
        parts.push(section);
    }

    if subsystem == "all" || subsystem == "mcp" {
        let servers = subsystem_handlers::build_mcp_server_info_list();
        let mut section = String::from("## MCP Servers\n");
        if servers.is_empty() {
            section.push_str("No MCP servers configured.\n");
        } else {
            for s in &servers {
                let mut line = format!("- {}: {} ({}, {} tools, {} resources)",
                    s.name, s.state, s.transport, s.tools_count, s.resources_count);
                if let Some(ref info) = s.server_info {
                    line.push_str(&format!(" [{}@{}]", info.name, info.version));
                }
                section.push_str(&format!("{}\n", line));
            }
        }
        parts.push(section);
    }

    if subsystem == "all" || subsystem == "plugins" {
        let plugins = subsystem_handlers::build_plugin_info_list();
        let mut section = String::from("## Plugins\n");
        if plugins.is_empty() {
            section.push_str("No plugins installed.\n");
        } else {
            for p in &plugins {
                let mut line = format!("- {}: {} (v{})", p.id, p.status, p.version);
                if !p.contributed_skills.is_empty() {
                    line.push_str(&format!("\n  Skills: {}", p.contributed_skills.join(", ")));
                }
                if !p.contributed_tools.is_empty() {
                    line.push_str(&format!("\n  Tools: {}", p.contributed_tools.join(", ")));
                }
                section.push_str(&format!("{}\n", line));
            }
        }
        parts.push(section);
    }

    if subsystem == "all" || subsystem == "skills" {
        let skills = subsystem_handlers::build_skill_info_list();
        let mut section = format!("## Skills ({} total)\n", skills.len());
        if skills.is_empty() {
            section.push_str("No skills loaded.\n");
        } else {
            for s in &skills {
                section.push_str(&format!("- {} [{}] — {}\n",
                    s.name, s.source, s.description));
            }
        }
        parts.push(section);
    }

    parts.join("\n")
}
```

- [ ] **Step 4: Register in `src/tools/mod.rs`**

Add: `pub mod system_status;`

- [ ] **Step 5: Register in `src/tools/registry.rs`**

Add import:
```rust
use super::system_status::SystemStatusTool;
```

Add to the `tools` vec:
```rust
Arc::new(SystemStatusTool),
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib tools::system_status -- --nocapture`
Expected: All 5 tests PASS

- [ ] **Step 7: Run full build**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully

- [ ] **Step 8: Commit**

```bash
git add src/tools/system_status.rs src/tools/mod.rs src/tools/registry.rs
git commit -m "feat(tools): add SystemStatus tool for Agent subsystem observability"
```

---

## Task 10: System Prompt Subsystem Status Injection

**Files:**
- Modify: `src/engine/system_prompt.rs`

- [ ] **Step 1: Add a new cached section for subsystem status**

In `build_system_prompt()`, add a new entry to the `dynamic_sections` vec, after the `external_channels` section:

```rust
cached_section("subsystem_status", || {
    build_subsystem_status_reminder()
}),
```

- [ ] **Step 2: Write the `build_subsystem_status_reminder()` function**

Add at the bottom of `system_prompt.rs` (before tests, if any):

```rust
/// Build a system-reminder with active subsystem counts.
///
/// Returns `None` when no subsystems are active (avoids cluttering the prompt).
fn build_subsystem_status_reminder() -> Option<String> {
    let lsp_configs = crate::lsp_service::default_server_configs().len();
    let mcp_count = crate::mcp::discovery::discover_mcp_servers(
        &std::env::current_dir().unwrap_or_default(),
    )
    .map(|v| v.len())
    .unwrap_or(0);
    let plugin_count = crate::plugins::get_enabled_plugins().len();
    let skill_count = crate::skills::get_all_skills().len();

    if mcp_count + plugin_count + skill_count == 0 {
        return None;
    }

    Some(format!(
        "# Active Subsystems\n\
         - LSP: {} language(s) configured\n\
         - MCP: {} server(s) configured\n\
         - Plugins: {} enabled\n\
         - Skills: {} loaded\n\
         Use the SystemStatus tool for detailed information.\n",
        lsp_configs, mcp_count, plugin_count, skill_count
    ))
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/engine/system_prompt.rs
git commit -m "feat(engine): inject subsystem status reminder into system prompt"
```

---

## Task 11: Resolve Warnings + Final Build

**Files:**
- Various (as needed)

- [ ] **Step 1: Full build with warnings**

Run: `cargo build 2>&1`
Expected: Check for warnings and fix any unused imports, dead code, etc.

- [ ] **Step 2: Run all tests**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: All tests pass

- [ ] **Step 3: Fix any warnings**

Address any `unused import`, `dead_code`, or other warnings introduced by the new code.

- [ ] **Step 4: Commit warning fixes**

```bash
git add -u
git commit -m "chore: resolve warnings from IPC subsystem extensions"
```

---

## Task 12: Uncomment Headless Event Bus Injection

**Files:**
- Modify: `src/ipc/headless.rs` (if event sender calls were commented out in Task 5)

- [ ] **Step 1: Uncomment the set_event_sender calls**

Ensure these lines in `run_headless()` are active:

```rust
crate::lsp_service::set_event_sender(event_bus.sender());
crate::mcp::set_event_sender(event_bus.sender());
crate::plugins::set_event_sender(event_bus.sender());
crate::skills::set_event_sender(event_bus.sender());
```

- [ ] **Step 2: Full build**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles with no errors

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

- [ ] **Step 4: Final commit**

```bash
git add -u
git commit -m "feat(ipc): complete subsystem extensions — event bus fully wired"
```
