//! Wezterm / termwiz re-exports and helpers.

use std::io::Write;
use std::sync::Arc;

pub use wezterm_surface::CursorVisibility;
pub use wezterm_term::color::{ColorAttribute, ColorPalette};
pub use wezterm_term::{
    Alert, AlertHandler, CellAttributes, Intensity, SemanticType, SemanticZone, Terminal,
    TerminalConfiguration, TerminalSize, Underline,
};

pub use termwiz::input::{KeyCode, KeyboardEncoding, Modifiers as KeyModifiers};

/// Default `TerminalConfiguration` when the host does not supply one.
#[derive(Debug)]
pub struct DefaultConfig {
    pub scrollback: usize,
    pub palette: ColorPalette,
    pub kitty_keyboard: bool,
    pub csi_u_keys: bool,
}

impl Default for DefaultConfig {
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            palette: ColorPalette::default(),
            kitty_keyboard: false,
            csi_u_keys: false,
        }
    }
}

impl TerminalConfiguration for DefaultConfig {
    fn color_palette(&self) -> ColorPalette {
        self.palette.clone()
    }
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }
    fn enable_kitty_keyboard(&self) -> bool {
        self.kitty_keyboard
    }
    fn enable_csi_u_key_encoding(&self) -> bool {
        self.csi_u_keys
    }
}

pub struct AlertChannel {
    pub tx: crossbeam_channel::Sender<Alert>,
}

impl AlertHandler for AlertChannel {
    fn alert(&mut self, alert: Alert) {
        let _ = self.tx.send(alert);
    }
}

/// Thread-safe PTY input writer, clonable via inner `Arc`.
#[derive(Clone)]
pub struct SharedWriter {
    inner: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
}

impl SharedWriter {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            inner: Arc::new(parking_lot::Mutex::new(writer)),
        }
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut g = self.inner.lock();
        g.write_all(bytes)?;
        g.flush()
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.lock().flush()
    }
}

pub fn make_terminal(
    size: TerminalSize,
    config: Arc<dyn TerminalConfiguration + Send + Sync>,
    writer: Box<dyn Write + Send>,
) -> (Terminal, crossbeam_channel::Receiver<Alert>, SharedWriter) {
    let shared = SharedWriter::new(writer);
    let (tx, rx) = crossbeam_channel::unbounded::<Alert>();
    let mut terminal = Terminal::new(
        size,
        config,
        "bevy_terminal",
        env!("CARGO_PKG_VERSION"),
        Box::new(shared.clone()),
    );
    terminal.set_notification_handler(Box::new(AlertChannel { tx }));
    (terminal, rx, shared)
}
