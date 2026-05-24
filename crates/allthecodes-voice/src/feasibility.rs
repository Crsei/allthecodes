//! Feasibility checks for voice dictation.
//!
//! In this build, `/voice` should clearly report that real recording and
//! transcription are unsupported. Build-level unsupported backends are
//! surfaced before auth or remote-environment gates so users are not
//! told to log in for a feature that does not exist here.

use allthecodes_auth::AuthMethod;

use super::audio::{AudioCaptureBackend, AudioUnavailable};
use super::stt::{SttUnavailable, TranscriptionClient};

/// Whether voice mode is usable right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feasibility {
    /// Ready: the user can flip `voiceEnabled` and use push-to-talk.
    Ready {
        /// Friendly backend label.
        backend: String,
        /// STT client label.
        client: String,
    },
    /// Not usable for a specific reason.
    Blocked(FeasibilityReason),
}

impl Feasibility {
    pub fn is_ready(&self) -> bool {
        matches!(self, Feasibility::Ready { .. })
    }

    pub fn reason(&self) -> Option<&FeasibilityReason> {
        match self {
            Feasibility::Ready { .. } => None,
            Feasibility::Blocked(r) => Some(r),
        }
    }
}

/// Concrete blocker surfaced to the user by `/voice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeasibilityReason {
    /// Auth layer cannot serve voice dictation.
    UnsupportedAuth { auth_label: String, message: String },
    /// User is not logged in at all.
    NotAuthenticated(String),
    /// Remote environment (SSH session, CC_RUST_REMOTE).
    RemoteEnvironment(String),
    /// Audio capture backend is unavailable.
    AudioBackend(AudioUnavailable),
    /// STT endpoint is unavailable.
    SttBackend(SttUnavailable),
}

impl FeasibilityReason {
    pub fn short(&self) -> &str {
        match self {
            FeasibilityReason::UnsupportedAuth { message, .. }
            | FeasibilityReason::NotAuthenticated(message)
            | FeasibilityReason::RemoteEnvironment(message) => message.as_str(),
            FeasibilityReason::AudioBackend(u) => u.reason(),
            FeasibilityReason::SttBackend(u) => u.reason(),
        }
    }
}

/// Check whether voice is supported right now. Build-level unsupported
/// backends are reported first so the command output stays honest in
/// this downgraded build.
pub fn check_feasibility(
    auth: &AuthMethod,
    audio: &dyn AudioCaptureBackend,
    stt: &dyn TranscriptionClient,
) -> Feasibility {
    if let Some(reason) = build_support_blocker(audio, stt) {
        return Feasibility::Blocked(reason);
    }
    if let Some(reason) = auth_blocker(auth) {
        return Feasibility::Blocked(reason);
    }
    if let Some(reason) = remote_environment_blocker() {
        return Feasibility::Blocked(reason);
    }
    if let Err(e) = audio.is_available() {
        return Feasibility::Blocked(FeasibilityReason::AudioBackend(e));
    }
    if let Err(e) = stt.is_available() {
        return Feasibility::Blocked(FeasibilityReason::SttBackend(e));
    }
    Feasibility::Ready {
        backend: audio.name().to_string(),
        client: stt.name().to_string(),
    }
}

fn build_support_blocker(
    audio: &dyn AudioCaptureBackend,
    stt: &dyn TranscriptionClient,
) -> Option<FeasibilityReason> {
    if let Err(AudioUnavailable::NotImplemented(reason)) = audio.is_available() {
        return Some(FeasibilityReason::AudioBackend(
            AudioUnavailable::NotImplemented(reason),
        ));
    }
    match stt.is_available() {
        Err(SttUnavailable::NotImplemented(reason)) => Some(FeasibilityReason::SttBackend(
            SttUnavailable::NotImplemented(reason),
        )),
        Err(SttUnavailable::Disabled(reason)) => Some(FeasibilityReason::SttBackend(
            SttUnavailable::Disabled(reason),
        )),
        Err(_) | Ok(()) => None,
    }
}

/// Reject auth methods that cannot serve real voice dictation.
/// Only Claude.ai OAuth would be accepted once a real backend exists.
pub fn auth_blocker(auth: &AuthMethod) -> Option<FeasibilityReason> {
    match auth {
        AuthMethod::None => Some(FeasibilityReason::NotAuthenticated(
            "Voice mode requires a Claude.ai account. Run `/login` to sign in.".to_string(),
        )),
        AuthMethod::ApiKey(_) => Some(FeasibilityReason::UnsupportedAuth {
            auth_label: "api_key".into(),
            message: "Voice mode requires Claude.ai OAuth. API keys, Bedrock, Vertex, \
                      and Foundry cannot use real voice dictation."
                .into(),
        }),
        AuthMethod::ExternalToken(_) => Some(FeasibilityReason::UnsupportedAuth {
            auth_label: "external_token".into(),
            message: "Voice mode requires Claude.ai OAuth. External bearer tokens \
                      (ANTHROPIC_AUTH_TOKEN) are not supported for real voice dictation."
                .into(),
        }),
        AuthMethod::OAuthToken { method, .. } => {
            if method == "claude_ai" {
                None
            } else {
                Some(FeasibilityReason::UnsupportedAuth {
                    auth_label: method.clone(),
                    message: format!(
                        "Voice mode requires Claude.ai OAuth. The active OAuth method ({}) \
                         does not support real voice dictation.",
                        method
                    ),
                })
            }
        }
    }
}

