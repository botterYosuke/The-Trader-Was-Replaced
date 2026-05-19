use std::collections::HashSet;

use bevy::prelude::*;

use crate::trading::{
    AvailableInstruments, ExecutionMode, ExecutionModeRes, Tickers, TickersSource, TickersStatus,
    TransportCommand, TransportCommandSender, VenueState, VenueStatusRes,
    is_live_mode, is_venue_live,
};
use crate::ui::components::{InstrumentRegistry, ScenarioMetadata};
use crate::ui::instrument_picker::parse_scenario_end;

/// Live mode / Replay mode のどちらでも、`InstrumentRegistry` に入っているが
/// 現在有効な universe に含まれない銘柄を自動削除するシステム（§5.1 / D2 / D6b / D19）。
///
/// Replay: `AvailableInstruments.by_end_date[end]` が allowlist。end が parse できない場合は skip。
/// Live:   `Tickers.list` が allowlist。ただし三重 gate:
///   - `status == Loaded`
///   - `source in {LiveVenue, LocalVenueSnapshot}`
///   - `venue.state in {Connected, Subscribed}`  (D19)
pub fn prune_instruments_outside_universe_system(
    mut registry: ResMut<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    tickers: Res<Tickers>,
    available: Res<AvailableInstruments>,
    scenario: Res<ScenarioMetadata>,
    venue: Res<VenueStatusRes>,
) {
    let trigger = exec_mode.is_changed()
        || tickers.is_changed()
        || available.is_changed()
        || scenario.is_changed()
        || venue.is_changed();
    if !trigger {
        return;
    }
    if registry.ids.is_empty() {
        return;
    }

    let allowed: Option<HashSet<String>> = match exec_mode.mode {
        ExecutionMode::Replay => {
            let Some(end) = parse_scenario_end(&scenario) else {
                return;
            };
            // 空リスト（カタログにメタデータが無くて count=0 で返ってきた場合）は
            // 「ユニバース未確定」として扱い、プルーニングをスキップする。
            // None（キー未登録）と同様に早期 return で抜ける。
            available
                .by_end_date
                .get(&end)
                .filter(|v| !v.is_empty())
                .map(|v| v.iter().cloned().collect())
        }
        ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
            let status_ok = matches!(tickers.status, TickersStatus::Loaded);
            let source_ok = matches!(
                tickers.source,
                TickersSource::LiveVenue | TickersSource::LocalVenueSnapshot,
            );
            if status_ok && source_ok && is_venue_live(venue.state) {
                Some(tickers.list.iter().map(|t| t.id.clone()).collect())
            } else {
                None
            }
        }
    };

    let Some(allowed) = allowed else {
        return;
    };

    let before = registry.ids.clone();
    registry.ids.retain(|id| allowed.contains(id));
    if registry.ids != before {
        info!(
            "auto-prune: {} → {} (mode={:?})",
            before.len(),
            registry.ids.len(),
            exec_mode.mode
        );
    }
}

/// `InstrumentRegistry` から削除された銘柄に対して `UnsubscribeMarketData` を
/// backend に送信するシステム（§5.2 / D12）。
///
/// Live mode のみ送信する。mode 切替直後 frame は大量削除が unsubscribe storm に
/// ならないよう `mode_changed` guard で skip する。
pub fn unsubscribe_removed_instruments_system(
    registry: Res<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    sender: Option<Res<TransportCommandSender>>,
    mut prev_ids: Local<HashSet<String>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur_mode = exec_mode.mode;
    let mode_changed = prev_mode.replace(cur_mode) != Some(cur_mode);
    // `current` is always built to keep `prev_ids` accurate for future mode switches.
    // `removed` is only built when we will actually send unsubscribes.
    let current: HashSet<String> = registry.ids.iter().cloned().collect();
    if mode_changed || !is_live_mode(cur_mode) {
        *prev_ids = current;
        return;
    }
    let removed: Vec<String> = prev_ids.difference(&current).cloned().collect();
    *prev_ids = current;
    if removed.is_empty() {
        return;
    }
    let Some(tx) = sender.as_ref() else {
        return;
    };
    for id in removed {
        let _ = tx
            .tx
            .send(TransportCommand::UnsubscribeMarketData { instrument_id: id });
    }
}

