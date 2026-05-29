//! Modal layer — modal stack with frontmost-first dismissal (Issue #46 Slice B).
//!
//! A single [`ModalLayer`] resource owns a LIFO [`stack`](ModalLayer::stack) of
//! [`ActiveModal`] entries. Dismissal always targets the frontmost (last-pushed)
//! modal, and an entry may veto its own dismissal via the `on_before_dismiss`
//! hook returning [`DismissDecision::Pending`].
//!
//! Slice B1 ships the [`ModalLayer`] stack foundation: `push` / `pop` /
//! `try_dismiss_top` with the `on_before_dismiss` veto gate, guarded by
//! `#[cfg(test)]` unit tests. [`ActiveModal::previous_focus`] is record-only in
//! this pass (no global focus resource exists yet). The `ModalSkeleton` spawn
//! helper and the Esc-driven dismissal system arrive in Slice B2.

use bevy::prelude::*;

/// Result of an [`ActiveModal`]'s `on_before_dismiss` hook.
pub enum DismissDecision {
    /// The modal agrees to be removed from the stack.
    Dismiss,
    /// The modal vetoes dismissal (e.g. work in flight); it stays on the stack.
    Pending,
}

/// A single modal currently on the [`ModalLayer`] stack.
pub struct ActiveModal {
    /// Root UI entity of the modal content.
    pub root: Entity,
    /// Backdrop entity behind the modal.
    pub backdrop: Entity,
    /// Entity that held focus before this modal opened (record-only this pass).
    pub previous_focus: Option<Entity>,
    /// Veto hook consulted by [`ModalLayer::try_dismiss_top`].
    pub on_before_dismiss: fn() -> DismissDecision,
}

/// LIFO stack of open modals.
#[derive(Resource, Default)]
pub struct ModalLayer {
    /// Open modals, last element is the frontmost.
    pub stack: Vec<ActiveModal>,
}

impl ModalLayer {
    /// Push a modal onto the stack as the new frontmost entry.
    pub fn push(&mut self, modal: ActiveModal) {
        self.stack.push(modal);
    }

    /// Pop the frontmost modal, returning it if present.
    pub fn pop(&mut self) -> Option<ActiveModal> {
        self.stack.pop()
    }

    /// Attempt to dismiss the frontmost modal: consult its `on_before_dismiss`
    /// hook, pop only on [`DismissDecision::Dismiss`], and return whether a
    /// dismissal occurred.
    pub fn try_dismiss_top(&mut self) -> bool {
        match self.stack.last() {
            Some(top) => match (top.on_before_dismiss)() {
                DismissDecision::Dismiss => {
                    self.stack.pop();
                    true
                }
                DismissDecision::Pending => false,
            },
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dismiss() -> DismissDecision {
        DismissDecision::Dismiss
    }

    fn pending() -> DismissDecision {
        DismissDecision::Pending
    }

    fn e(i: u32) -> Entity {
        Entity::from_raw_u32(i).unwrap()
    }

    #[test]
    fn m_modal_01_push_records_entry() {
        let mut layer = ModalLayer::default();
        layer.push(ActiveModal {
            root: e(1),
            backdrop: e(2),
            previous_focus: None,
            on_before_dismiss: dismiss,
        });
        assert_eq!(layer.stack.len(), 1);
        assert_eq!(layer.stack[0].root, e(1));
        assert_eq!(layer.stack[0].backdrop, e(2));
    }

    #[test]
    fn m_modal_02_pop_clears_recorded_focus() {
        let mut layer = ModalLayer::default();
        layer.push(ActiveModal {
            root: e(10),
            backdrop: e(11),
            previous_focus: Some(e(99)),
            on_before_dismiss: dismiss,
        });
        let popped = layer.pop().expect("pop should return the pushed entry");
        assert_eq!(popped.previous_focus, Some(e(99)));
        assert!(layer.stack.is_empty());
    }

    #[test]
    fn m_modal_03_frontmost_dismiss_first() {
        let mut layer = ModalLayer::default();
        // relogin modal first ...
        layer.push(ActiveModal {
            root: e(20),
            backdrop: e(21),
            previous_focus: None,
            on_before_dismiss: dismiss,
        });
        // ... then reconcile modal (frontmost).
        layer.push(ActiveModal {
            root: e(30),
            backdrop: e(31),
            previous_focus: None,
            on_before_dismiss: dismiss,
        });
        assert!(layer.try_dismiss_top());
        assert_eq!(layer.stack.len(), 1);
        assert_eq!(layer.stack[0].root, e(20));
    }

    #[test]
    fn m_modal_04_pending_not_popped() {
        let mut layer = ModalLayer::default();
        layer.push(ActiveModal {
            root: e(40),
            backdrop: e(41),
            previous_focus: None,
            on_before_dismiss: pending,
        });
        assert!(!layer.try_dismiss_top());
        assert_eq!(layer.stack.len(), 1);
    }
}
