#![cfg(feature = "pty")]
//! PTY session lifecycle.

use std::io::Read;
use std::sync::Arc;
use std::thread::JoinHandle;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy_instanced_text::MonoCellWidth;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::backend;
use crate::messages::{TerminalReady, TerminalSpawnFailed};
use crate::plugin::TerminalApplyStateSet;
use crate::shell_integration::ShellIntegrationComponent;
use crate::text::{
    BevyTerminal, TerminalConfig, TerminalEventChannel, TerminalScrollback, TerminalSession,
};
use crate::viewport::{cells_from_viewport, MIN_COLS, MIN_ROWS};

pub(crate) struct ReaderHandle {
    pub join: JoinHandle<()>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
    pub pty_master: Mutex<Box<dyn MasterPty + Send>>,
}

#[derive(Resource, Default)]
pub struct TerminalEventLoopRegistry {
    pub(crate) handles: HashMap<Entity, ReaderHandle>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn open_pending_sessions(
    pending: Query<
        (
            Entity,
            &ComputedNode,
            &TextFont,
            &bevy::text::LineHeight,
            &MonoCellWidth,
            Option<&TerminalConfig>,
            Option<&TerminalScrollback>,
            Option<&ShellIntegrationComponent>,
        ),
        (With<BevyTerminal>, Without<TerminalSession>),
    >,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    mut commands: Commands,
    mut registry: ResMut<TerminalEventLoopRegistry>,
    mut ready_w: MessageWriter<TerminalReady>,
    mut failed_w: MessageWriter<TerminalSpawnFailed>,
) {
    let scale = windows
        .single()
        .map(|w| w.scale_factor() as u32)
        .unwrap_or(1)
        .max(1);

    for (entity, computed, font, lh, mono, config, scrollback, integration) in &pending {
        let line_height = bevy_instanced_text::resolve_line_height(*lh, font.font_size);
        let char_width = mono.px;
        let Some((cols, rows)) = cells_from_viewport(computed, char_width, line_height) else {
            continue;
        };
        if cols < MIN_COLS || rows < MIN_ROWS {
            continue;
        }
        let cell_w = char_width.round().max(1.0) as u16;
        let cell_h = line_height.round().max(1.0) as u16;
        let scrollback_lines = scrollback.cloned().unwrap_or_default().max_lines;
        let cmd = build_command(config);

        match build_session(cols, rows, cell_w, cell_h, scale, scrollback_lines, cmd) {
            Ok((session, channel, reader_handle)) => {
                if let Some(si) = integration {
                    si.0.inject(&session.pty_input);
                }
                registry.handles.insert(entity, reader_handle);
                commands.entity(entity).insert((session, channel));
                ready_w.write(TerminalReady { entity, cols, rows });
            }
            Err(err) => {
                let error = err.to_string();
                error!("bevy_terminal: PTY spawn failed for {entity:?}: {error}");
                failed_w.write(TerminalSpawnFailed { entity, error });
                commands.entity(entity).despawn();
            }
        }
    }
}

pub fn sync_pty_size(
    q: Query<(Entity, &TerminalSession), Changed<TerminalSession>>,
    registry: Res<TerminalEventLoopRegistry>,
) {
    for (entity, session) in &q {
        let Some(handle) = registry.handles.get(&entity) else {
            continue;
        };
        let s = &session.size;
        let pty_size = PtySize {
            cols: s.cols as u16,
            rows: s.rows as u16,
            pixel_width: s.pixel_width as u16,
            pixel_height: s.pixel_height as u16,
        };
        let _ = handle.pty_master.lock().resize(pty_size);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_session(
    cols: u16,
    rows: u16,
    cell_w: u16,
    cell_h: u16,
    scale: u32,
    scrollback_lines: usize,
    cmd: CommandBuilder,
) -> std::io::Result<(TerminalSession, TerminalEventChannel, ReaderHandle)> {
    let pty_system = native_pty_system();
    let pty_pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: cols * cell_w,
            pixel_height: rows * cell_h,
        })
        .map_err(io_error)?;

    let writer = pty_pair.master.take_writer().map_err(io_error)?;

    let size = backend::TerminalSize {
        rows: rows as usize,
        cols: cols as usize,
        pixel_width: (cols * cell_w) as usize,
        pixel_height: (rows * cell_h) as usize,
        dpi: scale,
    };

    let config = Arc::new(backend::DefaultConfig {
        scrollback: scrollback_lines,
        ..Default::default()
    }) as Arc<dyn backend::TerminalConfiguration + Send + Sync>;

    let (terminal, alerts_rx, pty_input) = backend::make_terminal(size, config, writer);
    let terminal = Arc::new(Mutex::new(terminal));

    let child = pty_pair.slave.spawn_command(cmd).map_err(io_error)?;
    let killer = child.clone_killer();

    let mut reader = pty_pair.master.try_clone_reader().map_err(io_error)?;

    let (tx, rx) = crossbeam_channel::unbounded::<Vec<u8>>();
    let join = std::thread::Builder::new()
        .name("bevy_terminal_pty_reader".into())
        .spawn(move || {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            drop(child);
        })?;

    Ok((
        TerminalSession {
            terminal,
            pty_input,
            size,
        },
        TerminalEventChannel {
            rx,
            alerts: alerts_rx,
        },
        ReaderHandle {
            join,
            killer,
            pty_master: Mutex::new(pty_pair.master),
        },
    ))
}

fn io_error<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

fn build_command(config: Option<&TerminalConfig>) -> CommandBuilder {
    let shell = config
        .and_then(|c| c.shell.clone())
        .unwrap_or_else(default_shell);
    let mut cmd = CommandBuilder::new(shell);

    if let Some(c) = config {
        cmd.args(&c.args);
        for (k, v) in &c.env {
            cmd.env(k, v);
        }
    }

    let cwd = config
        .and_then(|c| c.cwd.clone())
        .or_else(|| std::env::var("HOME").ok());
    if let Some(cwd) = cwd {
        cmd.cwd(cwd);
    }
    cmd
}

fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".into()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

pub fn on_terminal_removed(
    trigger: On<Remove, BevyTerminal>,
    mut registry: ResMut<TerminalEventLoopRegistry>,
) {
    let entity = trigger.entity;
    let Some(mut handle) = registry.handles.remove(&entity) else {
        return;
    };
    if let Err(e) = handle.killer.kill() {
        if e.kind() != std::io::ErrorKind::InvalidInput && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!("bevy_terminal: kill failed for {entity:?}: {e}");
        }
    }
    if let Err(panic) = handle.join.join() {
        warn!("bevy_terminal: reader thread panicked for {entity:?}: {panic:?}");
    }
}

