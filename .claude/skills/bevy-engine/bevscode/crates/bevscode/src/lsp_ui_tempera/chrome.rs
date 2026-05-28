//! Shared popup chrome — tempera-token-styled background, border, radius,
//! padding — plus position resolution via [`PopupAnchor`].
//!
//! Every popup renderer in this module calls [`apply_chrome`] to set its
//! own `Node`'s position and chrome, then fills in its own children. The
//! palette and metrics come from tempera's `ColorPalette`, `Spacing`,
//! and `MenuTokens` resources via [`PopupChrome`], so changing the
//! palette in the app re-tints every popup the same frame.

use bevy::picking::events::{Out, Over};
use bevy::prelude::*;

use crate::lsp_ui::state::{
    CodeActionsLifecycle, CodeActionsPopupBackref, CompletionLifecycle, CompletionPopupBackref,
    HoverLifecycle, HoverPopupBackref, PopupObserversAttached, RenameLifecycle, RenamePopupBackref,
    SignatureLifecycle, SignaturePopupBackref,
};

use crate::ui_kit::PopupChrome;

use super::anchor::{PopupAnchor, PopupPlacement, PopupRect};

/// Position + tempera-styled chrome for a popup `Node`.
///
/// - Sets `position_type = Absolute`, the requested size, column layout,
///   clipping overflow, a 1px border, and tempera's corner radius +
///   interior padding.
/// - Applies `BackgroundColor` from `palette.popover` and `BorderColor`
///   from `palette.border`.
/// - Hides the popup (`display = None`) when the anchor isn't resolvable
///   yet (e.g. layout not produced this frame). The renderer should then
///   skip rebuilding children.
///
/// Returns the resolved [`PopupRect`] when placement succeeded, or
/// `None` when the popup is hidden this frame.
pub struct PopupTarget {
    pub editor: Entity,
    pub line: u32,
    pub character: u32,
    pub size: Vec2,
    pub placement: PopupPlacement,
}

pub fn apply_chrome(
    commands: &mut Commands,
    entity: Entity,
    node: &mut Node,
    anchor: &PopupAnchor,
    chrome: &PopupChrome,
    target: &PopupTarget,
) -> Option<PopupRect> {
    node.position_type = PositionType::Absolute;
    node.width = Val::Px(target.size.x);
    node.height = Val::Px(target.size.y);
    node.flex_direction = FlexDirection::Column;
    node.overflow = Overflow::clip();
    node.padding = UiRect::all(Val::Px(chrome.spacing.xs));
    node.border = UiRect::all(Val::Px(chrome.menu.border_width));
    node.border_radius = BorderRadius::all(Val::Px(chrome.spacing.corner_radius_small));

    let rect = anchor.place(target.editor, target.line, target.character, target.size, target.placement);
    match rect {
        Some(r) => {
            node.left = r.left;
            node.top = r.top;
            node.display = Display::Flex;
        }
        None => {
            node.display = Display::None;
        }
    }

    commands.entity(entity).insert((
        BackgroundColor(chrome.palette.popover),
        BorderColor::all(chrome.palette.border),
    ));

    rect
}

/// Despawn every child of `entity`. Convenience for the "tear down old
/// children, rebuild list" loop every popup uses.
pub fn clear_children(commands: &mut Commands, children: Option<&Children>) {
    let Some(children) = children else { return };
    for child in children.iter() {
        commands.entity(child).despawn();
    }
}

/// Attach the per-kind `Pointer<Over>` / `Pointer<Out>` observers to
/// the popup chrome entity. Each kind has its own attacher because the
/// backref Component and Lifecycle Component types differ — Bevy
/// observers must name concrete `Query` types, so a generic helper
/// would need erased type witnesses we don't want to maintain.
///
/// Idempotent via [`PopupObserversAttached`] — the first call inserts
/// the marker and the renderer skips re-attaching on later frames.
///
/// `Over` cancels any pending dismiss-grace timer in addition to
/// flipping `pointer_in_popup`, which is what keeps the popup visible
/// when the cursor crosses from the editor onto the chrome.
macro_rules! popup_pointer_attacher {
    ($fn_name:ident, $backref:ty, $lifecycle:ty) => {
        pub fn $fn_name(commands: &mut Commands, popup: Entity, editor: Entity) {
            commands
                .entity(popup)
                .insert(PopupObserversAttached)
                .insert(<$backref>::from_editor(editor))
                .observe(
                    |trigger: On<Pointer<Over>>,
                     backrefs: Query<&$backref>,
                     mut lifecycles: Query<&mut $lifecycle>| {
                        let Ok(backref) = backrefs.get(trigger.entity) else {
                            return;
                        };
                        if let Ok(mut lc) = lifecycles.get_mut(backref.editor) {
                            lc.pointer_in_popup = true;
                            lc.dismiss_after = None;
                        }
                    },
                )
                .observe(
                    |trigger: On<Pointer<Out>>,
                     backrefs: Query<&$backref>,
                     mut lifecycles: Query<&mut $lifecycle>| {
                        let Ok(backref) = backrefs.get(trigger.entity) else {
                            return;
                        };
                        if let Ok(mut lc) = lifecycles.get_mut(backref.editor) {
                            lc.pointer_in_popup = false;
                        }
                    },
                );
        }
    };
}

popup_pointer_attacher!(attach_hover_observers, HoverPopupBackref, HoverLifecycle);
popup_pointer_attacher!(
    attach_completion_observers,
    CompletionPopupBackref,
    CompletionLifecycle
);
popup_pointer_attacher!(
    attach_signature_observers,
    SignaturePopupBackref,
    SignatureLifecycle
);
popup_pointer_attacher!(
    attach_code_actions_observers,
    CodeActionsPopupBackref,
    CodeActionsLifecycle
);
popup_pointer_attacher!(attach_rename_observers, RenamePopupBackref, RenameLifecycle);
