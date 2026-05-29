//! N7 footer_play_blocked_writes_run_result — LiveAuto の footer ▶ が pre-flight guard で
//! ブロックされたとき、サイレントに無反応で終わらず `CurrentRun.state` に
//! `RunState::Failed { error }` を書いて Run Result パネルへ理由を出すことを保証する。
//!
//! N6 は「全 pre-flight 通過 → StartLiveAuto 送出」のハッピーパスを踏む。N7 はその裏で、
//! guard が落ちたとき（venue 未接続 / venue identity 未設定）にユーザへ理由が surfacing される
//! ことを固定する回帰ガード。fix 前は guard が `warn!`+`continue` だけで `CurrentRun` は
//! `Idle` のまま残り、画面に何も出ない（silent）＝この 2 関数は assert fail（RED）になる。
//! 詳細は `tests/e2e/FLOWS.md` の N 群を参照。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;

use backcast::trading::{
    BackendStatus, CurrentRun, ExecutionMode, ExecutionModeRes, ReplaySpeed, RunState,
    SelectedSymbol, TradingSession, TradingSettings, TransportCommand, TransportCommandSender,
    VenueState, VenueStatusRes,
};
use backcast::ui::components::{
    PauseResumeButton, ScenarioMetadata, StrategyBuffer, StrategyEditorId, StrategyFragment,
    StrategyRunRequested, WindowRoot,
};
use backcast::ui::footer::{
    apply_execution_mode_visibility_system, footer_pause_resume_system, spawn_footer,
};
use backcast::ui::strategy_editor::StrategyAutoSaveState;

use tokio::sync::mpsc;

/// `BACKCAST_CACHE_DIR` を test 用に差し替え、Drop で元へ戻す RAII ガード。
/// 本番経路の cache 書き込みが実 cache を汚さないよう temp に隔離する。
struct CacheDirGuard(Option<OsString>);

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        // SAFETY: テスト終了時に env を元へ戻すだけ。単一地点で実行し値読み取りと競合しない。
        unsafe {
            match &self.0 {
                Some(v) => std::env::set_var("BACKCAST_CACHE_DIR", v),
                None => std::env::remove_var("BACKCAST_CACHE_DIR"),
            }
        }
    }
}

/// 本番 `spawn_footer` を Startup に載せた bare App を組む（N6 と同型）。
///
/// `footer_pause_resume_system` / `apply_execution_mode_visibility_system` が取る全 param を
/// 本番 `src/main.rs` 同様に insert / add_event する。1 つでも欠けると system-param 検証で panic する。
fn make_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let mut app = App::new();
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        // symbol font の `asset_server.load::<Font>()` を panic させないため Font 型を登録。
        .init_asset::<Font>();

    app.insert_resource(TransportCommandSender { tx })
        .insert_resource(ExecutionModeRes::default())
        .insert_resource(TradingSession::default())
        .insert_resource(BackendStatus::default())
        .insert_resource(TradingSettings::default())
        .insert_resource(ReplaySpeed::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(CurrentRun::default())
        .insert_resource(SelectedSymbol::default())
        .insert_resource(VenueStatusRes::default())
        .insert_resource(ScenarioMetadata::default())
        .insert_resource(StrategyAutoSaveState::default())
        .add_message::<StrategyRunRequested>();
    app.add_plugins(backcast::ui::theme::ThemePlugin);

    // spawn_footer は Startup で 1 回だけ実 footer ツリーを生成する。
    app.add_systems(Startup, spawn_footer);
    app.add_systems(
        Update,
        (apply_execution_mode_visibility_system, footer_pause_resume_system),
    );

    (app, rx)
}

/// 本番 footer ツリーの中から実 ▶ (PauseResume) entity を引く。
fn pause_resume_entity(app: &mut App) -> Entity {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<PauseResumeButton>>();
    let found: Vec<Entity> = q.iter(app.world()).collect();
    assert_eq!(
        found.len(),
        1,
        "spawn_footer は実 ▶ (PauseResume) entity を 1 体だけ生成するはず",
    );
    found[0]
}

