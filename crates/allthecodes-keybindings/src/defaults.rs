//! Default keybinding set.
//!
//! Source of truth for the built-in bindings, shared by the Rust TUI and —
//! via the JSON config — by the OpenTUI frontend. Mirrors the tables in
//! `docs/claude-code-configuration/customize-keyboard-shortcuts.md`.

use super::action::Action;
use super::context::Context;
use super::keystroke::Chord;

/// One default binding entry: context + chord + action.
pub struct Default {
    pub context: Context,
    pub chord: &'static str,
    pub action: &'static str,
}

/// The canonical built-in binding set.
///
/// Uses spec-compliant chord strings and `namespace:action` identifiers so
/// the table can be dumped directly to JSON for documentation or consumed
/// by the OpenTUI frontend as-is.
pub const DEFAULTS: &[Default] = &[
    // -- Global ---------------------------------------------------------
    Default {
        context: Context::Global,
        chord: "ctrl+c",
        action: "app:interrupt",
    },
    Default {
        context: Context::Global,
        chord: "ctrl+d",
        action: "app:exit",
    },
    Default {
        context: Context::Global,
        chord: "ctrl+t",
        action: "app:toggleTodos",
    },
    Default {
        context: Context::Global,
        chord: "ctrl+o",
        action: "app:toggleTranscript",
    },
    Default {
        context: Context::Global,
        chord: "ctrl+r",
        action: "history:search",
    },
    Default {
        context: Context::Global,
        chord: "up",
        action: "history:previous",
    },
    Default {
        context: Context::Global,
        chord: "down",
        action: "history:next",
    },
    // -- Chat -----------------------------------------------------------
    Default {
        context: Context::Chat,
        chord: "escape",
        action: "chat:cancel",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+l",
        action: "chat:clearInput",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+x ctrl+k",
        action: "chat:killAgents",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+x ctrl+a",
        action: "agents:tree",
    },
    Default {
        context: Context::Chat,
        chord: "shift+tab",
        action: "chat:cycleMode",
    },
    Default {
        context: Context::Chat,
        chord: "meta+p",
        action: "chat:modelPicker",
    },
    Default {
        context: Context::Chat,
        chord: "meta+o",
        action: "chat:fastMode",
    },
    Default {
        context: Context::Chat,
        chord: "meta+t",
        action: "chat:thinkingToggle",
    },
    Default {
        context: Context::Chat,
        chord: "enter",
        action: "chat:submit",
    },
    Default {
        context: Context::Chat,
        chord: "tab",
        action: "chat:queue",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+j",
        action: "chat:newline",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+g",
        action: "chat:externalEditor",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+x ctrl+e",
        action: "chat:externalEditor",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+s",
        action: "chat:stash",
    },
    Default {
        context: Context::Chat,
        chord: "ctrl+v",
        action: "chat:imagePaste",
    },
    Default {
        context: Context::Chat,
        chord: "shift+up",
        action: "chat:messageActions",
    },
    // Voice dictation (issue #13). Hold Ctrl+Space to capture audio;
    // release to transcribe. Ctrl+Space is chosen over bare Space so the
    // space bar still types a space. The user can rebind via
    // `~/.allthecodes/keybindings.json` -> `voice:pushToTalk`.
    Default {
        context: Context::Chat,
        chord: "ctrl+space",
        action: "voice:pushToTalk",
    },
    // -- Autocomplete ---------------------------------------------------
    Default {
        context: Context::Autocomplete,
        chord: "tab",
        action: "autocomplete:accept",
    },
    Default {
        context: Context::Autocomplete,
        chord: "escape",
        action: "autocomplete:dismiss",
    },
    Default {
        context: Context::Autocomplete,
        chord: "up",
        action: "autocomplete:previous",
    },
    Default {
        context: Context::Autocomplete,
        chord: "down",
        action: "autocomplete:next",
    },
    // -- Confirmation ---------------------------------------------------
    Default {
        context: Context::Confirmation,
        chord: "y",
        action: "confirm:yes",
    },
    Default {
        context: Context::Confirmation,
        chord: "enter",
        action: "confirm:yes",
    },
    Default {
        context: Context::Confirmation,
        chord: "n",
        action: "confirm:no",
    },
    Default {
        context: Context::Confirmation,
        chord: "escape",
        action: "confirm:no",
    },
    Default {
        context: Context::Confirmation,
        chord: "up",
        action: "confirm:previous",
    },
    Default {
        context: Context::Confirmation,
        chord: "down",
        action: "confirm:next",
    },
    Default {
        context: Context::Confirmation,
        chord: "tab",
        action: "confirm:nextField",
    },
    Default {
        context: Context::Confirmation,
        chord: "space",
        action: "confirm:toggle",
    },
    Default {
        context: Context::Confirmation,
        chord: "shift+tab",
        action: "confirm:cycleMode",
    },
    Default {
        context: Context::Confirmation,
        chord: "ctrl+e",
        action: "confirm:toggleExplanation",
    },
    // -- Transcript -----------------------------------------------------
    Default {
        context: Context::Transcript,
        chord: "ctrl+e",
        action: "transcript:toggleShowAll",
    },
    Default {
        context: Context::Transcript,
        chord: "q",
        action: "transcript:exit",
    },
    Default {
        context: Context::Transcript,
        chord: "escape",
        action: "transcript:exit",
    },
    // -- History search -------------------------------------------------
    Default {
        context: Context::HistorySearch,
        chord: "ctrl+r",
        action: "historySearch:next",
    },
    Default {
        context: Context::HistorySearch,
        chord: "escape",
        action: "historySearch:accept",
    },
    Default {
        context: Context::HistorySearch,
        chord: "tab",
        action: "historySearch:accept",
    },
    Default {
        context: Context::HistorySearch,
        chord: "ctrl+c",
        action: "historySearch:cancel",
    },
    Default {
        context: Context::HistorySearch,
        chord: "enter",
        action: "historySearch:execute",
    },
    // -- Task -----------------------------------------------------------
    Default {
        context: Context::Task,
        chord: "ctrl+b",
        action: "task:background",
    },
    // -- Theme picker ---------------------------------------------------
    Default {
        context: Context::ThemePicker,
        chord: "ctrl+t",
        action: "theme:toggleSyntaxHighlighting",
    },
    // -- Help -----------------------------------------------------------
    Default {
        context: Context::Help,
        chord: "escape",
        action: "help:dismiss",
    },
    // -- Tabs -----------------------------------------------------------
    Default {
        context: Context::Tabs,
        chord: "tab",
        action: "tabs:next",
    },
    Default {
        context: Context::Tabs,
        chord: "right",
        action: "tabs:next",
    },
    Default {
        context: Context::Tabs,
        chord: "shift+tab",
        action: "tabs:previous",
    },
    Default {
        context: Context::Tabs,
        chord: "left",
        action: "tabs:previous",
    },
    // -- Attachments ----------------------------------------------------
    Default {
        context: Context::Attachments,
        chord: "right",
        action: "attachments:next",
    },
    Default {
        context: Context::Attachments,
        chord: "left",
        action: "attachments:previous",
    },
    Default {
        context: Context::Attachments,
        chord: "backspace",
        action: "attachments:remove",
    },
    Default {
        context: Context::Attachments,
        chord: "delete",
        action: "attachments:remove",
    },
    Default {
        context: Context::Attachments,
        chord: "escape",
        action: "attachments:exit",
    },
    // -- Footer ---------------------------------------------------------
    Default {
        context: Context::Footer,
        chord: "right",
        action: "footer:next",
    },
    Default {
        context: Context::Footer,
        chord: "left",
        action: "footer:previous",
    },
    Default {
        context: Context::Footer,
        chord: "up",
        action: "footer:up",
    },
    Default {
        context: Context::Footer,
        chord: "down",
        action: "footer:down",
    },
    Default {
        context: Context::Footer,
        chord: "enter",
        action: "footer:openSelected",
    },
    Default {
        context: Context::Footer,
        chord: "escape",
        action: "footer:clearSelection",
    },
    // -- Message selector ----------------------------------------------
    Default {
        context: Context::MessageSelector,
        chord: "up",
        action: "messageSelector:up",
    },
    Default {
        context: Context::MessageSelector,
        chord: "k",
        action: "messageSelector:up",
    },
    Default {
        context: Context::MessageSelector,
        chord: "ctrl+p",
        action: "messageSelector:up",
    },
    Default {
        context: Context::MessageSelector,
        chord: "down",
        action: "messageSelector:down",
    },
    Default {
        context: Context::MessageSelector,
        chord: "j",
        action: "messageSelector:down",
    },
    Default {
        context: Context::MessageSelector,
        chord: "ctrl+n",
        action: "messageSelector:down",
    },
    Default {
        context: Context::MessageSelector,
        chord: "enter",
        action: "messageSelector:select",
    },
    // -- Message actions ------------------------------------------------
    Default {
        context: Context::MessageActions,
        chord: "up",
        action: "messageActions:prev",
    },
    Default {
        context: Context::MessageActions,
        chord: "down",
        action: "messageActions:next",
    },
    Default {
        context: Context::MessageActions,
        chord: "k",
        action: "messageActions:prev",
    },
    Default {
        context: Context::MessageActions,
        chord: "j",
        action: "messageActions:next",
    },
    Default {
        context: Context::MessageActions,
        chord: "meta+up",
        action: "messageActions:top",
    },
    Default {
        context: Context::MessageActions,
        chord: "meta+down",
        action: "messageActions:bottom",
    },
    Default {
        context: Context::MessageActions,
        chord: "shift+up",
        action: "messageActions:prevUser",
    },
    Default {
        context: Context::MessageActions,
        chord: "shift+down",
        action: "messageActions:nextUser",
    },
    Default {
        context: Context::MessageActions,
        chord: "escape",
        action: "messageActions:escape",
    },
    Default {
        context: Context::MessageActions,
        chord: "ctrl+c",
        action: "messageActions:ctrlc",
    },
    Default {
        context: Context::MessageActions,
        chord: "enter",
        action: "messageActions:enter",
    },
    Default {
        context: Context::MessageActions,
        chord: "o",
        action: "messageActions:o",
    },
    Default {
        context: Context::MessageActions,
        chord: "c",
        action: "messageActions:c",
    },
    Default {
        context: Context::MessageActions,
        chord: "p",
        action: "messageActions:p",
    },
    // -- Diff dialog ----------------------------------------------------
    Default {
        context: Context::DiffDialog,
        chord: "escape",
        action: "diff:dismiss",
    },
    Default {
        context: Context::DiffDialog,
        chord: "left",
        action: "diff:previousSource",
    },
    Default {
        context: Context::DiffDialog,
        chord: "right",
        action: "diff:nextSource",
    },
    Default {
        context: Context::DiffDialog,
        chord: "up",
        action: "diff:previousFile",
    },
    Default {
        context: Context::DiffDialog,
        chord: "down",
        action: "diff:nextFile",
    },
    Default {
        context: Context::DiffDialog,
        chord: "enter",
        action: "diff:viewDetails",
    },
    // -- Model picker ---------------------------------------------------
    Default {
        context: Context::ModelPicker,
        chord: "left",
        action: "modelPicker:decreaseEffort",
    },
    Default {
        context: Context::ModelPicker,
        chord: "right",
        action: "modelPicker:increaseEffort",
    },
    // -- Select ---------------------------------------------------------
    Default {
        context: Context::Select,
        chord: "down",
        action: "select:next",
    },
    Default {
        context: Context::Select,
        chord: "j",
        action: "select:next",
    },
    Default {
        context: Context::Select,
        chord: "ctrl+n",
        action: "select:next",
    },
    Default {
        context: Context::Select,
        chord: "up",
        action: "select:previous",
    },
    Default {
        context: Context::Select,
        chord: "k",
        action: "select:previous",
    },
    Default {
        context: Context::Select,
        chord: "ctrl+p",
        action: "select:previous",
    },
    Default {
        context: Context::Select,
        chord: "enter",
        action: "select:accept",
    },
    Default {
        context: Context::Select,
        chord: "escape",
        action: "select:cancel",
    },
    // -- Plugin ---------------------------------------------------------
    Default {
        context: Context::Plugin,
        chord: "space",
        action: "plugin:toggle",
    },
    Default {
        context: Context::Plugin,
        chord: "i",
        action: "plugin:install",
    },
    // -- Settings -------------------------------------------------------
    Default {
        context: Context::Settings,
        chord: "/",
        action: "settings:search",
    },
    Default {
        context: Context::Settings,
        chord: "r",
        action: "settings:retry",
    },
    Default {
        context: Context::Settings,
        chord: "enter",
        action: "settings:close",
    },
    // -- Doctor ---------------------------------------------------------
    Default {
        context: Context::Doctor,
        chord: "f",
        action: "doctor:fix",
    },
    // -- Scroll (fullscreen) --------------------------------------------
    Default {
        context: Context::Scroll,
        chord: "pageup",
        action: "scroll:pageUp",
    },
    Default {
        context: Context::Scroll,
        chord: "pagedown",
        action: "scroll:pageDown",
    },
    Default {
        context: Context::Scroll,
        chord: "ctrl+home",
        action: "scroll:top",
    },
    Default {
        context: Context::Scroll,
        chord: "ctrl+end",
        action: "scroll:bottom",
    },
    Default {
        context: Context::Scroll,
        chord: "ctrl+shift+c",
        action: "selection:copy",
    },
    // -- Busy -----------------------------------------------------------
    Default {
        context: Context::Busy,
        chord: "enter",
        action: "chat:steer",
    },
    Default {
        context: Context::Busy,
        chord: "tab",
        action: "chat:queue",
    },
];

