//! A14 — Footer `time:` label advances when backend returns a new timestamp
//! during Replay mode (StepForward → GetState response).
//!
//! Root cause (issue #56): `update_footer_system` ran **before**
//! `backend_update_system` in the same Bevy frame. Because the write and the
//! system's last-run tick were both stamped T, the `is_changed()` guard
//! evaluated `T > T = false` on every subsequent frame, permanently skipping
//! the render path. Fix: `.after(backend_update_system)` ordering constraint.
//!
//! See `tests/e2e/FLOWS.md` A14.

use crate::support::Harness;
use backcast::ui::components::TransportButton;

#[test]
fn a14_footer_time_advances_on_replay_step() {
    let mut h = Harness::new();

    // Initial label: no timestamp available yet.
    assert_eq!(h.footer_time_text(), "time: --");

    // Simulate: user clicks StepForward while PAUSED.
    h.set_replay_state(Some("PAUSED"));
    h.click(TransportButton::StepForward);
    h.drain_commands();

    // Simulate: backend processes the step and GetState returns the new bar
    // timestamp (2021-01-01 09:00:00 JST = 1 609 459 200 000 ms UTC).
    h.push_state(1_609_459_200_000);
    h.set_replay_state(Some("PAUSED"));

    // Footer must now show the advanced time (RED: stays "time: --" without fix).
    assert!(
        h.footer_time_text().contains("2021-01-01"),
        "footer should show 2021-01-01 JST after push_state, got: {:?}",
        h.footer_time_text()
    );
}
