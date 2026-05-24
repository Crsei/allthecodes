//! `/voice` slash command for compatibility-only voice settings.
//!
//! This build keeps the stored `voiceEnabled` flag, language
//! normalization, and diagnostics surface, but it does not ship real
//! recording or transcription support.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_auth::{self as auth, AuthMethod};
use allthecodes_config::settings::{self, RawSettings};
use allthecodes_voice::audio::{AudioUnavailable, NullAudioBackend};
use allthecodes_voice::feasibility::{check_feasibility, Feasibility, FeasibilityReason};
use allthecodes_voice::language::normalize_language_for_stt;
use allthecodes_voice::stt::{NullTranscriptionClient, SttUnavailable};

pub struct VoiceHandler;

#[async_trait]
impl CommandHandler for VoiceHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let sub = args
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let output = match sub.as_str() {
            "" | "status" | "show" => render_status(ctx),
            "on" | "enable" => toggle(true, ctx).await,
            "off" | "disable" => toggle(false, ctx).await,
            "toggle" => {
                let next = !ctx.app_state.settings.voice_enabled.unwrap_or(false);
                toggle(next, ctx).await
            }
            "diagnose" | "doctor" => render_diagnose(ctx),
            other => usage(other),
        };
        Ok(CommandResult::Output(output))
    }
}

fn usage(other: &str) -> String {
    format!(
        "Unknown /voice subcommand '{}'.\n\nUsage:\n  \
         /voice                 - show stored config + unsupported runtime status\n  \
         /voice on | enable     - attempt to enable voice if supported\n  \
         /voice off | disable   - disable the stored voice flag\n  \
         /voice toggle          - flip the stored enabled flag when allowed\n  \
         /voice diagnose        - full environment dump",
        other
    )
}

fn render_status(ctx: &CommandContext) -> String {
    let enabled = ctx.app_state.settings.voice_enabled.unwrap_or(false);
    let lang = normalize_language_for_stt(ctx.app_state.settings.language.as_deref());
    let (auth, auth_diagnostic) = resolve_auth_for_voice();
    let feas = current_feasibility_for_auth(&auth);

    let mut out = String::new();
    out.push_str("Voice dictation\n");
    out.push_str("---------------\n");
    out.push_str("  runtime support:   unsupported in this build\n");
    out.push_str(&format!(
        "  stored enabled:    {}\n",
        if enabled { "true" } else { "false" }
    ));
    out.push_str(&format!("  dictation lang:    {}", lang.code));
    if let Some(raw) = &lang.fell_back_from {
        out.push_str(&format!(
            " (fallback from \"{}\" because the configured language is unsupported)",
            raw
        ));
    }
    out.push('\n');
    out.push_str("  push-to-talk:      disabled in this build\n");
    match &feas {
        Feasibility::Ready { backend, client } => {
            out.push_str(&format!("  audio backend:     {}\n", backend));
            out.push_str(&format!("  stt client:        {}\n", client));
            out.push_str("  feasibility:       ready\n");
        }
        Feasibility::Blocked(reason) => {
            out.push_str(&format!(
                "  feasibility:       blocked ({})\n",
                short_reason_code(reason)
            ));
            out.push_str(&format!("  blocker:           {}\n", reason.short()));
            if enabled {
                out.push_str("  note:              voiceEnabled is stored, but runtime voice remains unavailable\n");
            }
        }
    }
    out.push_str("  config path:       ");
    out.push_str(&settings::user_settings_path().display().to_string());
    out.push('\n');
    if let Some(diagnostic) = auth_diagnostic {
        out.push_str(&format!("  auth diagnostic:   {}\n", diagnostic));
    }
    out
}

fn render_diagnose(ctx: &CommandContext) -> String {
    let mut out = render_status(ctx);
    out.push('\n');
    out.push_str("Environment\n");
    out.push_str("-----------\n");
    let (auth, auth_diagnostic) = resolve_auth_for_voice();
    out.push_str(&format!("  auth method:       {}\n", auth_label(&auth)));
    if let Some(diagnostic) = auth_diagnostic {
        out.push_str(&format!("  auth diagnostic:   {}\n", diagnostic));
    }
    out.push_str(&format!(
        "  ALLTHECODES_REMOTE:    {}\n",
        std::env::var("ALLTHECODES_REMOTE").unwrap_or_else(|_| "(unset)".into())
    ));
    out.push_str(&format!(
        "  ALLTHECODES_REMOTE: {}\n",
        std::env::var("ALLTHECODES_REMOTE").unwrap_or_else(|_| "(unset)".into())
    ));
    out.push_str(&format!(
        "  language setting:  {}\n",
        ctx.app_state
            .settings
            .language
            .as_deref()
            .unwrap_or("(unset)")
    ));
    out.push('\n');
    out.push_str(
        "Compatibility note: this build preserves `/voice`, `voiceEnabled`, language \
         normalization, and keybinding parsing, but it does not support real recording \
         or transcription.\n",
    );
    out
}

