//! WebSocket transport for `wasm32` Bevy builds. Each message carries raw LSP
//! wire bytes; the host-side bridge pipes them verbatim to/from the server's
//! stdio.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use bevy_log::warn;
use futures::channel::oneshot;
use futures::io::{AsyncRead, AsyncWrite};
use futures::stream::StreamExt;
use futures::FutureExt;
use futures::SinkExt;
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::Message;

use super::{BoxedFuture, LspTransport, TransportHandle};

pub struct WebSocketTransport {
    url: String,
}

impl WebSocketTransport {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

impl LspTransport for WebSocketTransport {
    type Reader = WsReader;
    type Writer = WsWriter;
    type Connect = BoxedFuture<io::Result<(Self::Reader, Self::Writer, TransportHandle)>>;

    fn connect(self) -> Self::Connect {
        async move {
            let ws = WebSocket::open(&self.url)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

            let (sink, stream) = ws.split();

            let buffer = Arc::new(Mutex::new(SharedReadBuffer::default()));
            let (exit_tx, exit_rx) = oneshot::channel::<()>();
            let exit_tx = Arc::new(Mutex::new(Some(exit_tx)));

            let reader = WsReader {
                buffer: buffer.clone(),
            };
            let writer = WsWriter {
                sink: Arc::new(Mutex::new(sink)),
            };

            wasm_bindgen_futures::spawn_local({
                let buffer = buffer.clone();
                let exit_tx = exit_tx.clone();
                async move {
                    let mut stream = stream;
                    while let Some(msg) = stream.next().await {
                        match msg {
                            Ok(Message::Bytes(bytes)) => buffer.lock().unwrap().push(bytes),
                            Ok(Message::Text(text)) => {
                                buffer.lock().unwrap().push(text.into_bytes())
                            }
                            Err(err) => {
                                warn!("[LSP] websocket recv error: {err}");
                                break;
                            }
                        }
                    }
                    buffer.lock().unwrap().close();
                    if let Some(tx) = exit_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                }
            });

            let exited = async move {
                let _ = exit_rx.await;
            }
            .boxed_local();

            Ok((
                reader,
                writer,
                TransportHandle {
                    auxiliary_tasks: Vec::new(),
                    exited,
                    client_process_id: None,
                },
            ))
        }
        .boxed_local()
    }
}

#[derive(Default)]
pub(crate) struct SharedReadBuffer {
    queue: VecDeque<u8>,
    closed: bool,
    waker: Option<Waker>,
}

impl SharedReadBuffer {
    pub(crate) fn push(&mut self, bytes: Vec<u8>) {
        self.queue.extend(bytes);
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    pub(crate) fn close(&mut self) {
        self.closed = true;
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }
}

pub struct WsReader {
    buffer: Arc<Mutex<SharedReadBuffer>>,
}

impl WsReader {
    #[cfg(test)]
    pub(crate) fn from_buffer(buffer: Arc<Mutex<SharedReadBuffer>>) -> Self {
        Self { buffer }
    }
}

impl AsyncRead for WsReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut guard = self.buffer.lock().unwrap();
        if guard.queue.is_empty() {
            if guard.closed {
                return Poll::Ready(Ok(0));
            }
            guard.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let n = guard.queue.len().min(out.len());
        for slot in out.iter_mut().take(n) {
            *slot = guard.queue.pop_front().unwrap();
        }
        Poll::Ready(Ok(n))
    }
}

type WsSink = futures::stream::SplitSink<WebSocket, Message>;

pub struct WsWriter {
    sink: Arc<Mutex<WsSink>>,
}

impl AsyncWrite for WsWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let bytes = buf.to_vec();
        let len = bytes.len();
        let sink = self.sink.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut guard = sink.lock().unwrap();
            if let Err(err) = guard.send(Message::Bytes(bytes)).await {
                warn!("[LSP] websocket send error: {err}");
            }
        });
        Poll::Ready(Ok(len))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::AsyncReadExt;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn ws_reader_round_trips_pushed_bytes() {
        let buffer = Arc::new(Mutex::new(SharedReadBuffer::default()));
        buffer.lock().unwrap().push(b"Conte".to_vec());
        buffer
            .lock()
            .unwrap()
            .push(b"nt-Length: 2\r\n\r\nok".to_vec());
        buffer.lock().unwrap().close();

        let mut reader = WsReader::from_buffer(buffer);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).await.expect("read failed");

        assert_eq!(out, b"Content-Length: 2\r\n\r\nok");
    }

    #[wasm_bindgen_test]
    async fn ws_reader_terminates_on_close() {
        let buffer = Arc::new(Mutex::new(SharedReadBuffer::default()));
        buffer.lock().unwrap().push(b"hi".to_vec());
        buffer.lock().unwrap().close();

        let mut reader = WsReader::from_buffer(buffer);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).await.expect("read failed");
        assert_eq!(out, b"hi");
    }
}
