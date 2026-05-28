//! Editor-side LSP adapter plugin.
//!
//! Bridges editor events to LSP requests, drains responses into per-editor
//! state, and materializes popup data into Components for the host renderer.

use bevy::prelude::*;

use crate::lsp_ui::event_listeners::{
    advance_tabstop_session, dismiss_completion_on_cursor_move, drive_completion_resolve,
    end_tabstop_session_on_cursor_leave, listen_apply_completion, listen_completion_requests,
    listen_dismiss_completion, listen_hover_requests, listen_rename_requests,
    listen_signature_help_requests, listen_text_edit_events, tick_lsp_debounce_timers,
};
use crate::lsp_ui::completion::LspCompletionPopup;
use crate::lsp_ui::inlay_splice::splice_inlays_into_line_styles;
use crate::lsp_ui::sync::{
    sync_code_actions_popup, sync_completion_popup, sync_document_highlights, sync_hover_popup,
    sync_inlay_hints, sync_rename_input, sync_signature_help_popup,
};
use crate::lsp_ui::systems::{
    cleanup_lsp_timeouts, clear_stale_diagnostics_on_edit, on_lsp_code_actions, on_lsp_completion,
    on_lsp_definition, on_lsp_diagnostics, on_lsp_document_highlights, on_lsp_format, on_lsp_hover,
    on_lsp_initialized, on_lsp_inlay_hints, on_lsp_prepare_rename, on_lsp_references,
    on_lsp_rename, on_lsp_resolved_completion, on_lsp_server_crashed, on_lsp_shutdown_ack,
    on_lsp_signature_help, request_document_highlights, request_inlay_hints, sync_lsp_document,
    MultipleLocationsEvent, NavigateToFileEvent, WorkspaceEditEvent,
};
use crate::settings::LspConfig;
use crate::types::CodeEditor;

pub struct LspPlugin;

impl Default for LspPlugin {
    fn default() -> Self {
        Self
    }
}

impl LspPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for LspPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<bevy_lsp::LspPlugin>() {
            app.add_plugins(bevy_lsp::LspPlugin);
        }

        app.add_message::<NavigateToFileEvent>();
        app.add_message::<MultipleLocationsEvent>();
        app.add_message::<WorkspaceEditEvent>();

        app.add_message::<crate::types::events::CompletionRequested>();
        app.add_message::<crate::types::events::HoverRequested>();
        app.add_message::<crate::types::events::RenameRequested>();
        app.add_message::<crate::types::events::SignatureHelpRequested>();
        app.add_message::<crate::types::events::CompletionDismissed>();
        app.add_message::<crate::types::events::CompletionApplied>();

        app.add_systems(
            Update,
            clear_stale_diagnostics_on_edit.before(on_lsp_diagnostics),
        );
        app.add_systems(
            Update,
            (
                on_lsp_initialized,
                on_lsp_diagnostics,
                on_lsp_completion,
                on_lsp_resolved_completion,
                on_lsp_hover,
                on_lsp_definition,
                on_lsp_references,
                on_lsp_format,
                on_lsp_signature_help,
                on_lsp_code_actions,
                on_lsp_inlay_hints,
            ),
        );
        app.add_systems(
            Update,
            (
                on_lsp_document_highlights,
                on_lsp_prepare_rename,
                on_lsp_rename,
                on_lsp_shutdown_ack,
                on_lsp_server_crashed,
                sync_lsp_document,
                request_inlay_hints,
                request_document_highlights,
                cleanup_lsp_timeouts,
                tick_lsp_debounce_timers,
            ),
        );

        app.add_systems(
            Update,
            (
                sync_completion_popup,
                sync_hover_popup,
                sync_signature_help_popup,
                sync_code_actions_popup,
                sync_rename_input,
                sync_inlay_hints,
                sync_document_highlights,
            ),
        );

        app.add_systems(
            Update,
            splice_inlays_into_line_styles
                .after(crate::display_map::plugin::produce_line_styles)
                .in_set(crate::display_map::plugin::LayoutSyncSet),
        );

        app.add_systems(
            Update,
            (
                listen_text_edit_events,
                listen_completion_requests,
                listen_hover_requests,
                listen_rename_requests,
                listen_signature_help_requests,
                listen_dismiss_completion,
                listen_apply_completion,
                dismiss_completion_on_cursor_move,
                drive_completion_resolve,
                sync_completion_settings,
                attach_snapshot_pre_edit_marker,
                end_tabstop_session_on_cursor_leave,
            ),
        );

        app.add_systems(
            Update,
            advance_tabstop_session
                .before(bevy_instanced_text_editor::widget::text_input::handle_insert_tab),
        );
    }
}

fn sync_completion_settings(
    mut query: Query<(&LspConfig, &mut LspCompletionPopup), With<CodeEditor>>,
) {
    for (settings, mut popup) in &mut query {
        let target = settings.completion.words_mode;
        if popup.words_mode != target {
            popup.words_mode = target;
        }
    }
}

type AttachSnapshotQuery<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<CodeEditor>,
        With<bevy_lsp::LspDocument>,
        Without<bevy_instanced_text_editor::SnapshotPreEdit>,
    ),
>;

fn attach_snapshot_pre_edit_marker(mut commands: Commands, q: AttachSnapshotQuery) {
    for entity in q.iter() {
        commands
            .entity(entity)
            .insert(bevy_instanced_text_editor::SnapshotPreEdit);
    }
}
