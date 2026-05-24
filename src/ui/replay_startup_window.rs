//! Center-screen progress window shown while a replay is starting up.
//! Visibility and stage label track `ReplayStartupProgress`; the indeterminate
//! bar sweeps left-right via `Time<Real>`.

use crate::replay::{
    ReplayStartupBarFill, ReplayStartupCloseButton, ReplayStartupPhase, ReplayStartupProgress,
    ReplayStartupStageLabel, ReplayStartupWindow,
};
use bevy::prelude::*;

/// Replay startup progress window を hidden 状態で spawn する。
pub fn spawn_replay_startup_window(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(360.0),
                height: Val::Px(120.0),
                margin: UiRect {
                    left: Val::Px(-180.0),
                    top: Val::Px(-60.0),
                    ..default()
                },
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.09, 0.92)),
            Visibility::Hidden,
            ReplayStartupWindow,
        ))
        .with_children(|p| {
            // Title
            p.spawn((
                Text::new("Starting replay"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.92, 1.0)),
            ));

            // Stage label
            p.spawn((
                Text::new(""),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.82, 0.95)),
                ReplayStartupStageLabel,
            ));

            // Bar container
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(8.0),
                    position_type: PositionType::Relative,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.15, 0.15, 0.22, 1.0)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(30.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.35, 0.55, 0.85, 1.0)),
                    ReplayStartupBarFill,
                ));
            });

            p.spawn((
                Button,
                Node {
                    width: Val::Px(60.0),
                    height: Val::Px(20.0),
                    align_self: AlignSelf::FlexEnd,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.20, 0.20, 0.28, 1.0)),
                Visibility::Hidden,
                ReplayStartupCloseButton,
            ))
            .with_children(|btn| {
                btn.spawn((
                    Text::new("Close"),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.85, 0.9)),
                ));
            });
        });
}

/// `ReplayStartupProgress` resource に応じて window の visibility と stage label を更新する。
pub fn update_replay_startup_window_system(
    progress: Res<ReplayStartupProgress>,
    mut window_q: Query<
        &mut Visibility,
        (With<ReplayStartupWindow>, Without<ReplayStartupCloseButton>),
    >,
    mut label_q: Query<&mut Text, With<ReplayStartupStageLabel>>,
    mut close_btn_q: Query<
        &mut Visibility,
        (With<ReplayStartupCloseButton>, Without<ReplayStartupWindow>),
    >,
) {
    let target_vis = if progress.visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if let Ok(mut visibility) = window_q.get_single_mut() {
        if *visibility != target_vis {
            *visibility = target_vis;
        }
    }

    if let Ok(mut text) = label_q.get_single_mut() {
        let new_label: &str = if let Some(err) = &progress.error {
            err.as_str()
        } else {
            match progress.phase {
                ReplayStartupPhase::Idle => "",
                ReplayStartupPhase::CommandAccepted => "Starting replay command...",
                ReplayStartupPhase::ResettingReplay => "Resetting previous replay...",
                ReplayStartupPhase::LoadingData => "Loading replay data...",
                ReplayStartupPhase::StartingStrategy => "Starting Python strategy...",
                ReplayStartupPhase::WaitingForFirstTick => "Waiting for first replay tick...",
            }
        };
        if text.0 != new_label {
            text.0 = new_label.to_string();
        }
    }

    let target_close_vis = if progress.error.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if let Ok(mut vis) = close_btn_q.get_single_mut() {
        if *vis != target_close_vis {
            *vis = target_close_vis;
        }
    }
}

/// Indeterminate bar の `left` を 0% から 70% まで三角波で往復させる。
pub fn animate_replay_startup_bar_system(
    time: Res<Time<Real>>,
    progress: Res<ReplayStartupProgress>,
    mut fill_q: Query<&mut Node, With<ReplayStartupBarFill>>,
) {
    if !progress.visible || progress.error.is_some() {
        return;
    }
    let t = (time.elapsed_secs() % 2.0) as f32;
    let phase = if t < 1.0 { t } else { 2.0 - t };
    let left_percent = 70.0 * phase;
    if let Ok(mut node) = fill_q.get_single_mut() {
        node.left = Val::Percent(left_percent);
    }
}

