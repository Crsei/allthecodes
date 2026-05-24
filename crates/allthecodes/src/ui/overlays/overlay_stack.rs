//! Z-indexed overlay stack for modal dialog management.
//!
//! Provides a push/pop/peek stack of active overlays.  The topmost overlay
//! receives keyboard events first; closing an overlay pops it and reveals the
//! one below.  Mirrors the modal-stack behaviour in the upstream TypeScript
//! codebase.

/// A unique identifier assigned to each overlay pushed onto the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(u64);

impl OverlayId {
    pub const NONE: OverlayId = OverlayId(0);
}

/// An active overlay on the stack.
///
/// Concrete overlay types (dialog, pager, fuzzy-picker, etc.) wrap themselves
/// in this enum to participate in stack management.
#[derive(Debug, Clone)]
pub enum OverlayKind {
    /// A generic dialog box.
    Dialog,
    /// A pager / scrollable text view.
    Pager,
    /// A fuzzy-finder / search overlay.
    FuzzyPicker,
    /// A permission request dialog.
    Permission,
}

/// A single entry on the overlay stack.
#[derive(Debug, Clone)]
pub struct OverlayEntry {
    pub id: OverlayId,
    pub kind: OverlayKind,
    /// Human-readable label for the overlay (used in stack traces / debug).
    pub label: String,
}

impl OverlayEntry {
    pub fn new(kind: OverlayKind, label: impl Into<String>) -> Self {
        Self {
            id: OverlayId(0), // assigned by push()
            kind,
            label: label.into(),
        }
    }
}

/// Z-indexed overlay stack.
///
/// The stack grows upward: index 0 is the bottom (earliest) overlay, and the
/// last element is the topmost (active) overlay.
#[derive(Debug, Default)]
pub struct OverlayStack {
    entries: Vec<OverlayEntry>,
    next_id: u64,
}

impl OverlayStack {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// Push a new overlay onto the top of the stack.
    ///
    /// Returns the assigned [`OverlayId`] which can be used later for
    /// targeted removal.
    pub fn push(&mut self, entry: OverlayEntry) -> OverlayId {
        let id = OverlayId(self.next_id);
        self.next_id += 1;
        self.entries.push(OverlayEntry { id, ..entry });
        id
    }

    /// Pop the topmost overlay from the stack.
    ///
    /// Returns `None` if the stack is empty.
    pub fn pop(&mut self) -> Option<OverlayEntry> {
        self.entries.pop()
    }

    /// Peek at the topmost overlay without removing it.
    pub fn peek(&self) -> Option<&OverlayEntry> {
        self.entries.last()
    }

    /// Peek mutably at the topmost overlay.
    pub fn peek_mut(&mut self) -> Option<&mut OverlayEntry> {
        self.entries.last_mut()
    }

    /// Remove a specific overlay by its ID.
    ///
    /// This is more targeted than `pop()` when the overlay may not be on top
    /// (e.g., closing a dialog from within).
    pub fn remove(&mut self, id: OverlayId) -> Option<OverlayEntry> {
        let pos = self.entries.iter().position(|e| e.id == id);
        pos.map(|i| self.entries.remove(i))
    }

    /// The number of active overlays.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the stack has any overlays active.
    pub fn is_active(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Iterate from bottom to top.
    pub fn iter(&self) -> impl Iterator<Item = &OverlayEntry> {
        self.entries.iter()
    }

    /// Iterate from top to bottom (for hit-testing).
    pub fn iter_top_down(&self) -> impl Iterator<Item = &OverlayEntry> {
        self.entries.iter().rev()
    }

    /// Clear all overlays.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stack() {
        let mut stack = OverlayStack::new();
        assert!(stack.is_empty());
        assert!(!stack.is_active());
        assert!(stack.peek().is_none());
        assert!(stack.pop().is_none());
    }

    #[test]
    fn push_and_peek() {
        let mut stack = OverlayStack::new();
        let id = stack.push(OverlayEntry::new(OverlayKind::Dialog, "test"));
        assert!(stack.is_active());
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.peek().unwrap().label, "test");
        assert!(matches!(stack.peek().unwrap().kind, OverlayKind::Dialog));
        assert_ne!(id, OverlayId::NONE);
    }

    #[test]
    fn push_and_pop() {
        let mut stack = OverlayStack::new();
        stack.push(OverlayEntry::new(OverlayKind::Dialog, "first"));
        stack.push(OverlayEntry::new(OverlayKind::Pager, "second"));
        assert_eq!(stack.len(), 2);

        let popped = stack.pop().unwrap();
        assert_eq!(popped.label, "second");
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.peek().unwrap().label, "first");
    }

    #[test]
    fn remove_by_id() {
        let mut stack = OverlayStack::new();
        stack.push(OverlayEntry::new(OverlayKind::Dialog, "a"));
        let id2 = stack.push(OverlayEntry::new(OverlayKind::Pager, "b"));
        stack.push(OverlayEntry::new(OverlayKind::FuzzyPicker, "c"));

        // Remove middle entry.
        let removed = stack.remove(id2).unwrap();
        assert_eq!(removed.label, "b");
        assert_eq!(stack.len(), 2);
        // Order preserved: a, c
        assert_eq!(stack.entries[0].label, "a");
        assert_eq!(stack.entries[1].label, "c");
    }

    #[test]
    fn clear_empties_stack() {
        let mut stack = OverlayStack::new();
        stack.push(OverlayEntry::new(OverlayKind::Dialog, "x"));
        stack.push(OverlayEntry::new(OverlayKind::Dialog, "y"));
        stack.clear();
        assert!(stack.is_empty());
    }

    #[test]
    fn iter_top_down() {
        let mut stack = OverlayStack::new();
        stack.push(OverlayEntry::new(OverlayKind::Dialog, "bottom"));
        stack.push(OverlayEntry::new(OverlayKind::Pager, "top"));
        let labels: Vec<&str> = stack.iter_top_down().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["top", "bottom"]);
    }

    #[test]
    fn iter_and_peek_mut_can_update_permission_overlay() {
        let mut stack = OverlayStack::new();
        stack.push(OverlayEntry::new(OverlayKind::Permission, "approval"));

        stack.peek_mut().expect("top").label = "approval updated".to_string();

        let labels: Vec<&str> = stack.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(labels, vec!["approval updated"]);
        assert!(matches!(
            stack.peek().expect("top").kind,
            OverlayKind::Permission
        ));
    }
}
