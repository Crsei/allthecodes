//! Root-owned worktree tool runtime.
//!
//! This remains in the root crate until worktree hook policy and status-line
//! session state have a shared owner.

pub mod delegate;
pub mod tool;

pub use delegate::{delegate_worktree_plan, DelegateWorktreePlan};
pub use tool::get_current_worktree_session;
pub use tool::WorktreeSession;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn delegate_worktree_plan_stays_under_allthecodes_worktree_root() {
        let home = TempDir::new().expect("temp home");
        let _home_guard = EnvGuard::set("ALLTHECODES_HOME", home.path());

        let plan = delegate_worktree_plan("child-session-12345678", Some("feature-a"))
            .expect("delegate worktree plan");

        assert_eq!(plan.branch_name, "agent-worktree-feature-a");
        assert_eq!(
            plan.worktree_path,
            home.path()
                .join("worktrees")
                .join("agent-worktree-feature-a")
        );
    }

    #[test]
    #[serial_test::serial]
    fn delegate_worktree_plan_rejects_path_traversal_slug() {
        let home = TempDir::new().expect("temp home");
        let _home_guard = EnvGuard::set("ALLTHECODES_HOME", home.path());

        let err = delegate_worktree_plan("child-session-12345678", Some("../escape"))
            .expect_err("path traversal must be rejected");

        assert!(err.to_string().contains("path separators"));
    }
}
