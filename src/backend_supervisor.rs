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

use crate::trading::engine::{
    data_engine_client::DataEngineClient, health_check_response::ServingStatus,
    health_client::HealthClient, GetStateRequest, HealthCheckRequest,
};
use bevy::prelude::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use tokio::sync::mpsc;
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

/// Commands sent from Bevy systems (Footer Restart button / AppExit handler)
/// to the supervisor task via an mpsc channel. (C-7b)
#[derive(Debug)]
pub enum SupervisorCommand {
    /// Footer [Restart Backend] button.
    Restart,
    /// AppExit / manual Shutdown. `reply_tx` is a std::sync::mpsc::SyncSender
    /// so the main-thread AppExit handler (outside the Tokio runtime context)
    /// can block on it with recv_timeout. `None` when no ack is required.
    Shutdown {
        grace_seconds: u32,
        reply_tx: Option<std::sync::mpsc::SyncSender<()>>,
    },
}

/// Supervisor-side env snapshot, read once when the task is spawned. (C-3..C-4)
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub enabled: bool,
    pub url: String,
    pub token: String,
    pub autospawn: bool,
    pub cwd: Option<String>,
    pub python_bin: Option<String>,
}

impl SupervisorConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("BACKEND_ENABLED")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false),
            url: std::env::var("BACKEND_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:19876".to_string()),
            token: std::env::var("BACKEND_TOKEN").unwrap_or_else(|_| "dev-token".to_string()),
            autospawn: std::env::var("BACKEND_AUTOSPAWN")
                .map(|v| v != "0")
                .unwrap_or(true),
            cwd: std::env::var("BACKEND_CWD").ok().filter(|s| !s.is_empty()),
            python_bin: std::env::var("PYTHON_BIN").ok().filter(|s| !s.is_empty()),
        }
    }
}

/// Parse `BACKEND_URL` into a `host:port` authority for TCP probing.
/// Only `http://` is accepted (Phase 8 has no TLS); a missing port is an
/// error. Returns `Err("BACKEND_URL_INVALID")` on any structural problem. (C-3)
pub fn parse_backend_url(url: &str) -> Result<String, &'static str> {
    let parsed = url::Url::parse(url).map_err(|_| "BACKEND_URL_INVALID")?;
    if parsed.scheme() != "http" {
        return Err("BACKEND_URL_INVALID");
    }
    let host = parsed.host_str().ok_or("BACKEND_URL_INVALID")?;
    let port = parsed.port().ok_or("BACKEND_URL_INVALID")?;
    Ok(format!("{}:{}", host, port))
}

/// Build the argv tail for `python -m engine`. Pure (no env/IO) so the
/// command-line contract (C-4) is unit-testable without spawning.
pub fn build_backend_command_args(token: &str, port: u16) -> Vec<String> {
    vec![
        "-m".to_string(),
        "engine".to_string(),
        "--token".to_string(),
        token.to_string(),
        "--port".to_string(),
        port.to_string(),
    ]
}

/// Parse a backend stdout line for the readiness sentinel
/// `GRPC_LISTENING port=<n>`. Returns the advertised port on match, else
/// `None`. Pure (regex compiled once via OnceLock) so the contract is
/// golden-testable without spawning a subprocess. (C-5)
pub fn parse_sentinel_line(line: &str) -> Option<u16> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"^GRPC_LISTENING port=(\d+)$").unwrap());
    re.captures(line.trim())
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u16>().ok())
}

/// Check whether a sentinel-advertised port matches the expected port from
/// BACKEND_URL. Returns `true` on match. Mismatch is non-fatal: the caller
/// logs an error and continues on the Health.Check path (C-5). Pure so the
/// contract is unit-testable without a subprocess.
pub fn sentinel_port_matches(advertised: u16, expected: u16) -> bool {
    advertised == expected
}

