use crate::trading::{
    BackendStatus, CurrentRun, ExecutionMode, ExecutionModeRes, ReplaySpeed, RunState,
    SelectedSymbol, TradingSession, TradingSettings, TransportCommand, TransportCommandSender,
    VenueState, VenueStatusRes, is_venue_live,
};
use crate::ui::components::{
    ExecutionModeToggleSegment, FooterRoot, GrpcStatusLabel, PauseResumeButton, PauseResumeLabel,
    ReplayStateBadge, ReplayTimeLabel, ScenarioMetadata, SpeedButton, StrategyBuffer, StrategyEditorId,
    StrategyFragment, StrategyRunRequested, TransportButton, VenueStateBadge, WindowRoot,
};
use crate::ui::strategy_editor::{StrategyAutoSaveState, flush_strategy_cache, merge_fragments};
use bevy::prelude::*;

const BTN_NORMAL: Color = Color::srgba(0.12, 0.12, 0.18, 1.0);
const BTN_HOVER: Color = Color::srgba(0.22, 0.22, 0.32, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.35, 0.35, 0.52, 1.0);
const BTN_SPEED_SELECTED: Color = Color::srgba(0.18, 0.38, 0.58, 1.0);

const SPEED_OPTIONS: &[u32] = &[1, 2, 5, 10, 50];

const BUTTON_DISABLED_ALPHA: f32 = 0.35;
const BUTTON_ENABLED_ALPHA: f32 = 1.0;

fn spawn_transport_btn(parent: &mut ChildSpawnerCommands, label: &str, action: TransportButton) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(34.0),
                height: Val::Px(20.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BTN_NORMAL),
            action,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
            ));
        });
}

fn spawn_speed_btn(parent: &mut ChildSpawnerCommands, multiplier: u32, selected: bool) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(30.0),
                height: Val::Px(20.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(if selected {
                BTN_SPEED_SELECTED
            } else {
                BTN_NORMAL
            }),
            SpeedButton(multiplier),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(format!("{}x", multiplier)),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.85, 1.0)),
            ));
        });
}

fn spawn_mode_segment(parent: &mut ChildSpawnerCommands, label: &str, mode: ExecutionMode) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(50.0),
                height: Val::Px(20.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BTN_NORMAL),
            ExecutionModeToggleSegment(mode),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
            ));
        });
}

