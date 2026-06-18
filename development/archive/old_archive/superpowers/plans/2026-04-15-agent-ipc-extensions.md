# Agent IPC Extensions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the minimal `bg_rx` completion-only channel with a full-lifecycle agent event channel supporting spawn tracking, streaming, tree snapshots, and agent teams messaging via IPC.

**Architecture:** A new `AgentIpcEvent` enum wraps `AgentEvent` and `TeamEvent`. The existing `mpsc::unbounded_channel` for background agents is widened to carry all agent/team events. `AgentTreeManager` maintains a global tree of agents and pushes snapshots on state changes. Background agent streams are forwarded through the channel. Team mailbox events are bridged to the channel.

**Tech Stack:** Rust, serde, tokio::sync::mpsc, parking_lot

**Spec:** [`../specs/2026-04-15-agent-ipc-extensions-design.md`](../specs/2026-04-15-agent-ipc-extensions-design.md)

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/ipc/agent_types.rs` | AgentNode, AgentInfo, TeamMemberInfo |
| Create | `src/ipc/agent_events.rs` | AgentEvent, AgentCommand, TeamEvent, TeamCommand enums |
| Create | `src/ipc/agent_channel.rs` | AgentIpcEvent enum, type aliases (AgentSender/AgentReceiver) |
| Create | `src/ipc/agent_tree.rs` | AgentTreeManager (global tree state + snapshot) |
| Create | `src/ipc/agent_handlers.rs` | handle_agent_command, handle_team_command |
| Modify | `src/ipc/mod.rs` | Declare new modules |
| Modify | `src/ipc/protocol.rs` | Add AgentEvent/TeamEvent + AgentCommand/TeamCommand variants |
| Modify | `src/ipc/headless.rs` | Replace Branch 2 with full AgentIpcEvent handling |
| Modify | `src/tools/background_agents.rs` | Keep CompletedBackgroundAgent, add AgentSender re-export |
| Modify | `src/tools/agent/mod.rs` | Add sdk_to_agent_event helper |
| Modify | `src/tools/agent/tool_impl.rs` | Emit Spawned, stream forwarding, Completed via agent_tx |
| Modify | `src/tools/agent/dispatch.rs` | Emit Spawned/Completed for sync agents |
| Modify | `src/tools/system_status.rs` | Add "agents" and "teams" subsystem queries |
| Modify | `src/engine/lifecycle/mod.rs` | Change bg_agent_tx type |
| Modify | `src/engine/lifecycle/deps.rs` | Change bg_agent_tx type in QueryDeps |
| Modify | `src/engine/lifecycle/submit_message.rs` | Adapt bg_agent_tx usage |
| Modify | `src/engine/system_prompt.rs` | Add agents/teams count to reminder |
| Modify | `src/teams/mailbox.rs` | Bridge MessageRouted to agent channel |

---

## Task 1: Agent Shared Types

**Files:**
- Create: `src/ipc/agent_types.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Write tests**

```rust
// src/ipc/agent_types.rs — bottom

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_node_serializes_with_children() {
        let node = AgentNode {
            agent_id: "a1".into(),
            parent_agent_id: None,
            description: "root".into(),
            agent_type: Some("Explore".into()),
            model: Some("haiku".into()),
            state: "running".into(),
            is_background: true,
            depth: 1,
            chain_id: "c1".into(),
            spawned_at: 100,
            completed_at: None,
            duration_ms: None,
            result_preview: None,
            had_error: false,
            children: vec![AgentNode {
                agent_id: "a2".into(),
                parent_agent_id: Some("a1".into()),
                description: "child".into(),
                agent_type: None,
                model: None,
                state: "completed".into(),
                is_background: false,
                depth: 2,
                chain_id: "c1".into(),
                spawned_at: 101,
                completed_at: Some(105),
                duration_ms: Some(4000),
                result_preview: Some("done".into()),
                had_error: false,
                children: vec![],
            }],
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["children"].as_array().unwrap().len(), 1);
        assert!(json.get("parent_agent_id").is_none()); // skip_serializing_if
        assert_eq!(json["children"][0]["parent_agent_id"], "a1");
    }

    #[test]
    fn agent_info_serializes() {
        let info = AgentInfo {
            agent_id: "a1".into(),
            parent_agent_id: None,
            description: "task".into(),
            state: "running".into(),
            is_background: true,
            depth: 1,
            duration_ms: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["state"], "running");
    }

    #[test]
    fn team_member_info_serializes() {
        let info = TeamMemberInfo {
            agent_id: "a1".into(),
            agent_name: "worker-1".into(),
            role: Some("coder".into()),
            is_active: true,
            unread_messages: 3,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["unread_messages"], 3);
    }
}
```

