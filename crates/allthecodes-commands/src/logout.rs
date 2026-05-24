//! `/logout` command — clear stored authentication credentials AND the
//! session-derived state that identity bleeds into (issue #43).
//!
//! The narrow predecessor wiped auth material only. The expanded version
//! walks every location where identity-derived state lives and reports the
//! outcome of each step, so users can see exactly what was purged and what
//! still needs manual attention:
//!
//! - Auth: keychain API key + `credentials.json` OAuth tokens.
//! - Onboarding: the `/onboarding` wizard flag + completion stamp.
//! - Environment: `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` are flagged
//!   when set — we cannot unset them in the parent shell so we surface
//!   them as a follow-up.
//! - Managed settings: pointed at but never touched — those are policy
//!   files placed by administrators, not user-scoped caches.
//!
//! The command reports a structured multi-line result and returns success
//! even when there was nothing to clear (the action is idempotent).

use anyhow::Result;
use async_trait::async_trait;
use std::sync::{OnceLock, RwLock};

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_auth as auth;

pub struct LogoutHandler;

#[async_trait]
impl CommandHandler for LogoutHandler {
    async fn execute(&self, _args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let auth_resolution = auth::try_resolve_auth().map_err(|e| format!("{e:#}"));
        let report = run_logout_after_auth_resolution(auth_resolution);
        Ok(CommandResult::Output(report.render()))
    }
}

// ---------------------------------------------------------------------------
// Core reset flow
// ---------------------------------------------------------------------------

/// Structured result of one `/logout` invocation. Rendered to a human-
/// readable report for the user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogoutReport {
    pub was_authenticated: bool,
    pub auth_resolution_diagnostic: Option<String>,
    pub auth_cleared: StepStatus,
    pub onboarding_cleared: StepStatus,
    pub env_override_warning: Option<String>,
    pub managed_settings_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StepStatus {
    /// Nothing to do (state was already clean).
    #[default]
    NoOp,
    /// The step successfully cleared state.
    Cleared,
    /// The step failed with the given error message.
    Failed(String),
}

pub type OnboardingLogoutClearer = fn() -> StepStatus;

static ONBOARDING_LOGOUT_CLEARER: OnceLock<RwLock<Option<OnboardingLogoutClearer>>> =
    OnceLock::new();

/// Install the runtime-owned onboarding reset hook used by `/logout`.
///
/// The root crate owns the onboarding store today; cc-commands only depends on
/// this small adapter so it does not reach into root-private services.
pub fn set_onboarding_logout_clearer(clearer: OnboardingLogoutClearer) {
    let slot = ONBOARDING_LOGOUT_CLEARER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(clearer);
    }
}

impl StepStatus {
    pub fn tag(&self) -> &'static str {
        match self {
            StepStatus::NoOp => "—",
            StepStatus::Cleared => "✓",
            StepStatus::Failed(_) => "!",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            StepStatus::NoOp => "nothing to clear".to_string(),
            StepStatus::Cleared => "cleared".to_string(),
            StepStatus::Failed(msg) => format!("failed: {}", msg),
        }
    }
}

