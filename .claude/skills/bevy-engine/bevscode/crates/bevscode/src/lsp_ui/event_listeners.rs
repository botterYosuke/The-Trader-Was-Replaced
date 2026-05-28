//! LSP event listener systems.
//!
//! These systems read editor events and translate them into LSP request
//! sends through each editor entity's `bevy_lsp::LspClient` Component.
//!
//! Position conversion goes through `bevy_lsp::rope_char_to_lsp_position`
//! with `PositionEncoding::Utf16` (LSP spec default).

use super::snippet;
use super::completion::{LspCompletionPopup, UnifiedCompletionItem};
use super::state::{
    CompletionLifecycle, HoverLifecycle, LspDebounceTimers, LspDidChangeBatcher, LspRenamePopup,
    LspSignatureHelpPopup, PendingLspRequest, RenameLifecycle, SessionTabstop,
    SignatureLifecycle, TabstopSession,
};
use crate::settings::LspConfig;
use crate::text_view::TextBuffer;
use crate::types::events::{
    CompletionApplied, CompletionDismissed, CompletionRequested, HoverRequested, RenameRequested,
    SignatureHelpRequested, TextEdited,
};
use crate::types::{CodeEditor, CursorState};
use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;
use bevy_lsp::{
    rope_byte_to_lsp_position, rope_char_to_lsp_position, LspDocument, LspMessage, LspRequest,
};
use lsp_types::{Range, TextDocumentContentChangeEvent};

type CompletionRequestQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static TextBuffer<RopeBuffer>,
        Option<&'static LspDocument>,
        &'static bevy_lsp::ServerCapabilities,
        &'static mut LspDebounceTimers,
        &'static LspConfig,
    ),
    With<CodeEditor>,
>;

type HoverRequestQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static TextBuffer<RopeBuffer>,
        Option<&'static LspDocument>,
        &'static bevy_lsp::ServerCapabilities,
        &'static mut LspDebounceTimers,
        &'static LspConfig,
    ),
    With<CodeEditor>,
>;

type RenameRequestQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TextBuffer<RopeBuffer>,
        Option<&'static LspDocument>,
        &'static bevy_lsp::ServerCapabilities,
        &'static mut LspRenamePopup,
        &'static mut RenameLifecycle,
    ),
    With<CodeEditor>,
>;

type SignatureHelpRequestQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TextBuffer<RopeBuffer>,
        Option<&'static LspDocument>,
        &'static bevy_lsp::ServerCapabilities,
        &'static mut LspSignatureHelpPopup,
        &'static mut SignatureLifecycle,
        Option<&'static crate::settings::Suggest>,
    ),
    With<CodeEditor>,
>;

type ApplyCompletionQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut bevy_instanced_text_editor::SelectionState,
        &'static mut bevy_instanced_text_editor::EditHistoryState,
        &'static mut CursorState,
        &'static mut TextBuffer<RopeBuffer>,
        &'static mut LspCompletionPopup,
        &'static mut CompletionLifecycle,
        &'static mut TabstopSession,
    ),
    With<CodeEditor>,
>;

/// Queue text edits into the [`LspDidChangeBatcher`] and arm its debounce
/// timer. The batched flush happens in
/// [`super::systems::sync_lsp_document`] when the timer expires.
///
/// Builds incremental [`TextDocumentContentChangeEvent`]s when each edit
/// carries a pre-edit rope snapshot (the editor entity has
/// [`bevy_instanced_text_editor::SnapshotPreEdit`]); otherwise the next flush
/// promotes to a full-document sync. The spec guarantees full-doc is
/// always valid, and `LspConfig::full_document_sync` forces this path
/// for recovery.
pub fn listen_text_edit_events(
    mut events: MessageReader<TextEdited>,
    mut query: Query<
        (
            &TextBuffer<RopeBuffer>,
            &bevy_lsp::ServerCapabilities,
            &mut LspDidChangeBatcher,
            &LspConfig,
        ),
        With<CodeEditor>,
    >,
) {
    let Ok((buffer, caps, mut batcher, settings)) = query.single_mut() else {
        return;
    };

    let enc = caps.position_encoding();
    let mut queued_any = false;
    for event in events.read() {
        queued_any = true;
        if settings.full_document_sync || event.pre_edit_rope.is_none() {
            batcher.force_full_doc = true;
            continue;
        }
        let pre = event.pre_edit_rope.as_ref().expect("checked above");
        let delta = &event.delta;
        let start = rope_byte_to_lsp_position(pre, delta.start_byte, enc);
        let end = rope_byte_to_lsp_position(pre, delta.old_end_byte, enc);
        let new_text = if delta.start_byte == delta.new_end_byte {
            String::new()
        } else {
            let new_start_char = buffer.byte_to_char(delta.start_byte);
            let new_end_char = buffer.byte_to_char(delta.new_end_byte);
            buffer
                .rope()
                .slice(new_start_char..new_end_char)
                .chars()
                .collect()
        };
        batcher.pending.push(TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text: new_text,
        });
    }

    if queued_any {
        let duration = std::time::Duration::from_millis(settings.did_change_delay_ms);
        if batcher.timer.duration() != duration {
            batcher.timer.set_duration(duration);
        }
        batcher.timer.reset();
    }
}

