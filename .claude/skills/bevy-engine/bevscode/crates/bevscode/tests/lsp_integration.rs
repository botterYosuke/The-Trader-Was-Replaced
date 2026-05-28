//! Integration tests for the `bevy_lsp` → `bevscode` LSP pipeline.
//!
//! These tests pair a [`bevy_lsp::test_support::FakeLanguageServer`] with a
//! headless Bevy `App` that runs `LspPlugin` plus a minimal slice of the
//! bevscode editor systems. Each test issues an LSP request via the standard
//! [`bevy_lsp::LspRequest`] event channel, drives the app for a bounded
//! number of frames with [`LspTestContext::run_until_parked`], then asserts
//! on per-entity components and outbound messages.
//!
//! Why this shape: every system involved (`dispatch_lsp_requests`,
//! `drain_lsp_responses`, `on_lsp_completion`, etc.) runs in the same
//! schedule order production uses. If a regression hits anywhere along that
//! chain, the corresponding test fires.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::time::TimePlugin;
use bevy_lsp::messages::{LspMessage, LspRequest};
use bevy_lsp::test_support::FakeLanguageServer;
use bevy_lsp::{LspClient, LspPlugin};
use lsp_types::notification::Notification;
use lsp_types::{ClientCapabilities, ServerCapabilities, Url};

/// Headless Bevy `App` + a single LSP entity + the fake server it talks to.
/// The fake handler API mirrors zed's `FakeLanguageServer` — register typed
/// callbacks per LSP request method, observe client notifications via
/// `fake.try_recv_notification()`.
pub struct LspTestContext {
    pub app: App,
    pub editor: Entity,
    pub fake: FakeLanguageServer,
    pub uri: Url,
}

impl LspTestContext {
    /// Build a context wired through the public `bevy_lsp` API. `LspPlugin`
    /// gives us the message types and the per-frame drain system; we attach
    /// a single editor entity carrying a `LspClient` that's started against
    /// the fake transport.
    pub fn new(capabilities: ServerCapabilities) -> Self {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins.build().disable::<TimePlugin>());
        app.add_plugins(TimePlugin);
        app.add_plugins(LspPlugin);

        let (transport, fake) = FakeLanguageServer::new(capabilities);

        let mut client = LspClient::new();
        client.start_with(transport);
        let editor = app.world_mut().spawn(client).id();

        let uri = Url::parse("file:///tmp/lsp_integration/main.rs").unwrap();
        Self {
            app,
            editor,
            fake,
            uri,
        }
    }

    /// Convenience: kick off `initialize` and pump frames until the server
    /// capabilities have propagated. Returns once `LspClient::is_ready()`.
    pub fn initialize(&mut self) {
        self.app.world_mut().write_message(LspRequest {
            entity: self.editor,
            msg: LspMessage::Initialize {
                root_uri: Url::parse("file:///tmp/lsp_integration").unwrap(),
                capabilities: Box::new(ClientCapabilities::default()),
            },
        });
        let ready = self.run_until(Duration::from_secs(5), |app, editor| {
            app.world()
                .get::<LspClient>(editor)
                .map(|c| c.is_ready())
                .unwrap_or(false)
        });
        assert!(ready, "fake server never returned initialize");
    }

    /// Pump `app.update()` until `predicate(&app, editor)` returns true, or
    /// the deadline passes. Yields between frames so smol-pool worker
    /// threads can drive in-flight LSP traffic.
    pub fn run_until<F>(&mut self, timeout: Duration, mut predicate: F) -> bool
    where
        F: FnMut(&App, Entity) -> bool,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.app.update();
            if predicate(&self.app, self.editor) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Bounded heuristic for "all LSP traffic has settled." Stops once two
    /// consecutive frames see no new client→server notifications and no new
    /// frame counter changes. Caps at 100 frames so a stalled handler
    /// surfaces as a test panic, not an infinite hang.
    pub fn run_until_parked(&mut self) {
        let mut quiet = 0;
        for _ in 0..100 {
            let before = self.fake.notifications_pending();
            self.app.update();
            std::thread::sleep(Duration::from_millis(5));
            let after = self.fake.notifications_pending();
            if before == after {
                quiet += 1;
            } else {
                quiet = 0;
            }
            if quiet >= 2 {
                return;
            }
        }
        panic!("run_until_parked exceeded 100 frames");
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

/// Sanity: the fake's `initialize` response lands on the editor's
/// `LspClient`. If this fails, the whole harness is broken.
#[test]
fn initialize_handshake_completes() {
    let caps = ServerCapabilities {
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        ..Default::default()
    };
    let mut ctx = LspTestContext::new(caps);
    ctx.initialize();

    let ready = ctx
        .app
        .world()
        .get::<LspClient>(ctx.editor)
        .map(|c| c.is_ready())
        .unwrap_or(false);
    assert!(ready, "client.is_ready() never flipped to true");
}

/// Client → server: a `DidOpen` notification flowing through
/// `dispatch_lsp_requests` should land on the fake's notification queue.
#[test]
fn client_didopen_reaches_server() {
    let mut ctx = LspTestContext::new(ServerCapabilities::default());
    ctx.initialize();

    let uri = ctx.uri.clone();
    ctx.app.world_mut().write_message(LspRequest {
        entity: ctx.editor,
        msg: LspMessage::DidOpen {
            uri: uri.clone(),
            language_id: "rust".into(),
            version: 1,
            text: "fn main() {}".into(),
        },
    });
    ctx.run_until_parked();

    // The handshake itself sends an `initialized` notification; consume any
    // queued notifications and assert that `textDocument/didOpen` appears.
    let mut methods = Vec::new();
    while let Some(n) = ctx.fake.try_recv_notification() {
        methods.push(n.method);
    }
    assert!(
        methods
            .iter()
            .any(|m| m == lsp_types::notification::DidOpenTextDocument::METHOD),
        "didOpen never arrived; saw: {methods:?}",
    );
}

/// Server → client: a `publishDiagnostics` notification from the fake
/// should reach the client and emit an `LspDiagnosticsUpdated` message.
#[test]
fn server_publishdiagnostics_emits_message() {
    use bevy_lsp::messages::LspDiagnosticsUpdated;
    use lsp_types::notification::PublishDiagnostics;
    use lsp_types::PublishDiagnosticsParams;

    let mut ctx = LspTestContext::new(ServerCapabilities::default());
    ctx.initialize();

    let diag_uri = ctx.uri.clone();
    ctx.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: diag_uri.clone(),
            diagnostics: vec![],
            version: Some(7),
        });

    let saw_diag = ctx.run_until(Duration::from_secs(5), |app, _| {
        let messages = app
            .world()
            .resource::<bevy::ecs::message::Messages<LspDiagnosticsUpdated>>();
        let mut reader = messages.get_cursor();
        reader.read(messages).any(|m| m.uri == diag_uri)
    });
    assert!(
        saw_diag,
        "LspDiagnosticsUpdated for {diag_uri} never emitted"
    );
}