async fn toggle(target: bool, ctx: &mut CommandContext) -> String {
    if !target {
        return match persist(false) {
            Ok(path) => {
                ctx.app_state.settings.voice_enabled = Some(false);
                format!(
                    "Voice dictation disabled.\n-> persisted to {}\nRuntime note: this build does not support real recording/transcription.",
                    path.display()
                )
            }
            Err(e) => format!("Failed to persist voiceEnabled=false: {}", e),
        };
    }

    match current_feasibility() {
        Feasibility::Blocked(reason) => format_blocked(&reason),
        Feasibility::Ready { .. } => match persist(true) {
            Ok(path) => {
                ctx.app_state.settings.voice_enabled = Some(true);
                let lang = normalize_language_for_stt(ctx.app_state.settings.language.as_deref());
                let mut out = format!(
                    "Voice dictation enabled.\n-> persisted to {}",
                    path.display()
                );
                if let Some(raw) = lang.fell_back_from {
                    out.push_str(&format!(
                        "\nNote: \"{}\" is not a supported dictation language; falling back to `en`.",
                        raw
                    ));
                } else {
                    out.push_str(&format!(
                        "\nDictation language: {} (from /config language).",
                        lang.code
                    ));
                }
                out
            }
            Err(e) => format!("Failed to persist voiceEnabled=true: {}", e),
        },
    }
}

fn format_blocked(reason: &FeasibilityReason) -> String {
    let mut out = if is_build_unsupported(reason) {
        String::from("Voice dictation is unsupported in this build.\n\n")
    } else {
        String::from("Voice dictation is currently unavailable.\n\n")
    };
    out.push_str(&format!("Reason: {}\n", reason.short()));
    match reason {
        FeasibilityReason::UnsupportedAuth { .. } | FeasibilityReason::NotAuthenticated(_) => {
            out.push_str(
                "Hint: use `/login` with Claude.ai OAuth once a real voice backend exists.\n",
            );
        }
        FeasibilityReason::RemoteEnvironment(_) => {
            out.push_str(
                "Hint: run allthecodes locally if and when a real voice backend is added.\n",
            );
        }
        FeasibilityReason::AudioBackend(AudioUnavailable::NotImplemented(_))
        | FeasibilityReason::SttBackend(SttUnavailable::NotImplemented(_))
        | FeasibilityReason::SttBackend(SttUnavailable::Disabled(_)) => {
            out.push_str(
                "Hint: leave `voiceEnabled` disabled. The stored setting and keybinding are kept only for config compatibility in this build.\n",
            );
        }
        FeasibilityReason::AudioBackend(_) => {
            out.push_str("Hint: fix local audio capture before enabling voice.\n");
        }
        FeasibilityReason::SttBackend(_) => {
            out.push_str("Hint: fix the transcription backend before enabling voice.\n");
        }
    }
    out
}

/// Produce a [`Feasibility`] snapshot using the live auth and the
/// shipped unsupported backends.
pub fn current_feasibility() -> Feasibility {
    let (auth, _) = resolve_auth_for_voice();
    current_feasibility_for_auth(&auth)
}

fn current_feasibility_for_auth(auth: &AuthMethod) -> Feasibility {
    let audio = NullAudioBackend::new();
    let stt = NullTranscriptionClient::new();
    check_feasibility(auth, &audio, &stt)
}

fn resolve_auth_for_voice() -> (AuthMethod, Option<String>) {
    match auth::try_resolve_auth() {
        Ok(auth) => (auth, None),
        Err(error) => (AuthMethod::None, Some(format!("{error:#}"))),
    }
}

fn is_build_unsupported(reason: &FeasibilityReason) -> bool {
    matches!(
        reason,
        FeasibilityReason::AudioBackend(AudioUnavailable::NotImplemented(_))
            | FeasibilityReason::SttBackend(SttUnavailable::NotImplemented(_))
            | FeasibilityReason::SttBackend(SttUnavailable::Disabled(_))
    )
}

fn short_reason_code(r: &FeasibilityReason) -> &'static str {
    match r {
        FeasibilityReason::UnsupportedAuth { .. } => "unsupported_auth",
        FeasibilityReason::NotAuthenticated(_) => "not_authenticated",
        FeasibilityReason::RemoteEnvironment(_) => "remote_environment",
        FeasibilityReason::AudioBackend(_) => "audio_backend",
        FeasibilityReason::SttBackend(_) => "stt_backend",
    }
}

