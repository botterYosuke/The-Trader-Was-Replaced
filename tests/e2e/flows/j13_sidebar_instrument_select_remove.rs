//! J13 sidebar_instrument_select_remove — Instruments 行クリックで選択銘柄が切り替わり、
//! x ボタンで銘柄を削除し、空になったら `No instruments` を表示することを保証する（kind:ui）。
//!
//! テストでは row click / remove button interaction を注入し、selected symbol / registry / sidebar rows を観測する。
//!
//! ## 実装状況
//! - `instrument_row_click_system`: SidebarInstrumentRowClick Pressed → SelectedSymbol 更新 → **実装済み**
//!   - Replay mode: SelectedSymbol のみ更新（SubscribeMarketData は送らない）
//! - `instrument_remove_button_system`: SidebarInstrumentRemoveButton Pressed → InstrumentRegistry.remove → **実装済み**
//!   - editable=false のときは no-op
//! - `update_sidebar_system`: InstrumentRegistry.is_changed() → SidebarInstrumentsList を再構築 → **実装済み**
//!   - ids.is_empty() → "No instruments" テキストを spawn

use bevy::prelude::*;

use backcast::trading::{ExecutionMode, ExecutionModeRes, SelectedSymbol};
use backcast::ui::components::{
    InstrumentRegistry, SidebarInstrumentRemoveButton, SidebarInstrumentRow,
    SidebarInstrumentRowClick, SidebarInstrumentsList, SidebarRoot,
};
use backcast::ui::sidebar::{instrument_remove_button_system, instrument_row_click_system, update_sidebar_system};

// ── 共通ヘルパー ─────────────────────────────────────────────────────────────

/// row click system に必要な最小 App を構築する。
fn make_click_app(ids: Vec<String>, editable: bool) -> App {
    let mut app = App::new();
    app.init_resource::<backcast::ui::theme::Theme>();
    app.insert_resource(SelectedSymbol { id: None })
        .insert_resource(InstrumentRegistry { ids, editable })
        .insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });

    app.add_systems(Update, instrument_row_click_system);
    app
}

/// remove button system に必要な最小 App を構築する。
/// `update_sidebar_system` もチェーンして sidebar 再描画まで確認する。
fn make_remove_app(ids: Vec<String>, editable: bool) -> App {
    let mut app = App::new();
    app.init_resource::<backcast::ui::theme::Theme>();
    app.insert_resource(InstrumentRegistry { ids, editable })
        .insert_resource(SelectedSymbol { id: None })
        .insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });

    // update_sidebar_system が SidebarRoot / SidebarInstrumentsList を必要とする。
    // spawn して system に渡す。
    app.world_mut()
        .spawn((Node::default(), SidebarRoot))
        .with_children(|parent| {
            parent.spawn((Node::default(), SidebarInstrumentsList));
        });

    app.add_systems(
        Update,
        (instrument_remove_button_system, update_sidebar_system).chain(),
    );
    app
}

// ── テスト ───────────────────────────────────────────────────────────────────

