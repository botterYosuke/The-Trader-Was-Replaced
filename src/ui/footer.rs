use crate::trading::{
    BackendStatus, LastRunResult, ReplaySpeed, RunState, TradingData, TradingSettings,
    TransportCommand, TransportCommandSender,
};
use crate::ui::components::{
    FooterRoot, GrpcStatusLabel, PauseResumeButton, PauseResumeLabel, ReplayStateBadge,
    ReplayTimeLabel, SpeedButton, StrategyBuffer, StrategyEditorId, StrategyFragment,
    StrategyRunRequested, TransportButton, WindowRoot,
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

fn spawn_transport_btn(parent: &mut ChildBuilder, label: &str, action: TransportButton) {
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

fn spawn_speed_btn(parent: &mut ChildBuilder, multiplier: u32, selected: bool) {
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
            // Transport buttons
            spawn_transport_btn(p, "|<", TransportButton::JumpToStart);
            spawn_transport_btn(p, "<", TransportButton::StepBack);
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
pub fn update_footer_system(
    data: Res<TradingData>,
    status: Res<BackendStatus>,
    settings: Res<TradingSettings>,
    buffer: Res<StrategyBuffer>,
    mut time_q: Query<
        &mut Text,
        (
            With<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<GrpcStatusLabel>,
            Without<PauseResumeLabel>,
        ),
    >,
    mut state_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<ReplayStateBadge>,
            Without<ReplayTimeLabel>,
            Without<GrpcStatusLabel>,
            Without<PauseResumeLabel>,
        ),
    >,
    mut grpc_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<GrpcStatusLabel>,
            Without<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<PauseResumeLabel>,
        ),
    >,
    mut pause_q: Query<
        (&mut Text, &mut TextColor),
        (
            With<PauseResumeLabel>,
            Without<ReplayTimeLabel>,
            Without<ReplayStateBadge>,
            Without<GrpcStatusLabel>,
        ),
    >,
) {
    if !data.is_changed() && !status.is_changed() && !settings.is_changed() && !buffer.is_changed()
    {
        return;
    }

    // Timestamp
    for mut text in &mut time_q {
        if data.timestamp_ms > 0 {
            let s = data.timestamp_ms / 1000;
            let ms = data.timestamp_ms % 1000;
            text.0 = format!("time: {}.{:03}", s, ms);
        } else {
            text.0 = "time: --".to_string();
        }
    }

    // Replay state badge
    let replay = data.replay_state.as_deref().unwrap_or("IDLE");
    for (mut text, mut color) in &mut state_q {
        text.0 = format!("state: {}", replay);
        color.0 = match replay {
            "RUNNING" => Color::srgb(0.20, 1.00, 0.45),
            "PAUSED" => Color::srgb(1.00, 0.75, 0.20),
            "LOADED" => Color::srgb(0.35, 0.70, 1.00),
            _ => Color::srgb(0.45, 0.45, 0.45), // IDLE
        };
    }

    // gRPC status
    for (mut text, mut color) in &mut grpc_q {
        if !settings.backend_enabled {
            text.0 = "grpc: DISABLED".to_string();
            color.0 = Color::srgb(0.38, 0.38, 0.38);
        } else if status.connected {
            text.0 = "grpc: OK".to_string();
            color.0 = Color::srgb(0.20, 1.00, 0.45);
        } else if status.last_error.is_some() {
            text.0 = "grpc: ERR".to_string();
            color.0 = Color::srgb(1.00, 0.28, 0.28);
        } else {
            text.0 = "grpc: ...".to_string();
            color.0 = Color::srgb(0.80, 0.75, 0.25);
        }
    }

    // PauseResume label: RUNNING → "||" (Pause action), それ以外 → "▶" (Run/Resume action)。
    // disabled 表現: IDLE/LOADED で cache_path 未設定なら半透明（Run できない）。
    // RUNNING/PAUSED は常に enabled（Pause/Resume は cache_path 不要）。
    let run_disabled = matches!(replay, "IDLE" | "LOADED") && buffer.cache_path.is_none();
    for (mut text, mut color) in &mut pause_q {
        let new_label = match replay {
            "RUNNING" => "||",
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
pub fn transport_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &TransportButton),
        (
            Changed<Interaction>,
            With<Button>,
            Without<PauseResumeButton>,
        ),
    >,
    data: Res<TradingData>,
    sender: Res<TransportCommandSender>,
) {
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
                    TransportButton::StepBack => info!("transport: step_back (not yet wired)"),
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
) {
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
/// `StrategyBuffer` / `StrategyAutoSaveState` / `LastRunResult` / `StrategyRunRequested` という
/// 別系統の依存が必要で、transport_button_system に詰め込むと責務が肥大化する。
/// `With<PauseResumeButton>` で物理的に分離することで、関心を 1 system 1 ボタンに保つ。
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn footer_pause_resume_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<PauseResumeButton>, With<Button>),
    >,
    data: Res<TradingData>,
    sender: Res<TransportCommandSender>,
    mut buffer: ResMut<StrategyBuffer>,
    last_run: Res<LastRunResult>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
    mut run_events: EventWriter<StrategyRunRequested>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), With<WindowRoot>>,
) {
    for (interaction, mut bg) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                match data.replay_state.as_deref() {
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
                    // None / Some("IDLE") / Some("LOADED") / 未知 → Run フロー。
                    // strategy_editor.rs の Run observer と同じ手順で再実装。
                    _ => {
                        if matches!(last_run.state, RunState::Running) {
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
                        run_events.send(StrategyRunRequested { cache_path: path });
                    }
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}
