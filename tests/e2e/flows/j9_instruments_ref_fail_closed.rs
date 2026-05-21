//! J9 instruments_ref_fail_closed — schema v3 の `instruments_ref` が存在しない・壊れている・
//! pointer 不正・空配列のときシナリオを読み込まず、Run を有効化しないことを保証する（kind:integration）。
//!
//! `resolve_instruments_ref` が None を返す 4 つのケースを parse seam に通し、
//! `ScenarioLoadedFromFile` が発火しないことと `ScenarioMetadata.instruments` が空のままであることを検証する。

use bevy::prelude::*;

use backcast::ui::components::{
    ScenarioClearedFromFile, ScenarioFileWatchState, ScenarioLoadedFromFile, ScenarioMetadata,
    ScenarioReadTarget,
};
use backcast::ui::scenario_parser::parse_scenario_system;

/// 共通: parse_scenario_system だけを持つ最小 App を返す。
fn build_app(json_path: std::path::PathBuf) -> App {
    let mut app = App::new();
    app.insert_resource(ScenarioReadTarget(Some(json_path)));
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<ScenarioFileWatchState>();
    app.add_event::<ScenarioLoadedFromFile>();
    app.add_event::<ScenarioClearedFromFile>();
    app.add_systems(Update, parse_scenario_system);
    app
}

/// イベントストアから ScenarioLoadedFromFile の件数を読む。
fn count_loaded_events(app: &App) -> usize {
    let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
    let mut reader = events.get_cursor();
    reader.read(events).count()
}

#[test]
fn j9_instruments_ref_fail_closed() {
    let dir = tempfile::tempdir().unwrap();

    // ── ケース 1: 参照先ファイルが存在しない ──
    {
        let json_path = dir.path().join("case1.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "missing_universe.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = build_app(json_path);
        app.update();

        assert_eq!(
            count_loaded_events(&app),
            0,
            "ケース1: missing ref → ScenarioLoadedFromFile が発火しないはず"
        );
        assert!(
            app.world()
                .resource::<ScenarioMetadata>()
                .instruments
                .is_empty(),
            "ケース1: instruments は空のまま"
        );
    }

    // ── ケース 2: 参照先ファイルが壊れた JSON ──
    {
        let ref_path = dir.path().join("corrupt_universe.json");
        std::fs::write(&ref_path, b"\x00\xff{not valid json").unwrap();
        let json_path = dir.path().join("case2.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "corrupt_universe.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = build_app(json_path);
        app.update();

        assert_eq!(
            count_loaded_events(&app),
            0,
            "ケース2: corrupt JSON ref → ScenarioLoadedFromFile が発火しないはず"
        );
    }

    // ── ケース 3: JSON ポインタが不正（キーが存在しない） ──
    {
        let ref_path = dir.path().join("ptr_universe.json");
        // {"data": [...]} なのにポインタは /nonexistent を指す
        std::fs::write(&ref_path, r#"{"data": ["1301.TSE"]}"#).unwrap();
        let json_path = dir.path().join("case3.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "ptr_universe.json#/nonexistent", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = build_app(json_path);
        app.update();

        assert_eq!(
            count_loaded_events(&app),
            0,
            "ケース3: bad pointer → ScenarioLoadedFromFile が発火しないはず"
        );
    }

    // ── ケース 4: 空配列 `[]` → instruments が 0 件 → fail-closed ──
    {
        let ref_path = dir.path().join("empty_universe.json");
        std::fs::write(&ref_path, r#"[]"#).unwrap();
        let json_path = dir.path().join("case4.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "empty_universe.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = build_app(json_path);
        app.update();

        assert_eq!(
            count_loaded_events(&app),
            0,
            "ケース4: empty array ref → ScenarioLoadedFromFile が発火しないはず"
        );
        assert!(
            app.world()
                .resource::<ScenarioMetadata>()
                .instruments
                .is_empty(),
            "ケース4: instruments は空のまま"
        );
    }
}
