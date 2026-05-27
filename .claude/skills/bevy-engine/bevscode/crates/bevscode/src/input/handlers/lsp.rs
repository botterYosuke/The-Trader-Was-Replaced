//! LSP-related handlers -- RequestCompletion, GotoDefinition, RenameSymbol.

use crate::input::action_events::*;
use crate::input::actions::request_completion;
use crate::types::*;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;

pub fn handle_request_completion(
    mut events: MessageReader<RequestCompletionRequested>,
    input_focus: Res<InputFocus>,
    editor_q: Query<(&CursorState, &crate::text_view::TextBuffer<RopeBuffer>), With<CodeEditor>>,
    mut lsp_q: Query<
        (
            Option<&bevy_lsp::LspDocument>,
            &mut crate::lsp_ui::completion::LspCompletionPopup,
            &mut crate::lsp_ui::state::CompletionLifecycle,
        ),
        With<CodeEditor>,
    >,
    mut lsp_w: MessageWriter<bevy_lsp::LspRequest>,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((cursor, buffer)) = editor_q.get(entity) else {
        return;
    };
    let Ok((lsp_document, mut completion_state, mut completion_lc)) = lsp_q.get_mut(entity) else {
        return;
    };
    request_completion(
        entity,
        cursor,
        buffer.rope(),
        &mut completion_state,
        &mut completion_lc,
        lsp_document,
        &mut lsp_w,
    );
}

pub fn handle_goto_definition(mut events: MessageReader<GotoDefinitionRequested>) {
    // Mouse-click-to-go-to-definition fires this elsewhere; the keyboard
    // binding is unused but defined for future hosts.
    events.read().for_each(|_| {});
}

pub fn handle_rename_symbol(
    mut events: MessageReader<RenameSymbolRequested>,
    input_focus: Res<InputFocus>,
    editor_q: Query<(&CursorState, &crate::text_view::TextBuffer<RopeBuffer>), With<CodeEditor>>,
    mut lsp_q: Query<
        (
            Option<&bevy_lsp::LspDocument>,
            &bevy_lsp::ServerCapabilities,
            &mut crate::lsp_ui::state::LspRenamePopup,
        ),
        With<CodeEditor>,
    >,
    mut lsp_w: MessageWriter<bevy_lsp::LspRequest>,
) {
    if events.read().next().is_none() {
        return;
    }
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((cursor, buffer)) = editor_q.get(entity) else {
        return;
    };
    let Ok((lsp_document, capabilities, mut rename_state)) = lsp_q.get_mut(entity) else {
        return;
    };
    if !capabilities.supports_rename() {
        return;
    }
    let Some(doc) = lsp_document else {
        return;
    };
    let position = bevy_lsp::rope_char_to_lsp_position(
        buffer.rope(),
        cursor.cursor_pos,
        bevy_lsp::PositionEncoding::Utf16,
    );
    rename_state.start_prepare(position);
    crate::lsp_ui::systems::request_prepare_rename(
        entity,
        capabilities,
        &doc.uri,
        position,
        &mut lsp_w,
    );
}
