//! Glyph-margin click handling. Pointer presses inside the glyph
//! margin emit [`GlyphMarginClicked`] with the resolved buffer line
//! so hosts can wire breakpoints / step markers / etc.
//!
//! Also holds [`GlyphMarginRects`], a leftover Component that the
//! merge-overlay pipeline still references but that no longer carries
//! any rects (icons render as child Nodes now).

use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy_instanced_text::{DisplayLayout, RectOverlay};

use crate::settings::GutterConfig;
use crate::types::CodeEditor;

/// Stub Component preserved for the merge-overlay pipeline; no
/// longer populated (icons render as child Nodes now).
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct GlyphMarginRects(pub Vec<RectOverlay>);

pub(crate) fn update_glyph_margin_overlays(mut q: Query<&mut GlyphMarginRects, With<CodeEditor>>) {
    for mut rects in q.iter_mut() {
        if !rects.0.is_empty() {
            rects.0.clear();
        }
    }
}

/// Emitted when the user clicks inside the glyph-margin column.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct GlyphMarginClicked {
    pub editor: Entity,
    pub line: usize,
    pub button: PointerButton,
}

pub fn on_glyph_margin_press(
    trigger: On<Pointer<Press>>,
    editors: Query<(&ComputedNode, &GutterConfig, &DisplayLayout), With<CodeEditor>>,
    mut writer: MessageWriter<GlyphMarginClicked>,
) {
    let entity = trigger.event().entity;
    let Ok((computed, gutter, layout)) = editors.get(entity) else {
        return;
    };
    if gutter.glyph.is_empty() {
        return;
    }
    let Some(local_pos) = crate::input::mouse::hit_to_local_px(&trigger.event().hit, computed)
    else {
        return;
    };
    if local_pos.x < gutter.glyph.left || local_pos.x >= gutter.glyph.right() {
        return;
    }
    let Some(buffer_line) = super::common::buffer_line_at_y(layout, local_pos.y) else {
        return;
    };
    writer.write(GlyphMarginClicked {
        editor: entity,
        line: buffer_line,
        button: trigger.event().button,
    });
}
