//! Editor-specific helpers used by the focused-keyboard observer and LSP
//! handlers — bracket auto-close predicates and LSP completion glue.

use crate::text_view::TextBuffer;
use crate::types::*;
#[cfg(feature = "lsp")]
use bevy::prelude::{Entity, MessageWriter};
use bevy_instanced_text_editor::RopeBuffer;
use ropey::Rope;

#[cfg(feature = "lsp")]
use crate::lsp_ui::completion::LspCompletionPopup;
#[cfg(feature = "lsp")]
use bevy::log::trace;
#[cfg(feature = "lsp")]
use bevy_lsp::{LspDocument, LspMessage, LspRequest};

pub fn insert_closing_char(cursor: &CursorState, buffer: &mut TextBuffer<RopeBuffer>, c: char) {
    let cursor_pos = cursor.cursor_pos.min(buffer.len_chars());
    buffer.insert_char(cursor_pos, c);
}

pub fn get_closing_bracket(open: char, pairs: &[(char, char)]) -> Option<char> {
    pairs.iter().find(|(o, _)| *o == open).map(|(_, c)| *c)
}

pub fn get_closing_quote(c: char) -> Option<char> {
    match c {
        '"' | '\'' | '`' => Some(c),
        _ => None,
    }
}

/// Skip auto-close when the cursor already has the closing char in front of
/// it — typing the close key just steps over it.
pub fn should_skip_auto_close(cursor: &CursorState, rope: &Rope, closing: char) -> bool {
    let cursor_pos = cursor.cursor_pos;
    if cursor_pos >= rope.len_chars() {
        return false;
    }
    rope.char(cursor_pos) == closing
}

/// True when an `AutoClosingPairs` policy permits inserting the closing
/// character at the cursor's current position.
pub fn auto_close_allowed(
    mode: crate::settings::AutoClosingPairs,
    rope: &Rope,
    cursor_pos: usize,
) -> bool {
    use crate::settings::AutoClosingPairs;
    match mode {
        AutoClosingPairs::Never => false,
        AutoClosingPairs::Always | AutoClosingPairs::LanguageDefined => true,
        AutoClosingPairs::BeforeWhitespace => {
            if cursor_pos >= rope.len_chars() {
                return true;
            }
            let next = rope.char(cursor_pos);
            next.is_whitespace()
        }
    }
}

#[cfg(feature = "lsp")]
pub fn apply_completion(
    entity: Entity,
    cursor_pos: usize,
    completion_state: &mut LspCompletionPopup,
    completion_lc: &mut crate::lsp_ui::state::CompletionLifecycle,
    writer: &mut MessageWriter<bevy_instanced_text_editor::ReplaceRangeRequested>,
) {
    let filtered = completion_state.filtered_items();
    if let Some(item) = filtered.get(completion_state.selected_index) {
        let start = completion_state.start_char_index;
        let end = cursor_pos;
        if start <= end {
            writer.write(bevy_instanced_text_editor::ReplaceRangeRequested {
                entity,
                start,
                end,
                text: item.insert_text().to_string(),
                kind: bevy_instanced_text_editor::EditKind::Other,
                record_history: true,
            });
        }
    }
    completion_state.dismiss();
    completion_lc.dismiss();
}

#[cfg(feature = "lsp")]
pub fn find_word_start(rope: &ropey::Rope, cursor_pos: usize) -> usize {
    if cursor_pos == 0 {
        return 0;
    }

    let mut pos = cursor_pos;
    while pos > 0 {
        let prev_char = rope.char(pos - 1);
        if prev_char.is_alphanumeric() || prev_char == '_' {
            pos -= 1;
        } else {
            break;
        }
    }
    pos
}

#[cfg(feature = "lsp")]
pub fn update_completion_filter(
    cursor: &CursorState,
    rope: &Rope,
    completion_state: &mut LspCompletionPopup,
) {
    let cursor_pos = cursor.cursor_pos.min(rope.len_chars());
    let start = completion_state.start_char_index;

    if cursor_pos > start && start <= rope.len_chars() {
        let filter_text: String = rope.slice(start..cursor_pos).chars().collect();
        completion_state.filter = filter_text;
        completion_state.selected_index = 0;
        completion_state.scroll_offset = 0;

        trace!("[LSP] Filter updated: '{}'", completion_state.filter);
    } else {
        completion_state.filter.clear();
        completion_state.scroll_offset = 0;
    }
}

#[cfg(feature = "lsp")]
pub fn request_completion(
    entity: Entity,
    cursor: &CursorState,
    rope: &Rope,
    completion_state: &mut LspCompletionPopup,
    completion_lc: &mut crate::lsp_ui::state::CompletionLifecycle,
    lsp_document: Option<&LspDocument>,
    lsp_w: &mut MessageWriter<LspRequest>,
) {
    let cursor_pos = cursor.cursor_pos.min(rope.len_chars());
    let lsp_position =
        bevy_lsp::rope_char_to_lsp_position(rope, cursor_pos, bevy_lsp::PositionEncoding::Utf16);

    if let Some(doc) = lsp_document {
        trace!(
            "[LSP] Requesting completion at line={}, char={}, visible={}, start_idx={}",
            lsp_position.line,
            lsp_position.character,
            completion_state.visible,
            completion_state.start_char_index
        );

        if !completion_state.visible {
            completion_state.start_char_index = find_word_start(rope, cursor_pos);
            completion_state.items.clear();
            completion_state.selected_index = 0;
            completion_state.filter.clear();
            completion_state.initial_query.clear();
            completion_state.is_incomplete = false;
        }

        // Skip the LSP round-trip when the previous result was complete and
        // the new prefix is just an extension of the initial query — local
        // refilter is enough.
        let new_query: String = rope
            .slice(completion_state.start_char_index..cursor_pos)
            .chars()
            .collect();
        let can_refilter_locally = !completion_state.is_incomplete
            && !completion_state.items.is_empty()
            && !completion_state.initial_query.is_empty()
            && new_query.starts_with(&completion_state.initial_query);

        if !can_refilter_locally {
            let id = completion_lc.new_request();
            lsp_w.write(LspRequest {
                entity,
                msg: LspMessage::Completion {
                    uri: doc.uri.clone(),
                    position: lsp_position,
                    id,
                },
            });
            if completion_state.initial_query.is_empty() {
                completion_state.initial_query = new_query.clone();
            }
        }

        completion_state.filter = new_query;
        completion_state.update_word_completions(rope, cursor_pos);
        completion_state.visible =
            !completion_state.word_items.is_empty() || !completion_state.items.is_empty();
    } else {
        if !completion_state.visible {
            completion_state.start_char_index = find_word_start(rope, cursor_pos);
            completion_state.items.clear();
            completion_state.selected_index = 0;
            completion_state.filter.clear();
        }

        completion_state.update_word_completions(rope, cursor_pos);
        completion_state.visible = true;

        trace!(
            "[bevy_code_editor] No LSP document URI - using word completions only ({} words)",
            completion_state.word_items.len()
        );
    }
}
