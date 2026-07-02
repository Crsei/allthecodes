# Phase 4 TUI State Decoupling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Rust TUI consume domain stores and view-models instead of re-deriving business semantics inside render and input paths.

**Architecture:** Keep the current `crates/allthecodes/src/ui/` crate boundary and move state ownership into focused modules under `ui/app/` and `ui/messages/`. `App` remains the public facade for TUI runner code, but conversation data, overlays, message render state, and runtime navigation state get local stores with unit tests. Rendering receives view-model snapshots; input dispatch asks the overlay dispatcher which surface owns the key event.

**Tech Stack:** Rust 1.91.1, ratatui, crossterm, existing `allthecodes-types`, existing TUI test helpers, `cargo test -p allthecodes`.

## Global Constraints

- Work from a clean worktree based on `codebase-opt-phase3` commit `ccd3cf53`.
- Only modify Rust TUI code under `crates/allthecodes/src/ui/` unless a test needs a local fixture.
- Do not introduce new crates or dependencies.
- Preserve full-build behavior; do not remove upstream-compatible branches as "simplification".
- Keep path isolation: all persistent paths remain `.allthecodes` / `~/.allthecodes`.
- Use the repository cargo environment:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

---

## File Structure

- Create `crates/allthecodes/src/ui/app/domain.rs`: conversation, prompt queue, session metadata, and dirty/render-layout stores used by `App`.
- Create `crates/allthecodes/src/ui/app/overlays.rs`: overlay ownership, overlay priority, key dispatch, and overlay view state.
- Create `crates/allthecodes/src/ui/messages/view_model.rs`: stable `MessageListViewModel` built from existing message normalization and render context.
- Create `crates/allthecodes/src/ui/app/runtime_state.rs`: agent/task/MCP runtime store facade used by backend event application and footer/tree rendering.
- Modify `crates/allthecodes/src/ui/app.rs`: keep `App` public methods, move direct field ownership to the new stores incrementally.
- Modify `crates/allthecodes/src/ui/app/input.rs`: delegate overlay key handling to `OverlayState`.
- Modify `crates/allthecodes/src/ui/app/render.rs`: render from overlay and message view-model snapshots.
- Modify `crates/allthecodes/src/ui/messages/render/context.rs`: expose prepared render context through the view-model module without duplicating preprocessing.
- Modify `crates/allthecodes/src/ui/messages/render/mod.rs`: consume `MessageListViewModel` in the main message render path.
- Modify `crates/allthecodes/src/ui/app/tests.rs`: add store and dispatcher regression coverage.

---

### Task 1: Create App Domain Stores

**Files:**
- Create: `crates/allthecodes/src/ui/app/domain.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/input.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Test: `crates/allthecodes/src/ui/app/domain.rs`

**Interfaces:**
- Produces: `ConversationStore`, `PromptQueueStore`, `SessionUiStore`, `RenderLayoutStore`.
- Consumes: existing `Message`, `HistorySearchEntry`, `VirtualScroll`, `Rect`.
- Later tasks rely on:
  - `ConversationStore::messages(&self) -> &[Message]`
  - `ConversationStore::add_message(&mut self, Message)`
  - `ConversationStore::replace_last_message(&mut self, Message)`
  - `ConversationStore::remove_last_message(&mut self)`
  - `ConversationStore::clear(&mut self)`
  - `ConversationStore::selection(&self) -> Option<usize>`
  - `ConversationStore::set_selection(&mut self, Option<usize>)`
  - `ConversationStore::render_context_inputs(&self) -> (Option<usize>, bool)`
  - `PromptQueueStore::queue(&mut self, String) -> usize`
  - `PromptQueueStore::pop_next(&mut self) -> Option<String>`

- [ ] **Step 1: Write failing domain-store tests**

Add this test module to the new file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, MessageContent, UserMessage};

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn conversation_store_clamps_selection_after_remove() {
        let mut store = ConversationStore::default();
        store.add_message(user_message("one"));
        store.add_message(user_message("two"));
        store.set_selection(Some(1));

        store.remove_last_message();

        assert_eq!(store.messages().len(), 1);
        assert_eq!(store.selection(), Some(0));
    }

    #[test]
    fn prompt_queue_store_preserves_fifo_order() {
        let mut store = PromptQueueStore::default();

        assert_eq!(store.queue("first".to_string()), 1);
        assert_eq!(store.queue("second".to_string()), 2);

        assert_eq!(store.pop_next().as_deref(), Some("first"));
        assert_eq!(store.pop_next().as_deref(), Some("second"));
        assert_eq!(store.pop_next(), None);
    }
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p allthecodes ui::app::domain
```

