//! Tests for the editor's syntax → layout → render pipeline.
//!
//! The tests bisect the pipeline into three layers and walk them in order:
//! 1. `LineStyles.by_line[row]` — what tree-sitter said.
//! 2. `DisplayLayout.lines[i].runs[j].fg` — what `produce_layouts` baked.
//! 3. `GlyphBatchComponent.instances[k].color` — what `update_text_views`
//!    sent to the GPU.
//!
//! When a test fails, the failure message identifies *which* layer dropped
//! or corrupted the color, not just "colors are wrong."

#![cfg(test)]
// `ComputedNode` has no constructor that accepts size/scale — tests must
// build it via `default()` then assign fields.
#![allow(clippy::field_reassign_with_default)]

use super::plugin::DisplayMapPlugin;
use crate::plugin::syntax_highlighting::SyntaxPlugin;
use crate::settings::{
    BracketConfig, EditorTheme, EditorUi, Indentation, Performance, SyntaxColors, Wrapping,
};
use crate::types::events::TextEdited;
use crate::types::{BracketMatchState, CodeEditor, CursorState, FoldState, SelectionState};
use bevy::asset::{AssetId, Assets, Handle};
use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use bevy::image::Image;
use bevy::math::{Affine2, Vec2};
use bevy::prelude::*;
use bevy::text::{Font, DEFAULT_FONT_DATA};
use bevy::time::TimePlugin;
use bevy::ui::ui_transform::UiGlobalTransform;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::gpu::{GlyphAtlas, GlyphAtlasPlugin};
use bevy_instanced_text::view::measurement::LayoutTuning;
use bevy_instanced_text::view::plugin::update_text_views;
use bevy_instanced_text::view::render::{GlyphBatchComponent, GlyphInstance};
use bevy_instanced_text::view::text_access::produce_layouts;
use bevy_instanced_text::{
    DisplayLayout, LineStyles, MonoCellWidth, TextBounds, TextBuffer, TextOverlays, TextUnderlays,
    TextViewBatchEntity,
};
use bevy_instanced_text_editor::{BlinkPhase, EditDelta, EditPoint, RopeBuffer};
use bevy_tree_sitter::{SyntaxTree, TreeSitterGrammar, TreeSitterPlugin};
use std::time::{Duration, Instant};

/// Build a headless test app with the minimum plugins for syntax / layout.
fn make_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins.build().disable::<TimePlugin>());
    app.add_plugins(TimePlugin);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_resource::<Assets<Font>>();
    app.configure_sets(
        Update,
        (
            crate::plugin::InputSet,
            bevy_instanced_text_editor::EditEmitSet.after(crate::plugin::InputSet),
            crate::plugin::ApplyStateSet.after(bevy_instanced_text_editor::EditEmitSet),
        )
            .chain(),
    );
    app.add_message::<TextEdited>();
    app.add_plugins(TreeSitterPlugin);
    app.add_plugins(SyntaxPlugin);
    app.add_plugins(DisplayMapPlugin);
    app
}

/// Spawn a `CodeEditor` entity carrying every Component the syntax /
/// styling / layout plumbing reads — the same bundle the editor's host
/// app would spawn in production.
fn spawn_test_editor(app: &mut App, text: &str) -> Entity {
    let mut computed = ComputedNode::default();
    computed.size = Vec2::new(800.0, 600.0);
    computed.inverse_scale_factor = 1.0;

    let font_bundle = (
        TextFont::from_font_size(14.0),
        bevy::text::LineHeight::Px(21.0),
        MonoCellWidth { px: 8.0 },
        bevy_instanced_text::MonoFontFaces::default(),
        bevy::text::TextLayout::default(),
    );
    let scroll_bundle = (ScrollPosition::default(),);
    let color_bundle = (
        bevy::text::TextColor::default(),
        bevy::text::TextBackgroundColor::default(),
    );
    let layout_bundle = (
        TextBuffer::<RopeBuffer>::new(RopeBuffer::new(text)),
        bevy_instanced_text::ContentMetrics::default(),
        computed,
        DisplayLayout::default(),
        TextUnderlays::default(),
        TextOverlays::default(),
        TextBounds::default(),
        LayoutTuning::default(),
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(400.0, 300.0))),
    );
    let settings_bundle = (
        EditorTheme::default(),
        SyntaxColors::default(),
        EditorUi::default(),
        Indentation::default(),
        Wrapping::default(),
        Performance::default(),
    );
    let editor_state_bundle = (
        SelectionState::default(),
        CursorState::default(),
        FoldState::default(),
        BracketMatchState::default(),
        BracketConfig::default(),
    );
    let language = TreeSitterGrammar::new(
        bevy_tree_sitter::arborium::lang_rust::language().into(),
        bevy_tree_sitter::arborium::lang_rust::HIGHLIGHTS_QUERY,
    );
    let entity = app
        .world_mut()
        .spawn((CodeEditor, Name::new("TestEditor")))
        .insert(font_bundle)
        .insert(scroll_bundle)
        .insert(color_bundle)
        .insert(layout_bundle)
        .insert(settings_bundle)
        .insert(editor_state_bundle)
        .insert(language)
        .id();
    // Run Startup once so `init_editor_syntax` attaches `EditorSyntaxState` /
    // `ParseSourceComp` / `SyntaxTree` to the entity.
    app.update();
    entity
}

/// Install `GlyphAtlas` + a usable default font into the test app. Needed
/// by every test that drives `produce_layouts` or `update_text_views`.
fn install_atlas_and_font(app: &mut App) {
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<Assets<Font>>();
    let atlas = {
        let mut images = world.resource_mut::<Assets<Image>>();
        GlyphAtlas::new(&mut images)
    };
    world.insert_resource(atlas);
    let font = Font::try_from_bytes(DEFAULT_FONT_DATA.to_vec()).unwrap();
    world
        .resource_mut::<Assets<Font>>()
        .insert(AssetId::default(), font)
        .unwrap();
}