- [ ] **Step 2: Write the types**

```rust
// src/ipc/agent_types.rs

//! Shared data types for agent and team IPC messages.

use serde::{Deserialize, Serialize};

/// Tree node representing an agent and its children.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentNode {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// "running"|"completed"|"error"|"aborted"
    pub state: String,
    pub is_background: bool,
    pub depth: usize,
    pub chain_id: String,
    pub spawned_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    pub had_error: bool,
    pub children: Vec<AgentNode>,
}

/// Flat agent info for SystemStatus tool output.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentInfo {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub description: String,
    /// "running"|"completed"|"error"|"aborted"
    pub state: String,
    pub is_background: bool,
    pub depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Team member info for team status queries.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TeamMemberInfo {
    pub agent_id: String,
    pub agent_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub is_active: bool,
    pub unread_messages: usize,
}
```

- [ ] **Step 3: Add `pub mod agent_types;` to `src/ipc/mod.rs`**

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test --bin claude-code-rs agent_types`

- [ ] **Step 5: Commit**

```bash
git add src/ipc/agent_types.rs src/ipc/mod.rs
git commit -m "feat(ipc): add agent and team shared data types"
```

---

## Task 2: Agent Event and Command Enums

**Files:**
- Create: `src/ipc/agent_events.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Write tests for serialization/deserialization**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::agent_types::*;

    #[test]
    fn agent_event_spawned_serializes() {
        let e = AgentEvent::Spawned {
            agent_id: "a1".into(),
            parent_agent_id: None,
            description: "test".into(),
            agent_type: Some("Explore".into()),
            model: Some("haiku".into()),
            is_background: true,
            depth: 1,
            chain_id: "c1".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "spawned");
        assert_eq!(json["is_background"], true);
    }

    #[test]
    fn agent_event_stream_delta_serializes() {
        let e = AgentEvent::StreamDelta { agent_id: "a1".into(), text: "hello".into() };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "stream_delta");
    }

    #[test]
    fn agent_event_tree_snapshot_serializes() {
        let e = AgentEvent::TreeSnapshot { roots: vec![] };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "tree_snapshot");
        assert!(json["roots"].as_array().unwrap().is_empty());
    }

    #[test]
    fn agent_command_abort_deserializes() {
        let json = r#"{"kind":"abort_agent","agent_id":"a1"}"#;
        let cmd: AgentCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, AgentCommand::AbortAgent { .. }));
    }

    #[test]
    fn agent_command_query_deserializes() {
        let json = r#"{"kind":"query_active_agents"}"#;
        let cmd: AgentCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, AgentCommand::QueryActiveAgents));
    }

    #[test]
    fn team_event_message_routed_serializes() {
        let e = TeamEvent::MessageRouted {
            team_name: "t1".into(),
            from: "lead".into(),
            to: "worker".into(),
            text: "do it".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            summary: None,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "message_routed");
    }

    #[test]
    fn team_command_inject_deserializes() {
        let json = r#"{"kind":"inject_message","team_name":"t1","to":"worker","text":"hello"}"#;
        let cmd: TeamCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, TeamCommand::InjectMessage { .. }));
    }
}
```

- [ ] **Step 2: Write all enums**

```rust
// src/ipc/agent_events.rs

//! Agent and Team event/command enums for IPC.

use serde::{Deserialize, Serialize};
use super::agent_types::*;