impl LogoutReport {
    pub fn render(&self) -> String {
        let mut out = String::new();

        if !self.was_authenticated
            && self.auth_resolution_diagnostic.is_none()
            && matches!(self.auth_cleared, StepStatus::NoOp)
            && matches!(self.onboarding_cleared, StepStatus::NoOp)
        {
            out.push_str(
                "Not currently authenticated and no onboarding state — nothing to clear.\n",
            );
        } else {
            out.push_str("Logout complete. Cleanup summary:\n");
        }

        out.push_str(&format!(
            "  {} Auth credentials ({}, {})\n",
            self.auth_cleared.tag(),
            if self.auth_resolution_diagnostic.is_some() {
                "present but invalid"
            } else if self.was_authenticated {
                "keychain + credentials.json"
            } else {
                "none was present"
            },
            self.auth_cleared.detail()
        ));
        if let Some(diagnostic) = &self.auth_resolution_diagnostic {
            out.push_str(&format!(
                "    diagnostic: auth resolution failed: {}\n",
                diagnostic
            ));
        }
        out.push_str(&format!(
            "  {} Onboarding state ({}, {})\n",
            self.onboarding_cleared.tag(),
            "onboarding.json",
            self.onboarding_cleared.detail()
        ));

        if let Some(warn) = &self.env_override_warning {
            out.push_str(&format!(
                "\nHeads up: {} — the environment still authenticates; \
                 unset it in your shell before restarting allthecodes.\n",
                warn
            ));
        }

        if let Some(managed) = &self.managed_settings_path {
            out.push_str(&format!(
                "\nManaged (policy) settings at {} were NOT touched — they are \
                 administrator-owned and outside the scope of /logout.\n",
                managed
            ));
        }

        out
    }
}

/// Run the full logout sequence. Parameterized for testability — unit tests
/// substitute runtime clearers and a pre-built auth state.
fn run_logout(current_auth: &auth::AuthMethod) -> LogoutReport {
    run_logout_with_diagnostic(current_auth, None)
}

fn run_logout_after_auth_resolution(
    auth_resolution: Result<auth::AuthMethod, String>,
) -> LogoutReport {
    match auth_resolution {
        Ok(current_auth) => run_logout(&current_auth),
        Err(diagnostic) => run_logout_with_diagnostic(&auth::AuthMethod::None, Some(diagnostic)),
    }
}

fn run_logout_with_diagnostic(
    current_auth: &auth::AuthMethod,
    auth_resolution_diagnostic: Option<String>,
) -> LogoutReport {
    run_logout_with_clearers(
        current_auth,
        auth_resolution_diagnostic,
        auth::oauth_logout,
        clear_onboarding_for_logout,
    )
}

fn run_logout_with_clearers<F, G>(
    current_auth: &auth::AuthMethod,
    auth_resolution_diagnostic: Option<String>,
    clear_auth: F,
    clear_onboarding: G,
) -> LogoutReport
where
    F: FnOnce() -> anyhow::Result<()>,
    G: FnOnce() -> StepStatus,
{
    let was_authenticated = current_auth.is_authenticated();
    let has_invalid_present_auth = auth_resolution_diagnostic.is_some();

    let auth_cleared = if was_authenticated || has_invalid_present_auth {
        match clear_auth() {
            Ok(_) => StepStatus::Cleared,
            Err(e) => StepStatus::Failed(e.to_string()),
        }
    } else {
        StepStatus::NoOp
    };

    let onboarding_cleared = clear_onboarding();

    let env_override_warning = detect_env_override();
    let managed_settings_path = detect_managed_settings();

    LogoutReport {
        was_authenticated,
        auth_resolution_diagnostic,
        auth_cleared,
        onboarding_cleared,
        env_override_warning,
        managed_settings_path,
    }
}

fn clear_onboarding_for_logout() -> StepStatus {
    crate::runtime::ensure_runtime_installed();
    let Some(slot) = ONBOARDING_LOGOUT_CLEARER.get() else {
        return StepStatus::Failed(
            "onboarding runtime adapter is not installed; root must inject a logout reset hook"
                .to_string(),
        );
    };
    match slot.read() {
        Ok(guard) => match *guard {
            Some(clearer) => clearer(),
            None => StepStatus::Failed(
                "onboarding runtime adapter is empty; root must inject a logout reset hook"
                    .to_string(),
            ),
        },
        Err(_) => StepStatus::Failed("onboarding runtime adapter lock is poisoned".to_string()),
    }
}

fn detect_env_override() -> Option<String> {
    const VARS: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];
    let present: Vec<&str> = VARS
        .iter()
        .copied()
        .filter(|var| {
            std::env::var(var)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        })
        .collect();
    if present.is_empty() {
        None
    } else {
        Some(format!("{} is set in the environment", present.join(", ")))
    }
}