/// Drive `app.update()` until `pred` returns true, or `timeout` elapses.
/// Returns the number of ticks consumed (0 if `pred` was already true).
fn run_until<F>(app: &mut App, entity: Entity, timeout: Duration, mut pred: F) -> usize
where
    F: FnMut(&World, Entity) -> bool,
{
    let start = Instant::now();
    let mut ticks = 0;
    loop {
        if pred(app.world(), entity) {
            return ticks;
        }
        if start.elapsed() > timeout {
            return ticks;
        }
        app.update();
        ticks += 1;
        // Give the AsyncComputeTaskPool a chance to make progress.
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Drive the app until tree-sitter's async parse has populated `LineStyles`
/// for the editor — the precondition for any layer-walking test.
fn await_initial_parse(app: &mut App, entity: Entity) -> usize {
    run_until(app, entity, Duration::from_secs(5), |w, e| {
        let st = w.get::<SyntaxTree>(e).unwrap();
        let ls = w.get::<LineStyles>(e);
        st.tree.is_some() && ls.map(|s| !s.by_line.is_empty()).unwrap_or(false)
    })
}

/// One-shot drive of `produce_layouts` + `update_text_views`, then an
/// `app.update()` to flush the `Commands` that spawn the batch entity.
/// Used after seeding state for tests that don't run a full PostUpdate
/// schedule.
fn drive_layout_and_render_once(app: &mut App) {
    app.world_mut()
        .run_system_once(produce_layouts::<RopeBuffer>)
        .unwrap();
    app.world_mut().run_system_once(update_text_views).unwrap();
    app.update();
}

fn to_linear_rgba(c: bevy::color::Color) -> [f32; 4] {
    let l = c.to_linear();
    [l.red, l.green, l.blue, l.alpha]
}

fn color_eq(a: [f32; 4], b: [f32; 4]) -> bool {
    const EPS: f32 = 0.001;
    (a[0] - b[0]).abs() < EPS
        && (a[1] - b[1]).abs() < EPS
        && (a[2] - b[2]).abs() < EPS
        && (a[3] - b[3]).abs() < EPS
}

/// Group `GlyphInstance`s into y-clusters (rows) and map each cluster to
/// the `DisplayLayout` `display_row` it represents.
///
/// Glyph instances on the same row share a y-coordinate (within sub-pixel
/// jitter). We sort by y descending, walk linearly, and start a new cluster
/// whenever the gap exceeds `line_height / 2`. The K-th cluster (largest y
/// first) maps to the K-th non-empty `display_row` in `DisplayLayout`,
/// because both are emitted in increasing display-row order.
fn cluster_instances_to_display_rows<'a>(
    instances: &'a [GlyphInstance],
    layout: &DisplayLayout,
    line_height: f32,
) -> Vec<(u32, Vec<&'a GlyphInstance>)> {
    // Sort ascending Y so cluster N maps to display row N (+Y down).
    let mut sorted: Vec<&_> = instances.iter().collect();
    sorted.sort_by(|a, b| a.position.y.partial_cmp(&b.position.y).unwrap());

    let gap_threshold = line_height * 0.5;
    let mut clusters: Vec<Vec<&_>> = Vec::new();
    let mut current: Vec<&_> = Vec::new();
    let mut last_y: Option<f32> = None;
    for inst in sorted {
        let new_cluster = match last_y {
            None => false,
            Some(prev) => (prev - inst.position.y).abs() > gap_threshold,
        };
        if new_cluster && !current.is_empty() {
            clusters.push(std::mem::take(&mut current));
        }
        last_y = Some(inst.position.y);
        current.push(inst);
    }
    if !current.is_empty() {
        clusters.push(current);
    }

    let mut non_empty_rows: Vec<u32> = layout
        .lines
        .iter()
        .filter(|l| !l.runs.is_empty() && !l.is_wrap_continuation)
        .map(|l| l.display_row)
        .collect();
    non_empty_rows.sort();

    clusters
        .into_iter()
        .zip(non_empty_rows)
        .map(|(c, r)| (r, c))
        .collect()
}

/// Diagnostic dump for failing layer assertions. Prints the full
/// `DisplayLayout` and the top-N glyph instances sorted by y.
fn dump_layout_and_batch(layout: &DisplayLayout, batch: &GlyphBatchComponent, kw_linear: [f32; 4]) {
    eprintln!(
        "\n=== DIAGNOSTIC DUMP ===\n\
         line_height = {}\n\
         keyword color (linear): {:?}\n\
         \n\
         DisplayLayout has {} lines:",
        layout.line_height,
        kw_linear,
        layout.lines.len(),
    );
    for l in layout.lines.iter() {
        eprintln!(
            "  display_row={} buffer_row={} wrap_cont={} y_top={} text={:?}\n    runs: {:?}",
            l.display_row,
            l.buffer_row,
            l.is_wrap_continuation,
            l.y_top,
            l.text,
            l.runs
                .iter()
                .map(|r| (r.byte_range.clone(), r.fg))
                .collect::<Vec<_>>(),
        );
    }
    let mut sorted: Vec<_> = batch.instances.iter().collect();
    sorted.sort_by(|a, b| a.position.y.partial_cmp(&b.position.y).unwrap());
    eprintln!(
        "\nGlyphBatch has {} instances (sorted by y asc, top 40):",
        batch.instances.len(),
    );
    for inst in sorted.iter().take(40) {
        let is_kw = color_eq(inst.color, kw_linear);
        eprintln!(
            "  y={:8.2}  x={:8.2}  color={:?}  {}",
            inst.position.y,
            inst.position.x,
            inst.color,
            if is_kw { "<-- KEYWORD COLOR" } else { "" }
        );
    }
    eprintln!("=== END DIAGNOSTIC DUMP ===\n");
}

/// Walk all three pipeline layers and assert the keyword color flows
/// correctly. Caller specifies which `buffer_row` should contain the `fn`
/// run (Layer 1's text == `"fn"`). Used by the initial + post-edit tests.
fn assert_pipeline_consistent_for_keyword(
    line_styles: &LineStyles,
    display_layout: &DisplayLayout,
    batch: &GlyphBatchComponent,
    expected_fn_buffer_row: u32,
    label: &str,
) {
    let default_fg = EditorTheme::default().foreground;

    // ── Layer 1: LineStyles has an `fn` run on the expected buffer row,
    //   with a non-default fg color (the keyword color).
    let row_styled = line_styles
        .by_line
        .get(&expected_fn_buffer_row)
        .cloned()
        .unwrap_or_default();
    let kw_styled = row_styled
        .iter()
        .find(|r| r.text == "fn")
        .unwrap_or_else(|| {
            panic!(
                "{}: LAYER 1 (LineStyles): buffer_row={} has no run with text='fn'. \
                 tree-sitter didn't classify the keyword. Runs: {:?}",
                label,
                expected_fn_buffer_row,
                row_styled
                    .iter()
                    .map(|r| (r.text.clone(), r.format.fg))
                    .collect::<Vec<_>>()
            );
        });
    let kw_color = kw_styled.format.fg;
    assert_ne!(
        kw_color, default_fg,
        "{}: LAYER 1: `fn` on buffer_row={} is the default foreground color — \
         tree-sitter didn't apply keyword styling",
        label, expected_fn_buffer_row,
    );

    // ── Layer 2: DisplayLayout has a run covering bytes 0..2 on the same
    //   buffer row with the same color.
    let line = display_layout
        .lines
        .iter()
        .find(|l| l.buffer_row == expected_fn_buffer_row && !l.is_wrap_continuation)
        .unwrap_or_else(|| {
            panic!(
                "{}: LAYER 2: no DisplayLayout line with buffer_row={}. Got: {:?}",
                label,
                expected_fn_buffer_row,
                display_layout
                    .lines
                    .iter()
                    .map(|l| (l.buffer_row, l.display_row, l.text.clone()))
                    .collect::<Vec<_>>()
            );
        });
    let kw_run = line
        .runs
        .iter()
        .find(|r| r.byte_range.start == 0 && r.byte_range.end == 2)
        .unwrap_or_else(|| {
            panic!(
                "{}: LAYER 2: line for buffer_row={} has no run covering bytes 0..2. \
                 Runs: {:?}",
                label,
                expected_fn_buffer_row,
                line.runs
                    .iter()
                    .map(|r| (r.byte_range.clone(), r.fg))
                    .collect::<Vec<_>>()
            );
        });
    assert_eq!(
        kw_run.fg, kw_color,
        "{}: LAYER 1→2 DIVERGENCE: LineStyles said `fn` is {:?}, but \
         DisplayLayout has it as {:?}",
        label, kw_color, kw_run.fg,
    );

    // ── Layer 3: GlyphBatch has the keyword color on the display_row that
    //   DisplayLayout placed `fn` on, AND no stale keyword color on any
    //   display_row DisplayLayout says is empty.
    let kw_linear = to_linear_rgba(kw_color);
    let cluster_rows = cluster_instances_to_display_rows(
        &batch.instances,
        display_layout,
        display_layout.line_height,
    );
    let kw_rows: std::collections::HashSet<u32> = cluster_rows
        .iter()
        .filter(|(_, is)| is.iter().any(|i| color_eq(i.color, kw_linear)))
        .map(|(r, _)| *r)
        .collect();
    let expected_fn_display_row = line.display_row;
    let empty_rows: std::collections::HashSet<u32> = display_layout
        .lines
        .iter()
        .filter(|l| l.runs.is_empty() && !l.is_wrap_continuation)
        .map(|l| l.display_row)
        .collect();
    let stale_rows: Vec<u32> = kw_rows
        .iter()
        .filter(|r| empty_rows.contains(r))
        .copied()
        .collect();

    if !kw_rows.contains(&expected_fn_display_row) || !stale_rows.is_empty() {
        dump_layout_and_batch(display_layout, batch, kw_linear);
        eprintln!(
            "Cluster→display_row mapping: {:?}\nkw_rows: {:?}\n\
             expected_fn_display_row: {}\nempty_rows: {:?}\nstale_rows: {:?}",
            cluster_rows
                .iter()
                .map(|(r, i)| (*r, i.len()))
                .collect::<Vec<_>>(),
            kw_rows,
            expected_fn_display_row,
            empty_rows,
            stale_rows,
        );
    }
    assert!(
        kw_rows.contains(&expected_fn_display_row),
        "{}: LAYER 3: GlyphBatch has no keyword-colored instance on \
         display_row={} (where DisplayLayout placed `fn`). Keyword color \
         found on rows: {:?}.",
        label,
        expected_fn_display_row,
        kw_rows,
    );
    assert!(
        stale_rows.is_empty(),
        "{}: LAYER 3 STALE: keyword color {:?} appears on display row(s) \
         {:?} which DisplayLayout says are EMPTY. Stale-glyph bug.",
        label,
        kw_color,
        stale_rows,
    );
}

/// Smoke check: after Startup, the editor entity should have both
/// `EditorSyntaxState` (with a usable provider) and `SyntaxTree`.
#[test]
fn editor_initializes_with_syntax_provider() {
    use crate::plugin::syntax_highlighting::EditorSyntaxState;

    let mut app = make_test_app();
    let entity = spawn_test_editor(&mut app, "fn main() {}\n");

    let world = app.world();
    let state = world
        .get::<EditorSyntaxState>(entity)
        .expect("EditorSyntaxState attached after Startup");
    assert!(
        state.is_available(),
        "provider must be installed (TreeSitterGrammar was attached at spawn)"
    );
    assert!(
        world.get::<SyntaxTree>(entity).is_some(),
        "SyntaxTree attached after Startup",
    );
}

/// Subsystem check: `ComputedNode` physical-pixel values convert correctly to logical pixels.
/// The pipeline reads `ComputedNode` directly (no intermediate cache); this test verifies
/// the logical-pixel arithmetic that all layout/render systems rely on.
#[test]
fn computed_node_logical_pixel_conversion() {
    // 1600x1200 physical at 2x DPI → 800x600 logical.
    // padding.left=100px physical (50 logical), padding.top=20px physical (10 logical).
    let mut computed = ComputedNode::default();
    computed.size = Vec2::new(1600.0, 1200.0);
    computed.inverse_scale_factor = 0.5;
    computed.padding.min_inset = Vec2::new(100.0, 20.0);

    let inv = computed.inverse_scale_factor();
    assert_eq!((computed.size().x * inv) as u32, 800, "logical width");
    assert_eq!((computed.size().y * inv) as u32, 600, "logical height");
    assert!(
        (computed.content_inset().min_inset.x * inv - 50.0).abs() < 0.1,
        "padding.left logical"
    );
    assert!(
        (computed.content_inset().min_inset.y * inv - 10.0).abs() < 0.1,
        "padding.top logical"
    );
}

/// Regression: tree-sitter highlights must be applied to every visible row,
/// not just the first. The batching change in `produce_line_styles` (one
/// tree-sitter query covering the whole visible window instead of one per
/// line) must distribute the returned highlights back to each line.
#[test]
fn many_lines_all_get_highlighted() {
    let mut source = String::new();
    for i in 0..200 {
        source.push_str(&format!("fn f{}() {{ let x = {}; }}\n", i, i));
    }
    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, &source);

    await_initial_parse(&mut app, entity);
    drive_layout_and_render_once(&mut app);

    let world = app.world();
    let line_styles = world.get::<LineStyles>(entity).unwrap();
    let rows_with_fn: Vec<u32> = line_styles
        .by_line
        .iter()
        .filter_map(|(row, runs)| runs.iter().any(|r| r.text == "fn").then_some(*row))
        .collect();
    assert!(
        rows_with_fn.len() > 1,
        "expected `fn` keyword highlight on many rows, got rows: {:?}. \
         LineStyles.by_line has {} entries.",
        rows_with_fn,
        line_styles.by_line.len()
    );
}