fn auth_label(a: &AuthMethod) -> &'static str {
    match a {
        AuthMethod::None => "none",
        AuthMethod::ApiKey(_) => "api_key",
        AuthMethod::ExternalToken(_) => "external_token",
        AuthMethod::OAuthToken { method, .. } => match method.as_str() {
            "claude_ai" => "oauth.claude_ai",
            "console" => "oauth.console",
            "openai_codex" => "oauth.openai_codex",
            _ => "oauth.other",
        },
    }
}

/// Read / patch / write the user's settings.json with the new flag.
fn persist(enabled: bool) -> Result<std::path::PathBuf> {
    let path = settings::user_settings_path();
    let mut raw: RawSettings = if path.exists() {
        let txt = std::fs::read_to_string(&path)?;
        serde_json::from_str(&txt)?
    } else {
        RawSettings::default()
    };
    raw.voice_enabled = Some(enabled);
    settings::write_user_settings(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn make_ctx() -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    async fn status_shows_unsupported_runtime_with_config_path() {
        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("runtime support:   unsupported in this build"));
                assert!(s.contains("stored enabled:    false"));
                assert!(s.contains("push-to-talk:      disabled in this build"));
                assert!(s.contains("config path:"));
                assert!(!s.contains("Hold Ctrl+Space"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn status_reports_stored_enabled_flag_and_language_fallback() {
        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        ctx.app_state.settings.voice_enabled = Some(true);
        ctx.app_state.settings.language = Some("klingon".into());
        let r = handler.execute("status", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("stored enabled:    true"));
                assert!(s.contains("dictation lang:    en"));
                assert!(s.contains("fallback from \"klingon\""));
                assert!(s.contains("voiceEnabled is stored, but runtime voice remains unavailable"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_emits_usage_hint() {
        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("bogus", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("Unknown /voice subcommand"));
                assert!(s.contains("Usage:"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn diagnose_includes_auth_method_and_compatibility_note() {
        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("diagnose", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("auth method:"));
                assert!(s.contains("language setting:"));
                assert!(s.contains("Compatibility note:"));
                assert!(!s.contains("follow-up"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn diagnose_surfaces_invalid_present_auth_instead_of_plain_none() {
        let _api = EnvGuard::set("ANTHROPIC_API_KEY", Some("not-a-valid-key"));
        let _token = EnvGuard::set("ANTHROPIC_AUTH_TOKEN", None);
        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("diagnose", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("auth method:       none"));
                assert!(s.contains("auth diagnostic:"));
                assert!(s.contains("ANTHROPIC_API_KEY"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn enable_reports_unsupported_build_before_auth_hints() {
        let _api = EnvGuard::set("ANTHROPIC_API_KEY", None);
        let _token = EnvGuard::set("ANTHROPIC_AUTH_TOKEN", None);
        let _remote = EnvGuard::set("ALLTHECODES_REMOTE", None);
        let _remote2 = EnvGuard::set("ALLTHECODES_REMOTE", None);
        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("on", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("Voice dictation is unsupported in this build."));
                assert!(s.contains("config compatibility"));
                assert!(!s.contains("/login"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn disable_persists_false_flag_for_compatibility() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().to_string_lossy().to_string();
        let _home = EnvGuard::set("ALLTHECODES_HOME", Some(&home));

        let handler = VoiceHandler;
        let mut ctx = make_ctx();
        ctx.app_state.settings.voice_enabled = Some(true);
        let r = handler.execute("off", &mut ctx).await.unwrap();

        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("Voice dictation disabled."));
                assert!(s.contains(
                    "Runtime note: this build does not support real recording/transcription."
                ));
            }
            _ => panic!("expected Output"),
        }

        assert_eq!(ctx.app_state.settings.voice_enabled, Some(false));
        let saved =
            std::fs::read_to_string(settings::user_settings_path()).expect("saved settings");
        let json: serde_json::Value = serde_json::from_str(&saved).expect("valid json");
        assert_eq!(json["voiceEnabled"], serde_json::Value::Bool(false));
    }

    #[test]
    fn short_reason_code_covers_all_feasibility_variants() {
        let variants = [
            FeasibilityReason::UnsupportedAuth {
                auth_label: "x".into(),
                message: "y".into(),
            },
            FeasibilityReason::NotAuthenticated("none".into()),
            FeasibilityReason::RemoteEnvironment("remote".into()),
            FeasibilityReason::AudioBackend(AudioUnavailable::NotImplemented("x".into())),
            FeasibilityReason::SttBackend(SttUnavailable::Disabled("y".into())),
        ];
        let codes: Vec<_> = variants.iter().map(short_reason_code).collect();
        assert_eq!(codes.len(), 5);
        let set: std::collections::HashSet<_> = codes.iter().copied().collect();
        assert_eq!(set.len(), 5);
    }
}
