//! LSP client transport over Bevy's `AsyncComputeTaskPool`, with an
//! async-channel bridge into ECS via [`LspClient::try_recv`].

use std::collections::HashMap;
use std::future::Future;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ResponseError, ServerSocket};
use bevy_ecs::prelude::*;
use bevy_log::{debug, info, warn};
use bevy_tasks::{AsyncComputeTaskPool, Task};
use futures::channel::oneshot;

#[cfg(not(target_arch = "wasm32"))]
use crate::transport::StdioTransport;
use crate::transport::{LspTransport, MaybeSend};

/// Spawn on Bevy's compute pool: `spawn_local` on wasm32 (`!Send` handles),
/// thread-pooled `spawn` elsewhere.
fn spawn_task<Fut>(future: Fut) -> Task<()>
where
    Fut: Future<Output = ()> + MaybeSend + 'static,
{
    let pool = AsyncComputeTaskPool::get();
    #[cfg(not(target_arch = "wasm32"))]
    {
        pool.spawn(future)
    }
    #[cfg(target_arch = "wasm32")]
    {
        pool.spawn_local(future)
    }
}
use lsp_types::notification::{
    Initialized as InitializedNotif, LogMessage, LogTrace,
    Notification as LspNotificationTrait, Progress, PublishDiagnostics, ShowMessage,
    TelemetryEvent,
};
use lsp_types::request::{
    ApplyWorkspaceEdit, CodeLensRefresh, Initialize as InitializeRequest,
    InlayHintRefreshRequest, RegisterCapability, Request as LspRequestTrait,
    SemanticTokensRefresh, ShowDocument, ShowMessageRequest, UnregisterCapability,
    WorkDoneProgressCreate, WorkspaceConfiguration, WorkspaceDiagnosticRefresh,
    WorkspaceFoldersRequest,
};
use lsp_types::*;
use tower::ServiceBuilder;

use super::messages::{LspMessage, LspResponse};
use crate::dispatch::dispatch;

pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

pub(crate) type ReplySlots<R> = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<R, ResponseError>>>>>;

#[derive(Default)]
pub(crate) struct InboundReplySlots {
    pub(crate) configuration: ReplySlots<Vec<serde_json::Value>>,
    pub(crate) apply_edit: ReplySlots<ApplyWorkspaceEditResponse>,
    pub(crate) show_message: ReplySlots<Option<MessageActionItem>>,
    pub(crate) show_document: ReplySlots<ShowDocumentResult>,
    pub(crate) work_done_progress_create: ReplySlots<()>,
    pub(crate) register_capability: ReplySlots<()>,
    pub(crate) unregister_capability: ReplySlots<()>,
    pub(crate) workspace_folders: ReplySlots<Option<Vec<WorkspaceFolder>>>,
}

#[derive(Component)]
pub struct LspClient {
    server: Option<ServerSocket>,
    response_tx: async_channel::Sender<LspResponse>,
    response_rx: async_channel::Receiver<LspResponse>,
    pub(crate) initialized: bool,
    _tasks: Vec<Task<()>>,
    pub(crate) pre_init_queue: Vec<LspMessage>,
    init_done: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
    next_inbound_request_id: Arc<AtomicU64>,
    inbound_slots: Arc<InboundReplySlots>,
    client_process_id: Arc<OnceLock<Option<u32>>>,
}

impl Default for LspClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LspClient {
    pub fn new() -> Self {
        let (response_tx, response_rx) = async_channel::unbounded();
        Self {
            server: None,
            response_tx,
            response_rx,
            initialized: false,
            _tasks: Vec::new(),
            pre_init_queue: Vec::new(),
            init_done: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            next_inbound_request_id: Arc::new(AtomicU64::new(1)),
            inbound_slots: Arc::new(InboundReplySlots::default()),
            client_process_id: Arc::new(OnceLock::new()),
        }
    }

