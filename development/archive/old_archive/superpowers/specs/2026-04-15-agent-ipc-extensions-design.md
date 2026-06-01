# Agent IPC Extensions Design

**Date:** 2026-04-15
**Status:** Draft
**Scope:** Headless IPC 协议扩展 — Agent 全生命周期 + 流式输出 + Agent Teams 消息

## 概述

扩展 Headless IPC 协议，使前端能够：

1. **Agent 全生命周期追踪** — spawn / running / completed / error / aborted
2. **全量流式输出** — background agent 的 StreamDelta、ThinkingDelta、ToolUse、ToolResult 实时推送
3. **Agent 树结构** — 嵌套 subagent 的层级树，每次变更推送完整快照
4. **Agent 管理** — 查询活跃 agent 列表、中止指定 agent
5. **Agent Teams 消息观察** — 被动接收所有 team 消息副本
6. **Agent Teams 消息注入** — 前端可主动发消息给任意 agent

**通道架构**: 方案 B — Agent 专用 `mpsc::unbounded_channel`，替代现有 `bg_rx`，与 SubsystemEventBus 并行。

---

## 1. 通道架构

### 1.1 替代现有 bg_rx

现有实现:
```rust
// background_agents.rs
let (bg_tx, bg_rx) = tokio::sync::mpsc::unbounded_channel::<CompletedBackgroundAgent>();
```

替换为:
```rust
// agent_channel.rs
let (agent_tx, agent_rx) = tokio::sync::mpsc::unbounded_channel::<AgentIpcEvent>();
```

### 1.2 AgentIpcEvent — 通道内部类型

```rust
/// 通过 agent 专用通道传递的所有事件。
/// 不需要 Serialize — 在 headless.rs 中转换为 BackendMessage 后才序列化。
#[derive(Debug)]
pub enum AgentIpcEvent {
    Agent(AgentEvent),
    Team(TeamEvent),
}
```

### 1.3 Sender 注入点

`agent_tx: mpsc::UnboundedSender<AgentIpcEvent>` 需要注入到：

| 注入点 | 用途 |
|--------|------|
| `QueryEngine.bg_agent_tx` | 替换现有字段类型 |
| Agent tool `dispatch.rs` | spawn 事件、background agent 的流式转发 |
| Agent tool `tool_impl.rs` | spawned/completed/error 事件 |
| Teams `mailbox.rs` | 消息路由观察 |
| Teams `runner.rs` | 成员加入/离开事件 |

---

## 2. Agent 事件枚举

### 2.1 AgentEvent（Backend → Frontend）

```rust
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    // ── 生命周期 ──

    /// Agent 已创建并开始执行
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

    /// Agent 正常完成
    Completed {
        agent_id: String,
        result_preview: String,
        had_error: bool,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
    },

    /// Agent 执行出错
    Error {
        agent_id: String,
        error: String,
        duration_ms: u64,
    },

    /// Agent 被用户/前端中止
    Aborted {
        agent_id: String,
    },

    // ── 流式输出（background agents）──

    /// 文本流式增量
    StreamDelta {
        agent_id: String,
        text: String,
    },

    /// 思维链流式增量
    ThinkingDelta {
        agent_id: String,
        thinking: String,
    },

    /// Agent 调用了一个工具
    ToolUse {
        agent_id: String,
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
    },

    /// 工具调用结果
    ToolResult {
        agent_id: String,
        tool_use_id: String,
        output: String,
        is_error: bool,
    },

    // ── 树快照 ──

    /// Agent 树状态变更（每次 spawn/complete/error 后推送）
    TreeSnapshot {
        roots: Vec<AgentNode>,
    },
}
```

### 2.2 AgentCommand（Frontend → Backend）

```rust
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCommand {
    /// 中止指定 agent
    AbortAgent { agent_id: String },
    /// 查询当前活跃 agent 树
    QueryActiveAgents,
    /// 查询指定 agent 的完整输出
    QueryAgentOutput { agent_id: String },
}
```

---

## 3. Team 事件枚举

### 3.1 TeamEvent（Backend → Frontend）

```rust
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamEvent {
    /// 成员加入 team
    MemberJoined {
        team_name: String,
        agent_id: String,
        agent_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },

    /// 成员离开 team
    MemberLeft {
        team_name: String,
        agent_id: String,
        agent_name: String,
    },

    /// Agent 间消息被路由（观察副本）
    MessageRouted {
        team_name: String,
        from: String,
        to: String,
        text: String,
        timestamp: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    /// Team 状态快照（响应 QueryTeamStatus）
    StatusSnapshot {
        team_name: String,
        members: Vec<TeamMemberInfo>,
        pending_messages: usize,
    },
}
```

