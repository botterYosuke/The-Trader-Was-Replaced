//! Inbound message handlers: clipboard ops + direct host commands.

use std::collections::HashMap;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;

use bevy::ui::ScrollPosition;
use bevy_instanced_text::{MonoCellWidth, TextBuffer, TextSpan};
use bevy_instanced_text_interaction::{ClipboardResource, SelectionState};

use crate::messages::{
    TerminalClear, TerminalCopySelection, TerminalFocus, TerminalKeyInput, TerminalPaste,
    TerminalResize, TerminalRunCommand, TerminalScrollFollowChanged, TerminalScrollTo,
    TerminalScrollToBottom, TerminalScrollToTop, TerminalWriteBytes,
};
use crate::text::{TerminalGridSnapshot, TerminalScrollFollow, TerminalSession};

pub fn handle_copy_selection(
    mut events: MessageReader<TerminalCopySelection>,
    clipboard: Res<ClipboardResource>,
    q: Query<(&SelectionState, &TextBuffer<TextSpan>)>,
) {
    for ev in events.read() {
        let Ok((sel, buffer)) = q.get(ev.entity) else {
            continue;
        };
        let primary = sel.selections.primary();
        if !primary.has_selection() {
            continue;
        }
        let (start, end) = (primary.start(), primary.end());
        let text = &buffer.0 .0;
        let chars: Vec<char> = text.chars().collect();
        let s = start.min(chars.len());
        let e = end.min(chars.len());
        if s < e {
            let selected: String = chars[s..e].iter().collect();
            clipboard.set_text(&selected);
        }
    }
}

pub fn handle_paste(mut events: MessageReader<TerminalPaste>, q: Query<&TerminalSession>) {
    for ev in events.read() {
        let Ok(session) = q.get(ev.entity) else {
            continue;
        };
        let _ = session.terminal.lock().send_paste(&ev.text);
    }
}

pub fn handle_write_bytes(
    mut events: MessageReader<TerminalWriteBytes>,
    q: Query<&TerminalSession>,
) {
    for ev in events.read() {
        let Ok(session) = q.get(ev.entity) else {
            continue;
        };
        let _ = session.pty_input.write_bytes(&ev.bytes);
    }
}

pub fn handle_run_command(
    mut events: MessageReader<TerminalRunCommand>,
    q: Query<&TerminalSession>,
) {
    for ev in events.read() {
        let Ok(session) = q.get(ev.entity) else {
            continue;
        };
        let mut bytes = Vec::with_capacity(ev.command.len() + 1);
        bytes.extend_from_slice(ev.command.as_bytes());
        bytes.push(b'\r');
        let _ = session.pty_input.write_bytes(&bytes);
    }
}

pub fn handle_resize(
    mut events: MessageReader<TerminalResize>,
    mut q: Query<&mut TerminalSession>,
) {
    for ev in events.read() {
        let Ok(mut session) = q.get_mut(ev.entity) else {
            continue;
        };
        if ev.cols == 0 || ev.rows == 0 {
            continue;
        }
        let cell_w = (session.size.pixel_width / session.size.cols.max(1)) as u16;
        let cell_h = (session.size.pixel_height / session.size.rows.max(1)) as u16;
        let new_size = crate::backend::TerminalSize {
            cols: ev.cols as usize,
            rows: ev.rows as usize,
            pixel_width: (ev.cols * cell_w) as usize,
            pixel_height: (ev.rows * cell_h) as usize,
            dpi: session.size.dpi,
        };
        session.terminal.lock().resize(new_size);
        session.size = new_size;
    }
}

pub(crate) fn handle_scroll_to(
    mut events: MessageReader<TerminalScrollTo>,
    mut q: Query<(
        &mut ScrollPosition,
        &mut TerminalScrollFollow,
        &mut crate::text::ScrollFollowState,
        &TextFont,
        &bevy::text::LineHeight,
        &MonoCellWidth,
    )>,
) {
    for ev in events.read() {
        let Ok((mut scroll, mut follow, mut follow_state, font, lh, _mono)) = q.get_mut(ev.entity)
        else {
            continue;
        };
        let line_height = bevy_instanced_text::resolve_line_height(*lh, font.font_size);
        let target = ev.line.max(0) as f32 * line_height;
        scroll.y = target;
        follow.stick_to_bottom = false;
        follow_state.last_applied_target = target;
    }
}

pub fn handle_scroll_to_bottom(
    mut events: MessageReader<TerminalScrollToBottom>,
    mut q: Query<&mut TerminalScrollFollow>,
) {
    for ev in events.read() {
        let Ok(mut follow) = q.get_mut(ev.entity) else {
            continue;
        };
        follow.stick_to_bottom = true;
    }
}

pub(crate) fn handle_scroll_to_top(
    mut events: MessageReader<TerminalScrollToTop>,
    mut q: Query<(
        &mut ScrollPosition,
        &mut TerminalScrollFollow,
        &mut crate::text::ScrollFollowState,
    )>,
) {
    for ev in events.read() {
        let Ok((mut scroll, mut follow, mut follow_state)) = q.get_mut(ev.entity) else {
            continue;
        };
        scroll.y = 0.0;
        follow.stick_to_bottom = false;
        follow_state.last_applied_target = 0.0;
    }
}

pub fn handle_clear(
    mut events: MessageReader<TerminalClear>,
    mut q: Query<(&mut TerminalSession, &mut TerminalGridSnapshot)>,
) {
    const CLEAR_SEQUENCE: &[u8] = b"\x1b[3J\x1b[2J\x1b[H";
    for ev in events.read() {
        let Ok((session, mut snapshot)) = q.get_mut(ev.entity) else {
            continue;
        };
        session.terminal.lock().advance_bytes(CLEAR_SEQUENCE);
        snapshot.version = snapshot.version.wrapping_add(1);
    }
}

pub fn handle_key_input(mut events: MessageReader<TerminalKeyInput>, q: Query<&TerminalSession>) {
    for ev in events.read() {
        let Ok(session) = q.get(ev.entity) else {
            continue;
        };
        let _ = session.terminal.lock().key_down(ev.key, ev.mods);
    }
}

pub fn handle_focus(
    mut events: MessageReader<TerminalFocus>,
    sessions: Query<(), With<TerminalSession>>,
    mut input_focus: ResMut<InputFocus>,
) {
    for ev in events.read() {
        if sessions.get(ev.entity).is_ok() {
            input_focus.set(ev.entity);
        }
    }
}

pub fn emit_scroll_follow_changed(
    q: Query<(Entity, &TerminalScrollFollow), Changed<TerminalScrollFollow>>,
    mut writer: MessageWriter<TerminalScrollFollowChanged>,
    mut last: Local<HashMap<Entity, bool>>,
) {
    for (entity, follow) in q.iter() {
        let prev = last.insert(entity, follow.stick_to_bottom);
        if prev != Some(follow.stick_to_bottom) {
            writer.write(TerminalScrollFollowChanged {
                entity,
                stick_to_bottom: follow.stick_to_bottom,
            });
        }
    }
    last.retain(|e, _| q.get(*e).is_ok());
}
