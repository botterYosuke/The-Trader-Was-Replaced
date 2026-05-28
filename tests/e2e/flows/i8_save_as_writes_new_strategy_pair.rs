//! I8 save_as_writes_new_strategy_pair — Save As が新しい `.json` / `.py` のペアを作成し、
//! 以後の `StrategyBuffer.original_path` と `ScenarioReadTarget` を新しい保存先へ
//! 切り替えることを保証する（kind:integration）。
//!
//! # 駆動経路（Issue #21 案A+案B）
//! Save As は `handle_save_as_layout_system` で rfd ダイアログ task を起動し、
//! `poll_save_as_dialog_system` が完了パスを受けて書き込む 2 段構成にリファクタされた。
//! `PendingFileDialog::inject_resolved(SaveAs, Some(path))` で確定パスを headless 注入し、
//! `poll_save_as_dialog_system` のみを駆動する（rfd を一切触らない）。
//!
//! # 注意
//! `poll_save_as_dialog_system` は `cache_state_paths()` / `sync_to_cache()` を内部で呼ぶため、
//! `BACKCAST_CACHE_DIR` を temp に逃がして実 cache を汚さない（`CacheDirGuard`）。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    ChartSizeMap,
    InstrumentRegistry, ScenarioInstrumentsWritebackState, ScenarioMetadata, ScenarioReadTarget,
    ScenarioWritebackPaths, StrategyBuffer, StrategyEditorId, StrategyFragment, WindowRoot,
};
use backcast::ui::layout_persistence::{
    poll_save_as_dialog_system, FileDialogKind, LayoutSaveAsRequested, PendingFileDialog,
};
use backcast::ui::strategy_editor::StrategyAutoSaveState;

/// `BACKCAST_CACHE_DIR` を test 用に差し替え、Drop で元へ戻す RAII ガード。
struct CacheDirGuard(Option<OsString>);

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var("BACKCAST_CACHE_DIR", v),
                None => std::env::remove_var("BACKCAST_CACHE_DIR"),
            }
        }
    }
}

#[test]
#[serial]
fn i8_save_as_writes_new_strategy_pair() {
    let dir = tempfile::tempdir().unwrap();
    let old_py = dir.path().join("old_strat.py");
    let old_json = dir.path().join("old_strat.json");
    let new_json = dir.path().join("new_strat.json");
    let new_py = new_json.with_extension("py"); // poll_save_as と同じ算出: json.with_extension("py")
    let cache_dir = dir.path().join("cache");

    // 既存 (古い) ペアを seed: original_path は old_py を指す。
    std::fs::write(&old_py, "# old strat\n").unwrap();
    let old_sidecar = serde_json::json!({
        "schema_version": 1,
        "scenario": {
            "instrument": "7203.TSE",
            "start": "2025-01-06",
            "end": "2025-03-31",
            "granularity": "Daily",
            "initial_cash": 1000000
        }
    });
    std::fs::write(&old_json, serde_json::to_string(&old_sidecar).unwrap()).unwrap();

    // cache 書き込みを temp に隔離（poll_save_as は cache_state_paths/sync_to_cache を呼ぶ）。
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    // poll_save_as_dialog_system が要求するすべての resource を挿入する。
    app.insert_resource(StrategyBuffer {
        original_path: Some(old_py.clone()), // 旧 .py を指している
        cache_path: None,
        last_merged_source: None,
    });
    app.insert_resource(ScenarioWritebackPaths {
        cache_sidecar: None,
    });
    {
        let mut reg = InstrumentRegistry::default();
        reg.editable = false; // editable=false → build_layout_for_explicit_save は早期 Some（scenario 必須欠落で skip しない）
        app.insert_resource(reg);
    }
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<StrategyAutoSaveState>();
    app.init_resource::<ScenarioInstrumentsWritebackState>();
    app.init_resource::<PendingFileDialog>();
    app.init_resource::<backcast::ui::components::ChartSizeMap>();

    // 旧 scenario_target を seed（preserve_scenario の第一候補 / 切替の before 値）。
    app.insert_resource(ScenarioReadTarget(Some(old_json.clone())));

    app.init_resource::<ChartSizeMap>();
    app.add_message::<LayoutSaveAsRequested>();

    // build_layout 内 camera.get_single() 用（無くても panic しないが i7 同様 spawn）。
    app.world_mut().spawn((
        Camera2d,
        Transform::default(),
            ));

    // .py を書かせるため StrategyFragment を持つ WindowRoot を 1 つ spawn。
    // poll_save_as の .py 書込は fragments_q (WindowRoot + StrategyEditorId + StrategyFragment) に依存する。
    app.world_mut().spawn((
        WindowRoot,
        StrategyEditorId {
            region_key: "main".to_string(),
        },
        StrategyFragment {
            source: "# new strat\n".to_string(),
            dirty: true,
        },
    ));

    app.add_systems(Update, poll_save_as_dialog_system);

    // ── Save As の確定パスを headless 注入（rfd を触らない seam）──
    {
        let mut pending = app.world_mut().resource_mut::<PendingFileDialog>();
        pending.inject_resolved(FileDialogKind::SaveAs, Some(new_json.clone()));
    }
    app.update();

    // ── 新しい .json / .py ペアが生成されたことを確認 ──
    assert!(
        new_json.exists(),
        "Save As 後に新 JSON {:?} が存在するはず",
        new_json
    );
    assert!(
        new_py.exists(),
        "Save As 後に新 .py {:?} が存在するはず",
        new_py
    );

    // ── JSON の strategy_path が新 .py を指し、schema_version を持つ ──
    let body = std::fs::read_to_string(&new_json).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).expect("JSON として読めるはず");
    assert!(
        v.get("schema_version").is_some(),
        "保存 JSON に schema_version が存在するはず: {v}"
    );
    let sp = v["strategy_path"]
        .as_str()
        .expect("strategy_path フィールドがあるはず");
    assert!(
        sp.ends_with("new_strat.py"),
        "strategy_path は new_strat.py を指すはず、got {sp}"
    );

    // ── original path / read target が新パスへ切り替わったことを確認 ──
    let buffer = app.world().resource::<StrategyBuffer>();
    assert_eq!(
        buffer.original_path,
        Some(new_py.clone()),
        "Save As 後に StrategyBuffer.original_path は新 .py へ切り替わるはず"
    );
    let target = app.world().resource::<ScenarioReadTarget>();
    assert_eq!(
        target.0,
        Some(new_json.clone()),
        "Save As 後に ScenarioReadTarget は新 .json へ切り替わるはず"
    );
}