// ── Agent Events (Backend → Frontend) ──

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    Spawned {
        agent_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_agent_id: Option<String>,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        is_background: bool,
        depth: usize,
        chain_id: String,
    },
    Completed {
        agent_id: String,
        result_preview: String,
        had_error: bool,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
    },
    Error {
        agent_id: String,
        error: String,
        duration_ms: u64,
    },
    Aborted {
        agent_id: String,
    },
    StreamDelta {
        agent_id: String,
        text: String,
    },
    ThinkingDelta {
        agent_id: String,
        thinking: String,
    },
    ToolUse {
        agent_id: String,
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    ToolResult {
        agent_id: String,
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
    TreeSnapshot {
        roots: Vec<AgentNode>,
    },
}

// ── Agent Commands (Frontend → Backend) ──

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCommand {
    AbortAgent { agent_id: String },
    QueryActiveAgents,
    QueryAgentOutput { agent_id: String },
}

// ── Team Events (Backend → Frontend) ──

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamEvent {
    MemberJoined {
        team_name: String,
        agent_id: String,
        agent_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    MemberLeft {
        team_name: String,
        agent_id: String,
        agent_name: String,
    },
    MessageRouted {
        team_name: String,
        from: String,
        to: String,
        text: String,
        timestamp: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    StatusSnapshot {
        team_name: String,
        members: Vec<TeamMemberInfo>,
        pending_messages: usize,
    },
}

// ── Team Commands (Frontend → Backend) ──

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamCommand {
    InjectMessage {
        team_name: String,
        to: String,
        text: String,
    },
    QueryTeamStatus {
        team_name: String,
    },
}
```

- [ ] **Step 3: Add `pub mod agent_events;` to `src/ipc/mod.rs`**

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

```bash
git add src/ipc/agent_events.rs src/ipc/mod.rs
git commit -m "feat(ipc): add agent and team event/command enums"
```

---

## Task 3: Agent Channel Type

**Files:**
- Create: `src/ipc/agent_channel.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Write the channel types**

```rust
// src/ipc/agent_channel.rs

//! Agent IPC channel — the dedicated mpsc channel for agent + team events.
//!
//! Replaces the old `BgAgentSender` = `mpsc::UnboundedSender<CompletedBackgroundAgent>`.

use super::agent_events::{AgentEvent, TeamEvent};

/// All events that flow through the agent channel.
#[derive(Debug)]
pub enum AgentIpcEvent {
    Agent(AgentEvent),
    Team(TeamEvent),
}

/// Sender half — injected into agent tool and team modules.
pub type AgentSender = tokio::sync::mpsc::UnboundedSender<AgentIpcEvent>;

/// Receiver half — consumed by the headless event loop.
pub type AgentReceiver = tokio::sync::mpsc::UnboundedReceiver<AgentIpcEvent>;

/// Create a new agent channel pair.
pub fn agent_channel() -> (AgentSender, AgentReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}
```

- [ ] **Step 2: Write test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::agent_events::AgentEvent;

    #[test]
    fn agent_channel_send_receive() {
        let (tx, mut rx) = agent_channel();
        tx.send(AgentIpcEvent::Agent(AgentEvent::Aborted {
            agent_id: "a1".into(),
        })).unwrap();
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentIpcEvent::Agent(AgentEvent::Aborted { .. })));
    }
}
```

- [ ] **Step 3: Add module, run tests, commit**

```bash
git commit -m "feat(ipc): add agent channel types (AgentIpcEvent, AgentSender)"
```

---

## Task 4: Agent Tree Manager

**Files:**
- Create: `src/ipc/agent_tree.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, parent: Option<&str>, bg: bool) -> AgentNode {
        AgentNode {
            agent_id: id.into(),
            parent_agent_id: parent.map(|s| s.into()),
            description: format!("agent {}", id),
            agent_type: None,
            model: None,
            state: "running".into(),
            is_background: bg,
            depth: if parent.is_some() { 2 } else { 1 },
            chain_id: "c1".into(),
            spawned_at: 100,
            completed_at: None,
            duration_ms: None,
            result_preview: None,
            had_error: false,
            children: vec![],
        }
    }

    #[test]
    fn register_and_snapshot() {
        let mut mgr = AgentTreeManager::new();
        mgr.register(make_node("a1", None, true));
        mgr.register(make_node("a2", Some("a1"), false));
        let snap = mgr.build_snapshot();
        assert_eq!(snap.len(), 1); // one root
        assert_eq!(snap[0].agent_id, "a1");
        assert_eq!(snap[0].children.len(), 1);
        assert_eq!(snap[0].children[0].agent_id, "a2");
    }

    #[test]
    fn update_state() {
        let mut mgr = AgentTreeManager::new();
        mgr.register(make_node("a1", None, false));
        mgr.update_state("a1", "completed", Some("done".into()), Some(5000), false);
        let node = mgr.get("a1").unwrap();
        assert_eq!(node.state, "completed");
        assert_eq!(node.result_preview.as_deref(), Some("done"));
    }

    #[test]
    fn active_agents_filters() {
        let mut mgr = AgentTreeManager::new();
        mgr.register(make_node("a1", None, true));
        mgr.register(make_node("a2", None, false));
        mgr.update_state("a2", "completed", None, None, false);
        let active = mgr.active_agents();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, "a1");
    }
}
```

- [ ] **Step 2: Implement AgentTreeManager**

```rust
// src/ipc/agent_tree.rs

//! Agent tree state manager — maintains a global tree of agent nodes.

use std::collections::HashMap;
use std::sync::LazyLock;
use parking_lot::Mutex;

use super::agent_types::AgentNode;