fn detect_managed_settings() -> Option<String> {
    let path = allthecodes_config::settings::managed_settings_path();
    if path.exists() {
        Some(path.display().to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::cell::Cell;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_logout_returns_output() {
        let handler = LogoutHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(!text.is_empty());
            }
            _ => panic!("Expected Output"),
        }
    }

    #[test]
    fn unauthenticated_and_empty_onboarding_is_no_op_report() {
        let report = run_logout_with_clearers(
            &auth::AuthMethod::None,
            None,
            || Ok(()),
            || StepStatus::NoOp,
        );
        assert!(!report.was_authenticated);
        assert!(matches!(report.auth_cleared, StepStatus::NoOp));
        assert!(matches!(report.onboarding_cleared, StepStatus::NoOp));
        let text = report.render();
        assert!(text.contains("nothing to clear"));
    }

    #[test]
    fn clears_onboarding_when_present_even_if_unauthenticated() {
        let cleared = Cell::new(false);
        let report = run_logout_with_clearers(
            &auth::AuthMethod::None,
            None,
            || Ok(()),
            || {
                cleared.set(true);
                StepStatus::Cleared
            },
        );
        assert!(matches!(report.onboarding_cleared, StepStatus::Cleared));
        assert!(cleared.get());
    }

    #[test]
    fn render_lists_every_step() {
        let report = LogoutReport {
            was_authenticated: true,
            auth_resolution_diagnostic: None,
            auth_cleared: StepStatus::Cleared,
            onboarding_cleared: StepStatus::Cleared,
            env_override_warning: None,
            managed_settings_path: None,
        };
        let text = report.render();
        assert!(text.contains("Auth credentials"));
        assert!(text.contains("Onboarding state"));
        assert!(text.contains("Logout complete"));
    }

    #[test]
    fn render_shows_env_override_warning_when_set() {
        let report = LogoutReport {
            env_override_warning: Some("ANTHROPIC_API_KEY is set in the environment".into()),
            ..LogoutReport::default()
        };
        let text = report.render();
        assert!(text.contains("ANTHROPIC_API_KEY"));
        assert!(text.contains("unset it"));
    }

    #[test]
    fn render_notes_managed_settings_when_present() {
        let report = LogoutReport {
            managed_settings_path: Some("/etc/allthecodes/managed-settings.json".into()),
            ..LogoutReport::default()
        };
        let text = report.render();
        assert!(text.contains("Managed (policy) settings"));
        assert!(text.contains("NOT touched"));
    }

    #[test]
    fn auth_resolution_diagnostic_is_visible_and_still_attempts_clear() {
        let report = run_logout_with_clearers(
            &auth::AuthMethod::None,
            Some("ANTHROPIC_API_KEY is present but has an invalid API key format".into()),
            || Ok(()),
            || StepStatus::NoOp,
        );

        assert!(!report.was_authenticated);
        assert!(report.auth_resolution_diagnostic.is_some());
        assert!(matches!(report.auth_cleared, StepStatus::Cleared));
        let text = report.render();
        assert!(text.contains("present but invalid"));
        assert!(text.contains("ANTHROPIC_API_KEY"));
        assert!(text.starts_with("Logout complete."));
    }

    #[test]
    fn status_detail_strings_are_informative() {
        assert_eq!(StepStatus::NoOp.detail(), "nothing to clear");
        assert_eq!(StepStatus::Cleared.detail(), "cleared");
        assert!(StepStatus::Failed("disk full".into())
            .detail()
            .contains("disk full"));
    }

    #[test]
    fn missing_onboarding_adapter_is_reported() {
        let status = clear_onboarding_for_logout();
        assert!(matches!(status, StepStatus::Failed(_)));
        assert!(status.detail().contains("adapter"));
    }
}
