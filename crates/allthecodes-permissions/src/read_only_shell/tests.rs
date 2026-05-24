use super::*;

use allthecodes_shell_command::model::ReadOnlyResult;

// --- Helpers ---

fn ro(argv: &[&str], raw: &str) -> ReadOnlyResult {
    let a: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    is_read_only_shell_command(&a, raw)
}

fn is_ro(argv: &[&str], raw: &str) -> bool {
    ro(argv, raw) == ReadOnlyResult::ReadOnly
}

fn is_not_ro(argv: &[&str], raw: &str) -> bool {
    matches!(ro(argv, raw), ReadOnlyResult::NotReadOnly(_))
}

fn is_unsupported(argv: &[&str], raw: &str) -> bool {
    matches!(ro(argv, raw), ReadOnlyResult::Unsupported(_))
}

// --- GIT ---

#[test]
fn test_git_status_read_only() {
    assert!(is_ro(&["git", "status"], "git status"));
    assert!(is_ro(&["git", "status", "-s"], "git status -s"));
    assert!(is_ro(
        &["git", "status", "--porcelain"],
        "git status --porcelain"
    ));
}

#[test]
fn test_git_diff_read_only() {
    assert!(is_ro(&["git", "diff"], "git diff"));
    assert!(is_ro(&["git", "diff", "--cached"], "git diff --cached"));
    assert!(is_ro(
        &["git", "diff", "HEAD~1", "HEAD"],
        "git diff HEAD~1 HEAD"
    ));
}

#[test]
fn test_git_log_read_only() {
    assert!(is_ro(&["git", "log"], "git log"));
    assert!(is_ro(
        &["git", "log", "--oneline", "-n", "5"],
        "git log --oneline -n 5"
    ));
    assert!(is_ro(
        &["git", "log", "--all", "--graph"],
        "git log --all --graph"
    ));
}

#[test]
fn test_git_push_not_read_only() {
    assert!(is_not_ro(&["git", "push"], "git push"));
    assert!(is_not_ro(
        &["git", "push", "origin", "main"],
        "git push origin main"
    ));
}

#[test]
fn test_git_commit_not_read_only() {
    assert!(is_not_ro(
        &["git", "commit", "-m", "msg"],
        "git commit -m msg"
    ));
}

#[test]
fn test_git_checkout_not_read_only() {
    assert!(is_not_ro(
        &["git", "checkout", "branch"],
        "git checkout branch"
    ));
}

#[test]
fn test_git_reset_not_read_only() {
    assert!(is_not_ro(&["git", "reset"], "git reset"));
}

#[test]
fn test_git_clean_not_read_only() {
    assert!(is_not_ro(&["git", "clean", "-fd"], "git clean -fd"));
}

#[test]
fn test_git_reflog_read_only() {
    assert!(is_ro(&["git", "reflog"], "git reflog"));
    assert!(is_ro(&["git", "reflog", "show"], "git reflog show"));
}

#[test]
fn test_git_reflog_expire_not_read_only() {
    assert!(is_not_ro(&["git", "reflog", "expire"], "git reflog expire"));
}

#[test]
fn test_git_branch_list_read_only() {
    assert!(is_ro(&["git", "branch", "-a"], "git branch -a"));
}

#[test]
fn test_git_branch_delete_not_read_only() {
    assert!(is_not_ro(
        &["git", "branch", "-d", "old-branch"],
        "git branch -d old-branch"
    ));
}

#[test]
fn test_git_fetch_not_read_only() {
    assert!(is_not_ro(&["git", "fetch", "--all"], "git fetch --all"));
    assert!(is_not_ro(
        &["git", "fetch", "--dry-run"],
        "git fetch --dry-run"
    ));
}

#[test]
fn test_git_ls_files_read_only() {
    assert!(is_ro(&["git", "ls-files"], "git ls-files"));
    assert!(is_ro(&["git", "ls-files", "-c"], "git ls-files -c"));
}

#[test]
fn test_git_describe_read_only() {
    assert!(is_ro(&["git", "describe"], "git describe"));
}

#[test]
fn test_git_blame_read_only() {
    assert!(is_ro(&["git", "blame", "file.txt"], "git blame file.txt"));
}