/// Manages the agent hierarchy tree.
pub struct AgentTreeManager {
    nodes: HashMap<String, AgentNode>,
    roots: Vec<String>,
}

/// Global agent tree instance.
pub static AGENT_TREE: LazyLock<Mutex<AgentTreeManager>> =
    LazyLock::new(|| Mutex::new(AgentTreeManager::new()));

impl AgentTreeManager {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            roots: Vec::new(),
        }
    }

    /// Register a new agent node.
    pub fn register(&mut self, node: AgentNode) {
        let id = node.agent_id.clone();
        let parent_id = node.parent_agent_id.clone();
        self.nodes.insert(id.clone(), node);
        if parent_id.is_none() {
            if !self.roots.contains(&id) {
                self.roots.push(id);
            }
        }
    }

    /// Update an agent's state.
    pub fn update_state(
        &mut self,
        agent_id: &str,
        state: &str,
        result_preview: Option<String>,
        duration_ms: Option<u64>,
        had_error: bool,
    ) {
        if let Some(node) = self.nodes.get_mut(agent_id) {
            node.state = state.to_string();
            node.had_error = had_error;
            if let Some(rp) = result_preview {
                node.result_preview = Some(rp);
            }
            if let Some(d) = duration_ms {
                node.duration_ms = Some(d);
                node.completed_at = Some(chrono::Utc::now().timestamp());
            }
        }
    }

    /// Build a tree snapshot (flat nodes → nested tree).
    pub fn build_snapshot(&self) -> Vec<AgentNode> {
        let mut result = Vec::new();
        for root_id in &self.roots {
            if let Some(tree) = self.build_subtree(root_id) {
                result.push(tree);
            }
        }
        result
    }

    fn build_subtree(&self, id: &str) -> Option<AgentNode> {
        let node = self.nodes.get(id)?;
        let mut cloned = node.clone();
        cloned.children = self.nodes.values()
            .filter(|n| n.parent_agent_id.as_deref() == Some(id))
            .filter_map(|n| self.build_subtree(&n.agent_id))
            .collect();
        Some(cloned)
    }

    /// Get a reference to a node.
    pub fn get(&self, agent_id: &str) -> Option<&AgentNode> {
        self.nodes.get(agent_id)
    }

    /// Get all currently running agents.
    pub fn active_agents(&self) -> Vec<&AgentNode> {
        self.nodes.values()
            .filter(|n| n.state == "running")
            .collect()
    }

    /// Remove completed agents older than max_age_secs.
    pub fn remove_completed(&mut self, max_age_secs: u64) {
        let now = chrono::Utc::now().timestamp();
        let to_remove: Vec<String> = self.nodes.iter()
            .filter(|(_, n)| {
                n.state != "running" &&
                n.completed_at.map(|t| (now - t) as u64 > max_age_secs).unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &to_remove {
            self.nodes.remove(id);
            self.roots.retain(|r| r != id);
        }
    }
}
```

- [ ] **Step 3: Add module, run tests, commit**

```bash
git commit -m "feat(ipc): add AgentTreeManager for agent hierarchy tracking"
```

---

## Task 5: Protocol Extension + Agent Handlers

**Files:**
- Modify: `src/ipc/protocol.rs`
- Create: `src/ipc/agent_handlers.rs`
- Modify: `src/ipc/mod.rs`

- [ ] **Step 1: Add BackendMessage variants to protocol.rs**

```rust
/// Agent lifecycle + streaming events.
AgentEvent { event: super::agent_events::AgentEvent },

/// Team events.
TeamEvent { event: super::agent_events::TeamEvent },
```

- [ ] **Step 2: Add FrontendMessage variants to protocol.rs**

```rust
/// Agent management commands.
AgentCommand { command: super::agent_events::AgentCommand },

/// Team management commands.
TeamCommand { command: super::agent_events::TeamCommand },
```

- [ ] **Step 3: Add tests to protocol.rs**

```rust
#[test]
fn backend_agent_event_serializes() {
    use crate::ipc::agent_events::AgentEvent;
    let msg = BackendMessage::AgentEvent {
        event: AgentEvent::Spawned {
            agent_id: "a1".into(), parent_agent_id: None,
            description: "test".into(), agent_type: None, model: None,
            is_background: false, depth: 1, chain_id: "c1".into(),
        },
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "agent_event");
    assert_eq!(json["event"]["kind"], "spawned");
}

#[test]
fn frontend_agent_command_deserializes() {
    let json = r#"{"type":"agent_command","command":{"kind":"abort_agent","agent_id":"a1"}}"#;
    let msg: FrontendMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, FrontendMessage::AgentCommand { .. }));
}
```

- [ ] **Step 4: Create agent_handlers.rs**

```rust
// src/ipc/agent_handlers.rs

