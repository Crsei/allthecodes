# UI File-Level Allow Cleanup Deletions - 2026-05-20

## crates/claude-code-rs/src/ui/input/keybindings.rs

- Deleted objects: the local `Action`, `KeyBind`, `BindingContext`, and `KeybindingRegistry` implementations, their default binding table, and their unit tests.
- Reason: this file was an orphaned duplicate of the production `cc_keybindings` crate. The Rust TUI already stores and resolves keybindings through `cc_keybindings::KeybindingRegistry`, so the local registry could not be connected without reintroducing a competing source of truth.
- Recovery: restore the deleted implementation from Git history if a UI-local registry is explicitly needed. Prefer adding missing behavior to `crates/cc-keybindings` and importing it from the existing production call sites in `ui/app.rs` and `ui/app/input.rs`.