    /// Convenience for [`Self::start_with`] with a [`StdioTransport`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(&mut self, command: &str, args: &[&str]) -> std::io::Result<()> {
        self.start_with(StdioTransport::new(command, args.iter().copied()));
        Ok(())
    }

    /// Spawn the language server using `transport`. Transport-side failures
    /// surface as [`LspResponse::Crashed`].
    pub fn start_with<T: LspTransport>(&mut self, transport: T) {
        let bridge_tx = self.response_tx.clone();
        let next_id = self.next_inbound_request_id.clone();
        let slots = self.inbound_slots.clone();
        let (mainloop, server) = async_lsp::MainLoop::new_client(move |_server| {
            let router = build_router(bridge_tx.clone(), next_id.clone(), slots.clone());
            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        self.server = Some(server);

        let watchdog_tx = self.response_tx.clone();
        let watchdog_flag = self.shutting_down.clone();
        let pid_slot = self.client_process_id.clone();

        info!("[LSP] start_with: scheduling driver task on AsyncComputeTaskPool");
        let driver = spawn_task(async move {
            info!("[LSP] driver task started; calling transport.connect()");
            let (reader, writer, handle) = match transport.connect().await {
                Ok(t) => {
                    info!("[LSP] transport.connect() succeeded");
                    t
                }
                Err(err) => {
                    warn!("[LSP] transport connect failed: {err}");
                    let _ = watchdog_tx.try_send(LspResponse::Crashed);
                    return;
                }
            };
            let _ = pid_slot.set(handle.client_process_id);
            info!(
                "[LSP] transport pid={:?}; entering MainLoop::run_buffered",
                handle.client_process_id,
            );
            let _aux = handle.auxiliary_tasks;

            let outcome = mainloop.run_buffered(reader, writer).await;
            info!(
                "[LSP] MainLoop::run_buffered returned outcome={:?}",
                outcome.as_ref().map(|_| ()).map_err(|e| e.to_string())
            );
            handle.exited.await;
            info!("[LSP] transport handle exited");

            if !watchdog_flag.load(Ordering::Acquire) {
                if let Err(err) = outcome {
                    warn!("[LSP] main loop exited unexpectedly: {err}");
                } else {
                    warn!("[LSP] main loop exited unexpectedly");
                }
                let _ = watchdog_tx.try_send(LspResponse::Crashed);
            }
        });

        self._tasks.push(driver);
    }

    pub fn started(&self) -> bool {
        self.server.is_some()
    }

    pub fn send(&mut self, message: LspMessage) {
        let Some(server) = self.server.as_ref() else {
            #[cfg(debug_assertions)]
            debug!("[LSP] send() called before start(); dropping message");
            return;
        };

        match message {
            LspMessage::Initialize {
                root_uri,
                capabilities,
            } => {
                self.start_initialize(server.clone(), root_uri, capabilities);
            }
            LspMessage::Initialized => {}
            other @ (LspMessage::Shutdown { .. } | LspMessage::Exit) => {
                dispatch(server, &self.response_tx, &self.inbound_slots, other);
            }
            other if !self.init_done.load(Ordering::Acquire) => {
                self.pre_init_queue.push(other);
            }
            other => dispatch(server, &self.response_tx, &self.inbound_slots, other),
        }
    }

    fn start_initialize(
        &self,
        server: ServerSocket,
        root_uri: Url,
        capabilities: Box<ClientCapabilities>,
    ) {
        let tx = self.response_tx.clone();
        let init_done = self.init_done.clone();
        let pid = self.client_process_id.get().and_then(|p| *p);
        spawn_task(async move {
            #[allow(deprecated)]
            let params = InitializeParams {
                process_id: pid,
                root_uri: Some(root_uri),
                capabilities: *capabilities,
                client_info: Some(ClientInfo {
                    name: "bevy_lsp".into(),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                }),
                ..InitializeParams::default()
            };
            info!("[LSP] sending initialize request");
            match server.request::<InitializeRequest>(params).await {
                Ok(result) => {
                    info!("[LSP] initialize response received");
                    if let Err(err) = server.notify::<InitializedNotif>(InitializedParams {}) {
                        warn!("[LSP] initialized notify failed: {err}");
                    }
                    init_done.store(true, Ordering::Release);
                    emit(
                        &tx,
                        LspResponse::Initialized {
                            capabilities: Box::new(result.capabilities),
                        },
                    );
                }
                Err(err) => warn!("[LSP] {} failed: {err}", InitializeRequest::METHOD),
            }
        })
        .detach();
    }

    pub fn try_recv(&self) -> Option<LspResponse> {
        self.response_rx.try_recv().ok()
    }

    pub fn cleanup_timeouts(&self) {}

    pub fn is_ready(&self) -> bool {
        self.initialized
    }

    pub fn shutdown(&mut self) {
        if self.server.is_none() {
            return;
        }
        self.shutting_down.store(true, Ordering::Release);
        self.send(LspMessage::Shutdown { id: 0 });
        self.send(LspMessage::Exit);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }
}

pub(crate) type Tx = async_channel::Sender<LspResponse>;

pub(crate) fn spawn<R>(
    server: &ServerSocket,
    tx: &Tx,
    params: R::Params,
    map: impl FnOnce(R::Result, &Tx) + MaybeSend + 'static,
) where
    R: LspRequestTrait + 'static,
    R::Params: MaybeSend + 'static,
    R::Result: MaybeSend + 'static,
{
    let server = server.clone();
    let tx = tx.clone();
    spawn_task(async move {
        match server.request::<R>(params).await {
            Ok(result) => map(result, &tx),
            Err(err) => log_request_error::<R>(err),
        }
    })
    .detach();
}

fn log_request_error<R: LspRequestTrait>(err: async_lsp::Error) {
    use async_lsp::{Error, ErrorCode};
    if let Error::Response(ref resp) = err {
        if resp.code == ErrorCode::CONTENT_MODIFIED || resp.code == ErrorCode::REQUEST_CANCELLED {
            debug!("[LSP] {} cancelled by server: {err}", R::METHOD);
            return;
        }
    }
    warn!("[LSP] {} failed: {err}", R::METHOD);
}

pub(crate) fn fire<N>(server: &ServerSocket, params: N::Params)
where
    N: LspNotificationTrait + 'static,
    N::Params: Send + 'static,
{
    if let Err(err) = server.notify::<N>(params) {
        warn!("[LSP] {} failed: {err}", N::METHOD);
    }
}

#[inline]
pub(crate) fn emit(tx: &Tx, r: LspResponse) {
    let _ = tx.try_send(r);
}

pub(crate) fn text_pos(uri: Url, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    }
}