//! Command handlers for agent and team IPC commands.

use tracing::debug;
use crate::ipc::protocol::{send_to_frontend, BackendMessage};
use crate::ipc::agent_events::*;
use crate::ipc::agent_tree::AGENT_TREE;

pub async fn handle_agent_command(cmd: AgentCommand) {
    match cmd {
        AgentCommand::AbortAgent { agent_id } => {
            debug!(agent_id = %agent_id, "IPC: abort agent requested");
            AGENT_TREE.lock().update_state(&agent_id, "aborted", None, None, false);
            let _ = send_to_frontend(&BackendMessage::AgentEvent {
                event: AgentEvent::Aborted { agent_id },
            });
            // Push updated tree snapshot
            let roots = AGENT_TREE.lock().build_snapshot();
            let _ = send_to_frontend(&BackendMessage::AgentEvent {
                event: AgentEvent::TreeSnapshot { roots },
            });
        }
        AgentCommand::QueryActiveAgents => {
            let roots = AGENT_TREE.lock().build_snapshot();
            let _ = send_to_frontend(&BackendMessage::AgentEvent {
                event: AgentEvent::TreeSnapshot { roots },
            });
        }
        AgentCommand::QueryAgentOutput { agent_id } => {
            debug!(agent_id = %agent_id, "IPC: query agent output");
            // Output buffer not implemented yet — return info message
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!("Agent output replay not yet implemented for {}", agent_id),
                level: "info".into(),
            });
        }
    }
}

pub async fn handle_team_command(cmd: TeamCommand) {
    match cmd {
        TeamCommand::InjectMessage { team_name, to, text } => {
            debug!(team = %team_name, to = %to, "IPC: inject team message");
            match crate::teams::mailbox::deliver_message(&team_name, "__frontend__", &to, &text) {
                Ok(()) => {}
                Err(e) => {
                    let _ = send_to_frontend(&BackendMessage::SystemInfo {
                        text: format!("Failed to inject message: {}", e),
                        level: "error".into(),
                    });
                }
            }
        }
        TeamCommand::QueryTeamStatus { team_name } => {
            debug!(team = %team_name, "IPC: query team status");
            let _ = send_to_frontend(&BackendMessage::SystemInfo {
                text: format!("Team status query not yet implemented for {}", team_name),
                level: "info".into(),
            });
        }
    }
}
```

- [ ] **Step 5: Add modules, handle FrontendMessage stubs in headless.rs, build, commit**

```bash
git commit -m "feat(ipc): add agent/team protocol variants and command handlers"
```

---

## Task 6: Wire Headless Event Loop — Replace Branch 2

**Files:**
- Modify: `src/ipc/headless.rs`

- [ ] **Step 1: Replace bg_rx channel creation with agent_channel**

Change:
```rust
let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
engine.set_bg_agent_tx(bg_tx);
```
To:
```rust
let (agent_tx, mut agent_rx) = crate::ipc::agent_channel::agent_channel();
engine.set_agent_tx(agent_tx);
```

- [ ] **Step 2: Replace Branch 2 (bg_rx) with full agent event handling**

Replace the existing `Some(completed) = bg_rx.recv()` branch with:

```rust
Some(event) = agent_rx.recv() => {
    match event {
        crate::ipc::agent_channel::AgentIpcEvent::Agent(ref agent_event) => {
            // Backward compat: also send BackgroundAgentComplete for completed bg agents
            if let crate::ipc::agent_events::AgentEvent::Completed {
                ref agent_id, ref result_preview, had_error, duration_ms, ..
            } = agent_event {
                // Check if this was a background agent
                let tree = crate::ipc::agent_tree::AGENT_TREE.lock();
                if let Some(node) = tree.get(agent_id) {
                    if node.is_background {
                        let _ = send_to_frontend(&BackendMessage::BackgroundAgentComplete {
                            agent_id: agent_id.clone(),
                            description: node.description.clone(),
                            result_preview: result_preview.clone(),
                            had_error,
                            duration_ms,
                        });
                        // Push to pending_bg for query loop injection
                        pending_bg.push(crate::tools::background_agents::CompletedBackgroundAgent {
                            agent_id: agent_id.clone(),
                            description: node.description.clone(),
                            result_text: result_preview.clone(),
                            had_error,
                            duration: std::time::Duration::from_millis(duration_ms),
                        });
                    }
                }
                drop(tree);
            }
            let _ = send_to_frontend(&BackendMessage::AgentEvent {
                event: agent_event.clone(),
            });
        }
        crate::ipc::agent_channel::AgentIpcEvent::Team(team_event) => {
            let _ = send_to_frontend(&BackendMessage::TeamEvent {
                event: team_event,
            });
        }
    }
}
```

- [ ] **Step 3: Add FrontendMessage command dispatch**

```rust
FrontendMessage::AgentCommand { command } => {
    debug!("headless: Agent command: {:?}", command);
    crate::ipc::agent_handlers::handle_agent_command(command).await;
}
FrontendMessage::TeamCommand { command } => {
    debug!("headless: Team command: {:?}", command);
    crate::ipc::agent_handlers::handle_team_command(command).await;
}
```

- [ ] **Step 4: Build, fix compilation**

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(ipc): replace bg_rx with full agent event channel in headless loop"
```

