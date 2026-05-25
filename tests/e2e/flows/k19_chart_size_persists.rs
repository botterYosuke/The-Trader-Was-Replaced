//! K19 chart_size_persists — チャートパネルのリサイズ後サイズが銘柄再 spawn で復元されることを保証する
//! (kind:ui)。
//!
//! `ChartSizeMap` に保存済みサイズがある銘柄は、`instrument_chart_sync_system` が spawn する
//! チャートパネルにそのサイズを使う。
//!
//! Slice 3 実装前: spawn_chart_panel は常に CHART_PANEL_SIZE を使う → assert が RED で fail する。
//! Slice 3 実装後: 保存済みサイズが使われる → assert が GREEN で pass する。

use bevy::prelude::*;
use backcast::trading::InstrumentTradingDataMap;
use backcast::ui::chart_viewstate::CHART_PANEL_SIZE;
use backcast::ui::components::{
    ChartInstrument, ChartSizeMap, InstrumentRegistry, WindowRoot,
};
use backcast::ui::window::instrument_chart_sync_system;

#[test]
fn k19_chart_size_persists_across_respawn() {
    let mut app = App::new();
    app.init_resource::<InstrumentRegistry>();
    app.init_resource::<InstrumentTradingDataMap>();
    app.init_resource::<ChartSizeMap>();
    app.add_systems(Update, instrument_chart_sync_system);

    // 保存済みサイズを設定（デフォルト CHART_PANEL_SIZE より大きい）
    let custom_size = Vec2::new(500.0, 320.0);
    app.world_mut()
        .resource_mut::<ChartSizeMap>()
        .map
        .insert("1301.TSE".to_string(), custom_size);

    // 銘柄を登録 → instrument_chart_sync_system が spawn
    {
        let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.replace_all(&["1301.TSE".to_string()]);
    }
    app.update();

    // assert A: spawn されたチャートが保存済みサイズを持つ (RED: Slice 3 前は CHART_PANEL_SIZE)
    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Sprite, &ChartInstrument), With<WindowRoot>>();
        let results: Vec<_> = q.iter(world).collect();
        assert_eq!(results.len(), 1, "1 つのチャートが spawn されるはず");
        assert_eq!(
            results[0].0.custom_size,
            Some(custom_size),
            "RED: spawn された chart root サイズ {:?} は保存済み {:?} であるはず (Slice 3 前は {:?})",
            results[0].0.custom_size,
            custom_size,
            Some(CHART_PANEL_SIZE),
        );
    }

    // assert B: 銘柄を再 spawn（registry remove → re-add）しても同サイズが復元される
    {
        let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.replace_all(&[]);
    }
    app.update();
    {
        let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.replace_all(&["1301.TSE".to_string()]);
    }
    app.update();

    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Sprite, &ChartInstrument), With<WindowRoot>>();
        let results: Vec<_> = q.iter(world).collect();
        assert_eq!(results.len(), 1, "再 spawn 後も 1 つのチャートが存在するはず");
        assert_eq!(
            results[0].0.custom_size,
            Some(custom_size),
            "再 spawn 後も保存済みサイズ {:?} が復元されるはず",
            custom_size,
        );
    }
}
