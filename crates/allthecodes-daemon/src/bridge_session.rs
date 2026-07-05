use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::process_state::{self, DaemonBridgeSessionState};
use crate::supervisor::ASSISTANT_WORKER_ID;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionIdentity {
    pub cwd: PathBuf,
    pub account_id: Option<String>,
    pub profile: Option<String>,
    pub terminal_id: Option<String>,
    pub remote_session_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSessionReusePolicy {
    ReuseWorkspace,
    NewSession,
    ExplicitSession(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionLease {
    pub owner: String,
    pub ttl: Duration,
    pub allow_stale_takeover: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionLeaseConflict {
    pub session_id: String,
    pub lease_owner: String,
    pub lease_expires_at: Option<DateTime<Utc>>,
}

impl fmt::Display for BridgeSessionLeaseConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.lease_expires_at {
            Some(expires_at) => write!(
                f,
                "active bridge session lease for {} is owned by {} until {}",
                self.session_id, self.lease_owner, expires_at
            ),
            None => write!(
                f,
                "active bridge session lease for {} is owned by {}",
                self.session_id, self.lease_owner
            ),
        }
    }
}

impl std::error::Error for BridgeSessionLeaseConflict {}

pub fn derive_workspace_key(identity: &BridgeSessionIdentity) -> Result<String> {
    let cwd = identity
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| identity.cwd.clone());
    Ok(format!(
        "cwd={}|account={}|profile={}",
        cwd.display(),
        identity.account_id.as_deref().unwrap_or(""),
        identity.profile.as_deref().unwrap_or("")
    ))
}

pub fn select_or_create_bridge_session(
    identity: BridgeSessionIdentity,
    policy: BridgeSessionReusePolicy,
    lease: BridgeSessionLease,
) -> Result<DaemonBridgeSessionState> {
    let workspace_key = derive_workspace_key(&identity)?;
    let state = match policy {
        BridgeSessionReusePolicy::ExplicitSession(session_id) => {
            process_state::read_bridge_session_state(&session_id)?
                .with_context(|| format!("bridge session not found: {session_id}"))?
        }
        BridgeSessionReusePolicy::NewSession => new_bridge_session_state(&identity, workspace_key),
        BridgeSessionReusePolicy::ReuseWorkspace => {
            match process_state::find_bridge_session_by_workspace_key(
                &workspace_key,
                identity.account_id.as_deref(),
                identity.profile.as_deref(),
            )? {
                Some(state) => state,
                None => new_bridge_session_state(&identity, workspace_key),
            }
        }
    };
    lease_bridge_session(state, &lease)
}

pub fn refresh_bridge_session_lease(
    session_id: &str,
    owner: &str,
    ttl: Duration,
) -> Result<DaemonBridgeSessionState> {
    let state = process_state::read_bridge_session_state(session_id)?
        .with_context(|| format!("bridge session not found: {session_id}"))?;
    lease_bridge_session(
        state,
        &BridgeSessionLease {
            owner: owner.to_string(),
            ttl,
            allow_stale_takeover: false,
        },
    )
}

pub fn release_bridge_session_lease(session_id: &str, owner: &str) -> Result<()> {
    let Some(mut state) = process_state::read_bridge_session_state(session_id)? else {
        return Ok(());
    };
    if state.lease_owner.as_deref() == Some(owner) {
        state.lease_owner = None;
        state.lease_expires_at = None;
        process_state::write_bridge_session_state(&state)?;
    }
    Ok(())
}

fn new_bridge_session_state(
    identity: &BridgeSessionIdentity,
    workspace_key: String,
) -> DaemonBridgeSessionState {
    let now = Utc::now();
    DaemonBridgeSessionState {
        schema_version: 2,
        session_id: uuid::Uuid::new_v4().to_string(),
        account_id: identity.account_id.clone(),
        profile: identity.profile.clone(),
        cwd: identity.cwd.clone(),
        assistant_worker_id: ASSISTANT_WORKER_ID.to_string(),
        last_poll_cursor: None,
        last_ack_at: None,
        updated_at: now,
        workspace_key,
        terminal_id: identity.terminal_id.clone(),
        remote_session_key: identity.remote_session_key.clone(),
        assistant_session_id: None,
        last_run_id: None,
        lease_owner: None,
        lease_expires_at: None,
    }
}

fn lease_bridge_session(
    mut state: DaemonBridgeSessionState,
    lease: &BridgeSessionLease,
) -> Result<DaemonBridgeSessionState> {
    if let Some(conflict) = active_foreign_lease(&state, &lease.owner) {
        return Err(conflict.into());
    }
    if has_foreign_stale_lease(&state, &lease.owner) && !lease.allow_stale_takeover {
        return Err(BridgeSessionLeaseConflict {
            session_id: state.session_id,
            lease_owner: state.lease_owner.unwrap_or_default(),
            lease_expires_at: state.lease_expires_at,
        }
        .into());
    }

    state.lease_owner = Some(lease.owner.clone());
    state.lease_expires_at = Some(
        Utc::now()
            + chrono::Duration::from_std(lease.ttl)
                .context("bridge session lease ttl is out of range")?,
    );
    process_state::write_bridge_session_state(&state)
}

fn active_foreign_lease(
    state: &DaemonBridgeSessionState,
    owner: &str,
) -> Option<BridgeSessionLeaseConflict> {
    let lease_owner = state.lease_owner.as_deref()?;
    if lease_owner == owner {
        return None;
    }
    let expires_at = state.lease_expires_at?;
    if expires_at <= Utc::now() {
        return None;
    }
    Some(BridgeSessionLeaseConflict {
        session_id: state.session_id.clone(),
        lease_owner: lease_owner.to_string(),
        lease_expires_at: Some(expires_at),
    })
}

fn has_foreign_stale_lease(state: &DaemonBridgeSessionState, owner: &str) -> bool {
    state
        .lease_owner
        .as_deref()
        .is_some_and(|lease_owner| lease_owner != owner)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use chrono::Utc;
    use serial_test::serial;

    use super::*;
    use crate::process_state::{self, DaemonBridgeSessionState};
    use crate::supervisor::ASSISTANT_WORKER_ID;

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

    fn identity(cwd: &Path) -> BridgeSessionIdentity {
        BridgeSessionIdentity {
            cwd: cwd.to_path_buf(),
            account_id: Some("acct_1".to_string()),
            profile: Some("default".to_string()),
            terminal_id: None,
            remote_session_key: None,
        }
    }

    fn lease(owner: &str, allow_stale_takeover: bool) -> BridgeSessionLease {
        BridgeSessionLease {
            owner: owner.to_string(),
            ttl: Duration::from_secs(60),
            allow_stale_takeover,
        }
    }

    fn stored_session(
        cwd: &Path,
        session_id: &str,
        owner: Option<&str>,
    ) -> DaemonBridgeSessionState {
        let workspace_key = derive_workspace_key(&identity(cwd)).unwrap();
        DaemonBridgeSessionState {
            schema_version: 2,
            session_id: session_id.to_string(),
            account_id: Some("acct_1".to_string()),
            profile: Some("default".to_string()),
            cwd: cwd.to_path_buf(),
            assistant_worker_id: ASSISTANT_WORKER_ID.to_string(),
            last_poll_cursor: None,
            last_ack_at: None,
            updated_at: Utc::now(),
            workspace_key,
            terminal_id: None,
            remote_session_key: None,
            assistant_session_id: None,
            last_run_id: None,
            lease_owner: owner.map(str::to_string),
            lease_expires_at: owner.map(|_| Utc::now() + chrono::Duration::seconds(60)),
        }
    }

    #[test]
    #[serial]
    fn reuse_workspace_selects_existing_session() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        process_state::write_bridge_session_state(&stored_session(&cwd, "existing", None)).unwrap();

        let selected = select_or_create_bridge_session(
            identity(&cwd),
            BridgeSessionReusePolicy::ReuseWorkspace,
            lease("owner-a", true),
        )
        .unwrap();

        assert_eq!(selected.session_id, "existing");
        assert_eq!(selected.lease_owner.as_deref(), Some("owner-a"));
        assert!(selected.lease_expires_at.is_some());
    }

    #[test]
    #[serial]
    fn new_session_always_creates_distinct_session() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();

        let first = select_or_create_bridge_session(
            identity(&cwd),
            BridgeSessionReusePolicy::NewSession,
            lease("owner-a", true),
        )
        .unwrap();
        let second = select_or_create_bridge_session(
            identity(&cwd),
            BridgeSessionReusePolicy::NewSession,
            lease("owner-a", true),
        )
        .unwrap();

        assert_ne!(first.session_id, second.session_id);
        assert_eq!(first.workspace_key, second.workspace_key);
    }

    #[test]
    #[serial]
    fn explicit_session_loads_exact_session() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        process_state::write_bridge_session_state(&stored_session(&cwd, "exact", None)).unwrap();

        let selected = select_or_create_bridge_session(
            identity(&cwd),
            BridgeSessionReusePolicy::ExplicitSession("exact".to_string()),
            lease("owner-a", true),
        )
        .unwrap();

        assert_eq!(selected.session_id, "exact");
        assert_eq!(selected.lease_owner.as_deref(), Some("owner-a"));
    }

    #[test]
    #[serial]
    fn active_foreign_lease_blocks_reuse() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        process_state::write_bridge_session_state(&stored_session(&cwd, "leased", Some("owner-a")))
            .unwrap();

        let error = select_or_create_bridge_session(
            identity(&cwd),
            BridgeSessionReusePolicy::ReuseWorkspace,
            lease("owner-b", true),
        )
        .expect_err("active foreign lease should conflict");

        assert!(error.to_string().contains("active bridge session lease"));
    }

    #[test]
    #[serial]
    fn stale_foreign_lease_can_be_taken_over() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        let mut session = stored_session(&cwd, "stale", Some("owner-a"));
        session.lease_expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
        process_state::write_bridge_session_state(&session).unwrap();

        let selected = select_or_create_bridge_session(
            identity(&cwd),
            BridgeSessionReusePolicy::ReuseWorkspace,
            lease("owner-b", true),
        )
        .unwrap();

        assert_eq!(selected.session_id, "stale");
        assert_eq!(selected.lease_owner.as_deref(), Some("owner-b"));
        assert!(selected.lease_expires_at.unwrap() > Utc::now());
    }
}
