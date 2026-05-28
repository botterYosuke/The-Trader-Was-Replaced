//! Byte transport abstraction for the JSON-RPC connection to a language server.
//! Stdio on native, WebSocket in the browser.

use bevy_tasks::Task;
use futures::io::{AsyncRead, AsyncWrite};
use std::future::Future;

#[cfg(not(target_arch = "wasm32"))]
pub mod stdio;
#[cfg(not(target_arch = "wasm32"))]
pub use stdio::StdioTransport;

#[cfg(target_arch = "wasm32")]
pub mod websocket;
#[cfg(target_arch = "wasm32")]
pub use websocket::WebSocketTransport;

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub mod fake;
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub use fake::{FakeTransport, FakeTransportEndpoints};

/// `Send`-boxed future on native, `!Send` on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxedFuture<T> = futures::future::BoxFuture<'static, T>;
#[cfg(target_arch = "wasm32")]
pub type BoxedFuture<T> = futures::future::LocalBoxFuture<'static, T>;

pub struct TransportHandle {
    pub auxiliary_tasks: Vec<Task<()>>,
    /// Resolves when the transport exits.
    pub exited: BoxedFuture<()>,
    /// `None` on transports without a local process (WebSocket, in-browser).
    pub client_process_id: Option<u32>,
}

/// `Send` on native, vacuous on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

pub trait LspTransport: MaybeSend + 'static {
    type Reader: AsyncRead + MaybeSend + Unpin + 'static;
    type Writer: AsyncWrite + MaybeSend + Unpin + 'static;
    type Connect: Future<Output = std::io::Result<(Self::Reader, Self::Writer, TransportHandle)>>
        + MaybeSend
        + 'static;

    fn connect(self) -> Self::Connect;
}
