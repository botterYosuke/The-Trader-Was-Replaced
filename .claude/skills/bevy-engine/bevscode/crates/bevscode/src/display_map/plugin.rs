//! `DisplayMapPlugin` — plumbs the editor's fold / syntax / wrap state into
//! the engine's per-frame layout system via plain-data Components.
//!
//! The engine's `produce_layouts` (in `bevy_instanced_text::view::layout_builder`)
//! reads `HiddenLines` / `LineStyles` / `TextBounds` Components off each
//! `TextView` entity and drives layout production itself. This plugin owns:
//!
//! - A startup system that inserts default `HiddenLines` / `LineStyles`
//!   Components on every `CodeEditor` entity.
//! - Three producer systems (`produce_hidden_lines`, `produce_line_styles`,
//!   `sync_layout_wrap`) that recompute each Component when the editor's
//!   domain state changes. They run in [`LayoutSyncSet`], scheduled
//!   `.before(LayoutProduceSet)`.

use crate::types::events::TextEdited;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::{
    visible_buffer_range, FormattedSpan, HiddenLines, LayoutProduceSet, LineStyles, MonoCellWidth,
    TextBounds, TextBuffer,
};
use bevy_instanced_text_editor::RopeBuffer;
use std::collections::{HashMap, HashSet};

use super::styling::segs_to_runs;
use crate::plugin::syntax_highlighting::EditorSyntaxState;
use crate::settings::{EditorTheme, Indentation, SyntaxColors, Wrapping};
use crate::types::CodeEditor;
use crate::types::FoldState;

/// System set for sync systems that update the engine's data Components
/// from editor-domain inputs. Scheduled `.before(LayoutProduceSet)` so the
/// engine's layout system observes this frame's changes.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayoutSyncSet;

pub struct DisplayMapPlugin;

impl Plugin for DisplayMapPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Update,
            LayoutSyncSet
                .after(crate::plugin::ApplyStateSet)
                .before(LayoutProduceSet),
        );
        app.configure_sets(
            PostUpdate,
            LayoutSyncSet
                .after(bevy::ui::UiSystems::Layout)
                .before(LayoutProduceSet),
        );
        app.configure_sets(Update, LayoutProduceSet.in_set(crate::plugin::RenderingSet));

        app.add_systems(
            Startup,
            insert_styling_components.after(crate::plugin::syntax_highlighting::init_editor_syntax),
        );
        app.add_systems(
            Update,
            insert_styling_components
                .after(crate::plugin::syntax_highlighting::init_editor_syntax)
                .in_set(crate::plugin::ApplyStateSet),
        );
        app.add_systems(
            Update,
            (produce_hidden_lines, produce_line_styles, sync_layout_wrap).in_set(LayoutSyncSet),
        );
        app.add_systems(PostUpdate, sync_layout_wrap.in_set(LayoutSyncSet));
    }
}

/// On startup, attach default `HiddenLines` / `LineStyles` Components to
/// every `CodeEditor` entity that doesn't already have them. The producer
/// systems write into these on subsequent ticks.
pub(crate) fn insert_styling_components(
    mut commands: Commands,
    editors: Query<Entity, (With<CodeEditor>, Without<LineStyles>)>,
) {
    for entity in editors.iter() {
        commands
            .entity(entity)
            .insert((HiddenLines::default(), LineStyles::default()));
    }
}

/// Refresh the `HiddenLines` Component when `FoldState` changes.
///
/// Only writes when the hidden-line set actually differs from the current one.
/// `FoldState` change-detection fires on every async fold-detection completion
/// (which preserves `is_folded` flags across reparses), so without this check
/// every reparse would invalidate `HiddenLines` and cascade into a full
/// `produce_line_styles` rebuild via `Changed<HiddenLines>`.
type ProduceHiddenLinesQuery<'w, 's> = Query<
    'w,
    's,
    (&'static FoldState, &'static mut HiddenLines),
    (With<CodeEditor>, Changed<FoldState>),
