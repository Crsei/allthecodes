# Command 统一风险分类 TDD 测试计划

> 来源计划: `development/command/unified-command-risk-classification-plan.md`
> 生成日期: 2026-07-03
> 覆盖范围: `crates/allthecodes-permissions/src/command_risk/`

## 状态

尚未实现 — Phase 1（纯分类 API）是当前目标。

## 整体策略

Phase 1 先新增纯分类 API + 单元测试，不接入执行链路；Phase 2 接执行链路安全校验；Phase 3 接 UI 和 Auto classifier。

---

## Phase 1: 纯分类 API

### L1 Unit: 类型定义

```rust
// CommandRiskLevel serde
#[test]
fn command_risk_level_serde() {
    let levels = vec![
        (r#""read""#, CommandRiskLevel::Read),
        (r#""build""#, CommandRiskLevel::Build),
        (r#""mutate""#, CommandRiskLevel::Mutate),
        (r#""destructive""#, CommandRiskLevel::Destructive),
        (r#""deploy""#, CommandRiskLevel::Deploy),
        (r#""secret""#, CommandRiskLevel::Secret),
    ];
    for (json, expected) in levels {
        assert_eq!(serde_json::from_str::<CommandRiskLevel>(json).unwrap(), expected);
    }
}

// CommandRisk struct field consistency
#[test]
fn command_risk_constructs() {
    let risk = CommandRisk {
        level: CommandRiskLevel::Destructive,
        reason: "rm -rf recognized".into(),
        matched_rule: Some("rm -rf".into()),
        segments: vec![],
        confidence: CommandRiskConfidence::High,
    };
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}
```

### L1 Unit: Bash 分类 — Read

```rust
#[test]
fn bash_read_ls() {
    let risk = classify_command_risk("ls -la", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Read);
}

#[test]
fn bash_read_cat() { ... }     // cat /path/to/file → Read
#[test]
fn bash_read_grep() { ... }    // grep -r "foo" . → Read
#[test]
fn bash_read_git_status() { ... }
#[test]
fn bash_read_git_diff() { ... }
#[test]
fn bash_read_git_log() { ... }
#[test]
fn bash_read_rg() { ... }     // rg pattern → Read

// 带管道/重定向不能 Read
#[test]
fn bash_read_with_pipe_is_not_read() {
    let risk = classify_command_risk("ls | head", ShellKind::Bash);
    assert_ne!(risk.level, CommandRiskLevel::Read);
}

#[test]
fn bash_read_with_variable_expansion_not_read() {
    let risk = classify_command_risk("cat $FILE", ShellKind::Bash);
    assert_ne!(risk.level, CommandRiskLevel::Read);
}

#[test]
fn bash_read_with_redirect_not_read() {
    let risk = classify_command_risk("cat file > out", ShellKind::Bash);
    assert_ne!(risk.level, CommandRiskLevel::Read);
}
```

### L1 Unit: Bash 分类 — Build

```rust
#[test]
fn bash_build_cargo_build() {
    let risk = classify_command_risk("cargo build --release", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Build);
}

#[test]
fn bash_build_cargo_test() { ... }
#[test]
fn bash_build_cargo_check() { ... }
#[test]
fn bash_build_cargo_clippy() { ... }
#[test]
fn bash_build_npm_run_build() { ... }
#[test]
fn bash_build_npm_test() { ... }
#[test]
fn bash_build_pnpm_build() { ... }
#[test]
fn bash_build_pytest() { ... }
#[test]
fn bash_build_vitest() { ... }
#[test]
fn bash_build_make() { ... }
#[test]
fn bash_build_go_build() { ... }
#[test]
fn bash_build_go_test() { ... }

// Build 命令含发布/上传按更高风险
#[test]
fn bash_build_with_publish_is_deploy() {
    let risk = classify_command_risk("cargo build && cargo publish", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Deploy);
}
```

### L1 Unit: Bash 分类 — Destructive

```rust
#[test]
fn bash_destructive_rm_rf_target() {
    let risk = classify_command_risk("rm -rf target", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}

#[test]
fn bash_destructive_rm_recursive_force() {
    let risk = classify_command_risk("rm -r -f /tmp/data", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}

#[test]
fn bash_destructive_git_reset_hard() {
    let risk = classify_command_risk("git reset --hard", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}

#[test]
fn bash_destructive_git_clean_fd() {
    let risk = classify_command_risk("git clean -fd", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}

#[test]
fn bash_destructive_git_clean_dfx() {
    let risk = classify_command_risk("git clean -dfx", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}

// --dry-run 不是 Destructive
#[test]
fn bash_destructive_git_clean_dry_run_not_destructive() {
    let risk = classify_command_risk("git clean -fd --dry-run", ShellKind::Bash);
    assert_ne!(risk.level, CommandRiskLevel::Destructive);
}

#[test]
fn bash_destructive_git_push_force() { ... }
#[test]
fn bash_destructive_git_stash_drop() { ... }
#[test]
fn bash_destructive_git_stash_clear() { ... }
#[test]
fn bash_destructive_terraform_destroy() {
    let risk = classify_command_risk("terraform destroy -auto-approve", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}
#[test]
fn bash_destructive_pulumi_destroy() { ... }
#[test]
fn bash_destructive_dd_if_dev() {
    let risk = classify_command_risk("dd if=/dev/zero of=/dev/sda", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}
#[test]
fn bash_destructive_mkfs() { ... }
```

### L1 Unit: Bash 分类 — Deploy

