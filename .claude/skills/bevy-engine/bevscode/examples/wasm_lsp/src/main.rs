//! Browser-hosted bevscode editor that talks to rust-analyzer through a
//! WebSocket-to-stdio bridge. Build and serve with `trunk serve` from this
//! directory; see `README.md` for the full setup including the bridge
//! script that proxies the WebSocket to the language server.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("This example targets wasm32-unknown-unknown — build with `trunk serve`.");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use bevscode::prelude::*;
    use bevy::prelude::*;
    use bevy_lsp::{LspClient, LspDocument, LspMessage, LspRequest, WebSocketTransport};

    console_error_panic_hook::set_once();

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "bevscode — wasm + LSP".to_string(),
                        canvas: Some("#bevy".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: "assets".into(),
                    ..default()
                }),
        )
        .add_plugins(CodeEditorPlugins)
        .add_systems(Startup, (setup_camera, spawn_editor))
        .add_systems(PostStartup, setup_editor)
        .run();

    fn spawn_editor(mut commands: Commands) {
        commands.spawn((CodeEditor, AutoResizeViewport, Name::new("CodeEditor")));
    }

    fn setup_camera(mut commands: Commands) {
        commands.spawn((
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(EditorTheme::default().background),
                ..default()
            },
        ));
    }

    fn setup_editor(
        mut commands: Commands,
        mut editor_query: Query<(Entity, &mut LspClient), With<CodeEditor>>,
        asset_server: Res<AssetServer>,
        mut input_focus: ResMut<bevy::input_focus::InputFocus>,
        mut set_text_writer: MessageWriter<SetTextRequested>,
        mut lsp_w: MessageWriter<LspRequest>,
    ) {
        let Ok((entity, mut lsp_client)) = editor_query.single_mut() else {
            return;
        };

        commands.entity(entity).insert((
            TextFont::from_font_size(14.0)
                .with_font(asset_server.load("fonts/FiraMono-Regular.ttf")),
            MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf")),
        ));
        input_focus.set(entity);

        let initial_text = include_str!("sample.rs").to_string();
        set_text_writer.write(SetTextRequested {
            entity,
            text: initial_text.clone(),
        });

        commands.entity(entity).insert(TreeSitterGrammar::new(
            bevy_tree_sitter::arborium::lang_rust::language().into(),
            bevy_tree_sitter::arborium::lang_rust::HIGHLIGHTS_QUERY,
        ));

        // Trunk's `[ws_protocol]` proxy hands /lsp to the local bridge, so a
        // host-relative URL is enough — no hardcoded port.
        let ws_url = browser_ws_url("/lsp");
        bevy::log::info!("opening LSP WebSocket: {ws_url}");
        lsp_client.start_with(WebSocketTransport::new(ws_url));

        let doc_uri = lsp_types::Url::parse("file:///workspace/main.rs")
            .expect("Failed to parse workspace URI");
        let root_uri = lsp_types::Url::parse("file:///workspace/")
            .expect("Failed to parse workspace root URI");

        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::Initialize {
                root_uri,
                capabilities: Box::new(lsp_types::ClientCapabilities::default()),
            },
        });
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::Initialized,
        });
        lsp_w.write(LspRequest {
            entity,
            msg: LspMessage::DidOpen {
                uri: doc_uri.clone(),
                language_id: "rust".to_string(),
                version: 1,
                text: initial_text,
            },
        });

        commands
            .entity(entity)
            .insert(LspDocument::new(doc_uri, "rust"));
    }

    fn browser_ws_url(path: &str) -> String {
        let window = web_sys::window().expect("no window in this context");
        let location = window.location();
        let proto = match location.protocol().as_deref() {
            Ok("https:") => "wss",
            _ => "ws",
        };
        let host = location.host().unwrap_or_else(|_| "localhost:8765".into());
        format!("{proto}://{host}{path}")
    }
}