Expected: compile fails because `ui::app::domain` and the store types do not exist.

- [ ] **Step 3: Add store implementations**

Create `crates/allthecodes/src/ui/app/domain.rs`:

```rust
use std::collections::VecDeque;

use allthecodes_types::message::Message;
use ratatui::layout::Rect;

use crate::ui::history_search_dialog::HistorySearchEntry;
use crate::ui::virtual_scroll::VirtualScroll;

#[derive(Debug, Default)]
pub(super) struct ConversationStore {
    messages: Vec<Message>,
    selected_message: Option<usize>,
    selected_message_expanded: bool,
    scroll_offset: usize,
    vscroll: VirtualScroll,
}

impl ConversationStore {
    pub(super) fn add_message(&mut self, msg: Message) {
        self.messages.push(msg);
        self.clamp_selection();
    }

    pub(super) fn replace_last_message(&mut self, msg: Message) {
        if let Some(last) = self.messages.last_mut() {
            *last = msg;
        } else {
            self.messages.push(msg);
        }
        self.clamp_selection();
    }

    pub(super) fn remove_last_message(&mut self) {
        self.messages.pop();
        self.clamp_selection();
    }

    pub(super) fn clear(&mut self) {
        self.messages.clear();
        self.selected_message = None;
        self.scroll_offset = 0;
        self.vscroll.clear();
    }

    pub(super) fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub(super) fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    pub(super) fn selection(&self) -> Option<usize> {
        self.selected_message
    }

    pub(super) fn set_selection(&mut self, selected: Option<usize>) {
        self.selected_message = selected;
        self.clamp_selection();
    }

    pub(super) fn selected_expanded(&self) -> bool {
        self.selected_message_expanded
    }

    pub(super) fn set_selected_expanded(&mut self, expanded: bool) {
        self.selected_message_expanded = expanded;
    }

    pub(super) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(super) fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    pub(super) fn vscroll(&self) -> &VirtualScroll {
        &self.vscroll
    }

    pub(super) fn vscroll_mut(&mut self) -> &mut VirtualScroll {
        &mut self.vscroll
    }

    pub(super) fn render_context_inputs(&self) -> (Option<usize>, bool) {
        (self.selected_message, self.selected_message_expanded)
    }

    fn clamp_selection(&mut self) {
        if self.messages.is_empty() {
            self.selected_message = None;
            self.selected_message_expanded = false;
            return;
        }
        if let Some(index) = self.selected_message {
            self.selected_message = Some(index.min(self.messages.len() - 1));
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct PromptQueueStore {
    queued_prompts: VecDeque<String>,
}

impl PromptQueueStore {
    pub(super) fn queue(&mut self, text: String) -> usize {
        self.queued_prompts.push_back(text);
        self.queued_prompts.len()
    }

    pub(super) fn pop_next(&mut self) -> Option<String> {
        self.queued_prompts.pop_front()
    }

    pub(super) fn len(&self) -> usize {
        self.queued_prompts.len()
    }
}

#[derive(Debug, Default)]
pub(super) struct SessionUiStore {
    pub(super) model_name: String,
    pub(super) backend_name: String,
    pub(super) session_id: String,
    pub(super) cwd: String,
    pub(super) output_style: Option<String>,
    pub(super) history: Vec<HistorySearchEntry>,
}

#[derive(Debug, Default)]
pub(super) struct RenderLayoutStore {
    pub(super) session_scrollbar: Option<super::SessionScrollbarState>,
    pub(super) session_scrollbar_dragging: bool,
    pub(super) message_area: Option<Rect>,
    pub(super) prompt_area: Option<Rect>,
}
```

