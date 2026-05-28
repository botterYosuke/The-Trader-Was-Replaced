# bevy_tree_sitter

[![crates.io](https://img.shields.io/crates/v/bevy_tree_sitter.svg)](https://crates.io/crates/bevy_tree_sitter)
[![docs.rs](https://docs.rs/bevy_tree_sitter/badge.svg)](https://docs.rs/bevy_tree_sitter)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/PoHsuanLai/bevscode/blob/main/LICENSE-MIT)
[![Bevy](https://img.shields.io/badge/Bevy-0.18-blue)](https://bevyengine.org)

Component-driven tree-sitter integration for Bevy. Returns capture names, not colors.

What this crate is for: code editors mapping captures to a theme, code-outline panels, AI agents reasoning about syntactic structure, structural search tools, log viewers highlighting stack traces — anything that wants tree-sitter parsing without dragging in a renderer.

What this crate is **not** for: deciding what color a `keyword` should be (that's a theme decision, lives in the consumer), shaping or rendering glyphs, owning a buffer.

## Architecture

The integration is per-entity Components. Attach `Language` + `ParseSourceComp` + `SyntaxTree` to your entity; the `parse_dirty` system drives async parsing in the background; you observe results by filtering on `Changed<SyntaxTree>`.

| Component | What it carries |
|---|---|
| **`Language`** | Grammar + highlight query. Cheap to clone (tree-sitter language is internally `Arc`-like). |
| **`ParseSourceComp(Arc<dyn ParseSource>)`** | Consumer-supplied buffer adapter. The trait gives `content_version() -> u64`, `snapshot() -> Rope`, and an optional `apply_edit(InputEdit)` for tree interpolation. |
| **`SyntaxTree`** | `Option<ts::Tree>` + `content_version` + `tree_version`. Written by `parse_dirty`. Consumers read it via `Query<&SyntaxTree>` and use `Changed<SyntaxTree>` for invalidation. Not `Reflect` (`tree_sitter::Tree` doesn't impl it). |

Async parsing runs on `AsyncComputeTaskPool`. `parse_dirty` (in `ParseSet`) detects per-entity drift between `ParseSource::content_version()` and `SyntaxTree::content_version`, kicks off a child-entity `ParseTask`, and on completion writes the new tree back into the parent's `SyntaxTree`. Single-flight per entity — multiple editors can parse concurrently.

## Quick start

```rust
use std::sync::{Arc, RwLock};
use bevy::prelude::*;
use bevy_tree_sitter::prelude::*;
use ropey::Rope;

struct MyBuffer { rope: Rope, version: u64 }

struct MyParseSource(Arc<RwLock<MyBuffer>>);
impl ParseSource for MyParseSource {
    fn content_version(&self) -> u64 { self.0.read().unwrap().version }
    fn snapshot(&self) -> Rope { self.0.read().unwrap().rope.clone() }
}

fn setup(mut commands: Commands) {
    let buf = Arc::new(RwLock::new(MyBuffer {
        rope: Rope::from_str("fn main() {}"),
        version: 1,
    }));
    commands.spawn((
        Language::rust(),
        SyntaxTree::default(),
        ParseSourceComp::new(MyParseSource(buf)),
    ));
}

fn react(q: Query<&SyntaxTree, Changed<SyntaxTree>>) {
    for tree in &q {
        if let Some(t) = tree.tree() {
            // Walk the tree, query highlights, invalidate caches.
        }
    }
}

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(TreeSitterPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, react)
        .run();
}
```

## Capture names, not colors

`HighlightRange { byte_range, capture_name: Arc<str> }` — the capture name is the raw tree-sitter query capture (e.g. `"keyword"`, `"function.method"`, `"string"`). Mapping that to a `Color` is the consumer's job.

`Arc<str>` so emitting one highlight per token doesn't allocate — the same `"keyword"` string is shared across thousands of ranges in a typical file.

The editor's `bevscode::syntax_highlighting` module is a worked example: it holds a `Theme` HashMap from capture name to `Color` and converts `HighlightRange` runs into engine `StyleRun`s on the fly.

## Querying highlights from a tree

`SyntaxTree` carries the parsed tree; running a highlight query is the consumer's responsibility (since the consumer owns the `TreeSitterProvider` that holds the compiled query). Pattern in the editor:

```rust
fn run_highlights(
    q: Query<(&SyntaxTree, &TreeSitterProvider)>,
) {
    for (tree, provider) in &q {
        if tree.tree().is_some() {
            let ranges: Vec<Vec<HighlightRange>> =
                provider.highlight_range(/* text, lines, start_byte */);
            // map ranges -> StyleRuns
        }
    }
}
```

## Incremental parsing

`ParseSource::apply_edit(InputEdit)` is the tree-interpolation hook. Implement it to forward the edit into your `TreeSitterProvider::apply_sync_edit` so the cached tree stays valid for highlight queries while the async re-parse runs:

```rust
impl ParseSource for MyParseSource {
    fn apply_edit(&self, edit: bevy_tree_sitter::ts::InputEdit) {
        if let Some(provider) = self.0.read().unwrap().provider.as_mut() {
            provider.apply_sync_edit(edit, &self.snapshot());
        }
    }
    // content_version, snapshot as above
}
```

`byte_to_point` (re-exported at the crate root) computes a `ts::Point` from a rope + byte offset for building `InputEdit`s.

## Languages

The `bevy_tree_sitter::languages` convenience module provides built-in `Language` constructors for Rust, Python, JavaScript, TypeScript, etc.

```rust
let lang = Language::rust();
commands.spawn((lang, SyntaxTree::default(), ParseSourceComp::new(...)));
```

For custom languages, `Language::from_grammar(name, ts_language, highlights_query)` builds one from your own grammar.

## What's not here

- **Themes.** The crate emits capture names; theming is the consumer's job. `bevscode` ships a default theme; AI / outline consumers can ignore colors entirely.
- **Buffer storage.** Consumers expose their buffer via `ParseSource`. The crate doesn't own a `Rope` or `String` itself.
- **Bevy reflection on `Tree` / `Parser` / `Task`.** Tree-sitter's C-binding types don't implement `Reflect`. The capture-name `Arc<str>` strings are reachable via the consumer's own reflected types.

## Re-export

The crate re-exports the underlying `tree-sitter` crate as `bevy_tree_sitter::ts` so consumers can name `ts::Tree`, `ts::InputEdit`, etc. without taking a direct dep on the C-binding crate.

## Bevy compatibility

| `bevy_tree_sitter` | Bevy |
|---|---|
| 0.1 | 0.18 |

## License

MIT OR Apache-2.0
