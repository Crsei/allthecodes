# 代码拆分与复杂度优化 TDD 测试计划

> 来源计划: `development/code-split/codebase-optimization-plan-2026-07-03.md`
> 生成日期: 2026-07-03
> 验证原则: 先补测试护栏，再拆分；禁止顺手改行为

## 整体策略

所有 P0 工作流实施前必须先完成 regression coverage（Phase 0 护栏），然后在拆分过程中逐项验证单测 + 集成测是否通过。每项改动需满足**验收标准**才能算完成。

### 测试层级

| 层级 | 位置 | 覆盖目标 |
|------|------|----------|
| L1 Unit | `crates/*/src/` 模块内 `#[cfg(test)]` | 函数级行为、状态机转换、schema roundtrip |
| L2 Integration | `crates/*/tests/` | crate 边界行为、错误恢复、边界条件 |
| L3 E2E | `tests/pty_tui_e2e/` | CLI/交互式完整路径 |
| L4 Golden | `tests/fixtures/` | 输出快照 + 临界输入黄金文件 |

---

## CS-001: Tool execution pipeline

> 目标: `execute_tool_impl` 从 ~1272 行降为只编排 pipeline（<180 行），拆分为 validate / run_pre_hooks / resolve_permission / execute / run_post_hooks / emit_records

### Phase 0 护栏测试（前置，不改行为）

```rust
// 1. 基线：现有执行路径不变
#[test]
fn execute_tool_impl_baseline_bash_allow() {
    // 用已知工具输入，断言：输出与 current main 一致
}

#[test]
fn execute_tool_impl_baseline_bash_deny() { ... }
#[test]
fn execute_tool_impl_baseline_readonly() { ... }

// 2. Permission prompt 响应不变
#[test]
fn execute_tool_impl_permission_allow_response() {
    // auto review allow → 执行
}
#[test]
fn execute_tool_impl_permission_deny_response() {
    // permission deny → 跳过执行
}

// 3. Hook 路径不变
#[test]
fn execute_tool_impl_pre_hook_modify_input() {
    // PreToolUse hook 修改 input → 后续用改后 input
}
#[test]
fn execute_tool_impl_pre_hook_deny() { ... }

// 4. Sandbox preflight 不变
#[test]
fn execute_tool_impl_sandbox_preflight() { ... }
```

### L1 核心测试

```rust
// 1. ToolExecutionPlan 构造
#[test]
fn tool_execution_plan_constructs_from_raw_input() {
    // 从 ToolUse block → ToolExecutionPlan
    // 含: original_input, permission_subject, sandbox_requirement
}

#[test]
fn tool_execution_plan_post_hook_update() {
    // hook 修改后 input → plan.hook_adjusted_input 更新
}

// 2. ToolExecutionPipeline 各阶段
#[test]
fn pipeline_validate_rejects_missing_params() { ... }
#[test]
fn pipeline_run_pre_hooks_runs_all() { ... }
#[test]
fn pipeline_resolve_permission_auto_allow() { ... }
#[test]
fn pipeline_resolve_permission_ask_user() { ... }
#[test]
fn pipeline_execute_tool_call() { ... }
#[test]
fn pipeline_run_post_hooks_after_success() { ... }
#[test]
fn pipeline_run_post_hooks_after_error() { ... }       // 错误后 hook 仍执行
#[test]
fn pipeline_emit_records_audit_trail() { ... }          // audit decision id 一致

// 3. 编排函数
#[test]
fn execute_tool_impl_orchestrates_pipeline() {
    // 验证编排函数调用所有 pipeline stage 且 <180 行
}
```

### L2 集成测试

```rust
#[test]
fn pipeline_full_flow_bash_allow() {
    // 完整: validate → pre_hook → permission → execute → post_hook → emit
    // 断言: 每阶段有记录, final record 含 permission decision id
}

#[test]
fn pipeline_full_flow_pre_hook_denies() {
    // pre_hook deny → 不 resolve_permission → 不 execute → post_hook 执行
}

#[test]
fn pipeline_full_flow_permission_timeout() {
    // permission timeout → 不 execute
}
```

---

## CS-002: Hook full implementation matrix

