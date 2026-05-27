//! Bevy systems for LSP integration.
//!
//! Drains [`bevy_lsp::LspResponse`]s into per-editor state Components, drives
//! debounced `did_change` notifications, and emits editor-side messages
//! (navigate, multiple-locations, workspace-edit) when the response flow
//! requires host action.

use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;
use lsp_types::*;

use crate::text_view::TextBuffer;
use crate::types::{CodeEditor, CursorState};
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::MonoCellWidth;

use super::completion::LspCompletionPopup;
use super::state::{
    CodeActionsLifecycle, CompletionLifecycle, HoverLifecycle, LspCodeActionsPopup,
    LspDidChangeBatcher, LspDocumentHighlights, LspHoverPopup, LspInlayHints, LspRenamePopup,
    LspSignatureHelpPopup, RenameLifecycle, SignatureLifecycle,
};
use bevy_lsp::{
    CodeActionOrCommand, LspClient, LspCodeActionsResponse, LspCompletionResponse,
    LspDefinitionResponse, LspDiagnosticsUpdated, LspDocument, LspDocumentHighlightsResponse,
    LspFormatResponse, LspHoverResponse, LspInlayHintsResponse, LspMessage,
    LspPrepareRenameResponse, LspReferencesResponse, LspRenameResponse, LspRequest,
    LspResolvedCompletionItem, LspServerCrashed, LspServerInitialized, LspShutdownAck,
    LspSignatureHelpResponse, ServerCapabilities,
};

type LspServerCrashedQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut LspCompletionPopup,
        &'static mut LspHoverPopup,
        &'static mut LspSignatureHelpPopup,
        &'static mut LspCodeActionsPopup,
        &'static mut LspDocumentHighlights,
        &'static mut LspRenamePopup,
        &'static mut CompletionLifecycle,
        &'static mut HoverLifecycle,
        &'static mut SignatureLifecycle,
        &'static mut CodeActionsLifecycle,
        &'static mut RenameLifecycle,
    ),
    With<CodeEditor>,
>;

type RequestInlayHintsQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static LspClient,
        &'static ServerCapabilities,
        Ref<'static, TextBuffer<RopeBuffer>>,
        Ref<'static, ScrollPosition>,
        Ref<'static, ComputedNode>,
        Option<&'static LspDocument>,
        &'static mut LspInlayHints,
        &'static TextFont,
        &'static bevy::text::LineHeight,
        &'static MonoCellWidth,
        Option<&'static crate::settings::Suggest>,
    ),
    With<CodeEditor>,
>;

type RequestDocumentHighlightsQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ServerCapabilities,
        &'static CursorState,
        &'static TextBuffer<RopeBuffer>,
        Option<&'static LspDocument>,
        &'static mut LspDocumentHighlights,
        &'static crate::settings::LspConfig,
    ),
    With<CodeEditor>,
>;

/// Diagnostic marker for rendering in editor
#[derive(Component, Clone, Debug)]
pub struct DiagnosticMarker {
    /// URI of the file this diagnostic belongs to — used to scope despawn to
    /// the editor whose document matches the incoming batch's URI.
    pub uri: Url,
    /// Line number (0-indexed)
    pub line: usize,
    /// Diagnostic severity
    pub severity: DiagnosticSeverity,
    /// Diagnostic message
    pub message: String,
    /// Text range
    pub range: Range,
}

/// Message emitted when navigation to a different file is requested
#[derive(bevy::prelude::Message, Clone, Debug)]
pub struct NavigateToFileEvent {
    /// URI of the file to open
    pub uri: Url,
    /// Line number (0-indexed)
    pub line: usize,
    /// Character position in line (0-indexed)
    pub character: usize,
}

/// Message emitted when there are multiple definition/reference locations
#[derive(bevy::prelude::Message, Clone, Debug)]
pub struct MultipleLocationsEvent {
    /// All available locations
    pub locations: Vec<Location>,
    /// Type of locations (definition, references, etc.)
    pub location_type: LocationType,
}

/// Type of location event
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationType {
    Definition,
    References,
}

/// Message emitted when a workspace edit needs to be applied
#[derive(bevy::prelude::Message, Clone, Debug)]
pub struct WorkspaceEditEvent {
    /// The workspace edit to apply
    pub edit: WorkspaceEdit,
}

