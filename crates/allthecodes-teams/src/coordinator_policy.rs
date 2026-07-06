#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorPolicySnapshot {
    pub max_parallel_workers: usize,
    pub max_retry_per_task: usize,
    pub max_worker_turns: Option<usize>,
}

impl CoordinatorPolicySnapshot {
    pub fn from_env() -> Self {
        Self {
            max_parallel_workers: read_positive_usize_env(
                "ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS",
                4,
            ),
            max_retry_per_task: read_positive_usize_env(
                "ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK",
                1,
            ),
            max_worker_turns: Some(read_positive_usize_env(
                "ALLTHECODES_COORDINATOR_MAX_WORKER_TURNS",
                12,
            )),
        }
    }

    pub fn prompt_fragment(&self) -> String {
        format!(
            "- Maximum parallel workers: {}\n- Retry failed worker tasks at most {} time(s) unless the user explicitly asks for more.\n- Worker max turns: {}.\n",
            self.max_parallel_workers,
            self.max_retry_per_task,
            self.max_worker_turns
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unlimited".into())
        )
    }
}

fn read_positive_usize_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_policy_defaults_are_conservative() {
        let _parallel = EnvGuard::remove("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS");
        let _retry = EnvGuard::remove("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK");
        let _turns = EnvGuard::remove("ALLTHECODES_COORDINATOR_MAX_WORKER_TURNS");

        let policy = CoordinatorPolicySnapshot::from_env();

        assert_eq!(policy.max_parallel_workers, 4);
        assert_eq!(policy.max_retry_per_task, 1);
        assert_eq!(policy.max_worker_turns, Some(12));
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_policy_rejects_zero_values() {
        let _parallel = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS", "0");
        let _retry = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK", "0");
        let _turns = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_WORKER_TURNS", "0");

        let policy = CoordinatorPolicySnapshot::from_env();

        assert_eq!(policy.max_parallel_workers, 4);
        assert_eq!(policy.max_retry_per_task, 1);
        assert_eq!(policy.max_worker_turns, Some(12));
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_policy_prompt_fragment_uses_runtime_values() {
        let _parallel = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS", "2");
        let _retry = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK", "3");
        let _turns = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_WORKER_TURNS", "7");

        let fragment = CoordinatorPolicySnapshot::from_env().prompt_fragment();

        assert!(fragment.contains("Maximum parallel workers: 2"));
        assert!(fragment.contains("at most 3 time(s)"));
        assert!(fragment.contains("Worker max turns: 7."));
    }
}