fn build_router(tx: Tx, next_id: Arc<AtomicU64>, slots: Arc<InboundReplySlots>) -> Router<()> {
    let mut router: Router<()> = Router::new(());

    let t = tx.clone();
    router.notification::<PublishDiagnostics>(move |_, params| {
        info!(
            "[LSP] publishDiagnostics uri={} version={:?} count={}",
            params.uri,
            params.version,
            params.diagnostics.len(),
        );
        for (i, d) in params.diagnostics.iter().enumerate() {
            info!(
                "[LSP]   raw diag[{}] line={} col={}..{} severity={:?} src={:?} code={:?} msg={:?}",
                i,
                d.range.start.line,
                d.range.start.character,
                d.range.end.character,
                d.severity,
                d.source,
                d.code,
                d.message,
            );
        }
        let _ = t.try_send(LspResponse::Diagnostics {
            uri: params.uri,
            version: params.version,
            diagnostics: params.diagnostics,
        });
        ControlFlow::Continue(())
    });

    let t = tx.clone();
    router.notification::<LogMessage>(move |_, params| {
        let _ = t.try_send(LspResponse::LogMessage {
            typ: params.typ,
            message: params.message,
        });
        ControlFlow::Continue(())
    });

    let t = tx.clone();
    router.notification::<ShowMessage>(move |_, params| {
        let _ = t.try_send(LspResponse::ShowMessage {
            typ: params.typ,
            message: params.message,
        });
        ControlFlow::Continue(())
    });

    let t = tx.clone();
    router.notification::<Progress>(move |_, params| {
        let _ = t.try_send(LspResponse::Progress {
            token: params.token,
            value: params.value,
        });
        ControlFlow::Continue(())
    });

    let t = tx.clone();
    router.notification::<TelemetryEvent>(move |_, value| {
        let data = match value {
            OneOf::Left(map) => serde_json::Value::Object(map),
            OneOf::Right(arr) => serde_json::Value::Array(arr),
        };
        let _ = t.try_send(LspResponse::Telemetry { data });
        ControlFlow::Continue(())
    });

    let t = tx.clone();
    router.notification::<LogTrace>(move |_, params| {
        let _ = t.try_send(LspResponse::LogTrace {
            message: params.message,
            verbose: params.verbose,
        });
        ControlFlow::Continue(())
    });

    inbound_request::<WorkspaceConfiguration, _>(
        &mut router,
        next_id.clone(),
        slots.configuration.clone(),
        tx.clone(),
        |request_id, params| LspResponse::ConfigurationRequested {
            request_id,
            items: params.items,
        },
    );

    inbound_request::<ApplyWorkspaceEdit, _>(
        &mut router,
        next_id.clone(),
        slots.apply_edit.clone(),
        tx.clone(),
        |request_id, params| LspResponse::ApplyEditRequested {
            request_id,
            label: params.label,
            edit: params.edit,
        },
    );

    inbound_request::<ShowMessageRequest, _>(
        &mut router,
        next_id.clone(),
        slots.show_message.clone(),
        tx.clone(),
        |request_id, params| LspResponse::ShowMessageRequestRequested {
            request_id,
            typ: params.typ,
            message: params.message,
            actions: params.actions,
        },
    );

    inbound_request::<ShowDocument, _>(
        &mut router,
        next_id.clone(),
        slots.show_document.clone(),
        tx.clone(),
        |request_id, params| LspResponse::ShowDocumentRequested {
            request_id,
            uri: params.uri,
            external: params.external,
            take_focus: params.take_focus,
            selection: params.selection,
        },
    );

    inbound_request::<WorkDoneProgressCreate, _>(
        &mut router,
        next_id.clone(),
        slots.work_done_progress_create.clone(),
        tx.clone(),
        |request_id, params| LspResponse::WorkDoneProgressCreateRequested {
            request_id,
            token: params.token,
        },
    );

    inbound_request::<RegisterCapability, _>(
        &mut router,
        next_id.clone(),
        slots.register_capability.clone(),
        tx.clone(),
        |request_id, params| LspResponse::RegisterCapabilityRequested {
            request_id,
            registrations: params.registrations,
        },
    );

    inbound_request::<UnregisterCapability, _>(
        &mut router,
        next_id.clone(),
        slots.unregister_capability.clone(),
        tx.clone(),
        |request_id, params| LspResponse::UnregisterCapabilityRequested {
            request_id,
            unregistrations: params.unregisterations,
        },
    );

    inbound_request::<WorkspaceFoldersRequest, _>(
        &mut router,
        next_id,
        slots.workspace_folders.clone(),
        tx.clone(),
        |request_id, _params| LspResponse::WorkspaceFoldersRequested { request_id },
    );

    let t = tx.clone();
    router.request::<SemanticTokensRefresh, _>(move |_, _params| {
        let _ = t.try_send(LspResponse::SemanticTokensRefreshRequested);
        async move { Ok(()) }
    });

    let t = tx.clone();
    router.request::<InlayHintRefreshRequest, _>(move |_, _params| {
        let _ = t.try_send(LspResponse::InlayHintRefreshRequested);
        async move { Ok(()) }
    });

    let t = tx.clone();
    router.request::<CodeLensRefresh, _>(move |_, _params| {
        let _ = t.try_send(LspResponse::CodeLensRefreshRequested);
        async move { Ok(()) }
    });

    let t = tx;
    router.request::<WorkspaceDiagnosticRefresh, _>(move |_, _params| {
        let _ = t.try_send(LspResponse::DiagnosticsRefreshRequested);
        async move { Ok(()) }
    });

    router
        .unhandled_notification(|_, _| ControlFlow::Continue(()))
        .unhandled_request(|_, _| async move {
            Err(ResponseError::new(
                async_lsp::ErrorCode::METHOD_NOT_FOUND,
                "request not handled by bevy_lsp",
            ))
        });

    router
}

