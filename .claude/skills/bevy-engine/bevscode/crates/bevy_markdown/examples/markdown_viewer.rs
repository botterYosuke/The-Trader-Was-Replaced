//! Renders a sample markdown document covering every v1 block type.
//!
//! Uses Bevy's bundled default font (`default_font` feature) for both
//! body and mono so the example runs without any asset files.
//!
//! Pass an output path to write a PNG and exit:
//!   `cargo run -p bevy_markdown --example markdown_viewer -- out.png`

use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy_markdown::prelude::*;
use bevy_markdown::tree_sitter::TreeSitterHighlighter;

#[derive(Resource)]
struct CaptureConfig {
    out_path: Option<String>,
    capture_at_frame: u64,
    exit_at_frame: u64,
}

#[derive(Resource, Default)]
struct FrameCounter(u64);

const SAMPLE: &str = r##"# bevy_markdown

A CommonMark renderer for **bevy_ui** and *bevy_text*.

## What it covers

Paragraphs flow normally with `inline code`, [links](https://bevyengine.org),
**bold**, *italic*, and ***both at once***.

### Lists

- first item
- second item with **emphasis**
- nested:
  - inner a
  - inner b

1. ordered one
2. ordered two

### Code blocks

```rust
fn render(commands: &mut Commands) {
    commands.spawn(Markdown { source: "# hi".into() });
}
```

### Blockquote

> Markdown blocks become bevy_ui flex children. One paragraph per
> `Text` entity, one styled run per `TextSpan` child.

---

End of sample.
"##;

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(MarkdownHighlighter(Arc::new(
        TreeSitterHighlighter::with_default_colors(),
    )));
    commands.spawn(Camera2d);
    commands.spawn((
        Markdown {
            source: SAMPLE.into(),
        },
        MarkdownFonts {
            body: assets.load("fonts/Inter-Regular.ttf"),
            mono: assets.load("fonts/FiraMono-Regular.ttf"),
            bold: Some(assets.load("fonts/Inter-Bold.ttf")),
            italic: Some(assets.load("fonts/Inter-Italic.ttf")),
            bold_italic: Some(assets.load("fonts/Inter-BoldItalic.ttf")),
        },
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(24.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.08, 0.09, 0.11)),
    ));
}

fn main() {
    let out_path = std::env::args().nth(1);
    let mut app = App::new();
    app.insert_resource(CaptureConfig {
        out_path,
        capture_at_frame: 180,
        exit_at_frame: 300,
    });
    app.init_resource::<FrameCounter>();
    let assets_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .to_string_lossy()
        .into_owned();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "markdown_viewer".to_string(),
                    resolution: (900u32, 700u32).into(),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets_path,
                ..default()
            }),
    )
    .add_plugins(BevyMarkdownPlugin)
    .add_systems(Startup, setup)
    .add_systems(Update, (tick_frames, capture_then_exit));
    app.run();
}

fn tick_frames(mut counter: ResMut<FrameCounter>) {
    counter.0 += 1;
}

fn capture_then_exit(
    mut commands: Commands,
    counter: Res<FrameCounter>,
    config: Res<CaptureConfig>,
    mut exit: MessageWriter<AppExit>,
    mut captured: Local<bool>,
) {
    let Some(out_path) = config.out_path.as_ref() else {
        return;
    };
    if !*captured && counter.0 == config.capture_at_frame {
        if let Some(parent) = std::path::Path::new(out_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        println!("[screenshot] capturing frame {} -> {}", counter.0, out_path);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(out_path.clone()));
        *captured = true;
    }
    if counter.0 >= config.exit_at_frame {
        exit.write(AppExit::Success);
    }
}
