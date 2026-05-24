//! Speech-to-text client abstraction for voice dictation.
//!
//! This build deliberately ships only [`NullTranscriptionClient`]. The
//! trait boundary remains so the controller can be exercised safely, but
//! no real transcription service is available here.

use async_trait::async_trait;

use super::audio::RecordingHandle;

/// Result of running a full record-and-transcribe session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionResult {
    /// Text to insert into the composer, trimmed.
    pub text: String,
    /// Language code requested for transcription.
    pub language: String,
}

/// Every STT client implements this. Implementations drain
/// `handle.audio` for PCM frames and return a final transcription once
/// the handle closes.
#[async_trait]
pub trait TranscriptionClient: Send + Sync {
    /// Short label (`voice_stream`, `null`, etc.).
    fn name(&self) -> &'static str;

    /// Check whether the client can actually transcribe. Called before
    /// a recording session so `/voice` can surface a specific error.
    fn is_available(&self) -> Result<(), SttUnavailable>;

    /// Drive a full record-and-transcribe exchange.
    async fn transcribe(
        &self,
        handle: RecordingHandle,
        language: &str,
    ) -> Result<TranscriptionResult, SttError>;
}

/// Reasons the STT client cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttUnavailable {
    /// Endpoint not wired up in this build.
    NotImplemented(String),
    /// User isn't logged in with a supported auth.
    AuthRequired(String),
    /// Voice is intentionally disabled in this build.
    Disabled(String),
    /// Anything else.
    Other(String),
}

impl SttUnavailable {
    pub fn reason(&self) -> &str {
        match self {
            SttUnavailable::NotImplemented(s)
            | SttUnavailable::AuthRequired(s)
            | SttUnavailable::Disabled(s)
            | SttUnavailable::Other(s) => s.as_str(),
        }
    }
}

/// Runtime errors surfaced from `transcribe()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttError {
    /// Upstream transport failed (socket reset, timeout, 5xx).
    Transport(String),
    /// Server returned an error response.
    Server(String),
    /// Client-side issue (bad format, cancelled).
    Client(String),
}

impl SttError {
    pub fn reason(&self) -> &str {
        match self {
            SttError::Transport(s) | SttError::Server(s) | SttError::Client(s) => s.as_str(),
        }
    }
}

// ---------------------------------------------------------------------------
// NullTranscriptionClient - explicit unsupported-build placeholder.
// ---------------------------------------------------------------------------

/// Default client: reports unavailable with a clear reason.
pub struct NullTranscriptionClient {
    reason: String,
}

impl NullTranscriptionClient {
    pub fn new() -> Self {
        Self {
            reason: "Voice transcription is disabled in this build of cc-rust. \
                     No STT backend is compiled, so recordings cannot be transcribed."
                .to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_reason(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Default for NullTranscriptionClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TranscriptionClient for NullTranscriptionClient {
    fn name(&self) -> &'static str {
        "null"
    }

    fn is_available(&self) -> Result<(), SttUnavailable> {
        Err(SttUnavailable::Disabled(self.reason.clone()))
    }

    async fn transcribe(
        &self,
        _handle: RecordingHandle,
        _language: &str,
    ) -> Result<TranscriptionResult, SttError> {
        Err(SttError::Client(self.reason.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioCaptureBackend;
    use crate::audio::NullAudioBackend;

    #[test]
    fn null_client_reports_disabled_build() {
        let c = NullTranscriptionClient::new();
        assert_eq!(c.name(), "null");
        assert!(matches!(
            c.is_available().unwrap_err(),
            SttUnavailable::Disabled(_)
        ));
    }

    #[tokio::test]
    async fn null_client_transcribe_returns_client_error() {
        let c = NullTranscriptionClient::new();
        let b = NullAudioBackend::with_reason("stub");
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stopped = std::sync::Arc::new(parking_lot::Mutex::new(false));
        let h = RecordingHandle::new(rx, stopped);
        let r = c.transcribe(h, "en").await;
        let err = r.unwrap_err();
        assert!(matches!(err, SttError::Client(_)));
        assert!(err.reason().contains("disabled"));
        let _ = b.name();
    }
}
