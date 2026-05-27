#![allow(clippy::type_complexity)]

//! Embeddable code editor plugin for Bevy.
//!
//! Designed to be dropped into any Bevy application as a self-contained
//! widget. The plugin handles the mechanics — input dispatch, cursor movement,
//! edit history, syntax highlighting, code folding, bracket matching — and
//! exposes everything as readable ECS components so the host app can react to
//! or extend editor state without forking the plugin.
//!
//! ## Spawning an editor
//!
//! Spawn [`crate::types::editor::CodeEditor`] with any overrides; the
//! `#[require]` cascade fills in sensible defaults for everything else:
//!
//! ```rust,no_run
//! # use bevy::prelude::*;
//! # use bevscode::prelude::*;
//! # fn setup(mut commands: Commands) {
//! // Minimal — auto-sizes to window, default theme and keybindings.
//! commands.spawn(CodeEditor);
//!
//! // With overrides.
//! commands.spawn((
//!     CodeEditor,
//!     TextFont::from_font_size(18.0),
//!     EditorTheme { background: bevy::color::palettes::css::DARK_SLATE_GRAY.into(), ..default() },
//! ));
//! # }
//! ```
//!
//! All settings are per-entity components (see [`settings`]): two editors in
//! the same app can have different fonts, themes, indentation rules, or LSP
//! configurations.
//!
//! ## State the host can read
//!
//! The plugin writes these components every frame — hosts can `Query` them
//! freely without any coupling to the plugin internals:
//!
//! - [`crate::types::fold::FoldState`] — which line ranges are currently
//!   folded. Drive fold/unfold via the [`crate::types::events`] messages
//!   or listen to [`crate::types::events::FoldStateChanged`] for transitions.
//! - [`crate::plugin::syntax_highlighting::EditorSyntaxState`] — the current
//!   highlight ranges, keyed by capture name. Useful for outline panels, AI
//!   context extraction, or custom overlays.
//! - [`crate::types::editor::BracketMatchState`] — the matched bracket pair
//!   under the cursor, if any.
//! - [`bevy_instanced_text_editor::CursorState`], [`bevy_instanced_text_editor::SelectionState`],
//!   [`bevy_instanced_text_editor::EditHistoryState`] — cursor position, selections, and
//!   undo stack from the editing layer.
//! - [`bevy_instanced_text::DisplayLayout`] — the shaped-line snapshot used for
//!   rendering; also useful for hit-testing and overlay placement.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevscode::prelude::*;
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(CodeEditorPlugins)
//!     .add_systems(Startup, setup)
//!     .run();
//!
//! fn setup(mut commands: Commands) {
//!     // One Camera2d is all that's needed for a single full-window editor.
//!     commands.spawn(Camera2d);
//!     // CodeEditor auto-sizes to the window via AutoResizeViewport.
//!     commands.spawn(CodeEditor);
//! }
//! ```
//!
//! ## Camera and layout
//!
//! `CodeEditor` is a standard Bevy UI `Node`. Size and position it the same
//! way you would any other UI element — `Node::width`, `Node::height`, flex
//! layout, etc. The editor reads its dimensions from `ComputedNode` each frame;
//! no manual viewport management is needed.
//!
//! **Single window (default):** spawn `CodeEditor` alone. The
//! `AutoResizeViewport` component (added automatically) keeps the node
//! pixel-perfect with the primary window. One `Camera2d` at the default origin
//! is all the rendering side needs.
//!
//! **Split panes:** omit `AutoResizeViewport` and set explicit `Node` sizes.
//! Give each editor entity a `RenderLayers` component and spawn a matching
//! `Camera2d` with a `Camera::viewport` rect (in physical pixels) for that
//! layer. The editor's glyph instances are rendered in world space relative to
//! the camera — two cameras, two viewports, each seeing only its own layer:
//!
//! ```rust,no_run
//! # use bevy::prelude::*;
//! # use bevy::camera::visibility::RenderLayers;
//! # use bevscode::prelude::*;
//! # fn split(mut commands: Commands, window: Query<&Window>) {
//! let window = window.single().unwrap();
//! let scale = window.scale_factor();
//! let half_phys = (window.width() * scale / 2.0) as u32;
//! let full_phys = (window.height() * scale) as u32;
//! let half_log = window.width() / 2.0;
//!
//! commands.spawn((Camera2d, Camera {
//!     viewport: Some(bevy::camera::Viewport {
//!         physical_position: UVec2::ZERO,
//!         physical_size: UVec2::new(half_phys, full_phys),
//!         ..default()
//!     }),
//!     ..default()
//! }, RenderLayers::layer(0)));
//!
//! commands.spawn((
//!     CodeEditor,
//!     Node { width: Val::Px(half_log), height: Val::Px(window.height()), ..default() },
//!     RenderLayers::layer(0),
//! ));
//! # }
//! ```
//!
//! ## Plugin composition
//!
//! [`crate::plugin::CodeEditorPlugins`] is the full bundle. Hosts that already
//! add parts of the rendering stack can use [`crate::plugin::CodeEditorPlugin`]
//! alone and compose only the sub-plugins they need, or disable specific ones:
//! `CodeEditorPlugins.build().disable::<EditorUiPlugin>()`.
//!
//! ## Keybindings
//!
//! Spawn an `EditorInputManager` with your own `InputMap<EditorAction>` before
//! `PostStartup`; the plugin's default input manager is only added if none exists.

