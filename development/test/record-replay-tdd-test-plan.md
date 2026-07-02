# Record/Replay TDD 测试计划

> 来源计划: `development/record-replay/implementation-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: `crates/allthecodes-session/src/record_replay/` 及 engine/web 集成

## 整体策略

覆盖 Phase 0-7 所有阶段。按 PR 切分递进验证，最小可交付版本（Phase 0-4）是第一阶段硬目标。

### 测试层级

| 层级 | 位置 | 覆盖目标 |
|------|------|----------|
| L1 Unit | `crates/allthecodes-session/src/record_replay/*.rs` | types serde、policy、recorder、reader、reconstruct |
| L2 Integration | `crates/allthecodes-session/tests/` | CLI resume、session switch、migration |
| L3 E2E | `tests/pty_tui_e2e/` | CLI new → write → shutdown → resume 完整路径 |
| L4 Golden | `tests/fixtures/record-replay/` | 合法/corrupt 的 rollout JSONL 黄金文件 |

---

## Phase 0: Baseline fixtures & behavior locking

> 验收: `cargo test -p allthecodes-session` 全通过

```rust
// 1. 现有行为锁定
#[test]
fn storage_load_session_fixture_roundtrip() {
    // 从 fixture 加载 session JSON → 序列化 → 与 fixture 一致
}

#[test]
fn resume_session_baseline() {
    // 从已知 fixture 路径恢复 → 消息数正确
}

#[test]
fn sqlite_projection_matches_session_json() {
    // 同一条 session 的 SQLite 投影与 session JSON 投影一致
}

// 2. SerializableMessage 反序列化缺口
#[test]
fn serializable_message_deserialize_user_message() { ... }
#[test]
fn serializable_message_deserialize_assistant_message() { ... }
#[test]
fn serializable_message_deserialize_system_message() { ... }
#[test]
fn serializable_message_deserialize_progress() {
    // 当前反序列化可能不完整 → fixture + 断言不 panic
}
#[test]
fn serializable_message_deserialize_attachment() { ... }
#[test]
fn serializable_message_deserialize_tool_use() { ... }
#[test]
fn serializable_message_deserialize_tool_result() { ... }
```

### Golden fixtures 清单

```
tests/fixtures/record-replay/
├── session-basic.json              # 含 user/assistant/system 消息
├── session-with-tools.json         # 含 tool_use + tool_result
├── session-with-progress.json      # 含 progress block
├── session-with-attachment.json    # 含 attachment
├── session-empty.json              # 空消息列表
├── rollout-basic.jsonl             # 合法 JSONL: SessionMeta + Messages
├── rollout-with-snapshot.jsonl     # 含 Snapshot checkpoint
├── rollout-corrupt-line.jsonl      # 中间一行损坏
├── rollout-partial-last-line.jsonl # 最后一行不完整
├── rollout-schema-v0.jsonl         # 旧 schema 版本
└── rollout-legacy-only.json        # 无 rollout，只有 legacy JSON
```

---

## Phase 1: record_replay crate module

> 验收: types serde roundtrip, recorder 可写出合法 JSONL, flush/shutdown 幂等, 坏行不 panic

### L1 Unit: types.rs

```rust
#[test]
fn record_line_serde_roundtrip() {
    let line = RecordLine {
        schema_version: 1,
        seq: 42,
        timestamp: Utc::now(),
        session_id: "test-session".into(),
        turn_id: Some("turn-0001".into()),
        item: RecordItem::Message(MessageRecord { msg: sample_message() }),
    };
    let json = serde_json::to_string(&line).unwrap();
    let deserialized: RecordLine = serde_json::from_str(&json).unwrap();
    assert_eq!(line.seq, deserialized.seq);
    assert_eq!(line.session_id, deserialized.session_id);
}

// 所有 RecordItem 变体 serde roundtrip
#[test]
fn record_item_session_meta_roundtrip() { ... }
#[test]
fn record_item_turn_started_roundtrip() { ... }
#[test]
fn record_item_turn_finished_roundtrip() { ... }
#[test]
fn record_item_message_roundtrip() { ... }
#[test]
fn record_item_query_event_roundtrip() { ... }
#[test]
fn record_item_tool_progress_roundtrip() { ... }
#[test]
fn record_item_permission_request_roundtrip() { ... }
#[test]
fn record_item_permission_response_roundtrip() { ... }
#[test]
fn record_item_question_request_roundtrip() { ... }
#[test]
fn record_item_question_response_roundtrip() { ... }
#[test]
fn record_item_compaction_boundary_roundtrip() { ... }
#[test]
fn record_item_snapshot_roundtrip() { ... }
#[test]
fn record_item_rollback_roundtrip() { ... }
#[test]
fn record_item_branch_roundtrip() { ... }
#[test]
fn record_item_legacy_message_roundtrip() { ... }

// 错误类型
#[test]
fn replay_error_serde() { ... }
```

### L1 Unit: config.rs

```rust
#[test]
fn record_replay_config_defaults() {
    let cfg = RecordReplayConfig::default();
    assert!(cfg.enabled);            // CLI/Web 默认 true
    assert!(!cfg.include_raw_stream);
    assert!(!cfg.include_tool_progress);
}

#[test]
fn record_replay_config_disabled_in_test() {
    // 测试环境中默认保持 disabled
}
```

### L1 Unit: paths.rs

```rust
#[test]
fn rollout_day_dir_format() {
    let dt = Utc.with_ymd_and_hms(2026, 7, 2, 13, 14, 55).unwrap();
    let dir = rollout_day_dir(dt);
    assert_eq!(dir, PathBuf::from("rollouts/2026/07/02"));
}

#[test]
fn new_rollout_file_contains_session_id_and_timestamp() {
    let dt = Utc.with_ymd_and_hms(2026, 7, 2, 13, 14, 55).unwrap();
    let f = new_rollout_file("sess-001", dt);
    assert!(f.to_string_lossy().contains("sess-001"));
    assert!(f.to_string_lossy().contains("20260702T131455Z"));
}

#[test]
fn rollout_path_validation_accepts_correct_format() { ... }
#[test]
fn rollout_path_validation_rejects_path_traversal() { ... }
```

### L1 Unit: policy.rs

```rust
#[test]
fn classify_canonical_message() {
    assert_eq!(
        classify_record_item(&RecordItem::Message(..), &default_config()),
        RecordClass::Canonical
    );
}

#[test]
fn classify_canonical_permission() { ... }
#[test]
fn classify_diagnostic_raw_stream() { ... }
#[test]
fn classify_diagnostic_tool_progress() { ... }

#[test]
fn should_persist_canonical_always() {
    assert!(should_persist(&canonical_item(), &config_with_everything_off()));
}

#[test]
fn should_persist_diagnostic_only_when_enabled() {
    let cfg = config_with_tool_progress_off();
    assert!(!should_persist(&tool_progress_item(), &cfg));

    let cfg = config_with_tool_progress_on();
    assert!(should_persist(&tool_progress_item(), &cfg));
}

#[test]
fn may_drop_under_pressure_diagnostic() {
    assert!(may_drop_under_pressure(&diagnostic_item(), &default_config()));
}

#[test]
fn may_drop_under_pressure_canonical_false() {
    assert!(!may_drop_under_pressure(&canonical_item(), &default_config()));
}
```

### L1 Unit: recorder.rs

```rust
#[test]
fn recorder_create_writes_valid_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let handle = SessionRecorderHandle::create(
        RecorderOpenMode::Create {
            session_id: "test".into(),
            cwd: "/tmp".into(),
            created_at: Utc::now(),
            metadata: sample_meta_record(),
        },
        &dir.path().join("rollouts"),
    ).unwrap();
    handle.add(vec![sample_message_item()]).unwrap();
    handle.flush(None).unwrap().unwrap();
    handle.shutdown(None).unwrap().unwrap();

    let content = std::fs::read_to_string(dir.path().join("rollouts/...")).unwrap();
    assert!(content.lines().count() >= 2);  // SessionMeta + Message
}

#[test]
fn recorder_resume_uses_existing_file() {
    // create → write → shutdown → resume → write → shutdown
    // 验证 seq 连续递增
}

#[test]
fn recorder_flush_is_idempotent() {
    // 多次 flush 不报错
}

#[test]
fn recorder_shutdown_is_idempotent() { ... }
#[test]
fn recorder_create_rejects_existing_open() {
    // create_new(true) → 已存在文件时报错
}

#[test]
fn recorder_channel_pressure_drops_diagnostic_only() {
    // channel 满时 canonical items 阻塞或等待
    // diagnostic items 进入丢队列
}

#[test]
fn recorder_stats_after_flush() {
    let stats = handle.flush(None).unwrap().unwrap();
    assert!(stats.event_count > 0);
    assert!(stats.last_seq > 0);
}
```

### L1 Unit: reader.rs

```rust
#[test]
fn reader_empty_file_returns_no_events() {
    let empty = tempfile::NamedTempFile::new().unwrap();
    let events = ReplayReader::read_all(empty.path()).unwrap();
    assert!(events.is_empty());
}

#[test]
fn reader_good_file_returns_all_events() {
    // 从 fixture 读取基本 JSONL → 2 条事件
    assert_eq!(events.len(), 2);
}

#[test]
fn reader_bad_line_generates_warning_continues() {
    // corrupt-line fixture → 返回可恢复的完整行 + warnings
    assert_eq!(events.len(), 2);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn reader_partial_last_line_ignores_truncation() {
    // 最后一行不完整 → 被忽略不 panic
}

#[test]
fn reader_schema_mismatch_returns_error() {
    // v999 schema → 明确错误，不 panic
}

#[test]
fn reader_first_session_meta_defines_session_id() { ... }
#[test]
fn reader_session_id_inconsistency_generates_warning() {
    // 后续行 session_id 与 meta 不一致 → warning
}
```

---

## Phase 2: Engine canonical events

> 验收: 新会话生成 rollout JSONL, turn 完成有明确事件, abort/error 有 TurnFinished

### L2 Integration

```rust
#[test]
fn engine_new_session_creates_rollout() {
    // 创建 engine + 打开 recorder
    // 提交一个用户消息 → shutdown
    // 验证 rollout JSONL 包含: SessionMeta, TurnStarted, Message(user), Message(assistant), TurnFinished
}

#[test]
fn engine_submit_writes_turn_events() {
    // 验证 TurnStarted / TurnFinished 被写入
}

#[test]
fn engine_abort_turn_has_turn_finished() {
    // 提交消息 → 中途 abort → TurnFinished 含 abort_reason
}

#[test]
fn engine_error_turn_has_turn_finished() {
    // 触发 API error → TurnFinished 含错误摘要
}

#[test]
fn engine_permission_request_response_recorded() {
    // permission prompt → PermissionRequest
    // 决策完成后 → PermissionResponse
}

#[test]
fn engine_question_request_response_recorded() { ... }

#[test]
fn engine_shutdown_flushes_recorder_first() {
    // shutdown → recorder flush 被调用 → 再检查 JSONL
}

#[test]
fn engine_write_failure_does_not_corrupt_messages() {
    // recorder 写入失败 → state.messages 不变但返回 error
}

#[test]
fn engine_session_switch_flushes_before_switch() {
    // switch session → 旧 recorder flush → 新 recorder open
}
```

---

## Phase 3: Replay-first resume

> 验收: shutdown 后 resume 消息一致, corrupt 文件可恢复到最后完整行, legacy fallback

### L2 Integration

```rust
#[test]
fn resume_replay_session_messages_match() {
    // 创建 session → 写入消息 → shutdown → resume(session_id) → 消息数一致
}

#[test]
fn resume_replay_with_corrupt_tail() {
    // rollout 最后半行损坏 → resume 到最后完整行
}

#[test]
fn resume_replay_missing_rollout_fallback_legacy() {
    // 只有 legacy JSON 无 rollout → fallback 成功
}

#[test]
fn resume_replay_missing_both_returns_error() {
    // 无 rollout 无 legacy → 返回 session not found error
}

#[test]
fn resume_replay_reconstructs_metadata() {
    // SessionMeta 中的 cwd / created_at / config 摘要恢复正确
}

#[test]
fn resume_replay_pending_interactions_detected() {
    // 有未完成 PermissionRequest → pending_interactions 非空
    // resume 后不自动重发权限弹窗
}

#[test]
fn resume_replay_compaction_boundary_messages_preserved() { ... }
```

### L3 E2E

```rust
#[ignore = "requires full CLI binary"]
#[test]
fn cli_continue_session_after_restart() {
    // 启动 CLI → 输入消息 → exit
    // 启动 CLI --continue → 消息可见
}
```

---

## Phase 4: SQLite index, snapshot, migration

> 验收: 删除 SQLite 后可从 JSONL reindex, legacy JSON 可懒迁移

### L1 Unit: index.rs

```rust
#[test]
fn index_upsert_and_query() {
    // upsert → query → 字段匹配
}

#[test]
fn index_update_after_recorder_flush() { ... }     // last_seq / event_count 正确
#[test]
fn index_update_failure_does_not_block_rollout() {
    // 索引更新失败 → JSONL 仍写入完成
}

// reindex
#[test]
fn reindex_from_rollout_builds_correct_index() {
    // 删除 SQLite → reindex → 查询有记录
}
```

### L1 Unit: migration.rs

```rust
#[test]
fn migrate_legacy_json_to_synthetic_rollout() {
    let legacy = load_fixture("session-basic.json");
    let rollout = Migration::migrate(legacy);
    assert!(rollout.lines().any(|l| l.contains("migrated_from")));
    assert!(rollout.lines().any(|l| l.contains("Snapshot")));
}

#[test]
fn migrate_legacy_json_keeps_original_file() {
    // 迁移后原始 JSON 不被删除
}

#[test]
fn migrate_legacy_message_falls_back_to_legacy_message_type() { ... }
#[test]
fn migrate_partial_message_creates_legacy_message_record() { ... }

// Session export
#[test]
fn session_export_includes_rollout_path_and_last_seq() { ... }
#[test]
fn session_export_prefers_replay_over_legacy() { ... }
```

### L1 Unit: redaction.rs

```rust
#[test]
fn redact_api_key_from_tool_input() {
    let input = r#"{"command": "export API_KEY=sk-abc123"}"#;
    let redacted = redact_sensitive(&input);
    assert!(!redacted.contains("sk-abc123"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn redact_authorization_header() { ... }
#[test]
fn redact_large_base64_blob() { ... }
#[test]
fn redact_leaves_normal_paths_untouched() { ... }

#[test]
fn redaction_summary_in_metadata() {
    // RecordItem 的 redaction_summary 计数正确
}
```

---

## Phase 5-7: 扩展功能

### Phase 5: TUI/Web replay state

```rust
#[test]
fn web_session_detail_uses_replay_first() {
    // request → response 包含 replay warnings + last_seq
}

#[test]
fn tui_session_switch_from_reconstructed_session() { ... }

#[test]
fn transcript_generated_from_replay_matches_original() { ... }
```

### Phase 6: Rollback/branch/edit/regenerate

```rust
#[test]
fn rollback_event_added_not_truncated() {
    // rollback → 原始消息不被删除 → Rollback event 被追加
}

#[test]
fn rollback_visible_messages_match_expected() { ... }
#[test]
fn branch_records_parent_session_id_and_seq() { ... }
#[test]
fn regenerate_does_not_overwrite_original() { ... }
#[test]
fn rollback_preview_shows_change() { ... }
```

### Phase 7: Performance & verify

```rust
#[test]
fn verify_detects_seq_gap() { ... }
#[test]
fn verify_detects_session_id_mismatch() { ... }
#[test]
fn verify_detects_snapshot_hash_mismatch() { ... }
#[test]
fn compressed_rollout_still_readable() { ... }
```

---

## 测试运行命令

```bash
# Phase 0: 基线
cargo test -p allthecodes-session

# Phase 1: record_replay 核心模块
cargo test -p allthecodes-session record_replay::
cargo test -p allthecodes-session -- record_line
cargo test -p allthecodes-session -- recorder
cargo test -p allthecodes-session -- reader
cargo test -p allthecodes-session -- policy
cargo test -p allthecodes-session -- migration

# Phase 2: Engine 集成
cargo test -p allthecodes-engine -- record_replay
cargo test -p allthecodes-engine -- record_replay_integration

# Phase 3: Resume
cargo test -p allthecodes-session -- resume_replay

# Phase 4: Index & migration
cargo test -p allthecodes-session -- index
cargo test -p allthecodes-session -- redaction

# 全量
cargo test -p allthecodes-session
cargo test -p allthecodes-engine
cargo test -p allthecodes-web sessions
cargo test -p allthecodes --lib
cargo build --workspace --release
```

## 最少可交付验收标准

1. [x] 新 session 生成 rollout JSONL, 可独立于 legacy JSON 恢复
2. [x] Legacy JSON session 仍可读取并可懒迁移
3. [x] SQLite 删除后可重建
4. [x] Reader 对坏行/半行/旧 schema 有明确行为
5. [x] Shutdown 前 flush canonical events