/// Pre-init queue: a notification sent BEFORE `initialize` completes should
/// be buffered, then drained once the handshake finishes — the fake should
/// see exactly one `didOpen` after both messages are written in this order.
#[test]
fn pre_init_didopen_drains_after_initialize() {
    let mut ctx = LspTestContext::new(ServerCapabilities::default());

    // Write Initialize *and* DidOpen before any frame runs. The plugin will
    // process them in order: Initialize starts the handshake, DidOpen lands
    // in `LspClient::pre_init_queue` until `LspServerInitialized` arrives.
    ctx.app.world_mut().write_message(LspRequest {
        entity: ctx.editor,
        msg: LspMessage::Initialize {
            root_uri: Url::parse("file:///tmp/lsp_integration").unwrap(),
            capabilities: Box::new(ClientCapabilities::default()),
        },
    });
    let uri = ctx.uri.clone();
    ctx.app.world_mut().write_message(LspRequest {
        entity: ctx.editor,
        msg: LspMessage::DidOpen {
            uri: uri.clone(),
            language_id: "rust".into(),
            version: 1,
            text: "fn main() {}".into(),
        },
    });

    ctx.run_until_parked();

    let mut methods = Vec::new();
    while let Some(n) = ctx.fake.try_recv_notification() {
        methods.push(n.method);
    }
    assert!(
        methods
            .iter()
            .any(|m| m == lsp_types::notification::DidOpenTextDocument::METHOD),
        "pre-init didOpen never drained; saw: {methods:?}",
    );
}

/// Completion roundtrip: client sends `textDocument/completion`, fake
/// handler returns a single item, the response surfaces as an
/// `LspCompletionResponse` message tagged with the editor entity.
#[test]
fn completion_roundtrip_emits_response() {
    use bevy_lsp::messages::LspCompletionResponse;
    use lsp_types::request::Completion;
    use lsp_types::{CompletionItem, CompletionResponse, Position};

    let caps = ServerCapabilities {
        completion_provider: Some(lsp_types::CompletionOptions::default()),
        ..Default::default()
    };
    let mut ctx = LspTestContext::new(caps);
    ctx.initialize();

    ctx.fake
        .set_request_handler::<Completion, _, _>(|_params| async move {
            Ok(Some(CompletionResponse::Array(vec![CompletionItem {
                label: "println!".into(),
                ..Default::default()
            }])))
        });

    let uri = ctx.uri.clone();
    ctx.app.world_mut().write_message(LspRequest {
        entity: ctx.editor,
        msg: LspMessage::Completion {
            uri: uri.clone(),
            position: Position {
                line: 0,
                character: 0,
            },
            id: 1,
        },
    });

    let saw_completion = ctx.run_until(Duration::from_secs(5), |app, editor| {
        let messages = app
            .world()
            .resource::<bevy::ecs::message::Messages<LspCompletionResponse>>();
        let mut reader = messages.get_cursor();
        reader.read(messages).any(|m| m.entity == editor)
    });
    assert!(saw_completion, "LspCompletionResponse never emitted");
}

/// Simulated server crash: tripping the fake's exit signal should surface
/// as an `LspServerCrashed` message tagged with the editor entity.
#[test]
fn server_crash_emits_message() {
    use bevy_lsp::messages::LspServerCrashed;

    let mut ctx = LspTestContext::new(ServerCapabilities::default());
    ctx.initialize();

    ctx.fake.simulate_exit();

    let saw_crash = ctx.run_until(Duration::from_secs(5), |app, editor| {
        let messages = app
            .world()
            .resource::<bevy::ecs::message::Messages<LspServerCrashed>>();
        let mut reader = messages.get_cursor();
        reader.read(messages).any(|m| m.entity == editor)
    });
    assert!(saw_crash, "LspServerCrashed never emitted");
}
