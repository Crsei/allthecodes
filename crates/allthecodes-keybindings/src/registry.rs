//! Effective keybinding registry: merged defaults + user bindings, with
//! context-aware lookup and mtime-polled hot reload.
//!
//! Resolution order:
//!   1. Exact chord match in the requested context.
//!   2. Fallback to the `Global` context (unless request was already Global).
//!   3. Misses return `None`.
//!
//! The registry holds an `Arc<RwLock<State>>` so the Rust TUI can share one
//! instance across threads. `resolve()` transparently re-reads the user file
//! when its `mtime` changes; parse failures are logged and the last good
//! config is retained.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::RwLock;
use serde::Serialize;

use super::action::Action;
use super::config::{BindingValue, KeybindingsConfigError, UserBindings};
use super::context::Context;
use super::defaults::iter_parsed;
use super::keystroke::{Chord, Keystroke};

/// A prefix match that waits for the next keystroke in a chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Chord fully matched; here is the action.
    Action(Action),
    /// Pressed keystroke matches the prefix of some chord; wait for the next
    /// keystroke.
    Pending,
    /// No binding matches.
    None,
}

/// JSON-friendly snapshot of the effective bindings for frontend consumers.
///
/// The leader can wire this into IPC from the shared hotspot files without
/// recreating merge logic there.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EffectiveKeybindingsSnapshot {
    pub bindings: Vec<EffectiveKeybindingBlock>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EffectiveKeybindingBlock {
    pub context: String,
    pub bindings: HashMap<String, String>,
}

/// Inner registry state.
#[derive(Debug, Default)]
struct State {
    effective: HashMap<Context, HashMap<Chord, Action>>,
    /// Context-local unbinds that must suppress global fallback for that
    /// context. Prefix entries suppress longer matching chords too.
    blocked: HashMap<Context, Vec<Chord>>,
    last_mtime: Option<SystemTime>,
    user_path: Option<PathBuf>,
    last_issues: Vec<String>,
}

/// Shared registry handle. Cheap to clone (`Arc`).
#[derive(Debug, Clone, Default)]
pub struct KeybindingRegistry {
    inner: Arc<RwLock<State>>,
}

impl KeybindingRegistry {
    /// Build a registry populated with the default bindings only.
    pub fn with_defaults() -> Self {
        let mut state = State::default();
        install_defaults(&mut state.effective);
        Self {
            inner: Arc::new(RwLock::new(state)),
        }
    }

    /// Build with defaults and an optional user file path (not yet read).
    pub fn with_user_path(user_path: Option<PathBuf>) -> Self {
        let me = Self::with_defaults();
        {
            let mut state = me.inner.write();
            state.user_path = user_path;
        }
        let _ = me.reload();
        me
    }

    pub fn has_user_path(&self) -> bool {
        self.inner.read().user_path.is_some()
    }

    pub fn user_path(&self) -> Option<PathBuf> {
        self.inner.read().user_path.clone()
    }

    /// Force a reload now. Returns `Ok(())` when the reload succeeds (or the
    /// file does not exist) and `Err` when the user file exists but cannot be
    /// parsed; in that case the previous effective set is retained.
    pub fn reload(&self) -> Result<(), KeybindingsConfigError> {
        let mut state = self.inner.write();
        let Some(path) = state.user_path.clone() else {
            return Ok(());
        };
        let mtime = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok();
        state.last_mtime = mtime;

        match fs::read_to_string(&path) {
            Ok(text) => {
                let user = UserBindings::parse_json(&text)?;
                let mut effective = HashMap::new();
                let mut blocked = HashMap::new();
                install_defaults(&mut effective);
                apply_user(&mut effective, &mut blocked, &user);
                state.effective = effective;
                state.blocked = blocked;
                state.last_issues.clear();
                Ok(())
            }
            Err(_) => {
                let mut effective = HashMap::new();
                install_defaults(&mut effective);
                state.effective = effective;
                state.blocked.clear();
                Ok(())
            }
        }
    }

    /// Check the user file's mtime and reload if it changed.
    ///
    /// On parse failure, the error is stashed (see [`Self::last_issues`]) and
    /// the previous effective config is kept so the UI stays usable.
    pub fn refresh_if_changed(&self) {
        let path = self.inner.read().user_path.clone();
        let Some(path) = path else { return };
        let new_mtime = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok();
        let changed = {
            let state = self.inner.read();
            state.last_mtime != new_mtime
        };
        if !changed {
            return;
        }
        if let Err(error) = self.reload() {
            let mut state = self.inner.write();
            state.last_issues = vec![error.to_string()];
            state.last_mtime = new_mtime;
            tracing::warn!(error = %error, "keybindings.json parse failed; keeping previous config");
        }
    }

