//! J16 startup_panel_field_commit — Startup パネルのフィールドへのコミットが
//! `ScenarioStartupParams` を更新し、`writeback_pending = true` をセットし、
//! `write_startup_params_to_cache_sidecar_system` が cache sidecar JSON を書き直すことを保証する（kind:ui）。
//!
//! `ScenarioStartupParamCommit` イベントを直接発火して本番 commit/writeback system を駆動し、
//! `ScenarioStartupParams` resource と sidecar ファイルの内容を観測する。
//! `BACKCAST_CACHE_DIR` を temp に逃がして実 cache を汚さない（CacheDirGuard パターン）。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;

use backcast::replay::ReplayStartupProgress;
use backcast::ui::components::{
    GranularityChoice, ScenarioMetadata, ScenarioStartupParams, ScenarioWritebackPaths,
};
use backcast::ui::scenario_startup_panel::{
    commit_startup_params_to_scenario_system, write_startup_params_to_cache_sidecar_system,
    ScenarioStartupParamCommit,
};

/// `BACKCAST_CACHE_DIR` を test 用に差し替え、Drop で元へ戻す RAII ガード。
struct CacheDirGuard(Option<OsString>);

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        // SAFETY: テスト終了時に env を元へ戻すだけ。
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
fn j16_startup_panel_field_commit() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    // SAFETY: テスト app 構築前の単一地点。
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    // 初期 cache sidecar ファイルを用意する（writeback はこのファイルを更新する）
    let cache_sidecar = dir.path().join("app_state.json");
    std::fs::write(
        &cache_sidecar,
        r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE"], "start": "2020-01-01", "end": "2020-12-31", "granularity": "Daily", "initial_cash": 100000}}"#,
    )
    .unwrap();

    // ── App 構築 ──
    let mut app = App::new();
    app.init_resource::<ScenarioMetadata>()
        .init_resource::<ScenarioStartupParams>()
        .init_resource::<ReplayStartupProgress>()
        .insert_resource(ScenarioWritebackPaths {
            cache_sidecar: Some(cache_sidecar.clone()),
        })
        .add_message::<ScenarioStartupParamCommit>()
        .add_systems(
            Update,
            (
                commit_startup_params_to_scenario_system,
                write_startup_params_to_cache_sidecar_system,
            )
                .chain(),
        );

    // ── テスト 1: Start フィールドのコミット ──
    app.world_mut()
        .write_message(ScenarioStartupParamCommit::Start("2025-01-06".into()));
    app.update();

    {
        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(
            params.start, "2025-01-06",
            "Start コミット後 params.start が更新されるはず"
        );
        assert!(
            params.errors.start.is_none(),
            "有効な Start 日付でエラーなし"
        );
    }

    // ── テスト 2: End フィールドのコミット ──
    app.world_mut()
        .write_message(ScenarioStartupParamCommit::End("2025-12-31".into()));
    app.update();

    {
        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.end, "2025-12-31");
        assert!(params.errors.end.is_none());
    }

    // ── テスト 3: Granularity ボタンのコミット ──
    app.world_mut()
        .write_message(ScenarioStartupParamCommit::Granularity(GranularityChoice::Minute));
    app.update();

    {
        let params = app.world().resource::<ScenarioStartupParams>();
        assert_eq!(params.granularity, GranularityChoice::Minute);
        assert!(params.errors.granularity.is_none());
    }

    // ── テスト 4: Initial cash のコミット + 全フィールド有効 → writeback_pending + 実ファイル書き込み ──
    app.world_mut()
        .write_message(ScenarioStartupParamCommit::InitialCash("2000000".into()));
    app.update();

    // コミット後 writeback_pending が true → write system が走ってファイル更新 → false に戻る
    {
        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            !params.writeback_pending,
            "writeback_pending は write system が実行後 false になるはず"
        );
        assert_eq!(params.initial_cash, "2000000");
    }

    // ファイルが書き直されていることを確認
    let written = std::fs::read_to_string(&cache_sidecar).unwrap();
    let v: serde_json::Value = serde_json::from_str(&written).unwrap();
    let scenario = v.get("scenario").unwrap();

    assert_eq!(
        scenario.get("start").and_then(|s| s.as_str()),
        Some("2025-01-06"),
        "cache sidecar の start が更新されるはず"
    );
    assert_eq!(
        scenario.get("end").and_then(|s| s.as_str()),
        Some("2025-12-31"),
        "cache sidecar の end が更新されるはず"
    );
    assert_eq!(
        scenario.get("granularity").and_then(|s| s.as_str()),
        Some("Minute"),
        "cache sidecar の granularity が更新されるはず"
    );
    assert_eq!(
        scenario.get("initial_cash").and_then(|s| s.as_i64()),
        Some(2_000_000),
        "cache sidecar の initial_cash が更新されるはず"
    );

    // 他のフィールドが保持されていること（上書きではなくパッチ更新）
    let instruments = scenario
        .get("instruments")
        .and_then(|i| i.as_array())
        .unwrap();
    assert_eq!(
        instruments[0].as_str(),
        Some("1301.TSE"),
        "instruments は保持されるはず（startup params writeback は上書きしない）"
    );

    // ── テスト 5: ScenarioMetadata も更新されること ──
    let meta = app.world().resource::<ScenarioMetadata>();
    assert_eq!(meta.start.as_deref(), Some("2025-01-06"));
    assert_eq!(meta.end.as_deref(), Some("2025-12-31"));
    assert_eq!(meta.granularity.as_deref(), Some("Minute"));
    assert_eq!(meta.initial_cash, Some(2_000_000));

    // ── テスト 6: 無効なコミットはファイルを書き換えない ──
    let before_content = std::fs::read_to_string(&cache_sidecar).unwrap();
    app.world_mut()
        .write_message(ScenarioStartupParamCommit::Start("bad-date".into()));
    app.update();

    {
        let params = app.world().resource::<ScenarioStartupParams>();
        assert!(
            params.errors.start.is_some(),
            "不正な日付で errors.start がセットされるはず"
        );
        assert!(
            !params.writeback_pending,
            "エラーありのとき writeback_pending はセットされないはず"
        );
    }

    let after_content = std::fs::read_to_string(&cache_sidecar).unwrap();
    assert_eq!(
        before_content, after_content,
        "無効なコミットはファイルを変更しないはず"
    );
}