>;

pub(crate) fn produce_hidden_lines(mut editors: ProduceHiddenLinesQuery) {
    for (fold_state, mut hidden) in editors.iter_mut() {
        let mut set = HashSet::new();
        for region in &fold_state.regions {
            if !region.is_folded {
                continue;
            }
            for line in (region.start_line + 1)..=region.end_line {
                set.insert(line);
            }
        }
        if *hidden.0 != set {
            *hidden = HiddenLines::new(set);
        }
    }
}

/// Recompute styled runs for each editor's visible buffer-line window and
/// write them into the entity's `LineStyles` Component.
///
/// On a pure content edit (only `TextBuffer<RopeBuffer>` changed), only the lines
/// touched by the edit are re-highlighted and merged into the existing map —
/// unchanged lines keep their cached runs. On any other change (scroll,
/// viewport resize, theme swap, new parse tree, hidden-lines update) the
/// full visible window is rebuilt from scratch.
pub(crate) fn produce_line_styles(
    mut editors: Query<
        (
            Entity,
            crate::settings::EditorRenderView,
            Option<&TextBounds>,
            Option<&HiddenLines>,
            &mut EditorSyntaxState,
            Option<&bevy_tree_sitter::SyntaxTree>,
            &mut LineStyles,
            &EditorTheme,
            &SyntaxColors,
            &crate::settings::RenderSettings,
        ),
        With<CodeEditor>,
    >,
    content_changed: Query<Entity, (With<CodeEditor>, Changed<TextBuffer<RopeBuffer>>)>,
    full_rebuild_changed: Query<
        Entity,
        (
            With<CodeEditor>,
            Or<(
                Changed<ScrollPosition>,
                Changed<ComputedNode>,
                Changed<HiddenLines>,
                Changed<EditorTheme>,
                Changed<SyntaxColors>,
            )>,
        ),
    >,
    syntax_tree_changed: Query<
        (Entity, &bevy_tree_sitter::SyntaxTree),
        (With<CodeEditor>, Changed<bevy_tree_sitter::SyntaxTree>),
    >,
    mut edit_events: MessageReader<TextEdited>,
    mut dirty_lines: Local<
        HashMap<
            Entity,
            (
                Option<(u32, u32)>,
                Vec<bevy_instanced_text_editor::EditDelta>,
            ),
        >,
    >,
) {
    let _span = bevy::prelude::info_span!("produce_line_styles").entered();
    for event in edit_events.read() {
        let start_row = event.delta.start_position.row;
        let new_end_row = event.delta.new_end_position.row;
        let dirty_range = Some((start_row, new_end_row));
        for entity in content_changed.iter() {
            match dirty_lines.entry(entity) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert((dirty_range, vec![event.delta]));
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    let entry = o.get_mut();
                    match (entry.0.as_mut(), dirty_range) {
                        (Some((lo, hi)), Some((new_lo, new_hi))) => {
                            *lo = (*lo).min(new_lo);
                            *hi = (*hi).max(new_hi);
                            entry.1.push(event.delta);
                        }
                        _ => *entry = (None, Vec::new()),
                    }
                }
            }
        }
    }

    for (entity, syntax_tree) in syntax_tree_changed.iter() {
        let incoming = (syntax_tree.dirty_rows, Vec::new());
        let entry = dirty_lines.entry(entity).or_insert(incoming);
        match (entry.0.as_mut(), syntax_tree.dirty_rows) {
            (Some((lo, hi)), Some((new_lo, new_hi))) => {
                *lo = (*lo).min(new_lo);
                *hi = (*hi).max(new_hi);
            }
            _ => entry.0 = None,
        }
    }

    let full_rebuild: HashSet<Entity> = full_rebuild_changed.iter().collect();
    let content_only: HashSet<Entity> = content_changed.iter().collect();
    let syntax_changed: HashSet<Entity> = syntax_tree_changed.iter().map(|(e, _)| e).collect();

    let any_dirty =
        !full_rebuild.is_empty() || !content_only.is_empty() || !syntax_changed.is_empty();
    if !any_dirty {
        dirty_lines.retain(|e, _| {
            content_only.contains(e) || full_rebuild.contains(e) || syntax_changed.contains(e)
        });
        return;
    }

    for (
        entity,
        rv,
        wrap,
        hidden,
        mut syntax,
        syntax_tree,
        mut line_styles,
        theme,
        syntax_theme,
        render,
    ) in editors.iter_mut()
    {
        let needs_full = full_rebuild.contains(&entity);
        let needs_content = content_only.contains(&entity);
        let needs_syntax = syntax_changed.contains(&entity);
        if !needs_full && !needs_content && !needs_syntax {
            continue;
        }

        let m = rv.metrics();
        let wrap = wrap.copied().unwrap_or_default();
        let visible = visible_buffer_range(
            &**rv.buffer,
            rv.scroll.y,
            m.viewport_height,
            m.text_area_top,
            m.line_height,
            m.char_width,
            wrap,
            hidden,
        );
        if visible.start >= visible.end {
            *line_styles = LineStyles::new(HashMap::new());
            syntax.covered = 0..0;
            dirty_lines.remove(&entity);
            continue;
        }

        let total_lines = rv.buffer.len_lines();

        // Extend the highlight window past the engine's render window so
        // `by_line` stays warm across small scroll deltas — same idea as
        // Zed's syntax-cache margin: render tight, cache wide.
        const HIGHLIGHT_LOOKAHEAD_LINES: usize = 64;
        let range = visible.start.saturating_sub(HIGHLIGHT_LOOKAHEAD_LINES)
            ..visible
                .end
                .saturating_add(HIGHLIGHT_LOOKAHEAD_LINES)
                .min(total_lines);

        let (dirty_range, pending_deltas) = if needs_full {
            (None, Vec::new())
        } else {
            dirty_lines.get(&entity).cloned().unwrap_or_default()
        };

        let highlight_lines: Box<dyn Iterator<Item = usize>> = match dirty_range {
            Some((dirty_start, dirty_end)) => {
                let lo = (dirty_start as usize).max(range.start).min(range.end);
                let hi = (dirty_end as usize + 1).min(range.end);
                Box::new(lo..hi)
            }
            None => Box::new(range.start..range.end),
        };

        let is_incremental = dirty_range.is_some();
        let mut shifted_keys = false;
        let mut by_line: HashMap<u32, Vec<FormattedSpan>> = if !is_incremental {
            HashMap::new()
        } else {
            let mut map = (*line_styles.by_line).clone();
            for delta in &pending_deltas {
                if delta.old_end_position.row != delta.new_end_position.row {
                    shifted_keys = true;
                }
                shift_by_line(&mut map, delta);
            }
            map
        };

        let new_covered = if !is_incremental {
            range.start as u32..range.end as u32
        } else {
            let old = &syntax.covered;
            old.start.min(range.start as u32)..old.end.max(range.end as u32)
        };

        let mut batch: Vec<(usize, String)> = Vec::new();
        for buffer_line in highlight_lines {
            if buffer_line >= total_lines {
                break;
            }
            if let Some(h) = hidden {
                if !h.is_visible(buffer_line) {
                    by_line.remove(&(buffer_line as u32));
                    continue;
                }
            }
            let line_text: String = rv.buffer.line(buffer_line).to_string();
            let capped = if render.stop_rendering_line_after > 0 {
                let cap = render.stop_rendering_line_after as usize;
                if line_text.chars().count() > cap {
                    let mut s = String::with_capacity(cap);
                    for (i, ch) in line_text.chars().enumerate() {
                        if i >= cap {
                            break;
                        }
                        s.push(ch);
                    }
                    if line_text.ends_with('\n') {
                        s.push('\n');
                    }
                    s
                } else {
                    line_text
                }
            } else {
                line_text
            };
            batch.push((buffer_line, capped));
        }

        let mut map_changed = false;

        if let Some(h) = hidden {
            for &(li, _) in &batch {
                if !h.is_visible(li) && by_line.remove(&(li as u32)).is_some() {
                    map_changed = true;
                }
            }
        }

        if !batch.is_empty() {
            let line_inputs: Vec<(usize, &str)> = batch
                .iter()
                .map(|(li, line_text)| (rv.buffer.line_to_byte(*li), line_text.as_str()))
                .collect();

            let _hl_span = bevy::prelude::info_span!("highlight_line").entered();
            let per_line_segs = if let Some(st) = syntax_tree {
                syntax.highlight_lines(
                    &line_inputs,
                    st,
                    rv.buffer.rope(),
                    syntax_theme,
                    theme.foreground,
                )
            } else {
                vec![vec![]; batch.len()]
            };
            for (i, (buffer_line, _)) in batch.iter().enumerate() {
                let segs = per_line_segs.get(i).cloned().unwrap_or_default();
                if segs.iter().all(|s| s.text.trim().is_empty()) {
                    if by_line.remove(&(*buffer_line as u32)).is_some() {
                        map_changed = true;
                    }
                } else {
                    by_line.insert(*buffer_line as u32, segs_to_runs(&segs));
                    map_changed = true;
                }
            }
        }

        let covered_changed = syntax.covered != new_covered;
        if map_changed || covered_changed || shifted_keys || !is_incremental {
            *line_styles = LineStyles::new(by_line);
            syntax.covered = new_covered;
        }
        dirty_lines.remove(&entity);
    }
}