> 目标: 移除 structural stub, 每个 hook type 有 schema test 和 error/timeout test

### L1 单元测试

```rust
// 1. 事件矩阵测试
#[test]
fn hook_event_session_start_payload_schema() {
    // SessionStart 的 payload 符合定义
}
#[test]
fn hook_event_user_prompt_submit_schema() { ... }
#[test]
fn hook_event_pre_tool_use_schema() { ... }
#[test]
fn hook_event_post_tool_use_schema() { ... }
#[test]
fn hook_event_stop_schema() { ... }
#[test]
fn hook_event_stop_failure_schema() { ... }
#[test]
fn hook_event_agent_hooks_schema() { ... }
#[test]
fn hook_event_file_watcher_schema() { ... }

// 2. 可修改字段
#[test]
fn hook_modifiable_fields_pre_tool_use() {
    // PreToolUse 可修改 input params
}
#[test]
fn hook_modifiable_fields_user_prompt_submit() {
    // UserPromptSubmit 可修改 message
}

// 3. 阻断行为
#[test]
fn hook_can_block_execution_pre_tool_use() {
    // hook 返回 deny → 执行阻断
}
#[test]
fn hook_cannot_block_output_post_tool_use() {
    // PostToolUse 不能阻断（只是观察）
}

// 4. Timeout & error
#[test]
fn hook_timeout_does_not_block_main_path() {
    // hook timeout → warn + continue，不 panic
}
#[test]
fn hook_error_does_not_panic() {
    // hook error → logged warn，不 panic
}

// 5. 无 stub
#[test]
fn no_structural_stub_in_production_path() {
    // 确认每个 hook type 的生产路径不是 placeholder 或 "todo"
}
```

### L2 集成测试

```rust
#[test]
fn hook_chain_multiple_hooks_all_succeed() { ... }
#[test]
fn hook_chain_first_fails_remaining_still_run() { ... }
#[test]
fn hook_chain_timeout_on_third_middle() { ... }
```

---

## CS-003: Shell policy decision

> 目标: 后端/UI/sandbox 使用单一 `ShellPolicyDecision`，不再多处解释 shell 风险

### L1 核心测试

```rust
// 1. 分类器
#[test]
fn shell_policy_bash_read_only() {
    // "ls -la" → read_only
}
#[test]
fn shell_policy_bash_build() {
    // "cargo build" → build
}
#[test]
fn shell_policy_bash_mutate() {
    // "rm file" → mutate
}
#[test]
fn shell_policy_bash_destructive() {
    // "rm -rf /" → destructive
    // "dd if=/dev/zero of=/dev/sda" → destructive
}
#[test]
fn shell_policy_bash_deploy() {
    // "kubectl apply" → deploy
}
#[test]
fn shell_policy_bash_secret() {
    // "export API_KEY=..." → secret
}
#[test]
fn shell_policy_bash_parse_failure() {
    // 不完整/复杂语法 → parser_failure
}

// 2. PowerShell classifier
#[test]
fn shell_policy_powershell_read_only() { ... }
#[test]
fn shell_policy_powershell_destructive() { ... }

// 3. UI 消费
#[test]
fn shell_policy_decision_display_risk_label() {
    // 每个 variant 显示正确的 human-readable risk label
}

// 4. Permission 消费
#[test]
fn shell_policy_decision_maps_to_permission_subject() { ... }

// 5. Sandbox preflight 消费
#[test]
fn shell_policy_decision_maps_to_sandbox_level() { ... }
```

### L2 Golden Tests

使用黄金文件测试文件保存预期决策，覆盖:

- `tests/fixtures/shell-policy/bash/read_only.txt`
- `tests/fixtures/shell-policy/bash/build.txt`
- `tests/fixtures/shell-policy/bash/mutate.txt`
- `tests/fixtures/shell-policy/bash/destructive.txt`
- `tests/fixtures/shell-policy/bash/deploy.txt`
- `tests/fixtures/shell-policy/bash/secret.txt`
- `tests/fixtures/shell-policy/powershell/read_only.txt`
- `tests/fixtures/shell-policy/powershell/destructive.txt`

