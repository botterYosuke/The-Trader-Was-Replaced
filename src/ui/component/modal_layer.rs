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

use crate::trading::SecretPrompt;
use crate::ui::modify_modal::ModifyForm;
use crate::ui::theme::{DynamicSpacing, ElevationIndex, Theme};
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
    /// Escape-dismiss priority (highest wins one Escape), NOT the visual
    /// GlobalZIndex. Used by [`ModalLayer::try_dismiss_highest_z`] to target the
    /// most-prioritized modal by z rather than by push order.
    pub z: i32,
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

    /// Attempt to dismiss the modal with the highest `z` (frontmost by stacking
    /// order, not by push order), consulting its `on_before_dismiss` veto.
    ///
    /// Selects the entry with the maximum `z`; on ties, the last-pushed wins
    /// (matching the `try_dismiss_top` last-element semantics). The veto applies
    /// to that single entry only: on [`DismissDecision::Pending`] nothing is
    /// removed and `false` is returned, just like `try_dismiss_top`.
    pub fn try_dismiss_highest_z(&mut self) -> bool {
        // `max_by_key` returns the LAST element among equal keys, so equal-z
        // ties resolve to the last-pushed entry (same as `try_dismiss_top`).
        let target = self
            .stack
            .iter()
            .enumerate()
            .max_by_key(|(_, m)| m.z)
            .map(|(i, _)| i);
        match target {
            Some(i) => match (self.stack[i].on_before_dismiss)() {
                DismissDecision::Dismiss => {
                    self.stack.remove(i);
                    true
                }
                DismissDecision::Pending => false,
            },
            None => false,
        }
    }
}

/// Generic `ModalLayer.stack` ⇄ owning-prompt reconcile step shared by the
/// relogin and reconcile notice modals (#46 Slice B, mechanism A). Each modal's
/// system computes its own `is_open` / `prompt_changed` and supplies a `clear`
/// closure (called only on the REVERSE/esc-pop arm); the subtle branch order +
/// `was_on_stack` bookkeeping lives ONCE here.
///
/// Branches (order is load-bearing):
/// - FORWARD (open): `prompt_changed && is_open && !on_stack` → push + mark, return.
/// - CLOSE (button/programmatic): `!is_open && on_stack` → retain-remove + unmark, return.
/// - REVERSE (esc pop): `was_on_stack && !on_stack && is_open` → run `clear`.
/// - fall-through: `was_on_stack = on_stack`.
pub fn reconcile_modal_stack(
    layer: &mut ModalLayer,
    root: Entity,
    z: i32,
    was_on_stack: &mut bool,
    is_open: bool,
    prompt_changed: bool,
    on_before_dismiss: fn() -> DismissDecision,
    clear: impl FnOnce(),
) {
    let on_stack = layer.stack.iter().any(|m| m.root == root);

    if prompt_changed && is_open && !on_stack {
        layer.push(ActiveModal {
            root,
            backdrop: root,
            previous_focus: None,
            z,
            on_before_dismiss,
        });
        *was_on_stack = true;
        return;
    }

    if !is_open && on_stack {
        layer.stack.retain(|m| m.root != root);
        *was_on_stack = false;
        return;
    }

    if *was_on_stack && !on_stack && is_open {
        clear();
    }

    *was_on_stack = on_stack;
}

/// Whether Esc is clear to dismiss the highest-z modal-layer entry. Mirrors the
/// relogin notice's yield guard (relogin_modal_button_system): a single
/// Escape must defer to any higher-priority input modal that is open, so the
/// one-shot Escape isn't consumed twice. The order-confirm modal is now a
/// stack entry (z 280) dismissed via `try_dismiss_highest_z`, so it is no
/// longer a yield input here.
fn esc_yield_clear(secret_active: bool, modify_open: bool) -> bool {
    !(secret_active || modify_open)
}