/// Records server capabilities on the editor when the server finishes
/// `initialize`. `LspClient.initialized` is already flipped by `bevy_lsp`'s
/// drain.
pub fn on_lsp_initialized(
    mut events: MessageReader<LspServerInitialized>,
    mut q: Query<&mut ServerCapabilities, With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok(mut capabilities) = q.get_mut(ev.entity) else {
            continue;
        };
        capabilities.set(ev.capabilities.clone());
        #[cfg(debug_assertions)]
        debug!("[LSP] Server initialized");
    }
}

/// Replace `DiagnosticMarker` entities for the editor whenever the server
/// publishes a fresh diagnostic batch.
pub fn on_lsp_diagnostics(
    mut commands: Commands,
    mut events: MessageReader<LspDiagnosticsUpdated>,
    diagnostics_q: Query<(Entity, &DiagnosticMarker)>,
    editors: Query<
        (
            &crate::settings::RenderSettings,
            &crate::settings::Misc,
            Option<&LspDocument>,
        ),
        With<CodeEditor>,
    >,
) {
    for ev in events.read() {
        let Ok((render, misc, lsp_document)) = editors.get(ev.entity) else {
            info!(
                "[LSP] on_lsp_diagnostics: dropping event for entity={} (not a CodeEditor with RenderSettings+Misc); diag_count={}",
                ev.entity,
                ev.diagnostics.len(),
            );
            continue;
        };
        // The LSP server publishes diagnostics for every file it knows about,
        // not just the one this editor has open. Skip batches whose URI does
        // not match this editor's document — otherwise we paint another file's
        // line offsets onto this editor's buffer.
        if let Some(doc) = lsp_document {
            if doc.uri != ev.uri {
                info!(
                    "[LSP] on_lsp_diagnostics: skipping entity={} (uri mismatch: ev.uri={} doc.uri={})",
                    ev.entity, ev.uri, doc.uri,
                );
                continue;
            }
            // Drop batches the server computed against an older buffer
            // version: their (line, col) offsets refer to text that no
            // longer exists. The next batch (against the latest didChange)
            // will arrive once the server catches up.
            if let Some(v) = ev.version {
                if v < doc.version() {
                    info!(
                        "[LSP] on_lsp_diagnostics: dropping stale batch entity={} (ev.version={} < doc.version={})",
                        ev.entity,
                        v,
                        doc.version(),
                    );
                    continue;
                }
            }
        }
        let render_decorations = match render.render_validation_decorations {
            crate::settings::RenderValidationDecorations::Off => false,
            crate::settings::RenderValidationDecorations::On => true,
            crate::settings::RenderValidationDecorations::Editable => !misc.read_only,
        };
        info!(
            "[LSP] on_lsp_diagnostics: entity={} uri={} diag_count={} render_decorations={} (mode={:?} read_only={})",
            ev.entity,
            ev.uri,
            ev.diagnostics.len(),
            render_decorations,
            render.render_validation_decorations,
            misc.read_only,
        );
        for (i, d) in ev.diagnostics.iter().enumerate() {
            info!(
                "[LSP]   diag[{}] line={} col={}..{} severity={:?} src={:?} code={:?} msg={:?}",
                i,
                d.range.start.line,
                d.range.start.character,
                d.range.end.character,
                d.severity,
                d.source,
                d.code,
                d.message,
            );
        }
        for (entity, marker) in diagnostics_q.iter() {
            if marker.uri == ev.uri {
                commands
                    .entity(entity)
                    .queue_silenced(bevy::ecs::system::entity_command::despawn());
            }
        }
        if !render_decorations {
            continue;
        }
        for diagnostic in &ev.diagnostics {
            commands.spawn(DiagnosticMarker {
                uri: ev.uri.clone(),
                line: diagnostic.range.start.line as usize,
                severity: diagnostic.severity.unwrap_or(DiagnosticSeverity::HINT),
                message: diagnostic.message.clone(),
                range: diagnostic.range,
            });
        }
    }
}

/// Drop `DiagnosticMarker` entities whose URI matches a recently-edited
/// editor's document. Stored line numbers refer to the pre-edit buffer; once
/// the user types, those numbers no longer match the current text, so the
/// squiggle would land on the wrong line. We clear them and wait for the
/// next `publishDiagnostics` from the server.
pub fn clear_stale_diagnostics_on_edit(
    mut commands: Commands,
    diagnostics_q: Query<(Entity, &DiagnosticMarker)>,
    editors: Query<(Ref<TextBuffer<RopeBuffer>>, &LspDocument), With<CodeEditor>>,
) {
    for (buffer, doc) in editors.iter() {
        if !buffer.is_changed() {
            continue;
        }
        for (entity, marker) in diagnostics_q.iter() {
            if marker.uri == doc.uri {
                commands
                    .entity(entity)
                    .queue_silenced(bevy::ecs::system::entity_command::despawn());
            }
        }
    }
}

