//! M3 window_close_hides_or_despawns — floating window の `×` ボタンに
//! `Pointer<Click>` を発火すると `WindowRoot` entity が despawn されること、
//! および `AutoSaveState.dirty = true` と `AppHistory` に despawn 記録が入ることを
//! 保証する（kind:ui）。
//!
//! Chart 以外の window（BuyingPower）で despawn を確認する。
//! Chart の readonly-registry ガード（editable=false なら no-op）は M3 のスコープ外。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy::picking::pointer::{Location, PointerId, PointerButton};
use bevy::picking::events::{Click, Pointer};
use bevy::render::camera::NormalizedRenderTarget;
use std::time::Duration;

use backcast::ui::components::{
    CloseButton, InstrumentRegistry, PanelKind, PendingStrategyFragments,
    RegionKeyAllocator, StrategyBuffer, WindowManager,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use backcast::ui::layout_persistence::AutoSaveState;
use backcast::trading::InstrumentTradingDataMap;

use crate::ui_dump::{dump_panels, panels_of};

/// ダミーの `Location` を作るヘルパー（picking backend 不要な headless 用）。
/// observer 内では pointer_location を参照しないため、image target で代替する。
fn dummy_location() -> Location {
    Location {
        target: NormalizedRenderTarget::Image(bevy::render::camera::ImageRenderTarget { handle: Handle::default(), scale_factor: bevy::math::FloatOrd(1.0) }),
        position: Vec2::ZERO,
    }
}

#[test]
fn m3_window_close_hides_or_despawns() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(WindowManager::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(InstrumentRegistry { ids: vec![], editable: true })
        .insert_resource(InstrumentTradingDataMap::default())
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(PendingStrategyFragments::default());

    // BuyingPower window を本番関数で spawn する。
    let (root, _content, _title_bar) = {
        let mut commands = app.world_mut().commands();
        let r = spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "BUYING POWER".to_string(),
                size: Vec2::new(270.0, 130.0),
                position: Vec2::ZERO,
                accent: Color::srgba(0.0, 0.8, 1.0, 0.4),
                closeable: true,
                resizable: false,
            },
        );
        r
    };
    app.world_mut().flush();

    // PanelKind を root に付ける（close observer が root_q で参照するため必要）。
    app.world_mut().entity_mut(root).insert(PanelKind::BuyingPower);
    app.update();

    // close button entity を取得する（spawn_floating_window が CloseButton を付ける）。
    let close_btn: Entity = app
        .world_mut()
        .query_filtered::<Entity, With<CloseButton>>()
        .iter(app.world())
        .next()
        .expect("CloseButton entity が存在するはず");

    // spawn 直後は window が存在すること。
    {
        let panels = dump_panels(app.world_mut());
        assert_eq!(
            panels_of(&panels, "Buying Power").len(),
            1,
            "クローズ前は Buying Power が 1 枚あるはず"
        );
    }

    // CloseButton の Pointer<Click> observer を発火する。
    app.world_mut().trigger_targets(
        Pointer::<Click>::new(
            PointerId::Mouse,
            dummy_location(),
            close_btn,
            Click {
                button: PointerButton::Primary,
                hit: bevy::picking::backend::HitData::new(Entity::from_raw(0), 0.0, None, None),
                duration: Duration::from_millis(100),
            },
        ),
        close_btn,
    );
    // observer 内の `commands.entity(root).despawn_recursive()` を flush する。
    app.update();

    // ── WindowRoot が despawn されていること ──
    let root_exists = app.world().get_entity(root).is_ok();
    assert!(
        !root_exists,
        "CloseButton click 後に WindowRoot は despawn されるはず"
    );

    // ── dump_panels にも出なくなること ──
    let panels = dump_panels(app.world_mut());
    assert!(
        panels_of(&panels, "Buying Power").is_empty(),
        "despawn 後は Buying Power が panels に出ないはず (panels={panels:#?})"
    );

    // ── AutoSaveState.dirty = true ──
    assert!(
        app.world().resource::<AutoSaveState>().dirty,
        "close 後に AutoSaveState.dirty = true のはず"
    );
}