/// Walks all three layers (LineStyles → DisplayLayout → GlyphBatch) for a
/// static buffer. First divergence is reported by layer in the failure
/// message, so this localizes any pipeline-color regression.
#[test]
fn pipeline_consistency_initial() {
    let source = "fn main() {\n    let x = 42;\n}\n";
    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, source);

    await_initial_parse(&mut app, entity);
    drive_layout_and_render_once(&mut app);

    let world = app.world();
    let line_styles = world.get::<LineStyles>(entity).unwrap().clone();
    let display_layout = world.get::<DisplayLayout>(entity).unwrap().clone();
    let batch_entity = world
        .get::<TextViewBatchEntity>(entity)
        .expect("update_text_views must spawn a batch entity")
        .0;
    let batch = world
        .get::<GlyphBatchComponent>(batch_entity)
        .unwrap()
        .clone();

    assert_pipeline_consistent_for_keyword(
        &line_styles,
        &display_layout,
        &batch,
        /*expected_fn_buffer_row=*/ 0,
        "initial",
    );
}

/// Inserts a `\n` at byte 0, shifting `fn` from buffer_row=0 to
/// buffer_row=1. Verifies the entire pipeline correctly reflects the new
/// row layout — including row 2 (`    let x = 42;`) shifting down from
/// pre-edit row 1. Regression check for the `LineStyles.by_line` index-
/// shift bug (now fixed in `record_edits_for_incremental_parsing` by
/// forcing a full rebuild on line-count-changing edits).
#[test]
fn pipeline_consistency_after_newline_insert() {
    let source = "fn main() {\n    let x = 42;\n}\n";
    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, source);

    await_initial_parse(&mut app, entity);
    drive_layout_and_render_once(&mut app);

    // EDIT: insert `\n` at byte 0 → `fn` moves from buffer_row=0 to row 1.
    let new_source = "\nfn main() {\n    let x = 42;\n}\n";
    {
        let mut buf = app
            .world_mut()
            .get_mut::<TextBuffer<RopeBuffer>>(entity)
            .unwrap();
        buf.0 = RopeBuffer(ropey::Rope::from_str(new_source));
    }
    app.world_mut()
        .resource_mut::<Messages<TextEdited>>()
        .write(TextEdited {
            delta: EditDelta {
                start_byte: 0,
                old_end_byte: 0,
                new_end_byte: 1,
                start_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                old_end_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                new_end_position: EditPoint {
                    row: 1,
                    column_byte: 0,
                },
            },
            content_version: 2,
            pre_edit_rope: None,
        });

    // Wait for `fn` to appear on row 1 in LineStyles (async reparse).
    run_until(&mut app, entity, Duration::from_secs(5), |w, e| {
        w.get::<LineStyles>(e)
            .and_then(|s| s.by_line.get(&1u32).cloned())
            .map(|runs| runs.iter().any(|r| r.text == "fn"))
            .unwrap_or(false)
    });
    drive_layout_and_render_once(&mut app);

    let world = app.world();
    let line_styles = world.get::<LineStyles>(entity).unwrap().clone();
    let display_layout = world.get::<DisplayLayout>(entity).unwrap().clone();
    let batch_entity = world.get::<TextViewBatchEntity>(entity).unwrap().0;
    let batch = world
        .get::<GlyphBatchComponent>(batch_entity)
        .unwrap()
        .clone();

    // `fn` moved to buffer_row=1.
    assert_pipeline_consistent_for_keyword(
        &line_styles,
        &display_layout,
        &batch,
        /*expected_fn_buffer_row=*/ 1,
        "after newline insert",
    );

    // Index-shift regression: row 2 must hold the shifted-down `let` line,
    // not stale pre-edit row 2 content (`}`).
    let row2_styled = line_styles.by_line.get(&2u32).cloned().unwrap_or_default();
    assert!(
        row2_styled.iter().any(|r| r.text == "let"),
        "INDEX-SHIFT REGRESSION: row 2 should hold `    let x = 42;` \
         (shifted down from pre-edit row 1). Got: {:?}",
        row2_styled
            .iter()
            .map(|r| (r.text.clone(), r.format.fg))
            .collect::<Vec<_>>()
    );
}

