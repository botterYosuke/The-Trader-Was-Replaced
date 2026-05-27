//! Code folding
//!
//! Fold-region detection runs as an async task on `AsyncComputeTaskPool`
//! when the `tree-sitter` feature is enabled. Without tree-sitter, `FoldState`
//! stays on the entity but `regions` remains empty — no folding is detected.
//!
//! No gutter chevron renderer is included. Downstream consumers wanting
//! `▶`/`▼` indicators read `FoldState` + `ScrollState` + `ComputedNode`
//! and emit whatever entity they prefer (Sprite, Text2d, or — best —
//! `RectOverlay`s into `TextViewOverlays` so they go through the engine's
//! GPU instanced batch). Click-to-toggle stays wired up via `on_gutter_click`
//! in `input/mouse.rs` regardless of whether anything is rendered there.

use crate::text_view::TextBuffer;
use crate::types::*;
use bevy::prelude::*;
use bevy_instanced_text_editor::{shift_line, LineShift, RopeBuffer};

use bevy::tasks::{block_on, futures_lite, AsyncComputeTaskPool, Task};

/// In-flight fold-detection task. Lives on a child entity so the parent's
/// `Changed<FoldState>` doesn't fire on each task spawn/despawn. Mirrors
/// the `bevy_tree_sitter::ParseTask` pattern.
#[derive(Component)]
pub(crate) struct FoldDetectTask {
    task: Task<Vec<FoldRegion>>,
    /// `SyntaxTree::tree_version` at kick-off; written into
    /// `FoldState::content_version` on completion to single-flight.
    tree_version: usize,
    /// Rope `content_version` at kick-off. Region line indices were computed
    /// against the rope at this version; if the rope has since been edited
    /// the task is stale and must be discarded — `shift_fold_regions_on_edit`
    /// has already corrected the prior region indices.
    rope_version: u64,
    /// The editor entity whose `FoldState` this task targets.
    target: Entity,
}

/// Spawn an async fold-detection task whenever an editor's `SyntaxTree`
/// produces a fresher version than the one already reflected in
/// `FoldState`. The walk happens on `AsyncComputeTaskPool`; the apply
/// step ([`apply_fold_detect_tasks`]) writes the result back on the
/// main thread, preserving prior `is_folded` flags.
///
/// Single-flight per editor: while a task is in flight for `entity`,
/// `Changed<SyntaxTree>` cycles on that entity are ignored until the
/// in-flight task lands. The check we then run in `apply` (`tree_version`
/// equality) catches the case where another tree version arrived during
/// the walk and re-spawns naturally on the next tick.
type FoldDetectQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FoldState,
        &'static TextBuffer<RopeBuffer>,
        &'static bevy_tree_sitter::SyntaxTree,
        &'static bevy_tree_sitter::ParseSourceComp,
        &'static crate::settings::Folding,
    ),
    (With<CodeEditor>, Changed<bevy_tree_sitter::SyntaxTree>),
>;

pub(crate) fn spawn_fold_detect_tasks(
    mut commands: Commands,
    editor_query: FoldDetectQuery,
    in_flight: Query<&FoldDetectTask>,
) {
    let busy: std::collections::HashSet<Entity> = in_flight.iter().map(|t| t.target).collect();

    for (entity, fold_state, buffer, syntax_tree, parse_source, folding) in editor_query.iter() {
        if !folding.enabled {
            continue;
        }
        if busy.contains(&entity) {
            continue;
        }
        let Some(tree) = syntax_tree.tree.as_ref() else {
            continue;
        };
        let tree_version = syntax_tree.tree_version as usize;
        if fold_state.content_version == tree_version {
            continue;
        }

        // Cheap clones: `Tree` is ref-counted FFI-side, `Rope` shares
        // chunks via Arc. Both are `Send + 'static`, suitable for the
        // worker.
        let tree_clone = tree.clone();
        let rope_clone = buffer.rope().clone();
        let rope_version = parse_source.0.content_version();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let mut regions: Vec<FoldRegion> = Vec::new();
            let root = tree_clone.root_node();
            collect_foldable_regions(&root, &rope_clone, &mut regions, false);
            regions
        });

        commands.spawn((
            FoldDetectTask {
                task,
                tree_version,
                rope_version,
                target: entity,
            },
            ChildOf(entity),
        ));
    }
}

