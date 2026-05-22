//! J12 instrument_picker_placeholders — 銘柄ピッカーが scenario.end 未設定・取得中・取得失敗・
//! Live venue 未接続・検索結果なしの各状態で正しい placeholder を表示することを保証する（kind:ui）。
//!
//! テストでは `AvailableInstruments` / `Tickers` / `VenueStatusRes` / mode を状態別に注入し、placeholder text を観測する。
//!
//! ## 実装状況
//! - `sync_picker_dropdown_visibility_system`: picker.visible → dropdown Node.display → **実装済み**
//! - `picker_list_rebuild_system`: 各状態ごとの placeholder row → **実装済み**
//!   - end_date 未設定: "Set scenario.end first"
//!   - in_flight: "Loading..."
//!   - last_error (同 end_date): "Error: ..."
//!   - NotFetched (Live): "Venue not connected"
//!   - InFlight (Live): "Loading..."
//!   - Failed (Live): "Error: ..."
//!   - No matches (候補あり + 検索クエリ全除外): "No matches"
//!   - 空 Universe (Replay, by_end_date[end]=[]): "No instruments for this date"（ADR-0002 before_oldest）
//!   - 空 Universe (Live, Tickers Loaded + 空 list): "No instruments in venue"
//! - `InstrumentPickerSearchText` の visible/hidden は `sync_picker_dropdown_visibility_system` が
//!   `InstrumentPickerDropdown` を通して制御するため dropdown Node.display で観測する。

use bevy::prelude::*;
use chrono::NaiveDate;

use backcast::trading::{
    AvailableInstruments, ExecutionMode, ExecutionModeRes, Ticker, Tickers, TickersSource,
    TickersStatus,
};
use backcast::ui::components::{InstrumentRegistry, ScenarioMetadata};
use backcast::ui::instrument_picker::{
    InstrumentPickerDropdown, InstrumentPickerListContainer, InstrumentPickerRow,
    InstrumentPickerState, picker_list_rebuild_system, sync_picker_dropdown_visibility_system,
};

/// 各 placeholder シナリオを通すための最小 App を作るヘルパー。
/// picker は visible=true にセットする（list_rebuild_system は visible でなければ no-op）。
fn make_picker_app() -> (App, Entity, Entity) {
    let mut app = App::new();

    app.insert_resource(InstrumentPickerState {
        visible: true,
        ..Default::default()
    })
    .insert_resource(InstrumentRegistry {
        ids: vec![],
        editable: true,
    })
    .insert_resource(AvailableInstruments::default())
    .insert_resource(ExecutionModeRes {
        mode: ExecutionMode::Replay,
    })
    .insert_resource(Tickers::default())
    .insert_resource(ScenarioMetadata::default());

    app.add_systems(
        Update,
        (
            sync_picker_dropdown_visibility_system,
            picker_list_rebuild_system,
        )
            .chain(),
    );

    // dropdown UI entity（表示 on/off の観測用）
    let dropdown = app
        .world_mut()
        .spawn((
            Node {
                display: Display::None,
                ..Default::default()
            },
            InstrumentPickerDropdown,
        ))
        .id();

    // 候補行の親コンテナ（list_rebuild_system の get_single 用）
    let container = app
        .world_mut()
        .spawn((Node::default(), InstrumentPickerListContainer))
        .id();

    (app, dropdown, container)
}

/// container の子から Text を持つ entity のテキストを収集する。
fn collect_child_texts(app: &mut App, container: Entity) -> Vec<String> {
    let children: Vec<Entity> = app
        .world()
        .get::<Children>(container)
        .map(|c| c.iter().copied().collect())
        .unwrap_or_default();

    let mut texts = Vec::new();
    for child in children {
        // 直接の Text
        if let Some(t) = app.world().get::<Text>(child) {
            texts.push(t.0.clone());
        }
        // 1 段深い子（button → text child の構造）
        if let Some(grandchildren) = app.world().get::<Children>(child) {
            let gc: Vec<Entity> = grandchildren.iter().copied().collect();
            for gc_ent in gc {
                if let Some(t) = app.world().get::<Text>(gc_ent) {
                    texts.push(t.0.clone());
                }
            }
        }
    }
    texts
}

