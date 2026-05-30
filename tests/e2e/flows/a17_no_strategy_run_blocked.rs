//! A17 no_strategy_run_blocked — Strategy Editor が空かつ cache_path 未設定で
//! footer ▶ を押すと RunState::Failed になること。
//!
//! cache_path = None かつ fragments = 空の状態で ▶ を押すと、
//! `transport_button_system` の `items.is_empty() && buffer.cache_path.is_none()`
//! ガードが RunState::Failed を設定し、RunStrategy コマンドが送出されない。
//! issue #68 Slice 8 回帰ガード。
//! 詳細は `tests/e2e/FLOWS.md` の A17 を参照。

use crate::support::Harness;
use backcast::trading::{ExecutionMode, ExecutionModeRes, RunState, TransportCommand};
use backcast::ui::components::ScenarioMetadata;

#[test]
fn a17_no_strategy_run_blocked() {
    let mut h = Harness::new();

    // Replay モード + scenario は有効（instruments/date は設定済み）だが cache_path は未設定
    {
        let mut mode = h.app.world_mut().resource_mut::<ExecutionModeRes>();
        mode.mode = ExecutionMode::Replay;
    }
    {
        let mut sc = h.app.world_mut().resource_mut::<ScenarioMetadata>();
        sc.instruments = vec!["7203.TSE".to_string()];
        sc.start = Some("2025-01-06".to_string());
        sc.end = Some("2025-03-31".to_string());
        sc.granularity = Some("Daily".to_string());
        sc.initial_cash = Some(1_000_000);
    }
    // buffer.cache_path は None のまま（StrategyBuffer::default()）
    // fragments_q も空（StrategyFragment entity を spawn しない）
    h.set_replay_state(None);

    // ▶ ボタン押下
    h.app.world_mut().spawn((
        backcast::ui::components::PauseResumeButton,
        bevy::prelude::Button,
        bevy::prelude::BackgroundColor::default(),
        bevy::prelude::Interaction::Pressed,
    ));
    h.tick();

    // RunStrategy が送出されていない
    let cmds = h.drain_commands();
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, TransportCommand::RunStrategy { .. })),
        "cache_path=None のとき RunStrategy は送出されてはいけない (got {:?})",
        cmds
    );

    // RunState::Failed になっている
    assert!(
        matches!(h.run_state(), RunState::Failed { .. }),
        "cache_path=None のとき RunState は Failed になるはず (got {:?})",
        h.run_state()
    );
}
