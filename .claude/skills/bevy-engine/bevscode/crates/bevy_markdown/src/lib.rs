//! CommonMark renderer for `bevy_ui` + `bevy_text`.
//!
//! Spawn a [`Markdown`] component on a UI [`Node`]; the plugin rebuilds
//! children whenever the source string or any theme component changes.
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_markdown::prelude::*;
//!
//! fn setup(mut commands: Commands, assets: Res<AssetServer>) {
//!     commands.spawn(Camera2d);
//!     commands.spawn((
//!         Markdown { source: "# Hello\n\nSome **bold** text.".into() },
//!         MarkdownFonts {
//!             body: assets.load("fonts/Inter.ttf"),
//!             mono: assets.load("fonts/FiraMono.ttf"),
//!             ..default()
//!         },
//!         Node {
//!             flex_direction: FlexDirection::Column,
//!             padding: UiRect::all(Val::Px(16.0)),
//!             ..default()
//!         },
//!     ));
//! }
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(BevyMarkdownPlugin)
//!     .add_systems(Startup, setup)
//!     .run();
//! ```

pub mod highlight;
pub mod parse;
pub mod spawn;
pub mod theme;
pub mod tree_sitter;

use bevy::prelude::*;

pub use highlight::{CodeHighlighter, MarkdownHighlighter, NoHighlight};
pub use parse::{parse, Block, Inline, InlineStyle};
pub use spawn::{spawn_markdown, MarkdownLink};
pub use theme::{MarkdownColors, MarkdownFonts, MarkdownScales, MarkdownSpacing};

/// Source string to render. Children are rebuilt whenever this
/// component or any required theme component changes.
#[derive(Component, Clone, Debug)]
#[require(Node, MarkdownFonts, MarkdownColors, MarkdownSpacing, MarkdownScales)]
pub struct Markdown {
    pub source: String,
}

pub struct BevyMarkdownPlugin;

impl Plugin for BevyMarkdownPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, rebuild_markdown);
    }
}

#[allow(clippy::type_complexity)]
fn rebuild_markdown(
    mut commands: Commands,
    targets: Query<
        (
            Entity,
            &Markdown,
            &MarkdownFonts,
            &MarkdownColors,
            &MarkdownSpacing,
            &MarkdownScales,
        ),
        Or<(
            Changed<Markdown>,
            Changed<MarkdownFonts>,
            Changed<MarkdownColors>,
            Changed<MarkdownSpacing>,
            Changed<MarkdownScales>,
        )>,
    >,
    highlighter: Option<Res<MarkdownHighlighter>>,
) {
    for (entity, md, fonts, colors, spacing, scales) in &targets {
        // Whole rebuild runs inside `queue_silenced` so if the host
        // despawns the markdown root between change-detection and
        // command flush, the closure is skipped atomically — no
        // orphan children get spawned at viewport (0,0).
        if commands.get_entity(entity).is_err() {
            continue;
        }
        let highlighter = highlighter
            .as_ref()
            .map(|h| MarkdownHighlighter(h.0.clone()));
        let md_source = md.source.clone();
        let fonts = fonts.clone();
        let colors = colors.clone();
        let spacing = spacing.clone();
        let scales = scales.clone();
        commands
            .entity(entity)
            .queue_silenced(move |mut e: bevy::ecs::world::EntityWorldMut| {
                e.despawn_related::<bevy::prelude::Children>();
                let parent_id = e.id();
                // Need `Commands` for `ChildSpawnerCommands`; flush
                // keeps despawn-then-spawn atomic within the closure.
                e.world_scope(|world| {
                    let mut cmds = world.commands();
                    cmds.entity(parent_id).with_children(|p| {
                        spawn::spawn_markdown(
                            p,
                            &md_source,
                            &fonts,
                            &colors,
                            &spacing,
                            &scales,
                            highlighter.as_ref(),
                        );
                    });
                    world.flush();
                });
            });
    }
}

pub mod prelude {
    pub use crate::{
        BevyMarkdownPlugin, CodeHighlighter, Markdown, MarkdownColors, MarkdownFonts,
        MarkdownHighlighter, MarkdownLink, MarkdownScales, MarkdownSpacing, NoHighlight,
    };
}
