//! J10 instruments_ref_readonly_sidebar — `instruments_ref` を使うサイドカーを開くと
//! `InstrumentRegistry.editable == false` になることを保証する（kind:integration）。
//!
//! `sync_registry_from_scenario_loaded_system` は `ScenarioLoadedFromFile.ref_path.is_some()`
//! のとき `registry.editable = false` にセットする。これがサイドバーの + Add / Remove 無効化と
//! 警告表示の backing flag であることを検証する。inline instruments のときは editable=true のまま。

use bevy::prelude::*;

use backcast::ui::components::{
    sync_registry_from_scenario_loaded_system, InstrumentRegistry, ScenarioClearedFromFile,
    ScenarioFileWatchState, ScenarioInstrumentsWritebackState, ScenarioLoadedFromFile,
    ScenarioMetadata, ScenarioReadTarget,
};
use backcast::ui::scenario_parser::parse_scenario_system;

fn build_app(json_path: std::path::PathBuf) -> App {
    let mut app = App::new();
    app.insert_resource(ScenarioReadTarget(Some(json_path)));
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<ScenarioFileWatchState>();
    app.init_resource::<InstrumentRegistry>();
    app.init_resource::<ScenarioInstrumentsWritebackState>();
    app.add_event::<ScenarioLoadedFromFile>();
    app.add_event::<ScenarioClearedFromFile>();
    app.add_systems(
        Update,
        (parse_scenario_system, sync_registry_from_scenario_loaded_system).chain(),
    );
    app
}

#[test]
fn j10_instruments_ref_readonly_sidebar() {
    let dir = tempfile::tempdir().unwrap();

    // ── ケース A: instruments_ref あり → editable=false (読み取り専用) ──
    {
        let ref_path = dir.path().join("universe_a.json");
        std::fs::write(&ref_path, r#"["7203.TSE","1301.TSE"]"#).unwrap();

        let json_path = dir.path().join("sidecar_a.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "universe_a.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = build_app(json_path);
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert!(
            !reg.editable,
            "instruments_ref ありのとき editable=false になるはず (readonly sidebar)"
        );
        assert_eq!(
            reg.as_slice(),
            &["7203.TSE".to_string(), "1301.TSE".to_string()],
            "instruments は ref から正しくロードされるはず"
        );
    }

    // ── ケース B: instruments (inline) → editable=true (編集可) ──
    {
        let json_path = dir.path().join("sidecar_b.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = build_app(json_path);
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert!(
            reg.editable,
            "inline instruments では editable=true のままのはず"
        );
        assert_eq!(reg.as_slice(), &["1301.TSE".to_string()]);
    }

    // ── ケース C: instruments_ref で locked 後に inline sidecar へ切替 → editable=true に戻る ──
    // sync_registry_from_scenario_cleared_system がトリガーされる経路
    {
        use backcast::ui::components::sync_registry_from_scenario_cleared_system;

        let ref_path = dir.path().join("universe_c.json");
        std::fs::write(&ref_path, r#"["7203.TSE"]"#).unwrap();

        let locked_json = dir.path().join("locked.json");
        std::fs::write(
            &locked_json,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "universe_c.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let legacy_json = dir.path().join("legacy.json");
        std::fs::write(&legacy_json, r#"{"window_layout": []}"#).unwrap();

        let mut app = App::new();
        app.insert_resource(ScenarioReadTarget(Some(locked_json)));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.init_resource::<InstrumentRegistry>();
        app.init_resource::<ScenarioInstrumentsWritebackState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(
            Update,
            (
                parse_scenario_system,
                sync_registry_from_scenario_loaded_system,
                sync_registry_from_scenario_cleared_system,
            )
                .chain(),
        );

        // tick 1: locked sidecar → editable=false
        app.update();
        assert!(
            !app.world().resource::<InstrumentRegistry>().editable,
            "tick1: locked sidecar で editable=false になるはず"
        );

        // legacy sidecar に切替
        app.world_mut()
            .insert_resource(ScenarioReadTarget(Some(legacy_json)));
        app.update();

        assert!(
            app.world().resource::<InstrumentRegistry>().editable,
            "tick2: scenario なし sidecar に切替後 editable=true に戻るはず"
        );
    }
}
