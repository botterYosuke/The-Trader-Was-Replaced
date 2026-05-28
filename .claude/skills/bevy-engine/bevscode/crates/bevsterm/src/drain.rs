//! Drain PTY bytes + wezterm alerts into ECS state.

use bevy::prelude::*;

use crate::backend;
use crate::messages::*;
use crate::text::{
    TerminalEventChannel, TerminalGridSnapshot, TerminalInputMode, TerminalSession,
    TerminalShellInfo,
};

#[allow(clippy::too_many_arguments)]
pub fn drain_pty_events(
    mut q: Query<(
        Entity,
        &TerminalEventChannel,
        &TerminalSession,
        &mut TerminalShellInfo,
        &mut TerminalInputMode,
        &mut TerminalGridSnapshot,
    )>,
    mut title_w: MessageWriter<TerminalTitleChanged>,
    mut bell_w: MessageWriter<TerminalBell>,
    mut cwd_w: MessageWriter<TerminalCwdChanged>,
) {
    for (entity, channel, session, mut shell, mut mode, mut snapshot) in q.iter_mut() {
        {
            let mut term = session.terminal.lock();
            while let Ok(bytes) = channel.rx.try_recv() {
                term.advance_bytes(&bytes);
            }
        }

        while let Ok(alert) = channel.alerts.try_recv() {
            match alert {
                backend::Alert::Bell => {
                    bell_w.write(TerminalBell { entity });
                }
                backend::Alert::WindowTitleChanged(title) if shell.title != title => {
                    shell.title = title.clone();
                    title_w.write(TerminalTitleChanged { entity, title });
                }
                backend::Alert::CurrentWorkingDirectoryChanged => {
                    let term = session.terminal.lock();
                    if let Some(url) = term.get_current_dir() {
                        let cwd = url.path().to_string();
                        if shell.cwd.as_deref() != Some(&cwd) {
                            shell.cwd = Some(cwd.clone());
                            cwd_w.write(TerminalCwdChanged { entity, cwd });
                        }
                    }
                }
                _ => {}
            }
        }

        let term = session.terminal.lock();
        let new_mode = TerminalInputMode {
            cursor_key_application: false,
            keypad_application: false,
            bracketed_paste: term.bracketed_paste_enabled(),
            alt_screen: term.is_alt_screen_active(),
            mouse_reporting: term.is_mouse_grabbed(),
            kitty_keyboard: matches!(
                term.get_keyboard_encoding(),
                backend::KeyboardEncoding::Kitty(_)
            ),
        };
        if *mode != new_mode {
            *mode = new_mode;
        }
        let seqno = term.current_seqno() as u64;
        if snapshot.version != seqno {
            snapshot.version = seqno;
        }
    }
}