pub struct TerminalPtyPlugin;

impl Plugin for TerminalPtyPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            app.is_plugin_added::<crate::plugin::TerminalPlugin>(),
            "TerminalPtyPlugin requires TerminalPlugin to be added first"
        );
        app.init_resource::<TerminalEventLoopRegistry>()
            .add_systems(
                Update,
                (
                    open_pending_sessions,
                    sync_pty_size.after(open_pending_sessions),
                )
                    .in_set(TerminalApplyStateSet),
            )
            .add_systems(Update, handle_send_signal.in_set(TerminalApplyStateSet))
            .add_observer(on_terminal_removed);
    }
}

#[cfg(unix)]
pub fn handle_send_signal(
    mut events: MessageReader<crate::messages::TerminalSendSignal>,
    q: Query<Entity, With<TerminalSession>>,
    registry: Res<TerminalEventLoopRegistry>,
) {
    for ev in events.read() {
        if q.get(ev.entity).is_err() {
            continue;
        }
        let Some(handle) = registry.handles.get(&ev.entity) else {
            continue;
        };
        let pgid = handle.pty_master.lock().process_group_leader();
        if let Some(pgid) = pgid {
            unsafe {
                libc::killpg(pgid as libc::pid_t, ev.signal);
            }
        }
    }
}

#[cfg(not(unix))]
pub fn handle_send_signal(mut events: MessageReader<crate::messages::TerminalSendSignal>) {
    for _ in events.read() {}
}
