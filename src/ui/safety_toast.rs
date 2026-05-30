//! Phase 10 §2.10 — Safety Rail violation toast.
//!
//! A transient warning overlay anchored bottom-right, just above the footer. It is
//! the project's first toast: `OrderFeedback` is a persistent inline line, but a
//! safety-rail violation is a momentary alarm that must catch
//! the eye and then fade (success criterion: "`SafetyRailViolation` トーストが
//! Footer 右下に出る").
//!
//! **Bevy UI Node** 流派 (it lives in the flexbox UI layer with the footer). Spawned
//! once at Startup, `Node.display`-gated, driven by the `SafetyToast` resource which
//! `backend_event_drain_system` sets from a `SafetyRailViolation` push. A single
//! system arms it on a new violation and auto-dismisses it after `TOAST_DURATION_S`.

use bevy::prelude::*;

use crate::trading::{SafetyToast, SafetyToastEntry, ToastKind, short_id};
use crate::ui::theme::Theme;

/// How long a violation toast stays on screen before auto-dismiss.
const TOAST_DURATION_S: f32 = 6.0;

// Mirrors `python/engine/live/safety_rails.py` `KIND_*` — the only kinds the backend
// emits as a `SafetyRailViolation` (independent pre/post-trade rails).
pub const KIND_MAX_DAILY_LOSS: &str = "MAX_DAILY_LOSS";
pub const KIND_MAX_POSITION_SIZE: &str = "MAX_POSITION_SIZE";
pub const KIND_ALLOWED_INSTRUMENTS: &str = "ALLOWED_INSTRUMENTS";

// ===========================================================================
// Pure helpers (testable)
// ===========================================================================

/// Header accent color for a violation `kind`. An unknown / future kind falls back
/// to warning (never panics).
pub fn toast_color(kind: &str, theme: &Theme) -> Color {
    match kind {
        KIND_MAX_DAILY_LOSS => theme.status.error,
        KIND_MAX_POSITION_SIZE | KIND_ALLOWED_INSTRUMENTS => theme.status.warning,
        _ => theme.status.warning,
    }
}

/// Header line: an ASCII-only label (FiraMono has no ⚠ glyph, so no emoji).
pub fn toast_header(entry: &SafetyToastEntry) -> String {
    match entry.toast_kind {
        ToastKind::SafetyRail => format!("SAFETY RAIL — {}", entry.kind),
        ToastKind::BackendError => format!("BACKEND ERROR — {}", entry.kind),
    }
}

/// Body line: the violation detail, with the run id when present.
pub fn toast_body(entry: &SafetyToastEntry) -> String {
    if entry.run_id.is_empty() {
        entry.detail.clone()
    } else {
        format!("{} (run {})", entry.detail, short_id(&entry.run_id, 6))
    }
}

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct SafetyToastRoot;

#[derive(Component, Clone, Copy)]
pub enum SafetyToastCell {
    Header,
    Body,
}

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

/// Spawn the (initially hidden) toast overlay. Sits above the 28px footer.
pub fn spawn_safety_toast(mut commands: Commands, theme: Res<Theme>) {
    let toast_bg = theme.colors.notification_background;
    let header_color = theme.status.warning;
    let body_color = theme.colors.text;
    commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                bottom: Val::Px(36.0),
                right: Val::Px(12.0),
                max_width: Val::Px(380.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(3.0),
                ..default()
            },
            BackgroundColor(toast_bg),
            // Above the Live Run Panel (62) so a violation is never occluded.
            GlobalZIndex(70),
            SafetyToastRoot,
            Name::new("SafetyToast"),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(header_color),
                SafetyToastCell::Header,
            ));
            p.spawn((
                Text::new(""),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(body_color),
                SafetyToastCell::Body,
            ));
        });
}

// ===========================================================================
// System
// ===========================================================================