```rust
#[test_case("tests/fixtures/shell-policy/bash/read_only.txt")]
#[test_case("tests/fixtures/shell-policy/bash/destructive.txt")]
// ...
fn shell_policy_golden_test(file_path: &str) {
    let input = std::fs::read_to_string(file_path).unwrap();
    let decision = classify_shell_command(&input, ShellKind::Bash);
    insta::assert_debug_snapshot!(decision);
}
```

---

## CS-004: Query turn state machine

> 目标: query 主循环 < 250 行，可局部测试每个 turn state

### L1 单元测试

```rust
// 1. 状态转换
#[test]
fn query_turn_state_transitions() {
    let mut sm = QueryTurnStateMachine::new();
    assert_eq!(sm.state(), QueryTurnState::Preparing);

    sm.transition(QueryTurnEvent::StreamStarted);
    assert_eq!(sm.state(), QueryTurnState::Streaming);

    sm.transition(QueryTurnEvent::AssistantContentReceived);
    assert_eq!(sm.state(), QueryTurnState::AssistantReceived);

    sm.transition(QueryTurnEvent::ToolExecutionStarted);
    assert_eq!(sm.state(), QueryTurnState::ExecutingTools);

    sm.transition(QueryTurnEvent::AllToolsCompleted);
    assert_eq!(sm.state(), QueryTurnState::TerminalCheck);

    sm.transition(QueryTurnEvent::ShouldContinue);
    assert_eq!(sm.state(), QueryTurnState::Continuing);
}

#[test]
fn query_turn_state_abort_before_stream() { ... }
#[test]
fn query_turn_state_abort_during_tool() { ... }
#[test]
fn query_turn_state_abort_during_stream() { ... }

// 2. 非法转换
#[test]
#[should_panic(expected = "invalid transition")]
fn query_turn_state_invalid_transition() {
    // ExecutingTools → Streaming 非法
}

// 3. Recovery 模块
#[test]
fn recovery_prompt_too_long() { ... }
#[test]
fn recovery_max_output_tokens() { ... }
#[test]
fn recovery_fallback_model_retry() { ... }
#[test]
fn recovery_stream_timeout() { ... }
```

### L2 集成测试

```rust
#[test]
fn query_turn_full_cycle() {
    // 开始 → 流 → assistant → 工具 → 终端检查 → 结束
    // 验证每步事件
}

#[test]
fn query_turn_aborts_cleanly() { ... }
#[test]
fn query_turn_max_tokens_triggers_recovery() { ... }
#[test]
fn query_turn_tool_error_continues() { ... }
#[test]
fn query_turn_stop_hook_halts() { ... }
```

---

## CS-005: Submit transaction

> 目标: submit side effects（append message / persist session / update usage / emit event / flush stream）收敛到 `SubmitTransaction`

### L1 单元测试

```rust
// 1. Transaction 提交
#[test]
fn submit_transaction_appends_message() {
    let mut tx = SubmitTransaction::new(messages);
    tx.append_message(user_message());
    assert_eq!(tx.messages().len(), 1);
}

#[test]
fn submit_transaction_persists_session() {
    // mock 后验证 persist 被调用
}

#[test]
fn submit_transaction_updates_usage() { ... }
#[test]
fn submit_transaction_emits_event() { ... }
#[test]
fn submit_transaction_flushes_stream() { ... }

// 2. 原子性
#[test]
fn submit_transaction_all_or_nothing() {
    // 任意一个 side effect 失败 → 整体回滚
}

#[test]
fn submit_transaction_partial_failure_logs_error() {
    // 持久化失败但记录日志，不 panic
}
```

---

## CS-006: Typed permission UI payload

> 目标: UI 不再解析 tool JSON, 未知标签 fail closed

### L1 单元测试

