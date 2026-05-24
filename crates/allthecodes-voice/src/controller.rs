//! Push-to-talk state machine for voice dictation.
//!
//! [`VoiceController`] owns the `Idle -> Recording -> Transcribing ->
//! Idle` transitions and produces [`VoiceEvent`] values for the TUI.
//! The controller stays I/O-light: it coordinates the audio backend, the
//! STT client, and an event queue while a tokio task performs the async
//! transcription work.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use super::audio::{AudioCaptureBackend, AudioUnavailable, RecordingHandle};
use super::stt::{SttError, TranscriptionClient, TranscriptionResult};

/// Observable state of the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceState {
    /// Microphone not engaged; ready to start.
    Idle,
    /// Actively capturing audio.
    Recording,
    /// Audio capture stopped; waiting for the STT client to finalize.
    Transcribing,
    /// Something went wrong; carries a human message.
    Error(String),
}

impl VoiceState {
    pub fn is_active(&self) -> bool {
        matches!(self, VoiceState::Recording | VoiceState::Transcribing)
    }

    pub fn label(&self) -> &'static str {
        match self {
            VoiceState::Idle => "idle",
            VoiceState::Recording => "recording",
            VoiceState::Transcribing => "transcribing",
            VoiceState::Error(_) => "error",
        }
    }
}

/// Events pushed to the TUI. The TUI drains these with
/// [`VoiceController::drain_events`] on each render tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEvent {
    /// State changed; the TUI should redraw the composer / status line.
    StateChanged(VoiceState),
    /// Final transcription ready for insertion.
    Transcription(String),
    /// Recoverable error; caller may choose to display and keep going.
    Error(String),
}

/// Shared handle consumed by the TUI. Cheap to clone.
#[derive(Clone)]
pub struct VoiceController {
    inner: Arc<Mutex<ControllerState>>,
    events_tx: UnboundedSender<VoiceEvent>,
    events_rx: Arc<Mutex<Option<UnboundedReceiver<VoiceEvent>>>>,
}

struct ControllerState {
    audio: Arc<dyn AudioCaptureBackend>,
    stt: Arc<dyn TranscriptionClient>,
    state: VoiceState,
    next_recording_id: u64,
    active_recording: Option<ActiveRecording>,
}

struct ActiveRecording {
    id: u64,
    stop_flag: Arc<Mutex<bool>>,
}

