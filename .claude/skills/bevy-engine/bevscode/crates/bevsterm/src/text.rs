//! Core terminal components.

use std::sync::Arc;

use bevy::picking::Pickable;
use bevy::prelude::*;
use parking_lot::Mutex;

use crate::backend;

/// Marker component for a terminal. PTY opens lazily once the viewport
/// produces a non-zero size. Configure via [`TerminalConfig`].
#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
#[require(
    bevy_instanced_text::TextBuffer<bevy_instanced_text::TextSpan>,
    bevy_instanced_text::ContentMetrics,
    bevy_instanced_text::MonoCellWidth,
    bevy_instanced_text::LineStyles,
    bevy_instanced_text::HiddenLines,
    bevy_instanced_text::TextColor,
    bevy_instanced_text::TextBackgroundColor,
    bevy_instanced_text_interaction::SelectionState,
    bevy_instanced_text_interaction::TextViewDragState,
    bevy_instanced_text_interaction::TextCursorColor,
    bevy_instanced_text_interaction::TextSelectionColor,
    bevy_instanced_text_interaction::CursorSettings,
    bevy_instanced_text_interaction::BlinkPhase,
    bevy_instanced_text_interaction::InteractionSettings,
    bevy_instanced_text_interaction::ScrollConfig,
    TerminalGridSnapshot,
    TerminalShellInfo,
    TerminalInputMode,
    TerminalBlockState,
    TerminalColorPalette,
    TerminalScrollback,
    TerminalScrollFollow,
    crate::cursor::TerminalCursorCell,
    Pickable
)]
pub struct BevyTerminal;

/// Per-spawn shell configuration. Read once when the PTY opens; mutating
/// after the session exists has no effect.
#[derive(Component, Clone, Debug, Default, Reflect)]
#[reflect(Component, Default)]
pub struct TerminalConfig {
    /// `None` falls back to `$SHELL` (Unix) / `powershell.exe` (Windows).
    pub shell: Option<String>,
    pub args: Vec<String>,
    /// Layered on top of the parent process's env.
    pub env: Vec<(String, String)>,
    /// `None` falls back to `$HOME` (Unix) / parent cwd (Windows).
    pub cwd: Option<String>,
}

#[derive(Component)]
pub struct TerminalSession {
    pub terminal: Arc<Mutex<backend::Terminal>>,
    pub pty_input: backend::SharedWriter,
    pub size: backend::TerminalSize,
}

/// Channels from the PTY reader thread to the ECS drain system.
#[derive(Component)]
pub struct TerminalEventChannel {
    pub rx: crossbeam_channel::Receiver<Vec<u8>>,
    pub alerts: crossbeam_channel::Receiver<backend::Alert>,
}

/// Grid dimensions and cursor position, updated each frame from the term lock.
#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
pub struct TerminalGridSnapshot {
    pub version: u64,
    pub cols: u16,
    pub rows: u16,
    /// Buffer-line index (0 = top of scrollback).
    pub cursor_row: u32,
    pub cursor_col: u16,
    pub cursor_hidden: bool,
}

/// Shell title and CWD from OSC 0/1/2/7.
#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
pub struct TerminalShellInfo {
    pub title: String,
    pub cwd: Option<String>,
}

/// Terminal mode flags mirrored into ECS to avoid taking the term lock.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component, Default, PartialEq)]
pub struct TerminalInputMode {
    pub cursor_key_application: bool,
    pub keypad_application: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    pub mouse_reporting: bool,
    pub kitty_keyboard: bool,
}

/// OSC 133 command blocks. Empty without shell integration.
#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
pub struct TerminalBlockState {
    pub blocks: Vec<TerminalBlock>,
    pub current_block: Option<usize>,
}

/// One OSC 133 block (prompt + command + output).
#[derive(Clone, Debug, Default, Reflect)]
pub struct TerminalBlock {
    pub id: u64,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    pub prompt_row: i64,
    pub output_row: i64,
    pub end_row: i64,
    pub command_text: String,
}

#[derive(Clone, Copy, Debug, Default, Reflect, PartialEq, Eq)]
pub enum BlockStatus {
    #[default]
    Pending,
    Running,
    Completed,
}

/// ANSI 16-color palette for terminal rendering.
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct TerminalColorPalette {
    pub ansi: [Color; 16],
}

impl Default for TerminalColorPalette {
    fn default() -> Self {
        let ansi = [
            Color::srgb(0.000, 0.000, 0.000), // 0 black
            Color::srgb(0.804, 0.000, 0.000), // 1 red
            Color::srgb(0.000, 0.804, 0.000), // 2 green
            Color::srgb(0.804, 0.804, 0.000), // 3 yellow
            Color::srgb(0.000, 0.000, 0.804), // 4 blue
            Color::srgb(0.804, 0.000, 0.804), // 5 magenta
            Color::srgb(0.000, 0.804, 0.804), // 6 cyan
            Color::srgb(0.898, 0.898, 0.898), // 7 white
            Color::srgb(0.494, 0.494, 0.494), // 8 bright black
            Color::srgb(1.000, 0.000, 0.000), // 9 bright red
            Color::srgb(0.000, 1.000, 0.000), // 10 bright green
            Color::srgb(1.000, 1.000, 0.000), // 11 bright yellow
            Color::srgb(0.357, 0.502, 1.000), // 12 bright blue
            Color::srgb(1.000, 0.000, 1.000), // 13 bright magenta
            Color::srgb(0.000, 1.000, 1.000), // 14 bright cyan
            Color::srgb(1.000, 1.000, 1.000), // 15 bright white
        ];
        Self { ansi }
    }
}

#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct TerminalScrollback {
    pub max_lines: usize,
}

impl Default for TerminalScrollback {
    fn default() -> Self {
        Self { max_lines: 10_000 }
    }
}

/// When `stick_to_bottom` is true, new output auto-scrolls into view.
#[derive(Component, Clone, Copy, Debug, Reflect)]
#[require(ScrollFollowState)]
#[reflect(Component, Default, Debug)]
pub struct TerminalScrollFollow {
    pub stick_to_bottom: bool,
}

impl Default for TerminalScrollFollow {
    fn default() -> Self {
        Self {
            stick_to_bottom: true,
        }
    }
}

/// Tracks the last scroll offset written by the follower, so the apply system
/// can distinguish host-driven scrolls from its own writes.
#[derive(Component, Clone, Copy, Debug, Default, Reflect)]
#[reflect(Component, Default, Debug)]
pub(crate) struct ScrollFollowState {
    pub(crate) last_applied_target: f32,
}