/// Resolve the working directory used as the base for `.venv` discovery and
/// `PYTHONPATH=<cwd>/python`. (C-4)
///
/// Order: `BACKEND_CWD` env -> (release) walk up from `current_exe().parent()`
/// to a dir containing `Cargo.toml` -> (debug) `CARGO_MANIFEST_DIR`.
/// Returns `Err("BACKEND_CWD_NOT_FOUND")` if the release walk finds no
/// `Cargo.toml` ancestor.
pub fn resolve_cwd(cfg_cwd: Option<&str>) -> Result<std::path::PathBuf, &'static str> {
    // 1. explicit BACKEND_CWD (already snapshotted into SupervisorConfig.cwd)
    if let Some(c) = cfg_cwd {
        return Ok(std::path::PathBuf::from(c));
    }
    // 3. debug: compile-time manifest dir
    #[cfg(debug_assertions)]
    {
        return Ok(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    }
    // 2. release: walk up from current_exe parent looking for Cargo.toml
    #[cfg(not(debug_assertions))]
    {
        let exe = std::env::current_exe().map_err(|_| "BACKEND_CWD_NOT_FOUND")?;
        let mut dir = exe.parent();
        while let Some(d) = dir {
            if d.join("Cargo.toml").is_file() {
                return Ok(d.to_path_buf());
            }
            dir = d.parent();
        }
        Err("BACKEND_CWD_NOT_FOUND")
    }
}

/// Resolve the Python interpreter path. (C-4)
///
/// Order: explicit `PYTHON_BIN` (already in cfg) -> `<cwd>/.venv/{bin/python |
/// Scripts/python.exe}` -> `<cwd>/venv/...`. No PATH fallback. Returns
/// `Err("BACKEND_VENV_NOT_FOUND")` if no candidate exists on disk.
pub fn resolve_python_bin(
    cfg_python_bin: Option<&str>,
    cwd: &std::path::Path,
) -> Result<std::path::PathBuf, &'static str> {
    if let Some(p) = cfg_python_bin {
        return Ok(std::path::PathBuf::from(p));
    }
    #[cfg(windows)]
    let rel = ["Scripts", "python.exe"];
    #[cfg(not(windows))]
    let rel = ["bin", "python"];

    for venv_dir in [".venv", "venv"] {
        let cand = cwd.join(venv_dir).join(rel[0]).join(rel[1]);
        if cand.is_file() {
            return Ok(cand);
        }
    }
    Err("BACKEND_VENV_NOT_FOUND")
}

/// Preflight `<python_bin> -c "import engine"` with a 5s timeout, inheriting
/// `PYTHONPATH=<cwd>/python`. Only invoked when `PYTHON_BIN` was set
/// explicitly. (C-4)
///
/// Returns `Err("BACKEND_VENV_NOT_FOUND")` on non-zero exit, spawn failure, or
/// timeout. Logs a distinct line on timeout before failing.
pub fn run_preflight(
    python_bin: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<(), &'static str> {
    use wait_timeout::ChildExt;

    let pythonpath = cwd.join("python");
    let mut child = std::process::Command::new(python_bin)
        .args(["-c", "import engine"])
        .env("PYTHONPATH", &pythonpath)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "BACKEND_VENV_NOT_FOUND")?;

    match child.wait_timeout(std::time::Duration::from_secs(5)) {
        Ok(Some(status)) if status.success() => Ok(()),
        Ok(Some(_)) => Err("BACKEND_VENV_NOT_FOUND"),
        Ok(None) => {
            // timed out
            let _ = child.kill();
            let _ = child.wait();
            bevy::log::warn!("[backend] PYTHON_BIN preflight timed out — assuming venv mismatch");
            Err("BACKEND_VENV_NOT_FOUND")
        }
        Err(_) => Err("BACKEND_VENV_NOT_FOUND"),
    }
}

