//! Per-event keyboard observer: character insertion, bracket/quote
//! auto-close, and LSP completion triggers.

use super::actions::{
    auto_close_allowed, get_closing_bracket, get_closing_quote, insert_closing_char,
    should_skip_auto_close,
};
#[cfg(feature = "lsp")]
use super::actions::{find_word_start, request_completion, update_completion_filter};
use super::auto_indent::should_dedent_close_brace;
use super::picking_backend::move_cursor;
#[cfg(feature = "lsp")]
use crate::settings::LspConfig;
use crate::types::*;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy_instanced_text_editor::{EditKind, RopeBuffer};

/// True when any modifier key is held — used by the char observer to skip
/// shortcut keystrokes (Ctrl+C, Cmd+S, etc.) that should be handled by
/// the action dispatcher via leafwing's `ActionState`, not inserted as
/// raw characters.
fn modifier_held(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight)
        || keyboard.pressed(KeyCode::SuperLeft)
        || keyboard.pressed(KeyCode::SuperRight)
        || keyboard.pressed(KeyCode::AltLeft)
        || keyboard.pressed(KeyCode::AltRight)
}

#[cfg(feature = "lsp")]
type KeyboardLspQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static mut bevy_lsp::LspDocument>,
        &'static bevy_lsp::ServerCapabilities,
        &'static mut crate::lsp_ui::completion::LspCompletionPopup,
        &'static mut crate::lsp_ui::state::LspRenamePopup,
        &'static mut crate::lsp_ui::state::CompletionLifecycle,
        &'static mut crate::lsp_ui::state::RenameLifecycle,
        Option<&'static bevy_tree_sitter::SyntaxTree>,
        &'static LspConfig,
    ),
    With<CodeEditor>,
>;

/// Per-event observer for keyboard input dispatched to the focused editor.
///
/// `bevy_input_focus` already routed this event because the editor entity
/// is in `InputFocus`. We never manually compare to `input_focus.get()`.
pub fn on_focused_keyboard(
    trigger: On<FocusedInput<KeyboardInput>>,
    mut editor_query: Query<
        (
            &mut SelectionState,
            &mut EditHistoryState,
            &mut CursorState,
            &mut crate::text_view::TextBuffer<RopeBuffer>,
            &crate::settings::AutoEdit,
            &crate::settings::Indentation,
            &crate::settings::Misc,
        ),
        With<CodeEditor>,
    >,
    #[cfg(feature = "lsp")] mut lsp_query: KeyboardLspQuery,
    #[cfg(feature = "lsp")] mut lsp_w: MessageWriter<bevy_lsp::LspRequest>,
    #[cfg(feature = "lsp")] suggest_q: Query<&crate::settings::Suggest, With<CodeEditor>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let entity = trigger.event().focused_entity;

    let Ok((mut sel, mut hist, mut cursor, mut buffer, auto_edit, indentation, misc)) =
        editor_query.get_mut(entity)
    else {
        return;
    };
    if misc.read_only {
        return;
    }

    #[cfg(feature = "lsp")]
    let Ok((
        mut lsp_document,
        capabilities,
        mut completion_state,
        mut rename_state,
        mut completion_lc,
        mut rename_lc,
        syntax_tree,
        lsp,
    )) = lsp_query.get_mut(entity)
    else {
        return;
    };

    let event = &trigger.event().input;
    if !event.state.is_pressed() {
        return;
    }

    // Rename modal eats input until dismissed.
    #[cfg(feature = "lsp")]
    if rename_state.visible {
        match &event.logical_key {
            Key::Character(text) => {
                for c in text.chars() {
                    if !c.is_control() {
                        rename_state.new_name.push(c);
                    }
                }
            }
            Key::Space => rename_state.new_name.push(' '),
            Key::Backspace => {
                rename_state.new_name.pop();
            }
            Key::Enter => {
                if rename_state.can_submit() {
                    if let (Some(position), Some(doc)) =
                        (rename_state.position, lsp_document.as_deref())
                    {
                        crate::lsp_ui::systems::execute_rename(
                            entity,
                            capabilities,
                            &doc.uri,
                            position,
                            rename_state.new_name.clone(),
                            &mut lsp_w,
                        );
                    }
                }
                rename_state.reset();
                rename_lc.dismiss();
            }
            Key::Escape => {
                rename_state.reset();
                rename_lc.dismiss();
            }
            _ => {}
        }
        return;
    }

    // Shortcut keystrokes (Ctrl+C, Cmd+S, …) are handled by the action
    // dispatcher; the char observer must not insert their key as text.
    if modifier_held(&keyboard) {
        return;
    }

    #[cfg(feature = "lsp")]
    let suggest = suggest_q.get(entity).ok();

    match &event.logical_key {
        Key::Character(text) => {
            for c in text.chars() {
                if c.is_control() {
                    continue;
                }
                insert_typed_char(
                    c,
                    &mut InsertCharCtx {
                        sel: &mut sel,
                        hist: &mut hist,
                        cursor: &mut cursor,
                        buffer: &mut buffer,
                        auto_edit,
                        indentation,
                    },
                    #[cfg(feature = "lsp")]
                    &mut InsertCharLspCtx {
                        lsp,
                        entity,
                        capabilities,
                        completion_state: &mut completion_state,
                        completion_lc: &mut completion_lc,
                        lsp_document: lsp_document.as_deref_mut(),
                        syntax_tree,
                        lsp_w: &mut lsp_w,
                        suggest,
                    },
                );
            }
        }
        Key::Space => {
            bevy_instanced_text_editor::widget::text_input::insert_char(
                &mut sel,
                &mut hist,
                &mut cursor,
                &mut buffer,
                ' ',
            );
            #[cfg(feature = "lsp")]
            {
                let _ = &lsp_document;
                completion_state.dismiss();
                completion_lc.dismiss();
            }
        }
        _ => {}
    }
}