/// Arm the toast on a new violation and auto-dismiss it after `TOAST_DURATION_S`.
///
/// `remaining` (a `Local`) holds the countdown so the resource is only mutated at
/// show time (by `backend_event_drain_system`) and once at expiry — no per-frame
/// resource churn. Clearing `active` at expiry lets an identical repeat violation
/// re-trigger `is_changed()` next time.
pub fn safety_toast_system(
    mut toast: ResMut<SafetyToast>,
    time: Res<Time>,
    theme: Res<Theme>,
    mut remaining: Local<f32>,
    mut root_q: Query<&mut Node, With<SafetyToastRoot>>,
    mut cells: Query<(&SafetyToastCell, &mut Text, &mut TextColor)>,
) {
    if toast.is_changed()
        && let Some(entry) = toast.active.clone()
    {
        *remaining = TOAST_DURATION_S;
        let accent = toast_color(&entry.kind, &theme);
        let body_color = theme.colors.text;
        for (cell, mut text, mut color) in &mut cells {
            let (s, c) = match cell {
                SafetyToastCell::Header => (toast_header(&entry), accent),
                SafetyToastCell::Body => (toast_body(&entry), body_color),
            };
            if text.0 != s {
                text.0 = s;
            }
            if color.0 != c {
                color.0 = c;
            }
        }
        set_display(&mut root_q, Display::Flex);
    }

    if *remaining > 0.0 {
        *remaining -= time.delta_secs();
        if *remaining <= 0.0 {
            *remaining = 0.0;
            set_display(&mut root_q, Display::None);
            if toast.active.is_some() {
                toast.active = None;
            }
        }
    }
}

fn set_display(root_q: &mut Query<&mut Node, With<SafetyToastRoot>>, target: Display) {
    for mut node in root_q.iter_mut() {
        if node.display != target {
            node.display = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str) -> SafetyToastEntry {
        SafetyToastEntry {
            toast_kind: ToastKind::SafetyRail,
            run_id: "run-abcdef0011".to_string(),
            kind: kind.to_string(),
            detail: "limit exceeded".to_string(),
            ts_ms: 1,
        }
    }

    #[test]
    fn color_maps_by_kind() {
        use crate::ui::theme::Theme;
        let theme = Theme::default();
        assert_eq!(toast_color(KIND_MAX_DAILY_LOSS, &theme), theme.status.error);
        assert_eq!(toast_color(KIND_MAX_POSITION_SIZE, &theme), theme.status.warning);
        assert_eq!(toast_color(KIND_ALLOWED_INSTRUMENTS, &theme), theme.status.warning);
        // Unknown future kind still gets a sane color.
        assert_eq!(toast_color("SOMETHING_NEW", &theme), theme.status.warning);
    }

    #[test]
    fn header_and_body_format() {
        let e = entry("MAX_DAILY_LOSS");
        assert_eq!(toast_header(&e), "SAFETY RAIL — MAX_DAILY_LOSS");
        assert_eq!(toast_body(&e), "limit exceeded (run …ef0011)");
        let mut anon = entry("MAX_POSITION_SIZE");
        anon.run_id = String::new();
        assert_eq!(toast_body(&anon), "limit exceeded");
    }

    #[test]
    fn show_makes_toast_visible_and_sets_text() {
        use crate::ui::theme::Theme;
        let mut app = App::new();
        app.init_resource::<SafetyToast>();
        app.init_resource::<Time>();
        app.insert_resource(Theme::default());
        app.add_systems(Update, safety_toast_system);
        let root = app
            .world_mut()
            .spawn((Node::default(), SafetyToastRoot))
            .id();
        let theme = Theme::default();
        let header = app
            .world_mut()
            .spawn((Text::new(""), TextColor(theme.colors.text), SafetyToastCell::Header))
            .id();
        app.world_mut().resource_mut::<SafetyToast>().show(
            ToastKind::SafetyRail,
            "run-abcdef0011".to_string(),
            "MAX_DAILY_LOSS".to_string(),
            "daily loss limit hit".to_string(),
            1,
        );
        app.update();
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::Flex,
            "toast becomes visible on a new violation"
        );
        assert_eq!(
            app.world().get::<Text>(header).unwrap().0,
            "SAFETY RAIL — MAX_DAILY_LOSS"
        );
        assert_eq!(app.world().get::<TextColor>(header).unwrap().0, theme.status.error);
    }
}