/// Spawn `python -m engine --token <t> --port <p>` with stdout/stderr piped
/// and `PYTHONPATH=<cwd>/python`. Pure command construction + spawn; the
/// caller owns the returned `Child` (C-7b) and drains the pipes. (C-4/C-5)
pub fn spawn_python_backend(
    python_bin: &std::path::Path,
    cwd: &std::path::Path,
    token: &str,
    port: u16,
) -> std::io::Result<Child> {
    Command::new(python_bin)
        .args(build_backend_command_args(token, port))
        .env("PYTHONPATH", cwd.join("python"))
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// The single supervisor Tokio task (C-7b). Drives the backend through its
/// lifecycle. 4-B-1 scope: env gate + URL parse + AUTOSPAWN=0 short-circuit;
/// TCP probe / spawn / Health.Check / GetState handshake land in 4-B-2.
pub async fn run_supervisor(
    config: SupervisorConfig,
    lifecycle_tx: watch::Sender<BackendLifecycle>,
    cmd_rx: mpsc::UnboundedReceiver<SupervisorCommand>,
    ownership_tx: watch::Sender<BackendOwnership>,
) {
    if !config.enabled {
        let _ = lifecycle_tx.send(BackendLifecycle::Disabled);
        return;
    }

    let authority = match parse_backend_url(&config.url) {
        Ok(a) => a,
        Err(code) => {
            bevy::log::error!("[backend] invalid BACKEND_URL {:?}: {}", config.url, code);
            let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed(code));
            return;
        }
    };

    let _ = lifecycle_tx.send(BackendLifecycle::NotStarted);
    let _ = lifecycle_tx.send(BackendLifecycle::ProbingExisting);

    // TCP probe: 100ms timeout, single attempt.
    let probe_ok = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        tokio::net::TcpStream::connect(&authority),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);

    if probe_ok {
        // Attach path: an existing backend answered the probe.
        run_attach_handshake(&config, &lifecycle_tx).await;
    } else if !config.autospawn {
        // AUTOSPAWN=0: no existing backend and we are forbidden to start one.
        bevy::log::warn!("[backend] TCP probe of {} failed, AUTOSPAWN=0", authority);
        let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_NOT_REACHABLE"));
    } else {
        // Spawn path (C-3b): probe failed and AUTOSPAWN=1.
        // Resolve cwd / interpreter / preflight on a blocking thread (these
        // touch the filesystem and may run a 5s preflight subprocess).
        let resolve_cfg = config.clone();
        let resolved = tokio::task::spawn_blocking(move || {
            let cwd = resolve_cwd(resolve_cfg.cwd.as_deref())?;
            let python_bin = resolve_python_bin(resolve_cfg.python_bin.as_deref(), &cwd)?;
            // Preflight only when PYTHON_BIN was set explicitly (C-4).
            if resolve_cfg.python_bin.is_some() {
                run_preflight(&python_bin, &cwd)?;
            }
            Ok::<_, &'static str>((cwd, python_bin))
        })
        .await;

        let (cwd, python_bin) = match resolved {
            Ok(Ok(paths)) => paths,
            Ok(Err(code)) => {
                bevy::log::error!("[backend] spawn preflight failed: {}", code);
                let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed(code));
                drain_commands(cmd_rx).await;
                return;
            }
            Err(e) => {
                bevy::log::error!("[backend] spawn preflight join error: {}", e);
                let _ = lifecycle_tx
                    .send(BackendLifecycle::StartupFailed("BACKEND_CWD_NOT_FOUND"));
                drain_commands(cmd_rx).await;
                return;
            }
        };

        // Extract the port from BACKEND_URL for the --port arg.
        let port = match url::Url::parse(&config.url).ok().and_then(|u| u.port()) {
            Some(p) => p,
            None => {
                let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_URL_INVALID"));
                drain_commands(cmd_rx).await;
                return;
            }
        };

        // Spawn the subprocess.
        let mut child = match spawn_python_backend(&python_bin, &cwd, &config.token, port) {
            Ok(c) => c,
            Err(e) => {
                bevy::log::error!("[backend] failed to spawn python backend: {}", e);
                let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_VENV_NOT_FOUND"));
                drain_commands(cmd_rx).await;
                return;
            }
        };

        // We started it: claim ownership before announcing Spawning.
        let _ = ownership_tx.send(BackendOwnership { own_process: true });
        let _ = lifecycle_tx.send(BackendLifecycle::Spawning);

        // Drain stdout/stderr on dedicated OS threads. stdout lines are scanned
        // for the readiness sentinel and forwarded to `_sentinel_rx` (consumed
        // in 4-B-2b); stderr is logged at warn. (C-5)
        let (sentinel_tx, mut sentinel_rx) = mpsc::channel::<u16>(16);
        if let Some(stdout) = child.stdout.take() {
            let tx = sentinel_tx.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(p) = parse_sentinel_line(&line) {
                        if let Err(e) = tx.try_send(p) {
                            bevy::log::warn!("[backend] sentinel channel send failed: {}", e);
                        }
                    } else {
                        bevy::log::info!("[backend] {}", line);
                    }
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    bevy::log::warn!("[backend] {}", line);
                }
            });
        }

        // 4-B-2b-ii-β-2b: wait for the readiness sentinel up to a bounded
        // timeout so it becomes the fastest trigger to start the handshake.
        // If the sentinel arrives we validate its advertised port; a mismatch
        // is logged but non-fatal (the handshake still probes BACKEND_URL).
        // If the timeout fires first we fall through to the handshake anyway,
        // so a backend that never emits the sentinel still gets probed. (C-1/C-5)
        tokio::select! {
            maybe_port = sentinel_rx.recv() => {
                match maybe_port {
                    Some(p) if sentinel_port_matches(p, port) => {
                        bevy::log::info!("[backend] readiness sentinel on port {}", p);
                    }
                    Some(p) => {
                        bevy::log::error!(
                            "[backend] sentinel port {} != expected {}; proceeding to handshake",
                            p, port
                        );
                    }
                    None => {
                        bevy::log::warn!("[backend] sentinel channel closed before readiness");
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                bevy::log::warn!("[backend] sentinel not received within 5s; proceeding to handshake");
            }
        }

        // `child` is held alive here so the pipes stay open; AppExit cleanup /
        // crash detection (Step 5) take ownership of it later.
        run_health_and_getstate_handshake(&config, &lifecycle_tx, 75, "BACKEND_SERVICER_MISSING")
            .await;
        // Keep `child` alive until the task ends (drop kills nothing on its own;
        // explicit cleanup is Step 5). Bind to silence unused warnings.
        let _ = &mut child;
    }

    // Keep the task alive draining commands so the channel doesn't error on send.
    drain_commands(cmd_rx).await;
}

