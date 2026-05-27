//! Push the terminal caret into the engine's `TextOverlays`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text::{MonoCellWidth, RectOverlay, TextOverlays};
use bevy_instanced_text_interaction::{
    caret_overlay, cursor_blink_visible, BlinkPhase, CursorSettings, TextCursorColor,
};

use crate::text::TerminalGridSnapshot;

#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
pub struct TerminalCursorCell {
    pub last_row: u32,
    pub last_col: u16,
}

pub fn track_cursor_blink(
    time: Res<Time>,
    mut q: Query<(
        &TerminalGridSnapshot,
        &mut TerminalCursorCell,
        &mut BlinkPhase,
    )>,
) {
    for (snapshot, mut cell, mut blink) in q.iter_mut() {
        if snapshot.cursor_row != cell.last_row || snapshot.cursor_col != cell.last_col {
            blink.last_change_secs = time.elapsed_secs_f64();
            cell.last_row = snapshot.cursor_row;
            cell.last_col = snapshot.cursor_col;
        }
    }
}

pub fn push_terminal_caret(
    time: Res<Time>,
    input_focus: Res<InputFocus>,
    mut q: Query<(
        Entity,
        &TerminalGridSnapshot,
        &BlinkPhase,
        &MonoCellWidth,
        &TextCursorColor,
        &CursorSettings,
        &mut TextOverlays,
    )>,
) {
    for (entity, snapshot, blink, mono, theme, cursor_settings, mut overlays) in q.iter_mut() {
        let focused = input_focus.get() == Some(entity);
        let visible = focused
            && !snapshot.cursor_hidden
            && cursor_blink_visible(
                cursor_settings.blink_rate,
                cursor_settings.blink_pause_secs,
                time.elapsed_secs_f64(),
                blink.last_change_secs,
            );

        let new_rect: Option<RectOverlay> = if visible {
            let x_left = snapshot.cursor_col as f32 * mono.px;
            Some(caret_overlay(
                snapshot.cursor_row,
                x_left,
                cursor_settings,
                **theme,
            ))
        } else {
            None
        };

        let current: Option<&RectOverlay> = overlays.0.first();
        let same = match (&new_rect, current) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if !same {
            overlays.0.clear();
            if let Some(rect) = new_rect {
                overlays.0.push(rect);
            }
        }
    }
}
