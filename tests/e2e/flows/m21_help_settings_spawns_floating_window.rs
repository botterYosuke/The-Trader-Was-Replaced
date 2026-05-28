//! M21 help_settings_spawns_floating_window — Help メニューから Settings を選択すると
//! Settings floating window が 1 枚 spawn され、2 回選択しても重複しないことを保証する。
//!
//! `panel_spawn_dispatcher_system` が `PanelSpawnRequested { kind: PanelKind::Settings }` を
//! 受け取って WindowRoot を持つ entity を spawn するパスと、
//! 既に存在するときは無視する dedup ガードを通す。

use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PanelSpawnSource,
    PendingStrategyFragments, RegionKeyAllocator, StrategyBuffer, WindowManager, WindowRoot,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::AutoSaveState;
use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy_cosmic_edit::prelude::CosmicFontSystem;
use cosmic_text::FontSystem;

fn settings_app() -> App {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.insert_resource(WindowManager::default())
        .insert_resource(CosmicFontSystem(FontSystem::new()))
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(PendingStrategyFragments::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(InstrumentRegistry::default());
    app.add_message::<PanelSpawnRequested>();
    app.add_systems(Update, panel_spawn_dispatcher_system);
    app
}

fn settings_panel_count(app: &mut App) -> usize {
    let mut q = app
        .world_mut()
        .query_filtered::<&PanelKind, With<WindowRoot>>();
    q.iter(app.world())
        .filter(|kind| **kind == PanelKind::Settings)
        .count()
}

#[test]
fn m21_help_settings_spawns_floating_window() {
    let mut app = settings_app();
    app.world_mut().write_message(PanelSpawnRequested {
        kind: PanelKind::Settings,
        source: PanelSpawnSource::User,
        strategy_spec: None,
    });
    app.update();
    app.update();
    assert_eq!(
        settings_panel_count(&mut app),
        1,
        "Settings request should spawn exactly 1 window"
    );
}

#[test]
fn m21_help_settings_no_duplicate_on_second_spawn() {
    let mut app = settings_app();
    for _ in 0..2 {
        app.world_mut().write_message(PanelSpawnRequested {
            kind: PanelKind::Settings,
            source: PanelSpawnSource::User,
            strategy_spec: None,
        });
        app.update();
        app.update();
    }
    assert_eq!(
        settings_panel_count(&mut app),
        1,
        "Settings dedup guard: 2 requests should still yield exactly 1 window"
    );
}