/// Deletes the leading `\n`, shifting `fn` from buffer_row=1 to row=0.
/// This is the regression test for the `LineStyles.by_line` index-shift
/// bug — before the fix, post-edit row 1 still held pre-edit row 1's `fn`
/// runs even though buffer row 1 now contained `    let x = 42;`.
#[test]
fn pipeline_consistency_after_backspace_join() {
    let source = "\nfn main() {\n    let x = 42;\n}\n";
    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, source);

    // Wait for the initial parse to place `fn` on row 1.
    run_until(&mut app, entity, Duration::from_secs(5), |w, e| {
        w.get::<LineStyles>(e)
            .and_then(|s| s.by_line.get(&1u32).cloned())
            .map(|runs| runs.iter().any(|r| r.text == "fn"))
            .unwrap_or(false)
    });
    drive_layout_and_render_once(&mut app);

    // EDIT: delete the leading `\n` (backspace-join row 1 into row 0).
    let new_source = "fn main() {\n    let x = 42;\n}\n";
    {
        let mut buf = app
            .world_mut()
            .get_mut::<TextBuffer<RopeBuffer>>(entity)
            .unwrap();
        buf.0 = RopeBuffer(ropey::Rope::from_str(new_source));
    }
    app.world_mut()
        .resource_mut::<Messages<TextEdited>>()
        .write(TextEdited {
            delta: EditDelta {
                start_byte: 0,
                old_end_byte: 1,
                new_end_byte: 0,
                start_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                old_end_position: EditPoint {
                    row: 1,
                    column_byte: 0,
                },
                new_end_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
            },
            content_version: 2,
            pre_edit_rope: None,
        });

    run_until(&mut app, entity, Duration::from_secs(5), |w, e| {
        w.get::<LineStyles>(e)
            .and_then(|s| s.by_line.get(&0u32).cloned())
            .map(|runs| runs.iter().any(|r| r.text == "fn"))
            .unwrap_or(false)
    });
    drive_layout_and_render_once(&mut app);

    let world = app.world();
    let line_styles = world.get::<LineStyles>(entity).unwrap().clone();
    let display_layout = world.get::<DisplayLayout>(entity).unwrap().clone();
    let batch_entity = world.get::<TextViewBatchEntity>(entity).unwrap().0;
    let batch = world
        .get::<GlyphBatchComponent>(batch_entity)
        .unwrap()
        .clone();

    assert_pipeline_consistent_for_keyword(
        &line_styles,
        &display_layout,
        &batch,
        /*expected_fn_buffer_row=*/ 0,
        "after backspace join",
    );

    // The bug-regression check: row 1 must hold the new `let` line, not the
    // stale `fn main() {` runs that were under by_line[1] pre-edit.
    let row1_styled = line_styles.by_line.get(&1u32).cloned().unwrap_or_default();
    assert!(
        !row1_styled.iter().any(|r| r.text == "fn"),
        "INDEX-SHIFT REGRESSION: row 1 still has stale `fn` runs after \
         backspace-join: {:?}",
        row1_styled
            .iter()
            .map(|r| (r.text.clone(), r.format.fg))
            .collect::<Vec<_>>()
    );
}

