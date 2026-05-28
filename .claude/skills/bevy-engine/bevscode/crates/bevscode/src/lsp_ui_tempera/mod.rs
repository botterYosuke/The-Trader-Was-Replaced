//! Tempera-skinned renderer for the LSP popup data layer.
//!
//! [`crate::lsp_ui`] turns LSP state into semantic *PopupData* components
//! `(line, character, …)`. This module turns those components into a
//! `Node` tree parented under the editor entity, painted with tempera's
//! shadcn-style tokens so popups match the look of the user's other
//! tempera-based apps.
//!
//! Each popup is one update system reading `Changed<*PopupData>`. Spawn
//! and despawn ride on the existing `lsp_ui::sync` lifecycle: every
//! `*PopupData` `#[require]`s a [`Node`], [`LspPopupRoot`], and a
//! `ZIndex`, so the sync layer's `commands.spawn((PopupData, ...))` /
//! `entity.despawn()` already does the heavy lifting.
//!
//! [`CodeEditorPlugins`]: crate::plugin::CodeEditorPlugins

pub mod anchor;
pub mod chrome;
pub mod code_actions;
pub mod completion;
pub mod hover;
pub mod inline_decorations;
pub mod rename;
pub mod signature_help;

use bevy::prelude::*;

pub use anchor::{PopupAnchor, PopupPlacement};

/// Marker on every popup entity, inserted via `#[require]` on each
/// `*PopupData`. An `on_add` observer reparents the entity under its
/// owning editor so popups inherit `UiTargetCamera` and `RenderLayers`.
///
/// The owning editor is carried by the `*PopupData` itself (every popup
/// stores `editor: Entity`).
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct LspPopupRoot;

/// System set for the update systems that turn `*PopupData` components
/// into `Node` trees. Runs after `lsp_ui::sync::*` so the popup data
/// produced this frame is rendered the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LspUiViewSet;

/// Adds the tempera-skinned popup renderer.
///
/// Pulled in by [`CodeEditorPlugins`] under `cfg(feature = "lsp")`.
///
/// [`CodeEditorPlugins`]: crate::plugin::CodeEditorPlugins
#[derive(Default)]
pub struct LspUiTemperaPlugin;

impl Plugin for LspUiTemperaPlugin {
    fn build(&self, app: &mut App) {
        // Tempera's theme resources are the source of truth for every
        // popup color / metric. Pull in `BevscodePalettePlugin` so the
        // resources exist and `EditorTheme` stays in sync.
        if !app.is_plugin_added::<crate::ui_kit::BevscodePalettePlugin>() {
            app.add_plugins(crate::ui_kit::BevscodePalettePlugin);
        }

        // Markdown rendering for hover docs. Idempotent: only add the
        // plugin if no host has already registered it.
        if !app.is_plugin_added::<bevy_markdown::BevyMarkdownPlugin>() {
            app.add_plugins(bevy_markdown::BevyMarkdownPlugin);
        }
        // Default tree-sitter highlighter for fenced code blocks in
        // hover docs. Hosts can replace the resource to swap themes.
        if !app
            .world()
            .contains_resource::<bevy_markdown::MarkdownHighlighter>()
        {
            app.insert_resource(bevy_markdown::MarkdownHighlighter(std::sync::Arc::new(
                bevy_markdown::tree_sitter::TreeSitterHighlighter::with_default_colors(),
            )));
        }

        // Reparent each popup under its owning editor as soon as the data
        // component lands on the entity. One observer per popup type
        // because `editor: Entity` is on the typed *PopupData, not on
        // `LspPopupRoot` (which is the marker the `#[require]` cascade
        // also inserts).
        use crate::lsp_ui::components::*;
        app.add_observer(reparent_on_add::<CompletionPopupData>);
        app.add_observer(reparent_on_add::<HoverPopupData>);
        app.add_observer(reparent_on_add::<SignatureHelpPopupData>);
        app.add_observer(reparent_on_add::<CodeActionsPopupData>);
        app.add_observer(reparent_on_add::<RenameInputData>);

        app.init_resource::<inline_decorations::InlineDecorationsTheme>();

        app.add_systems(
            Update,
            (
                completion::update_completion_popup
                    .after(crate::lsp_ui::sync::sync_completion_popup),
                hover::update_hover_popup.after(crate::lsp_ui::sync::sync_hover_popup),
                signature_help::update_signature_help_popup
                    .after(crate::lsp_ui::sync::sync_signature_help_popup),
                code_actions::update_code_actions_popup
                    .after(crate::lsp_ui::sync::sync_code_actions_popup),
                rename::update_rename_input.after(crate::lsp_ui::sync::sync_rename_input),
                // Document highlights push `RectOverlay`s into the
                // engine's `TextOverlays`; the engine consumes them in
                // `TextViewRenderSet`. Same ordering requirement as
                // selection / cursor highlights.
                inline_decorations::render_document_highlights
                    .before(bevy_instanced_text::TextViewRenderSet),
            )
                .in_set(LspUiViewSet),
        );

        // Dismiss-grace ticks: one per popup kind. Run before the
        // render-update systems above so a grace-fired dismiss is
        // visible the same frame.
        app.add_systems(
            Update,
            (
                crate::lsp_ui::systems::tick_popup_dismiss_hover,
                crate::lsp_ui::systems::tick_popup_dismiss_completion,
                crate::lsp_ui::systems::tick_popup_dismiss_signature,
                crate::lsp_ui::systems::tick_popup_dismiss_code_actions,
                crate::lsp_ui::systems::tick_popup_dismiss_rename,
            )
                .before(LspUiViewSet),
        );
    }
}

trait PopupOwner: Component {
    fn editor(&self) -> Entity;
}

impl PopupOwner for crate::lsp_ui::components::CompletionPopupData {
    fn editor(&self) -> Entity {
        self.editor
    }
}
impl PopupOwner for crate::lsp_ui::components::HoverPopupData {
    fn editor(&self) -> Entity {
        self.editor
    }
}
impl PopupOwner for crate::lsp_ui::components::SignatureHelpPopupData {
    fn editor(&self) -> Entity {
        self.editor
    }
}
impl PopupOwner for crate::lsp_ui::components::CodeActionsPopupData {
    fn editor(&self) -> Entity {
        self.editor
    }
}
impl PopupOwner for crate::lsp_ui::components::RenameInputData {
    fn editor(&self) -> Entity {
        self.editor
    }
}

fn reparent_on_add<P: PopupOwner>(trigger: On<Add, P>, mut commands: Commands, popups: Query<&P>) {
    let entity = trigger.entity;
    let Ok(data) = popups.get(entity) else {
        return;
    };
    commands.entity(entity).insert(ChildOf(data.editor()));
}