#[test]
fn test_git_grep_read_only() {
    assert!(is_ro(&["git", "grep", "pattern"], "git grep pattern"));
    assert!(is_ro(
        &["git", "grep", "-n", "pattern", "--", "*.rs"],
        "git grep -n pattern -- *.rs"
    ));
}

#[test]
fn test_git_config_read_only() {
    assert!(is_ro(&["git", "config", "--list"], "git config --list"));
    assert!(is_ro(
        &["git", "config", "user.name"],
        "git config user.name"
    ));
}

#[test]
fn test_git_config_write_not_read_only() {
    assert!(is_not_ro(
        &["git", "config", "user.name", "new-name"],
        "git config user.name new-name"
    ));
}

#[test]
fn test_git_unknown_subcommand_unsupported() {
    assert!(is_not_ro(&["git", "foo"], "git foo"));
    assert!(is_not_ro(&["git", "bisect", "start"], "git bisect start"));
}

#[test]
fn test_git_tag_list_read_only() {
    assert!(is_ro(&["git", "tag", "-l"], "git tag -l"));
}

#[test]
fn test_git_diff_late_output_flag_not_read_only() {
    assert!(is_not_ro(
        &["git", "diff", "HEAD", "--output=/tmp/pwn.patch"],
        "git diff HEAD --output=/tmp/pwn.patch"
    ));
}

#[test]
fn test_git_archive_output_not_read_only() {
    assert!(is_not_ro(
        &["git", "archive", "--output", "/tmp/archive.tar", "HEAD"],
        "git archive --output /tmp/archive.tar HEAD"
    ));
}

// --- GH ---

#[test]
fn test_gh_pr_view_read_only() {
    assert!(is_ro(&["gh", "pr", "view", "123"], "gh pr view 123"));
}

#[test]
fn test_gh_pr_list_read_only() {
    assert!(is_ro(&["gh", "pr", "list"], "gh pr list"));
}

#[test]
fn test_gh_issue_list_read_only() {
    assert!(is_ro(&["gh", "issue", "list"], "gh issue list"));
}

#[test]
fn test_gh_run_list_read_only() {
    assert!(is_ro(&["gh", "run", "list"], "gh run list"));
}

#[test]
fn test_gh_pr_merge_not_read_only() {
    assert!(is_not_ro(&["gh", "pr", "merge", "123"], "gh pr merge 123"));
}

#[test]
fn test_gh_api_get_read_only() {
    assert!(is_ro(
        &["gh", "api", "/repos/owner/repo"],
        "gh api /repos/owner/repo"
    ));
}

#[test]
fn test_gh_api_post_not_read_only() {
    assert!(is_not_ro(
        &["gh", "api", "POST", "/repos"],
        "gh api POST /repos"
    ));
}

// --- DOCKER ---

#[test]
fn test_docker_ps_read_only() {
    assert!(is_ro(&["docker", "ps"], "docker ps"));
    assert!(is_ro(&["docker", "ps", "-a"], "docker ps -a"));
}

#[test]
fn test_docker_images_read_only() {
    assert!(is_ro(&["docker", "images"], "docker images"));
}

#[test]
fn test_docker_inspect_read_only() {
    assert!(is_ro(
        &["docker", "inspect", "container"],
        "docker inspect container"
    ));
}

#[test]
fn test_docker_run_not_read_only() {
    assert!(is_not_ro(&["docker", "run", "ubuntu"], "docker run ubuntu"));
}

// --- RIPGREP ---

#[test]
fn test_rg_read_only() {
    assert!(is_ro(&["rg", "pattern"], "rg pattern"));
    assert!(is_ro(
        &["rg", "-n", "pattern", "src/"],
        "rg -n pattern src/"
    ));
    assert!(is_ro(&["rg", "--json", "pattern"], "rg --json pattern"));
}

#[test]
fn test_pyright_write_flags_not_read_only() {
    assert!(is_not_ro(
        &["pyright", "--createstub", "os"],
        "pyright --createstub os"
    ));
    assert!(is_not_ro(&["pyright", "--watch"], "pyright --watch"));
}

// --- EXTERNAL ---

