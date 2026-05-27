//! Stdio transport — spawns the language server as a child process and pipes
//! JSON-RPC over its stdin/stdout. The native default.

use std::process::Stdio;

use async_process::Child;
use bevy_log::debug;
use bevy_tasks::AsyncComputeTaskPool;
use futures::FutureExt;

use super::{BoxedFuture, LspTransport, TransportHandle};

pub struct StdioTransport {
    command: String,
    args: Vec<String>,
}

impl StdioTransport {
    pub fn new(
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

impl LspTransport for StdioTransport {
    type Reader = async_process::ChildStdout;
    type Writer = async_process::ChildStdin;
    type Connect = BoxedFuture<std::io::Result<(Self::Reader, Self::Writer, TransportHandle)>>;

    fn connect(self) -> Self::Connect {
        async move {
            #[cfg(debug_assertions)]
            debug!("[LSP] Starting server: {} {:?}", self.command, self.args);

            let mut child = async_process::Command::new(&self.command)
                .args(&self.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let stdin = child.stdin.take().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child stdin missing")
            })?;
            let stdout = child.stdout.take().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child stdout missing")
            })?;
            let stderr = child.stderr.take().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child stderr missing")
            })?;

            let pool = AsyncComputeTaskPool::get();

            let stderr_task = pool.spawn(async move {
                use futures::AsyncBufReadExt;
                let mut reader = futures::io::BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => debug!("[LSP stderr] {}", line.trim_end()),
                    }
                }
            });

            let exited = async move {
                let _ = Child::status(&mut { child }).await;
            }
            .boxed();

            Ok((
                stdout,
                stdin,
                TransportHandle {
                    auxiliary_tasks: vec![stderr_task],
                    exited,
                    client_process_id: Some(std::process::id()),
                },
            ))
        }
        .boxed()
    }
}
