# Record/Replay Engine 集成 TDD 测试计划

> 来源计划: `development/runtime/agent-runtime-execution-record-fields-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: engine 写入 canonical events、execution record、权限决策回填、dashboard/web 消费

## 状态

**已全部实现**（2026-07-02）。本计划作为回归测试清单。

---

## Phase 1: 类型与序列化（已完成回归）

```rust
#[test]
fn execution_record_target_fields_stable() {
    let record = AgentRuntimeExecutionRecord {
        session_id: "s1".into(),
        agent_id: "a1".into(),
        parent_agent_id: None,
        agent_role: Some("build-agent".into()),
        tool: "shell".into(),
        tool_use_id: Some("tu-001".into()),
        command: Some("npm test".into()),
        cwd: Some(PathBuf::from("/proj")),
        exit_code: Some(1),
        stdout_digest: Some("abc123...".into()),
        stderr_digest: Some("def456...".into()),
        retry_count: 0,
        model: Some("claude-sonnet-4".into()),
        fallback_used: false,
        permission_decision: Some("allowed_by_policy".into()),
        duration_ms: Some(1500),
        had_error: true,
        schema_version: 1,
    };
    let json = serde_json::to_string(&record).unwrap();
    // 12 个目标键必须存在
    for key in &["session_id", "agent_role", "tool", "command", "cwd", "exit_code",
                 "stdout_digest", "stderr_digest", "retry_count", "model",
                 "fallback_used", "permission_decision"] {
        assert!(json.contains(key), "missing key: {}", key);
    }
}

#[test]
fn execution_record_nullable_fields() {
    // 非 shell 工具 → 专属字段 null
}

#[test]
fn agent_event_execution_record_variant_serde() { ... }
```

---

## Phase 2-3: Shell 结果/SHA256 digest

### L2 Integration

```rust
#[test]
fn bash_runtime_outputs_execution_record() {
    // shell 命令执行完成 → runtime 产生 execution record
    // 含 command, cwd, exit_code, stdout_digest, stderr_digest
}

#[test]
fn bash_command_failed_exit_code_preserved() {
    // exit code 1 → had_error = true, exit_code = 1
}

#[test]
fn bash_runtime_stdout_digest_sha256() {
    // digest 算法 = SHA-256 hex
}

#[test]
fn bash_runtime_stdout_empty_digest() {
    // 空 stdout → digest 仍是合法 SHA-256
}

#[test]
fn bash_runtime_stdout_truncation() {
    // 超长 stdout → digest 来自原始输出，不是截断后
}

#[test]
fn non_shell_tool_produces_record_with_null_fields() {
    // Read/Edit 工具 → command/cwd/exit_code/stdout_digest = null
}

#[test]
fn tool_name_normalized_to_shell() {
    // Bash/PowerShell → tool = "shell"
}
```

---

## Phase 4: 权限决策回填

### L2 Integration

```rust
#[test]
fn permission_allowed_by_policy() {
    // policy allow → permission_decision = "allowed_by_policy"
}

#[test]
fn permission_allowed_by_user() {
    // 用户 allow → "allowed_by_user"
}

#[test]
fn permission_allowed_by_hook() {
    // hook allow → "allowed_by_hook"
}

#[test]
fn permission_denied_by_policy() {
    // policy deny → "denied_by_policy"
}

#[test]
fn permission_denied_by_user() {
    // 用户 deny → "denied_by_user"
}

#[test]
fn permission_denied_by_hook() {
    // hook deny → "denied_by_hook"
}

#[test]
fn permission_not_required() {
    // 无需权限 → "not_required"
}
```

---

## Phase 5: 事件输出 & 消费者兼容

```rust
#[test]
fn headless_jsonl_contains_execution_record() {
    // headless 输出包含 execution_record kind
}

#[test]
fn dashboard_ndjson_has_execution_record() {
    // dashboard NDJSON 增加 execution_record event
}

#[test]
fn normalized_ipc_maps_execution_record() {
    // normalized IPC 把 AgentEvent::ExecutionRecord 映射为
    // agent_runtime/execution_record payload
}

#[test]
fn web_ipc_bridge_receives_execution_record() {
    // web/API IPC session hub 接入 agent runtime event channel
    // ExecutionRecord → runtime queue → event log → WS replay
}

#[test]
fn tui_ignores_execution_record() {
    // Rust TUI 不展示 execution record（兼容降级）
}
```

---

## Phase 6: Context 贯穿

```rust
#[test]
fn session_id_in_record_matches_engine() {
    // record.session_id == QueryEngine.session_id
}

#[test]
fn agent_role_from_agent_type() {
    // AgentContext.agent_type → record.agent_role
}

#[test]
fn model_from_query_loop() {
    // query loop 记录本轮模型 → record.model
}

#[test]
fn fallback_used_true_after_fallback() {
    // fallback 后 → fallback_used = true
}

#[test]
fn retry_count_tracks_query_retry() {
    // retry count > 0
}

#[test]
fn parent_agent_id_from_agent_context() {
    // 子 agent → parent_agent_id 非空
}
```

---

## 运行命令

```bash
cargo test -p allthecodes-types agent_runtime_record
cargo test -p allthecodes-engine execution_record
cargo test -p allthecodes-engine permission_decision
cargo test -p allthecodes-engine bash_record
cargo test -p allthecodes-observability
cargo test -p allthecodes-web execution_record
cargo build --workspace --release
```
