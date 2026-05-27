//! Per-popup `EditorAction` interceptors.
//!
//! Hosted by the LSP UI feature so the dispatcher (`crate::input::dispatch`)
//! doesn't need to know about popup state details — each interceptor
//! returns `true` when it consumed the action and the dispatcher early-
//! returns. The editing handlers in `bevy_instanced_text_editor` never see the
//! consumed event.

use crate::input::actions;
use crate::input::keybindings::EditorAction;
use crate::lsp_ui::completion::{LspCompletionPopup, UnifiedCompletionItem};
use crate::lsp_ui::state::CompletionLifecycle;
use crate::settings::{AcceptSuggestionOnEnter, LspConfig, Suggest, TabCompletion};
use crate::text_view::TextBuffer;
use crate::types::{CodeEditor, CursorState};
use bevy::ecs::world::Mut;
use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;
use lsp_types::CompletionItemKind;

/// LSP completion popup interceptor.
///
/// When the popup is visible on `focused` and has filtered items, certain
/// `EditorAction`s are reinterpreted as popup navigation:
/// - `MoveCursorUp` / `MoveCursorDown`: cycle the selected item.
/// - `InsertNewline`: accept under `Suggest::accept_on_enter`
///   (`On` always, `Smart` for snippet / function / method / constructor,
///   `Off` falls through to a literal newline).
/// - `InsertTab`: accept under `Suggest::tab_completion`
///   (`On` always, `OnlySnippets` for snippet items, `Off` falls through
///   to a literal tab).
/// - `ClearSelection`: dismiss the popup.
///
pub struct CompletionPopupConfig {
    pub max_visible: usize,
    pub accept_on_enter: AcceptSuggestionOnEnter,
    pub tab_completion: TabCompletion,
}

impl CompletionPopupConfig {
    pub fn new(lsp_settings: &LspConfig, suggest: Option<&Suggest>) -> Self {
        Self {
            max_visible: lsp_settings.completion.max_items,
            accept_on_enter: suggest
                .map(|s| s.accept_on_enter)
                .unwrap_or(AcceptSuggestionOnEnter::On),
            tab_completion: suggest
                .map(|s| s.tab_completion)
                .unwrap_or(TabCompletion::Off),
        }
    }
}

pub fn completion_popup_intercept(
    action: EditorAction,
    focused: Entity,
    completion_state: &mut Mut<'_, LspCompletionPopup>,
    completion_lc: &mut Mut<'_, CompletionLifecycle>,
    editor_q: &mut Query<
        (
            &mut CursorState,
            &mut TextBuffer<RopeBuffer>,
            &mut crate::types::fold::GotoLineState,
        ),
        With<CodeEditor>,
    >,
    config: &CompletionPopupConfig,
    replace_writer: &mut MessageWriter<bevy_instanced_text_editor::ReplaceRangeRequested>,
) -> bool {
    let filtered = completion_state.filtered_items();
    let filtered_count = filtered.len();
    let max_visible = config.max_visible;
    if !completion_state.visible || filtered_count == 0 {
        return false;
    }
    match action {
        EditorAction::MoveCursorUp => {
            if completion_state.selected_index > 0 {
                completion_state.selected_index -= 1;
            } else {
                completion_state.selected_index = filtered_count.saturating_sub(1);
            }
            completion_state.ensure_selected_visible_with_max(max_visible);
            true
        }
        EditorAction::MoveCursorDown => {
            if completion_state.selected_index + 1 < filtered_count {
                completion_state.selected_index += 1;
            } else {
                completion_state.selected_index = 0;
            }
            completion_state.ensure_selected_visible_with_max(max_visible);
            true
        }
        EditorAction::InsertNewline => {
            let accept = match config.accept_on_enter {
                AcceptSuggestionOnEnter::Off => false,
                AcceptSuggestionOnEnter::Smart => filtered
                    .get(completion_state.selected_index)
                    .is_some_and(is_snippet_or_callable),
                AcceptSuggestionOnEnter::On => true,
            };
            if !accept {
                return false;
            }
            apply_selected(
                focused,
                completion_state,
                completion_lc,
                &filtered,
                editor_q,
                replace_writer,
            );

            true
        }
        EditorAction::InsertTab => {
            let accept = match config.tab_completion {
                TabCompletion::Off => false,
                TabCompletion::OnlySnippets => filtered
                    .get(completion_state.selected_index)
                    .is_some_and(is_snippet),
                TabCompletion::On => true,
            };
            if !accept {
                return false;
            }
            apply_selected(
                focused,
                completion_state,
                completion_lc,
                &filtered,
                editor_q,
                replace_writer,
            );

            true
        }
        EditorAction::ClearSelection => {
            completion_state.dismiss();
            completion_lc.dismiss();
            true
        }
        _ => false,
    }
}

fn apply_selected(
    focused: Entity,
    completion_state: &mut Mut<'_, LspCompletionPopup>,
    completion_lc: &mut Mut<'_, CompletionLifecycle>,
    filtered: &[UnifiedCompletionItem],
    editor_q: &mut Query<
        (
            &mut CursorState,
            &mut TextBuffer<RopeBuffer>,
            &mut crate::types::fold::GotoLineState,
        ),
        With<CodeEditor>,
    >,
    replace_writer: &mut MessageWriter<bevy_instanced_text_editor::ReplaceRangeRequested>,
) {
    let Ok((cursor, _buffer, _)) = editor_q.get(focused) else {
        return;
    };
    if let Some(item) = filtered.get(completion_state.selected_index) {
        let label = item.label().to_string();
        let filter = completion_state.filter.clone();
        completion_state.remember_acceptance(&label, &filter);
    }
    actions::apply_completion(
        focused,
        cursor.cursor_pos,
        completion_state,
        completion_lc,
        replace_writer,
    );
}

fn is_snippet(item: &UnifiedCompletionItem) -> bool {
    matches!(
        item,
        UnifiedCompletionItem::Lsp(lsp) if lsp.kind == Some(CompletionItemKind::SNIPPET)
    )
}

fn is_snippet_or_callable(item: &UnifiedCompletionItem) -> bool {
    match item {
        UnifiedCompletionItem::Lsp(lsp) => matches!(
            lsp.kind,
            Some(
                CompletionItemKind::SNIPPET
                    | CompletionItemKind::FUNCTION
                    | CompletionItemKind::METHOD
                    | CompletionItemKind::CONSTRUCTOR
            )
        ),
        UnifiedCompletionItem::Word(_) => false,
    }
}
