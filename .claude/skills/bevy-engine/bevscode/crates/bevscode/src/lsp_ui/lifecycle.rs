use bevy::prelude::*;
use lsp_types::*;

/// Dismiss-side lifecycle state shared by all five LSP popups.
///
/// Not a `Component` itself — wrapped by the five typed lifecycle
/// Components ([`HoverLifecycle`] et al.) so each popup gets its own
/// instance on the editor entity and a `Query<&mut HoverLifecycle>`
/// doesn't conflict with `Query<&mut CompletionLifecycle>`.
///
/// The shared concerns live here so we only encode the dismiss state
/// machine once:
///
/// - `request_id` deduplicates LSP responses — bumped on every send
///   AND on every dismiss, so any in-flight response for a popup that
///   has since closed (or been retriggered at a different position)
///   is dropped by the response handler's id check.
/// - `popup_entity` is the chrome `Entity` produced by the renderer.
///   `None` while the popup is hidden; the renderer writes it when it
///   spawns chrome and clears it on dismiss.
/// - `pointer_in_popup` is flipped by `Pointer<Over>`/`Pointer<Out>`
///   observers attached to the popup chrome, so the editor's hover
///   tracker can tell "pointer left the editor, but landed on my own
///   popup" from "pointer actually left everything".
/// - `dismiss_after` is the *grace* timer — `Some` means "schedule
///   dismiss" and the generic tick system fires the feature's
///   `dismiss()` only after it elapses with `pointer_in_popup ==
///   false`. Moving the cursor onto the popup before it fires cancels
///   it.
/// - `hot_zone` is the LSP range the popup is "about" (set from the
///   response). Pointer moves *inside* the range don't re-arm the
///   debounce timer or cancel the visible popup — fixes the "wobble
///   within an identifier dismisses my hover" symptom.
#[derive(Default, Debug)]
pub struct PopupLifecycleData {
    pub request_id: u64,
    /// Highest request id whose response has been accepted into the
    /// popup state. Newer requests are still in flight; older
    /// responses are dropped as stale. This lets us accept whichever
    /// response arrives latest in wall-clock order — important when
    /// the LSP server takes longer to respond than the user takes to
    /// re-arm a new request at a nearby position (rust-analyzer cold
    /// hovers are ~3s; a user moving the mouse may have already armed
    /// requests 2-3 ahead).
    pub last_accepted_id: u64,
    pub popup_entity: Option<Entity>,
    pub pointer_in_popup: bool,
    pub dismiss_after: Option<Timer>,
    pub hot_zone: Option<Range>,
}

impl PopupLifecycleData {
    /// Bump and return the next request id. The send-site stamps it
    /// onto the wire message; the response handler matches.
    pub fn new_request(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }

    /// Bump the id (invalidating any in-flight response), clear the
    /// popup-entity ref and the hot zone, and cancel any pending
    /// dismiss timer. Callers also reset their feature-specific state
    /// (selected_index, content, etc.).
    pub fn dismiss(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.last_accepted_id = self.request_id;
        self.popup_entity = None;
        self.hot_zone = None;
        self.dismiss_after = None;
        self.pointer_in_popup = false;
    }

    /// Returns `true` when a response with `id` should supersede
    /// whatever is currently in the popup. Used by response handlers
    /// in place of a strict `id == request_id` check, which loses
    /// responses whenever the user has re-armed before the previous
    /// reply arrives. Accepts responses for *any* request the editor
    /// has actually sent (`id <= request_id`) and that is newer than
    /// what the popup is currently showing (`id > last_accepted_id`).
    pub fn accept_response(&mut self, id: u64) -> bool {
        if id == 0 || id > self.request_id || id <= self.last_accepted_id {
            return false;
        }
        self.last_accepted_id = id;
        true
    }

    /// Schedule dismissal after `ms` milliseconds. Cancelled by setting
    /// `dismiss_after = None` (e.g. when the pointer re-enters the
    /// popup or the hot zone).
    pub fn arm_dismiss(&mut self, ms: u32) {
        self.dismiss_after = Some(Timer::new(
            std::time::Duration::from_millis(ms as u64),
            TimerMode::Once,
        ));
    }

    /// True when the LSP `hot_zone` covers `position`. False when no
    /// hot zone has been published yet (e.g. between debounce-fire and
    /// response).
    pub fn hot_zone_contains(&self, position: Position) -> bool {
        let Some(range) = self.hot_zone else {
            return false;
        };
        let after_start =
            (position.line, position.character) >= (range.start.line, range.start.character);
        let before_end =
            (position.line, position.character) <= (range.end.line, range.end.character);
        after_start && before_end
    }
}

