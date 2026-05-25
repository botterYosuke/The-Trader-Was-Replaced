//! Real-plugin regression test for the Strategy Editor seed crash (#35 smoke, 2026-05-25).
//!
//! The e2e_replay harness uses `MinimalPlugins` and never runs `TextInputPlugin`, which is
//! why all 106 e2e tests stayed green through a deterministic startup panic. This binary
//! wires up the REAL `bevy_ui_text_input::TextInputPlugin` headless so the seed path is
//! exercised end-to-end.
//!
//! The bug: the plugin chains `process_text_input_queues` BEFORE `text_input_system`, so a
//! freshly-spawned `TextInputNode`'s buffer is still `Buffer::new_empty` (0 lines) when a
//! queued SelectAll+Paste runs on its first frame → cosmic-text indexes `buffer.lines[0]`
//! → `index out of bounds` panic. Production used to seed at spawn (`seed_queue`); the fix
//! defers seeding to `apply_pending_editor_seed_system`, which waits for the buffer to be ready.

use bevy::prelude::*; // TextureAtlasLayout comes from the bevy_image prelude re-export
use bevy_ui_text_input::actions::{TextInputAction, TextInputEdit};
use bevy_ui_text_input::{TextInputContents, TextInputNode, TextInputPlugin, TextInputQueue};

use backcast::ui::components::{
    PanelSpawnSource, RegionKeyAllocator, ScenarioStartupField, ScenarioStartupFieldEditor,
    ScenarioStartupParams, StrategyEditorSpawnSpec,
};
use backcast::ui::scenario_startup_panel::sync_startup_param_editors_text_system;
use backcast::ui::strategy_editor::{
    StrategyEditorContent, apply_pending_editor_seed_system, spawn_strategy_editor_panel,
};
use bevy_ui_text_input::TextInputMode;

const SEED: &str = "# dummy";

/// Minimal headless app that actually runs `TextInputPlugin`'s PostUpdate systems.
/// `InputPlugin` supplies KeyboardInput / MouseWheel / ButtonInput (InputDispatchPlugin +
/// mouse_wheel_scroll); AssetPlugin + the three asset types and `HoverMap` feed the
/// remaining system params. No RenderApp is created, so the plugin skips its render systems.
fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(bevy::input::InputPlugin)
        .init_asset::<Font>()
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .init_resource::<bevy::picking::hover::HoverMap>()
        .add_plugins(TextInputPlugin);
    app
}

/// Install a real font at the DEFAULT font handle so the production editor (which uses
/// `TextFont::default().font`) shapes text and the pipeline gives the buffer its first line.
fn install_default_font(app: &mut App) {
    let bytes = std::fs::read("assets/fonts/NotoSansSymbols2-Regular.ttf")
        .expect("NotoSansSymbols2-Regular.ttf present in assets/fonts");
    let font = Font::try_from_bytes(bytes).expect("valid ttf");
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(Handle::<Font>::default().id(), font);
}

fn spawn_editor_via_production(mut commands: Commands, mut alloc: ResMut<RegionKeyAllocator>) {
    spawn_strategy_editor_panel(
        &mut commands,
        &mut alloc,
        StrategyEditorSpawnSpec {
            region_key: None,
            source: Some(SEED.to_string()),
            layout_source: PanelSpawnSource::User,
        },
    );
}

/// Pinned UPSTREAM HAZARD: queuing SelectAll+Paste directly onto a freshly-spawned
/// `TextInputNode` panics, because `process_text_input_queues` runs before the buffer's
/// first `set_text`. This is exactly the crash the live smoke hit and the reason production
/// must defer seeding. If a future `bevy_ui_text_input` stops panicking here, this test will
/// start failing — a signal that the `PendingEditorSeed` workaround can be simplified.
#[test]
#[should_panic(expected = "index out of bounds")]
fn raw_seed_at_spawn_panics_with_real_plugin() {
    let mut app = build_app();
    let mut q = TextInputQueue::default();
    q.add(TextInputAction::Edit(TextInputEdit::SelectAll));
    q.add(TextInputAction::Edit(TextInputEdit::Paste(SEED.to_string())));
    app.world_mut().spawn((
        TextInputNode {
            clear_on_submit: false,
            unfocus_on_submit: false,
            ..default()
        },
        Node {
            width: Val::Px(200.0),
            height: Val::Px(100.0),
            ..default()
        },
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextInputContents::default(),
        q,
    ));
    for _ in 0..5 {
        app.update();
    }
}

/// GREEN regression guard: the production spawn (`spawn_strategy_editor_panel`) seeds the
/// editor via `PendingEditorSeed` + `apply_pending_editor_seed_system`, so the real plugin
/// never panics and the seed text lands in the editor once the buffer is initialized.
#[test]
fn production_editor_spawn_seeds_without_panic() {
    let mut app = build_app();
    install_default_font(&mut app);
    app.init_resource::<RegionKeyAllocator>()
        .add_systems(Startup, spawn_editor_via_production)
        .add_systems(Update, apply_pending_editor_seed_system);

    // Frame 1: Startup spawns the editor (buffer empty) → PostUpdate initializes the buffer.
    // Frame 2: the apply system sees a ready buffer and queues the seed → PostUpdate applies it.
    // Extra frames let update_text_input_contents propagate the text into TextInputContents.
    for _ in 0..8 {
        app.update();
    }

    let world = app.world_mut();
    let mut state = world.query_filtered::<&TextInputContents, With<StrategyEditorContent>>();
    let contents: Vec<String> = state.iter(world).map(|c| c.get().to_string()).collect();
    assert_eq!(contents.len(), 1, "exactly one Strategy Editor content node");
    assert_eq!(
        contents[0], SEED,
        "seed text must reach the editor buffer once it is ready"
    );
}

/// GREEN regression guard for the Startup panel (the second crash site found in the smoke):
/// `sync_startup_param_editors_text_system` fires on cache restore (`params.is_changed()`)
/// while the field buffer is still 0-line. Pre-fix it pushed SelectAll+Paste directly and
/// panicked; post-fix it stashes `PendingEditorSeed` so the readiness-gated apply system
/// seeds the field once the buffer is ready. The param value must land without a panic.
#[test]
fn startup_field_param_sync_seeds_without_panic() {
    let mut app = build_app();
    install_default_font(&mut app);

    let params = ScenarioStartupParams {
        start: "2025-01-06".to_string(),
        ..default()
    };
    app.insert_resource(params); // freshly inserted ⇒ is_changed on frame 1
    app.add_systems(
        Update,
        (
            sync_startup_param_editors_text_system,
            apply_pending_editor_seed_system,
        ),
    );

    // Field spawned with an empty (0-line) buffer, exactly like the production startup panel.
    app.world_mut().spawn((
        TextInputNode {
            mode: TextInputMode::SingleLine,
            clear_on_submit: false,
            unfocus_on_submit: false,
            ..default()
        },
        Node {
            width: Val::Px(120.0),
            height: Val::Px(20.0),
            ..default()
        },
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextInputContents::default(),
        ScenarioStartupFieldEditor {
            field: ScenarioStartupField::Start,
        },
    ));

    for _ in 0..8 {
        app.update();
    }

    let world = app.world_mut();
    let mut state = world.query_filtered::<&TextInputContents, With<ScenarioStartupFieldEditor>>();
    let contents: Vec<String> = state.iter(world).map(|c| c.get().to_string()).collect();
    assert_eq!(contents.len(), 1, "exactly one startup field");
    assert_eq!(
        contents[0], "2025-01-06",
        "startup param must reach the field buffer once it is ready"
    );
}