/// Successive line-deleting backspaces across multiple frames must not leave
/// any lines uncolored. This is the regression test for the bug where
/// `produce_line_styles` received a bounded `Some(row_range)` from the
/// `TextEdited` event handler even when the edit changed the line count,
/// causing a partial incremental rebuild that left shifted rows stale.
///
/// Three backspace-joins, each in its own frame:
///   frame 1: delete `\n` at end of line 0 → 3 lines
///   frame 2: delete `\n` at end of line 0 → 2 lines
///   frame 3: delete `\n` at end of line 0 → 1 line (`fn` still on row 0)
///
/// After each edit, every visible styled line must still carry syntax color.
#[test]
fn pipeline_consistency_after_repeated_line_deletion() {
    // Start with 8 lines; we'll backspace 7 newlines one per frame.
    let source = "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n}\n";
    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, source);

    // Wait for initial parse to color `fn` on row 0.
    run_until(&mut app, entity, Duration::from_secs(5), |w, e| {
        w.get::<LineStyles>(e)
            .and_then(|s| s.by_line.get(&0u32).cloned())
            .map(|runs| runs.iter().any(|r| r.text == "fn"))
            .unwrap_or(false)
    });

    // Helper: emit one line-deleting backspace, run one frame (edit applied,
    // parse NOT yet complete), then assert no stale rows exist in LineStyles.
    // This catches the intermediate-frame bug where incremental rebuild leaves
    // old entries under shifted indices.
    let simulate_backspace_join = |app: &mut App,
                                   from_rope: &str,
                                   to_rope: &str,
                                   delete_row: u32,
                                   content_version: u64,
                                   expected_total_lines: usize| {
        {
            let mut buf = app
                .world_mut()
                .get_mut::<TextBuffer<RopeBuffer>>(entity)
                .unwrap();
            buf.0 = RopeBuffer(ropey::Rope::from_str(to_rope));
        }
        let newline_byte = from_rope
            .split('\n')
            .take(delete_row as usize + 1)
            .map(|s| s.len() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let col = from_rope
            .split('\n')
            .nth(delete_row as usize)
            .map(|s| s.len())
            .unwrap_or(0) as u32;
        app.world_mut()
            .resource_mut::<Messages<TextEdited>>()
            .write(TextEdited {
                delta: EditDelta {
                    start_byte: newline_byte,
                    old_end_byte: newline_byte + 1,
                    new_end_byte: newline_byte,
                    start_position: EditPoint {
                        row: delete_row,
                        column_byte: col,
                    },
                    old_end_position: EditPoint {
                        row: delete_row + 1,
                        column_byte: 0,
                    },
                    new_end_position: EditPoint {
                        row: delete_row,
                        column_byte: col,
                    },
                },
                content_version,
                pre_edit_rope: None,
            });

        // One frame: edit events processed, produce_line_styles runs.
        // The async parse is unlikely to be done yet — this is the frame
        // where the bug manifested: incremental rebuild with a stale map.
        app.update();

        let ls = app.world().get::<LineStyles>(entity).unwrap().clone();
        // No row at or beyond expected_total_lines should have styled runs.
        for row in expected_total_lines as u32..expected_total_lines as u32 + 4 {
            let stale = ls.by_line.get(&row).cloned().unwrap_or_default();
            assert!(
                stale.is_empty(),
                "STALE (frame after edit {content_version}): row {row} has runs \
                 after deletion to {expected_total_lines} lines: {:?}",
                stale.iter().map(|r| r.text.clone()).collect::<Vec<_>>()
            );
        }

        // Wait for reparse to complete before the next edit.
        run_until(app, entity, Duration::from_secs(5), |w, e| {
            w.get::<SyntaxTree>(e)
                .map(|st| st.content_version >= content_version)
                .unwrap_or(false)
        });
    };

    // Seven successive line-deleting backspaces, each in its own frame.
    // Source starts as 8 lines; we join them down to 1.
    let edits: &[(&str, &str, u32, u64, usize)] = &[
        // (from, to, delete_row, content_version, expected_line_count_after)
        (
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n}\n",
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n}",
            6, 2, 7,
        ),
        (
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n}",
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;}",
            5, 3, 6,
        ),
        (
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;}",
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;    let e = 5;}",
            4, 4, 5,
        ),
        (
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;    let e = 5;}",
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;    let d = 4;    let e = 5;}",
            3, 5, 4,
        ),
        (
            "fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;    let d = 4;    let e = 5;}",
            "fn main() {\n    let a = 1;\n    let b = 2;    let c = 3;    let d = 4;    let e = 5;}",
            2, 6, 3,
        ),
        (
            "fn main() {\n    let a = 1;\n    let b = 2;    let c = 3;    let d = 4;    let e = 5;}",
            "fn main() {\n    let a = 1;    let b = 2;    let c = 3;    let d = 4;    let e = 5;}",
            1, 7, 2,
        ),
        (
            "fn main() {\n    let a = 1;    let b = 2;    let c = 3;    let d = 4;    let e = 5;}",
            "fn main() {    let a = 1;    let b = 2;    let c = 3;    let d = 4;    let e = 5;}",
            0, 8, 1,
        ),
    ];
    for (from, to, row, ver, total) in edits {
        simulate_backspace_join(&mut app, from, to, *row, *ver, *total);
    }

    drive_layout_and_render_once(&mut app);

    let world = app.world();
    let line_styles = world.get::<LineStyles>(entity).unwrap().clone();

    // After all deletions only row 0 remains. It must still contain `fn` colored.
    let row0 = line_styles.by_line.get(&0u32).cloned().unwrap_or_default();
    assert!(
        row0.iter().any(|r| r.text == "fn"),
        "REGRESSION: row 0 lost `fn` color after repeated line-deleting backspaces. \
         Got runs: {:?}",
        row0.iter()
            .map(|r| (r.text.clone(), r.format.fg))
            .collect::<Vec<_>>()
    );

    // No rows beyond 0 should have any styled content (buffer only has 1 line).
    for row in 1u32..4u32 {
        let stale = line_styles.by_line.get(&row).cloned().unwrap_or_default();
        assert!(
            stale.is_empty(),
            "STALE RUNS: row {} has leftover styled runs after deletion: {:?}",
            row,
            stale
                .iter()
                .map(|r| (r.text.clone(), r.format.fg))
                .collect::<Vec<_>>()
        );
    }
}

/// Drives the full PostUpdate schedule as it runs in production:
/// `produce_layouts` → overlay producers → `update_text_views`. Verifies that bevscode's overlay systems
/// (selection, cursor-line highlight, cursor caret, bracket highlight)
/// don't interfere with the color path in the GPU batch.
///
/// If a future refactor reorders these systems and breaks the color path,
/// this is the test that fires.
#[test]
fn full_postupdate_schedule_with_real_overlays() {
    use bevy::input_focus::InputFocus;

    use crate::plugin::brackets::update_bracket_highlight;
    use crate::plugin::cursor::{push_cursor_overlays, update_cursor_line_highlight};
    use crate::plugin::ui_elements::update_selection_highlight;
    use crate::settings::{CursorLine, CursorSettings};

    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    app.world_mut().init_resource::<InputFocus>();
    app.add_plugins(GlyphAtlasPlugin);
    app.add_systems(
        PostUpdate,
        (
            produce_layouts::<RopeBuffer>.run_if(bevy_instanced_text::gpu::atlas_ready),
            (
                update_selection_highlight,
                update_cursor_line_highlight,
                push_cursor_overlays,
                update_bracket_highlight,
            ),
            update_text_views.run_if(bevy_instanced_text::gpu::atlas_ready),
        )
            .chain(),
    );

    let entity = spawn_test_editor(&mut app, "fn main() {}\n");
    app.world_mut().entity_mut(entity).insert((
        BlinkPhase::default(),
        CursorSettings::default(),
        CursorLine::default(),
    ));

    let mut computed = ComputedNode::default();
    computed.size = Vec2::new(800.0, 600.0);
    computed.inverse_scale_factor = 1.0;
    app.world_mut().entity_mut(entity).insert((
        computed,
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(400.0, 300.0))),
    ));
    app.world_mut().resource_mut::<InputFocus>().set(entity);

    let timeout = Duration::from_secs(5);
    let start = Instant::now();
    let default_fg_linear = to_linear_rgba(EditorTheme::default().foreground);
    let mut last_instances = 0usize;
    loop {
        app.update();
        std::thread::sleep(Duration::from_millis(2));
        if start.elapsed() > timeout {
            break;
        }
        let world = app.world();
        let Some(bh) = world.get::<TextViewBatchEntity>(entity) else {
            continue;
        };
        let Some(batch) = world.get::<GlyphBatchComponent>(bh.0) else {
            continue;
        };
        last_instances = batch.instances.len();
        if !batch.instances.is_empty()
            && batch
                .instances
                .iter()
                .any(|i| !color_eq(i.color, default_fg_linear))
        {
            return;
        }
    }
    panic!(
        "after {:?} the GPU batch had no colored instances (last seen len={}). \
         Overlay producers may be interfering with the color path.",
        timeout, last_instances,
    );
}