impl VoiceController {
    pub fn new(audio: Arc<dyn AudioCaptureBackend>, stt: Arc<dyn TranscriptionClient>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(Mutex::new(ControllerState {
                audio,
                stt,
                state: VoiceState::Idle,
                next_recording_id: 1,
                active_recording: None,
            })),
            events_tx: tx,
            events_rx: Arc::new(Mutex::new(Some(rx))),
        }
    }

    /// Observe current state without locking long-term.
    pub fn state(&self) -> VoiceState {
        self.inner.lock().state.clone()
    }

    /// Human-readable one-line status for the composer footer.
    pub fn status_line(&self) -> Option<String> {
        match self.state() {
            VoiceState::Idle => None,
            VoiceState::Recording => Some("voice: recording (release key to finish)".to_string()),
            VoiceState::Transcribing => Some("voice: finalizing transcription".to_string()),
            VoiceState::Error(msg) => Some(format!("voice error: {}", msg)),
        }
    }

    /// Push-to-talk key was pressed. Attempts to start audio capture
    /// and dispatches a transcription task. Repeated presses while a
    /// session is already active are ignored.
    pub fn press(&self, language: String) {
        let mut guard = self.inner.lock();
        if !matches!(guard.state, VoiceState::Idle | VoiceState::Error(_)) {
            return;
        }
        match guard.audio.start() {
            Ok(handle) => {
                let recording_id = guard.next_recording_id;
                guard.next_recording_id += 1;
                guard.state = VoiceState::Recording;
                guard.active_recording = Some(ActiveRecording {
                    id: recording_id,
                    stop_flag: handle.stop_flag(),
                });
                drop(guard);
                self.send(VoiceEvent::StateChanged(VoiceState::Recording));
                self.spawn_transcription_task(handle, recording_id, language);
            }
            Err(e) => {
                let msg = format_audio_error(&e);
                guard.state = VoiceState::Error(msg.clone());
                guard.active_recording = None;
                drop(guard);
                self.send(VoiceEvent::StateChanged(VoiceState::Error(msg.clone())));
                self.send(VoiceEvent::Error(msg));
            }
        }
    }

    /// Release of the push-to-talk key stops capturing audio. The
    /// spawned transcription task will finalize once the backend closes
    /// its sender side.
    pub fn release(&self) {
        let mut guard = self.inner.lock();
        if !matches!(guard.state, VoiceState::Recording) {
            return;
        }
        guard.state = VoiceState::Transcribing;
        if let Some(active) = &guard.active_recording {
            *active.stop_flag.lock() = true;
        }
        drop(guard);
        self.send(VoiceEvent::StateChanged(VoiceState::Transcribing));
    }

    /// Cancel an in-flight recording without producing a transcription.
    /// Used when the user flips voice off mid-recording or hits Esc.
    pub fn cancel(&self) {
        let mut guard = self.inner.lock();
        if matches!(guard.state, VoiceState::Idle) {
            return;
        }
        let active = guard.active_recording.take();
        guard.state = VoiceState::Idle;
        drop(guard);
        if let Some(active) = active {
            *active.stop_flag.lock() = true;
        }
        self.send(VoiceEvent::StateChanged(VoiceState::Idle));
    }

    /// Drain every queued event. Safe to call on every tick; returns an
    /// empty vec if nothing is pending.
    pub fn drain_events(&self) -> Vec<VoiceEvent> {
        let mut guard = self.events_rx.lock();
        let rx = match guard.as_mut() {
            Some(rx) => rx,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            out.push(evt);
        }
        out
    }

    fn send(&self, evt: VoiceEvent) {
        let _ = self.events_tx.send(evt);
    }

    /// Spawn the transcription task. Caller already holds
    /// `state = Recording`.
    fn spawn_transcription_task(
        &self,
        handle: RecordingHandle,
        recording_id: u64,
        language: String,
    ) {
        let stt = { self.inner.lock().stt.clone() };
        let events_tx = self.events_tx.clone();
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let result = stt.transcribe(handle, &language).await;
            match result {
                Ok(TranscriptionResult { text, .. }) => {
                    let trimmed = text.trim().to_string();
                    let should_publish = {
                        let mut guard = inner.lock();
                        if !matches!(
                            guard.active_recording.as_ref().map(|active| active.id),
                            Some(id) if id == recording_id
                        ) {
                            false
                        } else {
                            guard.state = VoiceState::Idle;
                            guard.active_recording = None;
                            true
                        }
                    };
                    if should_publish {
                        let _ = events_tx.send(VoiceEvent::StateChanged(VoiceState::Idle));
                        if !trimmed.is_empty() {
                            let _ = events_tx.send(VoiceEvent::Transcription(trimmed));
                        }
                    }
                }
                Err(e) => {
                    let msg = format_stt_error(&e);
                    let should_publish = {
                        let mut guard = inner.lock();
                        if !matches!(
                            guard.active_recording.as_ref().map(|active| active.id),
                            Some(id) if id == recording_id
                        ) {
                            false
                        } else {
                            guard.state = VoiceState::Error(msg.clone());
                            guard.active_recording = None;
                            true
                        }
                    };
                    if should_publish {
                        let _ = events_tx
                            .send(VoiceEvent::StateChanged(VoiceState::Error(msg.clone())));
                        let _ = events_tx.send(VoiceEvent::Error(msg));
                    }
                }
            }
        });
    }
}

fn format_audio_error(e: &AudioUnavailable) -> String {
    format!("voice capture unavailable: {}", e.reason())
}

