//! Shell-agnostic git operation tracking.
//!
//! Mirrors the TypeScript `tools/shared/gitOperationTracking.ts` helper for
//! Bash and PowerShell. The Rust port does not currently have the upstream
//! network analytics pipeline, so this module returns structured metadata that
//! shell tools can attach to their result payloads and that future local
//! analytics/session-linking code can reuse.

use std::sync::LazyLock;

use serde_json::{json, Map, Value};

use crate::safe_regex::SafeRegex;

static GIT_COMMIT_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgit(?:\s+-[cC]\s+\S+|\s+--\S+=\S+)*\s+commit\b"));
static GIT_PUSH_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgit(?:\s+-[cC]\s+\S+|\s+--\S+=\S+)*\s+push\b"));
static GIT_CHERRY_PICK_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgit(?:\s+-[cC]\s+\S+|\s+--\S+=\S+)*\s+cherry-pick\b"));
static GIT_MERGE_RE: LazyLock<SafeRegex> = LazyLock::new(|| {
    SafeRegex::new(r"\bgit(?:\s+-[cC]\s+\S+|\s+--\S+=\S+)*\s+merge(?:\s|$|[;&|><])")
});
static GIT_REBASE_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgit(?:\s+-[cC]\s+\S+|\s+--\S+=\S+)*\s+rebase\b"));
static GIT_COMMIT_ID_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\[[\w./-]+(?: \(root-commit\))? ([0-9a-fA-F]+)\]"));
static GIT_PUSH_BRANCH_RE: LazyLock<SafeRegex> = LazyLock::new(|| {
    SafeRegex::new(r"(?m)^\s*[+\-*!= ]?\s*(?:\[new branch\]|\S+\.\.+\S+)\s+\S+\s*->\s*(\S+)")
});
static GITHUB_PR_URL_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"https://github\.com/([^/\s]+/[^/\s]+)/pull/(\d+)"));
static GITHUB_PR_URL_ANYWHERE_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"https://github\.com/[^/\s]+/[^/\s]+/pull/\d+"));
static PR_NUMBER_TEXT_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"[Pp]ull request (?:\S+#)?#?(\d+)"));
static GH_PR_CREATE_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgh\s+pr\s+create\b"));
static GH_PR_EDIT_RE: LazyLock<SafeRegex> = LazyLock::new(|| SafeRegex::new(r"\bgh\s+pr\s+edit\b"));
static GH_PR_MERGE_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgh\s+pr\s+merge\b"));
static GH_PR_COMMENT_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgh\s+pr\s+comment\b"));
static GH_PR_CLOSE_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgh\s+pr\s+close\b"));
static GH_PR_READY_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bgh\s+pr\s+ready\b"));
static GLAB_MR_CREATE_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"\bglab\s+mr\s+create\b"));
static CURL_RE: LazyLock<SafeRegex> = LazyLock::new(|| SafeRegex::new(r"\bcurl\b"));
static CURL_POST_RE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"(?i)(?:-X\s*POST\b|--request\s*=?\s*POST\b|\s-d\s)"));
static PR_ENDPOINT_RE: LazyLock<SafeRegex> = LazyLock::new(|| {
    SafeRegex::new(
        r#"(?i)https?://[^\s'"]*/(?:pulls|pull-requests|merge[-_]requests)(?:[^\w/-]|$)"#,
    )
});
static AMEND_RE: LazyLock<SafeRegex> = LazyLock::new(|| SafeRegex::new(r"--amend\b"));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitOperationTracking {
    pub operations: Vec<&'static str>,
    pub commit: Option<GitCommitOperation>,
    pub push: Option<GitPushOperation>,
    pub branch: Option<GitBranchOperation>,
    pub pr: Option<GitPrOperation>,
}

