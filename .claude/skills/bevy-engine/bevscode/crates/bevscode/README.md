# bevscode

[![crates.io](https://img.shields.io/crates/v/bevscode.svg)](https://crates.io/crates/bevscode)
[![docs.rs](https://docs.rs/bevscode/badge.svg)](https://docs.rs/bevscode)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/PoHsuanLai/bevscode/blob/main/LICENSE-MIT)
[![Bevy](https://img.shields.io/badge/Bevy-0.18-blue)](https://bevyengine.org)

Embeddable code editor for Bevy. Spawn `CodeEditor` into any app and it runs as a normal ECS entity.

![Demo](https://raw.githubusercontent.com/PoHsuanLai/bevscode/main/assets/demo.gif)

**Scope:** `bevscode` is a widget, not a standalone IDE. Window management, project trees, debugger UIs, and similar IDE-level concerns are left to the host application.

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

For a fixed-size pane, omit `AutoResizeViewport` and set an explicit `Node`:

```rust
commands.spawn((
    CodeEditor,
    Node { width: Val::Px(800.0), height: Val::Px(600.0), ..default() },
));
```

## Features

Multi-cursor, folding, bracket matching, line numbers, scrollbar, syntax highlighting (via `bevy_tree_sitter`), LSP UI (via `bevy_lsp`).

## Reading state

All state is plain ECS — query it from any system:

```rust
use bevy_instanced_text_editor::RopeBuffer;

fn status_bar(
    editors: Query<(&TextBuffer<RopeBuffer>, &CursorState, &FoldState), With<CodeEditor>>,
) {
    for (buffer, cursor, folds) in &editors { /* … */ }
}
```

## Handling editor actions

File I/O and IDE-action events are public Bevy messages. Wire your own handler with `EditorAppExt`:

```rust
use bevscode::prelude::*;

App::new()
    .add_plugins(CodeEditorPlugins)
    .add_editor_action_handler(my_save_handler)
    .run();

fn my_save_handler(mut events: MessageReader<SaveRequested>) {
    for ev in events.read() { /* persist buffer */ }
}
```

## Embedding in a larger app

Disable sub-plugins your host already provides:

```rust
CodeEditorPlugins.build().disable::<EditorUiPlugin>()
```

Override components at spawn. Settings cascade onto `CodeEditor` via `#[require]`, so any explicit component on the spawn replaces the default:

```rust
commands.spawn((
    CodeEditor,
    Indentation { use_spaces: true, tab_width: 2, auto_indent: true },
));
```

## Feature flags

- `tree-sitter` (default) — syntax highlighting
- `lsp` — language server integration
- `clipboard` (default) — system clipboard

## Bevy compatibility

| `bevscode` | Bevy |
|---|---|
| 0.1 | 0.18 |

## License

MIT OR Apache-2.0
