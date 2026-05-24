use std::path::PathBuf;

fn data_root_from_env() -> Option<PathBuf> {
    let home = std::env::var("ALLTHECODES_HOME").ok()?;
    let home = home.trim();
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".allthecodes"))
}

pub(crate) fn teams_dir() -> PathBuf {
    data_root_from_env()
        .map(|root| root.join("teams"))
        .unwrap_or_else(allthecodes_config::paths::teams_dir)
}

pub(crate) fn tasks_dir() -> PathBuf {
    data_root_from_env()
        .map(|root| root.join("tasks"))
        .unwrap_or_else(allthecodes_config::paths::tasks_dir)
}

pub(crate) fn pr_activity_subscriptions_path() -> PathBuf {
    data_root_from_env()
        .map(|root| root.join("pr-activity-subscriptions.json"))
        .unwrap_or_else(allthecodes_config::paths::pr_activity_subscriptions_path)
}
