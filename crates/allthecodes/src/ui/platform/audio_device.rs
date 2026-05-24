// test infrastructure — upstream audio-device boundary; voice capture routed through allthecodes_voice instead
//! Audio-device compatibility surface for the Codex TUI layout.
//!
//! allthecodes currently routes voice capture through `allthecodes_voice`, whose default
//! backend is intentionally unsupported. This module keeps the Codex-shaped
//! `audio_device` boundary available to UI code while reporting that runtime
//! truth explicitly.

use allthecodes_voice::audio::{AudioCaptureBackend, NullAudioBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimeAudioDeviceKind {
    Input,
    Output,
}

impl RealtimeAudioDeviceKind {
    pub fn noun(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub kind: RealtimeAudioDeviceKind,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSupportStatus {
    pub backend: String,
    pub available: bool,
    pub reason: Option<String>,
}

pub fn audio_support_status() -> AudioSupportStatus {
    let backend = NullAudioBackend::new();
    match backend.is_available() {
        Ok(()) => AudioSupportStatus {
            backend: backend.name().to_string(),
            available: true,
            reason: None,
        },
        Err(err) => AudioSupportStatus {
            backend: backend.name().to_string(),
            available: false,
            reason: Some(err.reason().to_string()),
        },
    }
}

pub fn list_realtime_audio_device_names(
    kind: RealtimeAudioDeviceKind,
) -> Result<Vec<String>, String> {
    let status = audio_support_status();
    if status.available {
        Ok(Vec::new())
    } else {
        Err(format!(
            "Failed to load realtime {} devices: {}",
            kind.noun(),
            status
                .reason
                .unwrap_or_else(|| "audio is unavailable".to_string())
        ))
    }
}

pub fn list_realtime_audio_devices(
    kind: RealtimeAudioDeviceKind,
) -> Result<Vec<AudioDeviceInfo>, String> {
    list_realtime_audio_device_names(kind).map(|names| {
        names
            .into_iter()
            .enumerate()
            .map(|(idx, name)| AudioDeviceInfo {
                name,
                kind,
                is_default: idx == 0,
            })
            .collect()
    })
}

pub fn voice_capture_backend_name() -> &'static str {
    NullAudioBackend::new().name()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_null_backend_unavailable() {
        let status = audio_support_status();
        assert_eq!(status.backend, "null");
        assert!(!status.available);
        assert!(status.reason.unwrap().contains("unsupported"));
    }

    #[test]
    fn device_listing_reports_unavailable_capture_backend() {
        let input = list_realtime_audio_device_names(RealtimeAudioDeviceKind::Input)
            .expect_err("null input backend should be unavailable");
        assert!(input.contains("input devices"));

        let output = list_realtime_audio_devices(RealtimeAudioDeviceKind::Output)
            .expect_err("null output backend should be unavailable");
        assert!(output.contains("output devices"));

        assert_eq!(voice_capture_backend_name(), "null");
    }

    #[test]
    fn audio_device_info_carries_kind_and_default_flag() {
        let info = AudioDeviceInfo {
            name: "Default".to_string(),
            kind: RealtimeAudioDeviceKind::Input,
            is_default: true,
        };
        assert_eq!(info.kind.noun(), "input");
        assert!(info.is_default);
    }
}