#[test]
fn j12_instrument_picker_placeholders() {
    // ── ケース 1: scenario.end 未設定 → "Set scenario.end first" ────────────────
    {
        let (mut app, dropdown, container) = make_picker_app();
        // end_date は None のまま（ScenarioMetadata.end が None）
        // picker.end_date も None のまま
        app.update();

        assert_eq!(
            app.world().get::<Node>(dropdown).unwrap().display,
            Display::Flex,
            "ケース1: picker visible → dropdown は Flex のはず"
        );
        let row_count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 0, "ケース1: placeholder のみで row なし");

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("scenario.end")),
            "ケース1: 'scenario.end' を含む placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 2: Replay in_flight → "Loading..." ────────────────────────────
    {
        let (mut app, _dropdown, container) = make_picker_app();
        let end_date = NaiveDate::from_ymd_opt(2025, 3, 31).unwrap();
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .end_date = Some(end_date);
        // in_flight に同 date を挿入
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .in_flight
            .insert(end_date);
        app.update();

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("Loading")),
            "ケース2: in_flight → 'Loading...' placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 3: Replay last_error（同 end_date）→ "Error: ..." ────────────
    {
        let (mut app, _dropdown, container) = make_picker_app();
        let end_date = NaiveDate::from_ymd_opt(2025, 3, 31).unwrap();
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .end_date = Some(end_date);
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .last_error = Some((end_date, "timeout".to_string()));
        app.update();

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("Error")),
            "ケース3: last_error → 'Error: ...' placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 4: Live mode, Tickers::NotFetched → "Venue not connected" ────
    {
        let (mut app, _dropdown, container) = make_picker_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::Unknown,
            status: TickersStatus::NotFetched,
        });
        app.update();

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.to_lowercase().contains("venue") || t.to_lowercase().contains("not")),
            "ケース4: NotFetched → venue 未接続 placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 5: Live mode, Tickers::InFlight → "Loading..." ────────────────
    {
        let (mut app, _dropdown, container) = make_picker_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::LiveVenue,
            status: TickersStatus::InFlight,
        });
        app.update();

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("Loading")),
            "ケース5: InFlight → 'Loading...' placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 6: Live mode, Tickers::Failed → "Error: timeout" ─────────────
    {
        let (mut app, _dropdown, container) = make_picker_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::LiveVenue,
            status: TickersStatus::Failed("timeout".to_string()),
        });
        app.update();

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("Error")),
            "ケース6: Failed → 'Error: ...' placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 7: Live mode, Loaded だが検索クエリが一致しない → "No matches" ─
    {
        let (mut app, _dropdown, container) = make_picker_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![Ticker {
                id: "7203.T".to_string(),
                name: "Toyota".to_string(),
                market: "T".to_string(),
            }],
            source: TickersSource::LiveVenue,
            status: TickersStatus::Loaded,
        });
        // 一致しない検索クエリを設定
        app.world_mut().resource_mut::<InstrumentPickerState>().query = "ZZZNOMATCH".to_string();
        app.update();

        let row_count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 0, "ケース7: クエリ不一致 → row なし");

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("No matches")),
            "ケース7: 一致なし → 'No matches' placeholder が表示されるはず (texts={texts:?})"
        );
    }

    // ── ケース 8: picker closed → dropdown が Display::None ─────────────────
    {
        let (mut app, dropdown, _container) = make_picker_app();
        // picker を close 状態にする
        app.world_mut().resource_mut::<InstrumentPickerState>().visible = false;
        app.update();

        assert_eq!(
            app.world().get::<Node>(dropdown).unwrap().display,
            Display::None,
            "ケース8: picker closed → dropdown は Display::None になるはず"
        );
    }

    // ── ケース 9: Replay, by_end_date[end] が空 Universe → "No instruments for this date" ──
    // backend が before_oldest で success=True/ids=[] を返した結果（ADR-0002）。
    // 取得完了済みだが候補が 0 件。検索一致なし("No matches")とは別文言で表すこと。
    {
        let (mut app, _dropdown, container) = make_picker_app();
        let end_date = NaiveDate::from_ymd_opt(1999, 1, 1).unwrap();
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .end_date = Some(end_date);
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .by_end_date
            .insert(end_date, vec![]);
        app.update();

        let row_count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 0, "ケース9: 空 Universe → row なし");

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("No instruments for this date")),
            "ケース9: 空 Universe → 'No instruments for this date' はず (texts={texts:?})"
        );
        assert!(
            !texts.iter().any(|t| t.contains("No matches")),
            "ケース9: 空 Universe を 'No matches'（検索一致なし）と混同しないこと (texts={texts:?})"
        );
    }

    // ── ケース 10: Replay, 候補あり + 検索クエリ全除外 → "No matches" ──────────
    // ケース9（空 Universe）との対比で、(B)検索一致なしは依然 "No matches" であることを pin。
    {
        let (mut app, _dropdown, container) = make_picker_app();
        let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .end_date = Some(end_date);
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .by_end_date
            .insert(end_date, vec!["7203.TSE".to_string()]);
        app.world_mut().resource_mut::<InstrumentPickerState>().query = "ZZZNOMATCH".to_string();
        app.update();

        let texts = collect_child_texts(&mut app, container);
        assert!(
            texts.iter().any(|t| t.contains("No matches")),
            "ケース10: 候補あり+検索不一致 → 'No matches' はず (texts={texts:?})"
        );
        assert!(
            !texts.iter().any(|t| t.contains("No instruments")),
            "ケース10: 検索一致なしを空 Universe 文言と混同しないこと (texts={texts:?})"
        );
    }
}