/// Poll in-flight `FoldDetectTask`s; merge completed results into the
/// target editor's `FoldState` (preserving prior `is_folded` flags) and
/// despawn the task entity.
pub(crate) fn apply_fold_detect_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut FoldDetectTask)>,
    mut editors: Query<
        (
            &mut FoldState,
            &crate::settings::Folding,
            &bevy_tree_sitter::ParseSourceComp,
        ),
        With<CodeEditor>,
    >,
) {
    for (task_entity, mut task) in tasks.iter_mut() {
        let Some(regions) = block_on(futures_lite::future::poll_once(&mut task.task)) else {
            continue;
        };

        if let Ok((mut fold_state, folding_cfg, parse_source)) = editors.get_mut(task.target) {
            // Discard tasks whose rope has been edited since the task
            // captured its snapshot. Region line indices reflect that
            // snapshot; applying them now would overwrite the post-edit
            // indices `shift_fold_regions_on_edit` produced with stale ones.
            // The next tick's `spawn_fold_detect_tasks` will queue a fresh
            // detect against the current rope.
            if task.rope_version != parse_source.0.content_version() {
                commands.entity(task_entity).despawn();
                continue;
            }
            if fold_state.content_version != task.tree_version {
                let first_apply = fold_state.content_version == 0;
                let limit = folding_cfg.max_regions as usize;
                let prior: std::collections::HashMap<(usize, usize), bool> = fold_state
                    .regions
                    .iter()
                    .map(|r| ((r.start_line, r.end_line), r.is_folded))
                    .collect();

                let mut new_regions: Vec<FoldRegion> = Vec::with_capacity(regions.len());
                for mut region in regions {
                    if let Some(&was_folded) = prior.get(&(region.start_line, region.end_line)) {
                        region.is_folded = was_folded;
                    } else if first_apply
                        && folding_cfg.imports_by_default
                        && region.kind == FoldKind::Imports
                    {
                        region.is_folded = true;
                    }
                    new_regions.push(region);
                }
                if limit > 0 && new_regions.len() > limit {
                    new_regions.truncate(limit);
                }

                let unchanged = new_regions.len() == fold_state.regions.len()
                    && new_regions
                        .iter()
                        .zip(fold_state.regions.iter())
                        .all(|(a, b)| a == b);

                if unchanged {
                    fold_state.bypass_change_detection().content_version = task.tree_version;
                } else {
                    fold_state.regions = new_regions;
                    fold_state.content_version = task.tree_version;
                }
            }
        }

        commands.entity(task_entity).despawn();
    }
}