impl GitOperationTracking {
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
            && self.commit.is_none()
            && self.push.is_none()
            && self.branch.is_none()
            && self.pr.is_none()
    }

    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("operations".to_string(), json!(self.operations));
        if let Some(commit) = &self.commit {
            object.insert(
                "commit".to_string(),
                json!({
                    "sha": commit.sha,
                    "kind": commit.kind,
                }),
            );
        }
        if let Some(push) = &self.push {
            object.insert(
                "push".to_string(),
                json!({
                    "branch": push.branch,
                }),
            );
        }
        if let Some(branch) = &self.branch {
            object.insert(
                "branch".to_string(),
                json!({
                    "ref": branch.ref_name,
                    "action": branch.action,
                }),
            );
        }
        if let Some(pr) = &self.pr {
            let mut pr_object = Map::new();
            pr_object.insert("number".to_string(), json!(pr.number));
            pr_object.insert("action".to_string(), json!(pr.action));
            if let Some(url) = &pr.url {
                pr_object.insert("url".to_string(), json!(url));
            }
            if let Some(repository) = &pr.repository {
                pr_object.insert("repository".to_string(), json!(repository));
            }
            object.insert("pr".to_string(), Value::Object(pr_object));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitOperation {
    pub sha: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPushOperation {
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBranchOperation {
    pub ref_name: String,
    pub action: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPrOperation {
    pub number: u64,
    pub url: Option<String>,
    pub repository: Option<String>,
    pub action: &'static str,
}

struct GhPrAction {
    re: &'static LazyLock<SafeRegex>,
    action: &'static str,
    op: &'static str,
}

const GH_PR_ACTIONS: &[GhPrAction] = &[
    GhPrAction {
        re: &GH_PR_CREATE_RE,
        action: "created",
        op: "pr_create",
    },
    GhPrAction {
        re: &GH_PR_EDIT_RE,
        action: "edited",
        op: "pr_edit",
    },
    GhPrAction {
        re: &GH_PR_MERGE_RE,
        action: "merged",
        op: "pr_merge",
    },
    GhPrAction {
        re: &GH_PR_COMMENT_RE,
        action: "commented",
        op: "pr_comment",
    },
    GhPrAction {
        re: &GH_PR_CLOSE_RE,
        action: "closed",
        op: "pr_close",
    },
    GhPrAction {
        re: &GH_PR_READY_RE,
        action: "ready",
        op: "pr_ready",
    },
];

/// Detect successful git/PR operations from raw shell command text and output.
///
/// This mirrors upstream `trackGitOperations`: failed commands are ignored.
/// Pass stdout and stderr concatenated so git push ref updates written to
/// stderr are visible to the branch parser.
pub fn track_git_operations(
    command: &str,
    exit_code: i32,
    output: Option<&str>,
) -> Option<GitOperationTracking> {
    if exit_code != 0 {
        return None;
    }

    let output = output.unwrap_or_default();
    let mut tracking = GitOperationTracking::default();

    let is_cherry_pick = GIT_CHERRY_PICK_RE.is_match(command);
    if GIT_COMMIT_RE.is_match(command) || is_cherry_pick {
        if is_cherry_pick {
            tracking.operations.push("cherry_pick");
        } else {
            tracking.operations.push("commit");
            if AMEND_RE.is_match(command) {
                tracking.operations.push("commit_amend");
            }
        }
        if let Some(sha) = parse_git_commit_id(output) {
            tracking.commit = Some(GitCommitOperation {
                sha: sha.chars().take(6).collect(),
                kind: if is_cherry_pick {
                    "cherry-picked"
                } else if AMEND_RE.is_match(command) {
                    "amended"
                } else {
                    "committed"
                },
            });
        }
    }

    if GIT_PUSH_RE.is_match(command) {
        tracking.operations.push("push");
        if let Some(branch) = parse_git_push_branch(output) {
            tracking.push = Some(GitPushOperation { branch });
        }
    }

    if GIT_MERGE_RE.is_match(command)
        && (output.contains("Fast-forward") || output.contains("Merge made by"))
    {
        tracking.operations.push("merge");
        if let Some(ref_name) = parse_ref_from_command(command, &GIT_MERGE_RE) {
            tracking.branch = Some(GitBranchOperation {
                ref_name,
                action: "merged",
            });
        }
    }

    if GIT_REBASE_RE.is_match(command) && output.contains("Successfully rebased") {
        tracking.operations.push("rebase");
        if let Some(ref_name) = parse_ref_from_command(command, &GIT_REBASE_RE) {
            tracking.branch = Some(GitBranchOperation {
                ref_name,
                action: "rebased",
            });
        }
    }

    if let Some(action) = GH_PR_ACTIONS
        .iter()
        .find(|action| action.re.is_match(command))
    {
        tracking.operations.push(action.op);
        tracking.pr = find_pr_in_output(output, action.action);
    }

    if GLAB_MR_CREATE_RE.is_match(command) {
        tracking.operations.push("pr_create");
    }

    if CURL_RE.is_match(command)
        && CURL_POST_RE.is_match(command)
        && PR_ENDPOINT_RE.is_match(command)
    {
        tracking.operations.push("pr_create");
    }

    tracking.operations.sort_unstable();
    tracking.operations.dedup();

    if tracking.is_empty() {
        None
    } else {
        Some(tracking)
    }
}

pub fn track_git_operations_json(
    command: &str,
    exit_code: i32,
    output: Option<&str>,
) -> Option<Value> {
    track_git_operations(command, exit_code, output).map(|tracking| tracking.to_json())
}

pub fn parse_git_commit_id(output: &str) -> Option<String> {
    GIT_COMMIT_ID_RE
        .captures(output)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_ascii_lowercase())
}

fn parse_git_push_branch(output: &str) -> Option<String> {
    GIT_PUSH_BRANCH_RE
        .captures(output)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

fn find_pr_in_output(output: &str, action: &'static str) -> Option<GitPrOperation> {
    if let Some(match_) = GITHUB_PR_URL_ANYWHERE_RE.find(output) {
        if let Some(pr) = parse_pr_url(match_.as_str(), action) {
            return Some(pr);
        }
    }

    PR_NUMBER_TEXT_RE
        .captures(output)
        .and_then(|captures| captures.get(1))
        .and_then(|number| number.as_str().parse::<u64>().ok())
        .map(|number| GitPrOperation {
            number,
            url: None,
            repository: None,
            action,
        })
}

fn parse_pr_url(url: &str, action: &'static str) -> Option<GitPrOperation> {
    let captures = GITHUB_PR_URL_RE.captures(url)?;
    let repository = captures.get(1)?.as_str().to_string();
    let number = captures.get(2)?.as_str().parse::<u64>().ok()?;
    Some(GitPrOperation {
        number,
        url: Some(url.to_string()),
        repository: Some(repository),
        action,
    })
}

fn parse_ref_from_command(command: &str, command_re: &SafeRegex) -> Option<String> {
    let after = command_re.find(command).map(|m| &command[m.end()..])?;
    for token in after.split_whitespace() {
        if token.starts_with(['&', '|', ';', '>', '<']) {
            break;
        }
        if token.starts_with('-') {
            continue;
        }
        return Some(
            token
                .trim_matches(|ch| matches!(ch, '\'' | '"' | ',' | ';'))
                .to_string(),
        )
        .filter(|s| !s.is_empty());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_commit_with_global_options_and_sha() {
        let tracking = track_git_operations(
            "git -c commit.gpgsign=false -C repo commit -m fix",
            0,
            Some("[main abc1234] fix bug\n 1 file changed"),
        )
        .expect("commit should be tracked");

        assert_eq!(tracking.operations, vec!["commit"]);
        assert_eq!(
            tracking.commit,
            Some(GitCommitOperation {
                sha: "abc123".to_string(),
                kind: "committed",
            })
        );
    }

    #[test]
    fn tracks_amended_root_commit() {
        let tracking = track_git_operations(
            "git commit --amend -m init",
            0,
            Some("[main (root-commit) ABC1234] init"),
        )
        .expect("amend should be tracked");

        assert_eq!(tracking.operations, vec!["commit", "commit_amend"]);
        assert_eq!(tracking.commit.unwrap().kind, "amended");
    }

    #[test]
    fn tracks_cherry_pick_commit_output() {
        let tracking = track_git_operations(
            "git cherry-pick deadbeef",
            0,
            Some("[feature fedcba9] port fix"),
        )
        .expect("cherry-pick should be tracked");

        assert_eq!(tracking.operations, vec!["cherry_pick"]);
        assert_eq!(tracking.commit.unwrap().kind, "cherry-picked");
    }

    #[test]
    fn tracks_push_branch_from_stderr_like_output() {
        let tracking = track_git_operations(
            "git push origin HEAD",
            0,
            Some("To github.com:owner/repo.git\n * [new branch]      HEAD -> feature/demo"),
        )
        .expect("push should be tracked");

        assert_eq!(tracking.operations, vec!["push"]);
        assert_eq!(
            tracking.push,
            Some(GitPushOperation {
                branch: "feature/demo".to_string(),
            })
        );
    }

    #[test]
    fn tracks_merge_and_skips_merge_base() {
        let merge = track_git_operations(
            "git merge --no-ff origin/main",
            0,
            Some("Merge made by the 'ort' strategy."),
        )
        .expect("merge should be tracked");
        assert_eq!(
            merge.branch,
            Some(GitBranchOperation {
                ref_name: "origin/main".to_string(),
                action: "merged",
            })
        );

        let merge_base = track_git_operations("git merge-base HEAD origin/main", 0, Some("abc123"));
        assert!(merge_base.is_none());
    }

    #[test]
    fn tracks_rebase_ref_only_on_success_output() {
        let tracking = track_git_operations(
            "git rebase --autostash origin/main",
            0,
            Some("Successfully rebased and updated refs/heads/feature."),
        )
        .expect("rebase should be tracked");

        assert_eq!(
            tracking.branch,
            Some(GitBranchOperation {
                ref_name: "origin/main".to_string(),
                action: "rebased",
            })
        );
    }

    #[test]
    fn tracks_gh_pr_create_url() {
        let tracking = track_git_operations(
            "gh pr create --fill",
            0,
            Some("https://github.com/owner/repo/pull/42"),
        )
        .expect("gh pr create should be tracked");

        assert_eq!(tracking.operations, vec!["pr_create"]);
        assert_eq!(
            tracking.pr,
            Some(GitPrOperation {
                number: 42,
                url: Some("https://github.com/owner/repo/pull/42".to_string()),
                repository: Some("owner/repo".to_string()),
                action: "created",
            })
        );
    }

    #[test]
    fn tracks_gh_pr_text_action() {
        let tracking = track_git_operations(
            "gh pr merge 42 --squash",
            0,
            Some("✓ Merged pull request owner/repo#42"),
        )
        .expect("gh pr merge should be tracked");

        assert_eq!(tracking.operations, vec!["pr_merge"]);
        assert_eq!(
            tracking.pr,
            Some(GitPrOperation {
                number: 42,
                url: None,
                repository: None,
                action: "merged",
            })
        );
    }

    #[test]
    fn tracks_glab_and_curl_pr_creation_without_details() {
        let glab = track_git_operations("glab mr create --fill", 0, None)
            .expect("glab MR should count as PR creation");
        assert_eq!(glab.operations, vec!["pr_create"]);

        let curl = track_git_operations(
            "curl -d '{\"title\":\"x\"}' https://api.github.com/repos/o/r/pulls",
            0,
            None,
        )
        .expect("curl PR endpoint should count as PR creation");
        assert_eq!(curl.operations, vec!["pr_create"]);
    }

    #[test]
    fn ignores_failed_or_unrelated_commands() {
        assert!(track_git_operations(
            "git commit -m nope",
            1,
            Some("[main abc1234] should not count"),
        )
        .is_none());
        assert!(
            track_git_operations("git log -1", 0, Some("[main abc1234] old commit"),).is_none()
        );
    }

    #[test]
    fn converts_tracking_to_json() {
        let value = track_git_operations_json(
            "gh pr ready 42",
            0,
            Some("✓ Pull request owner/repo#42 is marked as ready for review"),
        )
        .expect("json should be produced");

        assert_eq!(value["operations"], json!(["pr_ready"]));
        assert_eq!(value["pr"]["number"], json!(42));
        assert_eq!(value["pr"]["action"], json!("ready"));
    }
}
