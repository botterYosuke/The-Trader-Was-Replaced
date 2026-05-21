//! J14 scenario_schema_normalization — SCENARIO の schema v1 / v2 / v3 と legacy `instrument` 単一文字列・
//! 配列形式を `ScenarioMetadata.instruments` へ正規化し、必須 field を反映することを保証する（kind:integration）。
//!
//! 各バリアントを `parse_scenario_system` に通し、`ScenarioMetadata.instruments` が
//! 正規化された `Vec<String>` になること（単数 → 1 要素リスト）を検証する。

use bevy::prelude::*;

use backcast::ui::components::{
    ScenarioClearedFromFile, ScenarioFileWatchState, ScenarioLoadedFromFile, ScenarioMetadata,
    ScenarioReadTarget,
};
use backcast::ui::scenario_parser::parse_scenario_system;

fn run_parse(json: &str) -> (ScenarioMetadata, usize) {
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("sidecar.json");
    std::fs::write(&json_path, json).unwrap();

    let mut app = App::new();
    app.insert_resource(ScenarioReadTarget(Some(json_path)));
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<ScenarioFileWatchState>();
    app.add_event::<ScenarioLoadedFromFile>();
    app.add_event::<ScenarioClearedFromFile>();
    app.add_systems(Update, parse_scenario_system);
    app.update();

    let meta = app.world().resource::<ScenarioMetadata>().clone();
    let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
    let mut reader = events.get_cursor();
    let count = reader.read(events).count();
    (meta, count)
}

#[test]
fn j14_scenario_schema_normalization() {
    // ── v1: `instrument` 単一文字列 → 1 要素リスト ──
    {
        let (meta, evt) = run_parse(
            r#"{"scenario": {"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}"#,
        );
        assert_eq!(
            meta.instruments,
            vec!["1301.TSE".to_string()],
            "v1 単一 instrument が 1 要素リストに正規化されるはず"
        );
        assert_eq!(meta.schema_version, Some(1));
        assert_eq!(evt, 1, "v1: ScenarioLoadedFromFile が 1 件発火するはず");
    }

    // ── v1 legacy: `instrument` 文字列リスト形式 → そのままリスト ──
    {
        let (meta, evt) = run_parse(
            r#"{"scenario": {"schema_version": 1, "instrument": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}"#,
        );
        assert_eq!(
            meta.instruments,
            vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
            "v1 リスト形式 instrument がそのままリストに正規化されるはず"
        );
        assert_eq!(evt, 1);
    }

    // ── v2: `instruments` 配列 → そのまま ──
    {
        let (meta, evt) = run_parse(
            r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Minute", "initial_cash": 500000}}"#,
        );
        assert_eq!(
            meta.instruments,
            vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
        );
        assert_eq!(meta.granularity.as_deref(), Some("Minute"));
        assert_eq!(meta.initial_cash, Some(500_000));
        assert_eq!(evt, 1);
    }

    // ── v3: `instruments_ref` で解決される ──
    {
        let dir = tempfile::tempdir().unwrap();
        let ref_path = dir.path().join("universe.json");
        std::fs::write(&ref_path, r#"["A.T","B.T","C.T"]"#).unwrap();
        let json_path = dir.path().join("sidecar.json");
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "universe.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(ScenarioReadTarget(Some(json_path)));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(
            meta.instruments,
            vec!["A.T".to_string(), "B.T".to_string(), "C.T".to_string()],
            "v3 instruments_ref が正しくリストに展開されるはず"
        );
        assert_eq!(meta.schema_version, Some(3));

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert_eq!(collected.len(), 1);
        assert!(
            collected[0].ref_path.is_some(),
            "v3 の ref_path は Some になるはず"
        );
    }

    // ── instruments も instrument もない → empty list ──
    {
        let (meta, evt) = run_parse(
            r#"{"scenario": {"schema_version": 2, "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        );
        assert!(
            meta.instruments.is_empty(),
            "instruments も instrument も無いときは空リストになるはず"
        );
        // instrument が空でも ScenarioLoadedFromFile は発火する（instruments フィールドが空なだけ）
        assert_eq!(evt, 1, "instruments 空でも ScenarioLoadedFromFile は発火するはず");
    }
}
