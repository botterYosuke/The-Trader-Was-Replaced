//! Tree-sitter syntax highlighting example
//!
//! Demonstrates how to use tree-sitter for syntax highlighting in the code editor.
//! This example highlights Rust code using the tree-sitter-rust grammar.

use bevscode::prelude::*;
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Tree-sitter Syntax Highlighting Example".to_string(),
                        resolution: (1200, 800).into(),
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
        .add_systems(PostStartup, setup_editor_with_treesitter)
        .run();
}

fn spawn_editor(mut commands: Commands) {
    use bevscode::plugin::{
        DecorationKind, GlyphKind, GlyphMarker, GlyphMarkers, GutterDecorations, LineDecoration,
    };
    use bevscode::settings::{
        CursorLine, RenderLineHighlight, RenderSettings, RenderWhitespace, RulerOption,
        WordWrapMode, Wrapping,
    };

    commands.spawn((
        CodeEditor,
        AutoResizeViewport,
        Rulers(vec![
            RulerOption {
                column: 80,
                color: None,
            },
            RulerOption {
                column: 120,
                color: None,
            },
        ]),
        CursorLine {
            render_line_highlight: RenderLineHighlight::All,
            ..Default::default()
        },
        RenderSettings {
            render_whitespace: RenderWhitespace::All,
            ..Default::default()
        },
        Wrapping {
            word_wrap: WordWrapMode::On,
            ..Default::default()
        },
        GlyphMarkers(vec![
            GlyphMarker {
                line: 5,
                kind: GlyphKind::Breakpoint,
                color: Color::srgba(0.93, 0.36, 0.39, 1.0),
            },
            GlyphMarker {
                line: 12,
                kind: GlyphKind::DebugCurrent,
                color: Color::srgba(0.40, 0.83, 0.40, 1.0),
            },
            GlyphMarker {
                line: 18,
                kind: GlyphKind::DiagnosticWarning,
                color: Color::srgba(0.97, 0.69, 0.0, 1.0),
            },
        ]),
        GutterDecorations(vec![
            LineDecoration {
                line: 3,
                kind: DecorationKind::Added,
                color: Color::srgb(0.32, 0.71, 0.45),
            },
            LineDecoration {
                line: 4,
                kind: DecorationKind::Modified,
                color: Color::srgb(0.31, 0.55, 0.85),
            },
            LineDecoration {
                line: 7,
                kind: DecorationKind::Deleted,
                color: Color::srgb(0.86, 0.31, 0.31),
            },
        ]),
        Name::new("CodeEditor"),
    ));
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

fn setup_editor_with_treesitter(
    mut commands: Commands,
    editor_query: Query<Entity, With<CodeEditor>>,
    asset_server: Res<AssetServer>,
    mut set_text_writer: MessageWriter<SetTextRequested>,
) {
    let Ok(entity) = editor_query.single() else {
        return;
    };

    commands.entity(entity).insert((
        TextFont::from_font_size(14.0).with_font(asset_server.load("fonts/FiraMono-Regular.ttf")),
        MonoFontFaces::default().with_bold(asset_server.load("fonts/FiraMono-Medium.ttf")),
    ));

    let text = r#"// Rust syntax highlighting with tree-sitter
// See docs at https://bevyengine.org/learn/ for more.
// Long line to demonstrate soft-wrap: lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.
use std::collections::HashMap;

/// A simple struct to demonstrate syntax highlighting
#[derive(Debug, Clone)]
pub struct Person {
    pub name: String,
    pub age: u32,
    tags: Vec<String>,
}

impl Person {
    /// Create a new person
    pub fn new(name: String, age: u32) -> Self {
        Self {
            name,
            age,
            tags: Vec::new(),
        }
    }

    /// Add a tag to the person
    pub fn add_tag(&mut self, tag: impl Into<String>) {
        self.tags.push(tag.into());
    }

    /// Check if person is an adult
    pub fn is_adult(&self) -> bool {
        self.age >= 18
    }
}

fn main() {
    let mut person = Person::new("Alice".to_string(), 25);
    person.add_tag("developer");
    person.add_tag("rust-enthusiast");

    println!("Person: {:?}", person);
    println!("Is adult: {}", person.is_adult());

    // HashMap example
    let mut scores = HashMap::new();
    scores.insert("Alice", 100);
    scores.insert("Bob", 85);

    for (name, score) in &scores {
        println!("{}: {}", name, score);
    }

    // Pattern matching
    match person.age {
        0..=17 => println!("Minor"),
        18..=65 => println!("Adult"),
        _ => println!("Senior"),
    }

    // Closure example
    let numbers = vec![1, 2, 3, 4, 5];
    let doubled: Vec<_> = numbers.iter().map(|x| x * 2).collect();
    println!("Doubled: {:?}", doubled);
}
"#;

    set_text_writer.write(SetTextRequested {
        entity,
        text: text.to_string(),
    });

    commands.entity(entity).insert(TreeSitterGrammar::new(
        bevy_tree_sitter::arborium::lang_rust::language().into(),
        bevy_tree_sitter::arborium::lang_rust::HIGHLIGHTS_QUERY,
    ));
}