pub fn listen_completion_requests(
    mut events: MessageReader<CompletionRequested>,
    mut query: CompletionRequestQuery,
) {
    let Ok((buffer, lsp_document, caps, mut debounce, settings)) = query.single_mut() else {
        return;
    };
    let Some(lsp_document) = lsp_document else {
        return;
    };
    let enc = caps.position_encoding();
    for event in events.read() {
        debounce.pending_completion = Some(PendingLspRequest {
            uri: lsp_document.uri.clone(),
            position: rope_char_to_lsp_position(buffer.rope(), event.cursor_char, enc),
        });
        debounce
            .completion_timer
            .set_duration(std::time::Duration::from_millis(
                settings.completion.delay_ms,
            ));
        debounce.completion_timer.reset();
    }
}

pub fn listen_hover_requests(
    mut events: MessageReader<HoverRequested>,
    mut query: HoverRequestQuery,
) {
    let Ok((buffer, lsp_document, caps, mut debounce, settings)) = query.single_mut() else {
        return;
    };
    let Some(lsp_document) = lsp_document else {
        return;
    };
    let enc = caps.position_encoding();
    for event in events.read() {
        debounce.pending_hover = Some(PendingLspRequest {
            uri: lsp_document.uri.clone(),
            position: rope_char_to_lsp_position(buffer.rope(), event.cursor_char, enc),
        });
        debounce
            .hover_timer
            .set_duration(std::time::Duration::from_millis(settings.hover.delay_ms));
        debounce.hover_timer.reset();
    }
}

pub fn listen_rename_requests(
    mut events: MessageReader<RenameRequested>,
    mut query: RenameRequestQuery,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    let Ok((entity, buffer, lsp_document, caps, mut rename_state, mut rename_lc)) =
        query.single_mut()
    else {
        return;
    };
    let Some(lsp_document) = lsp_document else {
        return;
    };
    let enc = caps.position_encoding();
    for event in events.read() {
        let position = rope_char_to_lsp_position(buffer.rope(), event.cursor_char, enc);
        rename_state.start_prepare(position);
        let id = rename_lc.new_request();
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::PrepareRename {
                uri: lsp_document.uri.clone(),
                position,
                id,
            },
        });
    }
}

pub fn listen_signature_help_requests(
    mut events: MessageReader<SignatureHelpRequested>,
    mut query: SignatureHelpRequestQuery,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    let Ok((entity, buffer, lsp_document, caps, mut sig_help_state, mut sig_help_lc, suggest)) =
        query.single_mut()
    else {
        return;
    };
    let Some(lsp_document) = lsp_document else {
        return;
    };
    if suggest.is_some_and(|s| !s.parameter_hints.enabled) {
        let _ = events.read().count();
        return;
    }
    let enc = caps.position_encoding();
    for event in events.read() {
        sig_help_state.dismiss();
        sig_help_lc.dismiss();
        let id = sig_help_lc.new_request();
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::SignatureHelp {
                uri: lsp_document.uri.clone(),
                position: rope_char_to_lsp_position(buffer.rope(), event.cursor_char, enc),
                id,
            },
        });
    }
}

