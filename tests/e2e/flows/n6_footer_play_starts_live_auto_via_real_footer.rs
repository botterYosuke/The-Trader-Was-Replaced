//! N6 footer_play_starts_live_auto_via_real_footer — 本番 `spawn_footer` が生成する
//! 実 ▶ entity を、本番 `footer_pause_resume_system` / `apply_execution_mode_visibility_system`
//! で駆動し、LiveAuto の footer ▶ が実機経路で `StartLiveAuto` を出すことを保証する。
//!
//! N5 は合成 ▶ entity を直 spawn し resource を直 seed するため branch 入力しか検証しない。
//! N6 は本番 `spawn_footer` を Startup に載せ、実際に画面へ出る ▶ entity（`PauseResumeButton`
//! + `TransportButton::PauseResume` + `Button`）そのものを `Interaction::Pressed` にして
//! 本番 system を回す。これにより「entity 生成 → 可視性 → 押下 → 送出」の実機リンクを丸ごと踏む。
//! 詳細は `tests/e2e/FLOWS.md` の N6 を参照。

use std::ffi::OsString;

use serial_test::serial;

use bevy::prelude::*;

use backcast::trading::{
    BackendStatus, CurrentRun, ExecutionMode, ExecutionModeRes, ReplaySpeed, SelectedSymbol,
    TradingSession, TradingSettings, TransportCommand, TransportCommandSender, VenueState,
    VenueStatusRes,
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

/// 本番 `spawn_footer` を Startup に載せた bare App を組む。
///
/// `spawn_footer` は `Res<AssetServer>` を取り symbol font を `load` するため、
/// `AssetPlugin` と `Font` asset 型の登録が要る（描画はしないので render plugin は載せない。
/// `load` は handle を即返す非同期要求で、1 frame では実 parse を走らせない）。
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
        .add_event::<StrategyRunRequested>();

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

fn display_of(app: &App, e: Entity) -> Display {
    app.world().entity(e).get::<Node>().unwrap().display
}

fn start_live_auto_count(rx: &mut mpsc::UnboundedReceiver<TransportCommand>) -> usize {
    let mut n = 0;
    while let Ok(cmd) = rx.try_recv() {
        if matches!(cmd, TransportCommand::StartLiveAuto { .. }) {
            n += 1;
        }
    }
    n
}

/// Case A: 可視性回帰ガード。実 ▶ entity は LiveAuto で `Display::Flex`、LiveManual で `Display::None`。
#[test]
#[serial]
fn n6_real_footer_play_visible_in_live_auto_only() {
    let (mut app, _rx) = make_app();
    // Startup を 1 回流して実 footer を生成。
    app.update();
    let pause = pause_resume_entity(&mut app);

    // LiveAuto: ▶ は出る（is_changed を立てるため毎回モードを書く）。
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.update();
    assert_eq!(
        display_of(&app, pause),
        Display::Flex,
        "LiveAuto では実 ▶ entity は表示されるはず",
    );

    // LiveManual: ▶ は隠れる。
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveManual;
    app.update();
    assert_eq!(
        display_of(&app, pause),
        Display::None,
        "LiveManual では実 ▶ entity は隠れるはず",
    );
}

/// Case B: ハッピーパス（root cause 本命）。LiveAuto + symbol + live venue + 実機 Open 後と同じ状態
/// （StrategyFragment 窓あり & cache_path 設定済み）で、実 ▶ entity を押すと `StartLiveAuto` が 1 件出る。
#[test]
#[serial]
fn n6_real_footer_play_emits_start_live_auto() {
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

    let (mut app, mut rx) = make_app();
    app.update();
    let pause = pause_resume_entity(&mut app);

    // 実機 Open 後の状態を再現: LiveAuto・symbol 選択・live venue。
    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.world_mut().resource_mut::<SelectedSymbol>().id = Some("7203.TSE".to_string());
    {
        let mut v = app.world_mut().resource_mut::<VenueStatusRes>();
        v.state = VenueState::Subscribed;
        v.venue_id = Some("tachibana".to_string());
        v.configured_venue = Some("tachibana".to_string());
    }
    // 起動銘柄は scenario から導出される（Replay と対称）。実機 Open 後の scenario を再現。
    app.world_mut().resource_mut::<ScenarioMetadata>().instruments = vec!["7203.TSE".to_string()];
    // 実機の Open は cache_path を埋める。`flush_strategy_cache` が Ok(true) を返す前提を再現。
    app.world_mut().resource_mut::<StrategyBuffer>().cache_path = Some(cache_py.clone());

    // 実機 Open は StrategyEditor 窓を 1 つ出す。root entity に
    // (StrategyEditorId, StrategyFragment, WindowRoot) を貼り、merge_fragments の入力にする。
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

    // 実 ▶ entity を Pressed にする（新規 Interaction は Changed 扱い → 本番 handler が 1 回 fire）。
    app.world_mut()
        .entity_mut(pause)
        .insert(Interaction::Pressed);
    app.update();

    assert_eq!(
        start_live_auto_count(&mut rx),
        1,
        "実機経路: LiveAuto で全 pre-flight 通過時、実 ▶ 押下は StartLiveAuto を 1 件出すはず",
    );
}

/// Case C: silent guard の文書化。StrategyFragment も cache_path も無い状態では
/// `flush_strategy_cache` が Ok(false) を返し、▶ 押下で何も送らない（現状の無反応を固定）。
#[test]
#[serial]
fn n6_real_footer_play_blocks_without_strategy_cache() {
    let (mut app, mut rx) = make_app();
    app.update();
    let pause = pause_resume_entity(&mut app);

    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.world_mut().resource_mut::<SelectedSymbol>().id = Some("7203.TSE".to_string());
    {
        let mut v = app.world_mut().resource_mut::<VenueStatusRes>();
        v.state = VenueState::Subscribed;
        v.venue_id = Some("tachibana".to_string());
        v.configured_venue = Some("tachibana".to_string());
    }
    app.world_mut().resource_mut::<ScenarioMetadata>().instruments = vec!["7203.TSE".to_string()];
    // cache_path 未設定・StrategyFragment 窓なし: flush は Ok(false) → silent continue。

    app.world_mut()
        .entity_mut(pause)
        .insert(Interaction::Pressed);
    app.update();

    assert_eq!(
        start_live_auto_count(&mut rx),
        0,
        "cache_path も fragment も無ければ ▶ 押下で StartLiveAuto は出ない（silent guard の現状）",
    );
}