### 3.2 TeamCommand（Frontend → Backend）

```rust
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamCommand {
    /// 前端向指定 agent 注入消息
    InjectMessage {
        team_name: String,
        to: String,
        text: String,
    },
    /// 查询 team 状态
    QueryTeamStatus {
        team_name: String,
    },
}
```

---

## 4. 共享数据类型

### 4.1 AgentNode — 树节点

```rust
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
    /// 子 agent 列表（嵌套递归）
    pub children: Vec<AgentNode>,
}
```

### 4.2 AgentInfo — 扁平信息（用于 SystemStatus 工具）

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentInfo {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub description: String,
    pub state: String,
    pub is_background: bool,
    pub depth: usize,
    pub duration_ms: Option<u64>,
}
```

### 4.3 TeamMemberInfo

```rust
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

---

## 5. BackendMessage / FrontendMessage 新增变体

### 5.1 BackendMessage

```rust
/// Agent 子系统事件（生命周期 + 流式 + 树快照）
AgentEvent { event: AgentEvent },

/// Team 子系统事件（成员变更 + 消息路由）
TeamEvent { event: TeamEvent },
```

注意：现有 `BackgroundAgentComplete` 保留不变，确保旧前端兼容。新前端监听 `AgentEvent::Completed` 代替。

### 5.2 FrontendMessage

```rust
/// Agent 管理命令
AgentCommand { command: AgentCommand },

/// Team 管理命令
TeamCommand { command: TeamCommand },
```

---

## 6. JSON Wire Format 示例

### 6.1 Backend → Frontend

```json
// Agent 创建
{
  "type": "agent_event",
  "event": {
    "kind": "spawned",
    "agent_id": "agent-abc123",
    "parent_agent_id": null,
    "description": "Explore codebase structure",
    "agent_type": "Explore",
    "model": "haiku",
    "is_background": true,
    "depth": 1,
    "chain_id": "chain-xyz"
  }
}

// Background agent 流式文本
{
  "type": "agent_event",
  "event": {
    "kind": "stream_delta",
    "agent_id": "agent-abc123",
    "text": "Found 3 relevant files..."
  }
}

// Agent 调用工具
{
  "type": "agent_event",
  "event": {
    "kind": "tool_use",
    "agent_id": "agent-abc123",
    "tool_use_id": "tu-001",
    "tool_name": "Grep",
    "input": {"pattern": "struct AgentNode", "path": "src/"}
  }
}

// Agent 完成
{
  "type": "agent_event",
  "event": {
    "kind": "completed",
    "agent_id": "agent-abc123",
    "result_preview": "Found 3 files matching...",
    "had_error": false,
    "duration_ms": 12500,
    "output_tokens": 1850
  }
}

// Agent 树快照
{
  "type": "agent_event",
  "event": {
    "kind": "tree_snapshot",
    "roots": [
      {
        "agent_id": "agent-abc123",
        "parent_agent_id": null,
        "description": "Research task",
        "state": "running",
        "is_background": true,
        "depth": 1,
        "chain_id": "chain-xyz",
        "spawned_at": 1744675200,
        "had_error": false,
        "children": [
          {
            "agent_id": "agent-def456",
            "parent_agent_id": "agent-abc123",
            "description": "Search for patterns",
            "state": "completed",
            "is_background": false,
            "depth": 2,
            "chain_id": "chain-xyz",
            "spawned_at": 1744675205,
            "completed_at": 1744675210,
            "duration_ms": 5000,
            "result_preview": "Found 2 matches",
            "had_error": false,
            "children": []
          }
        ]
      }
    ]
  }
}

// Team 消息路由观察
{
  "type": "team_event",
  "event": {
    "kind": "message_routed",
    "team_name": "my-team",
    "from": "lead",
    "to": "worker-1",
    "text": "Please review src/main.rs",
    "timestamp": "2026-04-15T10:30:00Z",
    "summary": null
  }
}
```

### 6.2 Frontend → Backend

```json
// 中止 agent
{"type": "agent_command", "command": {"kind": "abort_agent", "agent_id": "agent-abc123"}}

// 查询活跃 agent 树
{"type": "agent_command", "command": {"kind": "query_active_agents"}}

// 注入 team 消息
{"type": "team_command", "command": {"kind": "inject_message", "team_name": "my-team", "to": "worker-1", "text": "stop and report"}}

// 查询 team 状态
{"type": "team_command", "command": {"kind": "query_team_status", "team_name": "my-team"}}
```

