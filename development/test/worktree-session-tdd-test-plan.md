# Worktree-aware Session TDD 测试计划

> 来源计划: `development/worktree/worktree-aware-session-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: `allthecodes-worktree`, `allthecodes-engine/agent/worktree.rs`, `allthecodes-session` (worktree_sessions)

## 现有状态

大部分功能已实现。测试计划覆盖：
- 已落地功能的回归测试
- 边界条件（已隐式存在的场景）
- 文档中提到的 danger/fail-closed 验证
- 跨 crate 集成测试

---

## Phase 1: 类型 & 存储（当前需覆盖）

### L1 Unit: types

```rust
// 1. Serde roundtrip
#[test]
fn worktree_session_record_serde_roundtrip() {
    let record = WorktreeSessionRecord {
        schema_version: 1,
        session_id: "sess-001".into(),
        repo_id: "repo-xyz".into(),
        worktree_path: PathBuf::from("/tmp/worktrees/feature-x"),
        branch: "feature-x".into(),
        base_commit: Some("abc123".into()),
        linked_goal_id: None,
        created_by: WorktreeSessionCreator::Human,
        git_root: PathBuf::from("/repo/.git"),
        original_cwd: Some(PathBuf::from("/repo")),
        status: WorktreeSessionStatus::Active,
        source: WorktreeSessionSource::EnterWorktree,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        kept_at: None,
        removed_at: None,
    };
    let json = serde_json::to_string(&record).unwrap();
    let deserialized: WorktreeSessionRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(record.session_id, deserialized.session_id);
    assert_eq!(record.created_by, deserialized.created_by);
    assert_eq!(record.status, deserialized.status);
}

// 2. Enum variants exhaustive
#[test]
fn worktree_session_creator_only_human_or_agent() {
    // serde 不接受其他值
    let result: Result<WorktreeSessionCreator, _> = serde_json::from_str("\"robot\"");
    assert!(result.is_err());
}

#[test]
fn worktree_session_status_all_variants() {
    // Active, Kept, Removed, CleanupFailed, Orphaned
    let statuses = vec![
        ("\"active\"", WorktreeSessionStatus::Active),
        ("\"kept\"", WorktreeSessionStatus::Kept),
        // ...
    ];
    for (json, expected) in statuses {
        assert_eq!(serde_json::from_str::<WorktreeSessionStatus>(json).unwrap(), expected);
    }
}

#[test]
fn worktree_session_source_all_variants() {
    // EnterWorktree, AgentIsolation, WorktreeCreateHook, Imported
}

// 3. Status transition
#[test]
fn worktree_session_status_transitions() {
    // active → kept（exit keep）
    // active → removed（exit remove 成功）
    // active → cleanup_failed（exit remove 失败）
}
```

### L1 Unit: store

```rust
// Store API
#[test]
fn store_upsert_creates_new_record() {
    let store = setup_store();
    let record = sample_record();
    store.upsert_worktree_session(record.clone()).unwrap();
    let loaded = store.get_active_worktree_session("sess-001").unwrap();
    assert_eq!(loaded.session_id, "sess-001");
}

#[test]
fn store_upsert_updates_existing_record() {
    // 相同 PK → updated_at 更新
}

#[test]
fn store_mark_kept_updates_status() { ... }
#[test]
fn store_mark_removed_updates_status() { ... }
#[test]
fn store_mark_cleanup_failed_updates_status() { ... }

#[test]
fn store_list_for_repo_returns_correct_sessions() { ... }
#[test]
fn store_get_active_returns_none_for_missing() { ... }
#[test]
fn store_get_active_returns_only_active() {
    // kept 的不算 active
}

// workspace_key() — repo_id 一致性
#[test]
fn workspace_key_same_for_main_and_linked_worktree() {
    let main_key = workspace_key(&PathBuf::from("/repo/.git"));
    let linked_key = workspace_key(&PathBuf::from("/repo/.git/worktrees/feature-x"));
    assert_eq!(main_key, linked_key);
}

// Migration 可重复运行
#[test]
fn worktree_sessions_migration_idempotent() {
    let conn = setup_sqlite();
    run_migration(&conn).unwrap();
    run_migration(&conn).unwrap();  // 第二次不报错
}
```

---

## Phase 2: EnterWorktree / ExitWorktree 写入（已实现，需回归）

### L2 Integration

```rust
#[test]
fn enter_worktree_creates_record() {
    // 模拟 EnterWorktreeTool::call() 成功
    // → 数据库可查询到 active record
    // → created_by = Human
    let record = store.get_active_worktree_session("sess-001").unwrap();
    assert_eq!(record.created_by, WorktreeSessionCreator::Human);
    assert_eq!(record.status, WorktreeSessionStatus::Active);
}