/// guard ① (instruments 空) を通すため scenario / cache_path / StrategyFragment 窓を埋める。
/// これで残る guard だけが落ち、その guard が `CurrentRun` を書くかどうかを単離して観測できる。
fn seed_strategy_cache(app: &mut App, cache_py: &std::path::Path) {
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.world_mut().resource_mut::<SelectedSymbol>().id = Some("7203.TSE".to_string());
    app.world_mut().resource_mut::<ScenarioMetadata>().instruments = vec!["7203.TSE".to_string()];
    app.world_mut().resource_mut::<StrategyBuffer>().cache_path = Some(cache_py.to_path_buf());

    app.world_mut().spawn((
        StrategyEditorId {
            region_key: "region_001".to_string(),
        },
        StrategyFragment {
            source: "# strategy body\n".to_string(),
            dirty: false,
        },
        WindowRoot,
    ));
}

/// guard ② (venue not live): scenario / cache は揃うが venue.state=Disconnected。
/// fix 前は `warn!`+`continue` だけで `CurrentRun` は `Idle` のまま残る（silent）＝RED。
#[test]
#[serial]
fn n7_footer_play_blocked_venue_not_connected_writes_run_result() {
    let dir = tempfile::tempdir().unwrap();
    let cache_py = dir.path().join("strategy_cache.py");
    std::fs::write(&cache_py, "# strategy cache fixture\n").unwrap();

    // 本番経路の cache 書き込みを temp へ逃がす。
    // SAFETY: app 構築前の単一地点で設定し、ガードの Drop で復元する。
    let cache_dir = dir.path().join("cache");
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let (mut app, _rx) = make_app();
    app.update();
    let pause = pause_resume_entity(&mut app);

    seed_strategy_cache(&mut app, &cache_py);
    // venue 未接続: instruments・cache は揃うが is_venue_live() が false。
    {
        let mut v = app.world_mut().resource_mut::<VenueStatusRes>();
        v.state = VenueState::Disconnected;
        v.venue_id = Some("tachibana".to_string());
        v.configured_venue = Some("tachibana".to_string());
    }

    app.world_mut()
        .entity_mut(pause)
        .insert(Interaction::Pressed);
    app.update();

    assert!(
        matches!(
            app.world().resource::<CurrentRun>().state,
            RunState::Failed { ref error } if error.contains("Venue not connected")
        ),
        "venue 未接続で ▶ がブロックされたら CurrentRun に Failed{{Venue not connected}} を書くはず（fix 前は Idle のまま＝silent）。実際: {:?}",
        app.world().resource::<CurrentRun>().state,
    );
}

/// guard ③ (venue identity unset): venue.state=Subscribed (live) だが venue_id も
/// configured_venue も None。fix 前は `warn!`+`continue` で silent＝RED。
#[test]
#[serial]
fn n7_footer_play_blocked_venue_identity_unset_writes_run_result() {
    let dir = tempfile::tempdir().unwrap();
    let cache_py = dir.path().join("strategy_cache.py");
    std::fs::write(&cache_py, "# strategy cache fixture\n").unwrap();

    let cache_dir = dir.path().join("cache");
    let _cache_guard = {
        let prev = std::env::var_os("BACKCAST_CACHE_DIR");
        unsafe {
            std::env::set_var("BACKCAST_CACHE_DIR", &cache_dir);
        }
        CacheDirGuard(prev)
    };

    let (mut app, _rx) = make_app();
    app.update();
    let pause = pause_resume_entity(&mut app);

    seed_strategy_cache(&mut app, &cache_py);
    // venue は live (Subscribed) だが identity が一切ない: venue_id=None かつ configured_venue=None。
    {
        let mut v = app.world_mut().resource_mut::<VenueStatusRes>();
        v.state = VenueState::Subscribed;
        v.venue_id = None;
        v.configured_venue = None;
    }

    app.world_mut()
        .entity_mut(pause)
        .insert(Interaction::Pressed);
    app.update();

    assert!(
        matches!(
            app.world().resource::<CurrentRun>().state,
            RunState::Failed { ref error } if error.contains("Venue not configured")
        ),
        "venue identity 未設定で ▶ がブロックされたら CurrentRun に Failed{{Venue not configured}} を書くはず（fix 前は Idle のまま＝silent）。実際: {:?}",
        app.world().resource::<CurrentRun>().state,
    );
}