/// Generate one typed lifecycle Component + its `PopupBackref` peer.
/// Each Component is a newtype around [`PopupLifecycleData`] that
/// `Deref`s through, so call sites look like
/// `lc.new_request()` / `lc.popup_entity = …` rather than
/// `lc.0.new_request()`.
macro_rules! lifecycle_component {
    ($lifecycle:ident, $backref:ident) => {
        #[derive(Component, Default, Debug)]
        pub struct $lifecycle(pub PopupLifecycleData);

        impl std::ops::Deref for $lifecycle {
            type Target = PopupLifecycleData;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::ops::DerefMut for $lifecycle {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        /// Reverse handle inserted on the popup chrome entity so the
        /// chrome's `Pointer<Over>` / `Pointer<Out>` observers can
        /// locate the editor whose lifecycle to mutate.
        #[derive(Component, Debug)]
        pub struct $backref {
            pub editor: Entity,
        }

        impl $backref {
            pub fn from_editor(editor: Entity) -> Self {
                Self { editor }
            }
        }
    };
}

lifecycle_component!(HoverLifecycle, HoverPopupBackref);
lifecycle_component!(CompletionLifecycle, CompletionPopupBackref);
lifecycle_component!(SignatureLifecycle, SignaturePopupBackref);
lifecycle_component!(CodeActionsLifecycle, CodeActionsPopupBackref);
lifecycle_component!(RenameLifecycle, RenamePopupBackref);

/// Marker inserted on a popup chrome entity once its `Pointer<Over>` /
/// `Pointer<Out>` observers have been attached, so renderers can
/// re-spawn the chrome data Component every frame without re-attaching
/// observers (which would multiply per-frame and never deregister).
#[derive(Component, Debug)]
pub struct PopupObserversAttached;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_request_returns_strictly_increasing_ids() {
        let mut lc = PopupLifecycleData::default();
        let a = lc.new_request();
        let b = lc.new_request();
        let c = lc.new_request();
        assert!(a < b && b < c);
        assert_eq!(lc.request_id, c);
    }

    #[test]
    fn dismiss_bumps_id_and_clears_state() {
        let mut lc = PopupLifecycleData::default();
        let id_before = lc.new_request();
        lc.popup_entity = Some(Entity::from_raw_u32(42).unwrap());
        lc.hot_zone = Some(Range::new(Position::new(0, 0), Position::new(0, 5)));
        lc.arm_dismiss(200);
        lc.pointer_in_popup = true;

        lc.dismiss();

        assert_eq!(lc.request_id, id_before.wrapping_add(1));
        assert!(lc.popup_entity.is_none());
        assert!(lc.hot_zone.is_none());
        assert!(lc.dismiss_after.is_none());
        assert!(!lc.pointer_in_popup);
    }

    #[test]
    fn arm_dismiss_sets_timer_of_requested_duration() {
        let mut lc = PopupLifecycleData::default();
        lc.arm_dismiss(250);
        let t = lc.dismiss_after.expect("timer armed");
        assert_eq!(t.duration(), std::time::Duration::from_millis(250));
    }

    #[test]
    fn hot_zone_contains_inclusive_at_boundaries() {
        let lc = PopupLifecycleData {
            hot_zone: Some(Range::new(Position::new(2, 4), Position::new(2, 10))),
            ..Default::default()
        };
        assert!(lc.hot_zone_contains(Position::new(2, 4)));
        assert!(lc.hot_zone_contains(Position::new(2, 7)));
        assert!(lc.hot_zone_contains(Position::new(2, 10)));
        assert!(!lc.hot_zone_contains(Position::new(2, 3)));
        assert!(!lc.hot_zone_contains(Position::new(2, 11)));
        assert!(!lc.hot_zone_contains(Position::new(1, 4)));
    }

    #[test]
    fn hot_zone_contains_false_when_unset() {
        let lc = PopupLifecycleData::default();
        assert!(!lc.hot_zone_contains(Position::new(0, 0)));
    }

    #[test]
    fn accept_response_drops_id_zero() {
        let mut lc = PopupLifecycleData::default();
        lc.new_request();
        assert!(!lc.accept_response(0));
    }

    #[test]
    fn accept_response_drops_unseen_future_ids() {
        let mut lc = PopupLifecycleData::default();
        lc.new_request(); // request_id = 1
        assert!(!lc.accept_response(2));
        assert_eq!(lc.last_accepted_id, 0);
    }

    #[test]
    fn accept_response_drops_already_superseded() {
        let mut lc = PopupLifecycleData::default();
        lc.new_request();
        lc.new_request(); // request_id = 2
        assert!(lc.accept_response(2));
        assert_eq!(lc.last_accepted_id, 2);
        // A late-arriving response for the older request is now stale.
        assert!(!lc.accept_response(1));
    }

    #[test]
    fn accept_response_accepts_older_inflight_when_no_newer_seen() {
        // The case that motivated the helper: user re-arms requests
        // 1, 2, 3 before any response arrives. Response for 2 lands
        // first; it should display. Response for 1 then arrives — it's
        // stale because 2 has already been shown. Response for 3
        // arrives — it should supersede 2.
        let mut lc = PopupLifecycleData::default();
        lc.new_request();
        lc.new_request();
        lc.new_request(); // request_id = 3
        assert!(lc.accept_response(2));
        assert!(!lc.accept_response(1));
        assert!(lc.accept_response(3));
    }
}