    pub fn last_issues(&self) -> Vec<String> {
        self.inner.read().last_issues.clone()
    }

    /// Resolve a single-keystroke lookup. For full chords, use
    /// [`Self::resolve_chord`].
    pub fn resolve_single(&self, ctx: Context, stroke: &Keystroke) -> Resolution {
        self.refresh_if_changed();

        let state = self.inner.read();
        if let Some(map) = state.effective.get(&ctx) {
            if let Some(resolution) = resolve_single_from_map(map, stroke, None) {
                return resolution;
            }
        }
        if ctx == Context::Global {
            return Resolution::None;
        }
        if let Some(map) = state.effective.get(&Context::Global) {
            if let Some(resolution) = resolve_single_from_map(map, stroke, state.blocked.get(&ctx))
            {
                return resolution;
            }
        }
        Resolution::None
    }

    /// Resolve a complete chord (one or more keystrokes).
    pub fn resolve_chord(&self, ctx: Context, chord: &Chord) -> Resolution {
        self.refresh_if_changed();

        let state = self.inner.read();
        if let Some(map) = state.effective.get(&ctx) {
            if let Some(action) = map.get(chord) {
                return Resolution::Action(action.clone());
            }
        }
        if ctx == Context::Global {
            return Resolution::None;
        }
        if is_blocked_in_context(state.blocked.get(&ctx), chord) {
            return Resolution::None;
        }
        if let Some(map) = state.effective.get(&Context::Global) {
            if let Some(action) = map.get(chord) {
                return Resolution::Action(action.clone());
            }
        }
        Resolution::None
    }

    /// Iterate all effective bindings for introspection (`/keybindings list`,
    /// help pages, tests).
    pub fn all_bindings(&self) -> Vec<(Context, Chord, Action)> {
        let state = self.inner.read();
        let mut out = Vec::new();
        for (ctx, map) in &state.effective {
            for (chord, action) in map {
                out.push((*ctx, chord.clone(), action.clone()));
            }
        }
        out.sort_by(|left, right| {
            (left.0.as_str(), left.1.display()).cmp(&(right.0.as_str(), right.1.display()))
        });
        out
    }

    /// List all bindings for a specific action across every context.
    pub fn bindings_for(&self, action: &Action) -> Vec<(Context, Chord)> {
        let state = self.inner.read();
        let mut out = Vec::new();
        for (ctx, map) in &state.effective {
            for (chord, candidate) in map {
                if candidate == action {
                    out.push((*ctx, chord.clone()));
                }
            }
        }
        out
    }

    /// Export the current effective bindings as simple strings grouped by
    /// context so OpenTUI/IPC wiring can consume one resolved table.
    pub fn effective_snapshot(&self) -> EffectiveKeybindingsSnapshot {
        self.refresh_if_changed();

        let state = self.inner.read();
        let mut blocks = Vec::new();
        for ctx in Context::all() {
            let Some(map) = state.effective.get(ctx) else {
                continue;
            };
            if map.is_empty() {
                continue;
            }

            let mut bindings = HashMap::new();
            for (chord, action) in map {
                bindings.insert(chord.display(), action.as_str().to_string());
            }
            blocks.push(EffectiveKeybindingBlock {
                context: ctx.as_str().to_string(),
                bindings,
            });
        }
        EffectiveKeybindingsSnapshot { bindings: blocks }
    }
}

fn install_defaults(map: &mut HashMap<Context, HashMap<Chord, Action>>) {
    for (ctx, chord, action) in iter_parsed() {
        map.entry(ctx).or_default().insert(chord, action);
    }
}

fn apply_user(
    effective: &mut HashMap<Context, HashMap<Chord, Action>>,
    blocked: &mut HashMap<Context, Vec<Chord>>,
    user: &UserBindings,
) {
    for (ctx, entries) in &user.per_context {
        let ctx_map = effective.entry(*ctx).or_default();
        for (chord, value) in entries {
            match value {
                BindingValue::Unbind => {
                    ctx_map.retain(|candidate, _| !chord_has_prefix(candidate, chord));
                    blocked.entry(*ctx).or_default().push(chord.clone());
                }
                BindingValue::Bind(action) => {
                    ctx_map.insert(chord.clone(), action.clone());
                }
            }
        }
    }
}

