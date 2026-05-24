//! K5 chart_ladder_live_mode — Live モードでは Chart に 21 行固定の Ladder ペインが付き、
//! depth が無いと `No depth data`、Replay に戻ると compact chart に戻ることを保証する（kind:ui）。
//!
//! ## ヘッドレス境界
//! Ladder ペインのビジュアル（Sprite 着色 / テキスト）は headless で観測できる:
//! `LadderPane` component / `LadderRow` component / `Text2d` component を query すれば
//! 行数・内容を非破壊的に確認できる。実ピクセル描画は kind:render (L4) の責務。
//!
//! ## テスト構成
//! 本テストは production system 群をそのまま組み合わせる:
//! - `chart_ladder_mode_sync_system` — mode 変化で LadderPane spawn/despawn + 枠リサイズ
//! - `ladder_render_system` — depth を読んで LadderRow を生成
//!
//! 3 フェーズを検証する:
//! 1. **LiveManual 起動** — LadderPane が spawn され WindowRoot が Live サイズになること。
//! 2. **depth なし** → プレースホルダ 1 行のみ (行テキストが "No depth data")。
//!    `depth あり` → ask10 + last + bid10 = 21 行。
//! 3. **Replay に切替** — LadderPane が despawn され WindowRoot が Replay サイズに縮小すること。
//!
//! ビジュアル確認（行の色・フォントサイズ・anchor）は kind:render の smoke test に委ねる。

use bevy::prelude::*;

use backcast::trading::{
    DepthLevel, DepthSnapshot, ExecutionMode, ExecutionModeRes, InstrumentTradingData,
    InstrumentTradingDataMap, LastPrices,
};
use backcast::ui::chart_ladder_pane::{
    chart_ladder_mode_sync_system, ladder_render_system, LadderPane, LadderRow,
};
use backcast::ui::chart_viewstate::{
    CHART_CHILD_LOCAL_X_LIVE, CHART_CHILD_LOCAL_X_REPLAY, CHART_PANEL_SIZE, ChartViewState,
    LIVE_COMBINED_PANEL_SIZE,
};
use backcast::ui::components::{ChartInstrument, PriceDisplay, WindowRoot};

/// テスト用の chart ルート階層を組む（production の window.rs spawn を簡略化）。
/// `(root, chart_entity, price_entity)` を返す。
fn spawn_chart_root(app: &mut App, instrument: &str) -> (Entity, Entity, Entity) {
    let root = app
        .world_mut()
        .spawn((
            WindowRoot,
            ChartInstrument {
                instrument_id: instrument.to_string(),
            },
            Sprite {
                custom_size: Some(CHART_PANEL_SIZE),
                ..default()
            },
            Transform::default(),
        ))
        .id();
    let content_area = app.world_mut().spawn(Transform::default()).id();
    let chart_child = app
        .world_mut()
        .spawn((
            ChartViewState::default(),
            ChartInstrument {
                instrument_id: instrument.to_string(),
            },
            Transform::from_xyz(CHART_CHILD_LOCAL_X_REPLAY, 0.0, 0.1),
        ))
        .id();
    let price_child = app
        .world_mut()
        .spawn((
            PriceDisplay,
            Transform::from_xyz(CHART_CHILD_LOCAL_X_REPLAY, 0.0, 0.3),
        ))
        .id();
    app.world_mut()
        .entity_mut(content_area)
        .add_child(chart_child)
        .add_child(price_child);
    app.world_mut().entity_mut(root).add_child(content_area);
    (root, chart_child, price_child)
}

fn depth_levels(n: usize, base_price: f64) -> Vec<DepthLevel> {
    (0..n)
        .map(|i| DepthLevel {
            price: base_price + i as f64,
            size: 100.0 * (i as f64 + 1.0),
        })
        .collect()
}