#[test]
fn exit_worktree_keep_sets_status_kept() {
    // ExitWorktree { action: "keep" } → status = Kept
    // → kept_at 非空
    let record = store.get_active_worktree_session("sess-001").unwrap();
    assert_eq!(record.status, WorktreeSessionStatus::Kept);
    assert!(record.kept_at.is_some());
}

#[test]
fn exit_worktree_remove_sets_status_removed() {
    // remove 成功 → status = Removed
    // → removed_at 非空
}

#[test]
fn exit_worktree_remove_failure_does_not_set_removed() {
    // remove 失败 → 记录状态不变（不误标 removed）
}

#[test]
fn process_global_current_session_still_works() {
    // CURRENT_SESSION 降级后仍可作为快速缓存
}
```

---

## Phase 3: Agent isolation（已实现，需回归）

### L2 Integration

```rust
#[test]
fn agent_isolation_creates_record_with_agent_creator() {
    // AgentTool::run_in_worktree() 成功 → record 创建
    // → created_by = Agent
    let record = store.get_active_worktree_session("sess-001").unwrap();
    assert_eq!(record.created_by, WorktreeSessionCreator::Agent);
    assert_eq!(record.source, WorktreeSessionSource::AgentIsolation);
}

#[test]
fn agent_isolation_no_changes_marks_removed() {
    // agent 无变更 → 清理成功 → record = Removed
}

#[test]
fn agent_isolation_with_changes_marks_kept() {
    // agent 有变更 → 保留 → record = Kept
}

#[test]
fn agent_isolation_cleanup_failure_does_not_double_free() {
    // fail-closed: 记录更新失败不误删 worktree
}
```

---

## Phase 4: Linked goal（已实现，需回归）

```rust
#[test]
fn enter_worktree_with_active_goal_links_goal() {
    // 有 active goal → linked_goal_id 非空
}

#[test]
fn enter_worktree_without_goal_keeps_none() {
    // 无 goal → linked_goal_id = None
}

#[test]
fn agent_isolation_inherits_active_goal() {
    // 继承父 session 的 goal
}

#[test]
fn goal_completion_does_not_modify_worktree_status() {
    // goal 完成 → worktree record 不变
}
```

---

## Phase 5: API & UI

### L2 Integration

```rust
#[test]
fn web_api_list_worktree_sessions_by_repo() {
    // GET /api/worktree-sessions?repo_id=... → 200 + list
}

#[test]
fn web_api_get_current_worktree_session() {
    // GET /api/worktree-sessions/current → 200 + record
}

#[test]
fn web_api_get_by_session_id() {
    // GET /api/worktree-sessions/{session_id} → 200 + record
}

#[test]
fn web_api_returns_camel_case_fields() {
    let body: serde_json::Value = response_body();
    assert!(body.get("sessionId").is_some());
    assert!(body.get("repoId").is_some());
    assert!(body.get("worktreePath").is_some());
}

#[test]
fn web_api_works_after_restart() {
    // 重启后 API 仍可查询旧记录
}

#[test]
fn old_git_worktrees_api_unchanged() {
    // /api/git/worktrees 行为不变
}
```

---

## Phase 6: Recovery & orphan detection

```rust
#[test]
fn startup_detects_active_worktree_no_path() {
    // path 不存在 → status = Orphaned
}

#[test]
fn startup_detects_kept_worktree_no_path() {
    // kept 但 path 不存在 → status = Orphaned
}

#[test]
fn startup_detects_path_missing_from_git_worktree_list() {
    // path 存在但 git worktree list 无记录 → Orphaned
}

#[test]
fn resume_session_does_not_auto_chdir() {
    // resume 有 active worktree → cwd 不自动切换
}

#[test]
fn api_query_also_performs_orphan_reconciliation() {
    // 查询时也会做 best-effort reconciliation
}
```

---

## 退出条件

1. [ ] 所有现存 `WorktreeSessionRecord` 类型字段有 serde roundtrip test
2. [ ] 所有 status variants 有 JSON roundtrip test
3. [ ] `workspace_key()` 对 main repo 与 linked worktree 返回相同 repo id
4. [ ] `EnterWorktree` / `ExitWorktree` 写入/更新 record（集成测试）
5. [ ] Agent `isolation: "worktree"` 写入/更新 record
6. [ ] Cleanup failure 不误标 removed
7. [ ] Web API 重启后可查询旧 record
8. [ ] Resume session 不自动切到旧 worktree

---

## 运行命令

```bash
# 全量 worktree 相关测试
cargo test -p allthecodes-session worktree
cargo test -p allthecodes-worktree worktree
cargo test -p allthecodes-engine agent::worktree
cargo test -p allthecodes-web worktree

# 集成编译
cargo build --workspace --release
```
