//! Terminal message types (outbound events and inbound commands).

use bevy::prelude::*;
use std::process::ExitStatus;

use crate::backend::{KeyCode as TerminalKeyCode, KeyModifiers as TerminalKeyModifiers};

#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Debug)]
pub struct TerminalExited {
    pub entity: Entity,
    /// `None` when the OS didn't deliver a status (e.g. detached PTY).
    #[reflect(ignore)]
    pub status: Option<ExitStatusReflect>,
}

#[derive(Clone, Debug)]
pub struct ExitStatusReflect(pub ExitStatus);

/// OSC 0/1/2 title change.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalTitleChanged {
    pub entity: Entity,
    pub title: String,
}

/// BEL (`\a`).
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalBell {
    pub entity: Entity,
}

/// PTY is up and the initial dimensions are known.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalReady {
    pub entity: Entity,
    pub cols: u16,
    pub rows: u16,
}

/// PTY allocation or shell spawn failed.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalSpawnFailed {
    pub entity: Entity,
    pub error: String,
}

/// OSC 7 working directory change.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalCwdChanged {
    pub entity: Entity,
    pub cwd: String,
}

/// OSC 133 D -- a command block completed.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalBlockFinished {
    pub entity: Entity,
    pub block_id: u64,
    pub exit_code: Option<i32>,
}

#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalBlockSelected {
    pub entity: Entity,
    pub block_id: u64,
}

#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalScrollFollowChanged {
    pub entity: Entity,
    pub stick_to_bottom: bool,
}

/// Write raw bytes to the PTY — no interpretation, no newline appended.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalWriteBytes {
    pub entity: Entity,
    pub bytes: Vec<u8>,
}

/// Write a string + Enter to the PTY.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalRunCommand {
    pub entity: Entity,
    pub command: String,
}

#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalCopySelection {
    pub entity: Entity,
}

/// Paste text into the PTY (bracketed if the mode is active).
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalPaste {
    pub entity: Entity,
    pub text: String,
}

/// Force a (cols, rows) resize without owning the viewport.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalResize {
    pub entity: Entity,
    pub cols: u16,
    pub rows: u16,
}

/// Scroll to a buffer row (0 = top of scrollback). Disengages bottom-follow.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalScrollTo {
    pub entity: Entity,
    pub line: i64,
}

#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalScrollToBottom {
    pub entity: Entity,
}

/// Jump to the top of scrollback. Disengages bottom-follow.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalScrollToTop {
    pub entity: Entity,
}

/// Clear screen and scrollback.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalClear {
    pub entity: Entity,
}

/// Synthesize a keypress, encoded according to the terminal's current input mode.
#[derive(Message, Clone, Debug)]
pub struct TerminalKeyInput {
    pub entity: Entity,
    pub key: TerminalKeyCode,
    pub mods: TerminalKeyModifiers,
}

/// Forward a POSIX signal to the PTY child. Ignored on Windows.
#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalSendSignal {
    pub entity: Entity,
    pub signal: i32,
}

#[derive(Message, Clone, Debug, Reflect)]
pub struct TerminalFocus {
    pub entity: Entity,
}