- [ ] **Step 4: Wire `App` to the stores without changing public methods**

In `crates/allthecodes/src/ui/app.rs`, add:

```rust
mod domain;

use domain::{ConversationStore, PromptQueueStore, RenderLayoutStore, SessionUiStore};
```

Replace the direct fields:

```rust
messages: Vec<Message>,
selected_message: Option<usize>,
selected_message_expanded: bool,
queued_prompts: VecDeque<String>,
model_name: String,
backend_name: String,
session_id: String,
cwd: String,
output_style: Option<String>,
history: Vec<HistorySearchEntry>,
vscroll: VirtualScroll,
session_scrollbar: Option<SessionScrollbarState>,
session_scrollbar_dragging: bool,
message_area: Option<Rect>,
prompt_area: Option<Rect>,
```

with:

```rust
conversation: ConversationStore,
prompt_queue: PromptQueueStore,
session_ui: SessionUiStore,
render_layout: RenderLayoutStore,
```

Update existing `App` methods to delegate to the stores. For example:

```rust
pub fn add_message(&mut self, msg: Message) {
    self.conversation.add_message(msg);
    self.scroll_to_bottom_deferred();
    self.dirty = true;
}

pub fn messages(&self) -> &[Message] {
    self.conversation.messages()
}

pub fn queue_prompt(&mut self, text: String) -> usize {
    let count = self.prompt_queue.queue(text);
    self.dirty = true;
    count
}

pub fn pop_next_queued(&mut self) -> Option<String> {
    self.prompt_queue.pop_next()
}

pub fn queued_count(&self) -> usize {
    self.prompt_queue.len()
}
```

- [ ] **Step 5: Replace internal field reads mechanically**

Replace reads in `input.rs` and `render.rs` with store accessors:

```rust
self.messages
self.selected_message
self.selected_message_expanded
self.scroll_offset
self.vscroll
self.session_scrollbar
self.session_scrollbar_dragging
self.message_area
self.prompt_area
self.history
self.model_name
self.backend_name
self.session_id
self.cwd
self.output_style
```

become:

```rust
self.conversation.messages()
self.conversation.selection()
self.conversation.selected_expanded()
self.conversation.scroll_offset()
self.conversation.vscroll()
self.render_layout.session_scrollbar
self.render_layout.session_scrollbar_dragging
self.render_layout.message_area
self.render_layout.prompt_area
self.session_ui.history
self.session_ui.model_name
self.session_ui.backend_name
self.session_ui.session_id
self.session_ui.cwd
self.session_ui.output_style
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p allthecodes ui::app::domain
cargo test -p allthecodes ui::app::tests
```

Expected: both pass.

- [ ] **Step 7: Commit Task 1**

```bash
git add -A -- crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app/domain.rs crates/allthecodes/src/ui/app/input.rs crates/allthecodes/src/ui/app/render.rs
git commit -m "refactor(tui): split app domain stores"
```

---

### Task 2: Add Overlay Dispatcher