pub fn listen_dismiss_completion(
    mut events: MessageReader<CompletionDismissed>,
    mut query: Query<(&mut LspCompletionPopup, &mut CompletionLifecycle), With<CodeEditor>>,
) {
    let Ok((mut completion_state, mut completion_lc)) = query.single_mut() else {
        return;
    };
    for _ in events.read() {
        completion_state.items.clear();
        completion_state.dismiss();
        completion_lc.dismiss();
    }
}

/// Selection-change driver for `completionItem/resolve`. Owns the `&mut`
/// access to the popup; reads the local "last selected" cursor and fires
/// the resolve request when the selection has moved to an item we don't
/// have cached docs for.
pub fn drive_completion_resolve(
    mut query: Query<
        (
            Entity,
            &mut LspCompletionPopup,
            &bevy_lsp::ServerCapabilities,
        ),
        With<CodeEditor>,
    >,
    mut lsp_w: MessageWriter<LspRequest>,
    mut last_selected: Local<Option<usize>>,
) {
    let Ok((entity, mut popup, caps)) = query.single_mut() else {
        *last_selected = None;
        return;
    };
    if !popup.visible {
        *last_selected = None;
        return;
    }
    if !caps.supports_completion_resolve() {
        return;
    }
    let current = popup.selected_index;
    if Some(current) == *last_selected {
        return;
    }
    *last_selected = Some(current);

    let filtered = popup.filtered_items();
    let Some(item) = filtered.get(current).cloned() else {
        return;
    };
    let UnifiedCompletionItem::Lsp(lsp_item) = item else {
        return;
    };
    if popup.resolved.contains_key(&lsp_item.label) {
        return;
    }
    if let Some((label, _)) = &popup.pending_resolve {
        if label == &lsp_item.label {
            return;
        }
    }
    popup.resolve_request_id = popup.resolve_request_id.wrapping_add(1);
    let id = popup.resolve_request_id;
    popup.pending_resolve = Some((lsp_item.label.clone(), id));
    lsp_w.write(LspRequest {
        entity,
        msg: LspMessage::ResolveCompletionItem { item: lsp_item, id },
    });
}

/// Dismiss the completion popup when the cursor moves out of a position
/// where completions make sense. Mirrors Zed's logic: keep the menu
/// only when (a) the cursor is at-or-after the menu's anchor and (b) the
/// character just before the cursor is a word character. Anything else
/// (clicked elsewhere, typed `;` / `(` / space, hit Backspace past the
/// anchor) hides the menu immediately.
pub fn dismiss_completion_on_cursor_move(
    mut query: Query<
        (
            Ref<CursorState>,
            &TextBuffer<RopeBuffer>,
            &mut LspCompletionPopup,
            &mut CompletionLifecycle,
        ),
        With<CodeEditor>,
    >,
) {
    let Ok((cursor, buffer, mut completion_state, mut completion_lc)) = query.single_mut() else {
        return;
    };
    if !completion_state.visible || !cursor.is_changed() {
        return;
    }
    let start = completion_state.start_char_index;
    let pos = cursor.cursor_pos;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let in_anchor_range = pos >= start && pos <= buffer.len_chars();
    let prev_is_word = pos > 0 && is_word(buffer.char(pos - 1));
    if !in_anchor_range || !prev_is_word {
        completion_state.dismiss();
        completion_lc.dismiss();
    }
}

/// Tick the [`LspDebounceTimers`] (completion + hover) and fire the
/// pending LSP requests when they expire. Document highlights and code
/// actions tick their own component-local timers — see
/// [`super::systems::request_document_highlights`] and the popup-side
/// arming in `LspCodeActionsPopup`.
pub fn tick_lsp_debounce_timers(
    time: Res<Time>,
    mut query: Query<
        (
            Entity,
            &mut LspDebounceTimers,
            &mut CompletionLifecycle,
            &mut HoverLifecycle,
        ),
        With<CodeEditor>,
    >,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    let Ok((entity, mut debounce, mut completion_lc, mut hover_lc)) = query.single_mut() else {
        return;
    };

    if debounce.pending_completion.is_some() {
        debounce.completion_timer.tick(time.delta());
        if debounce.completion_timer.just_finished() {
            if let Some(req) = debounce.pending_completion.take() {
                let id = completion_lc.new_request();
                lsp_w.write(LspRequest {
                    entity,
                    msg: LspMessage::Completion {
                        uri: req.uri,
                        position: req.position,
                        id,
                    },
                });
            }
        }
    }

    if debounce.pending_hover.is_some() {
        debounce.hover_timer.tick(time.delta());
        if debounce.hover_timer.just_finished() {
            if let Some(req) = debounce.pending_hover.take() {
                let id = hover_lc.new_request();
                lsp_w.write(LspRequest {
                    entity,
                    msg: LspMessage::Hover {
                        uri: req.uri,
                        position: req.position,
                        id,
                    },
                });
            }
        }
    }
}

