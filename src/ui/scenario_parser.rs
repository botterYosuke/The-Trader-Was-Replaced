use crate::ui::components::{ScenarioMetadata, StrategyBuffer};
use bevy::prelude::*;
use serde::Deserialize;
use std::path::PathBuf;

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
    mut last_path: Local<Option<PathBuf>>,
) {
    // original_path が変化したときだけ再実行する（毎フレーム JSON 読みを防ぐ）
    let current_path = buffer.original_path.clone();
    if *last_path == current_path {
        return;
    }
    *last_path = current_path.clone();

    // path がなければリセット
    let Some(py_path) = current_path else {
        *scenario = ScenarioMetadata::default();
        return;
    };

    // <strategy>.json を読む
    let json_path = py_path.with_extension("json");

    let text = match std::fs::read_to_string(&json_path) {
        Ok(t) => t,
        Err(e) => {
            debug!(
                "no sidecar JSON for {:?}: {} — ScenarioMetadata reset",
                json_path, e
            );
            *scenario = ScenarioMetadata::default();
            return;
        }
    };

    let root: SidecarRoot = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            warn!("malformed sidecar JSON {:?}: {} — ScenarioMetadata reset", json_path, e);
            *scenario = ScenarioMetadata::default();
            return;
        }
    };

    let Some(sf) = root.scenario else {
        debug!(
            "no 'scenario' key in {:?} — ScenarioMetadata reset",
            json_path
        );
        *scenario = ScenarioMetadata::default();
        return;
    };

    // instruments 解決: instruments 優先、なければ instrument を 1 要素 list 化
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

    let new_meta = ScenarioMetadata {
        schema_version: sf.schema_version,
        instruments,
        start: sf.start,
        end: sf.end,
        granularity: sf.granularity,
        initial_cash: sf.initial_cash,
    };

    info!(
        "SCENARIO parsed from sidecar: schema_version={:?} instruments={:?} start={:?} end={:?} granularity={:?} initial_cash={:?}",
        new_meta.schema_version,
        new_meta.instruments,
        new_meta.start,
        new_meta.end,
        new_meta.granularity,
        new_meta.initial_cash,
    );

    *scenario = new_meta;
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
