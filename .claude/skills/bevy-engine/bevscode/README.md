# bevscode

[![CI](https://github.com/PoHsuanLai/bevscode/actions/workflows/ci.yml/badge.svg)](https://github.com/PoHsuanLai/bevscode/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

Embeddable text editing and rendering plugins for Bevy. Drop them into any app and they coexist with your existing ECS world.

![Demo](https://raw.githubusercontent.com/PoHsuanLai/bevscode/main/assets/demo.gif)

**Scope:** this is a component library, not a standalone IDE. It gives you a capable code-editing widget you can embed inside a Bevy application — window management, project trees, debugger UIs, and similar IDE-level concerns are outside its scope.

| Crate | What it does | |
|---|---|---|
| **[`bevy_instanced_text`](https://github.com/PoHsuanLai/bevy_instanced_text)** *(separate repo)* | GPU-instanced text rendering: glyph atlas, soft-wrap layout, overlays. | [![crates.io](https://img.shields.io/crates/v/bevy_instanced_text.svg)](https://crates.io/crates/bevy_instanced_text) [![docs.rs](https://docs.rs/bevy_instanced_text/badge.svg)](https://docs.rs/bevy_instanced_text) |
| **[`bevy_instanced_text_interaction`](https://github.com/PoHsuanLai/bevy_instanced_text)** *(separate repo)* | Shared UI primitives: clipboard, selection, caret rendering, picking observers. No ropey dep. | [![crates.io](https://img.shields.io/crates/v/bevy_instanced_text_interaction.svg)](https://crates.io/crates/bevy_instanced_text_interaction) [![docs.rs](https://docs.rs/bevy_instanced_text_interaction/badge.svg)](https://docs.rs/bevy_instanced_text_interaction) |
| **[`bevy_instanced_text_editor`](crates/bevy_instanced_text_editor)** | Rope-backed editable text: edit history, undo/redo, typed-char insertion, anchors. | [![crates.io](https://img.shields.io/crates/v/bevy_instanced_text_editor.svg)](https://crates.io/crates/bevy_instanced_text_editor) [![docs.rs](https://docs.rs/bevy_instanced_text_editor/badge.svg)](https://docs.rs/bevy_instanced_text_editor) |
| **[`bevy_tree_sitter`](crates/bevy_tree_sitter)** | Tree-sitter incremental syntax highlighting. | [![crates.io](https://img.shields.io/crates/v/bevy_tree_sitter.svg)](https://crates.io/crates/bevy_tree_sitter) [![docs.rs](https://docs.rs/bevy_tree_sitter/badge.svg)](https://docs.rs/bevy_tree_sitter) |
| **[`bevy_lsp`](crates/bevy_lsp)** | Async LSP transport. Responses arrive as Bevy messages. | [![crates.io](https://img.shields.io/crates/v/bevy_lsp.svg)](https://crates.io/crates/bevy_lsp) [![docs.rs](https://docs.rs/bevy_lsp/badge.svg)](https://docs.rs/bevy_lsp) |
| **[`bevscode`](crates/bevscode)** | Code editor: multi-cursor, folding, brackets, line numbers, LSP UI. | [![crates.io](https://img.shields.io/crates/v/bevscode.svg)](https://crates.io/crates/bevscode) [![docs.rs](https://docs.rs/bevscode/badge.svg)](https://docs.rs/bevscode) |
| **[`bevsterm`](crates/bevsterm)** | PTY-backed terminal widget. | *(not published — wezterm deps not on crates.io)* |

## Bevy compatibility

| bevscode | Bevy |
|---|---|
| 0.1 | 0.18 |

## Quick start

```rust
use bevy::prelude::*;
use bevscode::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(CodeEditorPlugins)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
            // AutoResizeViewport keeps the editor's Node sized to the window.
            commands.spawn((CodeEditor, AutoResizeViewport));
        })
        .run();
}
```

For a fixed-size pane (e.g. split layout), omit `AutoResizeViewport` and set an explicit `Node`:

```rust
commands.spawn((
    CodeEditor,
    Node { width: Val::Px(800.0), height: Val::Px(600.0), ..default() },
));
```

## Composition

Pick the plugin set that matches your use case:

```rust
// Just GPU text rendering (labels, HUDs)
.add_plugins(InstancedTextPlugins)

// Rendering + selection / clipboard / scroll-wheel
.add_plugins((
    InstancedTextPlugins,
    InstancedTextInteractionPlugin::<TextSpan>::default(),
))

// Full code editor (cursor, edits, syntax, folding, LSP UI)
.add_plugins(CodeEditorPlugins)
```

State is plain ECS — query it from any system:

```rust
use bevy_instanced_text_editor::RopeBuffer;

fn status_bar(
    editors: Query<(&TextBuffer<RopeBuffer>, &CursorState), With<CodeEditor>>,
) {
    for (buffer, cursor) in &editors { /* … */ }
}
```

To handle a built-in editor action (save, open, completion request, …), add a handler system with `EditorAppExt`:

```rust
use bevscode::prelude::*;

App::new()
    .add_plugins(CodeEditorPlugins)
    .add_editor_action_handler(my_save_handler)
    .run();

fn my_save_handler(mut events: MessageReader<SaveRequested>) {
    for ev in events.read() { /* … */ }
}
```

## License

MIT OR Apache-2.0
