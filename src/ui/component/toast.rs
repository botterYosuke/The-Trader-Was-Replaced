//! Toast component — transient notification overlay (Issue #46 Slice F).
//!
//! `spawn_toast` creates a themed, auto-expiring notification anchored to the
//! top-right corner. A `ToastLayer` resource tracks the live queue (max 5)
//! and evicts the oldest when full. `toast_expiry_system` despawns expired
//! entries each frame.

use crate::ui::theme::{ElevationIndex, Theme};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Semantic kind of a [`Toast`]. Determines the accent stripe color.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Info,
    Warning,
    Error,
    Success,
}

/// Marker component carried by every toast root entity.
#[derive(Component)]
pub struct Toast {
    pub kind: ToastKind,
    /// Countdown to auto-despawn.
    pub timer: Timer,
}

/// Application-level queue of live toast entities (max [`TOAST_MAX`]).
///
/// The resource is the single source of truth for which toasts are currently
/// visible. [`spawn_toast`] inserts into it and evicts the oldest entry when
/// the queue is full.
#[derive(Resource, Default)]
pub struct ToastLayer {
    /// Live toast entities, oldest first.
    pub queue: Vec<Entity>,
}

/// Maximum number of simultaneous toasts.
pub const TOAST_MAX: usize = 5;

impl ToastLayer {
    /// Return the accent [`Color`] for a given [`ToastKind`] against `theme`.
    pub fn kind_color(kind: ToastKind, theme: &Theme) -> Color {
        match kind {
            ToastKind::Info    => theme.status.info,
            ToastKind::Warning => theme.status.warning,
            ToastKind::Error   => theme.status.error,
            ToastKind::Success => theme.status.success,
        }
    }
}

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

/// Spawn a transient toast notification and register it in [`ToastLayer`].
///
/// If the queue already holds [`TOAST_MAX`] entries, the **oldest** entity is
/// despawned before the new one is inserted.
///
/// Returns the root entity of the new toast.
pub fn spawn_toast(
    commands: &mut Commands,
    theme: &Theme,
    layer: &mut ToastLayer,
    msg: impl Into<String>,
    kind: ToastKind,
    duration_secs: f32,
) -> Entity {
    // Evict oldest when at capacity.
    if layer.queue.len() >= TOAST_MAX {
        let oldest = layer.queue.remove(0);
        commands.entity(oldest).despawn();
    }

    let bg = ElevationIndex::Notification.background(theme);
    let accent = ToastLayer::kind_color(kind, theme);
    let z = ElevationIndex::Notification.z();

    let root = commands
        .spawn((
            Toast {
                kind,
                timer: Timer::from_seconds(duration_secs, TimerMode::Once),
            },
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                padding: UiRect::all(Val::Px(0.0)),
                ..default()
            },
            BackgroundColor(bg),
            ZIndex(z as i32),
        ))
        .with_children(|p| {
            // Accent stripe (left border visual).
            p.spawn((
                Node {
                    width: Val::Px(4.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(accent),
            ));
            // Message text.
            p.spawn((
                Text::new(msg.into()),
                TextColor(theme.colors.text),
            ));
        })
        .id();

    layer.queue.push(root);
    root
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Tick every live toast timer; despawn and dequeue finished ones.
pub fn toast_expiry_system(
    mut commands: Commands,
    mut layer: ResMut<ToastLayer>,
    mut toasts: Query<(Entity, &mut Toast)>,
    time: Res<Time>,
) {
    let mut expired = std::collections::HashSet::new();
    for (entity, mut toast) in &mut toasts {
        toast.timer.tick(time.delta());
        if toast.timer.just_finished() {
            commands.entity(entity).despawn();
            expired.insert(entity);
        }
    }
    layer.queue.retain(|e| !expired.contains(e));
}

// ---------------------------------------------------------------------------
// Tests (TDD: RED first — compile but assert-fail before spawn_toast exists)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    /// The kind_color helper must return the correct status color from theme.
    #[test]
    fn kind_color_returns_status_color() {
        let theme = Theme::default();
        assert_eq!(ToastLayer::kind_color(ToastKind::Info, &theme),    theme.status.info);
        assert_eq!(ToastLayer::kind_color(ToastKind::Warning, &theme), theme.status.warning);
        assert_eq!(ToastLayer::kind_color(ToastKind::Error, &theme),   theme.status.error);
        assert_eq!(ToastLayer::kind_color(ToastKind::Success, &theme), theme.status.success);
    }

    /// ToastLayer starts empty; TOAST_MAX is 5.
    #[test]
    fn toast_layer_default_is_empty() {
        let layer = ToastLayer::default();
        assert!(layer.queue.is_empty());
        assert_eq!(TOAST_MAX, 5);
    }

    /// spawn_toast pushes into the queue and the entity is recorded.
    #[test]
    fn spawn_toast_adds_to_queue() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let theme = Theme::default();
        let mut layer = ToastLayer::default();

        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, &world);

        let e = spawn_toast(&mut commands, &theme, &mut layer, "hello", ToastKind::Info, 3.0);

        assert_eq!(layer.queue.len(), 1);
        assert_eq!(layer.queue[0], e);
    }

    /// When TOAST_MAX entries exist, the oldest is evicted on the next spawn.
    #[test]
    fn spawn_toast_evicts_oldest_at_capacity() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let theme = Theme::default();
        let mut layer = ToastLayer::default();

        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, &world);

        let mut entities = Vec::new();
        for i in 0..TOAST_MAX {
            let e = spawn_toast(
                &mut commands, &theme, &mut layer,
                format!("msg {}", i), ToastKind::Info, 3.0,
            );
            entities.push(e);
        }
        assert_eq!(layer.queue.len(), TOAST_MAX);

        // One more push should evict the oldest (entities[0]).
        let _new = spawn_toast(&mut commands, &theme, &mut layer, "new", ToastKind::Warning, 3.0);

        assert_eq!(layer.queue.len(), TOAST_MAX);
        assert!(!layer.queue.contains(&entities[0]), "oldest should have been evicted");
    }
}