---

## 7. Agent 树管理

### 7.1 AgentTreeManager

后端维护一个全局的 `AgentTreeManager`，负责 agent 节点的增删改查和树快照推送：

```rust
// src/ipc/agent_tree.rs

pub struct AgentTreeManager {
    nodes: HashMap<String, AgentNode>,     // agent_id → flat node (children 为空)
    roots: Vec<String>,                     // 顶级 agent_id 列表
    tx: Option<mpsc::UnboundedSender<AgentIpcEvent>>,
}

impl AgentTreeManager {
    pub fn new() -> Self;

    /// 注入 channel sender
    pub fn set_sender(&mut self, tx: mpsc::UnboundedSender<AgentIpcEvent>);

    /// 注册新 agent，自动推送 TreeSnapshot
    pub fn register(&mut self, node: AgentNode);

    /// 更新 agent 状态（completed/error/aborted），自动推送 TreeSnapshot
    pub fn update_state(&mut self, agent_id: &str, state: &str, result_preview: Option<String>, duration_ms: Option<u64>, had_error: bool);

    /// 移除已完成的 agent（可选，用于清理）
    pub fn remove_completed(&mut self, max_age_secs: u64);

    /// 构建树快照（将 flat nodes 重组为嵌套树）
    pub fn build_snapshot(&self) -> Vec<AgentNode>;

    /// 查找 agent
    pub fn get(&self, agent_id: &str) -> Option<&AgentNode>;

    /// 所有运行中的 agent
    pub fn active_agents(&self) -> Vec<&AgentNode>;
}
```

全局实例:
```rust
static AGENT_TREE: LazyLock<Mutex<AgentTreeManager>> =
    LazyLock::new(|| Mutex::new(AgentTreeManager::new()));
```

### 7.2 树快照推送时机

| 触发事件 | 推送 TreeSnapshot |
|----------|------------------|
| Agent Spawned | 是 |
| Agent Completed | 是 |
| Agent Error | 是 |
| Agent Aborted | 是 |
| StreamDelta / ToolUse | 否（太频繁） |
| QueryActiveAgents | 是（作为响应） |

---

## 8. Background Agent 流式转发

### 8.1 当前实现

```rust
// tool_impl.rs — background spawn
tokio::spawn(async move {
    let stream = child_engine.submit_message(&prompt, source);
    let mut stream = std::pin::pin!(stream);
    let mut result_text = String::new();
    while let Some(sdk_msg) = stream.next().await {
        // 只收集最终文本，不转发
        collect_text(&sdk_msg, &mut result_text);
    }
    // 发送 CompletedBackgroundAgent
    let _ = bg_tx.send(CompletedBackgroundAgent { ... });
});
```

### 8.2 改造：加入流式转发

```rust
tokio::spawn(async move {
    let agent_id = agent_id.clone();
    let stream = child_engine.submit_message(&prompt, source);
    let mut stream = std::pin::pin!(stream);
    let mut result_text = String::new();

    while let Some(sdk_msg) = stream.next().await {
        collect_text(&sdk_msg, &mut result_text);

        // 转发流式事件到前端
        if let Some(forwarded) = sdk_to_agent_event(&sdk_msg, &agent_id) {
            let _ = agent_tx.send(AgentIpcEvent::Agent(forwarded));
        }
    }

    // 发送完成事件
    let _ = agent_tx.send(AgentIpcEvent::Agent(AgentEvent::Completed { ... }));
});
```

### 8.3 SdkMessage → AgentEvent 映射

```rust
fn sdk_to_agent_event(sdk_msg: &SdkMessage, agent_id: &str) -> Option<AgentEvent> {
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
            // 提取 ToolUse blocks
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
        },
        SdkMessage::UserReplay(replay) => {
            // 提取 ToolResult blocks
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
        },
        _ => None,
    }
}
```

---

## 9. Team Mailbox 桥接

### 9.1 消息观察

在 `src/teams/mailbox.rs` 的 `deliver_message()` 函数中，投递成功后发送观察事件：

```rust
pub fn deliver_message(team_name: &str, from: &str, to: &str, text: &str) -> Result<()> {
    // ... 现有的文件 mailbox 投递逻辑 ...

    // 发送 IPC 观察事件
    emit_team_event(TeamEvent::MessageRouted {
        team_name: team_name.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        text: text.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        summary: None,
    });

    Ok(())
}
```

### 9.2 消息注入

`TeamCommand::InjectMessage` 处理：

