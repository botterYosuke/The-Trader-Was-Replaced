//! [`PopupAnchor`]: single positioning entry point for every LSP popup.
//!
//! Resolves *(line, character)* into `Val::Px` offsets relative to the
//! editor `Node`, flipping above/below the cursor row and clamping to
//! the editor's viewport so popups never paint into off-screen space.
//!
//! Because popups are parented under the editor entity (see
//! [`LspPopupRoot`](super::LspPopupRoot)), the editor-local
//! `AnchorPoint::top_left` from [`BufferAnchorParam`] is what an
//! absolutely-positioned child `Node` wants — no screen-offset addition,
//! unlike the `bevy_egui` reference renderer.

use bevy::prelude::*;
use bevy_instanced_text::BufferAnchorParam;
use bevy_instanced_text_editor::RopeBuffer;

use crate::settings::EditorTheme;
use crate::types::CodeEditor;

/// Where a popup should prefer to sit relative to the cursor row.
///
/// Both variants fall back to the opposite side when there isn't room on
/// the preferred one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupPlacement {
    /// Below the cursor row, flipping above when clipped (completion,
    /// code actions).
    PreferBelow,
    /// Above the cursor row, flipping below when clipped (signature help
    /// — sits where the function header would).
    PreferAbove,
}

/// Resolved popup position, in `Val::Px` offsets relative to the editor
/// `Node`. Set `node.position_type = PositionType::Absolute` and copy
/// these onto `node.left` / `node.top`.
#[derive(Clone, Copy, Debug)]
pub struct PopupRect {
    pub left: Val,
    pub top: Val,
}

/// `SystemParam` that turns *(editor entity, line, character, popup size)*
/// into a [`PopupRect`]. Also exposes the owning editor's [`EditorTheme`]
/// so popup renderers can avoid declaring a second `Query` for it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PopupAnchor<'w, 's> {
    anchors: BufferAnchorParam<'w, 's, RopeBuffer>,
    editors: Query<'w, 's, (&'static ComputedNode, &'static EditorTheme), With<CodeEditor>>,
}

impl PopupAnchor<'_, '_> {
    /// Look up the theme for `editor`. `None` when the entity isn't
    /// a `CodeEditor` (e.g. a stale popup whose editor was despawned).
    pub fn theme(&self, editor: Entity) -> Option<&EditorTheme> {
        self.editors.get(editor).ok().map(|(_, t)| t)
    }
}

impl PopupAnchor<'_, '_> {
    /// Place a popup of `popup_size` next to the cell at *(line, character)*
    /// on `editor`, preferring `placement` and flipping when clipped.
    ///
    /// Returns `None` when the editor entity isn't known to the buffer-
    /// anchor query (no layout yet, or wrong entity) — caller should
    /// keep the popup hidden until layout catches up.
    pub fn place(
        &self,
        editor: Entity,
        line: u32,
        character: u32,
        popup_size: Vec2,
        placement: PopupPlacement,
    ) -> Option<PopupRect> {
        let anchor = self.anchors.at_buffer_pos(editor, line, character)?;
        let (computed, _) = self.editors.get(editor).ok()?;

        let inv = computed.inverse_scale_factor();
        let logical = computed.size() * inv;

        // Hide the popup when its anchor row is outside the vertical
        // viewport. Without this, scrolling the anchor line off-screen
        // would clamp the popup's `top` to 0 and pin it to the editor's
        // top edge — visually a stray panel hovering at the upper-right.
        let row_top = anchor.top_left.y;
        let row_bot = row_top + anchor.line_height;
        if row_bot <= 0.0 || row_top >= logical.y {
            return None;
        }

        // Overlap the popup with the cursor row by half a line so moving
        // the mouse from text into the popup doesn't cross a dead band
        // that fires `Pointer<Out>` on the editor. Same trick the egui
        // reference renderer uses.
        let cursor_y = anchor.top_left.y;
        let lh = anchor.line_height;
        let below_y = cursor_y + lh * 0.5;
        let above_y = cursor_y - popup_size.y + lh * 0.5;

        let y = match placement {
            PopupPlacement::PreferBelow => {
                if below_y + popup_size.y <= logical.y {
                    below_y
                } else if above_y >= 0.0 {
                    above_y
                } else if (logical.y - below_y) > cursor_y {
                    below_y
                } else {
                    above_y.max(0.0)
                }
            }
            PopupPlacement::PreferAbove => {
                if above_y >= 0.0 {
                    above_y
                } else {
                    below_y
                }
            }
        };

        let x = anchor
            .top_left
            .x
            .max(0.0)
            .min((logical.x - popup_size.x).max(0.0));

        Some(PopupRect {
            left: Val::Px(x),
            top: Val::Px(y),
        })
    }
}