```rust
#[test]
fn bash_deploy_git_push() { ... }
#[test]
fn bash_deploy_npm_publish() { ... }
#[test]
fn bash_deploy_cargo_publish() { ... }
#[test]
fn bash_deploy_docker_push() { ... }
#[test]
fn bash_deploy_kubectl_apply() { ... }
#[test]
fn bash_deploy_kubectl_rollout() { ... }
#[test]
fn bash_deploy_helm_upgrade() { ... }
#[test]
fn bash_deploy_terraform_apply() { ... }
#[test]
fn bash_deploy_pulumi_up() { ... }

// 云 CLI 变更命令
#[test]
fn bash_deploy_aws_create() { ... }
#[test]
fn bash_deploy_gcloud_deploy() { ... }
#[test]
fn bash_deploy_az_create() { ... }

// kubectl delete → Destructive（更高优先级）
#[test]
fn bash_deploy_kubectl_delete_is_destructive() {
    let risk = classify_command_risk("kubectl delete pod foo", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Destructive);
}
```

### L1 Unit: Bash 分类 — Secret

```rust
#[test]
fn bash_secret_cat_ssh_key() {
    let risk = classify_command_risk("cat ~/.ssh/id_rsa", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Secret);
}

#[test]
fn bash_secret_cat_aws_credentials() {
    let risk = classify_command_risk("cat ~/.aws/credentials", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Secret);
}

#[test]
fn bash_secret_cat_dot_env() { ... }
#[test]
fn bash_secret_echo_token() {
    let risk = classify_command_risk("echo $GITHUB_TOKEN", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Secret);
}
#[test]
fn bash_secret_printenv() { ... }
#[test]
fn bash_secret_pbcopy_secret() { ... }
#[test]
fn bash_secret_curl_with_secret_header() { ... }
```

### L1 Unit: Bash 分类 — 默认 Mutate

```rust
#[test]
fn bash_mutate_unknown_command() {
    let risk = classify_command_risk("some_unknown_tool arg1", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Mutate);
}

#[test]
fn bash_mutate_mkdir() { ... }
#[test]
fn bash_mutate_cp() { ... }
#[test]
fn bash_mutate_mv() { ... }
#[test]
fn bash_mutate_sed_i() { ... }

// Parser failure fail-closed → Mutate
#[test]
fn bash_mutate_parse_failure() {
    let risk = classify_command_risk("$(malformed syntax", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Mutate);
}
```

### L1 Unit: PowerShell

```rust
#[test]
fn powershell_read_get_childitem() { ... }
#[test]
fn powershell_destructive_remove_item_recurse_force() { ... }
#[test]
fn powershell_destructive_clear_disk() { ... }
#[test]
fn powershell_destructive_format_volume() { ... }
#[test]
fn powershell_secret_get_secret() { ... }
```

### L1 Unit: 复合命令取最高风险

```rust
#[test]
fn compound_command_highest_risk_wins() {
    // cat .env → Secret 优先于管道后面的 Read
    let risk = classify_command_risk("cat .env | grep password", ShellKind::Bash);
    assert_eq!(risk.level, CommandRiskLevel::Secret);
}

#[test]
fn compound_command_mixed_mutate_and_read() { ... }
```

### L1 Unit: Confidence

```rust
#[test]
fn confidence_high_for_exact_match() { ... }
#[test]
fn confidence_low_for_heuristic() { ... }
```

### Phase 1 验收条件

- [x] `git reset --hard` → `Destructive`
- [x] `git clean -fd` → `Destructive`, `git clean -fdn` → 非 `Destructive`
- [x] `rm -rf target` → `Destructive`
- [x] `terraform destroy` / `pulumi destroy` → `Destructive`
- [x] `npm run build` / `cargo build` / `pytest` → `Build`
- [x] `cat`, `ls`, `rg`, `git status`, `git diff` 简单命令 → `Read`
- [x] 未知命令 → `Mutate`, parser failure → `Mutate`

---

## Phase 2: 执行链路接入

### L2 Integration

```rust
#[test]
fn security_validate_reads_risk() {
    // security_validate 调用 classify_command_risk
    // 记录 risk_level / risk_reason / matched_rule 到 audit
}

#[test]
fn security_validate_existing_dangerous_unaffected() {
    // 现有 dangerous 测试全部保留 → 不退化
}

#[test]
fn permission_risk_level_from_unified() {
    // permission_risk_level() 使用 CommandRisk → UI 展示一致
}

#[test]
fn auto_review_event_has_risk_fields() {
    // Auto Review event 包含 command_risk_level/reason
}
```

---

## Phase 3: UI / Auto classifier 接入

### L2 Integration

```rust
#[test]
fn shell_heuristic_risk_mapping_read() {
    // Read → Safe
}

#[test]
fn shell_heuristic_risk_mapping_build() { ... }    // Build → Medium
#[test]
fn shell_heuristic_risk_mapping_destructive() { ... } // Destructive → Destructive
#[test]
fn shell_heuristic_risk_mapping_secret() { ... }   // Secret → High

#[test]
fn safety_classifier_high_risk_triggers_thinking() {
    // Destructive/Secret/Deploy → thinking stage 触发
}

#[test]
fn safety_classifier_low_risk_skips_thinking() {
    // Read → 跳过 thinking
}
```

---

## 运行命令

```bash
# Phase 1
cargo test -p allthecodes-permissions command_risk

# Phase 2
cargo test -p allthecodes-engine permission_risk

# Phase 3
cargo test -p allthecodes-tool-display shell_heuristic
cargo test -p allthecodes-safety classifier

# 全量
cargo test -p allthecodes-permissions
cargo build --workspace --release
```
