use bevy::prelude::*;
use crate::trading::{BackendStatus, TradingData, TradingSettings, TransportCommand, TransportCommandSender};
use crate::ui::components::{
    FooterRoot, GrpcStatusLabel, PauseResumeLabel, ReplayStateBadge, ReplayTimeLabel,
    TransportButton,
};

const BTN_NORMAL: Color = Color::srgba(0.12, 0.12, 0.18, 1.0);
const BTN_HOVER: Color = Color::srgba(0.22, 0.22, 0.32, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.35, 0.35, 0.52, 1.0);

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
                TextFont { font_size: 11.0, ..default() },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
            ));
        });
}

pub fn spawn_footer(mut commands: Commands) {
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
            spawn_transport_btn(p, "<",  TransportButton::StepBack);
            // PauseResume: label gets PauseResumeLabel so update_footer_system can toggle it
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
            )).with_children(|pp| {
                pp.spawn((
                    Text::new("||"),
                    TextFont { font_size: 11.0, ..default() },
                    TextColor(Color::srgb(0.85, 0.85, 0.85)),
                    PauseResumeLabel,
                ));
            });
            spawn_transport_btn(p, ">",  TransportButton::StepForward);
            spawn_transport_btn(p, ">>", TransportButton::Run);

            // Flex spacer — pushes status labels to the right
            p.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

            // Status labels
            p.spawn((
                Text::new("time: --"),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.55, 0.55, 0.55)),
                ReplayTimeLabel,
            ));
            p.spawn((
                Text::new("state: IDLE"),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.45, 0.45, 0.45)),
                ReplayStateBadge,
            ));
            p.spawn((
                Text::new("grpc: DISABLED"),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.40, 0.40, 0.40)),
                GrpcStatusLabel,
            ));
        });
}

pub fn update_footer_system(
    data: Res<TradingData>,
    status: Res<BackendStatus>,
    settings: Res<TradingSettings>,
    mut time_q: Query<
        &mut Text,
        (With<ReplayTimeLabel>, Without<ReplayStateBadge>, Without<GrpcStatusLabel>, Without<PauseResumeLabel>),
    >,
    mut state_q: Query<
        (&mut Text, &mut TextColor),
        (With<ReplayStateBadge>, Without<ReplayTimeLabel>, Without<GrpcStatusLabel>, Without<PauseResumeLabel>),
    >,
    mut grpc_q: Query<
        (&mut Text, &mut TextColor),
        (With<GrpcStatusLabel>, Without<ReplayTimeLabel>, Without<ReplayStateBadge>, Without<PauseResumeLabel>),
    >,
    mut pause_q: Query<&mut Text, With<PauseResumeLabel>>,
) {
    if !data.is_changed() && !status.is_changed() && !settings.is_changed() {
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
            "PAUSED"  => Color::srgb(1.00, 0.75, 0.20),
            "LOADED"  => Color::srgb(0.35, 0.70, 1.00),
            _         => Color::srgb(0.45, 0.45, 0.45), // IDLE
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

    // PauseResume label: show "▶" when PAUSED (resume action), "||" otherwise
    let replay = data.replay_state.as_deref().unwrap_or("IDLE");
    for mut text in &mut pause_q {
        text.0 = if replay == "PAUSED" { "▶".to_string() } else { "||".to_string() };
    }
}

pub fn transport_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &TransportButton),
        (Changed<Interaction>, With<Button>),
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
                        let cmd = match replay {
                            "RUNNING" => Some(TransportCommand::Pause),
                            "PAUSED"  => Some(TransportCommand::Resume),
                            other => {
                                info!("transport: pause_resume ignored (state={})", other);
                                None
                            }
                        };
                        if let Some(cmd) = cmd {
                            let _ = sender.tx.send(cmd);
                        }
                    }
                    TransportButton::StepForward => {
                        if replay == "PAUSED" {
                            let _ = sender.tx.send(TransportCommand::StepForward);
                        } else {
                            info!("transport: step_forward ignored (state={})", replay);
                        }
                    }
                    TransportButton::JumpToStart => info!("transport: jump_to_start (not yet wired)"),
                    TransportButton::StepBack    => info!("transport: step_back (not yet wired)"),
                    TransportButton::Run         => info!("transport: run (not yet wired)"),
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None    => bg.0 = BTN_NORMAL,
        }
    }
}