/// Consume Escape and dismiss the highest-z modal entry — but only when no
/// higher-priority input modal (secret / modify) is open.
/// `try_dismiss_highest_z` itself respects each entry's `on_before_dismiss`
/// veto. The order-confirm modal participates as a stack entry (z 280) rather
/// than a yield input.
pub fn modal_layer_esc_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut layer: ResMut<ModalLayer>,
    secret_prompt: Res<SecretPrompt>,
    modify_form: Res<ModifyForm>,
) {
    if layer.stack.is_empty() {
        return;
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if !esc_yield_clear(secret_prompt.active.is_some(), modify_form.open) {
        return;
    }
    layer.try_dismiss_highest_z();
}

/// Declarative spec for a standard modal: a full-screen backdrop with a
/// centered card. `spawn_modal` builds the entities; content children are
/// added by the caller onto the returned `card`/`root` (Slice B2 migration).
pub struct ModalSkeleton {
    /// Card width in px (call sites pass a fixed width, e.g. relogin 360).
    pub width: f32,
    /// GlobalZIndex for the backdrop+card (preserves per-modal stacking:
    /// relogin 260, reconcile 262 … until ElevationIndex fully owns z).
    pub z_index: i32,
    /// Accessible name for the root node.
    pub name: &'static str,
}

/// Entities produced by [`spawn_modal`]. `root` is the backdrop (full-screen,
/// `Display::None` at spawn); `card` is the centered surface to populate.
pub struct ModalHandle {
    pub root: Entity,
    pub card: Entity,
}

pub fn spawn_modal(commands: &mut Commands, theme: &Theme, skeleton: ModalSkeleton) -> ModalHandle {
    let density = theme.spacing.density;
    let pad = DynamicSpacing::Base16.px(density);

    let card = commands
        .spawn((
            Node {
                width: Val::Px(skeleton.width),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(pad)),
                ..default()
            },
            BackgroundColor(ElevationIndex::ModalSurface.background(theme)),
            ElevationIndex::ModalSurface,
        ))
        .id();

    let root = commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.colors.background.with_alpha(0.6)),
            GlobalZIndex(skeleton.z_index),
            Name::new(skeleton.name),
        ))
        .add_child(card)
        .id();

    ModalHandle { root, card }
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
            z: 100,
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
            z: 110,
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
            z: 260,
            on_before_dismiss: dismiss,
        });
        // ... then reconcile modal (frontmost).
        layer.push(ActiveModal {
            root: e(30),
            backdrop: e(31),
            previous_focus: None,
            z: 262,
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
            z: 200,
            on_before_dismiss: pending,
        });
        assert!(!layer.try_dismiss_top());
        assert_eq!(layer.stack.len(), 1);
    }

    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn m_modal_05_spawn_modal_builds_backdrop_and_card() {
        let mut world = World::new();
        world.insert_resource(Theme::default());
        let handle = world
            .run_system_once(|mut commands: Commands, theme: Res<Theme>| {
                spawn_modal(
                    &mut commands,
                    &theme,
                    ModalSkeleton { width: 360.0, z_index: 260, name: "Test" },
                )
            })
            .unwrap();

        let root_node = world.entity(handle.root).get::<Node>().unwrap();
        assert_eq!(root_node.display, Display::None);

        let z = world.entity(handle.root).get::<GlobalZIndex>().unwrap();
        assert_eq!(z.0, 260);

        let elevation = world.entity(handle.card).get::<ElevationIndex>().unwrap();
        assert_eq!(*elevation, ElevationIndex::ModalSurface);

        let card_bg = world.entity(handle.card).get::<BackgroundColor>().unwrap();
        assert_eq!(card_bg.0, ElevationIndex::ModalSurface.background(&Theme::default()));
    }

    #[test]
    fn m_modal_06_esc_yield_clear_truth_table() {
        assert!(esc_yield_clear(false, false));
        assert!(!esc_yield_clear(true, false));
        assert!(!esc_yield_clear(false, true));
        assert!(!esc_yield_clear(true, true));
    }

    fn esc_app() -> App {
        let mut app = App::new();
        app.init_resource::<ModalLayer>();
        app.init_resource::<crate::trading::SecretPrompt>();
        app.init_resource::<crate::ui::order_panel::OrderConfirm>();
        app.init_resource::<crate::ui::modify_modal::ModifyForm>();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.add_systems(Update, modal_layer_esc_system);
        app
    }

    #[test]
    fn m_modal_07_esc_dismisses_top_when_clear() {
        let mut app = esc_app();
        app.world_mut().resource_mut::<ModalLayer>().push(ActiveModal {
            root: e(1),
            backdrop: e(2),
            previous_focus: None,
            z: 210,
            on_before_dismiss: dismiss,
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<ModalLayer>().stack.is_empty());
    }

    #[test]
    fn m_modal_08_esc_yields_to_open_secret_prompt() {
        use crate::trading::SecretPromptRequest;
        let mut app = esc_app();
        app.world_mut().resource_mut::<ModalLayer>().push(ActiveModal {
            root: e(1),
            backdrop: e(2),
            previous_focus: None,
            z: 300,
            on_before_dismiss: dismiss,
        });
        app.world_mut().resource_mut::<crate::trading::SecretPrompt>().active =
            Some(SecretPromptRequest {
                request_id: "r1".to_string(),
                venue: "TACHIBANA".to_string(),
                kind: "second_password".to_string(),
                purpose: "new_order".to_string(),
            });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(
            app.world().resource::<ModalLayer>().stack.len(),
            1,
            "Esc must yield to the open secret modal; the stack entry survives"
        );
    }

    #[test]
    fn m_modal_10_reconcile_stack_forward_then_reverse_clears() {
        let mut layer = ModalLayer::default();
        let root = e(50);
        let mut was = false;
        let mut cleared = false;

        reconcile_modal_stack(&mut layer, root, 262, &mut was, true, true, dismiss, || {});
        assert!(was);
        assert_eq!(layer.stack.len(), 1);

        layer.try_dismiss_top();
        assert!(layer.stack.is_empty());

        reconcile_modal_stack(&mut layer, root, 262, &mut was, true, false, dismiss, || {
            cleared = true;
        });
        assert!(cleared, "esc pop must trigger the clear closure");
        assert!(!was);
    }

    #[test]
    fn m_modal_09_esc_noop_when_stack_empty() {
        let mut app = esc_app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<ModalLayer>().stack.is_empty());
    }

    #[test]
    fn m_modal_11_dismiss_targets_highest_z() {
        // High-z entry is pushed FIRST, low-z SECOND, so push order (LIFO) and
        // z order disagree: a correct max-z dismissal must remove the high-z A,
        // not the last-pushed B.
        let mut layer = ModalLayer::default();
        // A: frontmost by z (300), pushed first.
        layer.push(ActiveModal {
            root: e(1),
            backdrop: e(1),
            previous_focus: None,
            z: 300,
            on_before_dismiss: dismiss,
        });
        // B: lower z (200), pushed last.
        layer.push(ActiveModal {
            root: e(2),
            backdrop: e(2),
            previous_focus: None,
            z: 200,
            on_before_dismiss: dismiss,
        });

        assert!(layer.try_dismiss_highest_z());
        assert_eq!(layer.stack.len(), 1);
        // The high-z A (e(1)) must be gone; the low-z B (e(2)) must remain.
        assert_eq!(
            layer.stack[0].root,
            e(2),
            "dismissal must target the highest-z modal, not the last-pushed one"
        );
    }
}
