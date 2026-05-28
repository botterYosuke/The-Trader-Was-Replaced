//! Post-action LSP follow-up: backspace refilter and popup dismiss.

use crate::types::*;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;

/// Carries the action snapshot from `dispatch_action_events` to
/// `lsp_followup`. Cleared at the end of `lsp_followup` so it never leaks
/// into the next frame.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PendingActionFollowup {
    /// Backspace; `update_completion_filter` only fires on `DeleteBackward`.
    pub was_delete_backward: bool,
    /// `false` short-circuits `lsp_followup` before any LSP work runs.
    pub action_fired: bool,
}

pub fn lsp_followup(
    mut pending: ResMut<PendingActionFollowup>,
    input_focus: Res<InputFocus>,
    editor_q: Query<(&CursorState, &crate::text_view::TextBuffer<RopeBuffer>), With<CodeEditor>>,
    mut lsp_q: Query<
        (
            &bevy_lsp::LspClient,
            Option<&mut bevy_lsp::LspDocument>,
            &mut crate::lsp_ui::completion::LspCompletionPopup,
            &mut crate::lsp_ui::state::CompletionLifecycle,
        ),
        With<CodeEditor>,
    >,
) {
    if !pending.action_fired {
        return;
    }
    let snapshot = *pending;
    pending.action_fired = false;

    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((cursor, buffer)) = editor_q.get(entity) else {
        return;
    };
    let Ok((lsp_client, lsp_document, mut completion_state, mut completion_lc)) =
        lsp_q.get_mut(entity)
    else {
        return;
    };

    // (1) Backspace inside an active completion popup refilters or hides
    //     the popup based on whether the cursor is still past the popup's
    //     anchor position.
    if snapshot.was_delete_backward && completion_state.visible {
        if cursor.cursor_pos > completion_state.start_char_index {
            crate::input::actions::update_completion_filter(
                cursor,
                buffer.rope(),
                &mut completion_state,
            );
        } else if cursor.cursor_pos == completion_state.start_char_index {
            // Empty prefix: hide (Zed behavior).
            completion_state.dismiss();
            completion_lc.dismiss();
        } else {
            completion_state.dismiss();
            completion_lc.dismiss();
        }
    }

    let _ = (buffer, lsp_client, lsp_document);
}
