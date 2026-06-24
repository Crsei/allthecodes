#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TtsRequest {
    pub text: String,
    /// Voice id: "alloy", "echo", "fable", "onyx", "nova", "shimmer"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Model: "tts-1", "tts-1-hd"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Speed: 0.25 – 4.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Response format: "mp3", "opus", "aac", "flac"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SttRequest {
    /// Base64-encoded audio data
    pub audio_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Model: "whisper-1"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TtsResponse {
    /// Base64-encoded audio bytes
    pub audio_base64: String,
    /// Audio format, e.g. "mp3"
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SttResponse {
    pub text: String,
    #[serde(default)]
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VoiceProvidersResponse {
    pub tts_available: bool,
    pub stt_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stt_provider: Option<String>,
    pub voices: Vec<VoiceInfo>,
}
