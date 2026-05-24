use crate::trading::{ExecutionMode, ExecutionModeRes};
use crate::ui::components::{InstrumentRegistry, ScenarioFileWatchState, ScenarioReadTarget};
use bevy::prelude::*;

/// Replay 再入時に `editable=false` の registry を fixed scenario から再 resolve する。
///
/// ExecutionMode が Replay でない状態 → Replay に遷移した瞬間だけ発火する。
/// `editable=false` (instruments_ref 由来の固定 universe) のときのみ
/// `ScenarioFileWatchState` を reset し、次フレームの `parse_scenario_system` に
/// scenario 再 parse → `sync_registry_from_scenario_loaded_system` 経由の registry
/// 再構築を起こす。
/// `editable=true` の場合は何もしない（Q2: ユーザーが Live で prune した状態を維持）。
pub fn restore_fixed_registry_on_replay_entry_system(
    exec_mode: Res<ExecutionModeRes>,
    registry: Res<InstrumentRegistry>,
    scenario_path: Res<ScenarioReadTarget>,
    mut watch: ResMut<ScenarioFileWatchState>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur = exec_mode.mode;
    let was = prev_mode.replace(cur);
    let entered_replay = was != Some(ExecutionMode::Replay) && cur == ExecutionMode::Replay;
    if !entered_replay {
        return;
    }
    if registry.editable {
        return;
    }
    if scenario_path.0.is_none() {
        return;
    }
    // scenario parse 経路を強制再 trigger する。watch を reset すると次フレームの
    // parse_scenario_system が ScenarioReadTarget(.json) を再 parse し、
    // ScenarioLoadedFromFile{ref_path} → sync_registry_from_scenario_loaded_system が
    // editable=false で registry を再構築する。.py を editor へ流す経路には一切触れない。
    watch.last_path = None;
    watch.last_mtime = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn seed_watch(app: &mut App) {
        // 各 update 前に「reset 前」の状態を必ず仕込む。
        // 発火すれば None に、非発火なら Some のまま残る。
        let mut watch = app.world_mut().resource_mut::<ScenarioFileWatchState>();
        watch.last_path = Some(PathBuf::from("seeded.json"));
        watch.last_mtime = Some(SystemTime::UNIX_EPOCH);
    }

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<ScenarioFileWatchState>();
        app.add_systems(Update, restore_fixed_registry_on_replay_entry_system);
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(InstrumentRegistry {
            editable: false,
            ids: vec!["1301.TSE".into()],
        });
        app.insert_resource(ScenarioReadTarget(Some(PathBuf::from("test.json"))));
        app
    }

    #[test]
    fn replay_reentry_with_editable_false_resets_watch() {
        let mut app = make_app();
        seed_watch(&mut app);
        app.update(); // LiveManual frame — entered_replay=false なので reset されない
        let watch = app.world().resource::<ScenarioFileWatchState>();
        assert!(
            watch.last_path.is_some() && watch.last_mtime.is_some(),
            "LiveManual frame must NOT reset ScenarioFileWatchState"
        );
        // Switch to Replay
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        seed_watch(&mut app);
        app.update();
        // editable=false で Replay 突入 → watch が reset されている
        let watch = app.world().resource::<ScenarioFileWatchState>();
        assert!(
            watch.last_path.is_none() && watch.last_mtime.is_none(),
            "Replay reentry with editable=false should reset ScenarioFileWatchState, \
             got last_path={:?} last_mtime={:?}",
            watch.last_path,
            watch.last_mtime
        );
    }

    #[test]
    fn replay_reentry_with_editable_true_keeps_watch() {
        let mut app = make_app();
        // editable=true に上書き
        app.insert_resource(InstrumentRegistry {
            editable: true,
            ids: vec![],
        });
        seed_watch(&mut app);
        app.update(); // LiveManual frame
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        seed_watch(&mut app);
        app.update();
        let watch = app.world().resource::<ScenarioFileWatchState>();
        assert!(
            watch.last_path.is_some() && watch.last_mtime.is_some(),
            "Replay reentry with editable=true should NOT reset ScenarioFileWatchState"
        );
    }

    #[test]
    fn replay_reentry_does_not_reset_when_already_in_replay() {
        let mut app = make_app();
        // 最初から Replay にしておく
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        seed_watch(&mut app);
        app.update(); // prev_mode: None→Replay、entered_replay=true → ここで reset される
        // 2回目フレーム用に再度仕込む
        seed_watch(&mut app);
        app.update(); // prev_mode: Replay→Replay、entered_replay=false → reset されない
        let watch = app.world().resource::<ScenarioFileWatchState>();
        assert!(
            watch.last_path.is_some() && watch.last_mtime.is_some(),
            "Second Replay frame should NOT reset ScenarioFileWatchState again"
        );
    }

    #[test]
    fn replay_reentry_with_no_scenario_path_does_nothing() {
        let mut app = make_app();
        // ScenarioReadTarget を None に設定
        app.insert_resource(ScenarioReadTarget(None));
        seed_watch(&mut app);
        app.update(); // LiveManual frame
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        seed_watch(&mut app);
        app.update();
        let watch = app.world().resource::<ScenarioFileWatchState>();
        assert!(
            watch.last_path.is_some() && watch.last_mtime.is_some(),
            "No scenario path → should NOT reset ScenarioFileWatchState"
        );
    }
}
