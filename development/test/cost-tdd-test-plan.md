# Session Cost Log TDD 测试计划

> 来源计划: `development/cost/session-cost-log-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: pricing, cost event, 聚合 helper, IPC/UI usage 字段

## 状态

Phase 0（基线）已完成；Phase 1（PricingMatch 结构化）和 Phase 2（cost event 写入）是当前阶段目标。

---

## Phase 0：基线与契约测试（已完成验证）

```rust
// 当前成本计算行为固定
#[test]
fn cost_command_aggregates_from_messages() { ... }
#[test]
fn extra_usage_command_breakdown() { ... }
#[test]
fn insights_cost_from_historical_sessions() { ... }
#[test]
fn web_usage_events_from_session() { ... }
#[test]
fn cache_read_create_pricing() { ... }        // cache read 0.1x, cache creation 1.25x
#[test]
fn reasoning_tokens_not_separately_priced() { ... }
#[test]
fn unknown_model_cost_is_zero() { ... }
#[test]
fn env_override_pricing() { ... }
#[test]
fn assistant_without_usage_defaults_zero() { ... }
```

---

## Phase 1：定价结果结构化

### L1 Unit: PricingMatch

```rust
#[test]
fn pricing_match_builtin_model() {
    let (row, source) = get_pricing("claude-sonnet-4-20250514").unwrap();
    assert_eq!(source, PricingSource::Builtin);
    assert_eq!(row.matched_key, Some("claude-sonnet-4".into()));
}

#[test]
fn pricing_match_env_override() {
    // env 覆盖 → source = EnvOverride
}

#[test]
fn pricing_match_unknown_model() {
    let result = get_pricing("unknown-model-v999");
    assert_eq!(result.source, PricingSource::Unknown);
    assert!(result.row.is_none());
}

#[test]
fn pricing_match_cache_multiplier() {
    // cache_read_multiplier = 0.1, cache_creation_multiplier = 1.25
}

#[test]
fn cost_breakdown_total_matches_old_cost() {
    // CostBreakdown.total() == 旧 calculate_cost()
}
```

---

## Phase 2：写入 cost event

### L1 Unit: Cost event schema

```rust
#[test]
fn cost_event_serde_roundtrip() {
    let event = CostRecordedEvent {
        schema_version: 1,
        session_id: "...".into(),
        submit_id: "...".into(),
        turn_id: Some("turn-001".into()),
        request_id: "...".into(),
        message_id: "...".into(),
        provider: "anthropic".into(),
        backend: "native".into(),
        model: "claude-sonnet-4".into(),
        pricing: PricingRow { source: PricingSource::Builtin, ... },
        usage: Usage { input_tokens: 1000, output_tokens: 250, .. },
        cost_usd: 0.00675,
        stop_reason: Some("end_turn".into()),
        is_retry: false,
        attempt: 1,
        backfilled: false,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: CostRecordedEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event.cost_usd, deserialized.cost_usd);
}
```

### L2 Integration

```rust
#[test]
fn assistant_message_finalization_emits_cost_event() {
    // stream_handler 最终化 assistant message 后
    // → 写入 audit event kind=CostRecorded
}

#[test]
fn cost_event_linked_to_request_snapshot() {
    // cost event 的 request_id 与 request snapshot 一致
}

#[test]
fn cost_event_emit_failure_does_not_block() {
    // emit 失败只 warning → conversation 不中断
}

#[test]
fn multiple_api_calls_multiple_cost_events() {
    // 多次 API call → 多条 cost event
}

#[test]
fn cost_event_after_fallback_model() {
    // fallback 后模型变化 → cost event model = fallback model
}
```

---

## Phase 3：共享聚合层

### L1 Unit: CostLedger

```rust
#[test]
fn cost_ledger_load_events() {
    // 从 events.ndjson 读取 cost events
}

#[test]
fn cost_ledger_backfill_from_messages() {
    // 无 cost event → backfill from assistant messages
    // → backfilled = true
}

#[test]
fn cost_ledger_aggregate_summary() {
    let events = vec![cost_event_a(), cost_event_b()];
    let summary = aggregate_cost_events(&events);
    assert_eq!(summary.total_cost_usd, 0.01);
    assert_eq!(summary.api_calls, 2);
    assert_eq!(summary.unknown_pricing_count, 0);
}

#[test]
fn cost_ledger_summary_with_unknown_pricing() {
    // unknown 模型的 cost event → unknown_pricing_count > 0
}

#[test]
fn cost_ledger_summary_with_backfilled() {
    // backfilled 事件 → backfilled_count > 0
}

// 各入口迁移后一致性
#[test]
fn cost_command_uses_ledger() {
    // /cost 迁移到 cost_ledger → 与旧聚合一致
}
#[test]
fn extra_usage_uses_ledger() { ... }
#[test]
fn insights_uses_ledger() { ... }
#[test]
fn web_usage_api_uses_ledger() { ... }
```

---

## Phase 4：UI/IPC/StatusLine 字段补齐

```rust
#[test]
fn usage_update_includes_cache_reasoning() {
    let update = UsageUpdate {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: Some(200),
        cache_creation_tokens: Some(100),
        reasoning_output_tokens: Some(50),
        api_calls: 3,
        kind: UpdateKind::Cumulative,
    };
    let json = serde_json::to_string(&update).unwrap();
    assert!(json.contains("cache_read_tokens"));
    assert!(json.contains("api_calls"));
}

#[test]
fn usage_update_clarifies_cumulative_or_delta() {
    // kind 字段区分 cumulative / delta
}

#[test]
fn status_line_cost_has_unknown_backfilled_count() {
    // payload 包含 unknown_count / backfilled_count
}
```

---

## Phase 5：历史数据迁移

```rust
#[test]
fn lazy_backfill_old_session() {
    // 旧 session → backfill cost events → backfilled=true
}

#[test]
fn lazy_backfill_no_file_change() {
    // 不改写原始 session JSON
}

#[test]
fn web_dashboard_warns_for_backfilled() {
    // usage API 返回 warnings 字段含 backfilled count
}
```

---

## 运行命令

```bash
# Phase 0-1
cargo test -p allthecodes-types models::pricing
cargo test -p allthecodes-api api::pricing
cargo test -p allthecodes-commands cost extra_usage insights

# Phase 2-3
cargo test -p allthecodes-observability
cargo test -p allthecodes-engine lifecycle::submit_message
cargo test -p allthecodes-services cost_ledger

# Phase 4-5
cargo test -p allthecodes-ipc-protocol
cargo test -p allthecodes-web handlers::usage

# 全量
cargo build --workspace --release
```