/// **Real GPU readback.** Drives Bevy's render pipeline against an
/// off-screen `Image` target via `Screenshot::image`, then asserts that
/// rendered pixels differ sufficiently from the editor background — i.e.,
/// the GPU actually drew text. End-to-end visual verification.
///
/// Requires a working wgpu adapter (Metal on macOS, Vulkan/llvmpipe on
/// Linux CI). Marked `#[ignore]` so CI without GPU doesn't fail it; run
/// manually with `--ignored`.
#[test]
#[ignore = "requires GPU; run with --ignored"]
fn gpu_readback_renders_colored_pixels() {
    use bevy::app::ScheduleRunnerPlugin;
    use bevy::camera::{Camera, Camera2d, RenderTarget};
    use bevy::render::render_resource::{TextureFormat, TextureUsages};
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    use bevy::window::{ExitCondition, WindowPlugin};
    use bevy::winit::WinitPlugin;
    use bevy::DefaultPlugins;
    use std::sync::{Arc, Mutex};

    const W: u32 = 800;
    const H: u32 = 600;

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    );
    app.add_plugins(ScheduleRunnerPlugin::run_once());
    app.add_plugins(bevy_instanced_text::gpu::GlyphAtlasPlugin);
    app.add_plugins(bevy_instanced_text::gpu::InstancedTextRenderPlugin);
    app.add_plugins(bevy_instanced_text::view::plugin::InstancedTextPlugin);
    app.add_plugins(TreeSitterPlugin);
    app.add_plugins(SyntaxPlugin);
    app.add_plugins(DisplayMapPlugin);
    app.configure_sets(
        Update,
        (
            crate::plugin::InputSet,
            bevy_instanced_text_editor::EditEmitSet.after(crate::plugin::InputSet),
            crate::plugin::ApplyStateSet.after(bevy_instanced_text_editor::EditEmitSet),
        )
            .chain(),
    );
    app.add_message::<TextEdited>();

    // `RenderPlugin` initializes the wgpu device asynchronously; wait for
    // plugins to be ready, then `finish + cleanup` installs `RenderDevice`
    // into the main world (which `bevy_pbr::no_automatic_skin_batching`
    // requires).
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    {
        let world = app.world_mut();
        let font = Font::try_from_bytes(DEFAULT_FONT_DATA.to_vec()).unwrap();
        world
            .resource_mut::<Assets<Font>>()
            .insert(AssetId::default(), font)
            .unwrap();
    }
    let target_handle: Handle<Image> = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let mut img = Image::new_target_texture(W, H, TextureFormat::bevy_default(), None);
        img.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        images.add(img)
    };
    app.world_mut().spawn((
        Camera2d,
        Camera::default(),
        RenderTarget::Image(target_handle.clone().into()),
    ));

    let entity = spawn_test_editor(&mut app, "fn main() {}\n");
    let mut computed = ComputedNode::default();
    computed.size = Vec2::new(W as f32, H as f32);
    computed.inverse_scale_factor = 1.0;
    app.world_mut().entity_mut(entity).insert((
        computed,
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(
            W as f32 / 2.0,
            H as f32 / 2.0,
        ))),
    ));

    await_initial_parse(&mut app, entity);
    // A few extra frames so the render world catches up.
    for _ in 0..5 {
        app.update();
        std::thread::sleep(Duration::from_millis(8));
    }

    let captured: Arc<Mutex<Option<Image>>> = Arc::new(Mutex::new(None));
    let sink = captured.clone();
    app.world_mut()
        .spawn(Screenshot::image(target_handle.clone()))
        .observe(move |trigger: On<ScreenshotCaptured>| {
            *sink.lock().unwrap() = Some(trigger.image.clone());
        });

    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    while captured.lock().unwrap().is_none() {
        app.update();
        std::thread::sleep(Duration::from_millis(8));
        if start.elapsed() > timeout {
            panic!("screenshot never landed after {:?}", timeout);
        }
    }
    let img = captured.lock().unwrap().take().unwrap();
    let data = img.data.as_ref().expect("Screenshot image has no data");

    let bg = EditorTheme::default().background;
    let bg_rgba_u8: [u8; 4] = {
        let l = bg.to_linear();
        [
            (l.red * 255.0).clamp(0.0, 255.0) as u8,
            (l.green * 255.0).clamp(0.0, 255.0) as u8,
            (l.blue * 255.0).clamp(0.0, 255.0) as u8,
            (l.alpha * 255.0).clamp(0.0, 255.0) as u8,
        ]
    };
    let mut nonbg = 0usize;
    let mut max_dist = 0i32;
    for px in data.chunks_exact(4) {
        let dr = px[0] as i32 - bg_rgba_u8[0] as i32;
        let dg = px[1] as i32 - bg_rgba_u8[1] as i32;
        let db = px[2] as i32 - bg_rgba_u8[2] as i32;
        let dist = dr.abs() + dg.abs() + db.abs();
        if dist > max_dist {
            max_dist = dist;
        }
        if dist > 60 {
            nonbg += 1;
        }
    }
    assert!(
        nonbg > 100,
        "screenshot has only {} pixels differing from background by >60 \
         (max delta={}) — GPU likely didn't draw the text. Size: {}×{}",
        nonbg,
        max_dist,
        img.width(),
        img.height(),
    );
}

