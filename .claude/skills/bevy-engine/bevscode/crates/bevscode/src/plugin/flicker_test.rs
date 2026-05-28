//! Regression tests for scroll-state flicker bugs.
//!
//! These tests spawn a `CodeEditor` entity, drive it through a few frames,
//! and assert invariants on `ScrollAnimator.target` / `ScrollPosition.y`.
//! When a test fails, the diagnostic output names the tick at which an
//! invariant was violated — enough to identify which system wrote a bad value.

#![cfg(test)]
#![allow(clippy::field_reassign_with_default)]

use bevy::input::mouse::MouseScrollUnit;
use bevy::math::{Affine2, Vec2, Vec3};
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, Scroll};
use bevy::picking::pointer::{Location, PointerId};
use bevy::prelude::*;
use bevy::ui::ui_transform::UiGlobalTransform;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_instanced_text::view::measurement::LayoutTuning;
use bevy_instanced_text::{
    ContentMetrics, DisplayLayout, HiddenLines, LineStyles, MonoCellWidth, TextBounds, TextBuffer,
    TextOverlays, TextUnderlays,
};
use bevy_instanced_text_editor::{RopeBuffer, TextViewDragState};

use crate::plugin::scroll_animator::ScrollAnimator;

use crate::plugin::{ApplyStateSet, InputSet};
use crate::settings::{
    BracketConfig, CursorLine, EditorTheme, EditorUi, GutterConfig, Indentation, Performance,
    SyntaxColors, Wrapping,
};
use crate::types::{
    BracketMatchRects, BracketMatchState, CaretRects, CodeEditor, CursorLineRects, CursorState,
    FoldState, IndentGuideRects, SelectionRects, SelectionState,
};

fn make_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.add_plugins(bevy::input::InputPlugin);
    app.init_resource::<Assets<bevy::text::Font>>();
    app.add_message::<crate::types::events::TextEdited>();
    // Window events are read by `bevy_picking::input::mouse_pick_events` (a
    // system in `bevy_picking`). Headless tests don't get `WindowPlugin`, so
    // register the message manually.
    app.add_message::<bevy::window::WindowEvent>();
    app.configure_sets(
        Update,
        (
            InputSet,
            bevy_instanced_text_editor::EditEmitSet.after(InputSet),
            ApplyStateSet.after(bevy_instanced_text_editor::EditEmitSet),
        )
            .chain(),
    );
    app
}

fn spawn_editor(app: &mut App, text: &str) -> Entity {
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
    let scroll_bundle = (ScrollPosition::default(), ScrollAnimator::default());
    let layout_bundle = (
        TextBuffer::<RopeBuffer>::new(RopeBuffer::new(text)),
        ContentMetrics::default(),
        computed,
        DisplayLayout::default(),
        TextUnderlays::default(),
        TextOverlays::default(),
        TextBounds::default(),
        LayoutTuning::default(),
        HiddenLines::default(),
        LineStyles::default(),
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(400.0, 300.0))),
    );
    let settings_bundle = (
        EditorTheme::default(),
        SyntaxColors::default(),
        EditorUi::default(),
        Indentation::default(),
        Wrapping::default(),
        Performance::default(),
        GutterConfig::default(),
        CursorLine::default(),
    );
    let editor_state_bundle = (
        SelectionState::default(),
        CursorState::default(),
        FoldState::default(),
        BracketMatchState::default(),
        BracketConfig::default(),
        TextViewDragState::default(),
    );
    let overlay_bundle = (
        SelectionRects::default(),
        IndentGuideRects::default(),
        CursorLineRects::default(),
        CaretRects::default(),
        BracketMatchRects::default(),
    );
    app.world_mut()
        .spawn((CodeEditor, Name::new("TestEditor")))
        .insert(font_bundle)
        .insert(scroll_bundle)
        .insert(layout_bundle)
        .insert(settings_bundle)
        .insert(editor_state_bundle)
        .insert(overlay_bundle)
        .id()
}