fn resolve_single_from_map(
    map: &HashMap<Chord, Action>,
    stroke: &Keystroke,
    blocked: Option<&Vec<Chord>>,
) -> Option<Resolution> {
    for (chord, action) in map {
        if is_blocked_in_context(blocked, chord) {
            continue;
        }
        if chord.is_single() && chord.strokes()[0] == *stroke {
            return Some(Resolution::Action(action.clone()));
        }
    }
    for chord in map.keys() {
        if is_blocked_in_context(blocked, chord) {
            continue;
        }
        if chord.strokes().len() > 1 && chord.strokes()[0] == *stroke {
            return Some(Resolution::Pending);
        }
    }
    None
}

fn is_blocked_in_context(blocked: Option<&Vec<Chord>>, chord: &Chord) -> bool {
    blocked
        .map(|entries| {
            entries
                .iter()
                .any(|blocked_chord| chord_has_prefix(chord, blocked_chord))
        })
        .unwrap_or(false)
}

fn chord_has_prefix(candidate: &Chord, prefix: &Chord) -> bool {
    let candidate_strokes = candidate.strokes();
    let prefix_strokes = prefix.strokes();
    if prefix_strokes.len() > candidate_strokes.len() {
        return false;
    }
    candidate_strokes
        .iter()
        .zip(prefix_strokes.iter())
        .all(|(candidate_stroke, prefix_stroke)| candidate_stroke == prefix_stroke)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_registry_has_defaults() {
        let reg = KeybindingRegistry::with_defaults();
        let all = reg.all_bindings();
        assert!(all.len() > 30, "expected many defaults, got {}", all.len());
    }

    #[test]
    fn resolve_global_ctrl_c() {
        let reg = KeybindingRegistry::with_defaults();
        let stroke = Keystroke::parse("ctrl+c").unwrap();
        let result = reg.resolve_single(Context::Chat, &stroke);
        assert_eq!(
            result,
            Resolution::Action(Action::new_static("app:interrupt"))
        );
    }

    #[test]
    fn resolve_context_specific_enter_submits() {
        let reg = KeybindingRegistry::with_defaults();
        let stroke = Keystroke::parse("enter").unwrap();
        let result = reg.resolve_single(Context::Chat, &stroke);
        assert_eq!(
            result,
            Resolution::Action(Action::new_static("chat:submit"))
        );
    }

    #[test]
    fn resolve_chord_prefix_returns_pending() {
        let reg = KeybindingRegistry::with_defaults();
        let stroke = Keystroke::parse("ctrl+x").unwrap();
        let result = reg.resolve_single(Context::Chat, &stroke);
        assert_eq!(result, Resolution::Pending);
    }

    #[test]
    fn resolve_full_chord_returns_action() {
        let reg = KeybindingRegistry::with_defaults();
        let chord = Chord::parse("ctrl+x ctrl+k").unwrap();
        let result = reg.resolve_chord(Context::Chat, &chord);
        assert_eq!(
            result,
            Resolution::Action(Action::new_static("chat:killAgents"))
        );
    }

    #[test]
    fn user_can_override_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keybindings.json");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{
                "bindings": [
                    {{"context": "Chat", "bindings": {{"enter": "chat:newline"}}}}
                ]
            }}"#
        )
        .unwrap();
        drop(file);

        let reg = KeybindingRegistry::with_user_path(Some(path));
        let stroke = Keystroke::parse("enter").unwrap();
        let result = reg.resolve_single(Context::Chat, &stroke);
        assert_eq!(
            result,
            Resolution::Action(Action::new_static("chat:newline"))
        );
    }

    #[test]
    fn user_can_unbind_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keybindings.json");
        std::fs::write(
            &path,
            r#"{
                "bindings": [
                    {"context": "Chat", "bindings": {"ctrl+l": null}}
                ]
            }"#,
        )
        .unwrap();

        let reg = KeybindingRegistry::with_user_path(Some(path));
        let stroke = Keystroke::parse("ctrl+l").unwrap();
        let result = reg.resolve_single(Context::Chat, &stroke);
        assert_eq!(result, Resolution::None);
    }

    #[test]
    fn local_unbind_suppresses_global_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keybindings.json");
        std::fs::write(
            &path,
            r#"{
                "bindings": [
                    {"context": "Chat", "bindings": {"ctrl+c": null}}
                ]
            }"#,
        )
        .unwrap();

        let reg = KeybindingRegistry::with_user_path(Some(path));
        let stroke = Keystroke::parse("ctrl+c").unwrap();
        assert_eq!(reg.resolve_single(Context::Chat, &stroke), Resolution::None);
        assert_eq!(
            reg.resolve_single(Context::Global, &stroke),
            Resolution::Action(Action::new_static("app:interrupt"))
        );
    }

    #[test]
    fn local_prefix_unbind_suppresses_global_pending() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keybindings.json");
        std::fs::write(
            &path,
            r#"{
                "bindings": [
                    {"context": "Global", "bindings": {"ctrl+x ctrl+e": "app:exit"}},
                    {"context": "Chat", "bindings": {"ctrl+x": null}}
                ]
            }"#,
        )
        .unwrap();

        let reg = KeybindingRegistry::with_user_path(Some(path));
        let stroke = Keystroke::parse("ctrl+x").unwrap();
        let chord = Chord::parse("ctrl+x ctrl+e").unwrap();
        assert_eq!(reg.resolve_single(Context::Chat, &stroke), Resolution::None);
        assert_eq!(reg.resolve_chord(Context::Chat, &chord), Resolution::None);
        assert_eq!(
            reg.resolve_chord(Context::Global, &chord),
            Resolution::Action(Action::new_static("app:exit"))
        );
    }

    #[test]
    fn missing_user_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let reg = KeybindingRegistry::with_user_path(Some(path));
        let stroke = Keystroke::parse("ctrl+c").unwrap();
        assert_eq!(
            reg.resolve_single(Context::Global, &stroke),
            Resolution::Action(Action::new_static("app:interrupt"))
        );
    }

    #[test]
    fn hot_reload_picks_up_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keybindings.json");
        std::fs::write(
            &path,
            r#"{"bindings":[{"context":"Chat","bindings":{"enter":"chat:newline"}}]}"#,
        )
        .unwrap();
        let reg = KeybindingRegistry::with_user_path(Some(path.clone()));

        let stroke = Keystroke::parse("enter").unwrap();
        assert_eq!(
            reg.resolve_single(Context::Chat, &stroke),
            Resolution::Action(Action::new_static("chat:newline"))
        );

        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(
            &path,
            r#"{"bindings":[{"context":"Chat","bindings":{"enter":"chat:submit"}}]}"#,
        )
        .unwrap();

        assert_eq!(
            reg.resolve_single(Context::Chat, &stroke),
            Resolution::Action(Action::new_static("chat:submit"))
        );
    }

    #[test]
    fn parse_failure_retains_previous_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keybindings.json");
        std::fs::write(
            &path,
            r#"{"bindings":[{"context":"Chat","bindings":{"enter":"chat:newline"}}]}"#,
        )
        .unwrap();
        let reg = KeybindingRegistry::with_user_path(Some(path.clone()));

        let stroke = Keystroke::parse("enter").unwrap();
        assert_eq!(
            reg.resolve_single(Context::Chat, &stroke),
            Resolution::Action(Action::new_static("chat:newline"))
        );

        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(
            reg.resolve_single(Context::Chat, &stroke),
            Resolution::Action(Action::new_static("chat:newline")),
            "bad JSON must not clobber previous config"
        );
        let issues = reg.last_issues();
        assert!(!issues.is_empty(), "issues should be recorded");
    }

    #[test]
    fn bindings_for_action_covers_multiple_contexts() {
        let reg = KeybindingRegistry::with_defaults();
        let cancel = Action::new_static("chat:cancel");
        let hits = reg.bindings_for(&cancel);
        assert!(
            hits.iter().any(|(context, _)| *context == Context::Chat),
            "chat:cancel should be bound in Chat context"
        );
    }

    #[test]
    fn effective_snapshot_contains_context_blocks() {
        let reg = KeybindingRegistry::with_defaults();
        let snapshot = reg.effective_snapshot();
        assert!(
            snapshot
                .bindings
                .iter()
                .any(|block| block.context == "Global"),
            "snapshot should include the Global block"
        );
        assert!(
            snapshot
                .bindings
                .iter()
                .find(|block| block.context == "Chat")
                .and_then(|block| block.bindings.get("enter"))
                .is_some(),
            "snapshot should expose the Chat enter binding"
        );
    }
}