/// Repro for "newline disappears next line, backspace brings it back".
///
/// With a folded region in place, inserting a `\n` before the fold should not
/// hide the wrong buffer line. `FoldState.regions[*].{start_line,end_line}`
/// are absolute buffer-line indices, so an insertion that shifts every
/// subsequent line down by one must shift the fold range too — otherwise the
/// old indices point at line content the user *can see*, and one visible line
/// vanishes from the layout until backspace shifts everything back into place.
///
/// Today the fold range is only refreshed by the async tree-sitter pass on
/// reparse completion. Between the edit and that completion, the stale range
/// hides the wrong line. This test simulates that window by holding a folded
/// region across an edit.
#[test]
fn newline_before_folded_region_does_not_hide_a_visible_line() {
    use crate::display_map::plugin::produce_hidden_lines;
    use crate::types::{FoldKind, FoldRegion};
    use bevy_instanced_text::HiddenLines;

    // 10 buffer lines, fold hiding lines 5..=7 (placeholder = 4).
    let source = (0..10)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, &source);

    // Seed a folded region. `produce_hidden_lines` listens on
    // `Changed<FoldState>`, so the first poll after this seed populates
    // HiddenLines with {5, 6, 7}.
    {
        let mut fold = app.world_mut().get_mut::<FoldState>(entity).unwrap();
        fold.regions.push(FoldRegion {
            start_line: 4,
            end_line: 7,
            is_folded: true,
            kind: FoldKind::Block,
            indent_level: 0,
        });
    }
    app.world_mut()
        .run_system_once(produce_hidden_lines)
        .unwrap();
    {
        let hidden = app.world().get::<HiddenLines>(entity).unwrap();
        let mut got: Vec<usize> = hidden.0.iter().copied().collect();
        got.sort();
        assert_eq!(
            got,
            vec![5, 6, 7],
            "pre-edit: rows 5..=7 should be hidden by the fold"
        );
    }

    // EDIT: insert `\n` at the very start of the buffer. Every line shifts
    // down by one. After the edit, the *original* line5 (`line5`) now lives
    // at buffer row 6. The fold should now hide rows 6..=8.
    let new_source = format!("\n{source}");
    {
        let mut buf = app
            .world_mut()
            .get_mut::<TextBuffer<RopeBuffer>>(entity)
            .unwrap();
        buf.0 = RopeBuffer(ropey::Rope::from_str(&new_source));
    }
    // Emit the TextEdited event the production edit path would emit. This is
    // what downstream consumers (LSP, syntax, fold-detection) react to.
    app.world_mut()
        .resource_mut::<Messages<TextEdited>>()
        .write(TextEdited {
            delta: EditDelta {
                start_byte: 0,
                old_end_byte: 0,
                new_end_byte: 1,
                start_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                old_end_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                new_end_position: EditPoint {
                    row: 1,
                    column_byte: 0,
                },
            },
            content_version: 2,
            pre_edit_rope: None,
        });

    // Tick so any system that wants to react to the TextEdited event runs.
    app.update();
    app.world_mut()
        .run_system_once(crate::plugin::folding::shift_fold_regions_on_edit)
        .unwrap();
    app.world_mut()
        .run_system_once(produce_hidden_lines)
        .unwrap();

    let buffer = app.world().get::<TextBuffer<RopeBuffer>>(entity).unwrap();
    let line_at = |i: usize| -> String {
        bevy_instanced_text::TextContent::line(&**buffer, i)
            .trim_end_matches('\n')
            .to_string()
    };
    let hidden = app.world().get::<HiddenLines>(entity).unwrap();
    let mut got: Vec<usize> = hidden.0.iter().copied().collect();
    got.sort();

    // What we *want*: the fold's content (originally lines 5,6,7) now lives at
    // buffer rows 6,7,8, so HiddenLines should be {6, 7, 8}.
    let originally_hidden_content: Vec<String> =
        vec!["line5".into(), "line6".into(), "line7".into()];
    let now_hidden_content: Vec<String> = got.iter().map(|&i| line_at(i)).collect();

    assert_eq!(
        now_hidden_content, originally_hidden_content,
        "After inserting `\\n` at row 0, the fold still hides buffer rows {got:?} \
         which now contain {now_hidden_content:?}. The same *content* \
         ({originally_hidden_content:?}) should stay hidden — i.e. fold range \
         should have shifted from 4..=7 to 5..=8 with the rest of the buffer.",
    );
}

/// End-to-end variant of `newline_before_folded_region_does_not_hide_a_visible_line`:
/// instead of poking the `shift_fold_regions_on_edit` system directly, this
/// goes through `app.update()` so the actual `FoldingPlugin` schedule order
/// is exercised. If this test passes while the manual version passes too,
/// the wiring is correct; if it fails, the system isn't running in the
/// expected schedule slot.
#[test]
fn newline_before_folded_region_full_schedule() {
    use crate::plugin::folding::shift_fold_regions_on_edit;
    use crate::types::{FoldKind, FoldRegion};
    use bevy_instanced_text::HiddenLines;

    let source = (0..10)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut app = make_test_app();
    // Register only the shift system, not the full FoldingPlugin — the async
    // fold-detect task would wipe our seeded fold (plain-text buffer has no
    // foldable regions) and obscure what we're testing.
    app.add_systems(
        Update,
        shift_fold_regions_on_edit.in_set(crate::plugin::ApplyStateSet),
    );
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, &source);

    {
        let mut fold = app.world_mut().get_mut::<FoldState>(entity).unwrap();
        fold.regions.push(FoldRegion {
            start_line: 4,
            end_line: 7,
            is_folded: true,
            kind: FoldKind::Block,
            indent_level: 0,
        });
    }
    // Tick once so `produce_hidden_lines` (in LayoutSyncSet) reacts to the
    // FoldState change and populates HiddenLines.
    app.update();
    {
        let hidden = app.world().get::<HiddenLines>(entity).unwrap();
        let mut got: Vec<usize> = hidden.0.iter().copied().collect();
        got.sort();
        assert_eq!(got, vec![5, 6, 7], "post-seed: rows 5..=7 hidden");
    }

    // Now: edit and TextEdited together, then a single tick. The shift
    // system + produce_hidden_lines must both have run before we observe.
    let new_source = format!("\n{source}");
    {
        let mut buf = app
            .world_mut()
            .get_mut::<TextBuffer<RopeBuffer>>(entity)
            .unwrap();
        buf.0 = RopeBuffer(ropey::Rope::from_str(&new_source));
    }
    app.world_mut()
        .resource_mut::<Messages<TextEdited>>()
        .write(TextEdited {
            delta: EditDelta {
                start_byte: 0,
                old_end_byte: 0,
                new_end_byte: 1,
                start_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                old_end_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                new_end_position: EditPoint {
                    row: 1,
                    column_byte: 0,
                },
            },
            content_version: 2,
            pre_edit_rope: None,
        });
    app.update();

    let buffer = app.world().get::<TextBuffer<RopeBuffer>>(entity).unwrap();
    let line_at = |i: usize| -> String {
        bevy_instanced_text::TextContent::line(&**buffer, i)
            .trim_end_matches('\n')
            .to_string()
    };
    let hidden = app.world().get::<HiddenLines>(entity).unwrap();
    let mut got: Vec<usize> = hidden.0.iter().copied().collect();
    got.sort();
    let now_hidden_content: Vec<String> = got.iter().map(|&i| line_at(i)).collect();
    assert_eq!(
        now_hidden_content,
        vec!["line5", "line6", "line7"],
        "End-to-end: after one app.update() the same fold content must stay hidden. \
         Got rows {got:?} = {now_hidden_content:?}.",
    );
}

