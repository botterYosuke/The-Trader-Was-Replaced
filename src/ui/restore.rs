use crate::trading::{ExecutionMode, ExecutionModeRes};
use crate::ui::components::{
    InstrumentRegistry, ScenarioReadTarget, StrategyFileLoadRequested, StrategyLoadMode,
};
use bevy::prelude::*;

/// Replay 再入時に `editable=false` の registry を fixed scenario から再 resolve する。
///
/// ExecutionMode が Replay でない状態 → Replay に遷移した瞬間だけ発火する。
/// `editable=false` (instruments_ref 由来の固定 universe) のときのみ
/// `StrategyFileLoadRequested` を再送出して registry を再構築する。
/// `editable=true` の場合は何もしない（Q2: ユーザーが Live で prune した状態を維持）。
pub fn restore_fixed_registry_on_replay_entry_system(
    exec_mode: Res<ExecutionModeRes>,
    registry: Res<InstrumentRegistry>,
    scenario_path: Res<ScenarioReadTarget>,
    mut prev_mode: Local<Option<ExecutionMode>>,
    mut event_writer: EventWriter<StrategyFileLoadRequested>,
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
    let Some(path) = scenario_path.0.clone() else {
        return;
    };
    event_writer.send(StrategyFileLoadRequested {
        path,
        mode: StrategyLoadMode::LayoutRestore,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use std::path::PathBuf;

    fn make_app() -> App {
        let mut app = App::new();
        app.add_event::<StrategyFileLoadRequested>();
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
    fn replay_reentry_with_editable_false_restores_fixed_registry() {
        let mut app = make_app();
        app.update(); // LiveManual frame — prev_mode は None→LiveManual、entered_replay=false
        // Switch to Replay
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.update();
        // StrategyFileLoadRequested が送出されていることを確認
        let events = app.world().resource::<Events<StrategyFileLoadRequested>>();
        assert!(
            !events.is_empty(),
            "Replay reentry with editable=false should fire StrategyFileLoadRequested"
        );
    }

    #[test]
    fn replay_reentry_with_editable_true_keeps_pruned_registry() {
        let mut app = make_app();
        // editable=true に上書き
        app.insert_resource(InstrumentRegistry {
            editable: true,
            ids: vec![],
        });
        app.update(); // LiveManual frame
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.update();
        let events = app.world().resource::<Events<StrategyFileLoadRequested>>();
        assert!(
            events.is_empty(),
            "Replay reentry with editable=true should NOT fire StrategyFileLoadRequested"
        );
    }

    #[test]
    fn replay_reentry_does_not_fire_when_already_in_replay() {
        let mut app = make_app();
        // 最初から Replay にしておく
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.update(); // prev_mode: None→Replay、was=None なので entered_replay=true
        // イベントをリセットして2回目のフレームを確認
        app.world_mut()
            .resource_mut::<Events<StrategyFileLoadRequested>>()
            .clear();
        app.update(); // prev_mode: Replay→Replay、entered_replay=false
        let events = app.world().resource::<Events<StrategyFileLoadRequested>>();
        assert!(
            events.is_empty(),
            "Second Replay frame should NOT fire again"
        );
    }

    #[test]
    fn replay_reentry_with_no_scenario_path_does_nothing() {
        let mut app = make_app();
        // ScenarioReadTarget を None に設定
        app.insert_resource(ScenarioReadTarget(None));
        app.update(); // LiveManual frame
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.update();
        let events = app.world().resource::<Events<StrategyFileLoadRequested>>();
        assert!(
            events.is_empty(),
            "No scenario path → should NOT fire StrategyFileLoadRequested"
        );
    }
}
