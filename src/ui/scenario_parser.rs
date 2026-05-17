use crate::ui::components::{
    ScenarioClearedFromFile, ScenarioFileWatchState, ScenarioLoadedFromFile, ScenarioMetadata,
    StrategyBuffer,
};
use bevy::prelude::*;
use serde::Deserialize;

/// サイドカー JSON のルート構造。`scenario` キー以外は無視する。
#[derive(Deserialize)]
struct SidecarRoot {
    #[serde(default)]
    scenario: Option<ScenarioFile>,
}

/// `scenario` キー内部の構造（v1/v2/v3 共通）。
#[derive(Deserialize)]
struct ScenarioFile {
    schema_version: Option<u32>,
    /// v1: 単一文字列 / v2 で単数キーを使うレガシー: 文字列またはリスト
    #[serde(default)]
    instrument: Option<StringOrList>,
    /// v2/v3: 複数銘柄リスト（正規化済みキー）
    #[serde(default)]
    instruments: Option<Vec<String>>,
    start: Option<String>,
    end: Option<String>,
    granularity: Option<String>,
    initial_cash: Option<i64>,
}

/// JSON の文字列 / 文字列リスト の両方を deserialize できる enum。
#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrList {
    One(String),
    Many(Vec<String>),
}

/// `original_path` が変化したときだけサイドカー JSON を再読み込みして
/// `ScenarioMetadata` を更新するシステム。
///
/// - ファイル不在 / "scenario" キーなし / JSON 破損 → `ScenarioMetadata::default()`（Run ボタングレーアウト）
pub fn parse_scenario_system(
    buffer: Res<StrategyBuffer>,
    mut scenario: ResMut<ScenarioMetadata>,
    mut watch: ResMut<ScenarioFileWatchState>,
    mut loaded_events: EventWriter<ScenarioLoadedFromFile>,
    mut cleared_events: EventWriter<ScenarioClearedFromFile>,
) {
    let current_path = buffer.original_path.clone();
    let current_mtime = current_path
        .as_ref()
        .map(|p| p.with_extension("json"))
        .and_then(|jp| std::fs::metadata(&jp).ok())
        .and_then(|m| m.modified().ok());
    if watch.last_path == current_path && watch.last_mtime == current_mtime {
        return;
    }
    watch.last_path = current_path.clone();
    watch.last_mtime = current_mtime;

    let Some(py_path) = current_path else {
        cleared_events.send(ScenarioClearedFromFile { source_path: None });
        *scenario = ScenarioMetadata::default();
        return;
    };

    let json_path = py_path.with_extension("json");

    let text = match crate::ui::layout_persistence::read_json_with_bom_strip(&json_path) {
        Ok(t) => t,
        Err(e) => {
            debug!(
                "no sidecar JSON for {:?}: {} — ScenarioMetadata reset",
                json_path, e
            );
            cleared_events.send(ScenarioClearedFromFile {
                source_path: Some(json_path.clone()),
            });
            *scenario = ScenarioMetadata::default();
            return;
        }
    };

    let root: SidecarRoot = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            warn!("malformed sidecar JSON {:?}: {} — ScenarioMetadata reset", json_path, e);
            cleared_events.send(ScenarioClearedFromFile {
                source_path: Some(json_path.clone()),
            });
            *scenario = ScenarioMetadata::default();
            return;
        }
    };

    let Some(sf) = root.scenario else {
        debug!(
            "no 'scenario' key in {:?} — ScenarioMetadata reset",
            json_path
        );
        cleared_events.send(ScenarioClearedFromFile {
            source_path: Some(json_path.clone()),
        });
        *scenario = ScenarioMetadata::default();
        return;
    };

    let instruments: Vec<String> = if let Some(list) = sf.instruments {
        list
    } else if let Some(sol) = sf.instrument {
        match sol {
            StringOrList::One(s) => vec![s],
            StringOrList::Many(v) => v,
        }
    } else {
        vec![]
    };

    let has_instruments_ref = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("scenario")
                .and_then(|s| s.get("instruments_ref"))
                .map(|_| true)
        })
        .unwrap_or(false);

    let new_meta = ScenarioMetadata {
        schema_version: sf.schema_version,
        instruments: instruments.clone(),
        start: sf.start,
        end: sf.end.clone(),
        granularity: sf.granularity,
        initial_cash: sf.initial_cash,
    };

    info!(
        "SCENARIO parsed from sidecar: schema_version={:?} instruments={:?} start={:?} end={:?} granularity={:?} initial_cash={:?} has_instruments_ref={}",
        new_meta.schema_version,
        new_meta.instruments,
        new_meta.start,
        new_meta.end,
        new_meta.granularity,
        new_meta.initial_cash,
        has_instruments_ref,
    );

    *scenario = new_meta;

    loaded_events.send(ScenarioLoadedFromFile {
        source_path: json_path,
        instruments,
        end: sf.end,
        has_instruments_ref,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::ScenarioLoadedFromFile;

    #[test]
    fn test_parse_v1_from_json() {
        let json = r#"{"scenario": {"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}"#;
        let root: SidecarRoot = serde_json::from_str(json).unwrap();
        let sf = root.scenario.unwrap();
        assert_eq!(sf.schema_version, Some(1));
        assert!(
            matches!(sf.instrument, Some(StringOrList::One(ref s)) if s == "1301.TSE"),
            "expected single instrument string"
        );
    }

    #[test]
    fn test_parse_v2_from_json() {
        let json = r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Minute", "initial_cash": 1000000}}"#;
        let root: SidecarRoot = serde_json::from_str(json).unwrap();
        let sf = root.scenario.unwrap();
        assert_eq!(
            sf.instruments,
            Some(vec!["1301.TSE".to_string(), "7203.TSE".to_string()])
        );
    }

    #[test]
    fn test_parse_pair_multi() {
        let json = r#"{"scenario": {"schema_version": 2, "instruments": ["A", "B"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Minute", "initial_cash": 1000000}}"#;
        let root: SidecarRoot = serde_json::from_str(json).unwrap();
        let sf = root.scenario.unwrap();
        assert_eq!(sf.instruments.unwrap().len(), 2);
    }

    #[test]
    fn test_missing_sidecar_returns_default() {
        // ファイルが存在しない場合のシステム動作は integration test。
        // ここでは "scenario" キーなしの JSON で None になることを確認。
        let json = r#"{}"#;
        let root: SidecarRoot = serde_json::from_str(json).unwrap();
        assert!(root.scenario.is_none());
    }

    #[test]
    fn test_sidecar_without_scenario_key_returns_default() {
        // layout-only の旧 JSON でも正常 deserialize でき、scenario は None
        let json = r#"{"schema_version": 1, "viewport": {}, "windows": []}"#;
        let root: SidecarRoot = serde_json::from_str(json).unwrap();
        assert!(root.scenario.is_none());
    }

    #[test]
    fn test_malformed_json_returns_default_and_warns() {
        let result = serde_json::from_str::<SidecarRoot>("{not valid");
        assert!(result.is_err());
    }

    #[test]
    fn test_v3_resolved_instruments_works() {
        // 事前解決済みの instruments リスト付き v3 は GUI 対応
        let json = r#"{"scenario": {"schema_version": 3, "instruments": ["1301.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
        let root: SidecarRoot = serde_json::from_str(json).unwrap();
        let sf = root.scenario.unwrap();
        assert_eq!(sf.instruments, Some(vec!["1301.TSE".to_string()]));
    }

    /// Integration-style: tempdir に <stem>.py と <stem>.json を置き、
    /// StrategyBuffer.original_path をセットして `parse_scenario_system` を 1 tick 回す。
    /// instruments が正しく ScenarioMetadata に反映されることを確認する。
    #[test]
    fn test_system_parses_instruments_from_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE", "7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.schema_version, Some(2));
        assert_eq!(meta.instruments, vec!["1301.TSE".to_string(), "7203.TSE".to_string()]);
        assert_eq!(meta.granularity.as_deref(), Some("Daily"));
    }

    /// v1 単数 instrument が 1 要素 list に正規化されることを system 経由で確認。
    #[test]
    fn test_system_normalizes_v1_single_instrument() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 1, "instrument": "1301.TSE", "start": "2025-01-06", "end": "2025-03-31", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.instruments, vec!["1301.TSE".to_string()]);
    }

    /// sidecar JSON 不在の場合 ScenarioMetadata がデフォルト（instruments 空）に
    /// リセットされることを system 経由で確認。
    #[test]
    fn test_system_resets_when_sidecar_missing() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("no_sidecar.py");
        std::fs::write(&py_path, "# dummy").unwrap();
        // .json は作らない

        let mut app = App::new();
        // 事前に instruments が詰まった状態を入れておき、reset されることを確認
        app.insert_resource(ScenarioMetadata {
            schema_version: Some(99),
            instruments: vec!["STALE".to_string()],
            start: Some("old".to_string()),
            end: None,
            granularity: None,
            initial_cash: None,
        });
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.schema_version, None);
        assert!(meta.instruments.is_empty());
        assert!(meta.start.is_none());
    }

    // ===== Step 2 Red tests: event emission + watch state + instruments_ref =====

    /// 初回読込時に `ScenarioLoadedFromFile` が 1 回発火し、
    /// instruments/end/source_path がそのまま乗ることを確認。
    #[test]
    fn test_system_emits_loaded_event_on_first_read() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert_eq!(collected.len(), 1, "expected exactly one ScenarioLoadedFromFile event on first read");
        let ev = &collected[0];
        assert_eq!(ev.source_path, py_path.with_extension("json"));
        assert_eq!(ev.instruments, vec!["1301.TSE".to_string()]);
        assert_eq!(ev.end.as_deref(), Some("2025-01-10"));
        assert!(!ev.has_instruments_ref, "plain instruments list must not set has_instruments_ref");
    }

    /// mtime 不変なら 2 tick 目以降は再発火しないこと (ScenarioFileWatchState の Resource 格上げ確認)。
    #[test]
    fn test_system_does_not_reemit_when_mtime_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 2, "instruments": ["A"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);

        app.update(); // 1 回目: 発火
        app.world_mut().resource_mut::<Events<ScenarioLoadedFromFile>>().clear();
        app.update(); // 2 回目: 発火してはいけない

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert!(collected.is_empty(), "no re-emit expected when mtime unchanged, got {} events", collected.len());
    }

    /// sidecar JSON の scenario 直下に `instruments_ref` キーがあれば
    /// 発火イベントの `has_instruments_ref = true` になること。
    #[test]
    fn test_system_detects_instruments_ref_key() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments": ["1301.TSE"], "instruments_ref": "universe/foo.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_event::<ScenarioLoadedFromFile>();
        app.add_event::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert_eq!(collected.len(), 1);
        assert!(collected[0].has_instruments_ref, "instruments_ref key must set has_instruments_ref=true");
    }

    // ===== §9.2 Red test: registry.editable leak across sidecar switch =====

    /// Fixture B (instruments_ref locked) を読んで registry.editable=false に
    /// 落ちた後、scenario キー不在の別 sidecar を Open すると
    /// `ScenarioLoadedFromFile` が発火しないため `editable=false` が残存することを
    /// schedule レベルで再現する。修正後 (ScenarioClearedFromFile 発火 +
    /// sync 側で editable=true 復元) で PASS になる想定。
    #[test]
    fn test_editable_resets_to_true_when_switching_to_sidecar_without_scenario() {
        use crate::ui::components::{
            InstrumentRegistry, ScenarioClearedFromFile, ScenarioInstrumentsWritebackState,
            sync_registry_from_scenario_cleared_system, sync_registry_from_scenario_loaded_system,
        };

        let dir = tempfile::tempdir().unwrap();

        // sidecar A: instruments_ref ありで editable=false に落とす
        let py_a = dir.path().join("locked.py");
        let json_a = dir.path().join("locked.json");
        std::fs::write(&py_a, "# dummy").unwrap();
        std::fs::write(
            &json_a,
            r#"{"scenario": {"schema_version": 3, "instruments": ["1301.TSE"], "instruments_ref": "universe/foo.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        // sidecar B: scenario キーなしの legacy layout JSON
        let py_b = dir.path().join("legacy.py");
        let json_b = dir.path().join("legacy.json");
        std::fs::write(&py_b, "# dummy").unwrap();
        std::fs::write(&json_b, r#"{"window_layout": []}"#).unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_a.clone()),
            cache_path: None,
            last_merged_source: None,
        });
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

        // tick 1: locked sidecar を読み editable=false が伝播する
        app.update();
        {
            let reg = app.world().resource::<InstrumentRegistry>();
            assert!(!reg.editable, "precondition: instruments_ref で editable=false に落ちる");
        }

        // sidecar B へ切り替え (StrategyBuffer.original_path を差し替え)
        app.world_mut().resource_mut::<StrategyBuffer>().original_path = Some(py_b.clone());

        // tick 2: parse_scenario_system は scenario キー不在で event を出さない
        //         → sync system が呼ばれず editable=false が残存 (= 現状のバグ)
        app.update();

        let reg = app.world().resource::<InstrumentRegistry>();
        assert!(
            reg.editable,
            "scenario なし sidecar に切り替えたら editable は true に戻るべき (§9.2)"
        );
        assert!(
            reg.as_slice().is_empty(),
            "scenario なし sidecar では registry も空 (instruments クリア) になるべき"
        );
    }
}