#[test]
fn j13_sidebar_instrument_select_remove() {
    // ── Phase A: 行クリックで SelectedSymbol が更新されること ────────────────────
    {
        let mut app = make_click_app(
            vec!["7203.TSE".to_string(), "9984.TSE".to_string()],
            true,
        );

        // 7203 をクリック
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRowClick {
                instrument_id: "7203.TSE".to_string(),
            },
        ));
        app.update();

        assert_eq!(
            app.world().resource::<SelectedSymbol>().id.as_deref(),
            Some("7203.TSE"),
            "Phase A: 行クリックで 7203.TSE が SelectedSymbol になるはず"
        );

        // 9984 をクリックすると切り替わること
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRowClick {
                instrument_id: "9984.TSE".to_string(),
            },
        ));
        app.update();

        assert_eq!(
            app.world().resource::<SelectedSymbol>().id.as_deref(),
            Some("9984.TSE"),
            "Phase A: 行クリックで 9984.TSE に切り替わるはず"
        );
    }

    // ── Phase B: remove ボタンで InstrumentRegistry から削除されること ──────────
    {
        let mut app = make_remove_app(
            vec!["7203.TSE".to_string(), "9984.TSE".to_string()],
            true,
        );

        // 初回 update: InstrumentRegistry が is_changed() のため sidebar が構築される
        app.update();

        // SidebarInstrumentRow が 2 体あること
        let row_count = app
            .world_mut()
            .query::<&SidebarInstrumentRow>()
            .iter(app.world())
            .count();
        assert_eq!(
            row_count, 2,
            "Phase B: 2 件の row が spawn されているはず (got {row_count})"
        );

        // 7203 の × ボタンを押す
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRemoveButton {
                instrument_id: "7203.TSE".to_string(),
            },
        ));
        app.update();

        let registry = app.world().resource::<InstrumentRegistry>();
        assert!(
            !registry.contains("7203.TSE"),
            "Phase B: × ボタンで 7203.TSE が registry から削除されるはず (ids={:?})",
            registry.ids
        );
        assert!(
            registry.contains("9984.TSE"),
            "Phase B: 9984.TSE は残るはず (ids={:?})",
            registry.ids
        );
    }

    // ── Phase C: registry が空になったら "No instruments" テキストが spawn ────
    {
        let mut app = make_remove_app(vec!["7203.TSE".to_string()], true);
        // 初回 update で sidebar 構築
        app.update();

        // 1 件のみの row が存在すること
        let row_count = app
            .world_mut()
            .query::<&SidebarInstrumentRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 1, "Phase C: 1 件の row がある (got {row_count})");

        // × ボタンを押して 0 件にする
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRemoveButton {
                instrument_id: "7203.TSE".to_string(),
            },
        ));
        app.update();

        let registry = app.world().resource::<InstrumentRegistry>();
        assert!(
            registry.ids.is_empty(),
            "Phase C: 削除後 registry は空のはず (ids={:?})",
            registry.ids
        );

        // update_sidebar_system が再描画し "No instruments" テキストを spawn するはず
        // SidebarInstrumentRow は 0 件
        let row_count = app
            .world_mut()
            .query::<&SidebarInstrumentRow>()
            .iter(app.world())
            .count();
        assert_eq!(
            row_count, 0,
            "Phase C: 空になったら row は 0 件のはず (got {row_count})"
        );

        // "No instruments" テキストが世界のどこかに存在するか確認
        let has_no_instruments = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .any(|t| t.0.contains("No instruments"));
        assert!(
            has_no_instruments,
            "Phase C: registry が空のとき 'No instruments' テキストが spawn されるはず"
        );
    }

    // ── Phase D: editable=false のとき × ボタンは no-op ────────────────────────
    {
        let mut app = make_remove_app(vec!["7203.TSE".to_string()], false);
        app.update();

        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRemoveButton {
                instrument_id: "7203.TSE".to_string(),
            },
        ));
        app.update();

        let registry = app.world().resource::<InstrumentRegistry>();
        assert!(
            registry.contains("7203.TSE"),
            "Phase D: editable=false のとき × は no-op であるはず (ids={:?})",
            registry.ids
        );
    }

    // ── Phase E: remove ボタンは row click を発火しない ─────────────────────────
    {
        // instrument_row_click_system だけが走っている状態でも SelectedSymbol は変わらない
        let mut app = make_click_app(vec!["7203.TSE".to_string()], true);

        // RemoveButton を spawn（RowClick マーカーなし）
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRemoveButton {
                instrument_id: "7203.TSE".to_string(),
            },
        ));
        app.update();

        assert_eq!(
            app.world().resource::<SelectedSymbol>().id,
            None,
            "Phase E: RemoveButton は SelectedSymbol を変えないはず"
        );
    }
}
