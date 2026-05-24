//! Loaded-thread tree filtering for resumed multi-agent sessions.

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedThread {
    pub thread_id: String,
    pub parent_thread_id: Option<String>,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSubagentThread {
    pub thread_id: String,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
}

pub fn find_loaded_subagent_threads_for_primary(
    threads: &[LoadedThread],
    primary_thread_id: &str,
) -> Vec<LoadedSubagentThread> {
    let mut included = HashSet::new();
    let mut pending = vec![primary_thread_id.to_string()];

    while let Some(parent_id) = pending.pop() {
        for thread in threads {
            if included.contains(&thread.thread_id) {
                continue;
            }
            if thread.parent_thread_id.as_deref() == Some(parent_id.as_str()) {
                included.insert(thread.thread_id.clone());
                pending.push(thread.thread_id.clone());
            }
        }
    }

    let mut result = threads
        .iter()
        .filter(|thread| included.contains(&thread.thread_id))
        .map(|thread| LoadedSubagentThread {
            thread_id: thread.thread_id.clone(),
            agent_nickname: thread.agent_nickname.clone(),
            agent_role: thread.agent_role.clone(),
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_descendant_threads_only() {
        let threads = vec![
            LoadedThread {
                thread_id: "child".into(),
                parent_thread_id: Some("root".into()),
                agent_nickname: None,
                agent_role: None,
            },
            LoadedThread {
                thread_id: "grandchild".into(),
                parent_thread_id: Some("child".into()),
                agent_nickname: None,
                agent_role: None,
            },
            LoadedThread {
                thread_id: "other".into(),
                parent_thread_id: Some("different".into()),
                agent_nickname: None,
                agent_role: None,
            },
        ];
        let found = find_loaded_subagent_threads_for_primary(&threads, "root");
        assert_eq!(found.len(), 2);
    }
}