/// Drop stale completion responses, reset resolve cache, decide visibility
/// based on whether the cursor is still in the prefix word.
pub fn on_lsp_completion(
    mut events: MessageReader<LspCompletionResponse>,
    mut q: Query<
        (
            &CursorState,
            &TextBuffer<RopeBuffer>,
            &mut LspCompletionPopup,
            &mut CompletionLifecycle,
            Option<&crate::settings::Suggest>,
        ),
        With<CodeEditor>,
    >,
) {
    for ev in events.read() {
        let Ok((cursor_state, buffer, mut completion_state, mut completion_lc, suggest)) =
            q.get_mut(ev.entity)
        else {
            continue;
        };
        trace!(
            "[LSP] Completion(id={}): {} items, incomplete={}",
            ev.id,
            ev.items.len(),
            ev.is_incomplete
        );
        if !completion_lc.accept_response(ev.id) {
            continue;
        }
        let cursor_in_prefix = {
            let pos = cursor_state.cursor_pos;
            let start = completion_state.start_char_index;
            let max_prefix_len = buffer.len_chars().saturating_sub(start);
            let end_max = start + max_prefix_len;
            if pos < start || pos > end_max {
                false
            } else {
                let slice: String = buffer.slice(start..pos).chars().collect();
                slice.chars().all(|c| c.is_alphanumeric() || c == '_')
            }
        };
        completion_state.items = ev.items.clone();
        completion_state.is_incomplete = ev.is_incomplete;
        completion_state.visible = cursor_in_prefix && !completion_state.items.is_empty();
        let mode = suggest
            .map(|s| s.selection_mode)
            .unwrap_or(crate::settings::SuggestSelection::First);
        let filtered = completion_state.filtered_items();
        completion_state.selected_index = completion_state
            .preselect_index(&filtered, mode)
            .unwrap_or(0);
        // New item list invalidates any cached resolves keyed by labels that
        // may no longer be present.
        completion_state.resolved.clear();
        completion_state.pending_resolve = None;
        completion_state.resolve_request_id = completion_state.resolve_request_id.wrapping_add(1);
    }
}

pub fn on_lsp_resolved_completion(
    mut events: MessageReader<LspResolvedCompletionItem>,
    mut q: Query<&mut LspCompletionPopup, With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok(mut completion_state) = q.get_mut(ev.entity) else {
            continue;
        };
        trace!(
            "[LSP] ResolvedCompletionItem(id={}, label={})",
            ev.id,
            ev.item.label
        );
        if ev.id != completion_state.resolve_request_id {
            continue;
        }
        if let Some((label, pending_id)) = &completion_state.pending_resolve {
            if *pending_id == ev.id && label == &ev.item.label {
                completion_state
                    .resolved
                    .insert(ev.item.label.clone(), ev.item.clone());
                completion_state.pending_resolve = None;
            }
        }
    }
}

pub fn on_lsp_hover(
    mut events: MessageReader<LspHoverResponse>,
    mut q: Query<(&mut LspHoverPopup, &mut HoverLifecycle), With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok((mut hover_state, mut hover_lc)) = q.get_mut(ev.entity) else {
            continue;
        };
        // Accept any in-flight response that hasn't been superseded
        // by a more recent reply we've already displayed. rust-analyzer
        // hover round-trips can take seconds on a cold workspace, and
        // by then the move observer may have armed several more
        // requests at nearby positions; a strict id-equality check
        // would drop every one of them.
        if !hover_lc.accept_response(ev.id) {
            continue;
        }
        hover_state.content = ev.content.clone();
        hover_state.kind = ev.kind.clone();
        hover_state.range = ev.range;
        // Mark visible even when content is empty: sync_hover_popup may
        // still produce a popup if a diagnostic covers the trigger
        // position (VSCode shows diagnostics over squiggles even when
        // the server has no hover content for that position).
        hover_state.visible = true;
        // Publish the range as the hot zone so the move observer
        // doesn't re-arm or dismiss while the pointer wanders within
        // the identifier the popup describes.
        hover_lc.hot_zone = ev.range;
    }
}

