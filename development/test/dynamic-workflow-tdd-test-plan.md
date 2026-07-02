# DynamicWorkflow TDD 测试计划

> 来源计划: `development/dynamic_workflow/01-arch-plan.zh.md`
> 生成日期: 2026-07-03
> 覆盖范围: `crates/allthecodes-tools/src/workflow_dynamic/`

## 状态

Phase 1（JSON DAG）待实现。测试计划覆盖 Phase 1（基线 MVP）和 Phase 2（动态 fan-out）。

---

## Phase 1: JSON DAG 编排

### L1 Unit: 类型与 schema

```rust
#[test]
fn dynamic_workflow_action_serde() {
    let action = DynamicWorkflowAction {
        name: "test".into(),
        plan: serde_json::json!({
            "stages": [{
                "id": "a1",
                "kind": "agent",
                "prompt": "review module A",
                "subagent_type": "general-purpose",
                "depends_on": []
            }]
        }),
        subagent_type: None,
        max_concurrency: 8,
    };
    let json = serde_json::to_string(&action).unwrap();
    let deserialized: DynamicWorkflowAction = serde_json::from_str(&json).unwrap();
    assert_eq!(action.name, deserialized.name);
}

// 各阶段 kind roundtrip
#[test]
fn workflow_stage_kind_agent() { ... }
#[test]
fn workflow_stage_kind_map() { ... }
#[test]
fn workflow_stage_kind_reduce() { ... }
#[test]
fn workflow_stage_kind_pipeline() { ... }

// Observation
#[test]
fn dynamic_workflow_observation_serde() { ... }
```

### L1 Unit: DAG 校验

```rust
#[test]
fn validate_dag_no_deps_topo_sort_returns_all() {
    let stages = vec![stage("a1", vec![]), stage("b1", vec![])];
    let sorted = validate_workflow_plan(&stages).unwrap();
    assert_eq!(sorted.len(), 2);
}

#[test]
fn validate_dag_linear_deps() {
    let stages = vec![
        stage("a1", vec![]),
        stage("b1", vec!["a1"]),
        stage("c1", vec!["b1"]),
    ];
    let sorted = validate_workflow_plan(&stages).unwrap();
    // a1 → b1 → c1
    assert!(sorted[0].id == "a1");
}

#[test]
fn validate_dag_fan_out() { ... }
#[test]
fn validate_dag_rejects_cycle() {
    let stages = vec![
        stage("a1", vec!["b1"]),
        stage("b1", vec!["a1"]),
    ];
    let result = validate_workflow_plan(&stages);
    assert!(result.is_err());
}

#[test]
fn validate_dag_rejects_missing_dep() {
    let stages = vec![stage("a1", vec!["missing"])];
    let result = validate_workflow_plan(&stages);
    assert!(result.is_err());
}

#[test]
fn validate_dag_rejects_empty_stages() { ... }
#[test]
fn validate_dag_rejects_duplicate_ids() { ... }
```

### L1 Unit: WorkflowContext

```rust
#[test]
fn context_run_agent_returns_result() {
    let ctx = WorkflowContext::new(max_concurrency: 8);
    let result = ctx.run_agent("test prompt", "general-purpose").await;
    assert!(result.is_ok());
}

#[test]
fn context_map_agents_all_complete() {
    let ctx = WorkflowContext::new(max_concurrency: 4);
    let results = ctx.map_agents(
        vec!["a", "b", "c"],
        "Audit {item}",
        "general-purpose",
        4,
    ).await;
    assert_eq!(results.len(), 3);
}

#[test]
fn context_map_agents_respects_concurrency() {
    // 最大并发 ≤ max_concurrency
}

#[test]
fn context_reduce_agent_synthesizes() { ... }

#[test]
fn context_results_accessible_by_id() {
    // 完成 stage 后 results[id] = output
}

#[test]
fn context_result_truncated_if_too_large() {
    // > MAX_REDUCE_INPUT_CHARS → truncated
}

#[test]
fn context_run_agent_permission_checked() {
    // action=start → Ask 权限
}
```

### L2 Integration: 完整 DAG 执行

```rust
#[test]
fn dynamic_workflow_execute_linear() {
    // stages: [a1: agent, b1: agent (depends_on: a1), c1: reduce (depends_on: b1)]
    // → a1 → b1 → c1
}

#[test]
fn dynamic_workflow_execute_fan_out() {
    // stages: [a1: agent, b1: agent, c1: reduce (depends_on: [a1, b1])]
    // → a1, b1 并发 → c1
}

#[test]
fn dynamic_workflow_execute_error_in_stage() {
    // 一个 stage 出错 → 后续依赖它的 stages 跳过
    // 不依赖的 stages 继续执行
}

#[test]
fn dynamic_workflow_execute_cancel() {
    // action=cancel → 正在运行的 agents 停止
}

#[test]
fn dynamic_workflow_execute_empty() {
    // 空 stages → 返回 error
}

#[test]
fn dynamic_workflow_permission_start_asks() {
    // start → permission asked → allowed → 执行
}

#[test]
fn dynamic_workflow_permission_deined() {
    // denied → 不执行
}
```

### Phase 1 验收

- [x] JSON DAG 定义 → 拓扑排序 → 执行
- [x] 线性/扇出/扇入 DAG
- [x] 有环/缺失 dep 校验错误
- [x] run_agent / map_agents / reduce_agent 并发控制
- [x] 结果可访问、超长截断
- [x] 工具走权限系统（Ask on start）

---

## Phase 2: 动态编排

```rust
#[test]
fn pipeline_stages_no_barrier() {
    // pipeline(items, stageA, stageB)
    // item1.stageA → item1.stageB 与 item2.stageA 并发
}

#[test]
fn map_items_from_prior_stage_result() {
    // map 的 items 引用前序 stage 结果
}

#[test]
fn dynamic_error_aggregation() {
    // 多个 agent 失败 → 聚合错误（ExceptionGroup 风格）
}

#[test]
fn agent_timeout_retry_configured() {
    // 超时/重试配置
}
```

---

## 运行命令

```bash
# Phase 1
cargo test -p allthecodes-tools workflow_dynamic
cargo test -p allthecodes-tools -- dynamic_workflow

# 全量
cargo build --workspace --release
```