---

## Task 7: Engine bg_agent_tx Type Migration

**Files:**
- Modify: `src/engine/lifecycle/mod.rs`
- Modify: `src/engine/lifecycle/deps.rs`
- Modify: `src/engine/lifecycle/submit_message.rs`
- Modify: `src/tools/background_agents.rs`

- [ ] **Step 1: Add AgentSender re-export in background_agents.rs**

```rust
// Add at bottom of background_agents.rs
pub use crate::ipc::agent_channel::AgentSender;
```

- [ ] **Step 2: Update QueryEngine state in mod.rs**

Change line 76:
```rust
pub(crate) bg_agent_tx: Option<crate::tools::background_agents::BgAgentSender>,
```
To:
```rust
pub(crate) bg_agent_tx: Option<crate::ipc::agent_channel::AgentSender>,
```

Update `set_bg_agent_tx` (line 173-174):
```rust
pub fn set_agent_tx(&self, tx: crate::ipc::agent_channel::AgentSender) {
    self.state.write().bg_agent_tx = Some(tx);
}
```

Keep `set_bg_agent_tx` as a deprecated alias for backward compat.

- [ ] **Step 3: Update deps.rs**

Change line 44 to match new type. Also update line 359 clone.

- [ ] **Step 4: Update submit_message.rs**

Line 288 reads `bg_agent_tx` — type changes but `.clone()` still works. Line 295 passes it to ToolUseContext — the `bg_agent_tx` field on ToolUseContext also needs updating.

- [ ] **Step 5: Update ToolUseContext in types/tool.rs**

Change:
```rust
pub bg_agent_tx: Option<crate::tools::background_agents::BgAgentSender>,
```
To:
```rust
pub bg_agent_tx: Option<crate::ipc::agent_channel::AgentSender>,
```

- [ ] **Step 6: Build and fix all compilation errors**

Run: `cargo build 2>&1`
Fix any remaining type mismatches.

- [ ] **Step 7: Commit**

```bash
git commit -m "refactor: migrate bg_agent_tx from BgAgentSender to AgentSender"
```

---

## Task 8: Agent Tool — Emit Spawned + Streaming + Completed

**Files:**
- Modify: `src/tools/agent/mod.rs`
- Modify: `src/tools/agent/tool_impl.rs`
- Modify: `src/tools/agent/dispatch.rs`

- [ ] **Step 1: Add `sdk_to_agent_event` helper in agent/mod.rs**

```rust
/// Convert an SdkMessage to an AgentEvent for IPC forwarding.
pub(crate) fn sdk_to_agent_event(
    sdk_msg: &crate::engine::sdk_types::SdkMessage,
    agent_id: &str,
) -> Option<crate::ipc::agent_events::AgentEvent> {
    use crate::engine::sdk_types::SdkMessage;
    use crate::ipc::agent_events::AgentEvent;
    use crate::types::message::{ContentBlock, StreamEvent, ToolResultContent};

    match sdk_msg {
        SdkMessage::StreamEvent(evt) => match &evt.event {
            StreamEvent::ContentBlockDelta { delta, .. } => {
                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                    Some(AgentEvent::StreamDelta {
                        agent_id: agent_id.to_string(),
                        text: text.to_string(),
                    })
                } else if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                    Some(AgentEvent::ThinkingDelta {
                        agent_id: agent_id.to_string(),
                        thinking: thinking.to_string(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        },
        SdkMessage::Assistant(a) => {
            for block in &a.message.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    return Some(AgentEvent::ToolUse {
                        agent_id: agent_id.to_string(),
                        tool_use_id: id.clone(),
                        tool_name: name.clone(),
                        input: input.clone(),
                    });
                }
            }
            None
        }
        SdkMessage::UserReplay(replay) => {
            if let Some(blocks) = &replay.content_blocks {
                for block in blocks {
                    if let ContentBlock::ToolResult { tool_use_id, content, is_error } = block {
                        let output = match content {
                            ToolResultContent::Text(t) => t.clone(),
                            ToolResultContent::Blocks(_) => "[complex output]".to_string(),
                        };
                        return Some(AgentEvent::ToolResult {
                            agent_id: agent_id.to_string(),
                            tool_use_id: tool_use_id.clone(),
                            output,
                            is_error: *is_error,
                        });
                    }
                }
            }
            None
        }
        _ => None,
    }
}
```

