use crate::settings::WordsCompletionMode;
use bevy::prelude::*;
use lsp_types::*;
use std::collections::{HashMap, VecDeque};

pub const COMPLETION_MAX_VISIBLE_DEFAULT: usize = 10;

#[derive(Clone, Debug)]
pub struct WordCompletionItem {
    pub word: String,
}

#[derive(Clone, Debug)]
pub enum UnifiedCompletionItem {
    Lsp(Box<CompletionItem>),
    Word(WordCompletionItem),
}

impl UnifiedCompletionItem {
    pub fn label(&self) -> &str {
        match self {
            UnifiedCompletionItem::Lsp(item) => &item.label,
            UnifiedCompletionItem::Word(item) => &item.word,
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            UnifiedCompletionItem::Lsp(item) => item.detail.as_deref(),
            UnifiedCompletionItem::Word(_) => Some("word"),
        }
    }

    pub fn insert_text(&self) -> &str {
        match self {
            UnifiedCompletionItem::Lsp(item) => item.insert_text.as_deref().unwrap_or(&item.label),
            UnifiedCompletionItem::Word(item) => &item.word,
        }
    }

    pub fn is_word(&self) -> bool {
        matches!(self, UnifiedCompletionItem::Word(_))
    }

    pub fn kind_icon(&self) -> &'static str {
        match self {
            UnifiedCompletionItem::Lsp(item) => match item.kind {
                Some(CompletionItemKind::FUNCTION) | Some(CompletionItemKind::METHOD) => "ƒ",
                Some(CompletionItemKind::VARIABLE) => "𝑥",
                Some(CompletionItemKind::CLASS) | Some(CompletionItemKind::STRUCT) => "○",
                Some(CompletionItemKind::INTERFACE) => "◇",
                Some(CompletionItemKind::MODULE) => "□",
                Some(CompletionItemKind::PROPERTY) | Some(CompletionItemKind::FIELD) => "▪",
                Some(CompletionItemKind::CONSTANT) => "𝐶",
                Some(CompletionItemKind::ENUM) => "∈",
                Some(CompletionItemKind::ENUM_MEMBER) => "∋",
                Some(CompletionItemKind::KEYWORD) => "⌘",
                Some(CompletionItemKind::SNIPPET) => "✂",
                Some(CompletionItemKind::TYPE_PARAMETER) => "𝑇",
                _ => "•",
            },
            UnifiedCompletionItem::Word(_) => "𝑤",
        }
    }
}

#[derive(Component, Default)]
pub struct LspCompletionPopup {
    pub visible: bool,
    pub items: Vec<CompletionItem>,
    pub word_items: Vec<WordCompletionItem>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub start_char_index: usize,
    pub filter: String,
    pub is_incomplete: bool,
    /// Initial filter at the time the menu was opened. When the user keeps
    /// typing identifier chars (extending this prefix) and the previous
    /// response was complete, we refilter locally instead of re-querying.
    pub initial_query: String,
    /// Mirror of `LspConfig::completion::words_mode`, kept on the
    /// component so `filtered_items` doesn't need access to the resource.
    /// Synced once per frame in `sync_completion_settings`.
    pub words_mode: WordsCompletionMode,
    /// Cache of `completionItem/resolve` results, keyed by the **label**
    /// of the original item. Label-keying survives reordering when the
    /// filter changes; index-keying would not.
    pub resolved: HashMap<String, CompletionItem>,
    /// Bumped on each resolve request and on every dismiss / item-list
    /// change so stale resolve responses are dropped before they reach
    /// the popup data.
    pub resolve_request_id: u64,
    /// `(label, request_id)` of the in-flight resolve. None when no
    /// resolve is in flight.
    pub pending_resolve: Option<(String, u64)>,
    /// Most-recently-accepted completion labels, newest at the front,
    /// capped at [`RECENT_LABELS_CAP`]. Drives
    /// [`crate::settings::SuggestSelection::RecentlyUsed`] preselection.
    pub recent_labels: VecDeque<String>,
    /// Last-accepted label per `RECENT_PREFIX_LEN`-char prefix of the
    /// typed word at acceptance time. Drives
    /// [`crate::settings::SuggestSelection::RecentlyUsedByPrefix`].
    pub recent_by_prefix: HashMap<String, String>,
}

