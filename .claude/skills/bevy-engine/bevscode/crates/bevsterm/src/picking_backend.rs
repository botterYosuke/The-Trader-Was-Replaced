//! `On<Pointer<Press>>` observer on the terminal root: hit-test the click
//! against `TerminalBlockState` and emit `TerminalBlockSelected`.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::DisplayLayout;

use crate::messages::TerminalBlockSelected;
use crate::text::TerminalBlockState;

pub fn on_terminal_block_press(
    trigger: On<Pointer<Press>>,
    q: Query<(
        &TerminalBlockState,
        &DisplayLayout,
        &ScrollPosition,
        &ComputedNode,
    )>,
    mut selected_w: MessageWriter<TerminalBlockSelected>,
) {
    if trigger.event().button != PointerButton::Primary {
        return;
    }
    let entity = trigger.event().entity;
    let Ok((state, layout, scroll, computed)) = q.get(entity) else {
        return;
    };
    // Hit position is normalized (-0.5..0.5) from node center.
    let Some(norm) = trigger.event().hit.position.map(|p| Vec2::new(p.x, p.y)) else {
        return;
    };
    let inv_scale = computed.inverse_scale_factor();
    let logical_size = computed.size() * inv_scale;
    let text_area_top = computed.content_inset().min_inset.y * inv_scale;
    let local_y = (norm.y + 0.5) * logical_size.y;
    let click_y = local_y - text_area_top + scroll.y;
    let default_lh = layout.line_height;
    let Some(line) = layout.lines.iter().find(|l| {
        let lh = l.line_height.unwrap_or(default_lh);
        click_y >= l.y_top && click_y < l.y_top + lh
    }) else {
        return;
    };

    let buffer_row = line.buffer_row as i64;
    let Some(block) = state
        .blocks
        .iter()
        .find(|b| buffer_row >= b.prompt_row && buffer_row <= b.end_row)
    else {
        return;
    };

    selected_w.write(TerminalBlockSelected {
        entity,
        block_id: block.id,
    });
}
