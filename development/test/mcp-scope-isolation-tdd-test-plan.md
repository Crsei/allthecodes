# MCP Scope Isolation TDD 测试计划

> 来源计划: `development/mcp/mcp-plugin-scope-isolation-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: `crates/allthecodes-mcp/`, `crates/allthecodes-engine/src/mcp_tool_adapter.rs`

## 状态

**已全部实现**（2026-07-02）。本计划作为回归测试清单，确保后续改动不破坏 scope isolation。

---

## Phase 0: 基线回归

```rust
// 旧 mcpServers 仍可加载
#[test]
fn legacy_mcp_servers_still_work() {
    // 旧配置无 mcpBindings → 按来源生成隐式 binding
    // user settings → global, project settings → project
}

#[test]
fn agent_thread_does_not_see_unbound_session_mcp() {
    // session A 绑定的 MCP tool 不出现在 session B
}

#[test]
fn thread_a_does_not_see_thread_b_binding() {
    // thread A 绑定的 MCP tool 不出现在 thread B
}
```

---

## Phase 1: 数据模型与存储

### L1 Unit: 类型

```rust
#[test]
fn mcp_tool_scope_serde() {
    let scopes = vec![
        (r#""global""#, McpToolScope::Global),
        (r#""project""#, McpToolScope::Project),
        (r#""session""#, McpToolScope::Session),
        (r#""thread""#, McpToolScope::Thread),
    ];
    for (json, expected) in scopes {
        assert_eq!(serde_json::from_str::<McpToolScope>(json).unwrap(), expected);
    }
}

#[test]
fn mcp_permission_serde() { ... }

#[test]
fn mcp_binding_serde_roundtrip() {
    let binding = McpBinding {
        server_id: "my-server".into(),
        scope: McpToolScope::Session,
        project_path: None,
        session_id: Some("sess-001".into()),
        thread_id: None,
        permissions: vec![McpPermission::Connect, McpPermission::CallTools],
    };
    let json = serde_json::to_string(&binding).unwrap();
    let deserialized: McpBinding = serde_json::from_str(&json).unwrap();
    assert_eq!(binding.server_id, deserialized.server_id);
    assert_eq!(binding.scope, deserialized.scope);
}
```

### L1 Unit: Settings

```rust
#[test]
fn settings_mcp_bindings_roundtrip() {
    let json = r#"{"mcpBindings": [{"serverId": "s1", "scope": "global"}]}"#;
    let settings: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.mcp_bindings.len(), 1);
}

#[test]
fn settings_legacy_mcp_servers_generates_implicit_binding() { ... }

// Session binding storage
#[test]
fn session_binding_create_list_delete() {
    // CRUD → path in ~/.allthecodes/runs/{session_id}/
}

#[test]
fn session_binding_rejects_empty_session_id() { ... }
#[test]
fn session_binding_rejects_empty_thread_id() { ... }
#[test]
fn session_binding_path_isolation() {
    // 不写 ~/.Codex
}
```

---

## Phase 2-3: Runtime Context & 工具过滤

### L1 Unit: McpBindingContext

```rust
#[test]
fn tools_for_context_global_binding() {
    let ctx = McpBindingContext { cwd: "/proj".into(), project_root: None, session_id: "s1".into(), thread_id: "main".into() };
    let tools = mgr.tools_for_context(&ctx);
    // 包含 global 绑定 server 的工具
}

#[test]
fn tools_for_context_project_binding() {
    // project 绑定 + 匹配 project_root → 可见
}

#[test]
fn tools_for_context_project_wrong_project() {
    // project 绑定 + 不匹配 project_root → 不可见
}

#[test]
fn tools_for_context_session_binding() {
    // session 绑定 + 匹配 session_id → 可见
}

#[test]
fn tools_for_context_session_wrong_session() {
    // session 绑定 + 不匹配 session_id → 不可见
}

#[test]
fn tools_for_context_thread_binding() {
    // thread 绑定 + 匹配 thread_id → 可见
}

#[test]
fn tools_for_context_main_thread_default_visibility() {
    // main thread 可见性 = 旧行为兼容
}
```

### L2 Integration

```rust
#[test]
fn engine_startup_filters_mcp_by_context() {
    // full_init 用当前 context 生成 MCP tools
    // 不在无条件下把 mgr.all_tools() 全量并入
}

#[test]
fn engine_mcp_refresh_uses_context() {
    // lifecycle/deps/model_call.rs 的 MCP refresh
    // → tools_for_context(ctx)
}

#[test]
fn agent_spawn_inherits_context() {
    // agent spawn → child thread context
    // → 只看到该 thread 可见的 MCP tools
}

#[test]
fn agent_mcp_servers_field_works() {
    // AgentDefinitionEntry.mcp_servers 实际生效
    // 未声明的 server 不可见
}

#[test]
fn skill_fork_context_no_global_leak() {
    // skill fork 不直接从全局 all_tools() 取 MCP tools
}

// MCP tool wrapper 二次校验
#[test]
fn mcp_wrapper_call_checks_permission() {
    // McpToolWrapper::call() 再次检查 call_tools 权限
}

#[test]
fn mcp_wrapper_call_denies_unauthorized() {
    // 越权直接调用 → 明确错误
}

#[test]
fn builtin_read_only_agent_no_mcp() {
    // Explore 等 read-only agent 默认不获得 MCP tools
    // 除非显式允许
}
```

---

## Phase 5: 控制面与 UX

```rust
#[test]
fn cli_bind_global_creates_binding() { ... }
#[test]
fn cli_bind_session_creates_binding() { ... }
#[test]
fn cli_bind_thread_creates_binding() { ... }
#[test]
fn cli_list_bindings() { ... }
#[test]
fn cli_unbind_removes_binding() { ... }

#[test]
fn ipc_binding_crud_roundtrip() { ... }

#[test]
fn web_binding_create_list_delete() { ... }

#[test]
fn tui_mcp_page_shows_scope() {
    // TUI /mcp 展示 config source + binding scope + permissions
}
```

---

## Phase 6: 回归

```rust
#[test]
fn no_new_warnings() {
    // cargo build --workspace --release 无 warning
}

#[test]
fn old_config_migration_compatible() {
    // 旧 mcpServers 无需迁移即可工作
}

#[test]
fn bindings_do_not_leak_token() {
    // CRUD round-trip 不泄漏 token/env secret
}
```

---

## 运行命令

```bash
cargo test -p allthecodes-mcp --lib
cargo test -p allthecodes-ipc-protocol --lib
cargo test -p allthecodes-engine --lib mcp
cargo test -p allthecodes-commands --lib mcp
cargo test -p allthecodes-web --lib mcp
cargo build --workspace --release
```