/// Detect remote-run environments where a local microphone is unlikely.
fn remote_environment_blocker() -> Option<FeasibilityReason> {
    if truthy_env("CC_RUST_REMOTE") || truthy_env("CLAUDE_CODE_REMOTE") {
        return Some(FeasibilityReason::RemoteEnvironment(
            "Voice mode needs a local microphone, but this session is marked remote. \
             Run cc-rust locally to use voice."
                .to_string(),
        ));
    }
    None
}

fn truthy_env(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::NullAudioBackend;
    use crate::stt::NullTranscriptionClient;
    use serial_test::serial;

    struct ReadyAudioBackend;

    impl AudioCaptureBackend for ReadyAudioBackend {
        fn name(&self) -> &'static str {
            "ready-audio"
        }

        fn is_available(&self) -> Result<(), AudioUnavailable> {
            Ok(())
        }

        fn start(&self) -> Result<super::super::audio::RecordingHandle, AudioUnavailable> {
            let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let stopped = std::sync::Arc::new(parking_lot::Mutex::new(false));
            Ok(super::super::audio::RecordingHandle::new(rx, stopped))
        }
    }

    struct ReadySttClient;

    #[async_trait::async_trait]
    impl TranscriptionClient for ReadySttClient {
        fn name(&self) -> &'static str {
            "ready-stt"
        }

        fn is_available(&self) -> Result<(), SttUnavailable> {
            Ok(())
        }

        async fn transcribe(
            &self,
            _handle: super::super::audio::RecordingHandle,
            language: &str,
        ) -> Result<super::super::stt::TranscriptionResult, super::super::stt::SttError> {
            Ok(super::super::stt::TranscriptionResult {
                text: String::new(),
                language: language.to_string(),
            })
        }
    }

    fn ready_pair() -> (ReadyAudioBackend, ReadySttClient) {
        (ReadyAudioBackend, ReadySttClient)
    }

    fn null_pair() -> (NullAudioBackend, NullTranscriptionClient) {
        (NullAudioBackend::new(), NullTranscriptionClient::new())
    }

    #[test]
    fn build_unsupported_takes_precedence_over_missing_auth() {
        let (a, s) = null_pair();
        let auth = AuthMethod::None;
        let f = check_feasibility(&auth, &a, &s);
        assert!(matches!(
            f.reason().unwrap(),
            FeasibilityReason::AudioBackend(AudioUnavailable::NotImplemented(_))
        ));
    }

    #[test]
    fn api_key_auth_is_rejected_when_backends_are_ready() {
        let (a, s) = ready_pair();
        let auth = AuthMethod::ApiKey("sk-foo".into());
        let f = check_feasibility(&auth, &a, &s);
        match f.reason().unwrap() {
            FeasibilityReason::UnsupportedAuth {
                auth_label,
                message,
            } => {
                assert_eq!(auth_label, "api_key");
                assert!(message.contains("Claude.ai OAuth"));
            }
            other => panic!("expected UnsupportedAuth, got {:?}", other),
        }
    }

    #[test]
    fn none_auth_is_rejected_with_login_hint_when_backends_are_ready() {
        let (a, s) = ready_pair();
        let auth = AuthMethod::None;
        let f = check_feasibility(&auth, &a, &s);
        assert!(matches!(
            f.reason().unwrap(),
            FeasibilityReason::NotAuthenticated(_)
        ));
    }

    #[test]
    fn claude_ai_oauth_is_ready_when_backends_are_ready() {
        let (a, s) = ready_pair();
        let auth = AuthMethod::OAuthToken {
            access_token: "tok".into(),
            method: "claude_ai".into(),
        };
        let f = check_feasibility(&auth, &a, &s);
        assert!(matches!(f, Feasibility::Ready { .. }));
    }

    #[test]
    fn non_claude_ai_oauth_is_rejected() {
        let (a, s) = ready_pair();
        let auth = AuthMethod::OAuthToken {
            access_token: "tok".into(),
            method: "console".into(),
        };
        let f = check_feasibility(&auth, &a, &s);
        match f.reason().unwrap() {
            FeasibilityReason::UnsupportedAuth { auth_label, .. } => {
                assert_eq!(auth_label, "console");
            }
            other => panic!("expected UnsupportedAuth, got {:?}", other),
        }
    }

    #[test]
    #[serial]
    fn remote_env_var_blocks_when_backends_and_auth_are_ready() {
        std::env::set_var("CC_RUST_REMOTE", "1");
        let (a, s) = ready_pair();
        let auth = AuthMethod::OAuthToken {
            access_token: "tok".into(),
            method: "claude_ai".into(),
        };
        let f = check_feasibility(&auth, &a, &s);
        let reason = f.reason().unwrap().clone();
        std::env::remove_var("CC_RUST_REMOTE");
        assert!(
            matches!(reason, FeasibilityReason::RemoteEnvironment(_)),
            "got {:?}",
            reason
        );
    }
}
