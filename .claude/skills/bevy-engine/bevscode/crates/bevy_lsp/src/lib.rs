#![allow(clippy::type_complexity)]

//! UI-agnostic Language Server Protocol transport for Bevy.
//!
//! Attach [`LspClient`], [`LspDocument`], and [`ServerCapabilities`] to an
//! entity (one per server session). [`LspPlugin`] drains the async transport
//! each frame and fans responses out as typed `Lsp*Response` Bevy messages.
//!
//! The [`pos`] module converts between `ropey` rope offsets and LSP
//! `Position` in UTF-8 / UTF-16 / UTF-32. This crate provides no UI.

pub mod capabilities;
pub mod client;
mod dispatch;
pub mod document;
pub mod messages;
pub mod plugin;
pub mod pos;
pub mod prelude;
pub mod transport;

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub mod test_support;

pub use crate::capabilities::ServerCapabilities;
pub use crate::client::{LspClient, DEFAULT_REQUEST_TIMEOUT_SECS};
pub use crate::document::LspDocument;
pub use crate::messages::{LspRequest, *};
pub use crate::plugin::LspPlugin;
pub use crate::pos::{
    lsp_position_to_rope_byte, lsp_position_to_rope_char, rope_byte_to_lsp_position,
    rope_char_to_lsp_position, rope_range_to_lsp_range, PositionEncoding,
};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::transport::StdioTransport;
#[cfg(target_arch = "wasm32")]
pub use crate::transport::WebSocketTransport;
pub use crate::transport::{LspTransport, TransportHandle};

pub use ::lsp_types;