```rust
async fn handle_inject_message(team_name: &str, to: &str, text: &str) {
    // 写入目标 agent 的 mailbox 文件
    let result = crate::teams::mailbox::deliver_message(
        team_name,
        "__frontend__",  // 特殊发送者标识
        to,
        text,
    );

    match result {
        Ok(()) => { /* 消息已投递，MessageRouted 事件会自动发出 */ }
        Err(e) => {
            // 发送错误到前端
        }
    }
}
```

### 9.3 成员变更

在 `src/teams/runner.rs` 的 agent 启动/停止点发送事件：

```rust
// Agent 启动时
emit_team_event(TeamEvent::MemberJoined {
    team_name, agent_id, agent_name, role,
});

// Agent 停止时
emit_team_event(TeamEvent::MemberLeft {
    team_name, agent_id, agent_name,
});
```

事件发送使用与 agent 相同的 `agent_tx` 通道（通过 `AgentIpcEvent::Team(event)` 包装）。

---

## 10. Headless 事件循环扩展

### 10.1 替换 Branch 2

现有 Branch 2 (`bg_rx.recv()`) 替换为处理全量 AgentIpcEvent:

```rust
// headless.rs — tokio::select!

// ── Branch 2: Agent events (替代原 bg_rx) ──────────────
Some(event) = agent_rx.recv() => {
    match event {
        AgentIpcEvent::Agent(agent_event) => {
            // 同时维护兼容性：对 Completed 事件也发 BackgroundAgentComplete
            if let AgentEvent::Completed { ref agent_id, ref result_preview, had_error, duration_ms, .. } = agent_event {
                if is_background_agent(agent_id) {
                    let _ = send_to_frontend(&BackendMessage::BackgroundAgentComplete {
                        agent_id: agent_id.clone(),
                        description: get_agent_description(agent_id),
                        result_preview: result_preview.clone(),
                        had_error,
                        duration_ms,
                    });
                    // 也推入 pending_bg 以兼容 query loop 注入
                    pending_bg.push(to_completed_bg_agent(&agent_event));
                }
            }
            let _ = send_to_frontend(&BackendMessage::AgentEvent { event: agent_event });
        }
        AgentIpcEvent::Team(team_event) => {
            let _ = send_to_frontend(&BackendMessage::TeamEvent { event: team_event });
        }
    }
}
```

### 10.2 新增 FrontendMessage 处理

```rust
FrontendMessage::AgentCommand { command } => {
    debug!("headless: Agent command: {:?}", command);
    handle_agent_command(command, &engine).await;
}
FrontendMessage::TeamCommand { command } => {
    debug!("headless: Team command: {:?}", command);
    handle_team_command(command).await;
}
```

### 10.3 命令处理

```rust
async fn handle_agent_command(cmd: AgentCommand, engine: &Arc<QueryEngine>) {
    match cmd {
        AgentCommand::AbortAgent { agent_id } => {
            // 查找并中止指定 agent（通过 abort signal）
            AGENT_TREE.lock().update_state(&agent_id, "aborted", None, None, false);
        }
        AgentCommand::QueryActiveAgents => {
            let tree = AGENT_TREE.lock().build_snapshot();
            let _ = send_to_frontend(&BackendMessage::AgentEvent {
                event: AgentEvent::TreeSnapshot { roots: tree },
            });
        }
        AgentCommand::QueryAgentOutput { agent_id } => {
            // 从 agent output buffer 读取完整输出
            // 作为 SystemInfo 发送（或新增专用变体）
        }
    }
}

async fn handle_team_command(cmd: TeamCommand) {
    match cmd {
        TeamCommand::InjectMessage { team_name, to, text } => {
            handle_inject_message(&team_name, &to, &text);
        }
        TeamCommand::QueryTeamStatus { team_name } => {
            let status = build_team_status(&team_name);
            let _ = send_to_frontend(&BackendMessage::TeamEvent {
                event: TeamEvent::StatusSnapshot {
                    team_name,
                    members: status.members,
                    pending_messages: status.pending,
                },
            });
        }
    }
}
```

---

## 11. Agent 集成到 SystemStatus 工具

扩展现有 `SystemStatus` 工具，新增 `"agents"` 和 `"teams"` 子系统查询：

```rust
// input_schema subsystem enum 扩展为:
"enum": ["lsp", "mcp", "plugins", "skills", "agents", "teams", "all"]
```

输出示例：
```
## Active Agents (3 total, 1 background)
- agent-abc123: running [background, Explore] — "Research codebase" (12s)
  - agent-def456: completed [Explore] — "Search patterns" (5s)
- agent-ghi789: running [general-purpose] — "Fix bug" (3s)

## Teams
- my-team: 3 members (2 active), 5 pending messages
  - lead: active
  - worker-1: active (2 unread)
  - worker-2: inactive
```

