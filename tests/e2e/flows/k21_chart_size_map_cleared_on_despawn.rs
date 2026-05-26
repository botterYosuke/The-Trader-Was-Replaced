//! K21 chart_size_map_cleared_on_despawn — `instrument_chart_sync_system` が
//! 銘柄チャートを despawn するとき、対応する `ChartSizeMap` エントリを削除することを保証する (kind:state)。
//!
//! 修正前: despawn しても ChartSizeMap のエントリが残り、map が際限なく成長する。
//! 修正後: despawn と同時にエントリを削除する → map がリークしない。
//!
//! RED＝回帰ガード・fix は #43 後に green

use bevy::prelude::*;
use backcast::trading::InstrumentTradingDataMap;
use backcast::ui::components::{ChartSizeMap, InstrumentRegistry};
use backcast::ui::window::instrument_chart_sync_system;

#[test]
fn k21_chart_size_map_cleared_on_despawn() {
    let mut app = App::new();
    app.init_resource::<InstrumentRegistry>();
    app.init_resource::<InstrumentTradingDataMap>();
    app.init_resource::<ChartSizeMap>();
    app.add_systems(Update, instrument_chart_sync_system);

    // ChartSizeMap にカスタムサイズを事前登録
    app.world_mut()
        .resource_mut::<ChartSizeMap>()
        .map
        .insert("1301.TSE".to_string(), Vec2::new(500.0, 320.0));

    // 銘柄を追加 → chart spawn
    {
        let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.replace_all(&["1301.TSE".to_string()]);
    }
    app.update();

    // 事前条件: ChartSizeMap にエントリが存在する
    assert!(
        app.world().resource::<ChartSizeMap>().map.contains_key("1301.TSE"),
        "前提: spawn 前に ChartSizeMap エントリが存在するはず"
    );

    // 銘柄を削除 → chart despawn
    {
        let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.replace_all(&[]);
    }
    app.update();

    // assert: despawn 後は ChartSizeMap エントリが削除されているはず
    assert!(
        !app.world().resource::<ChartSizeMap>().map.contains_key("1301.TSE"),
        "RED: despawn 後は ChartSizeMap から '1301.TSE' エントリが削除されるはず (fix #43)"
    );
}