fn inbound_request<R, F>(
    router: &mut Router<()>,
    next_id: Arc<AtomicU64>,
    slots: ReplySlots<R::Result>,
    tx: Tx,
    surface: F,
) where
    R: LspRequestTrait + 'static,
    R::Params: Send + 'static,
    R::Result: Send + 'static,
    F: Fn(u64, R::Params) -> LspResponse + Send + Sync + 'static,
{
    router.request::<R, _>(move |_, params| {
        let request_id = next_id.fetch_add(1, Ordering::Relaxed);
        let (resp_tx, resp_rx) = oneshot::channel();
        slots.lock().unwrap().insert(request_id, resp_tx);
        let _ = tx.try_send(surface(request_id, params));
        async move {
            match resp_rx.await {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(err)) => Err(err),
                Err(_) => Err(ResponseError::new(
                    async_lsp::ErrorCode::INTERNAL_ERROR,
                    "host dropped reply channel",
                )),
            }
        }
    });
}

pub(crate) fn fulfill_slot<T>(
    slots: &Mutex<HashMap<u64, oneshot::Sender<Result<T, ResponseError>>>>,
    id: u64,
    value: T,
) where
    T: 'static,
{
    if let Some(slot) = slots.lock().unwrap().remove(&id) {
        let _ = slot.send(Ok(value));
    } else {
        debug!("[LSP] respond for unknown inbound id {id}");
    }
}