**Files:**
- Create: `crates/allthecodes/src/ui/app/overlays.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/input.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Test: `crates/allthecodes/src/ui/app/overlays.rs`

**Interfaces:**
- Produces: `OverlayState`, `ActiveOverlay`, `OverlayOutcome`.
- Consumes: existing `PermissionDialog`, `QuestionDialog`, `BypassPermissionsModeDialog`, `CommandSurface`, `HistorySearchDialog`, `AgentTreeDialog`.
- Later tasks rely on `OverlayState::active_overlay() -> Option<ActiveOverlay>` as the single overlay priority source.

- [ ] **Step 1: Write failing overlay-priority tests**

Create `crates/allthecodes/src/ui/app/overlays.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_priority_prefers_permission_question_before_command_surface() {
        let mut overlays = OverlayState::default();
        overlays.set_command_surface_for_test();
        overlays.set_question_for_test();
        overlays.set_permission_for_test();

        assert_eq!(overlays.active_overlay(), Some(ActiveOverlay::Permission));

        overlays.clear_permission();
        assert_eq!(overlays.active_overlay(), Some(ActiveOverlay::Question));

        overlays.clear_question();
        assert_eq!(overlays.active_overlay(), Some(ActiveOverlay::CommandSurface));
    }
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p allthecodes ui::app::overlays
```

Expected: compile fails because `OverlayState` and `ActiveOverlay` do not exist.

- [ ] **Step 3: Implement overlay state and priority**

Create:

```rust
use crate::ui::command_surface::CommandSurface;
use crate::ui::history_search_dialog::HistorySearchDialog;
use crate::ui::permissions::bypass_permissions_mode_dialog::BypassPermissionsModeDialog;
use crate::ui::permissions::dialog_overlay::PermissionDialog;
use crate::ui::permissions::question_dialog::QuestionDialog;

use super::agent_tree_dialog::AgentTreeDialog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActiveOverlay {
    BypassPermissions,
    Question,
    Permission,
    AgentTree,
    HistorySearch,
    CommandSurface,
}

#[derive(Default)]
pub(super) struct OverlayState {
    pub(super) bypass_permissions_mode_dialog: Option<BypassPermissionsModeDialog>,
    pub(super) permission_dialog: Option<PermissionDialog>,
    pub(super) question_dialog: Option<QuestionDialog>,
    pub(super) command_surface: Option<CommandSurface>,
    pub(super) history_search_dialog: Option<HistorySearchDialog>,
    pub(super) agent_tree_dialog: Option<AgentTreeDialog>,
}

impl OverlayState {
    pub(super) fn active_overlay(&self) -> Option<ActiveOverlay> {
        if self.bypass_permissions_mode_dialog.is_some() {
            return Some(ActiveOverlay::BypassPermissions);
        }
        if self.question_dialog.is_some() {
            return Some(ActiveOverlay::Question);
        }
        if self.permission_dialog.is_some() {
            return Some(ActiveOverlay::Permission);
        }
        if self.agent_tree_dialog.is_some() {
            return Some(ActiveOverlay::AgentTree);
        }
        if self.history_search_dialog.is_some() {
            return Some(ActiveOverlay::HistorySearch);
        }
        if self.command_surface.is_some() {
            return Some(ActiveOverlay::CommandSurface);
        }
        None
    }

    pub(super) fn clear_permission(&mut self) {
        self.permission_dialog = None;
    }

    pub(super) fn clear_question(&mut self) {
        self.question_dialog = None;
    }
}
```

Add `#[cfg(test)]` constructors in the same file using existing constructors with harmless data.

- [ ] **Step 4: Move overlay fields from `App` to `OverlayState`**

In `app.rs`, add:

```rust
mod overlays;
use overlays::{ActiveOverlay, OverlayState};
```

Replace these `App` fields:

```rust
bypass_permissions_mode_dialog: Option<BypassPermissionsModeDialog>,
permission_dialog: Option<PermissionDialog>,
question_dialog: Option<QuestionDialog>,
command_surface: Option<CommandSurface>,
history_search_dialog: Option<HistorySearchDialog>,
agent_tree_dialog: Option<AgentTreeDialog>,
```

with:

```rust
overlays: OverlayState,
```

- [ ] **Step 5: Delegate input overlay priority**

In `input.rs`, replace the top of `handle_key_event` overlay chain with:

```rust
match self.overlays.active_overlay() {
    Some(ActiveOverlay::BypassPermissions) => return self.handle_bypass_permissions_key(key),
    Some(ActiveOverlay::Question) => return self.handle_question_key(key),
    Some(ActiveOverlay::Permission) => return self.handle_permission_key(key),
    Some(ActiveOverlay::AgentTree) => return self.handle_agent_tree_key(key),
    Some(ActiveOverlay::HistorySearch) => return self.handle_history_search_key(key),
    Some(ActiveOverlay::CommandSurface) => return self.handle_command_surface_key(key),
    None => {}
}
```

Split the existing inline blocks into these small methods on `App`:

```rust
fn handle_question_key(&mut self, key: KeyEvent) -> AppAction
fn handle_permission_key(&mut self, key: KeyEvent) -> AppAction
fn handle_bypass_permissions_key(&mut self, key: KeyEvent) -> AppAction
```

- [ ] **Step 6: Update render overlay reads**

In `render.rs`, replace direct reads:

```rust
self.permission_dialog
self.question_dialog
self.command_surface
self.history_search_dialog
self.agent_tree_dialog
```

with:

```rust
self.overlays.permission_dialog
self.overlays.question_dialog
self.overlays.command_surface
self.overlays.history_search_dialog
self.overlays.agent_tree_dialog
```

- [ ] **Step 7: Run focused tests**

```bash
cargo test -p allthecodes ui::app::overlays
cargo test -p allthecodes agent_tree_dialog_navigation_select_and_close
cargo test -p allthecodes question_dialog_collects_answer
cargo test -p allthecodes permission_dialog_can_render_above_prompt
```

Expected: all pass.

- [ ] **Step 8: Commit Task 2**

```bash
git add -A -- crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app/input.rs crates/allthecodes/src/ui/app/render.rs crates/allthecodes/src/ui/app/overlays.rs
git commit -m "refactor(tui): centralize overlay dispatch"
```

---

### Task 3: Introduce Message View-Model

**Files:**
- Create: `crates/allthecodes/src/ui/messages/view_model.rs`
- Modify: `crates/allthecodes/src/ui/messages.rs`
- Modify: `crates/allthecodes/src/ui/messages/render/context.rs`
- Modify: `crates/allthecodes/src/ui/messages/render/mod.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Test: `crates/allthecodes/src/ui/messages/view_model.rs`

**Interfaces:**
- Produces: `MessageListViewModel`.
- Consumes: existing `MessageRenderOptions`, `MessageRenderContext`, `RenderableMessage`.
- Render path must no longer build message preprocessing state ad hoc inside `App::render`.

- [ ] **Step 1: Write failing view-model tests**

Create:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, MessageContent, UserMessage};

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn message_view_model_preserves_selection_inputs() {
        let messages = vec![user_message("hello")];
        let vm = MessageListViewModel::build(
            &messages,
            Some(0),
            true,
            MessageRenderOptions::default(),
        );

        assert_eq!(vm.source_len(), 1);
        assert_eq!(vm.render_context().selected_message(), Some(0));
        assert!(vm.render_context().selected_expanded());
    }
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test -p allthecodes ui::messages::view_model
```

Expected: compile fails because `MessageListViewModel` does not exist.

- [ ] **Step 3: Expose read-only context accessors**

In `messages/render/context.rs`, add:

```rust
impl MessageRenderContext {
    pub(crate) fn selected_message(&self) -> Option<usize> {
        self.selected_message
    }

    pub(crate) fn selected_expanded(&self) -> bool {
        self.selected_expanded
    }
}
```

- [ ] **Step 4: Implement the view-model**

Create `messages/view_model.rs`:

```rust
use allthecodes_types::message::Message;

use crate::ui::messages::render::{
    build_message_render_context_with_options, MessageRenderContext, MessageRenderOptions,
};

#[derive(Debug, Clone)]
pub(crate) struct MessageListViewModel {
    source_len: usize,
    render_context: MessageRenderContext,
}

impl MessageListViewModel {
    pub(crate) fn build(
        messages: &[Message],
        selected_message: Option<usize>,
        selected_expanded: bool,
        options: MessageRenderOptions,
    ) -> Self {
        Self {
            source_len: messages.len(),
            render_context: build_message_render_context_with_options(
                messages,
                selected_message,
                selected_expanded,
                options,
            ),
        }
    }

    pub(crate) fn source_len(&self) -> usize {
        self.source_len
    }

    pub(crate) fn render_context(&self) -> &MessageRenderContext {
        &self.render_context
    }
}
```

In `messages.rs`, export the module:

```rust
pub mod view_model;
```

- [ ] **Step 5: Use the view-model in render**

In `app/render.rs`, replace direct `build_message_render_context_with_options(...)` calls with:

```rust
let (selected, selected_expanded) = self.conversation.render_context_inputs();
let message_vm = MessageListViewModel::build(
    self.conversation.messages(),
    selected,
    selected_expanded,
    MessageRenderOptions {
        verbose: self.verbose,
        is_transcript_mode: self.view_mode == ViewMode::Transcript,
        thinking_animation_frame: self.thinking_animation_frame(),
        ..MessageRenderOptions::default()
    },
);
```

Then pass `message_vm.render_context()` into `render_messages`.

- [ ] **Step 6: Run message-render tests**

```bash
cargo test -p allthecodes ui::messages::view_model
cargo test -p allthecodes messages_render_path_image_compact_and_interrupt_summaries
cargo test -p allthecodes render_pipeline_renders_read_search_operations_and_hides_microcompact
```

Expected: all pass.

- [ ] **Step 7: Commit Task 3**

```bash
git add -A -- crates/allthecodes/src/ui/messages.rs crates/allthecodes/src/ui/messages/view_model.rs crates/allthecodes/src/ui/messages/render/context.rs crates/allthecodes/src/ui/messages/render/mod.rs crates/allthecodes/src/ui/app/render.rs
git commit -m "refactor(tui): add message view model"
```

---

### Task 4: Add Typed Runtime View State

**Files:**
- Create: `crates/allthecodes/src/ui/app/runtime_state.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/agent_navigation.rs`
- Modify: `crates/allthecodes/src/ui/tasks/mod.rs`
- Modify: `crates/allthecodes/src/ui/mcp/mod.rs`
- Test: `crates/allthecodes/src/ui/app/runtime_state.rs`

**Interfaces:**
- Produces: `RuntimeViewState`.
- Consumes: existing `AgentNavigationState`, `TaskStatus`, and MCP render modules.
- Later render/input code reads agent/task/MCP state through `App::runtime_state()`.

- [ ] **Step 1: Write failing runtime-state tests**

Create:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::agent_navigation::{AgentThreadEntry, AgentThreadStatus};

    #[test]
    fn runtime_view_state_tracks_current_agent_thread() {
        let mut state = RuntimeViewState::default();
        state.upsert_agent(AgentThreadEntry {
            thread_id: "worker-1".to_string(),
            parent_thread_id: Some("session-main".to_string()),
            label: "Build worker".to_string(),
            status: AgentThreadStatus::Running,
            summary: None,
        });
        state.set_current_agent_thread(Some("worker-1".to_string()));

        assert_eq!(state.current_agent_thread_id().as_deref(), Some("worker-1"));
        assert_eq!(state.agent_nav().thread_count(), 1);
    }
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test -p allthecodes ui::app::runtime_state
```

Expected: compile fails because `RuntimeViewState` does not exist.

- [ ] **Step 3: Implement runtime state facade**

Create:

```rust
use crate::ui::app::agent_navigation::{AgentNavigationState, AgentThreadEntry};
use crate::ui::tasks::TaskStatus;

#[derive(Debug, Default)]
pub(super) struct RuntimeViewState {
    agent_nav: AgentNavigationState,
    current_agent_thread_id: Option<String>,
    tasks: Vec<TaskStatus>,
}