pub fn spawn_footer(mut commands: Commands, asset_server: Res<AssetServer>) {
    // U+25B6 ▶ と U+25A0 ■ は Bevy デフォルトフォント (FiraMono subset) に無いため、
    // Noto Sans Symbols 2 を読み込んで該当ボタンの Text にだけ適用する。
    // 他のボタン (`|<` `<` `>` `1x` 等) は ASCII なのでデフォルトのままで OK。
    let symbol_font: Handle<Font> = asset_server.load("fonts/NotoSansSymbols2-Regular.ttf");

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(28.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.04, 0.07, 0.93)),
            FooterRoot,
        ))
        .with_children(|p| {
            // ── ExecutionMode segment toggle (Phase 8 §3.5.1) ──
            spawn_mode_segment(p, "Replay", ExecutionMode::Replay);
            spawn_mode_segment(p, "Manual", ExecutionMode::LiveManual);
            spawn_mode_segment(p, "Auto", ExecutionMode::LiveAuto);

            p.spawn(Node {
                width: Val::Px(8.0),
                ..default()
            });

            // Transport buttons.
            // No StepBack ("<") button: the backend has no replay-rewind path
            // (`StepReplay` only advances), so a back button would be dead UI.
            // See issue #7.
            spawn_transport_btn(p, "|<", TransportButton::JumpToStart);
            // PauseResume: Button entity に PauseResumeButton marker、Text 子に PauseResumeLabel。
            // 初期表示は IDLE 起動を想定し "▶" (Run)。RUNNING 遷移時に "||" に切替。
            p.spawn((
                Button,
                Node {
                    width: Val::Px(34.0),
                    height: Val::Px(20.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BTN_NORMAL),
                TransportButton::PauseResume,
                PauseResumeButton,
            ))
            .with_children(|pp| {
                pp.spawn((
                    Text::new("▶"),
                    TextFont {
                        font: symbol_font.clone(),
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.85, 0.85)),
                    PauseResumeLabel,
                ));
            });
            spawn_transport_btn(p, ">", TransportButton::StepForward);
            // ForceStop の "■" (U+25A0) は symbol_font が必要 (デフォルト font には無い)。
            p.spawn((
                Button,
                Node {
                    width: Val::Px(34.0),
                    height: Val::Px(20.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BTN_NORMAL),
                TransportButton::ForceStop,
            ))
            .with_children(|pp| {
                pp.spawn((
                    Text::new("■"),
                    TextFont {
                        font: symbol_font.clone(),
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.85, 0.85)),
                ));
            });

            // Separator
            p.spawn(Node {
                width: Val::Px(6.0),
                ..default()
            });

            // Speed selector: 1x is selected by default
            for &mult in SPEED_OPTIONS {
                spawn_speed_btn(p, mult, mult == 1);
            }

            // Flex spacer — pushes status labels to the right
            p.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

            // Status labels
            p.spawn((
                Text::new("time: --"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.55, 0.55)),
                ReplayTimeLabel,
            ));
            p.spawn((
                Text::new("state: IDLE"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.45, 0.45, 0.45)),
                ReplayStateBadge,
            ));
            p.spawn((
                Text::new("Venue: DISCONNECTED"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.55, 0.55)),
                VenueStateBadge,
            ));
            p.spawn((
                Text::new("grpc: DISABLED"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.40, 0.40, 0.40)),
                GrpcStatusLabel,
            ));
        });
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn update_footer_system(
    data: Res<TradingSession>,
    status: Res<BackendStatus>,
    settings: Res<TradingSettings>,
    buffer: Res<StrategyBuffer>,
    venue: Res<VenueStatusRes>,
    exec_mode: Res<ExecutionModeRes>,
    current_run: Res<CurrentRun>,
    mut time_q: Query<
        &mut Text,
        (
            With<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<GrpcStatusLabel>,
            Without<PauseResumeLabel>,
            Without<VenueStateBadge>,
        ),
    >,
    mut state_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<ReplayStateBadge>,
            Without<ReplayTimeLabel>,
            Without<GrpcStatusLabel>,
            Without<PauseResumeLabel>,
            Without<VenueStateBadge>,
        ),
    >,
    mut grpc_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<GrpcStatusLabel>,
            Without<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<PauseResumeLabel>,
            Without<VenueStateBadge>,
        ),
    >,
    mut pause_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<PauseResumeLabel>,
            Without<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<GrpcStatusLabel>,
            Without<VenueStateBadge>,
        ),
    >,
    mut venue_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<VenueStateBadge>,
            Without<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<GrpcStatusLabel>,
            Without<PauseResumeLabel>,
        ),
    >,
    mut seg_q: Query<(
        &ExecutionModeToggleSegment,
        &mut BackgroundColor,
        &Interaction,
    )>,
) {
    let need_walltime_tick = !matches!(exec_mode.mode, ExecutionMode::Replay);
    if !need_walltime_tick
        && !data.is_changed()
        && !status.is_changed()
        && !settings.is_changed()
        && !buffer.is_changed()
        && !venue.is_changed()
        && !exec_mode.is_changed()
        && !current_run.is_changed()
    {
        return;
    }

    for mut text in &mut time_q {
        let new_text = match exec_mode.mode {
            ExecutionMode::Replay => {
                if data.timestamp_ms > 0 {
                    let jst = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
                    match chrono::DateTime::from_timestamp_millis(data.timestamp_ms) {
                        Some(utc) => {
                            let local = utc.with_timezone(&jst);
                            format!("time: {} JST (replay)", local.format("%Y-%m-%d %H:%M:%S"))
                        }
                        None => format!("time: {} ms (replay)", data.timestamp_ms),
                    }
                } else {
                    "time: -- (replay)".to_string()
                }
            }
            ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
                let now = chrono::Local::now();
                format!("time: {} (live)", now.format("%Y-%m-%d %H:%M:%S"))
            }
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
    }

    for (mut text, mut color) in &mut venue_q {
        let state_str = match venue.state {
            VenueState::Disconnected => "DISCONNECTED",
            VenueState::Authenticating => "AUTHENTICATING",
            VenueState::Connected => "CONNECTED",
            VenueState::Subscribed => "SUBSCRIBED",
            VenueState::Reconnecting => "RECONNECTING",
            VenueState::Error => "ERROR",
        };
        let new_text = match &venue.venue_id {
            Some(id) => format!("Venue: {} ({})", state_str, id),
            None => format!("Venue: {}", state_str),
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
        let new_color = match venue.state {
            VenueState::Disconnected => Color::srgb(0.55, 0.55, 0.55),
            VenueState::Authenticating | VenueState::Reconnecting => Color::srgb(1.00, 0.85, 0.20),
            VenueState::Connected => Color::srgb(0.30, 0.80, 1.00),
            VenueState::Subscribed => Color::srgb(0.20, 1.00, 0.45),
            VenueState::Error => Color::srgb(1.00, 0.28, 0.28),
        };
        if color.0 != new_color {
            color.0 = new_color;
        }
    }

    for (seg, mut bg, interaction) in &mut seg_q {
        let selected = seg.0 == exec_mode.mode;
        let target = if selected {
            BTN_SPEED_SELECTED
        } else if *interaction == Interaction::Hovered {
            BTN_HOVER
        } else {
            BTN_NORMAL
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }

    // Replay state badge
    let replay = data.replay_state.as_deref().unwrap_or("IDLE");
    let state_text = format!("state: {}", replay);
    let state_color = match replay {
        "RUNNING" => Color::srgb(0.20, 1.00, 0.45),
        "PAUSED" => Color::srgb(1.00, 0.75, 0.20),
        "LOADED" => Color::srgb(0.35, 0.70, 1.00),
        _ => Color::srgb(0.45, 0.45, 0.45), // IDLE
    };
    for (mut text, mut color) in &mut state_q {
        // 規約 2: 差分書き込み — avoid change-detection thrash per frame.
        if text.0 != state_text {
            text.0 = state_text.clone();
        }
        if color.0 != state_color {
            color.0 = state_color;
        }
    }

    // gRPC status
    let (grpc_text, grpc_color) = if !settings.backend_enabled {
        ("grpc: DISABLED", Color::srgb(0.38, 0.38, 0.38))
    } else if status.connected {
        ("grpc: OK", Color::srgb(0.20, 1.00, 0.45))
    } else if status.last_error.is_some() {
        ("grpc: ERR", Color::srgb(1.00, 0.28, 0.28))
    } else {
        ("grpc: ...", Color::srgb(0.80, 0.75, 0.25))
    };
    for (mut text, mut color) in &mut grpc_q {
        // 規約 2: 差分書き込み — avoid change-detection thrash per frame.
        if text.0 != grpc_text {
            text.0 = grpc_text.to_string();
        }
        if color.0 != grpc_color {
            color.0 = grpc_color;
        }
    }

    // PauseResume label: RUNNING → "||" (Pause action), それ以外 → "▶" (Run/Resume action)。
    // disabled 表現: IDLE/LOADED で cache_path 未設定なら半透明（Run できない）。
    // RUNNING/PAUSED は常に enabled（Pause/Resume は cache_path 不要）。
    let run_disabled = matches!(replay, "IDLE" | "LOADED") && buffer.cache_path.is_none();
    for (mut text, mut color) in &mut pause_q {
        let new_label = match exec_mode.mode {
            ExecutionMode::Replay => match replay {
                "RUNNING" => "||",
                _ => "▶",
            },
            ExecutionMode::LiveAuto => match current_run.state {
                RunState::Running => "||",
                _ => "▶",
            },
            _ => "▶",
        };
        if text.0 != new_label {
            text.0 = new_label.to_string();
        }
        let target_alpha = if run_disabled {
            BUTTON_DISABLED_ALPHA
        } else {
            BUTTON_ENABLED_ALPHA
        };
        if (color.0.alpha() - target_alpha).abs() > f32::EPSILON {
            color.0.set_alpha(target_alpha);
        }
    }
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn transport_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &TransportButton),
        (
            Changed<Interaction>,
            With<Button>,
            Without<PauseResumeButton>,
        ),
    >,
    data: Res<TradingSession>,
    sender: Res<TransportCommandSender>,
    exec_mode: Res<ExecutionModeRes>,
) {
    if !matches!(exec_mode.mode, ExecutionMode::Replay) {
        return;
    }
    for (interaction, mut bg, action) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                let replay = data.replay_state.as_deref().unwrap_or("IDLE");
                match action {
                    TransportButton::PauseResume => {
                        // PauseResume Button entity は With<PauseResumeButton> で除外済み。
                        // ここには来ない想定だが、enum exhaustive match を保つため arm を残す。
                    }
                    TransportButton::StepForward => {
                        if replay == "PAUSED" {
                            let _ = sender.tx.send(TransportCommand::StepForward);
                        } else {
                            info!("transport: step_forward ignored (state={})", replay);
                        }
                    }
                    TransportButton::JumpToStart => match replay {
                        "RUNNING" | "PAUSED" | "LOADED" => {
                            let _ = sender.tx.send(TransportCommand::ForceStop);
                        }
                        other => info!("transport: jump_to_start ignored (state={})", other),
                    },
                    TransportButton::ForceStop => match replay {
                        "RUNNING" | "PAUSED" | "LOADED" => {
                            let _ = sender.tx.send(TransportCommand::ForceStop);
                        }
                        other => info!("transport: force_stop ignored (state={})", other),
                    },
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn speed_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &SpeedButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut speed: ResMut<ReplaySpeed>,
    sender: Res<TransportCommandSender>,
    exec_mode: Res<ExecutionModeRes>,
) {
    if !matches!(exec_mode.mode, ExecutionMode::Replay) {
        return;
    }
    for (interaction, mut bg, SpeedButton(mult)) in &mut query {
        match interaction {
            Interaction::Pressed => {
                speed.current = *mult;
                let _ = sender.tx.send(TransportCommand::SetSpeed(*mult));
                bg.0 = BTN_SPEED_SELECTED;
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => {
                bg.0 = if speed.current == *mult {
                    BTN_SPEED_SELECTED
                } else {
                    BTN_NORMAL
                }
            }
        }
    }
}

/// Refreshes speed button highlight whenever ReplaySpeed changes.
pub fn update_speed_buttons_system(
    speed: Res<ReplaySpeed>,
    mut query: Query<(&mut BackgroundColor, &SpeedButton, &Interaction)>,
) {
    if !speed.is_changed() {
        return;
    }
    for (mut bg, SpeedButton(mult), interaction) in &mut query {
        bg.0 = if *mult == speed.current {
            BTN_SPEED_SELECTED
        } else if *interaction == Interaction::Hovered {
            BTN_HOVER
        } else {
            BTN_NORMAL
        };
    }
}

/// PauseResume Button 専用の入力ハンドラ。
/// - replay 状態に応じて Pause / Resume / Run を分岐
/// - hover/press の BackgroundColor 更新もここで担当（transport_button_system からは除外済み）
///
/// `transport_button_system` から分離する理由: Run フローには
/// `StrategyBuffer` / `StrategyAutoSaveState` / `CurrentRun` / `StrategyRunRequested` という
/// 別系統の依存が必要で、transport_button_system に詰め込むと責務が肥大化する。
/// `With<PauseResumeButton>` で物理的に分離することで、関心を 1 system 1 ボタンに保つ。
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn footer_pause_resume_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<PauseResumeButton>, With<Button>),
    >,
    data: Res<TradingSession>,
    sender: Res<TransportCommandSender>,
    mut buffer: ResMut<StrategyBuffer>,
    mut current_run: ResMut<CurrentRun>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
    mut run_events: MessageWriter<StrategyRunRequested>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), With<WindowRoot>>,
    exec_mode: Res<ExecutionModeRes>,
    selected: Res<SelectedSymbol>,
    scenario: Res<ScenarioMetadata>,
    venue: Res<VenueStatusRes>,
) {
    if !matches!(exec_mode.mode, ExecutionMode::Replay | ExecutionMode::LiveAuto) {
        return;
    }
    for (interaction, mut bg) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                match exec_mode.mode {
                    ExecutionMode::Replay => match data.replay_state.as_deref() {
                        Some("RUNNING") => {
                            if sender.tx.send(TransportCommand::Pause).is_err() {
                                warn!("transport: pause send failed (receiver dropped)");
                            }
                        }
                        Some("PAUSED") => {
                            if sender.tx.send(TransportCommand::Resume).is_err() {
                                warn!("transport: resume send failed (receiver dropped)");
                            }
                        }
                        _ => {
                            if matches!(current_run.state, RunState::Running) {
                                warn!("Run blocked: already running");
                                continue;
                            }
                            let mut items: Vec<(String, String)> = fragments_q
                                .iter()
                                .map(|(id, f)| (id.region_key.clone(), f.source.clone()))
                                .collect();
                            items.sort_by(|a, b| a.0.cmp(&b.0));
                            let merged = merge_fragments(&items);
                            match flush_strategy_cache(&merged, &mut buffer, &mut auto_save) {
                                Ok(true) => {}
                                Ok(false) => {
                                    warn!("Run blocked: no cache_path set");
                                    continue;
                                }
                                Err(e) => {
                                    error!("strategy flush before run failed: {}", e);
                                    continue;
                                }
                            }
                            let Some(path) = buffer.cache_path.clone() else {
                                warn!("Run blocked: no cache_path set");
                                continue;
                            };
                            run_events.write(StrategyRunRequested { cache_path: path });
                        }
                    },
                    ExecutionMode::LiveAuto => {
                        // Double-press guard: once a run is starting/running, ▶ must not
                        // start a 2nd run. run_id may still be None (server RUNNING not yet
                        // drained), so this must precede the run_id-based active-run branch.
                        if matches!(current_run.state, RunState::Running)
                            && current_run.run_id.is_none()
                        {
                            warn!("LiveAuto play blocked: run already starting/running");
                            continue;
                        }
                        // Active run: ▶ toggles Pause/Resume instead of starting a new run.
                        if let Some(run_id) = current_run.run_id.clone() {
                            match current_run.state {
                                RunState::Running => {
                                    if sender
                                        .tx
                                        .send(TransportCommand::PauseLiveStrategy { run_id })
                                        .is_err()
                                    {
                                        warn!("transport: PauseLiveStrategy send failed");
                                    }
                                    continue;
                                }
                                RunState::Paused => {
                                    if sender
                                        .tx
                                        .send(TransportCommand::ResumeLiveStrategy { run_id })
                                        .is_err()
                                    {
                                        warn!("transport: ResumeLiveStrategy send failed");
                                    }
                                    continue;
                                }
                                _ => {} // Idle / Stopped / Failed / Completed → start new run
                            }
                        }
                        // ▶ (LiveAuto): 全 pre-flight 通過時のみ StartLiveAuto を送出。
                        // SetExecutionMode は再送しない (ExecutionMode は backend 権威)。
                        // 起動銘柄は scenario（サイドカー JSON）から導出する（Replay Run と対称）。
                        // 複数銘柄では sidebar 選択が scenario 内ならそれを優先、無ければ先頭。
                        let instrument_id = match scenario.instruments.as_slice() {
                            [] => {
                                warn!("LiveAuto play: scenario has no instruments");
                                current_run.state = RunState::Failed {
                                    error: "No instrument selected".into(),
                                };
                                continue;
                            }
                            [only] => only.clone(),
                            instruments => selected
                                .id
                                .as_ref()
                                .filter(|id| instruments.iter().any(|instrument| instrument == *id))
                                .cloned()
                                .unwrap_or_else(|| instruments[0].clone()),
                        };
                        if !is_venue_live(venue.state) {
                            warn!("LiveAuto play: venue not live");
                            current_run.state = RunState::Failed {
                                error: "Venue not connected".into(),
                            };
                            continue;
                        }
                        // venue_id is empty for a `--live-venue` auto-connect (no md runner yet);
                        // fall back to the authoritative configured_venue (issue #40 E2E finding).
                        let Some(venue_identity) = venue
                            .venue_id
                            .clone()
                            .or_else(|| venue.configured_venue.clone())
                        else {
                            warn!("LiveAuto play: venue identity unset");
                            current_run.state = RunState::Failed {
                                error: "Venue not configured (launch with --live-venue)".into(),
                            };
                            continue;
                        };

                        let mut items: Vec<(String, String)> = fragments_q
                            .iter()
                            .map(|(id, f)| (id.region_key.clone(), f.source.clone()))
                            .collect();
                        items.sort_by(|a, b| a.0.cmp(&b.0));
                        let merged = merge_fragments(&items);
                        match flush_strategy_cache(&merged, &mut buffer, &mut auto_save) {
                            Ok(true) => {}
                            Ok(false) => {
                                warn!("LiveAuto play: no cache_path set");
                                current_run.state = RunState::Failed {
                                    error: "No strategy loaded (open a strategy file)".into(),
                                };
                                continue;
                            }
                            Err(e) => {
                                error!("LiveAuto play: strategy flush failed: {}", e);
                                continue;
                            }
                        }
                        let Some(path) = buffer.cache_path.clone() else {
                            warn!("LiveAuto play: no cache_path set");
                            current_run.state = RunState::Failed {
                                error: "No strategy loaded (open a strategy file)".into(),
                            };
                            continue;
                        };

                        if sender
                            .tx
                            .send(TransportCommand::StartLiveAuto {
                                instrument_id,
                                venue: venue_identity,
                                strategy_file: path,
                            })
                            .is_err()
                        {
                            warn!("transport: StartLiveAuto send failed (receiver dropped)");
                        } else {
                            // Optimistically mark Running so a 2nd ▶ press is blocked by the
                            // guard above. run_id stays None → backend_sync is_new_run still
                            // accepts the authoritative RUNNING event (backend_sync.rs:180).
                            current_run.state = RunState::Running;
                        }
                    }
                    ExecutionMode::LiveManual => {}
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

/// ExecutionMode segment クリックを `SetExecutionMode` RPC として backend に送る。
/// Optimistic local state は持たない: `ExecutionModeRes` は polling diff
/// (`BackendStatusUpdate::ExecutionModeChanged`) 経由でのみ更新される。これにより
/// backend が precondition で reject した場合の UI/backend desync を構造的に防ぐ。
///
/// クライアント側 precondition:
/// - Live (LiveManual / LiveAuto) への遷移: venue が Disconnected / Error なら blocked。
/// - Replay への遷移: 常に許可（ホームモード）。strategy 未ロード・replay IDLE でも到達可能。
/// precondition NG の場合は RPC を送らず warn! のみ。OK なら backend に送り、
/// backend 側 `EXECUTION_MODE_PRECONDITION` reject は polling diff で吸収される。
#[allow(clippy::type_complexity)]
pub fn execution_mode_toggle_system(
    query: Query<(&Interaction, &ExecutionModeToggleSegment), (Changed<Interaction>, With<Button>)>,
    exec_mode: Res<ExecutionModeRes>,
    venue: Res<VenueStatusRes>,
    sender: Option<Res<TransportCommandSender>>,
) {
    for (interaction, seg) in &query {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let target = seg.0;
        if exec_mode.mode == target {
            continue;
        }
        // precondition: Live への遷移は venue 接続必須（Replay は無条件許可）
        if matches!(target, ExecutionMode::LiveManual | ExecutionMode::LiveAuto)
            && matches!(venue.state, VenueState::Disconnected | VenueState::Error)
        {
            warn!(
                "ExecutionMode→Live blocked: venue not connected (state={:?})",
                venue.state
            );
            continue;
        }
        let Some(sender) = sender.as_ref() else {
            warn!("execution_mode: TransportCommandSender unavailable; SetExecutionMode dropped");
            continue;
        };
        info!(
            "execution_mode: requesting SetExecutionMode({}) (current: {})",
            target.as_wire_str(),
            exec_mode.mode.as_wire_str()
        );
        if sender
            .tx
            .send(TransportCommand::SetExecutionMode { mode: target })
            .is_err()
        {
            error!("execution_mode: transport channel closed; SetExecutionMode dropped");
        }
    }
}

/// Venue 接続状態に応じて Manual / Auto セグメントボタンの `Node.display` を切り替える。
/// Disconnected / Reconnecting 等は非表示、Connected / Subscribed は表示。
/// Replay ボタンは対象外（常に表示）。
pub fn apply_venue_live_button_visibility_system(
    venue: Res<VenueStatusRes>,
    mut live_btn_q: Query<
        (&ExecutionModeToggleSegment, &mut Node),
        (
            With<Button>,
            Without<PauseResumeButton>,
            Without<TransportButton>,
            Without<SpeedButton>,
        ),
    >,
) {
    if !venue.is_changed() {
        return;
    }
    let live = is_venue_live(venue.state);
    for (seg, mut node) in &mut live_btn_q {
        if matches!(seg.0, ExecutionMode::LiveManual | ExecutionMode::LiveAuto) {
            let target = if live { Display::Flex } else { Display::None };
            if node.display != target {
                node.display = target;
            }
        }
    }
}

/// ExecutionMode に応じて transport / speed ボタンの `Node.display` を切り替える。
#[allow(clippy::type_complexity)]
pub fn apply_execution_mode_visibility_system(
    exec_mode: Res<ExecutionModeRes>,
    mut pause_q: Query<&mut Node, With<PauseResumeButton>>,
    mut transport_q: Query<
        &mut Node,
        (
            With<TransportButton>,
            Without<PauseResumeButton>,
            Without<SpeedButton>,
        ),
    >,
    mut speed_q: Query<
        &mut Node,
        (
            With<SpeedButton>,
            Without<TransportButton>,
            Without<PauseResumeButton>,
        ),
    >,
) {
    if !exec_mode.is_changed() {
        return;
    }

    let pause_target = if matches!(exec_mode.mode, ExecutionMode::Replay | ExecutionMode::LiveAuto) {
        Display::Flex
    } else {
        Display::None
    };
    let replay_target = if matches!(exec_mode.mode, ExecutionMode::Replay) {
        Display::Flex
    } else {
        Display::None
    };

    for mut node in &mut pause_q {
        if node.display != pause_target {
            node.display = pause_target;
        }
    }
    for mut node in &mut transport_q {
        if node.display != replay_target {
            node.display = replay_target;
        }
    }
    for mut node in &mut speed_q {
        if node.display != replay_target {
            node.display = replay_target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{
        CurrentRun, ExecutionMode, ExecutionModeRes, ReplaySpeed, SelectedSymbol,
        TradingSession, TransportCommand, TransportCommandSender, VenueStatusRes,
    };
    use crate::ui::components::{
        PauseResumeButton, SpeedButton, StrategyBuffer, StrategyRunRequested, TransportButton,
    };
    use crate::ui::strategy_editor::StrategyAutoSaveState;
    use tokio::sync::mpsc;

    fn make_input_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
        let mut app = App::new();
        let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();
        app.insert_resource(TransportCommandSender { tx })
            .init_resource::<ExecutionModeRes>()
            .init_resource::<TradingSession>()
            .init_resource::<ReplaySpeed>()
            .init_resource::<StrategyBuffer>()
            .init_resource::<CurrentRun>()
            .init_resource::<SelectedSymbol>()
            .init_resource::<VenueStatusRes>()
            .init_resource::<ScenarioMetadata>()
            .init_resource::<StrategyAutoSaveState>()
            .add_message::<StrategyRunRequested>();
        app.add_systems(
            Update,
            (
                transport_button_system,
                footer_pause_resume_system,
                speed_button_system,
            ),
        );
        (app, rx)
    }

    fn spawn_pressed_transport(app: &mut App, kind: TransportButton) -> Entity {
        app.world_mut()
            .spawn((Node::default(), Button, Interaction::Pressed, kind))
            .id()
    }

    fn spawn_pressed_pause_resume(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((
                Node::default(),
                Button,
                Interaction::Pressed,
                TransportButton::PauseResume,
                PauseResumeButton,
            ))
            .id()
    }

    fn spawn_pressed_speed(app: &mut App, mult: u32) -> Entity {
        app.world_mut()
            .spawn((
                Node::default(),
                Button,
                Interaction::Pressed,
                SpeedButton(mult),
            ))
            .id()
    }

    fn set_mode(app: &mut App, mode: ExecutionMode) {
        app.world_mut().resource_mut::<ExecutionModeRes>().mode = mode;
    }

    #[test]
    fn transport_command_not_sent_in_manual() {
        let (mut app, mut rx) = make_input_app();
        set_mode(&mut app, ExecutionMode::LiveManual);
        app.world_mut()
            .resource_mut::<TradingSession>()
            .replay_state = Some("RUNNING".into());
        let _ = spawn_pressed_transport(&mut app, TransportButton::JumpToStart);
        app.update();
        assert!(
            matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "no TransportCommand must be sent in LiveManual",
        );
    }

    #[test]
    fn transport_command_not_sent_in_auto() {
        let (mut app, mut rx) = make_input_app();
        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.world_mut()
            .resource_mut::<TradingSession>()
            .replay_state = Some("RUNNING".into());
        let _ = spawn_pressed_transport(&mut app, TransportButton::JumpToStart);
        app.update();
        assert!(
            matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "no TransportCommand must be sent in LiveAuto",
        );
    }

    #[test]
    fn pause_resume_does_not_emit_run_event_in_manual() {
        let (mut app, _rx) = make_input_app();
        set_mode(&mut app, ExecutionMode::LiveManual);
        app.world_mut()
            .resource_mut::<TradingSession>()
            .replay_state = Some("IDLE".into());
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("strategy_cache.py");
        app.world_mut().resource_mut::<StrategyBuffer>().cache_path = Some(cache_path);
        let _ = spawn_pressed_pause_resume(&mut app);
        app.update();
        let events = app.world().resource::<Messages<StrategyRunRequested>>();
        let mut reader = events.get_cursor();
        assert_eq!(
            reader.read(events).count(),
            0,
            "StrategyRunRequested must not be emitted in LiveManual",
        );
    }

    #[test]
    fn pause_resume_does_not_emit_run_event_in_auto() {
        let (mut app, _rx) = make_input_app();
        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.world_mut()
            .resource_mut::<TradingSession>()
            .replay_state = Some("IDLE".into());
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("strategy_cache.py");
        app.world_mut().resource_mut::<StrategyBuffer>().cache_path = Some(cache_path);
        let _ = spawn_pressed_pause_resume(&mut app);
        app.update();
        let events = app.world().resource::<Messages<StrategyRunRequested>>();
        let mut reader = events.get_cursor();
        assert_eq!(
            reader.read(events).count(),
            0,
            "StrategyRunRequested must not be emitted in LiveAuto",
        );
    }

    #[test]
    fn speed_command_not_sent_in_live() {
        for mode in [ExecutionMode::LiveManual, ExecutionMode::LiveAuto] {
            let (mut app, mut rx) = make_input_app();
            set_mode(&mut app, mode);
            let _ = spawn_pressed_speed(&mut app, 10);
            app.update();
            assert!(
                matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
                "no SetSpeed must be sent in {:?}",
                mode,
            );
            assert_eq!(
                app.world().resource::<ReplaySpeed>().current,
                1,
                "ReplaySpeed.current must remain default (1) in {:?}",
                mode,
            );
        }
    }

    #[test]
    fn transport_command_sent_in_replay_smoke() {
        let (mut app, mut rx) = make_input_app();
        set_mode(&mut app, ExecutionMode::Replay);
        app.world_mut()
            .resource_mut::<TradingSession>()
            .replay_state = Some("RUNNING".into());
        let _ = spawn_pressed_transport(&mut app, TransportButton::JumpToStart);
        app.update();
        match rx.try_recv() {
            Ok(TransportCommand::ForceStop) => {}
            other => panic!("expected ForceStop in Replay smoke, got {:?}", other),
        }
    }

    fn make_visibility_app() -> App {
        let mut app = App::new();
        app.init_resource::<ExecutionModeRes>();
        app.add_systems(Update, apply_execution_mode_visibility_system);
        app
    }

    fn spawn_transport(app: &mut App, kind: TransportButton) -> Entity {
        app.world_mut().spawn((Node::default(), kind)).id()
    }

    fn spawn_pause_resume_vis(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((
                Node::default(),
                PauseResumeButton,
                TransportButton::PauseResume,
            ))
            .id()
    }

    fn spawn_speed(app: &mut App, mult: u32) -> Entity {
        app.world_mut()
            .spawn((Node::default(), SpeedButton(mult)))
            .id()
    }

    fn display_of(app: &App, e: Entity) -> Display {
        app.world().entity(e).get::<Node>().unwrap().display
    }

    #[test]
    fn transport_buttons_visible_in_replay() {
        let mut app = make_visibility_app();
        let entities = [
            spawn_transport(&mut app, TransportButton::JumpToStart),
            spawn_pause_resume_vis(&mut app),
            spawn_transport(&mut app, TransportButton::StepForward),
            spawn_transport(&mut app, TransportButton::ForceStop),
        ];
        app.update();
        for e in entities {
            assert_eq!(display_of(&app, e), Display::Flex);
        }
    }

    #[test]
    fn pause_resume_visible_in_replay_and_auto_only() {
        let mut app = make_visibility_app();
        let e = spawn_pause_resume_vis(&mut app);

        app.update();
        assert_eq!(display_of(&app, e), Display::Flex);

        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.update();
        assert_eq!(display_of(&app, e), Display::Flex);

        set_mode(&mut app, ExecutionMode::LiveManual);
        app.update();
        assert_eq!(display_of(&app, e), Display::None);

        set_mode(&mut app, ExecutionMode::Replay);
        app.update();
        assert_eq!(display_of(&app, e), Display::Flex);
    }

    #[test]
    fn transport_buttons_flip_back_to_flex_on_replay_return() {
        let mut app = make_visibility_app();
        let e = spawn_transport(&mut app, TransportButton::JumpToStart);
        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.update();
        assert_eq!(display_of(&app, e), Display::None);
        set_mode(&mut app, ExecutionMode::Replay);
        app.update();
        assert_eq!(
            display_of(&app, e),
            Display::Flex,
            "visibility system must write Flex back when returning to Replay",
        );
    }

    #[test]
    fn transport_buttons_hidden_in_manual() {
        let mut app = make_visibility_app();
        let entities = [
            spawn_transport(&mut app, TransportButton::JumpToStart),
            spawn_pause_resume_vis(&mut app),
            spawn_transport(&mut app, TransportButton::StepForward),
            spawn_transport(&mut app, TransportButton::ForceStop),
        ];
        set_mode(&mut app, ExecutionMode::LiveManual);
        app.update();
        for e in entities {
            assert_eq!(display_of(&app, e), Display::None);
        }
    }

    #[test]
    fn non_pause_transport_buttons_hidden_in_auto() {
        let mut app = make_visibility_app();
        let entities = [
            spawn_transport(&mut app, TransportButton::JumpToStart),
            spawn_transport(&mut app, TransportButton::StepForward),
            spawn_transport(&mut app, TransportButton::ForceStop),
        ];
        let pause = spawn_pause_resume_vis(&mut app);
        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.update();
        for e in entities {
            assert_eq!(display_of(&app, e), Display::None);
        }
        assert_eq!(display_of(&app, pause), Display::Flex);
    }

    #[test]
    fn speed_buttons_visible_only_in_replay() {
        let mut app = make_visibility_app();
        let speeds = [
            spawn_speed(&mut app, 1),
            spawn_speed(&mut app, 2),
            spawn_speed(&mut app, 5),
            spawn_speed(&mut app, 10),
            spawn_speed(&mut app, 50),
        ];
        app.update();
        for e in speeds {
            assert_eq!(display_of(&app, e), Display::Flex);
        }
        set_mode(&mut app, ExecutionMode::LiveManual);
        app.update();
        for e in speeds {
            assert_eq!(display_of(&app, e), Display::None);
        }
        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.update();
        for e in speeds {
            assert_eq!(display_of(&app, e), Display::None);
        }
    }

    #[test]
    fn mode_switch_toggles_display() {
        let mut app = make_visibility_app();
        let t = spawn_transport(&mut app, TransportButton::JumpToStart);
        let p = spawn_pause_resume_vis(&mut app);
        let s = spawn_speed(&mut app, 1);
        app.update();
        assert_eq!(display_of(&app, t), Display::Flex);
        assert_eq!(display_of(&app, p), Display::Flex);
        assert_eq!(display_of(&app, s), Display::Flex);

        set_mode(&mut app, ExecutionMode::LiveManual);
        app.update();
        assert_eq!(display_of(&app, t), Display::None);
        assert_eq!(display_of(&app, p), Display::None);
        assert_eq!(display_of(&app, s), Display::None);

        set_mode(&mut app, ExecutionMode::LiveAuto);
        app.update();
        assert_eq!(display_of(&app, t), Display::None);
        assert_eq!(display_of(&app, p), Display::Flex);
        assert_eq!(display_of(&app, s), Display::None);

        set_mode(&mut app, ExecutionMode::Replay);
        app.update();
        assert_eq!(display_of(&app, t), Display::Flex);
        assert_eq!(display_of(&app, p), Display::Flex);
        assert_eq!(display_of(&app, s), Display::Flex);
    }

    #[test]
    fn system_skips_when_mode_unchanged() {
        let mut app = make_visibility_app();
        let e = spawn_transport(&mut app, TransportButton::JumpToStart);
        app.update();
        app.world_mut()
            .entity_mut(e)
            .get_mut::<Node>()
            .unwrap()
            .display = Display::None;
        app.update();
        assert_eq!(
            display_of(&app, e),
            Display::None,
            "system must skip when ExecutionModeRes is unchanged",
        );
    }
}
