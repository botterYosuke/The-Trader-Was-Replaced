//! Backend lifecycle supervisor.
//!
//! Owns a single Tokio task that drives the Python `data_engine` backend through
//! its lifecycle states (probe existing / spawn / Health.Check / GetState handshake
//! / graceful shutdown / crash detection). The Bevy ECS side reads the current
//! state via `BackendLifecycleHandle` (watch::Receiver) and renders it in the
//! Footer; ECS systems never drive transitions themselves.
//!
//! See plans/backend-startup-sync.md §Step 4 (C-1 .. C-8) for the full spec.
//!
//! NOTE (4-A scope): this file currently provides only the type skeleton and a
//! placeholder plugin that publishes `BackendLifecycle::Disabled`. The actual
//! state machine driver (TCP probe, spawn, Health.Check tick, GetState
//! handshake, stdout drain) lands in Step 4-B.

use bevy::prelude::*;
use tokio::sync::watch;

/// Lifecycle phases of the Python backend process / connection.
///
/// `&'static str` payload on `StartupFailed` carries the error code
/// (`BACKEND_NOT_REACHABLE`, `BACKEND_URL_INVALID`, `BACKEND_IDENTITY_MISMATCH`,
/// `BACKEND_TOKEN_MISMATCH`, `BACKEND_SERVICER_MISSING`, `BACKEND_STARTUP_TIMEOUT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendLifecycle {
    /// `BACKEND_ENABLED=false` — supervisor is inert.
    Disabled,
    /// Initial state before any probe.
    NotStarted,
    /// Trying to TCP-connect to an existing backend at `BACKEND_URL`.
    ProbingExisting,
    /// `python -m engine` subprocess has been spawned; waiting for sentinel /
    /// Health.Check SERVING.
    Spawning,
    /// Health.Check returned SERVING and token-bearing GetState succeeded once.
    Ready,
    /// `Shutdown` RPC accepted; waiting for NOT_SERVING / subprocess exit.
    ShuttingDown,
    /// Graceful shutdown observed (NOT_SERVING * N or subprocess exit_code=0).
    /// Not counted against the crash-loop budget.
    Stopped,
    /// Health.Check failed 3 consecutive times outside of ShuttingDown, or
    /// subprocess exited with non-zero before Ready.
    Crashed,
    /// Terminal: startup failed with a structural error (see error code list).
    StartupFailed(&'static str),
}

impl BackendLifecycle {
    /// `true` while the connection task should drive its GetState polling
    /// inner loop. Bevy UI uses this to enable/disable transport buttons.
    pub fn is_ready(self) -> bool {
        matches!(self, BackendLifecycle::Ready)
    }
}

/// Whether this Bevy process owns the Python subprocess (spawn path) or just
/// attached to an existing one. AppExit cleanup only fires `Shutdown` RPC when
/// `own_process == true` (prevents collateral kill of an externally-managed
/// backend).
#[derive(Resource, Debug, Clone, Copy)]
pub struct BackendOwnership {
    pub own_process: bool,
}

impl Default for BackendOwnership {
    fn default() -> Self {
        // Default to `false`: until the supervisor decides to spawn, we
        // conservatively assume we did not start the backend ourselves.
        Self { own_process: false }
    }
}

/// Read-side handle to the lifecycle watch channel. Cloned freely into Bevy
/// systems and the connection Tokio task.
#[derive(Resource, Clone)]
pub struct BackendLifecycleHandle {
    rx: watch::Receiver<BackendLifecycle>,
}

impl BackendLifecycleHandle {
    pub fn current(&self) -> BackendLifecycle {
        *self.rx.borrow()
    }

    /// Clone the underlying watch::Receiver for use inside a Tokio task
    /// (e.g. `setup_backend_connection` awaits `Ready` here).
    pub fn subscribe(&self) -> watch::Receiver<BackendLifecycle> {
        self.rx.clone()
    }
}

/// Bevy plugin that wires the supervisor into the App.
///
/// 4-A scope: inserts a `BackendLifecycleHandle` whose state is permanently
/// `Disabled`. The actual supervisor Tokio task is spawned in 4-B.
pub struct BackendSupervisorPlugin;

impl Plugin for BackendSupervisorPlugin {
    fn build(&self, app: &mut App) {
        // Create the watch channel up-front so any system added in the same
        // App build can clone the Receiver. The Sender is dropped at the end
        // of this function in 4-A; that is intentional — the Receiver will
        // simply report the last value (`Disabled`) forever.
        //
        // In 4-B we will instead move the Sender into the supervisor Tokio
        // task spawned from a Startup system.
        let (tx, rx) = watch::channel(BackendLifecycle::Disabled);
        // TODO(4-B): move `tx` into the supervisor task spawned in Startup.
        drop(tx);

        app.insert_resource(BackendLifecycleHandle { rx })
            .insert_resource(BackendOwnership::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_is_ready_only_for_ready() {
        assert!(BackendLifecycle::Ready.is_ready());
        assert!(!BackendLifecycle::Disabled.is_ready());
        assert!(!BackendLifecycle::NotStarted.is_ready());
        assert!(!BackendLifecycle::ProbingExisting.is_ready());
        assert!(!BackendLifecycle::Spawning.is_ready());
        assert!(!BackendLifecycle::ShuttingDown.is_ready());
        assert!(!BackendLifecycle::Stopped.is_ready());
        assert!(!BackendLifecycle::Crashed.is_ready());
        assert!(!BackendLifecycle::StartupFailed("BACKEND_NOT_REACHABLE").is_ready());
    }

    #[test]
    fn startup_failed_carries_error_code() {
        let s = BackendLifecycle::StartupFailed("BACKEND_TOKEN_MISMATCH");
        match s {
            BackendLifecycle::StartupFailed(code) => assert_eq!(code, "BACKEND_TOKEN_MISMATCH"),
            _ => panic!("expected StartupFailed"),
        }
    }

    #[test]
    fn ownership_defaults_to_attached() {
        let o = BackendOwnership::default();
        assert!(!o.own_process);
    }

    #[test]
    fn plugin_inserts_resources_at_disabled() {
        let mut app = App::new();
        app.add_plugins(BackendSupervisorPlugin);
        // Run schedule once so plugin `build` has settled.
        app.update();

        let handle = app
            .world()
            .get_resource::<BackendLifecycleHandle>()
            .expect("BackendLifecycleHandle inserted");
        assert_eq!(handle.current(), BackendLifecycle::Disabled);

        let own = app
            .world()
            .get_resource::<BackendOwnership>()
            .expect("BackendOwnership inserted");
        assert!(!own.own_process);
    }
}