/// Relocate `by_line` keys across an [`EditDelta`]. Keys whose lines were
/// deleted are dropped; keys whose lines moved are re-keyed. Routes every
/// row through [`bevy_instanced_text_editor::shift_line`] so the line-shift
/// semantics stay in one place.
pub(crate) fn shift_by_line<V>(
    map: &mut HashMap<u32, V>,
    delta: &bevy_instanced_text_editor::EditDelta,
) {
    use bevy_instanced_text_editor::{shift_line, LineShift};
    let mut to_remove: Vec<u32> = Vec::new();
    let mut to_insert: Vec<(u32, V)> = Vec::new();
    let keys: Vec<u32> = map.keys().copied().collect();
    for key in keys {
        match shift_line(key, delta) {
            LineShift::Unchanged => {}
            LineShift::Deleted => {
                to_remove.push(key);
            }
            LineShift::Moved(new_key) => {
                if let Some(val) = map.remove(&key) {
                    to_insert.push((new_key, val));
                }
            }
        }
    }
    for k in to_remove {
        map.remove(&k);
    }
    for (k, v) in to_insert {
        map.insert(k, v);
    }
}

/// Refresh `TextBounds` from `Wrapping` + `Indentation`.
pub(crate) fn sync_layout_wrap(
    mut editors: Query<
        (
            &ComputedNode,
            &MonoCellWidth,
            &mut TextBounds,
            &Wrapping,
            &Indentation,
        ),
        With<CodeEditor>,
    >,
) {
    for (computed, mono, mut wrap, wrapping, indentation) in editors.iter_mut() {
        let char_width = mono.px;
        let wrap_enabled = !matches!(wrapping.word_wrap, crate::settings::WordWrapMode::Off);
        let width: Option<f32> = if wrap_enabled {
            let inv = computed.inverse_scale_factor();
            let viewport_text_w = (computed.size().x * inv
                - computed.content_inset().min_inset.x * inv)
                .max(char_width);
            let budget = match wrapping.word_wrap {
                crate::settings::WordWrapMode::WordWrapColumn
                | crate::settings::WordWrapMode::Bounded => {
                    (wrapping.word_wrap_column as f32) * char_width
                }
                _ => viewport_text_w,
            };
            Some(budget.max(char_width))
        } else {
            None
        };
        let indent_px = if wrap_enabled
            && !matches!(
                wrapping.wrapping_indent,
                crate::settings::WrappingIndent::None
            ) {
            indentation.tab_size as f32 * char_width
        } else {
            0.0
        };
        let next = TextBounds { width, indent_px };
        if wrap.width != next.width || wrap.indent_px != next.indent_px {
            *wrap = next;
        }
    }
}