pub fn on_lsp_definition(
    mut events: MessageReader<LspDefinitionResponse>,
    mut q: Query<
        (
            &mut CursorState,
            &TextBuffer<RopeBuffer>,
            Option<&LspDocument>,
        ),
        With<CodeEditor>,
    >,
    mut navigate_events: MessageWriter<NavigateToFileEvent>,
    mut multi_location_events: MessageWriter<MultipleLocationsEvent>,
) {
    for ev in events.read() {
        let Ok((mut cursor_state, buffer, lsp_document)) = q.get_mut(ev.entity) else {
            continue;
        };
        if ev.locations.is_empty() {
            continue;
        }

        #[cfg(debug_assertions)]
        debug!("[LSP] Definition: {} location(s)", ev.locations.len());

        if ev.locations.len() > 1 {
            multi_location_events.write(MultipleLocationsEvent {
                locations: ev.locations.clone(),
                location_type: LocationType::Definition,
            });
        }

        let location = &ev.locations[0];
        let current_uri = lsp_document.map(|d| &d.uri);
        let is_same_file = current_uri.is_some_and(|uri| uri == &location.uri);

        if is_same_file {
            let line_num = location.range.start.line as usize;
            let char_in_line = location.range.start.character as usize;
            if line_num < buffer.len_lines() {
                let line_start_char = buffer.line_to_char(line_num);
                let target_char_pos = line_start_char + char_in_line;
                cursor_state.cursor_pos = target_char_pos.min(buffer.len_chars());
            }
        } else {
            navigate_events.write(NavigateToFileEvent {
                uri: location.uri.clone(),
                line: location.range.start.line as usize,
                character: location.range.start.character as usize,
            });
        }
    }
}

pub fn on_lsp_references(
    mut events: MessageReader<LspReferencesResponse>,
    editors: Query<Entity, With<CodeEditor>>,
    mut multi_location_events: MessageWriter<MultipleLocationsEvent>,
) {
    for ev in events.read() {
        if editors.get(ev.entity).is_err() {
            continue;
        }
        #[cfg(debug_assertions)]
        debug!("[LSP] References: {} location(s)", ev.locations.len());
        if !ev.locations.is_empty() {
            multi_location_events.write(MultipleLocationsEvent {
                locations: ev.locations.clone(),
                location_type: LocationType::References,
            });
        }
    }
}

pub fn on_lsp_format(
    mut events: MessageReader<LspFormatResponse>,
    q: Query<&TextBuffer<RopeBuffer>, With<CodeEditor>>,
    mut replace_writer: MessageWriter<bevy_instanced_text_editor::ReplaceRangeRequested>,
) {
    for ev in events.read() {
        let Ok(buffer) = q.get(ev.entity) else {
            continue;
        };
        trace!("[LSP] Format: {} edit(s)", ev.edits.len());
        apply_text_edits(ev.entity, buffer, ev.edits.clone(), &mut replace_writer);
    }
}

pub fn on_lsp_signature_help(
    mut events: MessageReader<LspSignatureHelpResponse>,
    mut q: Query<(&mut LspSignatureHelpPopup, &mut SignatureLifecycle), With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok((mut sig_state, mut sig_lc)) = q.get_mut(ev.entity) else {
            continue;
        };
        #[cfg(debug_assertions)]
        debug!(
            "[LSP] SignatureHelp(id={}): {} signature(s)",
            ev.id,
            ev.signatures.len()
        );
        if !sig_lc.accept_response(ev.id) {
            continue;
        }
        sig_state.signatures = ev.signatures.clone();
        sig_state.active_signature = ev.active_signature.unwrap_or(0) as usize;
        sig_state.active_parameter = ev.active_parameter.unwrap_or(0) as usize;
        sig_state.visible = !sig_state.signatures.is_empty();
    }
}

pub fn on_lsp_code_actions(
    mut events: MessageReader<LspCodeActionsResponse>,
    mut q: Query<(&mut LspCodeActionsPopup, &mut CodeActionsLifecycle), With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok((mut action_state, mut action_lc)) = q.get_mut(ev.entity) else {
            continue;
        };
        #[cfg(debug_assertions)]
        debug!(
            "[LSP] CodeActions(id={}): {} action(s)",
            ev.id,
            ev.actions.len()
        );
        if !action_lc.accept_response(ev.id) {
            continue;
        }
        action_state.actions = ev.actions.clone();
        action_state.visible = !action_state.actions.is_empty();
        action_state.selected_index = 0;
    }
}