- [ ] **Step 2: Modify tool_impl.rs background spawn**

In the `tokio::spawn` block (around line 241-297), replace:
```rust
let (result_text, had_error) = collect_stream_result(stream).await;
```
With streaming forwarding:
```rust
let stream = child_engine.submit_message(&spawn_prompt, QuerySource::Agent(spawn_agent_id.clone()));
let mut stream = std::pin::pin!(stream);
let mut result_text = String::new();
let mut had_error = false;

while let Some(msg) = stream.next().await {
    // Collect text (existing logic)
    match &msg {
        SdkMessage::Assistant(ref a) => {
            for block in &a.message.content {
                if let ContentBlock::Text { text } = block {
                    if !result_text.is_empty() { result_text.push('\n'); }
                    result_text.push_str(text);
                }
            }
        }
        SdkMessage::Result(ref r) => {
            if r.is_error { had_error = true; if !r.result.is_empty() { result_text = r.result.clone(); } }
            else if result_text.is_empty() && !r.result.is_empty() { result_text = r.result.clone(); }
        }
        _ => {}
    }

    // Forward to IPC
    if let Some(agent_event) = super::sdk_to_agent_event(&msg, &spawn_agent_id) {
        let _ = bg_tx.send(crate::ipc::agent_channel::AgentIpcEvent::Agent(agent_event));
    }
}
```

- [ ] **Step 3: Emit Spawned event before background spawn**

Before the `tokio::spawn` block, add:
```rust
// Register in tree and emit Spawned
{
    let node = crate::ipc::agent_types::AgentNode {
        agent_id: agent_id.clone(),
        parent_agent_id: ctx.agent_id.clone(),
        description: description.to_string(),
        agent_type: params.subagent_type.clone(),
        model: Some(agent_model.clone()),
        state: "running".into(),
        is_background: true,
        depth: current_depth + 1,
        chain_id: ctx.query_tracking.as_ref().map(|t| t.chain_id.clone()).unwrap_or_default(),
        spawned_at: chrono::Utc::now().timestamp(),
        completed_at: None, duration_ms: None, result_preview: None,
        had_error: false, children: vec![],
    };
    crate::ipc::agent_tree::AGENT_TREE.lock().register(node);

    let _ = bg_tx.send(crate::ipc::agent_channel::AgentIpcEvent::Agent(
        crate::ipc::agent_events::AgentEvent::Spawned {
            agent_id: agent_id.clone(),
            parent_agent_id: ctx.agent_id.clone(),
            description: description.to_string(),
            agent_type: params.subagent_type.clone(),
            model: Some(agent_model.clone()),
            is_background: true,
            depth: current_depth + 1,
            chain_id: ctx.query_tracking.as_ref().map(|t| t.chain_id.clone()).unwrap_or_default(),
        },
    ));

    // Push tree snapshot
    let roots = crate::ipc::agent_tree::AGENT_TREE.lock().build_snapshot();
    let _ = bg_tx.send(crate::ipc::agent_channel::AgentIpcEvent::Agent(
        crate::ipc::agent_events::AgentEvent::TreeSnapshot { roots },
    ));
}
```

- [ ] **Step 4: Replace `bg_tx.send(CompletedBackgroundAgent{...})` with AgentEvent::Completed**

Replace:
```rust
let _ = bg_tx.send(crate::tools::background_agents::CompletedBackgroundAgent { ... });
```
With:
```rust
// Update tree
crate::ipc::agent_tree::AGENT_TREE.lock().update_state(
    &spawn_agent_id, if had_error { "error" } else { "completed" },
    Some(result_preview.clone()), Some(duration_ms), had_error,
);

// Send Completed event
let _ = bg_tx.send(crate::ipc::agent_channel::AgentIpcEvent::Agent(
    crate::ipc::agent_events::AgentEvent::Completed {
        agent_id: spawn_agent_id.clone(),
        result_preview,
        had_error,
        duration_ms,
        output_tokens: None,
    },
));

// Push tree snapshot
let roots = crate::ipc::agent_tree::AGENT_TREE.lock().build_snapshot();
let _ = bg_tx.send(crate::ipc::agent_channel::AgentIpcEvent::Agent(
    crate::ipc::agent_events::AgentEvent::TreeSnapshot { roots },
));
```

