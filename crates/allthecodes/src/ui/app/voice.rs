use allthecodes_voice::{VoiceController, VoiceEvent};

use super::App;

impl App {
    /// Install the shared voice controller (issue #13). The TUI runner
    /// supplies a controller already wired to AppState's audio + stt
    /// backends so `/voice` and the push-to-talk key observe the same
    /// state machine.
    pub fn set_voice_controller(&mut self, controller: VoiceController) {
        self.voice = Some(controller);
    }

    /// Update the cached `voiceEnabled` flag + normalized language.
    /// Called at startup and whenever `/voice` or `/config` flips the
    /// settings so the key handler doesn't need to read through AppState.
    pub fn set_voice_settings(&mut self, enabled: bool, language: String, supported: bool) {
        self.voice_enabled = enabled;
        self.voice_supported = supported;
        self.voice_language = language;
        self.dirty = true;
    }

    /// True when `voiceEnabled` is on *and* a controller has been
    /// installed (tests run without a controller, so push-to-talk is a
    /// no-op, as it should be).
    pub fn is_voice_ready(&self) -> bool {
        self.voice_enabled && self.voice_supported && self.voice.is_some()
    }

    /// Begin push-to-talk recording. No-op when voice is disabled or no
    /// controller is installed. Exposed so the TUI runner (and tests)
    /// can trigger the flow without replaying a key event.
    pub fn begin_push_to_talk(&mut self) {
        if !self.is_voice_ready() {
            return;
        }
        if let Some(v) = &self.voice {
            v.press(self.voice_language.clone());
        }
        self.dirty = true;
    }

    /// Release push-to-talk and transition to Transcribing.
    pub fn end_push_to_talk(&mut self) {
        if let Some(v) = &self.voice {
            v.release();
        }
        self.dirty = true;
    }

    /// Drain voice-controller events into the prompt input. Called once
    /// per render tick. Returns `true` when something changed, so the
    /// caller can force a redraw.
    pub fn drain_voice_events(&mut self) -> bool {
        let Some(v) = &self.voice else {
            return false;
        };
        let events = v.drain_events();
        if events.is_empty() {
            return false;
        }
        for evt in events {
            match evt {
                VoiceEvent::Transcription(text) => {
                    // Insert at cursor so transcribed text joins whatever
                    // the user was typing before they hit push-to-talk.
                    self.prompt.insert_str(&text);
                }
                // StateChanged / Error are reflected through the
                // composer footer; nothing more to do here.
                VoiceEvent::StateChanged(_) | VoiceEvent::Error(_) => {}
            }
        }
        self.dirty = true;
        true
    }
}