/// Advance the tabstop session on `Tab` / `Shift+Tab`, and end it on
/// `Escape`. Runs **before** `bevy_instanced_text_editor::handlers::edit::handle_insert_tab`
/// so we drain `InsertTabRequested` events when a session is active —
/// the underlying handler then sees no events and inserts no tabs.
///
/// `Escape` is intercepted via `ClearSelectionRequested` since that's
/// the action the dispatcher emits for Esc.
pub fn advance_tabstop_session(
    mut tab_events: MessageReader<bevy_instanced_text_editor::InsertTabRequested>,
    mut clear_events: MessageReader<bevy_instanced_text_editor::ClearSelectionRequested>,
    mut query: Query<
        (
            &mut bevy_instanced_text_editor::SelectionState,
            &mut bevy_instanced_text_editor::EditHistoryState,
            &mut CursorState,
            &TextBuffer<RopeBuffer>,
            &mut TabstopSession,
        ),
        With<CodeEditor>,
    >,
) {
    let Ok((mut sel, hist, mut cursor, buffer, mut session)) = query.single_mut() else {
        // Drain events even when there's no session entity (avoid leaks).
        let _ = tab_events.read().count();
        let _ = clear_events.read().count();
        return;
    };

    if !session.is_active() {
        // Session dormant — let the events flow through to the normal
        // handlers untouched.
        let _ = tab_events.read().count();
        let _ = clear_events.read().count();
        return;
    }

    // Esc ends the session.
    if clear_events.read().next().is_some() {
        session.end();
        // Drain remaining tab events so they don't trigger the underlying
        // tab handler this frame on a dead session.
        let _ = tab_events.read().count();
        return;
    }

    let tab_pressed = tab_events.read().next().is_some();
    if !tab_pressed {
        return;
    }

    // Drain any remaining Tab events — we consume the whole burst.
    let _ = tab_events.read().count();

    let next = session.current + 1;
    if next >= session.stops.len() {
        // Last stop was just visited; end the session and let cursor
        // remain wherever the user moved it. Final stop ($0) typically
        // sits where they want the caret to land.
        session.end();
        return;
    }
    session.current = next;
    let stop = session.stops[next].clone();
    let s = hist.resolve_anchor(buffer.rope(), &stop.start);
    let e = hist.resolve_anchor(buffer.rope(), &stop.end);
    cursor.cursor_pos = e;
    if s != e {
        sel.selections = bevy_instanced_text_editor::SelectionCollection::with_selection(e, s);
    } else {
        sel.selections = bevy_instanced_text_editor::SelectionCollection::with_cursor(e);
    }
}

/// End an active tabstop session when the cursor moves outside the
/// covered range (e.g. user clicked elsewhere) or when a non-snippet
/// edit happens. Cheap when no session is active.
pub fn end_tabstop_session_on_cursor_leave(
    mut query: Query<
        (
            Ref<CursorState>,
            &TextBuffer<RopeBuffer>,
            &mut bevy_instanced_text_editor::EditHistoryState,
            &mut TabstopSession,
        ),
        With<CodeEditor>,
    >,
) {
    let Ok((cursor, buffer, hist, mut session)) = query.single_mut() else {
        return;
    };
    if !session.is_active() || !cursor.is_changed() {
        return;
    }
    // Compute the covered range as [min(start), max(end)] across all
    // remaining stops. If the cursor leaves it, end.
    let mut min_start = usize::MAX;
    let mut max_end = 0;
    for stop in session.stops.iter().skip(session.current) {
        let s = hist.resolve_anchor(buffer.rope(), &stop.start);
        let e = hist.resolve_anchor(buffer.rope(), &stop.end);
        min_start = min_start.min(s);
        max_end = max_end.max(e);
    }
    let pos = cursor.cursor_pos;
    if pos < min_start || pos > max_end {
        session.end();
    }
}

