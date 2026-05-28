//! Code folding types

use bevy::prelude::*;
use bevy_instanced_text_editor::RopeBuffer;

use crate::text_view::TextBuffer;
use crate::types::{CursorState, SelectionState};

/// Per-editor "go to line" dialog state.
#[derive(Clone, Debug, Default, Component, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct GotoLineState {
    pub active: bool,
    pub input: String,
}

impl GotoLineState {
    /// Returns the parsed line number (1-indexed), or `None` on invalid input.
    pub fn parse_line_number(&self) -> Option<usize> {
        self.input.trim().parse::<usize>().ok()
    }

    pub fn goto(
        &self,
        sel: &mut SelectionState,
        cursor: &mut CursorState,
        buffer: &TextBuffer<RopeBuffer>,
    ) -> bool {
        if let Some(line_num) = self.parse_line_number() {
            let total_lines = buffer.len_lines();
            // 1-indexed input → 0-indexed, clamped
            let target_line = line_num
                .saturating_sub(1)
                .min(total_lines.saturating_sub(1));
            let char_pos = buffer.line_to_char(target_line);
            cursor.cursor_pos = char_pos;
            sel.apply_primary_cursor(cursor);

            return true;
        }
        false
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.input.clear();
    }
}

/// Goto-line dialog interceptor.
///
/// When the dialog is active and the user presses `ClearSelection`
/// (Escape), dismisses the dialog without falling through to the
/// `bevy_instanced_text_editor::ClearSelectionRequested` handler. Returns `true`
/// when the action was consumed.
pub fn goto_line_intercept(
    action: crate::input::keybindings::EditorAction,
    state: &mut GotoLineState,
) -> bool {
    if matches!(
        action,
        crate::input::keybindings::EditorAction::ClearSelection
    ) && state.active
    {
        state.clear();
        return true;
    }
    false
}

#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
#[reflect(Debug, PartialEq)]
pub struct FoldRegion {
    /// 0-indexed, inclusive.
    pub start_line: usize,
    /// 0-indexed, inclusive.
    pub end_line: usize,
    pub is_folded: bool,
    pub kind: FoldKind,
    pub indent_level: usize,
}

impl FoldRegion {
    pub fn new(start_line: usize, end_line: usize, kind: FoldKind) -> Self {
        Self {
            start_line,
            end_line,
            is_folded: false,
            kind,
            indent_level: 0,
        }
    }

    pub fn contains_line(&self, line: usize) -> bool {
        line >= self.start_line && line <= self.end_line
    }

    /// `true` when folded and `line` is inside but not the first row.
    pub fn hides_line(&self, line: usize) -> bool {
        self.is_folded && line > self.start_line && line <= self.end_line
    }

    pub fn line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }

    pub fn hidden_line_count(&self) -> usize {
        if self.is_folded {
            self.end_line.saturating_sub(self.start_line)
        } else {
            0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
#[reflect(Debug, PartialEq, Hash)]
pub enum FoldKind {
    Function,
    Class,
    Block,
    Imports,
    Comment,
    /// Manual fold marker (`#region`).
    Region,
    Literal,
    Other,
}

impl FoldKind {
    /// Gutter indicator character.
    pub fn indicator(&self) -> char {
        match self {
            FoldKind::Function => '\u{0192}',
            FoldKind::Class => '\u{25C6}',
            FoldKind::Comment => '/',
            _ => '\u{25B6}',
        }
    }
}

/// Per-editor fold-region state.
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct FoldState {
    /// Sorted by start line.
    pub regions: Vec<FoldRegion>,
    /// Initialized to `usize::MAX` to force detection on first run.
    pub content_version: usize,
}

impl Default for FoldState {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            content_version: usize::MAX,
        }
    }
}