pub fn on_lsp_inlay_hints(
    mut events: MessageReader<LspInlayHintsResponse>,
    mut q: Query<&mut LspInlayHints, With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok(mut hint_state) = q.get_mut(ev.entity) else {
            continue;
        };
        #[cfg(debug_assertions)]
        debug!("[LSP] InlayHints: {} hint(s)", ev.hints.len());
        hint_state.hints = ev.hints.clone();
        hint_state.needs_refresh = false;
    }
}

pub fn on_lsp_document_highlights(
    mut events: MessageReader<LspDocumentHighlightsResponse>,
    mut q: Query<&mut LspDocumentHighlights, With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok(mut highlight_state) = q.get_mut(ev.entity) else {
            continue;
        };
        trace!(
            "[LSP] DocumentHighlights: {} highlight(s)",
            ev.highlights.len()
        );
        highlight_state.highlights = ev.highlights.clone();
        highlight_state.visible = !highlight_state.highlights.is_empty();
        highlight_state.in_flight_position = None;
    }
}

pub fn on_lsp_prepare_rename(
    mut events: MessageReader<LspPrepareRenameResponse>,
    mut q: Query<&mut LspRenamePopup, With<CodeEditor>>,
) {
    for ev in events.read() {
        let Ok(mut rename_state) = q.get_mut(ev.entity) else {
            continue;
        };
        trace!(
            "[LSP] PrepareRename: range={:?}, placeholder={:?}",
            ev.range,
            ev.placeholder
        );
        rename_state.on_prepare_response(ev.range, ev.placeholder.clone());
    }
}

pub fn on_lsp_rename(
    mut events: MessageReader<LspRenameResponse>,
    mut q: Query<
        (
            &TextBuffer<RopeBuffer>,
            Option<&LspDocument>,
            &mut LspRenamePopup,
        ),
        With<CodeEditor>,
    >,
    mut workspace_edit_events: MessageWriter<WorkspaceEditEvent>,
    mut replace_writer: MessageWriter<bevy_instanced_text_editor::ReplaceRangeRequested>,
) {
    for ev in events.read() {
        let Ok((buffer, lsp_document, mut rename_state)) = q.get_mut(ev.entity) else {
            continue;
        };
        #[cfg(debug_assertions)]
        debug!("[LSP] Rename: workspace edit received");

        if let Some(changes) = &ev.edit.changes {
            if let Some(doc) = lsp_document {
                if let Some(edits) = changes.get(&doc.uri) {
                    apply_text_edits(ev.entity, buffer, edits.clone(), &mut replace_writer);
                }
            }
        }

        workspace_edit_events.write(WorkspaceEditEvent {
            edit: ev.edit.clone(),
        });
        rename_state.reset();
    }
}

pub fn on_lsp_shutdown_ack(mut events: MessageReader<LspShutdownAck>) {
    for _ev in events.read() {
        // Caller follows up with `Exit`; nothing else to do here.
        debug!("[LSP] ShutdownAck received");
    }
}

pub fn on_lsp_server_crashed(
    mut events: MessageReader<LspServerCrashed>,
    mut q: LspServerCrashedQuery,
) {
    for ev in events.read() {
        let Ok((
            mut completion_state,
            mut hover_state,
            mut sig_state,
            mut action_state,
            mut highlight_state,
            mut rename_state,
            mut completion_lc,
            mut hover_lc,
            mut sig_lc,
            mut action_lc,
            mut rename_lc,
        )) = q.get_mut(ev.entity)
        else {
            continue;
        };
        warn!("[LSP] server reported crashed / channel closed");
        completion_state.dismiss();
        completion_lc.dismiss();
        hover_state.reset();
        hover_lc.dismiss();
        sig_state.dismiss();
        sig_lc.dismiss();
        action_state.dismiss();
        action_lc.dismiss();
        highlight_state.reset();
        rename_state.reset();
        rename_lc.dismiss();
    }
}

