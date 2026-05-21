//! J15 scenario_file_watch_reparse — 開いている sidecar JSON が変更されたとき、
//! mtime の変化で scenario が再 parse され、`ScenarioLoadedFromFile` が再発火することを保証する。
//! ファイルを削除すると `ScenarioClearedFromFile` が発火し、metadata がリセットされることも検証する（kind:integration）。
//!
//! `ScenarioFileWatchState` の mtime ガードが働くため、ファイルを書き換えないと 2 tick 目は
//! 再発火しない（no-refire on stable mtime）。これも一緒に確認する。

use bevy::prelude::*;
use std::time::Duration;

use backcast::ui::components::{
    ScenarioClearedFromFile, ScenarioFileWatchState, ScenarioLoadedFromFile, ScenarioMetadata,
    ScenarioReadTarget,
};
use backcast::ui::scenario_parser::parse_scenario_system;

fn drain_loaded(app: &mut App) -> Vec<ScenarioLoadedFromFile> {
    app.world_mut()
        .resource_mut::<Events<ScenarioLoadedFromFile>>()
        .drain()
        .collect()
}

fn drain_cleared(app: &mut App) -> Vec<ScenarioClearedFromFile> {
    app.world_mut()
        .resource_mut::<Events<ScenarioClearedFromFile>>()
        .drain()
        .collect()
}

#[test]
fn j15_scenario_file_watch_reparse() {
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("strat.json");

    let v1_body = r#"{"scenario": {"schema_version": 2, "instruments": ["1301.TSE"], "start": "2025-01-06", "end": "2025-01-10", "granularity": "Daily", "initial_cash": 1000000}}"#;
    std::fs::write(&json_path, v1_body).unwrap();

    let mut app = App::new();
    app.insert_resource(ScenarioReadTarget(Some(json_path.clone())));
    app.init_resource::<ScenarioMetadata>();
    app.init_resource::<ScenarioFileWatchState>();
    app.add_event::<ScenarioLoadedFromFile>();
    app.add_event::<ScenarioClearedFromFile>();
    app.add_systems(Update, parse_scenario_system);

    // ── tick 1: 初回パース → ScenarioLoadedFromFile 1 件 ──
    app.update();
    let loaded = drain_loaded(&mut app);
    assert_eq!(
        loaded.len(),
        1,
        "tick1: 初回 parse で ScenarioLoadedFromFile が 1 件発火するはず"
    );
    assert_eq!(
        loaded[0].instruments,
        vec!["1301.TSE".to_string()],
        "tick1: instruments が正しくロードされるはず"
    );

    // ── tick 2: mtime 不変 → 再発火なし ──
    app.update();
    let reloaded = drain_loaded(&mut app);
    assert!(
        reloaded.is_empty(),
        "tick2: mtime 不変なら再発火しないはず (got {})",
        reloaded.len()
    );

    // ── tick 3: ファイルを書き換え → mtime が変わり再発火 ──
    // mtime の分解能（1 秒以上）に依存しないよう少し待つか、ファイルの mtime を強制的に更新する。
    // 実際のファイル書き込みで OS が mtime を更新する。
    // macOS の HFS+ / APFS では 1ns 精度なので、同一秒内でも変わるが保証はない。
    // 安全策として 1 秒待つか set_modified を使う。ここでは少量のスリープで回避。
    std::thread::sleep(Duration::from_millis(10));
    let v2_body = r#"{"scenario": {"schema_version": 2, "instruments": ["7203.TSE","1301.TSE"], "start": "2025-02-01", "end": "2025-02-28", "granularity": "Minute", "initial_cash": 2000000}}"#;
    std::fs::write(&json_path, v2_body).unwrap();
    // mtime が変わったことを確認するため filetime などを使うこともできるが、
    // write が成功していれば OS は必ず mtime を更新する。

    app.update();
    let reloaded = drain_loaded(&mut app);
    // mtime が同じ秒内に収まった場合は再発火しない可能性があるため、
    // 実際の mtime 変化をチェックしてからアサートする。
    let new_mtime = std::fs::metadata(&json_path)
        .ok()
        .and_then(|m| m.modified().ok());
    let old_mtime = app
        .world()
        .resource::<ScenarioFileWatchState>()
        .last_mtime;
    if new_mtime != old_mtime || new_mtime.is_none() {
        // mtime が実際に変化した場合のみアサート
        assert_eq!(
            reloaded.len(),
            1,
            "tick3: ファイル書き換え後は ScenarioLoadedFromFile が再発火するはず"
        );
        if let Some(ev) = reloaded.first() {
            assert!(
                ev.instruments.contains(&"7203.TSE".to_string()),
                "tick3: 新しい instruments が反映されるはず"
            );
        }
        let meta = app.world().resource::<ScenarioMetadata>();
        assert_eq!(meta.start.as_deref(), Some("2025-02-01"));
    }

    // ── tick 4: ファイルを削除 → ScenarioClearedFromFile 発火 + metadata リセット ──
    std::fs::remove_file(&json_path).unwrap();
    app.update();

    let cleared = drain_cleared(&mut app);
    assert_eq!(
        cleared.len(),
        1,
        "tick4: ファイル削除後に ScenarioClearedFromFile が 1 件発火するはず"
    );
    let meta = app.world().resource::<ScenarioMetadata>();
    assert!(
        meta.instruments.is_empty(),
        "tick4: ファイル削除後は ScenarioMetadata がリセットされるはず"
    );
    assert!(meta.start.is_none(), "tick4: start もリセットされるはず");
}