/// Move the cursor below the viewport once, then tick the schedule 10
/// times with no input. The expected behavior: tick 1 sets
/// `VerticalScroll.target` to the cursor's row, and from tick 2 onward the
/// target stays put (because `last_cursor_pos` was synced).
///
/// If this test sees `.target` move past tick 1, the auto-scroll loop is
/// mis-detecting cursor movement and re-firing every frame.
#[test]
fn auto_scroll_settles_after_single_cursor_move() {
    let mut app = make_test_app();
    app.add_systems(
        Update,
        crate::plugin::ui_elements::auto_scroll_to_cursor
            .run_if(crate::plugin::ui_elements::should_auto_scroll)
            .in_set(ApplyStateSet),
    );

    // 200-line buffer; place cursor on line 100 so it's well below the
    // visible 800x600 viewport (28 lines at 21px line-height).
    let mut text = String::new();
    for i in 0..200 {
        text.push_str(&format!("line {i}\n"));
    }
    let entity = spawn_editor(&mut app, &text);

    // Place the cursor on line 100 (char offset ~700).
    let target_char = text
        .char_indices()
        .filter(|(_, c)| *c == '\n')
        .nth(100)
        .map(|(i, _)| i)
        .unwrap();
    app.world_mut()
        .get_mut::<CursorState>(entity)
        .unwrap()
        .cursor_pos = target_char;

    let mut history: Vec<f32> = Vec::new();
    for _ in 0..10 {
        app.update();
        let animator = app.world().get::<ScrollAnimator>(entity).unwrap();
        history.push(animator.target.y);
    }

    let first = history[0];
    let later_changes: Vec<(usize, f32, f32)> = history
        .windows(2)
        .enumerate()
        .filter(|(_, w)| (w[0] - w[1]).abs() > 0.01)
        .map(|(i, w)| (i + 1, w[0], w[1]))
        .collect();

    assert!(
        first > 0.0,
        "Tick 1 should have moved ScrollAnimator.target.y off zero (got {first})"
    );
    assert!(
        later_changes.is_empty(),
        "ScrollAnimator.target.y kept changing after tick 1 — auto_scroll_to_cursor doesn't settle.\n\
         Changes: {later_changes:?}\nFull history: {history:?}",
    );
}

/// The minimum reproduction: `auto_scroll_to_cursor` registered as a system
/// in `ApplyStateSet`, run for 10 ticks with no input. `ScrollTarget` must
/// stay at `0.0`.
#[test]
fn auto_scroll_does_not_move_scroll_target_on_idle_frames() {
    let mut app = make_test_app();
    app.add_systems(
        Update,
        crate::plugin::ui_elements::auto_scroll_to_cursor
            .run_if(crate::plugin::ui_elements::should_auto_scroll)
            .in_set(ApplyStateSet),
    );

    let entity = spawn_editor(&mut app, "fn main() {\n    println!(\"hi\");\n}\n");

    let mut history: Vec<f32> = vec![0.0];
    for _ in 0..10 {
        app.update();
        let animator = app.world().get::<ScrollAnimator>(entity).unwrap();
        history.push(animator.target.y);
    }

    let moved: Vec<(usize, f32)> = history
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, &v)| v != 0.0)
        .map(|(i, &v)| (i, v))
        .collect();

    assert!(
        moved.is_empty(),
        "ScrollAnimator.target.y moved on idle frames: {moved:?}\nFull history: {history:?}",
    );
}

/// Same as above, but with the *full* plugin set (`CodeEditorPlugins`
/// PluginGroup minus rendering bits we can't run headlessly). This is the
/// production surface — if only this one fails (and the minimal one passes),
/// the culprit is some other system the group adds.
#[test]
fn full_code_editor_plugin_does_not_move_scroll_target_on_idle_frames() {
    let mut app = make_test_app();
    // Pull in everything the editor plugin needs to *function*; skip the
    // GPU-render plugins (`GlyphAtlasPlugin`, `InstancedTextRenderPlugin`)
    // and `InstancedTextPlugin` since they require a render device.
    app.add_plugins(bevy::input_focus::InputDispatchPlugin);
    app.add_plugins(bevy_instanced_text_editor::InstancedTextEditPlugin::without_typing_observer());
    app.add_plugins(leafwing_input_manager::plugin::InputManagerPlugin::<
        crate::input::EditorAction,
    >::default());
    app.add_plugins(crate::plugin::CodeEditorPlugin);
    app.add_plugins(crate::plugin::CursorPlugin);
    app.add_plugins(crate::plugin::SyntaxPlugin);
    app.add_plugins(crate::plugin::FoldingPlugin);
    app.add_plugins(crate::plugin::BracketPlugin);
    app.add_plugins(crate::display_map::DisplayMapPlugin);

    let entity = spawn_editor(&mut app, "fn main() {\n    println!(\"hi\");\n}\n");

    let mut history: Vec<f32> = vec![0.0];
    for _ in 0..10 {
        app.update();
        let animator = app.world().get::<ScrollAnimator>(entity).unwrap();
        history.push(animator.target.y);
    }

    let moved: Vec<(usize, f32)> = history
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, &v)| v != 0.0)
        .map(|(i, &v)| (i, v))
        .collect();

    assert!(
        moved.is_empty(),
        "ScrollAnimator.target.y moved on idle frames under full plugin set: {moved:?}\nFull history: {history:?}",
    );
}