/// Apply text edits by emitting `ReplaceRangeRequested` events. The editor's
/// handler routes each through `replace_range`, keeping history, anchors,
/// and `OnEdit` consistent.
fn apply_text_edits(
    entity: Entity,
    buffer: &TextBuffer<RopeBuffer>,
    edits: Vec<TextEdit>,
    writer: &mut MessageWriter<bevy_instanced_text_editor::ReplaceRangeRequested>,
) {
    let mut edits_sorted = edits;
    edits_sorted.sort_by(|a, b| {
        let a_pos = (a.range.start.line, a.range.start.character);
        let b_pos = (b.range.start.line, b.range.start.character);
        b_pos.cmp(&a_pos)
    });

    for edit in edits_sorted {
        let start_line = edit.range.start.line as usize;
        let end_line = edit.range.end.line as usize;
        let start_char_col = edit.range.start.character as usize;
        let end_char_col = edit.range.end.character as usize;

        if start_line >= buffer.len_lines() {
            continue;
        }
        let start_pos = (buffer.line_to_char(start_line) + start_char_col).min(buffer.len_chars());
        let end_pos = if end_line < buffer.len_lines() {
            (buffer.line_to_char(end_line) + end_char_col).min(buffer.len_chars())
        } else {
            buffer.len_chars()
        };

        writer.write(bevy_instanced_text_editor::ReplaceRangeRequested {
            entity,
            start: start_pos,
            end: end_pos,
            text: edit.new_text,
            kind: bevy_instanced_text_editor::EditKind::Other,
            record_history: true,
        });
    }
}

/// Flush the [`LspDidChangeBatcher`] when its debounce timer expires.
///
/// `listen_text_edit_events` queues incremental change events and arms
/// the timer; this system ticks the timer and, on expiry, sends one
/// `textDocument/didChange` carrying the whole batch (or a full-document
/// sync if any queued edit lacked a pre-edit rope snapshot or
/// `LspConfig::full_document_sync` is on).
pub fn sync_lsp_document(
    time: Res<Time>,
    mut query: Query<
        (
            &TextBuffer<RopeBuffer>,
            Option<&mut LspDocument>,
            &mut LspDidChangeBatcher,
        ),
        With<CodeEditor>,
    >,
) {
    let Ok((buffer, lsp_document, mut batcher)) = query.single_mut() else {
        return;
    };
    if batcher.pending.is_empty() && !batcher.force_full_doc {
        return;
    }
    let Some(mut lsp_document) = lsp_document else {
        batcher.pending.clear();
        batcher.force_full_doc = false;
        return;
    };

    batcher.timer.tick(time.delta());
    if !batcher.timer.is_finished() {
        return;
    }

    if batcher.force_full_doc {
        batcher.pending.clear();
        lsp_document.push_full_sync(buffer.chunks().collect());
    } else {
        for change in std::mem::take(&mut batcher.pending) {
            lsp_document.push_change(change);
        }
    }

    batcher.force_full_doc = false;
    batcher.timer.reset();
}

/// System to request inlay hints for visible range
pub fn request_inlay_hints(
    mut query: RequestInlayHintsQuery,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    let Ok((
        entity,
        lsp_client,
        capabilities,
        buffer,
        scroll,
        computed,
        lsp_document,
        mut hint_state,
        font,
        lh,
        _mono,
        suggest,
    )) = query.single_mut()
    else {
        return;
    };
    let line_height = bevy_instanced_text::resolve_line_height(*lh, font.font_size);
    if !lsp_client.is_ready() || !capabilities.supports_inlay_hints() {
        return;
    }
    if let Some(s) = suggest {
        use crate::settings::InlayHintsEnabled;
        if matches!(
            s.inlay_hints.enabled,
            InlayHintsEnabled::Off | InlayHintsEnabled::OnUnlessPressed
        ) {
            return;
        }
    }

    if !hint_state.needs_refresh
        && !buffer.is_changed()
        && !scroll.is_changed()
        && !computed.is_changed()
    {
        return;
    }

    // The buffer changing invalidates the cache by line number: the line
    // that *was* `let mut app = App::new()` (with an `app:` parameter hint)
    // might now be `let zzz = 1`, but the cached `cached_range` still
    // covers the same line indices, so without this the stale hints would
    // stick to the new content until the user scrolled. Clearing here
    // also drops the visible labels immediately while the new request is
    // in flight, instead of leaving wrong text on-screen for a round trip.
    if buffer.is_changed() {
        hint_state.invalidate();
    }

    let Some(lsp_document) = lsp_document else {
        return;
    };

    // Calculate visible range with some buffer
    let inv = computed.inverse_scale_factor();
    let viewport_height = computed.size().y * inv;
    let visible_start_line = (scroll.y / line_height) as u32;
    let visible_lines = (viewport_height / line_height) as u32 + 10;
    let visible_end_line = (visible_start_line + visible_lines).min(buffer.len_lines() as u32);

    let range = Range {
        start: Position {
            line: visible_start_line,
            character: 0,
        },
        end: Position {
            line: visible_end_line,
            character: 0,
        },
    };

    // Check if range is already cached
    if hint_state.is_range_cached(&range) && !hint_state.needs_refresh {
        return;
    }

    lsp_w.write(LspRequest {
        entity,
        msg: LspMessage::InlayHint {
            uri: lsp_document.uri.clone(),
            range,
            id: 0,
        },
    });

    hint_state.cached_range = Some(range);
    hint_state.needs_refresh = false;
}

