//! In-memory [`LspTransport`] for tests over `piper` SPSC byte pipes.

use futures::channel::oneshot;
use futures::FutureExt;
use piper::{Reader, Writer};

use super::{BoxedFuture, LspTransport, TransportHandle};

pub const FAKE_PIPE_BUFFER: usize = 64 * 1024;

pub struct FakeTransport {
    client_reader: Reader,
    client_writer: Writer,
    exit_signal: oneshot::Receiver<()>,
}

pub struct FakeTransportEndpoints {
    pub transport: FakeTransport,
    pub server_reader: Reader,
    pub server_writer: Writer,
    pub exit_tx: oneshot::Sender<()>,
}

impl FakeTransport {
    pub fn duplex() -> FakeTransportEndpoints {
        let (server_reader, client_writer) = piper::pipe(FAKE_PIPE_BUFFER);
        let (client_reader, server_writer) = piper::pipe(FAKE_PIPE_BUFFER);
        let (exit_tx, exit_rx) = oneshot::channel();
        FakeTransportEndpoints {
            transport: FakeTransport {
                client_reader,
                client_writer,
                exit_signal: exit_rx,
            },
            server_reader,
            server_writer,
            exit_tx,
        }
    }
}

impl LspTransport for FakeTransport {
    type Reader = Reader;
    type Writer = Writer;
    type Connect = BoxedFuture<std::io::Result<(Self::Reader, Self::Writer, TransportHandle)>>;

    fn connect(self) -> Self::Connect {
        async move {
            let exit_signal = self.exit_signal;
            let exited = async move {
                let _ = exit_signal.await;
            }
            .boxed();
            Ok((
                self.client_reader,
                self.client_writer,
                TransportHandle {
                    auxiliary_tasks: Vec::new(),
                    exited,
                    client_process_id: None,
                },
            ))
        }
        .boxed()
    }
}