/// Iterate over the defaults as typed (chord, action) pairs.
pub fn iter_parsed() -> impl Iterator<Item = (Context, Chord, Action)> {
    DEFAULTS.iter().filter_map(|d| {
        Some((
            d.context,
            Chord::parse(d.chord).ok()?,
            Action::new_static(d.action),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_chord_parses() {
        for d in DEFAULTS {
            Chord::parse(d.chord).unwrap_or_else(|e| {
                panic!(
                    "default chord '{}' for '{}' fails to parse: {}",
                    d.chord, d.action, e
                )
            });
        }
    }

    #[test]
    fn every_default_action_parses() {
        for d in DEFAULTS {
            Action::parse(d.action)
                .unwrap_or_else(|e| panic!("default action '{}' fails to parse: {}", d.action, e));
        }
    }

    #[test]
    fn iter_parsed_covers_every_default() {
        let count = iter_parsed().count();
        assert_eq!(count, DEFAULTS.len());
    }

    #[test]
    fn spec_anchor_actions_present() {
        // Sanity check: a few spec-anchor actions must be in the built-in set
        let actions: Vec<&str> = DEFAULTS.iter().map(|d| d.action).collect();
        for must in [
            "app:interrupt",
            "app:exit",
            "chat:submit",
            "chat:cancel",
            "chat:clearInput",
            "autocomplete:accept",
            "history:search",
            "transcript:exit",
            "scroll:pageUp",
            "select:cancel",
        ] {
            assert!(actions.contains(&must), "missing default: {}", must);
        }
    }

    #[test]
    fn no_ctrl_c_conflict_outside_global() {
        // Ctrl+C is the reserved interrupt; shouldn't be rebound elsewhere
        // except for HistorySearch's explicit cancel (which matches spec).
        for d in DEFAULTS {
            if d.chord == "ctrl+c"
                && d.context != Context::Global
                && d.context != Context::HistorySearch
                && d.context != Context::MessageActions
            {
                panic!("ctrl+c bound outside Global/HistorySearch: {:?}", d.context);
            }
        }
    }
}