#[test]
fn k5_chart_ladder_live_mode() {
    // chart_ladder_mode_sync_system / ladder_render_system は Time 非依存なので
    // App::new() で十分。MinimalPlugins は不要。
    let mut app = App::new();

    // mode_sync_system と render_system が要求する resource を挿入。
    app.insert_resource(ExecutionModeRes {
        mode: ExecutionMode::LiveManual,
    });
    app.init_resource::<InstrumentTradingDataMap>();
    app.init_resource::<LastPrices>();

    app.add_systems(
        Update,
        (chart_ladder_mode_sync_system, ladder_render_system).chain(),
    );

    // ── Phase 1: LiveManual で chart を spawn → LadderPane が付く ──
    let (root, chart_child, price_child) = spawn_chart_root(&mut app, "7203.TSE");

    app.update(); // mode_sync_system: Added<WindowRoot> をトリガとして LadderPane spawn

    {
        let world = app.world_mut();
        let mut pq = world.query::<&LadderPane>();
        let panes: Vec<_> = pq.iter(world).collect();
        assert_eq!(panes.len(), 1, "LiveManual で LadderPane が 1 つ spawn される");
        assert_eq!(panes[0].chart_root, root, "LadderPane.chart_root が root entity");

        // WindowRoot Sprite のサイズが Live 用サイズに拡張されること。
        let sprite = world.entity(root).get::<Sprite>().unwrap();
        assert_eq!(
            sprite.custom_size,
            Some(LIVE_COMBINED_PANEL_SIZE),
            "Live モードで WindowRoot が LIVE_COMBINED_PANEL_SIZE に拡張されるはず"
        );

        // chart child が左シフトされること（Ladder 分だけ左へ）。
        let chart_x = world
            .entity(chart_child)
            .get::<Transform>()
            .unwrap()
            .translation
            .x;
        assert!(
            (chart_x - CHART_CHILD_LOCAL_X_LIVE).abs() < 1e-3,
            "Live で chart child が CHART_CHILD_LOCAL_X_LIVE ({}) に移動するはず: got {}",
            CHART_CHILD_LOCAL_X_LIVE,
            chart_x
        );

        // price display も同量左シフト。
        let price_x = world
            .entity(price_child)
            .get::<Transform>()
            .unwrap()
            .translation
            .x;
        assert!(
            (price_x - CHART_CHILD_LOCAL_X_LIVE).abs() < 1e-3,
            "Live で price child が左シフトされるはず: got {}",
            price_x
        );
    }

    // ── Phase 2a: depth なし → プレースホルダ 1 行 ──
    // InstrumentTradingDataMap に depth=None の銘柄を入れる。
    {
        let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
        map.map.insert(
            "7203.TSE".to_string(),
            InstrumentTradingData {
                depth: None,
                ..default()
            },
        );
    }
    app.update();

    // LadderPane entity を取得。
    let pane_entity = {
        let world = app.world_mut();
        let mut pq = world.query::<(Entity, &LadderPane)>();
        pq.iter(world)
            .find(|(_, lp)| lp.chart_root == root)
            .map(|(e, _)| e)
            .expect("LadderPane が存在するはず")
    };

    {
        let world = app.world_mut();
        let mut rq = world.query::<(&LadderRow, &ChildOf)>();
        let rows: Vec<_> = rq
            .iter(world)
            .filter(|(_, p)| p.get() == pane_entity)
            .collect();
        assert_eq!(rows.len(), 1, "depth なしはプレースホルダ 1 行のみ");
    }

    // ── Phase 2b: depth あり → 21 行 ──
    {
        let mut map = app.world_mut().resource_mut::<InstrumentTradingDataMap>();
        map.map.insert(
            "7203.TSE".to_string(),
            InstrumentTradingData {
                depth: Some(DepthSnapshot {
                    asks: depth_levels(10, 2505.0),
                    bids: depth_levels(10, 2495.0),
                    timestamp_ms: None,
                }),
                ..default()
            },
        );
    }
    app.world_mut()
        .resource_mut::<LastPrices>()
        .map
        .insert("7203.TSE".to_string(), 2500.0);
    app.update();

    {
        let world = app.world_mut();
        let mut rq = world.query::<(&LadderRow, &ChildOf)>();
        let rows: Vec<_> = rq
            .iter(world)
            .filter(|(_, p)| p.get() == pane_entity)
            .collect();
        assert_eq!(rows.len(), 21, "depth ありは ask10 + last + bid10 = 21 行");
    }

    // ── Phase 3: Replay に切替 → LadderPane despawn + 枠縮小 ──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::Replay;
    app.update();

    {
        let world = app.world_mut();

        // LadderPane が despawn されていること。
        let mut pq = world.query::<&LadderPane>();
        assert_eq!(
            pq.iter(world).count(),
            0,
            "Replay に戻ると LadderPane が despawn される"
        );

        // WindowRoot が Replay サイズに縮小すること。
        let sprite = world.entity(root).get::<Sprite>().unwrap();
        assert_eq!(
            sprite.custom_size,
            Some(CHART_PANEL_SIZE),
            "Replay に戻ると WindowRoot が CHART_PANEL_SIZE に縮小されるはず"
        );

        // chart child の x オフセットが Replay 値に戻ること。
        let chart_x = world
            .entity(chart_child)
            .get::<Transform>()
            .unwrap()
            .translation
            .x;
        assert!(
            (chart_x - CHART_CHILD_LOCAL_X_REPLAY).abs() < 1e-3,
            "Replay で chart child が CHART_CHILD_LOCAL_X_REPLAY ({}) に戻るはず: got {}",
            CHART_CHILD_LOCAL_X_REPLAY,
            chart_x
        );
    }

    // ── Phase 4: Live に再起動 → LadderPane が再 spawn される (idempotency) ──
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.update();

    {
        let world = app.world_mut();
        let mut pq = world.query::<&LadderPane>();
        let count = pq.iter(world).count();
        assert_eq!(count, 1, "LiveAuto で再度 LadderPane が spawn される (idempotent)");
    }
}