/// Maximum number of remembered recently-accepted labels.
pub const RECENT_LABELS_CAP: usize = 32;

/// Prefix length used to key `LspCompletionPopup::recent_by_prefix`.
pub const RECENT_PREFIX_LEN: usize = 3;

impl LspCompletionPopup {
    /// Clear popup-local state (filter, selection, scroll, resolved
    /// cache). The lifecycle id-bump that invalidates in-flight LSP
    /// responses lives on `CompletionLifecycle` — call its
    /// `.dismiss()` at the same site.
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.filter.clear();
        self.initial_query.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.is_incomplete = false;
        self.resolve_request_id = self.resolve_request_id.wrapping_add(1);
        self.pending_resolve = None;
        self.resolved.clear();
    }

    pub fn ensure_selected_visible(&mut self) {
        self.ensure_selected_visible_with_max(COMPLETION_MAX_VISIBLE_DEFAULT);
    }

    pub fn ensure_selected_visible_with_max(&mut self, max_visible: usize) {
        let filtered_count = self.filtered_items().len();
        if filtered_count == 0 {
            self.scroll_offset = 0;
            return;
        }

        self.selected_index = self.selected_index.min(filtered_count.saturating_sub(1));

        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + max_visible {
            self.scroll_offset = self.selected_index - max_visible + 1;
        }

        let max_scroll = filtered_count.saturating_sub(max_visible);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
    }

    pub fn filtered_items(&self) -> Vec<UnifiedCompletionItem> {
        use fuzzy_matcher::skim::SkimMatcherV2;
        use fuzzy_matcher::FuzzyMatcher;
        use std::collections::HashSet;

        let matcher = SkimMatcherV2::default();

        let mut lsp_scored: Vec<(UnifiedCompletionItem, i64)> = if self.filter.is_empty() {
            self.items
                .iter()
                .map(|item| (UnifiedCompletionItem::Lsp(Box::new(item.clone())), 0))
                .collect()
        } else {
            self.items
                .iter()
                .filter_map(|item| {
                    let score = matcher.fuzzy_match(&item.label, &self.filter).or_else(|| {
                        item.filter_text
                            .as_ref()
                            .and_then(|f| matcher.fuzzy_match(f, &self.filter))
                    });
                    score.map(|s| (UnifiedCompletionItem::Lsp(Box::new(item.clone())), s))
                })
                .collect()
        };

        lsp_scored.sort_by_key(|b| std::cmp::Reverse(b.1));

        let include_words = match self.words_mode {
            WordsCompletionMode::Disabled => false,
            WordsCompletionMode::Always => true,
            WordsCompletionMode::Fallback => self.items.is_empty() || self.is_incomplete,
        };

        let lsp_labels: HashSet<&str> = self.items.iter().map(|i| i.label.as_str()).collect();

        let mut word_scored: Vec<(UnifiedCompletionItem, i64)> =
            if !include_words || self.filter.is_empty() {
                Vec::new()
            } else {
                self.word_items
                    .iter()
                    .filter(|item| !lsp_labels.contains(item.word.as_str()))
                    .filter_map(|item| {
                        matcher
                            .fuzzy_match(&item.word, &self.filter)
                            .map(|s| (UnifiedCompletionItem::Word(item.clone()), s))
                    })
                    .collect()
            };

        word_scored.sort_by_key(|b| std::cmp::Reverse(b.1));

        let mut result: Vec<UnifiedCompletionItem> =
            lsp_scored.into_iter().map(|(item, _)| item).collect();
        result.extend(word_scored.into_iter().map(|(item, _)| item));

        result
    }

    /// Update word completions from the rope. Skips the scan entirely when
    /// `words_mode == Disabled` so we don't pay the per-keystroke cost.
    pub fn update_word_completions(&mut self, rope: &ropey::Rope, cursor_pos: usize) {
        use std::collections::HashSet;

        if self.words_mode == WordsCompletionMode::Disabled {
            self.word_items.clear();
            return;
        }

        let mut seen: HashSet<String> = HashSet::new();
        let mut words: Vec<WordCompletionItem> = Vec::new();

        let cursor_word = get_word_at_position(rope, cursor_pos);

        let chunk_text: String = rope.chunks().collect();
        let mut word_start: Option<usize> = None;

        for (i, c) in chunk_text.char_indices() {
            let is_word_char = c.is_alphanumeric() || c == '_';

            if is_word_char {
                if word_start.is_none() {
                    word_start = Some(i);
                }
            } else if let Some(start) = word_start {
                let word = &chunk_text[start..i];
                if word.len() >= 2
                    && cursor_word.as_ref().is_none_or(|cw| cw != word)
                    && !seen.contains(word)
                {
                    seen.insert(word.to_string());
                    words.push(WordCompletionItem {
                        word: word.to_string(),
                    });
                }
                word_start = None;
            }
        }

        if let Some(start) = word_start {
            let word = &chunk_text[start..];
            if word.len() >= 2
                && cursor_word.as_ref().is_none_or(|cw| cw != word)
                && !seen.contains(word)
            {
                words.push(WordCompletionItem {
                    word: word.to_string(),
                });
            }
        }

        self.word_items = words;
    }

    pub fn reset(&mut self) {
        self.visible = false;
        self.items.clear();
        self.word_items.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.filter.clear();
        self.is_incomplete = false;
        self.resolved.clear();
        self.pending_resolve = None;
    }

    /// Record the acceptance of `label` against the typed prefix `query`
    /// so [`crate::settings::SuggestSelection::RecentlyUsed`] and
    /// [`crate::settings::SuggestSelection::RecentlyUsedByPrefix`] can
    /// preselect it next time.
    pub fn remember_acceptance(&mut self, label: &str, query: &str) {
        self.recent_labels.retain(|l| l != label);
        self.recent_labels.push_front(label.to_string());
        while self.recent_labels.len() > RECENT_LABELS_CAP {
            self.recent_labels.pop_back();
        }
        let prefix_key: String = query.chars().take(RECENT_PREFIX_LEN).collect();
        if !prefix_key.is_empty() {
            self.recent_by_prefix.insert(prefix_key, label.to_string());
        }
    }

    /// Return the index in `filtered` of the item to preselect under
    /// `mode`. `None` ⇒ caller should keep `0` (best fuzzy match).
    pub fn preselect_index(
        &self,
        filtered: &[UnifiedCompletionItem],
        mode: crate::settings::SuggestSelection,
    ) -> Option<usize> {
        use crate::settings::SuggestSelection;
        match mode {
            SuggestSelection::First => None,
            SuggestSelection::RecentlyUsed => self.recent_match(filtered),
            SuggestSelection::RecentlyUsedByPrefix => {
                let prefix_key: String = self.filter.chars().take(RECENT_PREFIX_LEN).collect();
                if !prefix_key.is_empty() {
                    if let Some(label) = self.recent_by_prefix.get(&prefix_key) {
                        if let Some(idx) = filtered.iter().position(|it| it.label() == label) {
                            return Some(idx);
                        }
                    }
                }
                self.recent_match(filtered)
            }
        }
    }

    /// Newest-first scan of `recent_labels` for the first item that appears
    /// in `filtered`.
    fn recent_match(&self, filtered: &[UnifiedCompletionItem]) -> Option<usize> {
        for label in &self.recent_labels {
            if let Some(idx) = filtered.iter().position(|it| it.label() == label) {
                return Some(idx);
            }
        }
        None
    }
}

fn get_word_at_position(rope: &ropey::Rope, char_pos: usize) -> Option<String> {
    if char_pos == 0 || char_pos > rope.len_chars() {
        return None;
    }

    let line_idx = rope.char_to_line(char_pos);
    let line = rope.line(line_idx);
    let line_start_char = rope.line_to_char(line_idx);
    let pos_in_line = char_pos - line_start_char;

    let line_text: String = line.chars().collect();

    let byte_pos_in_line = line_text
        .char_indices()
        .nth(pos_in_line)
        .map(|(i, _)| i)
        .unwrap_or(line_text.len());

    let start = line_text[..byte_pos_in_line]
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);

    let end = line_text[byte_pos_in_line..]
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| byte_pos_in_line + i)
        .unwrap_or(line_text.len());

    if start < end {
        Some(line_text[start..end].to_string())
    } else {
        None
    }
}
