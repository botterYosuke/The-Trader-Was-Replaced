//! LSP integration example using bevscode's built-in `bevy_ui` popups.
//!
//! All popup rendering (completion, hover, signature help, code actions,
//! rename) plus inline decorations (inlay hints, document highlights)
//! ship inside `CodeEditorPlugins` under the `lsp` feature — no host UI
//! code required. The host just spawns the editor, attaches an LSP
//! transport, and sends the `Initialize` / `DidOpen` requests.
//!
//! Run: `cargo run --example editor_lsp --features lsp`. Requires
//! `rust-analyzer` on `PATH` (`rustup component add rust-analyzer`).

use bevscode::lsp_ui::{LspClient, LspDocument, LspMessage, LspRequest};
use bevscode::prelude::*;
use bevscode::prelude::{BufferAnchorParam, RopeBuffer};
use bevscode::types::{CodeEditor, CursorState};
use bevy::prelude::*;
use bevy_lsp::messages::{LspLogMessage, LspShowMessage};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LSP Integration Example".to_string(),
                    resolution: (1200, 800).into(),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::asset::AssetPlugin {
                file_path: "assets".into(),
                ..default()
            }),
    );

    app.add_plugins(CodeEditorPlugins);

    app.add_systems(Startup, (setup_camera, spawn_editor))
        .add_systems(PostStartup, setup_editor)
        .add_systems(
            Update,
            (
                display_lsp_info,
                auto_request_completion,
                log_lsp_server_messages,
            ),
        )
        .run();
}

/// Surface every `window/logMessage` and `window/showMessage` the server
/// sends. Debugging aid: if rust-analyzer can't load the workspace, you'll
/// see it complain here.
fn log_lsp_server_messages(
    mut logs: MessageReader<LspLogMessage>,
    mut shows: MessageReader<LspShowMessage>,
) {
    for ev in logs.read() {
        info!("[ra log] {:?}: {}", ev.typ, ev.message);
    }
    for ev in shows.read() {
        info!("[ra show] {:?}: {}", ev.typ, ev.message);
    }
}

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
    mut set_text_writer: MessageWriter<SetTextRequested>,
    mut lsp_w: MessageWriter<LspRequest>,
) {
    let Ok((editor_entity, mut lsp_client)) = editor_query.single_mut() else {
        return;
    };

    commands.entity(editor_entity).insert((
        TextFont::from_font_size(14.0).with_font(asset_server.load("fonts/FiraMono-Regular.ttf")),
        MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf")),
    ));

    let example_file_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editor_lsp.rs");
    let rust_code =
        std::fs::read_to_string(&example_file_path).expect("Failed to read example file");

    set_text_writer.write(SetTextRequested {
        entity: editor_entity,
        text: rust_code.clone(),
    });

    commands
        .entity(editor_entity)
        .insert(TreeSitterGrammar::new(
            bevy_tree_sitter::arborium::lang_rust::language().into(),
            bevy_tree_sitter::arborium::lang_rust::HIGHLIGHTS_QUERY,
        ));

    let file_uri_str = format!("file://{}", example_file_path.to_string_lossy());
    #[cfg(target_os = "windows")]
    let file_uri_str = format!(
        "file:///{}",
        example_file_path.to_string_lossy().replace('\\', "/")
    );

    let doc_uri = lsp_types::Url::parse(&file_uri_str).expect("Failed to parse URI");

    if let Err(e) = lsp_client.start("rust-analyzer", &[]) {
        error!("Failed to start rust-analyzer: {:?}", e);
        return;
    }

    let project_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root_uri =
        lsp_types::Url::from_directory_path(&project_root).expect("Failed to get project root URI");
    // Advertise Markdown as the preferred format for hover, completion
    // docs, and signature help so rust-analyzer sends fenced code +
    // formatted prose instead of stripped plain text. `bevy_markdown`
    // renders the markdown in the popup chrome.
    let markdown_then_plain = vec![
        lsp_types::MarkupKind::Markdown,
        lsp_types::MarkupKind::PlainText,
    ];
    let capabilities = lsp_types::ClientCapabilities {
        text_document: Some(lsp_types::TextDocumentClientCapabilities {
            hover: Some(lsp_types::HoverClientCapabilities {
                content_format: Some(markdown_then_plain.clone()),
                ..Default::default()
            }),
            completion: Some(lsp_types::CompletionClientCapabilities {
                completion_item: Some(lsp_types::CompletionItemCapability {
                    documentation_format: Some(markdown_then_plain.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            signature_help: Some(lsp_types::SignatureHelpClientCapabilities {
                signature_information: Some(lsp_types::SignatureInformationSettings {
                    documentation_format: Some(markdown_then_plain.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    lsp_w.write(LspRequest {
        entity: editor_entity,
        msg: LspMessage::Initialize {
            root_uri: root_uri.clone(),
            capabilities: Box::new(capabilities),
        },
    });

    lsp_w.write(LspRequest {
        entity: editor_entity,
        msg: LspMessage::Initialized,
    });

    lsp_w.write(LspRequest {
        entity: editor_entity,
        msg: LspMessage::DidOpen {
            uri: doc_uri.clone(),
            language_id: "rust".to_string(),
            version: 1,
            text: rust_code.to_string(),
        },
    });

    commands
        .entity(editor_entity)
        .insert(LspDocument::new(doc_uri, "rust"));

    info!("LSP started for file: {:?}", example_file_path);
}

fn display_lsp_info(query: Query<&LspClient, (With<CodeEditor>, Changed<LspClient>)>) {
    if !query.is_empty() {
        debug!("LSP client state changed");
    }
}

/// Auto-trigger completion requests after typing.
fn auto_request_completion(
    editor_query: Query<(&CursorState, Ref<TextBuffer<RopeBuffer>>), With<CodeEditor>>,
    mut writer: MessageWriter<bevscode::types::events::CompletionRequested>,
    _anchors: BufferAnchorParam<RopeBuffer>,
) {
    let Ok((cursor, buffer)) = editor_query.single() else {
        return;
    };

    if !buffer.is_changed() {
        return;
    }

    let cursor_pos = cursor.cursor_pos.min(buffer.rope().len_chars());
    if cursor_pos == 0 {
        return;
    }
    writer.write(bevscode::types::events::CompletionRequested::new(
        cursor_pos,
    ));
}