/// Listens to CompletionApplied. Applies the edit synchronously via
/// `EditHistoryState::replace_range` (rather than emitting
/// `ReplaceRangeRequested`) so that, when the inserted item carries
/// snippet syntax, we can immediately create anchors for the tabstops
/// from the post-edit rope and start a `TabstopSession`.
pub fn listen_apply_completion(
    mut events: MessageReader<CompletionApplied>,
    mut query: ApplyCompletionQuery,
) {
    let Ok((
        mut sel,
        mut hist,
        mut cursor_state,
        mut buffer,
        mut completion_state,
        mut completion_lc,
        mut session,
    )) = query.single_mut()
    else {
        return;
    };
    for event in events.read() {
        let filtered = completion_state.filtered_items();
        if event.item_index >= filtered.len() {
            continue;
        }
        let item = &filtered[event.item_index];

        let cursor_pos = cursor_state.cursor_pos.min(buffer.len_chars());
        let line = buffer.char_to_line(cursor_pos);
        let line_start = buffer.line_to_char(line);
        let start_pos = completion_state
            .start_char_index
            .max(line_start)
            .min(cursor_pos);

        // Decide whether the item carries snippet syntax. LSP marks this
        // explicitly via `insert_text_format`; we only treat snippet items
        // (rust-analyzer marks function calls / for-loops / etc.) — word
        // completions go through verbatim.
        let parsed = match item {
            UnifiedCompletionItem::Lsp(lsp_item)
                if lsp_item.insert_text_format == Some(lsp_types::InsertTextFormat::SNIPPET) =>
            {
                Some(snippet::parse(
                    lsp_item.insert_text.as_deref().unwrap_or(&lsp_item.label),
                ))
            }
            _ => None,
        };
        let plain_text = match &parsed {
            Some(p) => p.text.clone(),
            None => item.insert_text().to_string(),
        };

        let outcome = hist.replace_range(
            &mut buffer,
            start_pos,
            cursor_pos,
            &plain_text,
            bevy_instanced_text_editor::EditKind::Other,
            true,
        );

        // Build a tabstop session from the parsed snippet.
        if let Some(parsed) = parsed {
            if parsed.has_tabstops() {
                session.end();
                let mut stops_sorted = parsed.tabstops.clone();
                // LSP semantics: walk in ascending id, with `0` (final
                // stop) at the end.
                stops_sorted.sort_by_key(|t| if t.id == 0 { u32::MAX } else { t.id });
                let inserted_start = outcome.start;
                let mut session_stops = Vec::with_capacity(stops_sorted.len());
                for stop in stops_sorted {
                    let abs_start = inserted_start + stop.start;
                    let abs_end = inserted_start + stop.end;
                    let start_anchor = hist.create_anchor(
                        buffer.rope(),
                        abs_start,
                        bevy_instanced_text_editor::AnchorBias::Left,
                    );
                    let end_anchor = hist.create_anchor(
                        buffer.rope(),
                        abs_end,
                        bevy_instanced_text_editor::AnchorBias::Right,
                    );
                    session_stops.push(SessionTabstop {
                        id: stop.id,
                        start: start_anchor,
                        end: end_anchor,
                    });
                }
                session.stops = session_stops;
                session.current = 0;
                // Move cursor to first tabstop and select its placeholder
                // range (if any).
                if let Some(first) = session.stops.first() {
                    let s = hist.resolve_anchor(buffer.rope(), &first.start);
                    let e = hist.resolve_anchor(buffer.rope(), &first.end);
                    cursor_state.cursor_pos = e;
                    if s != e {
                        sel.selections =
                            bevy_instanced_text_editor::SelectionCollection::with_selection(e, s);
                    } else {
                        sel.selections =
                            bevy_instanced_text_editor::SelectionCollection::with_cursor(e);
                    }
                }
            } else {
                cursor_state.cursor_pos = outcome.new_cursor_pos;
                sel.apply_primary_cursor(&cursor_state);
            }
        } else {
            cursor_state.cursor_pos = outcome.new_cursor_pos;
            sel.apply_primary_cursor(&cursor_state);
        }
        completion_state.dismiss();
        completion_lc.dismiss();
    }
}