/// Drain the supervisor command channel until all senders drop. Restart /
/// Shutdown handling is still a stub here (real handling lands in Step 5);
/// this keeps the task alive so command sends from Bevy don't error.
async fn drain_commands(mut cmd_rx: mpsc::UnboundedReceiver<SupervisorCommand>) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            SupervisorCommand::Restart => {
                bevy::log::info!("[backend] Restart received (no-op stub)");
            }
            SupervisorCommand::Shutdown { reply_tx, .. } => {
                if let Some(tx) = reply_tx {
                    let _ = tx.send(());
                }
            }
        }
    }
}

/// Attach-path handshake (4-B-2a): connect Health + DataEngine clients, tick
/// Health.Check up to 10 times (200ms apart) for SERVING, then GetState once.
/// Maps outcomes to terminal `StartupFailed(_)` codes or `Ready` per C-1/C-3.
/// Never returns early on failure: callers fall through to the command drain.
async fn run_attach_handshake(
    config: &SupervisorConfig,
    lifecycle_tx: &watch::Sender<BackendLifecycle>,
) {
    run_health_and_getstate_handshake(config, lifecycle_tx, 10, "BACKEND_HANDSHAKE_FAILED").await
}

/// Shared Health.Check -> GetState handshake body for both attach and spawn
/// paths. `max_health_ticks` bounds the SERVING poll (attach=10, spawn=75);
/// `getstate_unimplemented_code` is the terminal code emitted when GetState
/// returns `Unimplemented` (attach="BACKEND_HANDSHAKE_FAILED",
/// spawn="BACKEND_SERVICER_MISSING"). Never returns early on failure beyond
/// publishing the terminal `StartupFailed(_)`.
async fn run_health_and_getstate_handshake(
    config: &SupervisorConfig,
    lifecycle_tx: &watch::Sender<BackendLifecycle>,
    max_health_ticks: u32,
    getstate_unimplemented_code: &'static str,
) {
    let mut health = match HealthClient::connect(config.url.clone()).await {
        Ok(c) => c,
        Err(_) => {
            let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_HANDSHAKE_FAILED"));
            return;
        }
    };
    let mut data = match DataEngineClient::connect(config.url.clone()).await {
        Ok(c) => c,
        Err(_) => {
            let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_HANDSHAKE_FAILED"));
            return;
        }
    };

    // Health.Check tick: 200ms interval, max `max_health_ticks` attempts,
    // looking for SERVING.
    let mut serving = false;
    for _ in 0..max_health_ticks {
        match health
            .check(HealthCheckRequest {
                service: "DataEngine".to_string(),
            })
            .await
        {
            Ok(resp) => {
                let status = resp.into_inner().status;
                if status == ServingStatus::Serving as i32 {
                    serving = true;
                    break;
                } else if status == ServingStatus::ServiceUnknown as i32 {
                    let _ = lifecycle_tx
                        .send(BackendLifecycle::StartupFailed("BACKEND_IDENTITY_MISMATCH"));
                    return;
                }
            }
            Err(_) => {}
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if !serving {
        let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_STARTUP_TIMEOUT"));
        return;
    }

    match data
        .get_state(GetStateRequest {
            token: config.token.clone(),
        })
        .await
    {
        Ok(_) => {
            let _ = lifecycle_tx.send(BackendLifecycle::Ready);
        }
        Err(e) if e.code() == tonic::Code::Unauthenticated => {
            let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_TOKEN_MISMATCH"));
        }
        Err(e) if e.code() == tonic::Code::Unimplemented => {
            let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed(getstate_unimplemented_code));
        }
        Err(_) => {
            let _ = lifecycle_tx.send(BackendLifecycle::StartupFailed("BACKEND_HANDSHAKE_FAILED"));
        }
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

/// Read-side handle to the ownership watch channel. The supervisor task flips
/// `own_process=true` via the matching `watch::Sender` when it spawns the
/// backend itself; AppExit cleanup (later Step) reads this to decide whether to
/// fire the `Shutdown` RPC. (C-7b)
#[derive(Resource, Clone)]
pub struct BackendOwnershipHandle {
    rx: watch::Receiver<BackendOwnership>,
}

impl BackendOwnershipHandle {
    pub fn current(&self) -> BackendOwnership {
        *self.rx.borrow()
    }
}

/// Sender side of the supervisor command channel; lives as a Bevy resource so
/// Footer / AppExit systems can enqueue Restart / Shutdown. (C-7b)
#[derive(Resource, Clone)]
pub struct SupervisorCommandSender {
    pub tx: mpsc::UnboundedSender<SupervisorCommand>,
}

/// One-shot carrier for the pieces the Startup system needs to spawn the
/// supervisor task. `take()`-d exactly once by the main.rs Startup system.
#[derive(Resource)]
pub struct SupervisorTaskSeed {
    pub inner: Option<(
        SupervisorConfig,
        watch::Sender<BackendLifecycle>,
        mpsc::UnboundedReceiver<SupervisorCommand>,
        watch::Sender<BackendOwnership>,
    )>,
}

/// Bevy plugin that wires the supervisor into the App.
///
/// 4-A scope: inserts a `BackendLifecycleHandle` whose state is permanently
/// `Disabled`. The actual supervisor Tokio task is spawned in 4-B.
pub struct BackendSupervisorPlugin;

impl Plugin for BackendSupervisorPlugin {
    fn build(&self, app: &mut App) {
        let config = SupervisorConfig::from_env();
        let initial = if config.enabled {
            BackendLifecycle::NotStarted
        } else {
            BackendLifecycle::Disabled
        };
        let (lifecycle_tx, lifecycle_rx) = watch::channel(initial);
        let (ownership_tx, ownership_rx) = watch::channel(BackendOwnership::default());
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<SupervisorCommand>();

        app.insert_resource(BackendLifecycleHandle { rx: lifecycle_rx })
            .insert_resource(BackendOwnershipHandle { rx: ownership_rx })
            .insert_resource(SupervisorCommandSender { tx: cmd_tx })
            .insert_resource(SupervisorTaskSeed {
                inner: Some((config, lifecycle_tx, cmd_rx, ownership_tx)),
            });
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
            .get_resource::<BackendOwnershipHandle>()
            .expect("BackendOwnershipHandle inserted");
        assert!(!own.current().own_process);
    }

    #[test]
    fn parse_backend_url_accepts_http_host_port() {
        assert_eq!(
            parse_backend_url("http://127.0.0.1:19876"),
            Ok("127.0.0.1:19876".to_string())
        );
    }

    #[test]
    fn parse_backend_url_rejects_https() {
        assert_eq!(
            parse_backend_url("https://127.0.0.1:19876"),
            Err("BACKEND_URL_INVALID")
        );
    }

    #[test]
    fn parse_backend_url_rejects_missing_port() {
        assert_eq!(
            parse_backend_url("http://127.0.0.1/"),
            Err("BACKEND_URL_INVALID")
        );
    }

    #[test]
    fn parse_backend_url_rejects_garbage() {
        assert_eq!(parse_backend_url("not a url"), Err("BACKEND_URL_INVALID"));
    }

    #[tokio::test]
    async fn run_supervisor_autospawn_zero_short_circuits() {
        let config = SupervisorConfig {
            enabled: true,
            url: "http://127.0.0.1:1".to_string(),
            token: "x".to_string(),
            autospawn: false,
            cwd: None,
            python_bin: None,
        };
        let (lt, lr) = watch::channel(BackendLifecycle::Disabled);
        let (ct, cr) = mpsc::unbounded_channel();
        drop(ct);
        let (ot, _or) = watch::channel(BackendOwnership::default());
        run_supervisor(config, lt, cr, ot).await;
        assert_eq!(
            *lr.borrow(),
            BackendLifecycle::StartupFailed("BACKEND_NOT_REACHABLE")
        );
    }

    #[tokio::test]
    async fn run_supervisor_spawn_path_venv_not_found() {
        // Probe fails (port 1 is unreachable), AUTOSPAWN=1, explicit PYTHON_BIN
        // that does not exist on disk -> preflight fails before any real spawn.
        let config = SupervisorConfig {
            enabled: true,
            url: "http://127.0.0.1:1".to_string(),
            token: "x".to_string(),
            autospawn: true,
            cwd: Some("/tmp".to_string()),
            python_bin: Some("/no/such/python-binary-xyz".to_string()),
        };
        let (lt, lr) = watch::channel(BackendLifecycle::Disabled);
        let (ct, cr) = mpsc::unbounded_channel();
        drop(ct);
        let (ot, _or) = watch::channel(BackendOwnership::default());
        run_supervisor(config, lt, cr, ot).await;
        assert_eq!(
            *lr.borrow(),
            BackendLifecycle::StartupFailed("BACKEND_VENV_NOT_FOUND")
        );
    }

    #[test]
    fn build_backend_command_args_golden() {
        assert_eq!(
            build_backend_command_args("tok", 19876),
            vec!["-m", "engine", "--token", "tok", "--port", "19876"]
        );
    }

    #[test]
    fn parse_sentinel_line_matches_grpc_listening() {
        assert_eq!(parse_sentinel_line("GRPC_LISTENING port=19876"), Some(19876));
    }

    #[test]
    fn parse_sentinel_line_trims_trailing_newline() {
        assert_eq!(parse_sentinel_line("GRPC_LISTENING port=50051\n"), Some(50051));
    }

    #[test]
    fn parse_sentinel_line_ignores_non_sentinel() {
        assert_eq!(parse_sentinel_line("[engine] starting up"), None);
        assert_eq!(parse_sentinel_line("GRPC_LISTENING port=abc"), None);
        assert_eq!(parse_sentinel_line("prefix GRPC_LISTENING port=1"), None);
    }

    #[test]
    fn sentinel_port_matches_on_equal() {
        assert!(sentinel_port_matches(19876, 19876));
    }

    #[test]
    fn sentinel_port_matches_false_on_mismatch() {
        assert!(!sentinel_port_matches(50051, 19876));
    }

    // --- 4-B-2b-i: pure resolver / preflight unit tests ---

    #[test]
    fn resolve_cwd_uses_explicit_backend_cwd() {
        let got = resolve_cwd(Some("/tmp/explicit-cwd")).expect("explicit cwd ok");
        assert_eq!(got, std::path::PathBuf::from("/tmp/explicit-cwd"));
    }

    #[test]
    fn resolve_python_bin_uses_explicit_bin_without_disk_check() {
        let got = resolve_python_bin(Some("/no/such/python"), std::path::Path::new("/irrelevant"))
            .expect("explicit bin ok");
        assert_eq!(got, std::path::PathBuf::from("/no/such/python"));
    }

    #[test]
    fn resolve_python_bin_finds_dotvenv_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        #[cfg(windows)]
        let rel = std::path::Path::new(".venv").join("Scripts").join("python.exe");
        #[cfg(not(windows))]
        let rel = std::path::Path::new(".venv").join("bin").join("python");
        let py = cwd.join(&rel);
        std::fs::create_dir_all(py.parent().unwrap()).unwrap();
        std::fs::write(&py, b"#!/bin/sh\n").unwrap();

        let got = resolve_python_bin(None, cwd).expect("dotvenv python found");
        assert_eq!(got, py);
    }

    #[test]
    fn resolve_python_bin_missing_venv_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_python_bin(None, dir.path()).unwrap_err();
        assert_eq!(err, "BACKEND_VENV_NOT_FOUND");
    }

    #[test]
    fn run_preflight_succeeds_for_trivial_import() {
        let Some(py) = which_python3() else { return };
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("python");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("engine.py"), b"# importable shim\n").unwrap();
        let r = run_preflight(&py, dir.path());
        assert_eq!(r, Ok(()));
    }

    #[test]
    fn run_preflight_fails_for_unimportable_module() {
        let Some(py) = which_python3() else { return };
        let dir = tempfile::tempdir().unwrap(); // no python/engine.py shim
        let err = run_preflight(&py, dir.path()).unwrap_err();
        assert_eq!(err, "BACKEND_VENV_NOT_FOUND");
    }

    #[test]
    fn run_preflight_fails_for_nonexistent_bin() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_preflight(
            std::path::Path::new("/no/such/python-binary-xyz"),
            dir.path(),
        )
        .unwrap_err();
        assert_eq!(err, "BACKEND_VENV_NOT_FOUND");
    }

    /// Best-effort host python3 locator for preflight tests. Returns None
    /// (test self-skips) if no system python3 is on PATH.
    fn which_python3() -> Option<std::path::PathBuf> {
        let out = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .ok()?;
        if out.status.success() {
            Some(std::path::PathBuf::from("python3"))
        } else {
            None
        }
    }
}