impl RuntimeViewState {
    pub(super) fn agent_nav(&self) -> &AgentNavigationState {
        &self.agent_nav
    }

    pub(super) fn agent_nav_mut(&mut self) -> &mut AgentNavigationState {
        &mut self.agent_nav
    }

    pub(super) fn upsert_agent(&mut self, entry: AgentThreadEntry) {
        self.agent_nav.upsert(entry);
    }

    pub(super) fn set_current_agent_thread(&mut self, thread_id: Option<String>) {
        self.current_agent_thread_id = thread_id;
    }

    pub(super) fn current_agent_thread_id(&self) -> Option<&String> {
        self.current_agent_thread_id.as_ref()
    }

    pub(super) fn tasks(&self) -> &[TaskStatus] {
        &self.tasks
    }

    pub(super) fn set_tasks(&mut self, tasks: Vec<TaskStatus>) {
        self.tasks = tasks;
    }
}
```

- [ ] **Step 4: Move agent navigation ownership into runtime state**

In `app.rs`, replace:

```rust
agent_nav: AgentNavigationState,
current_agent_thread_id: Option<String>,
```

with:

```rust
runtime_view: RuntimeViewState,
```

Keep public helper methods by delegating:

```rust
pub(super) fn active_agent_thread_ids(&self) -> Vec<String> {
    self.runtime_view.agent_nav().active_non_primary_thread_ids()
}

pub(super) fn current_agent_thread_id(&self) -> &str {
    self.runtime_view
        .current_agent_thread_id()
        .map(String::as_str)
        .unwrap_or(&self.session_ui.session_id)
}
```

- [ ] **Step 5: Move backend event application to runtime state helpers**

In `apply_agent_event`, `apply_team_event`, `apply_background_agent_complete`, and `apply_primary_tool_progress`, replace direct `self.agent_nav.*` calls with:

```rust
self.runtime_view.agent_nav_mut().upsert(entry);
self.runtime_view.agent_nav_mut().mark_status(...);
self.runtime_view.agent_nav_mut().mark_tool_use(...);
```

This preserves behavior while giving agent navigation one runtime store.

- [ ] **Step 6: Add typed task snapshot access**

When backend task events are applied, store the current task list through:

```rust
self.runtime_view.set_tasks(tasks);
```

Render task surfaces from `self.runtime_view.tasks()` instead of deriving task collections in render code.

- [ ] **Step 7: Run runtime and snapshot tests**

```bash
cargo test -p allthecodes ui::app::runtime_state
cargo test -p allthecodes ui::app::agent_navigation
cargo test -p allthecodes snapshot_task_surfaces
cargo test -p allthecodes snapshot_mcp_surfaces
```

Expected: all pass.

- [ ] **Step 8: Commit Task 4**

```bash
git add -A -- crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app/runtime_state.rs crates/allthecodes/src/ui/app/agent_navigation.rs crates/allthecodes/src/ui/tasks/mod.rs crates/allthecodes/src/ui/mcp/mod.rs
git commit -m "refactor(tui): add runtime view state"
```

---

### Task 5: Final Integration and Guardrails

**Files:**
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/app/input.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/app/tests.rs`
- Test: existing focused TUI test modules.

**Interfaces:**
- Produces: final phase 4 shape where `App` is a facade over stores.
- Exit condition: render/input state is locally testable; permission, task, and agent navigation have one runtime store.

- [ ] **Step 1: Add guardrail tests for `App` facade behavior**

In `app/tests.rs`, add:

```rust
#[test]
fn app_facade_routes_messages_through_conversation_store() {
    let mut app = App::new();
    app.add_message(user_message("hello"));

    assert_eq!(app.messages().len(), 1);
    let Message::User(message) = &app.messages()[0] else {
        panic!("expected user message");
    };
    assert!(matches!(&message.content, MessageContent::Text(text) if text == "hello"));
}

#[test]
fn app_overlay_priority_is_stable_for_permission_then_question() {
    let mut app = App::new();
    app.show_question_dialog(
        "q-1",
        allthecodes_types::callbacks::AskUserRequestPayload {
            question: "Pick one".to_string(),
            choices: vec!["A".to_string()],
            allow_free_text: false,
        },
    );
    app.show_permission_dialog("Bash", r#"{"command":"cargo test"}"#, "Run command?");

    assert_eq!(app.active_overlay_for_tests(), Some(ActiveOverlay::Permission));
}
```

Add a `#[cfg(test)] pub(crate) fn active_overlay_for_tests(&self) -> Option<ActiveOverlay>` accessor in `app.rs`.

- [ ] **Step 2: Run failing guardrail tests**

```bash
cargo test -p allthecodes app_facade_routes_messages_through_conversation_store
cargo test -p allthecodes app_overlay_priority_is_stable_for_permission_then_question
```

Expected: compile fails until test helpers and text extraction imports are wired.

- [ ] **Step 3: Fix test helper imports and facade accessors**

Add `ActiveOverlay` test export:

```rust
#[cfg(test)]
pub(crate) use overlays::ActiveOverlay;

#[cfg(test)]
pub(crate) fn active_overlay_for_tests(&self) -> Option<ActiveOverlay> {
    self.overlays.active_overlay()
}
```

Use the existing test message helper pattern in `app/tests.rs` so no production-only message API is invented.

- [ ] **Step 4: Remove stale direct state derivation**

Search:

```bash
rg "self\\.(messages|selected_message|permission_dialog|question_dialog|agent_nav|current_agent_thread_id|command_surface|history_search_dialog|agent_tree_dialog)" crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app
```

Expected remaining direct references:

```text
crates/allthecodes/src/ui/app/domain.rs
crates/allthecodes/src/ui/app/overlays.rs
crates/allthecodes/src/ui/app/runtime_state.rs
```

Move any remaining direct `App` references to the corresponding store before continuing.

- [ ] **Step 5: Run full focused verification**

```bash
cargo fmt --all -- --check
cargo test -p allthecodes ui::app
cargo test -p allthecodes ui::messages
cargo test -p allthecodes ui::permissions
cargo test -p allthecodes snapshot_task_surfaces
cargo test -p allthecodes snapshot_mcp_surfaces
cargo check --workspace
```

Expected: all pass, no warnings.

- [ ] **Step 6: Run final release build before merge or handoff**

```bash
cargo build --workspace --release
```

Expected: exit 0, `Finished release profile`.

- [ ] **Step 7: Commit Task 5**

```bash
git add -A -- crates/allthecodes/src/ui/app.rs crates/allthecodes/src/ui/app/input.rs crates/allthecodes/src/ui/app/render.rs crates/allthecodes/src/ui/app/tests.rs
git commit -m "test(tui): guard state store boundaries"
```

---

## Self-Review

**Spec coverage:**
- `domain stores`: Task 1 creates and wires app domain stores.
- `overlay dispatcher`: Task 2 centralizes overlay priority and dispatch.
- `message view-model`: Task 3 creates `MessageListViewModel` and routes render through it.
- `tasks/agents/mcp typed view state`: Task 4 creates `RuntimeViewState` and moves agent/task state access behind it.
- Exit condition `TUI render/input 状态可以局部测试`: every task has focused unit tests.
- Exit condition `权限、任务、agent nav 只有一个 runtime store`: overlay state and runtime view state own these paths by Task 4.

**Placeholder scan:** No unspecified tasks remain. Each step has exact file paths, command lines, and expected results.

**Type consistency:** `ConversationStore`, `OverlayState`, `MessageListViewModel`, and `RuntimeViewState` are introduced before later tasks depend on them.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-03-phase-4-tui-state-decoupling.md`. Two execution options:

**1. Subagent-Driven (recommended)** - dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** - execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