pub mod display_map;
pub mod input;
pub mod plugin;
pub mod settings;
pub mod syntax;
pub mod text_view;
pub mod types;
pub mod ui_kit;

#[cfg(feature = "lsp")]
pub mod lsp_ui;
#[cfg(feature = "lsp")]
pub mod lsp_ui_tempera;

pub mod prelude {
    //! Convenient re-exports for common editor usage.
    //!
    //! Engine-side primitives (`TextBuffer<T>`, `TextFont`, `DisplayLayout`,
    //! `ContentMetrics`, `InstancedTextPlugin`, `InstancedTextPlugins`)
    //! come in via `bevy_instanced_text::prelude::*`. Scroll state is
    //! `bevy::ui::ScrollPosition`. The editor adds the editor plugin (and
    //! the `CodeEditorPlugins` plugin group), the UI plugin, the interaction
    //! plugin, the `CodeEditor` marker, and the handful of file/save events
    //! and scroll config that hosts touch day-to-day. Lower-level types
    //! such as display-map points, fold/wrap state, shaped lines, and
    //! history live on the crate path (`bevscode::types::*`,
    //! `::display_map::*`, etc.) for hosts that need them.

    pub use bevy_instanced_text::prelude::*;

    pub use crate::plugin::{
        AutoResizeViewport, CodeEditorPlugin, CodeEditorPlugins, EditorAppExt, EditorUiPlugin,
    };

    pub use crate::types::editor::{CodeEditor, OpenRequested, SaveRequested};
    pub use crate::types::events::FoldStateChanged;
    pub use crate::types::events::SetLanguageRequested;
    pub use bevy_tree_sitter::TreeSitterGrammar;

    pub use bevy_instanced_text_editor::{
        InstancedTextEditPlugin, InstancedTextInteractionPlugin, RopeBuffer, SetTextRequested,
        TextEditor,
    };

    pub use bevy_instanced_text::TextViewRenderSet;

    pub use crate::input::EditorAction;
    pub use crate::types::{Selection, SelectionCollection};

    pub use crate::settings::{EditorTheme, GutterConfig};

    #[cfg(feature = "lsp")]
    pub use crate::lsp_ui_tempera::{LspPopupRoot, LspUiTemperaPlugin, LspUiViewSet};
    pub use crate::settings::{
        AutoEdit, BracketConfig, CursorLine, EditorUi, Find, Folding, Guides, Indentation, Minimap,
        Misc, Padding, Performance, RenderSettings, Rulers, ScrollConfig, SelectionConfig,
        StickyScroll, SyntaxColors, Wrapping,
    };
    #[cfg(feature = "lsp")]
    pub use crate::settings::{DiagnosticColors, LspConfig, Suggest};

    pub use crate::ui_kit::BevscodePalettePlugin;
    pub use tempera::theme::{
        ColorPalette, FontHandle, MenuTokens, Spacing, ThemePlugin, Typography,
    };
}
