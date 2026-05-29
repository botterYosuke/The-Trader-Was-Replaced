//! M26 strategy_editor_spawn_uses_bevscode_not_cosmic — Slice 6c (#50) RED→GREEN
//!
//! Slice 6c の契約: PanelKind::StrategyEditor の spawn で
//!   - cosmic-typed component (`CosmicEditBuffer` / `TextEdit2d`) が 0 件
//!   - bevscode 側 (`StrategyEditorNode` / `CodeEditor`) が 1 件
//! になることを assert する。
//!
//! Slice 6c 開始時点: cosmic editor entity が 1 件 spawn される実装が残っており fail (RED)。
//! Slice 6c 完了時点: cosmic spawn が削除され bevscode peer のみ残る (GREEN)。
//!
//! Slice 6d で `CosmicFontSystem` 注入が test ヘルパーから消えた段階で、本 test の
//! cosmic 関連 import / insert_resource も削除する（cosmic crate 自体が 6e で消滅するため）。

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy_cosmic_edit::CosmicEditBuffer;
use bevy_cosmic_edit::prelude::{CosmicFontSystem, TextEdit2d};
use cosmic_text::FontSystem;

use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PendingStrategyFragments,
    RegionKeyAllocator, StrategyBuffer, WindowManager,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::AutoSaveState;
use backcast::ui::strategy_editor::{
    StrategyEditorNode, spawn_bevscode_peer_on_strategy_editor_added,
};

#[test]
fn m26_strategy_editor_spawn_uses_bevscode_not_cosmic() {
    let mut app = App::new();
    app.add_plugins(bevy::app::TaskPoolPlugin::default())
        .add_plugins(TransformPlugin)
        .add_plugins(AssetPlugin::default())
        // bevscode peer spawn が AssetServer.load::<Font>() を呼ぶため Font 型を登録。
        .init_asset::<Font>();

    app.insert_resource(WindowManager::default())
        .insert_resource(CosmicFontSystem(FontSystem::new()))
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(PendingStrategyFragments::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(InstrumentRegistry::default());

    app.add_message::<PanelSpawnRequested>();

    app.add_systems(
        Update,
        (
            panel_spawn_dispatcher_system,
            spawn_bevscode_peer_on_strategy_editor_added,
        )
            .chain(),
    );

    // PanelKind::StrategyEditor の spawn を要求する。
    app.world_mut()
        .write_message(PanelSpawnRequested {
            kind: PanelKind::StrategyEditor,
            source: backcast::ui::components::PanelSpawnSource::User,
            strategy_spec: None,
        });

    // 2 フレーム回す:
    //   frame 1: dispatcher が root spawn → 同 chain で peer system が Added<StrategyEditorRoot> を観測 → peer spawn
    //   frame 2: 念のためコマンドキューを完全に apply
    app.update();
    app.update();

    // ── cosmic 側 ── 6c 完了で 0 件になる必要がある
    let cosmic_buf_count = app
        .world_mut()
        .query_filtered::<Entity, With<CosmicEditBuffer>>()
        .iter(app.world())
        .count();
    let textedit2d_count = app
        .world_mut()
        .query_filtered::<Entity, With<TextEdit2d>>()
        .iter(app.world())
        .count();
    assert_eq!(
        cosmic_buf_count, 0,
        "Slice 6c: CosmicEditBuffer entity は 0 件のはず (got {cosmic_buf_count})"
    );
    assert_eq!(
        textedit2d_count, 0,
        "Slice 6c: TextEdit2d entity は 0 件のはず (got {textedit2d_count})"
    );

    // ── bevscode 側 ── 6c 完了で 1 件のまま
    let bevscode_node_count = app
        .world_mut()
        .query_filtered::<Entity, With<StrategyEditorNode>>()
        .iter(app.world())
        .count();
    assert_eq!(
        bevscode_node_count, 1,
        "Slice 6c: StrategyEditorNode (bevscode peer) は 1 件のはず (got {bevscode_node_count})"
    );
}