/// Close button が押されたら progress を Idle にリセットする。
pub fn replay_startup_close_button_system(
    mut progress: ResMut<ReplayStartupProgress>,
    interaction_q: Query<&Interaction, (Changed<Interaction>, With<ReplayStartupCloseButton>)>,
) {
    for interaction in &interaction_q {
        if matches!(interaction, Interaction::Pressed) {
            progress.visible = false;
            progress.phase = ReplayStartupPhase::Idle;
            progress.detail = None;
            progress.baseline_timestamp_ms = None;
            progress.started_at_elapsed = None;
            progress.start_engine_accepted = false;
            progress.error = None;
        }
    }
}

/// 経路 B: TradingSession の replay_state が RUNNING になる、もしくは timestamp が動いたら hide。
pub fn auto_hide_replay_startup_window_system(
    mut progress: ResMut<ReplayStartupProgress>,
    trading: Res<crate::trading::TradingSession>,
) {
    if !progress.visible || progress.error.is_some() || !progress.start_engine_accepted {
        return;
    }
    let running = trading.replay_state.as_deref() == Some("RUNNING");
    let timestamp_changed = matches!(
        progress.baseline_timestamp_ms,
        Some(b) if trading.timestamp_ms != b
    );
    if running || timestamp_changed {
        progress.visible = false;
        progress.phase = ReplayStartupPhase::Idle;
        progress.detail = None;
        progress.baseline_timestamp_ms = None;
        progress.started_at_elapsed = None;
        progress.start_engine_accepted = false;
        // Leave `progress.error` untouched: errors must remain visible until the
        // user dismisses them via the close button.
    }
}

/// 60s 経っても startup が完了しなければ soft timeout として error をセットする。
pub fn replay_startup_timeout_system(
    time: Res<Time<Real>>,
    mut progress: ResMut<ReplayStartupProgress>,
) {
    if !progress.visible || progress.error.is_some() {
        return;
    }
    let Some(started) = progress.started_at_elapsed else {
        return;
    };
    let elapsed = time.elapsed().saturating_sub(started);
    if elapsed >= std::time::Duration::from_secs(60) {
        progress.error = Some(
            "Replay startup is taking longer than expected. Check backend logs or try Force Stop."
                .to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_replay_startup_window_system_sets_stage_label() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.add_systems(Startup, spawn_replay_startup_window);
        app.add_systems(Update, update_replay_startup_window_system);

        // Startup + 初回 Update を回して spawn を完了させる。
        app.update();

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.phase = ReplayStartupPhase::LoadingData;
        }

        app.update();

        let world = app.world_mut();

        let mut label_q = world.query_filtered::<&Text, With<ReplayStartupStageLabel>>();
        let label_text = label_q.single(world).unwrap();
        assert_eq!(label_text.0, "Loading replay data...");

        let mut win_q = world.query_filtered::<&Visibility, With<ReplayStartupWindow>>();
        let vis = win_q.single(world).unwrap();
        assert!(matches!(vis, Visibility::Visible));
    }

    #[test]
    fn auto_hide_by_running_state() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_replay_startup_window_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = true;
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = Some("RUNNING".to_string());
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
        assert!(matches!(progress.phase, ReplayStartupPhase::Idle));
    }

    #[test]
    fn old_running_does_not_auto_hide() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_replay_startup_window_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = false;
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = Some("RUNNING".to_string());
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(progress.visible);
    }

    #[test]
    fn auto_hide_by_timestamp_change() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_replay_startup_window_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = true;
            progress.baseline_timestamp_ms = Some(1000);
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = None;
            trading.timestamp_ms = 2000;
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
    }

    #[test]
    fn auto_hide_by_timestamp_regression() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_replay_startup_window_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = true;
            progress.baseline_timestamp_ms = Some(2_000_000_000);
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = None;
            trading.timestamp_ms = 1_000_000_000;
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
    }

    #[test]
    fn timeout_sets_error_after_60s() {
        use std::time::Duration;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.add_systems(Update, replay_startup_timeout_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
            progress.started_at_elapsed = Some(Duration::ZERO);
        }
        {
            let mut time = app.world_mut().resource_mut::<Time<Real>>();
            time.advance_by(Duration::from_secs(61));
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(progress.error.is_some());
        assert!(matches!(
            progress.phase,
            ReplayStartupPhase::WaitingForFirstTick
        ));
    }
}