#[test]
fn test_ls_read_only() {
    assert!(is_ro(&["ls"], "ls"));
    assert!(is_ro(&["ls", "-la"], "ls -la"));
}

#[test]
fn test_cat_read_only() {
    assert!(is_ro(&["cat", "file.txt"], "cat file.txt"));
}

#[test]
fn test_echo_read_only() {
    assert!(is_ro(&["echo", "hello"], "echo hello"));
}

#[test]
fn test_unknown_command_unsupported() {
    assert!(is_unsupported(
        &["unknown_cmd", "--flag"],
        "unknown_cmd --flag"
    ));
}

// --- raw shell command text ---

#[test]
fn test_bash_text_read_only_safe_simple_command() {
    assert_eq!(
        is_read_only_bash_command("git status --short"),
        ReadOnlyResult::ReadOnly
    );
    assert_eq!(
        is_read_only_bash_command("rg pattern src"),
        ReadOnlyResult::ReadOnly
    );
}

#[test]
fn test_bash_text_blocks_shell_syntax() {
    assert!(matches!(
        is_read_only_bash_command("git status > out.txt"),
        ReadOnlyResult::NotReadOnly(_)
    ));
    assert!(matches!(
        is_read_only_bash_command("git status && git diff"),
        ReadOnlyResult::NotReadOnly(_)
    ));
    assert!(matches!(
        is_read_only_bash_command("echo $(git status)"),
        ReadOnlyResult::NotReadOnly(_)
    ));
}

#[test]
fn test_powershell_text_read_only_is_conservative() {
    assert_eq!(
        is_read_only_powershell_command("git status"),
        ReadOnlyResult::ReadOnly
    );
    assert!(matches!(
        is_read_only_powershell_command("git status | Select-Object -First 1"),
        ReadOnlyResult::NotReadOnly(_)
    ));
}

// --- UNC path ---

#[test]
fn test_vulnerable_unc_path_detected() {
    assert!(contains_vulnerable_unc_path(
        r"copy \\attacker\share\file.txt"
    ));
}

#[test]
fn test_extended_length_unc_not_vulnerable() {
    assert!(!contains_vulnerable_unc_path(r"\\?\C:\path\to\file.txt"));
}

#[test]
fn test_no_unc_path_not_vulnerable() {
    assert!(!contains_vulnerable_unc_path(r"cat /etc/passwd"));
}

// --- verify that Plan mode can use this ---

#[test]
fn test_read_only_commands_safe_for_plan_mode() {
    // These should all be ReadOnly
    let safe_commands = vec![
        vec!["git", "status"],
        vec!["git", "diff"],
        vec!["git", "log", "--oneline"],
        vec!["git", "show", "HEAD"],
        vec!["git", "describe"],
        vec!["git", "branch", "-a"],
        vec!["gh", "pr", "view", "123"],
        vec!["rg", "pattern"],
        vec!["ls", "-la"],
        vec!["cat", "file"],
        vec!["docker", "ps"],
    ];
    for argv in safe_commands {
        let raw = argv.join(" ");
        assert!(
            is_ro(&argv.iter().map(|s| *s).collect::<Vec<&str>>(), &raw),
            "expected ReadOnly for: {}",
            raw
        );
    }
}

#[test]
fn test_write_commands_blocked_in_plan_mode() {
    // These should NOT be ReadOnly
    let dangerous_commands = vec![
        vec!["git", "clean", "-fd"],
        vec!["git", "checkout", "branch"],
        vec!["git", "push"],
        vec!["git", "commit", "-m", "msg"],
        vec!["git", "reset", "--hard"],
        vec!["git", "branch", "-d", "old"],
        vec!["git", "fetch", "--all"],
        vec!["gh", "pr", "merge", "123"],
        vec!["docker", "run", "ubuntu"],
    ];
    for argv in dangerous_commands {
        let raw = argv.join(" ");
        assert!(
            is_not_ro(&argv.iter().map(|s| *s).collect::<Vec<&str>>(), &raw)
                || is_unsupported(&argv.iter().map(|s| *s).collect::<Vec<&str>>(), &raw),
            "expected NOT ReadOnly for: {}",
            raw
        );
    }
}