/// System to clean up LSP timeout requests
pub fn cleanup_lsp_timeouts(query: Query<&LspClient, With<CodeEditor>>) {
    for lsp_client in query.iter() {
        lsp_client.cleanup_timeouts();
    }
}

/// Returns `true` when the lifecycle's `dismiss_after` timer expired
/// this frame *and* the pointer is not inside the popup chrome — the
/// caller should then run its feature-specific dismiss. The lifecycle
/// `dismiss()` is called here so the timer can't fire twice.
fn tick_dismiss_grace(lc: &mut super::state::PopupLifecycleData, dt: std::time::Duration) -> bool {
    let Some(timer) = lc.dismiss_after.as_mut() else {
        return false;
    };
    timer.tick(dt);
    if !timer.just_finished() {
        return false;
    }
    if lc.pointer_in_popup {
        // Pointer arrived after we armed the grace — leave the popup
        // up, drop the timer; the next out-event will re-arm.
        lc.dismiss_after = None;
        return false;
    }
    lc.dismiss();
    true
}

pub fn tick_popup_dismiss_hover(
    time: Res<Time>,
    mut q: Query<(&mut super::state::HoverLifecycle, &mut LspHoverPopup), With<CodeEditor>>,
) {
    for (mut lc, mut state) in q.iter_mut() {
        if tick_dismiss_grace(&mut lc, time.delta()) {
            state.reset();
        }
    }
}

pub fn tick_popup_dismiss_completion(
    time: Res<Time>,
    mut q: Query<
        (
            &mut super::state::CompletionLifecycle,
            &mut LspCompletionPopup,
        ),
        With<CodeEditor>,
    >,
) {
    for (mut lc, mut state) in q.iter_mut() {
        if tick_dismiss_grace(&mut lc, time.delta()) {
            state.dismiss();
        }
    }
}

pub fn tick_popup_dismiss_signature(
    time: Res<Time>,
    mut q: Query<
        (
            &mut super::state::SignatureLifecycle,
            &mut LspSignatureHelpPopup,
        ),
        With<CodeEditor>,
    >,
) {
    for (mut lc, mut state) in q.iter_mut() {
        if tick_dismiss_grace(&mut lc, time.delta()) {
            state.dismiss();
        }
    }
}

pub fn tick_popup_dismiss_code_actions(
    time: Res<Time>,
    mut q: Query<
        (
            &mut super::state::CodeActionsLifecycle,
            &mut LspCodeActionsPopup,
        ),
        With<CodeEditor>,
    >,
) {
    for (mut lc, mut state) in q.iter_mut() {
        if tick_dismiss_grace(&mut lc, time.delta()) {
            state.dismiss();
        }
    }
}

pub fn tick_popup_dismiss_rename(
    time: Res<Time>,
    mut q: Query<(&mut super::state::RenameLifecycle, &mut LspRenamePopup), With<CodeEditor>>,
) {
    for (mut lc, mut state) in q.iter_mut() {
        if tick_dismiss_grace(&mut lc, time.delta()) {
            state.reset();
        }
    }
}

/// Helper to send signature help request. The id-bump lives on
/// [`SignatureLifecycle`] so the response handler can drop stale results.
pub fn request_signature_help(
    entity: Entity,
    capabilities: &ServerCapabilities,
    uri: &Url,
    position: Position,
    sig_lc: &mut SignatureLifecycle,
    lsp_w: &mut MessageWriter<LspRequest>,
) {
    if capabilities.supports_signature_help() {
        let id = sig_lc.new_request();
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::SignatureHelp {
                uri: uri.clone(),
                position,
                id,
            },
        });
    }
}