```rust
// 1. Typed payload
#[test]
fn permission_subject_serialization() {
    let subject = PermissionSubject {
        tool_name: "Bash".into(),
        risk: RiskClassification::Destructive,
        allowed_responses: vec![Allow, Deny],
    };
    let json = serde_json::to_string(&subject).unwrap();
    assert!(json.contains("destructive"));
}

// 2. UI 只渲染 typed payload
#[test]
fn ui_permission_renders_from_typed_payload() {
    let payload = TypedPermissionPayload { ... };
    let rendered = render_permission_dialog(&payload);
    assert!(rendered.contains("Bash"));
    assert!(rendered.contains("destructive"));
}

// 3. Fail closed
#[test]
fn choice_for_label_unknown_returns_deny() {
    let result = choice_for_label("nonexistent_btn");
    assert_eq!(result, PermissionResponse::Deny);
}

#[test]
fn choice_for_label_unknown_cancel() { ... }
#[test]
fn choice_for_label_unknown_permission_kind() { ... }

// 4. 不再调用 shell heuristic
#[test]
fn ui_permission_no_shell_heuristic_call() {
    // 验证 UI permission 层不调用 classify_shell_command
}
```

### L2 Snapshot Tests

```rust
#[test]
fn permission_dialog_bash_destructive_snapshot() {
    // 设置 typed payload → Bash destructive
    // insta::assert_display_snapshot!(rendered);
}

#[test]
fn permission_dialog_apply_patch_snapshot() { ... }
#[test]
fn permission_dialog_mcp_snapshot() { ... }
#[test]
fn permission_dialog_dynamic_workflow_snapshot() { ... }
```

---

## P1 工作流测试概览（展开简写）

### CS-007: API operation registry

```rust
#[test]
fn api_registry_register_and_dispatch() { ... }   // 一个注册点生成 metadata + binding
#[test]
fn api_registry_no_duplicate_registration() { ... }
#[test]
fn api_registry_migration_state_tracking() { ... }
```

### CS-008: Session mutation service

```rust
#[test]
fn session_mutation_resume() { ... }                   // 不依赖 HTTP handler
#[test]
fn session_mutation_branch() { ... }
#[test]
fn session_mutation_rollback_preview() { ... }
#[test]
fn session_mutation_rollback_apply() { ... }
#[test]
fn session_mutation_delete() { ... }
#[test]
fn ws_disconnect_does_not_corrupt_session() { ... }    // 断连/重连集成测试
#[test]
fn ws_pending_permission_survives_reconnect() { ... }
```

### CS-009: Record-replay truth source

```rust
#[test]
fn resume_corrupt_jsonl_recoverable() { ... }       // resume 路径
#[test]
fn resume_missing_index_fallback() { ... }
#[test]
fn resume_legacy_only_no_record_replay() { ... }
#[test]
fn resume_mixed_storage_no_data_loss() { ... }
#[test]
fn storage_drift_detected() { ... }                 // SQLite vs JSONL 不一致 → 报告
#[test]
fn new_session_no_legacy_json() { ... }             // 兼容开关关闭时不写 legacy JSON
```

### CS-010: Runtime capability registry

```rust
#[test]
fn capability_registry_hidden_tool() { ... }           // hidden/deferred/MCP 同一种 subject
#[test]
fn capability_registry_permission_subject_mcp() { ... }
#[test]
fn capability_registry_discovery() { ... }             // 命令展示/解析/执行来自同一 metadata
#[test]
fn execute_extra_tool_uses_registry() { ... }          // 不再绕 CORE_TOOLS
```

### CS-011: FS capability service

```rust
#[test]
fn fs_capability_path_traversal_rejected() { ... }     // path traversal → denied
#[test]
fn fs_capability_symlink_outside_root_rejected() { ... }
#[test]
fn fs_capability_missing_parent_creates() { ... }
#[test]
fn fs_capability_hash_conflict_detected() { ... }
#[test]
fn handlers_files_only_dto_mapping() { ... }           // handler 只保留 DTO/HTTP
```

### CS-012 ~ CS-014: TUI domain stores / overlay dispatcher / message view-model

```rust
// CS-012
#[test]
fn app_fields_reduced_below_40() { ... }                // App 直接字段 < 40
#[test]
fn chat_store_manages_conversation() { ... }
#[test]
fn overlay_store_manages_z_order() { ... }
#[test]
fn input_store_manages_key_events() { ... }

// CS-013
#[test]
fn handle_key_event_below_160_lines() { ... }           // handle_key_event < 160 行
#[test]
fn overlay_focus_explicit_owner() { ... }               // 显式 focus owner
#[test]
fn overlay_z_order_render_input_consistent() { ... }    // z-order/render/input 同源

// CS-014
#[test]
fn message_view_model_normalize_once() { ... }          // normalize once, render 消费
#[test]
fn renderable_message_used_by_copy_and_scroll() { ... } // copy/history/virtual scroll 用同一 projection
```