pub(crate) fn collect_foldable_regions(
    node: &bevy_tree_sitter::ts::Node,
    rope: &ropey::Rope,
    regions: &mut Vec<FoldRegion>,
    parent_is_foldable_construct: bool,
) {
    let kind = node.kind();

    let is_foldable_construct = matches!(
        kind,
        // Function-like constructs
        "function_item" | "function_definition" | "function_declaration" |
        "method_definition" | "method_declaration" | "function_expression" |
        "arrow_function" | "lambda" | "closure_expression" |
        // Class-like constructs
        "class_definition" | "class_declaration" | "struct_item" |
        "enum_item" | "interface_declaration" | "trait_item" | "impl_item"
    );

    // Skip block/body direct children of foldable constructs to avoid duplicates.
    let skip_this_node = parent_is_foldable_construct
        && matches!(
            kind,
            "block"
                | "compound_statement"
                | "statement_block"
                | "body"
                | "field_declaration_list"
                | "declaration_list"
                | "enum_variant_list"
        );

    if !skip_this_node {
        if let Some(region) = node_to_fold_region(node, rope) {
            if region.end_line > region.start_line {
                regions.push(region);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_foldable_regions(&child, rope, regions, is_foldable_construct);
    }
}

/// Start-of-fold row for a foldable item, skipping leading attribute,
/// annotation, decorator, and doc-comment children. Many tree-sitter
/// grammars attach those as the *first* children of an item, which makes
/// `node.start_position().row` point at the decoration rather than the
/// keyword line a user expects the fold chevron on. Skipping past those
/// children lets the chevron land on the same row LSP-driven editors
/// (which sanitize fold ranges in the server) place it.
fn fold_start_row_skipping_attributes(node: &bevy_tree_sitter::ts::Node) -> usize {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind = child.kind();
        let is_decoration = kind.starts_with("attribute")
            || kind.starts_with("annotation")
            || kind.starts_with("decorator")
            || kind == "attributes"
            || kind.ends_with("comment");
        if is_decoration {
            continue;
        }
        return child.start_position().row;
    }
    node.start_position().row
}

pub(crate) fn node_to_fold_region(
    node: &bevy_tree_sitter::ts::Node,
    rope: &ropey::Rope,
) -> Option<FoldRegion> {
    let kind = node.kind();

    let fold_kind = match kind {
        "function_item"
        | "function_definition"
        | "function_declaration"
        | "method_definition"
        | "method_declaration"
        | "function_expression"
        | "arrow_function"
        | "lambda"
        | "closure_expression" => Some(FoldKind::Function),

        "class_definition"
        | "class_declaration"
        | "struct_item"
        | "enum_item"
        | "interface_declaration"
        | "trait_item"
        | "impl_item" => Some(FoldKind::Class),

        "block" | "compound_statement" | "statement_block" | "if_expression" | "if_statement"
        | "match_expression" | "switch_statement" | "for_statement" | "for_expression"
        | "while_statement" | "while_expression" | "loop_expression" | "try_statement"
        | "catch_clause" | "finally_clause" => Some(FoldKind::Block),

        "use_declaration" | "import_statement" | "import_declaration" => Some(FoldKind::Imports),

        "comment" | "block_comment" | "line_comment" | "doc_comment" => Some(FoldKind::Comment),

        "string_literal" | "raw_string_literal" | "template_string" => Some(FoldKind::Literal),

        "region" | "preproc_region" => Some(FoldKind::Region),

        "array" | "array_expression" | "object" | "object_expression" | "struct_expression"
        | "tuple_expression" => Some(FoldKind::Other),

        _ => None,
    };

    fold_kind.and_then(|kind| {
        let start_line = fold_start_row_skipping_attributes(node);
        let end_line = node.end_position().row;

        let line_count = rope.len_lines();
        if start_line >= line_count || end_line >= line_count {
            return None;
        }

        let _line_start = rope.line_to_char(start_line);
        let line = rope.line(start_line);
        let mut indent_level = 0;
        for c in line.chars() {
            match c {
                ' ' => indent_level += 1,
                '\t' => indent_level += 4,
                _ => break,
            }
        }
        indent_level /= 4;

        Some(FoldRegion {
            start_line,
            end_line,
            is_folded: false,
            kind,
            indent_level,
        })
    })
}

/// Apply each `TextEdited` event's row delta to `FoldState.regions` so the
/// regions track buffer-line indices through edits. Without this, an edit
/// that inserts or removes newlines leaves region indices pointing at lines
/// that have shifted, and any folded region hides the wrong rows until the
/// async fold-detect pass lands a fresh region list.
pub(crate) fn shift_fold_regions_on_edit(
    mut events: bevy::ecs::message::MessageReader<crate::types::events::TextEdited>,
    mut editors: Query<&mut FoldState, With<CodeEditor>>,
) {
    let deltas: Vec<bevy_instanced_text_editor::EditDelta> =
        events.read().map(|e| e.delta).collect();
    if deltas.is_empty() {
        return;
    }
    for mut fold_state in editors.iter_mut() {
        if fold_state.regions.is_empty() {
            continue;
        }
        let mut touched = false;
        let mut drop_indices: Vec<usize> = Vec::new();
        for delta in &deltas {
            let start_row = delta.start_position.row as usize;
            for (i, region) in fold_state
                .bypass_change_detection()
                .regions
                .iter_mut()
                .enumerate()
            {
                let start_shift = shift_line(region.start_line as u32, delta);
                let end_shift = shift_line(region.end_line as u32, delta);
                match (start_shift, end_shift) {
                    (LineShift::Unchanged, LineShift::Unchanged) => {}
                    (LineShift::Deleted, _) | (_, LineShift::Deleted)
                        if region.end_line == region.start_line =>
                    {
                        drop_indices.push(i);
                        touched = true;
                    }
                    (LineShift::Deleted, _) => {
                        // Start row gone; collapse the region to the edit's
                        // start row (which is the last surviving line before
                        // the deletion) and let the end follow.
                        region.start_line = start_row;
                        region.end_line = match end_shift {
                            LineShift::Moved(r) => (r as usize).max(start_row),
                            LineShift::Deleted => start_row,
                            LineShift::Unchanged => region.end_line,
                        };
                        touched = true;
                    }
                    (_, LineShift::Deleted) => {
                        // End row gone; clamp to the edit's start row.
                        if let LineShift::Moved(r) = start_shift {
                            region.start_line = r as usize;
                        }
                        region.end_line = start_row.max(region.start_line);
                        touched = true;
                    }
                    (start, end) => {
                        if let LineShift::Moved(r) = start {
                            region.start_line = r as usize;
                            touched = true;
                        }
                        if let LineShift::Moved(r) = end {
                            region.end_line = (r as usize).max(region.start_line);
                            touched = true;
                        }
                    }
                }
            }
            if !drop_indices.is_empty() {
                drop_indices.sort_unstable_by(|a, b| b.cmp(a));
                for i in drop_indices.drain(..) {
                    fold_state.bypass_change_detection().regions.remove(i);
                }
            }
        }
        if touched {
            fold_state.set_changed();
        }
    }
}

pub struct FoldingPlugin;

impl Plugin for FoldingPlugin {
    fn build(&self, _app: &mut App) {
        _app.register_type::<crate::types::fold::GotoLineState>()
            .register_type::<crate::types::fold::FoldState>();
        _app.register_type::<crate::types::fold::FoldKind>()
            .register_type::<crate::types::fold::FoldRegion>();

        _app.add_systems(
            Update,
            (
                shift_fold_regions_on_edit.in_set(super::ApplyStateSet),
                spawn_fold_detect_tasks
                    .after(shift_fold_regions_on_edit)
                    .in_set(super::ApplyStateSet),
                apply_fold_detect_tasks
                    .after(spawn_fold_detect_tasks)
                    .in_set(super::ApplyStateSet),
            ),
        );
    }
}