#[cfg(test)]
mod shift_tests {
    use super::shift_by_line;
    use bevy_instanced_text_editor::{EditDelta, EditPoint};
    use std::collections::HashMap;

    fn map_from(pairs: &[(u32, &'static str)]) -> HashMap<u32, &'static str> {
        pairs.iter().copied().collect()
    }

    fn delta(start_row: u32, old_end_row: u32, new_end_row: u32) -> EditDelta {
        EditDelta {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 0,
            start_position: EditPoint {
                row: start_row,
                column_byte: 0,
            },
            old_end_position: EditPoint {
                row: old_end_row,
                column_byte: 0,
            },
            new_end_position: EditPoint {
                row: new_end_row,
                column_byte: 0,
            },
        }
    }

    /// Backspace at the start of row 2 joins rows 1 and 2. Row 2 vanishes;
    /// row 3 slides down to become row 2. Row 0 must remain untouched.
    #[test]
    fn delete_newline_at_start_of_row_2_does_not_clobber_row_0() {
        let mut map = map_from(&[(0, "line0"), (1, "line1"), (2, "line2")]);
        shift_by_line(&mut map, &delta(1, 2, 1));
        assert_eq!(
            map.get(&0).copied(),
            Some("line0"),
            "row 0 must be preserved"
        );
        assert!(!map.contains_key(&2), "row 2 must be dropped after merge");
    }

    /// Same shape, but deeper in the buffer.
    #[test]
    fn delete_newline_far_from_start_only_shifts_trailing_rows() {
        let mut map = map_from(&[(0, "a"), (1, "b"), (2, "c"), (3, "d"), (4, "e")]);
        shift_by_line(&mut map, &delta(2, 3, 2));
        assert_eq!(map.get(&0).copied(), Some("a"));
        assert_eq!(map.get(&1).copied(), Some("b"));
        assert!(!map.contains_key(&4));
        assert_eq!(map.get(&3).copied(), Some("e"));
    }

    /// Deleting 2 consecutive newlines (e.g. selecting `\n\n` and pressing
    /// backspace) drops two rows.
    #[test]
    fn multi_line_delete_drops_correct_rows() {
        let mut map = map_from(&[(0, "a"), (1, "b"), (2, "c"), (3, "d"), (4, "e")]);
        shift_by_line(&mut map, &delta(1, 3, 1));
        assert_eq!(map.get(&0).copied(), Some("a"));
        assert!(!map.contains_key(&3));
        assert!(!map.contains_key(&4));
        assert_eq!(map.get(&2).copied(), Some("e"));
    }

    /// Enter at end of row 0 splits it into rows 0 and 1; existing rows shift up.
    #[test]
    fn insert_newline_shifts_trailing_rows_up() {
        let mut map = map_from(&[(0, "first"), (1, "second"), (2, "third")]);
        shift_by_line(&mut map, &delta(0, 0, 1));
        assert_eq!(map.get(&0).copied(), Some("first"));
        assert_eq!(map.get(&2).copied(), Some("second"));
        assert_eq!(map.get(&3).copied(), Some("third"));
    }

    /// Same-row edit (typing a character) is a no-op for line keys.
    #[test]
    fn same_row_edit_is_noop() {
        let original = map_from(&[(0, "a"), (1, "b"), (2, "c")]);
        let mut map = original.clone();
        shift_by_line(&mut map, &delta(1, 1, 1));
        assert_eq!(map, original);
    }
}