/// Fire a `Pointer<Scroll>` at the editor and verify `on_pointer_scroll`
/// accumulates the delta into `ScrollPosition.y`. Wheel writes are instant —
/// `bevy_instanced_text_interaction` writes `ScrollPosition` directly so the wheel
/// stays out of the host animator path.
#[test]
fn pointer_scroll_event_accumulates_into_target() {
    let mut app = make_test_app();
    app.add_plugins(bevy_instanced_text::view::plugin::InstancedTextPlugin);
    app.add_plugins(bevy::input_focus::InputDispatchPlugin);
    app.add_plugins(bevy_instanced_text_editor::InstancedTextEditPlugin::without_typing_observer());
    app.add_plugins(leafwing_input_manager::plugin::InputManagerPlugin::<
        crate::input::EditorAction,
    >::default());
    app.add_plugins(crate::plugin::CodeEditorPlugin);

    // 200-line buffer so there's room to scroll.
    let mut text = String::new();
    for i in 0..200 {
        text.push_str(&format!("line {i}\n"));
    }
    let entity = spawn_editor(&mut app, &text);
    app.update();

    let dummy_window = app.world_mut().spawn_empty().id();
    let dummy_camera = app.world_mut().spawn_empty().id();
    let normalized_window = bevy::window::WindowRef::Entity(dummy_window)
        .normalize(None)
        .unwrap();
    // Three small swipes down: each `dy = -10` (Pixel) -> target += 10.
    for _ in 0..3 {
        let scroll_event = Pointer::<Scroll>::new(
            PointerId::Mouse,
            Location {
                target: bevy::camera::NormalizedRenderTarget::Window(normalized_window),
                position: Vec2::new(400.0, 300.0),
            },
            Scroll {
                unit: MouseScrollUnit::Pixel,
                x: 0.0,
                y: -10.0,
                hit: HitData::new(dummy_camera, 0.0, Some(Vec3::ZERO), None),
            },
            entity,
        );
        app.world_mut().trigger(scroll_event);
    }

    let scroll_y = app.world().get::<ScrollPosition>(entity).unwrap().y;
    assert!(
        (scroll_y - 30.0).abs() < 0.01,
        "Three Pixel scrolls of dy=-10 should accumulate ScrollPosition.y to 30.0, got {scroll_y}"
    );
}

/// Set `ScrollAnimator.target` once and tick. `ScrollPosition.y` must
/// (a) move monotonically toward `target.y`, (b) actually reach it within
/// roughly `duration` seconds of frames.
///
/// Regression test for the host animator that replaced the engine's
/// built-in smooth scroll.
#[test]
fn animator_drives_current_to_target_within_duration() {
    let mut app = make_test_app();
    app.add_plugins(bevy_instanced_text::view::plugin::InstancedTextPlugin);
    app.add_plugins(bevy::input_focus::InputDispatchPlugin);
    app.add_plugins(bevy_instanced_text_editor::InstancedTextEditPlugin::without_typing_observer());
    app.add_plugins(leafwing_input_manager::plugin::InputManagerPlugin::<
        crate::input::EditorAction,
    >::default());
    app.add_plugins(crate::plugin::CodeEditorPlugin);
    app.add_plugins(crate::plugin::EditorUiPlugin);
    app.add_plugins(crate::plugin::ScrollAnimatorPlugin);

    let entity = spawn_editor(&mut app, "fn main() {}\n");
    app.update();

    let target_y = 200.0;
    {
        let mut animator = app.world_mut().get_mut::<ScrollAnimator>(entity).unwrap();
        animator.duration = 0.125;
        animator.target = Vec2::new(0.0, target_y);
    }

    let mut history: Vec<f32> = Vec::new();
    for _ in 0..30 {
        app.update();
        history.push(app.world().get::<ScrollPosition>(entity).unwrap().y);
    }

    let regressions: Vec<(usize, f32, f32)> = history
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[1] + 0.01 < w[0]).then_some((i + 1, w[0], w[1])))
        .collect();
    let dump = || {
        history
            .iter()
            .enumerate()
            .map(|(i, c)| format!("  tick {i:>2}: current={c:>7.2}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(
        regressions.is_empty(),
        "ScrollPosition.y regressed during animation (target={target_y}):\n{}\nRegressions: {regressions:?}",
        dump(),
    );

    let final_v = *history.last().unwrap();
    assert!(
        (final_v - target_y).abs() < 0.5,
        "ScrollPosition.y never reached target after 30 frames: final={final_v:.2}, target={target_y:.2}\n{}",
        dump(),
    );
}