struct InsertCharCtx<'a> {
    sel: &'a mut SelectionState,
    hist: &'a mut EditHistoryState,
    cursor: &'a mut CursorState,
    buffer: &'a mut crate::text_view::TextBuffer<RopeBuffer>,
    auto_edit: &'a crate::settings::AutoEdit,
    indentation: &'a crate::settings::Indentation,
}

#[cfg(feature = "lsp")]
struct InsertCharLspCtx<'a, 'w> {
    lsp: &'a LspConfig,
    entity: Entity,
    capabilities: &'a bevy_lsp::ServerCapabilities,
    completion_state: &'a mut crate::lsp_ui::completion::LspCompletionPopup,
    completion_lc: &'a mut crate::lsp_ui::state::CompletionLifecycle,
    lsp_document: Option<&'a mut bevy_lsp::LspDocument>,
    syntax_tree: Option<&'a bevy_tree_sitter::SyntaxTree>,
    lsp_w: &'a mut MessageWriter<'w, bevy_lsp::LspRequest>,
    suggest: Option<&'a crate::settings::Suggest>,
}

fn insert_typed_char(
    c: char,
    ctx: &mut InsertCharCtx<'_>,
    #[cfg(feature = "lsp")] lsp_ctx: &mut InsertCharLspCtx<'_, '_>,
) {
    let InsertCharCtx { sel, hist, cursor, buffer, auto_edit, indentation } = ctx;
    let quotes_mode = auto_edit.auto_closing_quotes;
    let brackets_mode = auto_edit.auto_closing_brackets;
    let overtype_mode = auto_edit.auto_closing_overtype;

    let is_closing_bracket = auto_edit.pairs.iter().any(|(_, close)| *close == c);
    let is_closing_quote = matches!(c, '"' | '\'' | '`');

    if !sel.selections.primary().has_selection() {
        if let Some(delete_count) =
            should_dedent_close_brace(c, buffer.rope(), cursor.cursor_pos, auto_edit, indentation)
        {
            let pos = cursor.cursor_pos;
            let outcome = hist.replace_range(
                buffer,
                pos - delete_count,
                pos,
                &c.to_string(),
                EditKind::Other,
                true,
            );
            cursor.cursor_pos = outcome.new_cursor_pos;
            sel.apply_primary_cursor(cursor);
            return;
        }
    }

    if (is_closing_bracket || is_closing_quote) && should_skip_auto_close(cursor, buffer.rope(), c)
    {
        let allow_overtype = match overtype_mode {
            crate::settings::AutoClosingAuto::Always => true,
            crate::settings::AutoClosingAuto::Auto => {
                bevy_instanced_text_editor::is_auto_pair_neighbor(buffer.rope(), cursor.cursor_pos)
            }
            crate::settings::AutoClosingAuto::Never => false,
        };
        if allow_overtype {
            move_cursor(cursor, buffer.rope(), 1);
            return;
        }
    }

    let has_selection = sel.selections.primary().has_selection();
    if has_selection {
        let surround_mode = auto_edit.auto_surround;
        let want_brackets = matches!(
            surround_mode,
            crate::settings::AutoSurround::Brackets
                | crate::settings::AutoSurround::LanguageDefined
        );
        let want_quotes = matches!(
            surround_mode,
            crate::settings::AutoSurround::Quotes | crate::settings::AutoSurround::LanguageDefined
        );
        let closing = if want_brackets {
            get_closing_bracket(c, &auto_edit.pairs)
        } else {
            None
        }
        .or_else(|| {
            if want_quotes {
                get_closing_quote(c)
            } else {
                None
            }
        });
        if let Some(close_char) = closing {
            let (start, end) = sel.selections.primary().range();
            let start = start.min(buffer.len_chars());
            let end = end.min(buffer.len_chars());
            buffer.insert_char(end, close_char);
            buffer.insert_char(start, c);
            return;
        }
    }

    bevy_instanced_text_editor::widget::text_input::insert_char(sel, hist, cursor, buffer, c);

    if auto_close_allowed(brackets_mode, buffer.rope(), cursor.cursor_pos) {
        if let Some(closing) = get_closing_bracket(c, &auto_edit.pairs) {
            insert_closing_char(cursor, buffer, closing);
        }
    }
    if auto_close_allowed(quotes_mode, buffer.rope(), cursor.cursor_pos) {
        if let Some(closing) = get_closing_quote(c) {
            let should_close = if c == '\'' {
                let cur_pos = cursor.cursor_pos;
                if cur_pos >= 2 {
                    !buffer.char(cur_pos - 2).is_alphanumeric()
                } else {
                    true
                }
            } else {
                true
            };
            if should_close {
                insert_closing_char(cursor, buffer, closing);
            }
        }
    }

    #[cfg(feature = "lsp")]
    {
        let InsertCharLspCtx {
            lsp, entity, capabilities, completion_state, completion_lc,
            lsp_document, syntax_tree, lsp_w, suggest,
        } = lsp_ctx;
        let entity = *entity;

        if lsp.completion.enabled {
            let cursor_pos = cursor.cursor_pos;

            let context = match syntax_tree.and_then(|st| st.tree.as_ref()) {
                Some(tree) => {
                    let byte = buffer.char_to_byte(cursor_pos);
                    crate::plugin::syntax_highlighting::syntax_context(tree, byte)
                }
                _ => crate::plugin::syntax_highlighting::SyntaxContext::Other,
            };
            let quick = suggest.map(|s| s.quick_suggestions).unwrap_or_default();
            let context_allows = match context {
                crate::plugin::syntax_highlighting::SyntaxContext::Other => {
                    !matches!(quick.other, crate::settings::QuickSuggestion::Off)
                }
                crate::plugin::syntax_highlighting::SyntaxContext::Comment => {
                    !matches!(quick.comments, crate::settings::QuickSuggestion::Off)
                }
                crate::plugin::syntax_highlighting::SyntaxContext::String => {
                    !matches!(quick.strings, crate::settings::QuickSuggestion::Off)
                }
            };
            let in_completion_context = context_allows;
            let on_triggers_allowed = suggest.is_none_or(|s| s.on_trigger_characters);

            let server_triggers = capabilities.completion_triggers();
            let triggers: &[String] = if !server_triggers.is_empty() {
                &server_triggers
            } else {
                &lsp.completion.trigger_characters
            };

            let mut is_trigger = false;
            if on_triggers_allowed {
                for trigger in triggers {
                    if trigger.len() == 1 {
                        if c.to_string() == *trigger {
                            is_trigger = true;
                            break;
                        }
                    } else if cursor_pos >= trigger.len() {
                        let start = cursor_pos - trigger.len();
                        let recent_text: String = buffer.slice(start..cursor_pos).chars().collect();
                        if recent_text == *trigger {
                            is_trigger = true;
                            break;
                        }
                    }
                }
            }

            if is_trigger && in_completion_context {
                completion_state.dismiss();
                completion_lc.dismiss();
                request_completion(
                    entity,
                    cursor,
                    buffer.rope(),
                    completion_state,
                    completion_lc,
                    lsp_document.as_deref(),
                    lsp_w,
                );
            } else if (c.is_alphanumeric() || c == '_') && in_completion_context {
                if completion_state.visible {
                    update_completion_filter(cursor, buffer.rope(), completion_state);
                } else {
                    let word_start = find_word_start(buffer.rope(), cursor.cursor_pos);
                    let word_len = cursor.cursor_pos - word_start;
                    if word_len >= lsp.completion.min_word_length {
                        completion_state.start_char_index = word_start;
                        request_completion(
                            entity,
                            cursor,
                            buffer.rope(),
                            completion_state,
                            completion_lc,
                            lsp_document.as_deref(),
                            lsp_w,
                        );
                    }
                }
            } else if completion_state.visible {
                completion_state.dismiss();
                completion_lc.dismiss();
            }
        }
    }
}