/// Symmetric case: backspace-joining a row shifts every subsequent line up
/// by one. A folded region's start/end must follow.
#[test]
fn backspace_before_folded_region_keeps_same_content_hidden() {
    use crate::display_map::plugin::produce_hidden_lines;
    use crate::types::{FoldKind, FoldRegion};
    use bevy_instanced_text::HiddenLines;

    // 11 buffer lines (one leading blank), fold hiding rows 6..=8 (placeholder = 5).
    // After deleting the leading `\n`, the same content should land at rows 5..=7.
    let source = std::iter::once(String::new())
        .chain((0..10).map(|i| format!("line{i}")))
        .collect::<Vec<_>>()
        .join("\n");

    let mut app = make_test_app();
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, &source);

    {
        let mut fold = app.world_mut().get_mut::<FoldState>(entity).unwrap();
        fold.regions.push(FoldRegion {
            start_line: 5,
            end_line: 8,
            is_folded: true,
            kind: FoldKind::Block,
            indent_level: 0,
        });
    }
    app.world_mut()
        .run_system_once(produce_hidden_lines)
        .unwrap();

    // EDIT: delete the leading `\n`. Pivot row = 1, shift = -1.
    let new_source = source.strip_prefix('\n').unwrap().to_string();
    {
        let mut buf = app
            .world_mut()
            .get_mut::<TextBuffer<RopeBuffer>>(entity)
            .unwrap();
        buf.0 = RopeBuffer(ropey::Rope::from_str(&new_source));
    }
    app.world_mut()
        .resource_mut::<Messages<TextEdited>>()
        .write(TextEdited {
            delta: EditDelta {
                start_byte: 0,
                old_end_byte: 1,
                new_end_byte: 0,
                start_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
                old_end_position: EditPoint {
                    row: 1,
                    column_byte: 0,
                },
                new_end_position: EditPoint {
                    row: 0,
                    column_byte: 0,
                },
            },
            content_version: 2,
            pre_edit_rope: None,
        });

    app.update();
    app.world_mut()
        .run_system_once(crate::plugin::folding::shift_fold_regions_on_edit)
        .unwrap();
    app.world_mut()
        .run_system_once(produce_hidden_lines)
        .unwrap();

    let buffer = app.world().get::<TextBuffer<RopeBuffer>>(entity).unwrap();
    let line_at = |i: usize| -> String {
        bevy_instanced_text::TextContent::line(&**buffer, i)
            .trim_end_matches('\n')
            .to_string()
    };
    let hidden = app.world().get::<HiddenLines>(entity).unwrap();
    let mut got: Vec<usize> = hidden.0.iter().copied().collect();
    got.sort();
    let now_hidden_content: Vec<String> = got.iter().map(|&i| line_at(i)).collect();
    let originally_hidden_content: Vec<String> =
        vec!["line5".into(), "line6".into(), "line7".into()];
    assert_eq!(
        now_hidden_content, originally_hidden_content,
        "After deleting `\\n` at row 0, fold should still hide the same content. \
         Got rows {got:?} = {now_hidden_content:?}.",
    );
}

/// Reproduces the user-reported "next line disappears" bug — full file
/// + `FoldingPlugin` registered so the async fold-detect pipeline runs.
///
/// Loads the entire `examples/editor_lsp.rs` source, awaits initial parse
/// AND initial fold detection, then inserts `\n` at the **start of the
/// blank row above `fn main`** (row 17). Asserts every layer (rope,
/// LineStyles cache, DisplayLayout, HiddenLines) holds the displaced
/// `fn main` row in the right place.
#[test]
fn insert_newline_above_fn_main_full_pipeline() {
    use crate::plugin::folding::FoldingPlugin;
    use bevy_instanced_text::HiddenLines;

    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples/editor_lsp.rs"),
    )
    .expect("read editor_lsp.rs");
    let lines: Vec<&str> = source.lines().collect();
    assert_eq!(lines.get(17).copied(), Some(""), "row 17 should be blank");
    assert!(
        lines
            .get(18)
            .map(|s| s.starts_with("fn main"))
            .unwrap_or(false),
        "row 18 should start with `fn main`",
    );

    let mut app = make_test_app();
    app.add_plugins(FoldingPlugin);
    install_atlas_and_font(&mut app);
    let entity = spawn_test_editor(&mut app, &source);

    // Wait for initial parse and at least one fold-detection cycle.
    let _ = await_initial_parse(&mut app, entity);
    let _ = run_until(&mut app, entity, Duration::from_secs(5), |w, e| {
        let fold = w.get::<FoldState>(e).unwrap();
        !fold.regions.is_empty()
    });

    // Drive layout pass once so DisplayLayout reflects pre-edit state too.
    drive_layout_and_render_once(&mut app);
    let pre_fold = app.world().get::<FoldState>(entity).unwrap().clone();
    let pre_fn_main_fold = pre_fold
        .regions
        .iter()
        .find(|r| r.start_line == 18)
        .cloned();
    assert!(
        pre_fn_main_fold.is_some(),
        "expected a fold region starting at row 18 (`fn main`). \
         Got regions: {:?}",
        pre_fold.regions,
    );

    // EDIT: insert `\n` at start of row 17.
    let row_17_byte = {
        let buf = app.world().get::<TextBuffer<RopeBuffer>>(entity).unwrap();
        buf.rope().line_to_byte(17)
    };
    {
        let mut buf = app
            .world_mut()
            .get_mut::<TextBuffer<RopeBuffer>>(entity)
            .unwrap();
        let row_17_char = buf.rope().byte_to_char(row_17_byte);
        buf.0 .0.insert(row_17_char, "\n");
    }
    app.world_mut()
        .resource_mut::<Messages<TextEdited>>()
        .write(TextEdited {
            delta: EditDelta {
                start_byte: row_17_byte,
                old_end_byte: row_17_byte,
                new_end_byte: row_17_byte + 1,
                start_position: EditPoint {
                    row: 17,
                    column_byte: 0,
                },
                old_end_position: EditPoint {
                    row: 17,
                    column_byte: 0,
                },
                new_end_position: EditPoint {
                    row: 18,
                    column_byte: 0,
                },
            },
            content_version: 2,
            pre_edit_rope: None,
        });

    // One tick: shift_fold_regions_on_edit + produce_line_styles consume the event.
    app.update();
    // Drive layout against new state.
    drive_layout_and_render_once(&mut app);

    // Assert: rope is correct.
    let post_rope_lines: Vec<String> = {
        let buf = app.world().get::<TextBuffer<RopeBuffer>>(entity).unwrap();
        (0..buf.len_lines())
            .map(|i| {
                bevy_instanced_text::TextContent::line(&**buf, i)
                    .trim_end_matches('\n')
                    .to_string()
            })
            .collect()
    };
    assert!(
        post_rope_lines
            .get(19)
            .map(|s| s.starts_with("fn main"))
            .unwrap_or(false),
        "post-edit rope row 19 must start with `fn main`, got {:?}",
        post_rope_lines.get(19),
    );

    // Assert: fold region shifted to start at row 19.
    let post_fold = app.world().get::<FoldState>(entity).unwrap().clone();
    let post_fn_main_fold = post_fold
        .regions
        .iter()
        .find(|r| r.start_line == 19)
        .cloned();
    assert!(
        post_fn_main_fold.is_some(),
        "fold region for `fn main` did not shift from start_line=18 to start_line=19. \
         Post-edit regions: {:?}",
        post_fold.regions,
    );

    // Assert: HiddenLines doesn't hide row 19 (the fold is NOT folded).
    let hidden = app.world().get::<HiddenLines>(entity).unwrap();
    assert!(
        hidden.is_visible(19),
        "HiddenLines hides row 19 (`fn main`) after the edit. Hidden: {:?}",
        hidden.0.iter().copied().collect::<Vec<_>>(),
    );

    // Assert: DisplayLayout has a line for buffer_row=19 with `fn main` text.
    let layout = app.world().get::<DisplayLayout>(entity).unwrap();
    let row_19_line = layout.lines.iter().find(|l| l.buffer_row == 19);
    let layout_dump: Vec<(u32, u32, String)> = layout
        .lines
        .iter()
        .map(|l| (l.display_row, l.buffer_row, l.text.clone()))
        .collect();
    assert!(
        row_19_line.is_some(),
        "DisplayLayout has no ShapedLine with buffer_row=19. Lines: {layout_dump:?}",
    );
    let layout_text_19 = &row_19_line.unwrap().text;
    assert!(
        layout_text_19.starts_with("fn main"),
        "DisplayLayout.lines[buffer_row=19].text = {layout_text_19:?}. Lines: {layout_dump:?}",
    );
}