impl FoldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn add_region(&mut self, region: FoldRegion) {
        let pos = self
            .regions
            .iter()
            .position(|r| r.start_line > region.start_line)
            .unwrap_or(self.regions.len());
        self.regions.insert(pos, region);
    }

    pub fn region_at_line(&self, line: usize) -> Option<&FoldRegion> {
        self.regions.iter().find(|r| r.start_line == line)
    }

    pub fn region_at_line_mut(&mut self, line: usize) -> Option<&mut FoldRegion> {
        self.regions.iter_mut().find(|r| r.start_line == line)
    }

    pub fn toggle_fold_at_line(&mut self, line: usize) -> bool {
        if let Some(region) = self.region_at_line_mut(line) {
            region.is_folded = !region.is_folded;
            true
        } else {
            false
        }
    }

    pub fn fold_at_line(&mut self, line: usize) -> bool {
        if let Some(region) = self.region_at_line_mut(line) {
            if !region.is_folded {
                region.is_folded = true;
                return true;
            }
        }
        false
    }

    pub fn unfold_at_line(&mut self, line: usize) -> bool {
        if let Some(region) = self.region_at_line_mut(line) {
            if region.is_folded {
                region.is_folded = false;
                return true;
            }
        }
        false
    }

    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.regions.iter().any(|r| r.hides_line(line))
    }

    pub fn is_foldable_line(&self, line: usize) -> bool {
        self.regions.iter().any(|r| r.start_line == line)
    }

    pub fn is_folded_line(&self, line: usize) -> bool {
        self.regions
            .iter()
            .any(|r| r.start_line == line && r.is_folded)
    }

    pub fn fold_all(&mut self) {
        for region in &mut self.regions {
            region.is_folded = true;
        }
    }

    pub fn unfold_all(&mut self) {
        for region in &mut self.regions {
            region.is_folded = false;
        }
    }

    /// `level` 0 = top-level functions/classes.
    pub fn fold_level(&mut self, level: usize) {
        for region in &mut self.regions {
            if region.indent_level == level {
                region.is_folded = true;
            }
        }
    }

    pub fn total_hidden_lines(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| r.is_folded)
            .map(|r| r.hidden_line_count())
            .sum()
    }

    /// Convert a display line number to actual line number (accounting for folds).
    ///
    /// O(n_folded_regions) — walks the sorted fold list once instead of
    /// per-line `is_line_hidden` probes (which would be O(n_lines × n_regions)
    /// on big buffers like 150k-line sqlite3.c).
    pub fn display_to_actual_line(&self, display_line: usize) -> usize {
        // Each folded region hides rows `start_line+1..=end_line`. We walk
        // folds in `start_line` order, tracking a running `display` count
        // (visible rows emitted so far) and a `hidden_through` cursor — the
        // largest line index already known to be inside a folded ancestor.
        // Nested folds whose hidden range lies entirely under an ancestor
        // contribute nothing extra.
        let mut display = 0usize;
        let mut actual = 0usize;
        let mut hidden_through: usize = 0;

        for r in &self.regions {
            if !r.is_folded {
                continue;
            }
            // Skip folds fully contained inside an already-counted parent.
            if r.start_line < hidden_through {
                if r.end_line > hidden_through {
                    hidden_through = r.end_line;
                }
                continue;
            }
            // Visible rows from `actual` up to (and including) this fold's
            // placeholder line `r.start_line`.
            let visible_in_span = r.start_line + 1 - actual;
            if display + visible_in_span > display_line {
                return actual + (display_line - display);
            }
            display += visible_in_span;
            // After the fold, the next visible actual line is `end_line + 1`.
            actual = r.end_line + 1;
            hidden_through = r.end_line;
        }
        actual + (display_line - display)
    }

    /// Convert an actual line number to display line number (accounting for folds).
    ///
    /// O(n_folded_regions). Symmetric to `display_to_actual_line`.
    pub fn actual_to_display_line(&self, actual_line: usize) -> usize {
        let mut hidden = 0usize;
        let mut hidden_through: usize = 0;
        let mut started = false;

        for r in &self.regions {
            if !r.is_folded {
                continue;
            }
            if r.start_line >= actual_line {
                break;
            }
            // Skip folds inside a previously-counted parent.
            if started && r.start_line < hidden_through {
                if r.end_line > hidden_through {
                    let extra = r.end_line - hidden_through;
                    let cap = actual_line.saturating_sub(hidden_through + 1);
                    hidden += extra.min(cap);
                    hidden_through = r.end_line;
                }
                continue;
            }
            // Hidden rows: (start+1..=end), clamped to actual_line - 1.
            let fold_hidden_end = r.end_line.min(actual_line - 1);
            hidden += fold_hidden_end - r.start_line;
            hidden_through = r.end_line;
            started = true;
        }
        actual_line - hidden
    }

    pub fn innermost_region_containing(&self, line: usize) -> Option<&FoldRegion> {
        self.regions
            .iter()
            .filter(|r| r.contains_line(line))
            .max_by_key(|r| r.start_line) // The one starting latest is the innermost
    }

    pub fn reveal_line(&mut self, line: usize) {
        for region in &mut self.regions {
            if region.hides_line(line) {
                region.is_folded = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folded(start: usize, end: usize) -> FoldRegion {
        FoldRegion {
            start_line: start,
            end_line: end,
            is_folded: true,
            kind: FoldKind::Block,
            indent_level: 0,
        }
    }

    fn unfolded(start: usize, end: usize) -> FoldRegion {
        FoldRegion {
            start_line: start,
            end_line: end,
            is_folded: false,
            kind: FoldKind::Block,
            indent_level: 0,
        }
    }

    /// Reference implementation: walk every line, the same way the old code did.
    /// Quadratic but provably correct — used as a test oracle.
    fn ref_display_to_actual(state: &FoldState, display_line: usize) -> usize {
        let mut actual = 0usize;
        let mut display = 0usize;
        while display < display_line {
            if !state.is_line_hidden(actual) {
                display += 1;
            }
            actual += 1;
        }
        while state.is_line_hidden(actual) {
            actual += 1;
        }
        actual
    }

    fn ref_actual_to_display(state: &FoldState, actual_line: usize) -> usize {
        let mut display = 0usize;
        for line in 0..actual_line {
            if !state.is_line_hidden(line) {
                display += 1;
            }
        }
        display
    }

    fn check_against_ref(state: &FoldState, total_lines: usize) {
        // Map every visible display row through the new and reference impls;
        // they must agree for every input.
        let visible = (0..total_lines)
            .filter(|l| !state.is_line_hidden(*l))
            .count();
        for d in 0..=visible {
            let new = state.display_to_actual_line(d);
            let old = ref_display_to_actual(state, d);
            assert_eq!(new, old, "display_to_actual_line({d}) regressed");
        }
        for a in 0..total_lines {
            let new = state.actual_to_display_line(a);
            let old = ref_actual_to_display(state, a);
            assert_eq!(new, old, "actual_to_display_line({a}) regressed");
        }
    }

    #[test]
    fn no_folds_is_identity() {
        let state = FoldState::default();
        for d in 0..50 {
            assert_eq!(state.display_to_actual_line(d), d);
            assert_eq!(state.actual_to_display_line(d), d);
        }
    }

    #[test]
    fn single_fold_collapses_tail() {
        // Lines 0..20, fold rows 5..=10 (placeholder is 5; rows 6..=10 hidden).
        let mut state = FoldState::default();
        state.add_region(folded(5, 10));
        check_against_ref(&state, 20);
    }

    #[test]
    fn unfolded_regions_dont_hide() {
        let mut state = FoldState::default();
        state.add_region(unfolded(2, 7));
        state.add_region(unfolded(12, 15));
        for d in 0..20 {
            assert_eq!(state.display_to_actual_line(d), d);
            assert_eq!(state.actual_to_display_line(d), d);
        }
    }

    #[test]
    fn nested_folds_dont_double_count() {
        // Outer fold 5..=20 contains inner fold 8..=12. Both folded.
        let mut state = FoldState::default();
        state.add_region(folded(5, 20));
        state.add_region(folded(8, 12));
        check_against_ref(&state, 30);
    }

    #[test]
    fn adjacent_folds() {
        let mut state = FoldState::default();
        state.add_region(folded(3, 5));
        state.add_region(folded(6, 8));
        state.add_region(folded(9, 11));
        check_against_ref(&state, 15);
    }

    #[test]
    fn mixed_folded_and_unfolded() {
        let mut state = FoldState::default();
        state.add_region(folded(2, 5));
        state.add_region(unfolded(7, 10));
        state.add_region(folded(12, 15));
        check_against_ref(&state, 20);
    }

    #[test]
    fn many_folds_long_buffer() {
        // Stress: 200 small folds across 2000 lines, every other one folded.
        let mut state = FoldState::default();
        for i in 0..200 {
            let start = i * 10;
            let end = start + 4;
            state.add_region(if i % 2 == 0 {
                folded(start, end)
            } else {
                unfolded(start, end)
            });
        }
        check_against_ref(&state, 2000);
    }
}