系统提示词注入也相应扩展：
```
- Agents: 3 active (1 background)
- Teams: 1 team, 3 members
```

---

## 12. 文件组织

```
src/ipc/
├── mod.rs                     (修改: 声明新模块)
├── protocol.rs                (修改: 新增 AgentEvent/TeamEvent 变体)
├── headless.rs                (修改: 替换 Branch 2, 新增命令处理)
├── agent_events.rs            (NEW: AgentEvent, AgentCommand, TeamEvent, TeamCommand 枚举)
├── agent_types.rs             (NEW: AgentNode, AgentInfo, TeamMemberInfo)
├── agent_tree.rs              (NEW: AgentTreeManager)
├── agent_handlers.rs          (NEW: handle_agent_command, handle_team_command)
├── agent_channel.rs           (NEW: AgentIpcEvent, agent_tx/rx 类型别名)
├── subsystem_types.rs         (修改: 追加 AgentInfo 到 SubsystemStatusSnapshot)
├── subsystem_events.rs        (不变)
├── subsystem_handlers.rs      (修改: build_subsystem_status_snapshot 加 agents/teams)
└── ...existing files...

src/tools/
├── agent/
│   ├── tool_impl.rs           (修改: 发送 Spawned/Completed/Error, 替换 bg_tx 类型)
│   ├── dispatch.rs            (修改: background agent 流式转发)
│   └── ...
├── system_status.rs           (修改: 新增 agents/teams 子系统)
└── ...

src/tools/
└── background_agents.rs       (修改: AgentIpcEvent 替换 CompletedBackgroundAgent)

src/teams/
├── mailbox.rs                 (修改: 投递后发送 MessageRouted)
└── runner.rs                  (修改: 启停时发送 MemberJoined/Left)

src/engine/
├── system_prompt.rs           (修改: 注入 agents/teams 计数)
└── lifecycle/mod.rs           (修改: bg_agent_tx 类型变更)
```

---

## 13. 向后兼容

| 旧机制 | 新机制 | 兼容策略 |
|--------|--------|---------|
| `BackgroundAgentComplete` BackendMessage | `AgentEvent::Completed` | 两者同时发送，旧前端只看 `BackgroundAgentComplete` |
| `CompletedBackgroundAgent` 结构体 | `AgentEvent::Completed` | 保留结构体，在 headless.rs 中从 AgentEvent 转换 |
| `bg_agent_tx: BgAgentSender` | `agent_tx: AgentSender` | 类型别名更新，QueryEngine 接口同步修改 |
| `PendingBackgroundResults` | 保留 | Completed 事件同时推入 pending_bg |

---

## 14. 注意事项

1. **流式吞吐量**: Background agent 全量流式可能产生大量事件。使用 `mpsc::unbounded_channel` 而非 `broadcast` 避免丢失，但前端需处理高频更新（建议前端做 debounce/batch render）。

2. **Agent 输出缓存**: 每个 background agent 的完整输出应缓存在内存中（`HashMap<String, String>`），支持 `QueryAgentOutput` 回看。设置上限（如 1MB/agent, 最多 20 个 agent）防止 OOM。

3. **Agent 中止**: `AbortAgent` 需要将 abort signal 传播到目标 agent 的 `QueryEngine`。这要求 `AgentTreeManager` 持有每个 agent 的 `abort_tx` 引用。

4. **Teams feature gate**: Team 事件仅在 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` 启用时才发送。AgentEvent 不受 feature gate 限制。

5. **Synchronous agent**: 非 background 的 subagent（同步执行）只发 Spawned + Completed，不发 StreamDelta（其流式输出已经通过主对话的 StreamEvent 推送）。

6. **树快照大小**: 深层嵌套（MAX_DEPTH=5）的大量并发 agent 可能使 TreeSnapshot 较大。实际场景中并发 agent 数通常 < 10，不是问题。

7. **Daemon 模式预留**: 所有类型定义可复用于 daemon 模式的 REST/SSE 端点。预留端点:
   ```
   GET  /api/agents/tree              → TreeSnapshot
   POST /api/agents/{id}/abort        → AbortAgent
   GET  /api/agents/{id}/output       → Full output
   SSE  /api/agents/events            → AgentEvent stream
   POST /api/teams/{name}/message     → InjectMessage
   GET  /api/teams/{name}/status      → StatusSnapshot
   ```