### CS-015: Agent/MCP typed surface state

```rust
#[test]
fn no_magic_offset_100_in_mcp_surface() { ... }
#[test]
fn agent_wizard_step_enum_used() { ... }                // enum 替代硬编码 step 判断
#[test]
fn mcp_view_variants_exhaustive() { ... }               // 覆盖所有 MCP view 模式
```

---

## 执行顺序与交付物

### Phase 0: 护栏先行（全量 P0）

| 顺序 | 测试文件 | 测试数 | 退出条件 |
|------|----------|--------|----------|
| 1. | `crates/allthecodes-engine/tests/tool_execution_baseline.rs` | ~8 | 所有 P0 现有行为有 regression coverage |
| 2. | `crates/allthecodes-engine/tests/query_lifecycle_baseline.rs` | ~5 | abort/max token/fallback 已覆盖 |
| 3. | `crates/allthecodes-session/tests/session_replay_baseline.rs` | ~5 | JSONL/legacy/corrupt 已覆盖 |
| 4. | `crates/allthecodes/tests/ui_permission_fail_closed.rs` | ~3 | UI fail closed |

### Phase 1: 安全边界收敛

| 顺序 | 测试文件 | 测试数 | 退出条件 |
|------|----------|--------|----------|
| 1. | `crates/allthecodes-engine/tests/shell_policy.rs` | ~10 | golden + unit |
| 2. | `crates/allthecodes-engine/tests/tool_execution_pipeline.rs` | ~10 | pipeline stage 全部覆盖 |
| 3. | `crates/allthecodes/tests/permission_typed_payload.rs` | ~6 | UI 不再解析 shell |
| 4. | `crates/allthecodes-tools/tests/fs_capability.rs` | ~5 | 所有写入路径持 capability |

### Phase 2: 生命周期拆分

| 顺序 | 测试文件 | 测试数 | 退出条件 |
|------|----------|--------|----------|
| 1. | `crates/allthecodes-engine/tests/query_turn_state.rs` | ~8 | 主循环 < 250 行 |
| 2. | `crates/allthecodes-engine/tests/submit_transaction.rs` | ~5 | submit < 一半 |
| 3. | `crates/allthecodes-web/tests/session_mutation.rs` | ~6 | service-level tests |

### Phase 3: Registry 和协议治理

| 顺序 | 测试文件 | 测试数 |
|------|----------|--------|
| 1. | `crates/allthecodes-web/tests/api_registry.rs` | ~3 |
| 2. | `crates/allthecodes-commands/tests/capability_registry.rs` | ~4 |

### Phase 4: TUI 状态解耦

| 顺序 | 测试文件 | 测试数 |
|------|----------|--------|
| 1. | `crates/allthecodes/tests/tui_domain_stores.rs` | ~4 |
| 2. | `crates/allthecodes/tests/tui_overlay_dispatcher.rs` | ~3 |
| 3. | `crates/allthecodes/tests/tui_message_viewmodel.rs` | ~2 |
| 4. | `crates/allthecodes/tests/tui_surface_state.rs` | ~3 |

---

## 运行命令

```bash
# Phase 0 护栏
cargo test -p allthecodes-engine tool_execution_baseline
cargo test -p allthecodes-engine query_lifecycle_baseline
cargo test -p allthecodes-session session_replay_baseline
cargo test -p allthecodes ui_permission_fail_closed

# 全量
cargo test --workspace -- code_split
cargo test --workspace -- shell_policy
cargo test --workspace -- query_turn_state
```

## 禁止事项

1. 不删除现有 behavior test 仅因为测试框架重构
2. 不在 UI 层新增 shell heuristic 测试（只能删不能加）
3. 所有 snapshot test 不依赖随机值或时间戳
4. 不在 handler 中测试 service 行为（只测 DTO/HTTP mapping）