fn format_stt_error(e: &SttError) -> String {
    match e {
        SttError::Transport(s) => format!("voice transport error: {}", s),
        SttError::Server(s) => format!("voice server error: {}", s),
        SttError::Client(s) => format!("voice client error: {}", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::NullAudioBackend;
    use crate::stt::{NullTranscriptionClient, SttUnavailable};
    use async_trait::async_trait;

    struct InspectableBackend {
        flags: Arc<Mutex<Vec<Arc<Mutex<bool>>>>>,
    }

    impl Default for InspectableBackend {
        fn default() -> Self {
            Self {
                flags: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl InspectableBackend {
        fn latest_flag(&self) -> Arc<Mutex<bool>> {
            self.flags.lock().last().cloned().expect("recording flag")
        }
    }

    impl AudioCaptureBackend for InspectableBackend {
        fn name(&self) -> &'static str {
            "inspectable"
        }

        fn is_available(&self) -> Result<(), AudioUnavailable> {
            Ok(())
        }

        fn start(&self) -> Result<RecordingHandle, AudioUnavailable> {
            let (_tx, rx) = mpsc::unbounded_channel();
            let stopped = Arc::new(Mutex::new(false));
            self.flags.lock().push(Arc::clone(&stopped));
            Ok(RecordingHandle::new(rx, stopped))
        }
    }

    struct DelayedClient {
        text: String,
        delay_ms: u64,
    }

    #[async_trait]
    impl TranscriptionClient for DelayedClient {
        fn name(&self) -> &'static str {
            "delayed"
        }

        fn is_available(&self) -> Result<(), SttUnavailable> {
            Ok(())
        }

        async fn transcribe(
            &self,
            _handle: RecordingHandle,
            language: &str,
        ) -> Result<TranscriptionResult, SttError> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(TranscriptionResult {
                text: self.text.clone(),
                language: language.to_string(),
            })
        }
    }

    #[tokio::test]
    async fn null_backend_press_transitions_to_error() {
        let c = VoiceController::new(
            Arc::new(NullAudioBackend::new()),
            Arc::new(NullTranscriptionClient::new()),
        );
        assert_eq!(c.state(), VoiceState::Idle);
        c.press("en".into());
        assert!(matches!(c.state(), VoiceState::Error(_)));
        let events = c.drain_events();
        assert!(events.iter().any(|e| matches!(e, VoiceEvent::Error(_))));
    }

    #[tokio::test]
    async fn release_moves_to_transcribing_and_sets_stop_flag() {
        let backend = Arc::new(InspectableBackend::default());
        let c = VoiceController::new(
            backend.clone(),
            Arc::new(DelayedClient {
                text: "hello world".into(),
                delay_ms: 10,
            }),
        );

        c.press("en".into());
        assert_eq!(c.state(), VoiceState::Recording);
        assert!(!*backend.latest_flag().lock());

        c.release();
        assert_eq!(c.state(), VoiceState::Transcribing);
        assert!(*backend.latest_flag().lock());

        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let events = c.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, VoiceEvent::StateChanged(VoiceState::Recording))));
        assert!(events
            .iter()
            .any(|e| matches!(e, VoiceEvent::StateChanged(VoiceState::Transcribing))));
        assert!(events
            .iter()
            .any(|e| matches!(e, VoiceEvent::Transcription(t) if t == "hello world")));
        assert_eq!(c.state(), VoiceState::Idle);
    }

    #[tokio::test]
    async fn cancel_suppresses_stale_transcription_and_resets_to_idle() {
        let backend = Arc::new(InspectableBackend::default());
        let c = VoiceController::new(
            backend.clone(),
            Arc::new(DelayedClient {
                text: "ignored".into(),
                delay_ms: 20,
            }),
        );

        c.press("en".into());
        c.cancel();
        assert_eq!(c.state(), VoiceState::Idle);
        assert!(*backend.latest_flag().lock());

        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let events = c.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, VoiceEvent::StateChanged(VoiceState::Idle))));
        assert!(!events
            .iter()
            .any(|e| matches!(e, VoiceEvent::Transcription(t) if t == "ignored")));
        assert_eq!(c.state(), VoiceState::Idle);
    }
}
