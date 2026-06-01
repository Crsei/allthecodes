# Voice dictation

cc-rust currently ships voice dictation as a compatibility surface only. The
`/voice` command, `voiceEnabled` setting, `voice:pushToTalk` keybinding action,
and dictation-language normalization are preserved so existing settings files
remain understandable, but this build does not include a real audio recorder or
speech-to-text backend.

## Current behavior

`/voice` reports the stored setting and the unsupported runtime status:

```text
/voice
Voice dictation
---------------
  runtime support:   unsupported in this build
  push-to-talk:      disabled in this build
```

`/voice off` or `/voice disable` persists `voiceEnabled=false`. Enabling voice
with `/voice on`, `/voice enable`, or `/voice toggle` remains blocked until a
real audio/STT backend is added.

## Settings kept for compatibility

```json
{
  "voiceEnabled": false,
  "language": "english"
}
```

`language` is normalized to the same STT language codes used by the upstream
surface. Unsupported values fall back to `en` in diagnostics. This fallback does
not enable recording.

## Keybindings kept for compatibility

The `voice:pushToTalk` action can still appear in keybinding files, but it is
inactive while runtime voice support is unavailable:

```json
{
  "bindings": [
    {
      "context": "Chat",
      "bindings": {
        "space": "voice:pushToTalk"
      }
    }
  ]
}
```

## Release status

Real microphone capture, waveform UI, streaming transcription, and Claude.ai
voice STT integration are not release claims for this cc-rust build. Revisit
voice only when the project adds a supported audio backend and a transcription
client with tests for local, SSH/remote, WSL, and unsupported-auth cases.
