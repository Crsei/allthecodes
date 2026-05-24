//! Voice dictation subsystem.
//!
//! This build keeps only the compatibility surface for `/voice`,
//! `voiceEnabled`, language normalization, and the push-to-talk state
//! machine. Real recording and transcription are intentionally
//! unsupported here.
//!
//! # Layout
//!
//! - [`controller`] - [`VoiceController`] + [`VoiceState`], the front-end
//!   state machine consumed by the TUI.
//! - [`audio`] - [`AudioCaptureBackend`] trait + explicit unsupported
//!   stub used by this build.
//! - [`stt`] - [`TranscriptionClient`] trait + explicit disabled stub
//!   used by this build.
//! - [`language`] - `normalize_language_for_stt` maps a freeform
//!   `language` setting to a supported dictation language code.
//! - [`feasibility`] - gates the controller and `/voice` command.
//!
//! # Current contract
//!
//! - `/voice` reports stored config plus unsupported runtime status.
//! - `voiceEnabled` config remains accepted for compatibility.
//! - `voice:pushToTalk` remains bindable, but the shipped backends do
//!   not produce real recordings or transcriptions.
//! - Unsupported auth/backend errors stay explicit.
//! - `language` still normalizes to the dictation code that a future
//!   backend would use.

pub mod audio;
pub mod controller;
pub mod feasibility;
pub mod language;
pub mod stt;

pub use audio::{AudioCaptureBackend, NullAudioBackend, RecordingHandle};
pub use controller::{VoiceController, VoiceEvent, VoiceState};
pub use feasibility::{Feasibility, FeasibilityReason};
pub use language::{normalize_language_for_stt, NormalizedLanguage};
pub use stt::{NullTranscriptionClient, TranscriptionClient, TranscriptionResult};
