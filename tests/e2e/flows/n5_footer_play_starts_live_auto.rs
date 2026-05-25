//! N5 footer_play_starts_live_auto — Live Auto の footer ▶ が保存済み strategy cache を起動すること。
//!
//! LiveAuto + scenario instruments + live venue + cache_path が揃うと `StartLiveAuto` を 1 件だけ送る。
//! 起動銘柄は scenario（サイドカー JSON）から導出する（Replay Run と対称）。複数銘柄では sidebar 選択が
//! scenario 内ならそれを優先し、無ければ先頭。不完全な precondition では何も送らない。
//! 詳細は `tests/e2e/FLOWS.md` の N5 を参照。

use crate::support::Harness;
use backcast::trading::{ExecutionMode, TransportCommand, VenueState, VenueStatusRes};

fn start_live_auto_commands(cmds: &[TransportCommand]) -> Vec<&TransportCommand> {
    cmds.iter()
        .filter(|cmd| matches!(cmd, TransportCommand::StartLiveAuto { .. }))
        .collect()
}

fn assert_no_start_live_auto(cmds: &[TransportCommand]) {
    assert!(
        start_live_auto_commands(cmds).is_empty(),
        "StartLiveAuto must not be sent: {cmds:?}",
    );
}

#[test]
fn n5_footer_play_starts_live_auto() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["7203.TSE"]);
    h.set_venue(VenueState::Subscribed, "tachibana");
    let cache_path = h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    let cmds = h.drain_commands();
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, TransportCommand::SetExecutionMode { .. })),
        "footer play must not request an execution-mode change: {cmds:?}",
    );

    let starts = start_live_auto_commands(&cmds);
    assert_eq!(
        starts.len(),
        1,
        "footer play in LiveAuto must send exactly one StartLiveAuto: {cmds:?}",
    );

    match starts[0] {
        TransportCommand::StartLiveAuto {
            instrument_id,
            venue,
            strategy_file,
        } => {
            assert_eq!(instrument_id, "7203.TSE");
            assert_eq!(venue, "tachibana");
            assert_eq!(strategy_file, &cache_path);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn n5_footer_play_live_auto_multi_instrument_prefers_selected() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["1301.TSE", "7203.TSE"]);
    h.set_selected_symbol(Some("7203.TSE"));
    h.set_venue(VenueState::Subscribed, "tachibana");
    let cache_path = h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    let cmds = h.drain_commands();
    let starts = start_live_auto_commands(&cmds);
    assert_eq!(starts.len(), 1, "expected one StartLiveAuto: {cmds:?}");

    match starts[0] {
        TransportCommand::StartLiveAuto {
            instrument_id,
            venue,
            strategy_file,
        } => {
            assert_eq!(instrument_id, "7203.TSE");
            assert_eq!(venue, "tachibana");
            assert_eq!(strategy_file, &cache_path);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn n5_footer_play_live_auto_multi_instrument_falls_back_to_first() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["1301.TSE", "7203.TSE"]);
    h.set_venue(VenueState::Subscribed, "tachibana");
    let cache_path = h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    let cmds = h.drain_commands();
    let starts = start_live_auto_commands(&cmds);
    assert_eq!(starts.len(), 1, "expected one StartLiveAuto: {cmds:?}");

    match starts[0] {
        TransportCommand::StartLiveAuto {
            instrument_id,
            venue,
            strategy_file,
        } => {
            assert_eq!(instrument_id, "1301.TSE");
            assert_eq!(venue, "tachibana");
            assert_eq!(strategy_file, &cache_path);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn n5_footer_play_live_auto_blocks_when_venue_disconnected() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["7203.TSE"]);
    h.set_venue(VenueState::Disconnected, "tachibana");
    h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    assert_no_start_live_auto(&h.drain_commands());
}

#[test]
fn n5_footer_play_live_auto_blocks_without_scenario_instruments() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_selected_symbol(Some("7203.TSE"));
    h.set_venue(VenueState::Subscribed, "tachibana");
    h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    assert_no_start_live_auto(&h.drain_commands());
}

#[test]
fn n5_footer_play_live_auto_uses_configured_venue_when_venue_id_unset() {
    // A `--live-venue TACHIBANA` auto-connect yields venue_id=None but
    // configured_venue=Some("tachibana"); the ▶ must launch using configured_venue.
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["7203.TSE"]);
    h.set_venue(VenueState::Subscribed, "tachibana");
    h.app
        .world_mut()
        .resource_mut::<VenueStatusRes>()
        .venue_id = None;
    let cache_path = h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    let cmds = h.drain_commands();
    let starts = start_live_auto_commands(&cmds);
    assert_eq!(
        starts.len(),
        1,
        "footer play in LiveAuto must send exactly one StartLiveAuto using configured_venue: {cmds:?}",
    );

    match starts[0] {
        TransportCommand::StartLiveAuto {
            instrument_id,
            venue,
            strategy_file,
        } => {
            assert_eq!(instrument_id, "7203.TSE");
            assert_eq!(venue, "tachibana");
            assert_eq!(strategy_file, &cache_path);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn n5_footer_play_live_auto_blocks_without_any_venue_identity() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["7203.TSE"]);
    h.set_venue(VenueState::Subscribed, "tachibana");
    {
        let mut venue = h.app.world_mut().resource_mut::<VenueStatusRes>();
        venue.venue_id = None;
        venue.configured_venue = None;
    }
    h.set_strategy_cache_path("strategy_cache.py");

    h.click_pause_resume();

    assert_no_start_live_auto(&h.drain_commands());
}

#[test]
fn n5_footer_play_live_auto_blocks_without_cache_path() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["7203.TSE"]);
    h.set_venue(VenueState::Subscribed, "tachibana");
    // No set_strategy_cache_path: StrategyBuffer.cache_path stays None and no
    // fragments exist, so the flush yields Ok(false) and the ▶ must not launch.

    h.click_pause_resume();

    assert_no_start_live_auto(&h.drain_commands());
}