/// Send `textDocument/codeAction`. The id-bump lives on
/// [`CodeActionsLifecycle`] so the response handler can drop stale
/// results.
///
/// Helper, not a system — no producer wires this up yet. A future
/// "lightbulb / quick-fix" trigger system (cursor-on-diagnostic or
/// explicit `Ctrl+.`) will call this directly with the relevant range
/// and the diagnostics intersecting it.
pub fn request_code_actions(
    entity: Entity,
    capabilities: &ServerCapabilities,
    uri: &Url,
    range: Range,
    diagnostics: Vec<Diagnostic>,
    action_lc: &mut CodeActionsLifecycle,
    lsp_w: &mut MessageWriter<LspRequest>,
) {
    if capabilities.supports_code_actions() {
        let id = action_lc.new_request();
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::CodeAction {
                uri: uri.clone(),
                range,
                diagnostics,
                id,
            },
        });
    }
}

/// Execute a code action
pub fn execute_code_action(
    entity: Entity,
    action: &CodeActionOrCommand,
    lsp_w: &mut MessageWriter<LspRequest>,
) {
    match action {
        CodeActionOrCommand::Action(action) => {
            // TODO: Apply workspace edit when present.
            #[cfg(debug_assertions)]
            if let Some(edit) = &action.edit {
                debug!("[LSP] Code action has workspace edit: {:?}", edit);
            }

            if let Some(command) = &action.command {
                lsp_w.write(LspRequest {
                    entity,
                    msg: LspMessage::ExecuteCommand {
                        command: command.command.clone(),
                        arguments: command.arguments.clone(),
                    },
                });
            }
        }
        CodeActionOrCommand::Command(command) => {
            lsp_w.write(LspRequest {
                entity,
                msg: LspMessage::ExecuteCommand {
                    command: command.command.clone(),
                    arguments: command.arguments.clone(),
                },
            });
        }
    }
}

/// Fire `textDocument/documentHighlight` when the cursor settles on a
/// new position. Highlights all occurrences of the symbol under cursor
/// (the IDE feature where clicking on a name highlights every other use
/// in the same file). Debounce delay comes from
/// `LspConfig::highlight_delay_ms`.
pub fn request_document_highlights(
    time: Res<Time>,
    mut query: RequestDocumentHighlightsQuery,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    let Ok((
        entity,
        capabilities,
        cursor_state,
        buffer,
        lsp_document,
        mut highlight_state,
        settings,
    )) = query.single_mut()
    else {
        return;
    };
    if !capabilities.supports_document_highlight() {
        return;
    }

    let Some(lsp_document) = lsp_document else {
        return;
    };

    let cursor_pos = cursor_state.cursor_pos;

    if highlight_state.in_flight_position == Some(cursor_pos) {
        return;
    }
    if highlight_state.cursor_position == cursor_pos && highlight_state.visible {
        return;
    }

    if highlight_state.cursor_position != cursor_pos || highlight_state.debounce_timer.is_none() {
        highlight_state.cursor_position = cursor_pos;
        highlight_state.debounce_timer = Some(Timer::new(
            std::time::Duration::from_millis(settings.highlight_delay_ms),
            TimerMode::Once,
        ));
        if highlight_state.visible {
            highlight_state.highlights.clear();
            highlight_state.visible = false;
        }
        return;
    }

    let timer = highlight_state.debounce_timer.as_mut().unwrap();
    timer.tick(time.delta());
    if !timer.is_finished() {
        return;
    }
    highlight_state.debounce_timer = None;
    highlight_state.in_flight_position = Some(cursor_pos);

    let position = bevy_lsp::rope_char_to_lsp_position(
        buffer.rope(),
        cursor_pos,
        capabilities.position_encoding(),
    );
    lsp_w.write(LspRequest {
        entity,
        msg: LspMessage::DocumentHighlight {
            uri: lsp_document.uri.clone(),
            position,
            id: 0,
        },
    });
}

/// Helper to request prepare rename
pub fn request_prepare_rename(
    entity: Entity,
    capabilities: &ServerCapabilities,
    uri: &Url,
    position: Position,
    lsp_w: &mut MessageWriter<LspRequest>,
) {
    if capabilities.supports_prepare_rename() {
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::PrepareRename {
                uri: uri.clone(),
                position,
                id: 0,
            },
        });
    }
}

pub fn execute_rename(
    entity: Entity,
    capabilities: &ServerCapabilities,
    uri: &Url,
    position: Position,
    new_name: String,
    lsp_w: &mut MessageWriter<LspRequest>,
) {
    if capabilities.supports_rename() {
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::Rename {
                uri: uri.clone(),
                position,
                new_name,
                id: 0,
            },
        });
    }
}