/// VenueState が live（Connected / Subscribed）から !live に遷移した瞬間に
/// `Tickers` の source/status をリセットするシステム（§5.2.1 / D19）。
///
/// `tickers.list` は維持される（picker の placeholder 表示用）。
/// `Connected ↔ Subscribed` の往復では発火しない。
pub fn invalidate_tickers_on_venue_disconnect_system(
    venue: Res<VenueStatusRes>,
    mut tickers: ResMut<Tickers>,
    mut prev_state: Local<Option<VenueState>>,
) {
    let cur = venue.state;
    let was = prev_state.replace(cur);
    let was_live = was.is_some_and(is_venue_live);
    if !was_live || is_venue_live(cur) {
        return;
    }
    // 配信が落ちた瞬間。list は UI 表示用に維持し、source/status だけリセット。
    tickers.source = TickersSource::Unknown;
    tickers.status = TickersStatus::NotFetched;
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{AvailableInstruments, Ticker, Tickers, TickersSource, TickersStatus};
    use crate::ui::components::{InstrumentRegistry, ScenarioMetadata};
    use bevy::prelude::*;
    use chrono::NaiveDate;
    use std::sync::mpsc;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<InstrumentRegistry>()
            .init_resource::<ExecutionModeRes>()
            .init_resource::<Tickers>()
            .init_resource::<AvailableInstruments>()
            .init_resource::<ScenarioMetadata>()
            .init_resource::<VenueStatusRes>();
        app
    }

    fn set_registry(app: &mut App, ids: &[&str]) {
        let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
        reg.ids = ids.iter().map(|s| s.to_string()).collect();
        reg.editable = true;
    }

    fn set_mode(app: &mut App, mode: ExecutionMode) {
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = mode;
    }

    fn set_tickers(app: &mut App, ids: &[&str], source: TickersSource, status: TickersStatus) {
        let mut t = app.world_mut().resource_mut::<Tickers>();
        t.list = ids
            .iter()
            .map(|id| Ticker {
                id: id.to_string(),
                name: String::new(),
                market: String::new(),
            })
            .collect();
        t.source = source;
        t.status = status;
    }

    fn set_venue(app: &mut App, state: VenueState) {
        app.world_mut().resource_mut::<VenueStatusRes>().state = state;
    }

    fn set_available(app: &mut App, end: NaiveDate, ids: &[&str]) {
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .by_end_date
            .insert(end, ids.iter().map(|s| s.to_string()).collect());
    }

    fn set_scenario_end(app: &mut App, end: &str) {
        app.world_mut().resource_mut::<ScenarioMetadata>().end = Some(end.to_string());
    }

    // ── prune tests ──────────────────────────────────────────────────────────

    #[test]
    fn prune_removes_chart_only_id_on_switch_to_live() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "CHART_ONLY"]);
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::Loaded,
        );
        set_venue(&mut app, VenueState::Subscribed);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids, vec!["1301.TSE".to_string()]);
    }

    #[test]
    fn prune_removes_chart_only_id_on_switch_to_replay() {
        let end = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "CHART_ONLY"]);
        set_mode(&mut app, ExecutionMode::Replay);
        set_scenario_end(&mut app, "2025-01-10");
        set_available(&mut app, end, &["1301.TSE"]);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids, vec!["1301.TSE".to_string()]);
    }

    #[test]
    fn prune_skips_when_tickers_status_not_loaded() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "EXTRA"]);
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::NotFetched, // not loaded
        );
        set_venue(&mut app, VenueState::Subscribed);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        // gate not met → no prune
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn prune_skips_when_tickers_source_is_replay_catalog_fallback_in_live_mode() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "EXTRA"]);
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::ReplayCatalogFallback, // wrong source
            TickersStatus::Loaded,
        );
        set_venue(&mut app, VenueState::Subscribed);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids.len(), 2); // no prune
    }

    #[test]
    fn prune_runs_when_tickers_source_is_local_venue_snapshot_in_live_mode() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "NOT_IN_UNIVERSE"]);
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LocalVenueSnapshot,
            TickersStatus::Loaded,
        );
        set_venue(&mut app, VenueState::Connected);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids, vec!["1301.TSE".to_string()]);
    }

    #[test]
    fn prune_skips_when_available_not_fetched_in_replay() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE"]);
        set_mode(&mut app, ExecutionMode::Replay);
        set_scenario_end(&mut app, "2025-01-10");
        // by_end_date is empty → no allowlist → skip
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids.len(), 1); // unchanged
    }

    #[test]
    fn prune_runs_even_when_editable_is_false() {
        let end = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
        let mut app = make_app();
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.ids = vec!["1301.TSE".to_string(), "LOCKED_ONLY".to_string()];
            reg.editable = false; // locked
        }
        set_mode(&mut app, ExecutionMode::Replay);
        set_scenario_end(&mut app, "2025-01-10");
        set_available(&mut app, end, &["1301.TSE"]);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        // prune still runs regardless of editable
        assert_eq!(ids, vec!["1301.TSE".to_string()]);
    }

    #[test]
    fn prune_keeps_list_on_failed_status_does_not_prune_from_stale() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "EXTRA"]);
        set_mode(&mut app, ExecutionMode::LiveAuto);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::Failed("timeout".to_string()), // failed
        );
        set_venue(&mut app, VenueState::Subscribed);
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids.len(), 2); // no prune from stale/failed
    }

    #[test]
    fn prune_skips_when_venue_state_disconnected_even_if_tickers_loaded() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "EXTRA"]);
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::Loaded,
        );
        set_venue(&mut app, VenueState::Disconnected); // not live
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids.len(), 2); // D19 gate blocks prune
    }

    #[test]
    fn prune_skips_when_venue_state_reconnecting() {
        let mut app = make_app();
        set_registry(&mut app, &["1301.TSE", "EXTRA"]);
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::Loaded,
        );
        set_venue(&mut app, VenueState::Reconnecting); // not live
        app.add_systems(Update, prune_instruments_outside_universe_system);
        app.update();

        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids.len(), 2); // D19 gate blocks prune
    }

    // ── invalidate_tickers tests ──────────────────────────────────────────────

    #[test]
    fn invalidate_tickers_on_subscribed_to_disconnected_resets_source_and_status() {
        let mut app = make_app();
        // Seed the system Local: first run with Subscribed to initialize prev_state
        set_venue(&mut app, VenueState::Subscribed);
        {
            let mut t = app.world_mut().resource_mut::<Tickers>();
            t.list = vec![Ticker {
                id: "1301.TSE".into(),
                name: String::new(),
                market: String::new(),
            }];
            t.source = TickersSource::LiveVenue;
            t.status = TickersStatus::Loaded;
        }
        app.add_systems(Update, invalidate_tickers_on_venue_disconnect_system);
        app.update(); // tick 1: Subscribed → prev_state = Some(Subscribed)

        // tick 2: transition to Disconnected
        set_venue(&mut app, VenueState::Disconnected);
        app.update();

        let t = app.world().resource::<Tickers>();
        assert_eq!(t.source, TickersSource::Unknown);
        assert_eq!(t.status, TickersStatus::NotFetched);
        // list is preserved
        assert_eq!(t.list.len(), 1);
    }

    #[test]
    fn invalidate_tickers_on_connected_to_subscribed_does_not_fire() {
        let mut app = make_app();
        set_venue(&mut app, VenueState::Connected);
        {
            let mut t = app.world_mut().resource_mut::<Tickers>();
            t.source = TickersSource::LiveVenue;
            t.status = TickersStatus::Loaded;
        }
        app.add_systems(Update, invalidate_tickers_on_venue_disconnect_system);
        app.update(); // tick 1: Connected → prev = Connected

        // tick 2: Connected → Subscribed (still live, must NOT invalidate)
        set_venue(&mut app, VenueState::Subscribed);
        app.update();

        let t = app.world().resource::<Tickers>();
        assert_eq!(t.source, TickersSource::LiveVenue); // unchanged
        assert_eq!(t.status, TickersStatus::Loaded); // unchanged
    }

    #[test]
    fn invalidate_tickers_keeps_list_for_picker_placeholder() {
        let mut app = make_app();
        set_venue(&mut app, VenueState::Connected);
        {
            let mut t = app.world_mut().resource_mut::<Tickers>();
            t.list = vec![
                Ticker {
                    id: "1301.TSE".into(),
                    name: String::new(),
                    market: String::new(),
                },
                Ticker {
                    id: "7203.TSE".into(),
                    name: String::new(),
                    market: String::new(),
                },
            ];
            t.source = TickersSource::LiveVenue;
            t.status = TickersStatus::Loaded;
        }
        app.add_systems(Update, invalidate_tickers_on_venue_disconnect_system);
        app.update();

        // Disconnect
        set_venue(&mut app, VenueState::Disconnected);
        app.update();

        let t = app.world().resource::<Tickers>();
        assert_eq!(t.list.len(), 2, "list preserved for picker placeholder");
        assert_eq!(t.source, TickersSource::Unknown);
        assert_eq!(t.status, TickersStatus::NotFetched);
    }

    // ── unsubscribe tests ─────────────────────────────────────────────────────

    fn make_sender_app() -> (App, tokio::sync::mpsc::UnboundedReceiver<TransportCommand>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = make_app();
        app.insert_resource(TransportCommandSender { tx });
        (app, rx)
    }

    #[test]
    fn unsubscribe_sent_for_removed_id_in_live() {
        let (mut app, mut rx) = make_sender_app();
        // Set Live mode
        set_mode(&mut app, ExecutionMode::LiveManual);
        // Initialize prev_ids via first tick with two instruments
        set_registry(&mut app, &["1301.TSE", "7203.TSE"]);
        app.add_systems(Update, unsubscribe_removed_instruments_system);
        app.update(); // tick 1: prev_ids = {1301.TSE, 7203.TSE}

        // Remove 7203.TSE
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.ids.retain(|id| id != "7203.TSE");
        }
        app.update(); // tick 2: diff → UnsubscribeMarketData for 7203.TSE

        // Drain the channel (skip the mode_changed tick 1 which had no removal)
        let cmds: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let unsub: Vec<_> = cmds
            .into_iter()
            .filter_map(|c| match c {
                TransportCommand::UnsubscribeMarketData { instrument_id } => Some(instrument_id),
                _ => None,
            })
            .collect();
        assert!(
            unsub.contains(&"7203.TSE".to_string()),
            "expected UnsubscribeMarketData for 7203.TSE, got {:?}",
            unsub
        );
    }

    #[test]
    fn unsubscribe_not_sent_in_replay() {
        let (mut app, mut rx) = make_sender_app();
        set_mode(&mut app, ExecutionMode::Replay);
        set_registry(&mut app, &["1301.TSE", "7203.TSE"]);
        app.add_systems(Update, unsubscribe_removed_instruments_system);
        app.update();

        // Remove one
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.ids.retain(|id| id != "7203.TSE");
        }
        app.update();

        // No UnsubscribeMarketData in Replay
        let cmds: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let unsub: Vec<_> = cmds
            .into_iter()
            .filter_map(|c| match c {
                TransportCommand::UnsubscribeMarketData { instrument_id } => Some(instrument_id),
                _ => None,
            })
            .collect();
        assert!(unsub.is_empty(), "must not send Unsubscribe in Replay mode");
    }

    #[test]
    fn unsubscribe_not_sent_when_no_id_removed() {
        let (mut app, mut rx) = make_sender_app();
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_registry(&mut app, &["1301.TSE"]);
        app.add_systems(Update, unsubscribe_removed_instruments_system);
        app.update(); // tick 1: init prev_ids
        app.update(); // tick 2: no change

        let cmds: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let unsub: Vec<_> = cmds
            .into_iter()
            .filter_map(|c| match c {
                TransportCommand::UnsubscribeMarketData { instrument_id } => Some(instrument_id),
                _ => None,
            })
            .collect();
        assert!(unsub.is_empty(), "no unsubscribe when nothing removed");
    }

    #[test]
    fn unsubscribe_not_sent_on_mode_change_frame() {
        let (mut app, mut rx) = make_sender_app();
        // Start in Replay with two instruments
        set_mode(&mut app, ExecutionMode::Replay);
        set_registry(&mut app, &["1301.TSE", "7203.TSE"]);
        app.add_systems(Update, unsubscribe_removed_instruments_system);
        app.update(); // tick 1: Replay, prev_ids = {1301.TSE, 7203.TSE}

        // Switch to Live AND remove 7203 at same tick — mode_changed guard must suppress
        set_mode(&mut app, ExecutionMode::LiveManual);
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.ids.retain(|id| id != "7203.TSE");
        }
        app.update(); // tick 2: mode_changed = true → skip

        let cmds: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let unsub: Vec<_> = cmds
            .into_iter()
            .filter_map(|c| match c {
                TransportCommand::UnsubscribeMarketData { instrument_id } => Some(instrument_id),
                _ => None,
            })
            .collect();
        assert!(
            unsub.is_empty(),
            "mode_changed guard must suppress unsubscribe on mode-change frame, got {:?}",
            unsub
        );
    }

    // ── integration tests: prune → unsubscribe chain ─────────────────────────

    /// Step 12 統合テスト 2: Live mode で venue が接続済みかつ Tickers がロード済みの状態で、
    /// Tickers に存在しない銘柄が registry から prune され、かつ UnsubscribeMarketData が送信される。
    /// `prune_instruments_outside_universe_system` → `unsubscribe_removed_instruments_system` の
    /// 2 system chain を同一 tick で実行して連動を検証する。
    ///
    /// Tick 1: Tickers が NotFetched の状態で prev_ids = {1301.TSE, EXTRA} を初期化。
    /// Tick 2: Tickers を Loaded に切り替え → prune が EXTRA を削除 → unsubscribe が検出して送信。
    #[test]
    fn integration_live_prune_then_unsubscribe_chain() {
        let (mut app, mut rx) = make_sender_app();
        // Live mode, venue Connected
        set_mode(&mut app, ExecutionMode::LiveManual);
        set_venue(&mut app, VenueState::Connected);
        // Tick 1: Tickers NotFetched → prune gate not satisfied → registry unchanged
        // This allows unsubscribe system to initialize prev_ids with both instruments
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::NotFetched, // gate not satisfied yet
        );
        set_registry(&mut app, &["1301.TSE", "EXTRA"]);

        // Register both systems in chain order per §5.3
        app.add_systems(
            bevy::prelude::Update,
            (
                prune_instruments_outside_universe_system,
                unsubscribe_removed_instruments_system,
            )
                .chain(),
        );

        // tick 1: prune gate not met (NotFetched) → registry unchanged; prev_ids = {1301.TSE, EXTRA}
        app.update();
        // drain any commands from tick 1 (mode_changed frame)
        let _ = std::iter::from_fn(|| rx.try_recv().ok()).count();

        // tick 2: set Tickers to Loaded → prune fires, removes EXTRA → unsubscribe detects diff
        set_tickers(
            &mut app,
            &["1301.TSE"],
            TickersSource::LiveVenue,
            TickersStatus::Loaded,
        );
        app.update();

        // Collect UnsubscribeMarketData commands from tick 2
        let cmds: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let unsubs: Vec<String> = cmds
            .into_iter()
            .filter_map(|c| match c {
                TransportCommand::UnsubscribeMarketData { instrument_id } => Some(instrument_id),
                _ => None,
            })
            .collect();

        // Registry must be pruned
        let ids = app.world().resource::<InstrumentRegistry>().ids.clone();
        assert_eq!(ids, vec!["1301.TSE".to_string()], "prune must remove EXTRA");

        // Unsubscribe must have been sent for EXTRA
        assert!(
            unsubs.contains(&"EXTRA".to_string()),
            "UnsubscribeMarketData for EXTRA must be sent after prune, got {:?}",
            unsubs
        );
    }

    /// Step 12 統合テスト 3: `writeback_scenario_instruments_system` は
    /// Replay + editable=true のときだけ書き込む（Live または editable=false では書かない）。
    /// `writeback_scenario_instruments_system` は components.rs にあるが、
    /// ここでは Live mode 中に writeback が走らないことを pin する。
    #[test]
    fn integration_writeback_skipped_in_live_mode() {
        use crate::ui::components::{
            ScenarioFileWatchState, ScenarioInstrumentsWritebackState, ScenarioReadTarget,
            ScenarioWritebackPaths, writeback_scenario_instruments_system,
        };

        let mut app = App::new();
        // Set up resources
        app.init_resource::<InstrumentRegistry>()
            .init_resource::<ExecutionModeRes>()
            .init_resource::<ScenarioWritebackPaths>()
            .init_resource::<ScenarioReadTarget>()
            .init_resource::<ScenarioInstrumentsWritebackState>()
            .init_resource::<ScenarioFileWatchState>();

        // Dirty state: revision > flushed_revision, editable=true, but Live mode
        {
            let mut reg = app.world_mut().resource_mut::<InstrumentRegistry>();
            reg.editable = true;
            reg.ids = vec!["7203.TSE".to_string()];
        }
        {
            let mut wb = app.world_mut().resource_mut::<ScenarioInstrumentsWritebackState>();
            wb.revision = 5;
            wb.flushed_revision = 0;
        }
        // Live mode
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;

        app.add_systems(
            bevy::prelude::Update,
            writeback_scenario_instruments_system,
        );
        app.update();

        // flushed_revision must NOT have changed (writeback skipped in Live)
        let wb = app.world().resource::<ScenarioInstrumentsWritebackState>();
        assert_eq!(
            wb.flushed_revision, 0,
            "writeback must be skipped in Live mode (flushed_revision stays 0)"
        );
    }
}
