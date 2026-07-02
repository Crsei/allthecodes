# TUI Command Operation Display TDD 测试计划

> 来源计划: `development/tui/command-operation-display-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: `crates/allthecodes-tool-display/`, `crates/allthecodes/src/ui/messages/render/`

## 状态

**已全部实现**（2026-07-02）。本计划作为回归测试清单。

---

## Phase 1: 共享 operation classifier

### L1 Unit: 核心类型

```rust
#[test]
fn operation_kind_serde() {
    let kinds = vec![
        (r#""read""#, OperationKind::Read),
        (r#""search""#, OperationKind::Search),
        (r#""create""#, OperationKind::Create),
        (r#""modify""#, OperationKind::Modify),
        (r#""delete""#, OperationKind::Delete),
        (r#""execute""#, OperationKind::Execute),
        (r#""permission""#, OperationKind::Permission),
        (r#""plan""#, OperationKind::Plan),
        (r#""status""#, OperationKind::Status),
        (r#""delegate""#, OperationKind::Delegate),
        (r#""network""#, OperationKind::Network),
        (r#""system""#, OperationKind::System),
        (r#""unknown""#, OperationKind::Unknown),
    ];
    for (json, expected) in kinds {
        assert_eq!(serde_json::from_str::<OperationKind>(json).unwrap(), expected);
    }
}

#[test]
fn operation_subtype_serde() {
    // Build, Test, Format, Install, Shell, Mcp, Todo, Task
}

#[test]
fn tool_operation_serde_roundtrip() { ... }
```

### L1 Unit: Shell heuristic

```rust
#[test]
fn heuristic_read_identifies_read_commands() { ... }
#[test]
fn heuristic_search_identifies_grep() { ... }
#[test]
fn heuristic_create_mkdir() { ... }
#[test]
fn heuristic_modify_sed_i() { ... }
#[test]
fn heuristic_delete_rm() { ... }
#[test]
fn heuristic_build_cargo_build() { ... }
#[test]
fn heuristic_test_cargo_test() { ... }
#[test]
fn heuristic_format_cargo_fmt() { ... }
#[test]
fn heuristic_network_curl() { ... }
#[test]
fn heuristic_install_npm_install() { ... }
#[test]
fn heuristic_unknown_fallback() { ... }
```

### L1 Unit: Tool name classifier

```rust
#[test]
fn classify_tool_read_tools() {
    // Read, FileRead, file_read → Read
}

#[test]
fn classify_tool_search_tools() {
    // Grep, Search, file_search → Search
}

#[test]
fn classify_tool_create() { ... }   // Write, FileWrite, Create
#[test]
fn classify_tool_modify() { ... }   // Edit, FileEdit, ApplyPatch
#[test]
fn classify_tool_delete() { ... }   // Delete, FileDelete
#[test]
fn classify_tool_execute() { ... }  // Bash, PowerShell, ExecuteCommand
#[test]
fn classify_tool_delegate() { ... } // Agent, TaskAgent, Delegate
#[test]
fn classify_tool_plan() { ... }     // Workflow, Plan, UpdatePlan
#[test]
fn classify_tool_status() { ... }   // SystemStatus, QueryStatus
#[test]
fn classify_tool_unknown() { ... }  // 未识别
```

---

## Phase 2-3: TUI operation row & batch

### L2 Integration: Renderer

```rust
#[test]
fn operation_row_read_renders_path() {
    // Read src/lib.rs → 显示 path + status
}

#[test]
fn operation_row_search_shows_match_count() {
    // Search "keyword" → "4 files, 12 matches"
}

#[test]
fn operation_row_modify_shows_diff() {
    // Modify file → "+18 -4"
}

#[test]
fn operation_row_shell_shows_exit_code() {
    // Run cargo test → "exit 0"
}

#[test]
fn operation_row_cancelled_status() {
    // cancelled → 显示取消态
}

// Operation batch
#[test]
fn batch_read_search_gt1_auto_collapse() {
    // 同一 turn 中 2 个 Read → batch 折叠显示
}

#[test]
fn batch_modify_gt1_visible_risk() {
    // 2 个 Modify → batch 标题可见风险
}

#[test]
fn batch_single_not_collapsed() {
    // 1 个 Read → 不折叠
}

#[test]
fn batch_expandable() {
    // 折叠后可展开每个 operation row
}

// verbose mode
#[test]
fn verbose_skips_operation_batching() {
    // verbose → 原始 tool-use/tool-result 渲染
}

// TODO list
#[test]
fn todo_write_renders_as_checklist() {
    // TodoWrite → 不显示普通 tool card → 渲染 checklist
}
```

---

## Phase 4-5: 权限/Always Allow/Auto Review

### L2 Integration

```rust
#[test]
fn permission_request_shows_semantic_summary() {
    // Permission: Modify src/lib.rs
}

#[test]
fn permission_always_allow_persists_rule() {
    // 选择 Always Allow → 写入规则到 settings.local.json
}

#[test]
fn permission_auto_review_starts_and_completes() {
    // 选择 Auto Review → started/completed 事件
}

#[test]
fn permission_auto_review_approved() {
    // approved → allow once
}

#[test]
fn permission_auto_review_denied() {
    // denied → deny
}

#[test]
fn permission_auto_review_timed_out() {
    // timed out → deny/block with timeout message
}

#[test]
fn permission_auto_review_fail_closed() {
    // 审查失败/crash → deny/block
}

// 快捷键
#[test]
fn permission_key_y_allows() { ... }
#[test]
fn permission_key_n_denies() { ... }
#[test]
fn permission_key_a_always_allow() { ... }
#[test]
fn permission_key_r_auto_review() { ... }
#[test]
fn permission_key_e_expand() { ... }
```

---

## Phase 6-7: Side-channel / copy / 验收

### Side-channel

```rust
#[test]
fn side_channel_image_path_displayed() {
    // classifier 从 input/result 提取 preview/image/diff 引用
}

#[test]
fn side_channel_path_not_exist_shows_compact() {
    // 路径不存在 → 紧凑引用，不打开外部应用
}
```

### Copy

```rust
#[test]
fn copy_semantic_uses_operation_summary() {
    // 默认 semantic copy → 只复制主摘要
}

#[test]
fn copy_raw_debug_preserves_json() {
    // raw/debug copy → 保留原始 message JSON
}
```

### Release build

```rust
#[test]
fn no_new_warningsReleaseBuild() {
    // cargo build --workspace --release 无新增 warning
}
```

---

## 运行命令

```bash
# 核心 crate
cargo test -p allthecodes-tool-display

# TUI 聚焦测试（当前已知部分测试因 web/protocol 编译漂移阻塞）
cargo check -p allthecodes --bin allthecodes

# 全量
cargo build --workspace --release
```
