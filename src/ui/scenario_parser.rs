use crate::ui::components::{
    ScenarioClearedFromFile, ScenarioFileWatchState, ScenarioLoadedFromFile, ScenarioMetadata,
    ScenarioReadTarget,
};
use bevy::prelude::*;
use serde::Deserialize;
use std::path::{Path, PathBuf};

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
    /// v3: 外部 JSON ファイルへの参照（`<path>#<json-pointer>` 形式）。
    /// 存在する場合は `instruments` より優先して解決する。
    #[serde(default)]
    instruments_ref: Option<String>,
    start: Option<String>,
    end: Option<String>,
    granularity: Option<String>,
    initial_cash: Option<i64>,
}

/// `instruments_ref` 文字列（`"<path>#<json-pointer>"` または `"<path>"`）を
/// サイドカーの sibling ファイルから解決し、instrument id のリストを返す。
///
/// - `path_part`: `#` より前の相対パス（絶対パスも許容）
/// - `pointer`: RFC 6901 風の `#/key/N` サフィックス（省略時はルートが配列想定）
/// - 失敗 (ファイル不在 / JSON 破損 / ポインタ不一致 / 空リスト) → `None`
fn resolve_instruments_ref(ref_spec: &str, sidecar_path: &Path) -> Option<Vec<String>> {
    // "#" で分割 → (path_part, optional pointer)
    let (path_part, pointer) = if let Some(idx) = ref_spec.find('#') {
        (&ref_spec[..idx], Some(&ref_spec[idx + 1..]))
    } else {
        (ref_spec, None)
    };
    let target = sidecar_path.parent()?.join(path_part);
    let text = std::fs::read_to_string(&target).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    let list: Vec<String> = if let Some(ptr) = pointer {
        // 最小 RFC 6901: /key → obj[key], /key/N → obj[key][N]
        let parts: Vec<&str> = ptr.trim_start_matches('/').split('/').collect();
        let mut v = &val;
        for p in &parts {
            if p.is_empty() {
                continue;
            }
            v = if let Ok(n) = p.parse::<usize>() {
                v.get(n)?
            } else {
                v.get(*p)?
            };
        }
        v.as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect()
    } else {
        val.as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect()
    };
    if list.is_empty() {
        return None;
    }
    Some(list)
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
    target: Res<ScenarioReadTarget>,
    mut scenario: ResMut<ScenarioMetadata>,
    mut watch: ResMut<ScenarioFileWatchState>,
    mut loaded_events: MessageWriter<ScenarioLoadedFromFile>,
    mut cleared_events: MessageWriter<ScenarioClearedFromFile>,
) {
    let json_path: Option<PathBuf> = target.0.clone();
    let current_mtime = json_path
        .as_ref()
        .and_then(|jp| std::fs::metadata(jp).ok())
        .and_then(|m| m.modified().ok());
    if watch.last_path == json_path && watch.last_mtime == current_mtime {
        return;
    }
    watch.last_path = json_path.clone();
    watch.last_mtime = current_mtime;

    let Some(json_path) = json_path else {
        cleared_events.write(ScenarioClearedFromFile { source_path: None });
        *scenario = ScenarioMetadata::default();
        return;
    };

    let text = match crate::ui::layout_persistence::read_json_with_bom_strip(&json_path) {
        Ok(t) => t,
        Err(e) => {
            debug!(
                "no sidecar JSON for {:?}: {} — ScenarioMetadata reset",
                json_path, e
            );
            cleared_events.write(ScenarioClearedFromFile {
                source_path: Some(json_path.clone()),
            });
            *scenario = ScenarioMetadata::default();
            return;
        }
    };

    let root: SidecarRoot = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            warn!(
                "malformed sidecar JSON {:?}: {} — ScenarioMetadata reset",
                json_path, e
            );
            cleared_events.write(ScenarioClearedFromFile {
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

    // instruments_ref が存在する場合は fail-closed で解決する（D28）。
    // 解決失敗時は ScenarioLoadedFromFile を送出せず早期 return する。
    let instruments_ref: Option<String> = sf.instruments_ref;

    let instruments: Vec<String> = if let Some(ref ref_spec) = instruments_ref {
        match resolve_instruments_ref(ref_spec, &json_path) {
            Some(ids) => ids,
            None => {
                error!(
                    "instruments_ref resolve failed: {} (sidecar: {:?}) — ScenarioLoadedFromFile not sent",
                    ref_spec, json_path
                );
                // fail-closed: ScenarioLoadedFromFile を送出しない
                return;
            }
        }
    } else if let Some(list) = sf.instruments {
        list
    } else if let Some(sol) = sf.instrument {
        match sol {
            StringOrList::One(s) => vec![s],
            StringOrList::Many(v) => v,
        }
    } else {
        vec![]
    };

    let new_meta = ScenarioMetadata {
        schema_version: sf.schema_version,
        instruments: instruments.clone(),
        start: sf.start,
        end: sf.end.clone(),
        granularity: sf.granularity,
        initial_cash: sf.initial_cash,
    };

    info!(
        "SCENARIO parsed from sidecar: schema_version={:?} instruments={:?} start={:?} end={:?} granularity={:?} initial_cash={:?} ref_path={:?}",
        new_meta.schema_version,
        new_meta.instruments,
        new_meta.start,
        new_meta.end,
        new_meta.granularity,
        new_meta.initial_cash,
        instruments_ref,
    );

    *scenario = new_meta;

    loaded_events.write(ScenarioLoadedFromFile {
        source_path: json_path,
        instruments,
        end: sf.end,
        ref_path: instruments_ref,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::ScenarioLoadedFromFile;
    use crate::ui::components::StrategyBuffer;

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
        app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.schema_version, Some(2));
        assert_eq!(
            meta.instruments,
            vec!["1301.TSE".to_string(), "7203.TSE".to_string()]
        );
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
        app.insert_resource(ScenarioReadTarget(Some(json_path)));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
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
        app.insert_resource(ScenarioReadTarget(Some(dir.path().join("no_sidecar.json"))));
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
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
        app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert_eq!(
            collected.len(),
            1,
            "expected exactly one ScenarioLoadedFromFile event on first read"
        );
        let ev = &collected[0];
        assert_eq!(ev.source_path, py_path.with_extension("json"));
        assert_eq!(ev.instruments, vec!["1301.TSE".to_string()]);
        assert_eq!(ev.end.as_deref(), Some("2025-01-10"));
        assert!(
            ev.ref_path.is_none(),
            "plain instruments list must not set ref_path"
        );
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
        app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);

        app.update(); // 1 回目: 発火
        app.world_mut()
            .resource_mut::<Events<ScenarioLoadedFromFile>>()
            .clear();
        app.update(); // 2 回目: 発火してはいけない

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert!(
            collected.is_empty(),
            "no re-emit expected when mtime unchanged, got {} events",
            collected.len()
        );
    }

    /// instruments_ref が指す外部 JSON ファイルが存在し、配列を解決できる場合、
    /// ScenarioLoadedFromFile が発火して ref_path が Some になること。
    #[test]
    fn parse_resolves_instruments_ref_to_instruments() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        let ref_path = dir.path().join("universe.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        // 外部 universe ファイルを作成する
        std::fs::write(&ref_path, r#"["1301.TSE","7203.TSE"]"#).unwrap();
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "universe.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert_eq!(
            collected.len(),
            1,
            "instruments_ref resolve must emit ScenarioLoadedFromFile"
        );
        assert_eq!(
            collected[0].instruments,
            vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
            "instruments must come from the referenced file"
        );
        assert!(
            collected[0].ref_path.is_some(),
            "ref_path must be Some when instruments_ref was used"
        );
    }

    /// inline instruments のみの sidecar は従来通り動くこと。
    #[test]
    fn parse_inline_instruments_still_works() {
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
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert_eq!(collected.len(), 1, "inline instruments must emit event");
        assert_eq!(collected[0].instruments, vec!["1301.TSE".to_string()]);
        assert!(
            collected[0].ref_path.is_none(),
            "ref_path must be None for inline instruments"
        );
    }

    /// instruments_ref が指す外部ファイルが存在しない場合、fail-closed で
    /// ScenarioLoadedFromFile を送出しないこと（D28）。
    #[test]
    fn parse_falls_back_on_missing_ref_target() {
        let dir = tempfile::tempdir().unwrap();
        let py_path = dir.path().join("strat.py");
        let json_path = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        // 外部 universe ファイルは作らない
        std::fs::write(
            &json_path,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "missing_universe.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path),
            cache_path: None,
            last_merged_source: None,
        });
        app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let events = app.world().resource::<Events<ScenarioLoadedFromFile>>();
        let mut reader = events.get_cursor();
        let collected: Vec<_> = reader.read(events).cloned().collect();
        assert!(
            collected.is_empty(),
            "fail-closed: missing instruments_ref target must NOT emit ScenarioLoadedFromFile (got {} events)",
            collected.len()
        );
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
        // (参照先の universe ファイルを作成して fail-closed にならないようにする)
        let py_a = dir.path().join("locked.py");
        let json_a = dir.path().join("locked.json");
        let universe_dir = dir.path().join("universe");
        std::fs::create_dir_all(&universe_dir).unwrap();
        std::fs::write(universe_dir.join("foo.json"), r#"["1301.TSE"]"#).unwrap();
        std::fs::write(&py_a, "# dummy").unwrap();
        std::fs::write(
            &json_a,
            r#"{"scenario": {"schema_version": 3, "instruments_ref": "universe/foo.json", "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
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
        app.insert_resource(ScenarioReadTarget(Some(json_a.clone())));
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
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
            assert!(
                !reg.editable,
                "precondition: instruments_ref で editable=false に落ちる"
            );
        }

        // sidecar B へ切り替え (StrategyBuffer.original_path を差し替え)
        app.world_mut()
            .resource_mut::<StrategyBuffer>()
            .original_path = Some(py_b.clone());
        app.world_mut()
            .insert_resource(ScenarioReadTarget(Some(json_b.clone())));

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

    /// `ScenarioReadTarget = Some(cache.json)` なら buffer.original_path に関わらず
    /// cache.json が読まれることを確認する (Step 4 Red テスト)。
    /// — 現状の parse_scenario_system は buffer.original_path を見るため、
    ///   cache.json の内容が反映されず Red になる。
    #[test]
    fn parse_scenario_uses_target_path_not_buffer_original() {
        use crate::ui::components::ScenarioReadTarget;

        let dir = tempfile::tempdir().unwrap();

        // original sidecar: 7203.TSE
        let py_path = dir.path().join("strat.py");
        let original_json = dir.path().join("strat.json");
        std::fs::write(&py_path, "# dummy").unwrap();
        std::fs::write(
            &original_json,
            r#"{"scenario": {"schema_version": 2, "instruments": ["7203.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        // cache sidecar: 1301.TSE
        let cache_json = dir.path().join("app_state.json");
        std::fs::write(
            &cache_json,
            r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#,
        )
        .unwrap();

        let mut app = App::new();
        // buffer は original sidecar を指す
        app.insert_resource(StrategyBuffer {
            original_path: Some(py_path.clone()),
            cache_path: None,
            last_merged_source: None,
        });
        // target は cache sidecar を指す
        app.insert_resource(ScenarioReadTarget(Some(cache_json.clone())));
        app.init_resource::<ScenarioMetadata>();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_message::<ScenarioLoadedFromFile>();
        app.add_message::<ScenarioClearedFromFile>();
        app.add_systems(Update, parse_scenario_system);
        app.update();

        let meta = app.world().resource::<ScenarioMetadata>();
        // target が指す cache_json の内容 (1301.TSE) が反映されるべき
        assert_eq!(
            meta.instruments,
            vec!["1301.TSE".to_string()],
            "ScenarioReadTarget が cache_json を指していれば 1301.TSE が読まれるべき"
        );
    }
}