- [ ] **Step 5: Add Spawned/Completed for synchronous agents in dispatch.rs**

In `run_agent_normal()`, emit Spawned before the stream and Completed after:
```rust
// Before stream
{
    let node = crate::ipc::agent_types::AgentNode { ... state: "running", is_background: false, ... };
    crate::ipc::agent_tree::AGENT_TREE.lock().register(node);
}

// After collect_stream_result
{
    crate::ipc::agent_tree::AGENT_TREE.lock().update_state(agent_id, ...);
}
```

Note: sync agents don't have `bg_tx`, so they only update the tree — no IPC events are sent (their output goes through normal ToolUse/ToolResult flow).

- [ ] **Step 6: Build, fix compilation**

- [ ] **Step 7: Commit**

```bash
git commit -m "feat(agent): emit lifecycle events and stream forwarding via agent channel"
```

---

## Task 9: SystemStatus Tool — Add agents/teams

**Files:**
- Modify: `src/tools/system_status.rs`

- [ ] **Step 1: Extend input schema enum**

```rust
"enum": ["lsp", "mcp", "plugins", "skills", "agents", "teams", "all"],
```

- [ ] **Step 2: Add agents section to format_status_output**

```rust
if subsystem == "all" || subsystem == "agents" {
    let tree = crate::ipc::agent_tree::AGENT_TREE.lock();
    let active = tree.active_agents();
    let total = tree.build_snapshot().iter().map(|r| count_nodes(r)).sum::<usize>();
    let bg_count = active.iter().filter(|a| a.is_background).count();
    let mut section = format!("## Active Agents ({} total, {} background)\n", active.len(), bg_count);
    if active.is_empty() {
        section.push_str("No active agents.\n");
    } else {
        for a in &active {
            section.push_str(&format!("- {}: {} [{}{}] — \"{}\" (depth {})\n",
                a.agent_id, a.state,
                if a.is_background { "background" } else { "sync" },
                a.agent_type.as_ref().map(|t| format!(", {}", t)).unwrap_or_default(),
                a.description, a.depth,
            ));
        }
    }
    parts.push(section);
}
```

- [ ] **Step 3: Add teams section (placeholder if feature-gated)**

```rust
if subsystem == "all" || subsystem == "teams" {
    let mut section = String::from("## Teams\n");
    section.push_str("Team status requires CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS feature.\n");
    parts.push(section);
}
```

- [ ] **Step 4: Add helper**

```rust
fn count_nodes(node: &crate::ipc::agent_types::AgentNode) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}
```

- [ ] **Step 5: Update system_prompt.rs reminder**

Add after skill_count:
```rust
let agent_count = crate::ipc::agent_tree::AGENT_TREE.lock().active_agents().len();
// Add to format string:
// - Agents: {} active
```

- [ ] **Step 6: Build, test, commit**

```bash
git commit -m "feat(tools): extend SystemStatus with agents and teams subsystems"
```

---

## Task 10: Resolve Warnings + Final Build + Full Test

**Files:**
- Various

- [ ] **Step 1: Full build**

Run: `cargo build 2>&1`

- [ ] **Step 2: Fix all warnings (unused imports, dead_code, etc.)**

- [ ] **Step 3: Run full test suite**

Run: `cargo test --bin claude-code-rs 2>&1 | tail -20`

- [ ] **Step 4: Commit fixes**

```bash
git commit -m "chore: resolve warnings from agent IPC extensions"
```

---

## Spec Coverage Check

| Spec Section | Task(s) |
|---|---|
| 1. Channel architecture | T3 (channel types), T6 (headless wiring), T7 (engine migration) |
| 2. AgentEvent enum | T2 |
| 3. TeamEvent enum | T2 |
| 4. Shared types | T1 |
| 5. BackendMessage/FrontendMessage | T5 |
| 6. JSON wire format | T2 tests validate serialization |
| 7. Agent tree manager | T4 |
| 8. Background agent streaming | T8 |
| 9. Team mailbox bridging | T5 (handler), T8 not fully wired (mailbox emit deferred) |
| 10. Headless event loop | T6 |
| 11. SystemStatus extension | T9 |
| 12. File organization | All tasks follow spec layout |
| 13. Backward compat | T6 (dual send), T7 (type alias) |
