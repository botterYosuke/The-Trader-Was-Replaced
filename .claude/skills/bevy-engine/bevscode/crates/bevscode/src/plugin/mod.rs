use crate::types::*;
use bevy::app::{PluginGroup, PluginGroupBuilder};
use bevy::prelude::*;

pub mod brackets;
pub mod copy_highlight;
pub mod cursor;
#[cfg(feature = "lsp")]
pub mod diagnostic_underlines;
pub mod editor_ui;
pub mod folding;
pub mod gutter_decorations;
pub mod line_numbers;
pub mod links;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod scroll_animator;
pub mod syntax_highlighting;
pub mod ui_elements;

#[cfg(test)]
mod flicker_test;

#[cfg(feature = "lsp")]
pub use self::lsp::LspPlugin;

pub use self::brackets::BracketPlugin;
pub use self::cursor::CursorPlugin;
#[cfg(feature = "lsp")]
pub use self::diagnostic_underlines::DiagnosticUnderlineRects;
pub use self::editor_ui::{AutoResizeViewport, EditorUiPlugin};
pub use self::folding::FoldingPlugin;
pub use self::gutter_decorations::{
    DecorationKind, GlyphKind, GlyphMarginClicked, GlyphMarginRects, GlyphMarker, GlyphMarkers,
    GutterDecorations, GutterIcon, IconAtlas, LineDecoration, LineDecorationRects,
};
pub use self::links::{HoveredLink, LinkRange, LinkRanges, LinkRects};
pub use self::scroll_animator::{ScrollAnimator, ScrollAnimatorPlugin};

pub use self::syntax_highlighting::{EditorSyntaxState, SyntaxPlugin};

pub(crate) use self::brackets::{update_bracket_highlight, update_bracket_match};
pub(crate) use self::cursor::update_cursor_line_highlight;
pub(crate) use self::line_numbers::{
    setup_gutter_text_view, sync_gutter_container, sync_gutter_text_font, sync_gutter_text_view,
};
pub(crate) use self::ui_elements::{
    update_fold_highlights, update_indent_guides, update_rulers, update_selection_highlight,
    update_whitespace_markers,
};

#[derive(Component)]
#[require(KeyRepeatState)]
pub struct EditorInputManager;

pub fn to_bevy_coords_dynamic(x: f32, y: f32, viewport_w: f32, viewport_h: f32) -> Vec3 {
    let world_x = -viewport_w / 2.0 + x;
    let world_y = viewport_h / 2.0 - y;
    Vec3::new(world_x, world_y, 0.0)
}

pub fn to_bevy_coords_left_aligned(
    x: f32,
    y: f32,
    viewport_w: f32,
    viewport_h: f32,
    scroll_x: f32,
) -> Vec3 {
    let world_x = -viewport_w / 2.0 + x - scroll_x;
    let world_y = viewport_h / 2.0 - y;
    Vec3::new(world_x, world_y, 0.0)
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputSet;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionDispatchSet;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApplyStateSet;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderingSet;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorSetupSet;

/// Convenience trait for hosts adding their own per-action handler systems.
///
/// Drops a system into [`Update`] inside [`InputSet`] running after
/// [`ActionDispatchSet`] — the same wiring used by the built-in IDE handlers.
/// Hosts that want different scheduling can call `app.add_systems(...)`
/// directly with the public system-set markers from this module.
///
/// ```rust,no_run
/// # use bevy::prelude::*;
/// # use bevscode::prelude::*;
/// # use bevscode::plugin::EditorAppExt;
/// # #[derive(Message)] struct MyActionRequested;
/// # fn handle_my_action(_: MessageReader<MyActionRequested>) {}
/// # fn build(app: &mut App) {
/// app.add_message::<MyActionRequested>()
///    .add_editor_action_handler(handle_my_action);
/// # }
/// ```
pub trait EditorAppExt {
    /// Register a per-action handler system that runs after the action dispatcher.
    fn add_editor_action_handler<M>(
        &mut self,
        system: impl bevy::ecs::schedule::IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>,
    ) -> &mut Self;
}

impl EditorAppExt for App {
    fn add_editor_action_handler<M>(
        &mut self,
        system: impl bevy::ecs::schedule::IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_systems(Update, system.in_set(InputSet).after(ActionDispatchSet));
        self
    }
}

/// Core editor plugin -- systems, observers, and event wiring.
/// Most hosts should use [`CodeEditorPlugins`] instead.
#[derive(Default)]
pub struct CodeEditorPlugin;

impl Plugin for CodeEditorPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "lsp")]
        if !app.is_plugin_added::<bevy_lsp::LspPlugin>() {
            app.add_plugins(bevy_lsp::LspPlugin);
        }

        app.configure_sets(
            Update,
            (
                InputSet,
                bevy_instanced_text_editor::EditApplySet,
                bevy_instanced_text_editor::EditEmitSet,
                ApplyStateSet,
            )
                .chain(),
        );
        app.configure_sets(
            PostUpdate,
            RenderingSet
                .after(bevy_instanced_text::LayoutProduceSet)
                .before(bevy_instanced_text::TextViewRenderSet),
        );

        app.add_systems(PostStartup, spawn_default_input_manager);

        app.add_message::<SaveRequested>();
        app.add_message::<OpenRequested>();
        app.add_message::<crate::types::events::FoldStateChanged>();
        app.register_type::<crate::types::events::FoldStateChanged>();
        app.add_message::<crate::types::events::SetLanguageRequested>();
        register_ide_action_events(app);

        #[cfg(feature = "lsp")]
        app.init_resource::<crate::input::handlers::lsp_followup::PendingActionFollowup>();

        app.add_observer(crate::input::on_focused_keyboard);
        app.add_systems(
            Update,
            crate::input::dispatch_action_events
                .in_set(InputSet)
                .in_set(ActionDispatchSet),
        );
        app.add_observer(crate::input::on_fold_gutter_press);
        app.add_observer(crate::input::on_alt_click);
        app.add_observer(crate::input::on_click_past_eol_unfold);
        app.add_observer(crate::input::on_pointer_move_for_gutter_hover);
        app.add_observer(crate::plugin::links::on_ctrl_click_open_url);
        app.add_observer(crate::plugin::links::on_pointer_move_for_link_hover);
        app.add_observer(crate::plugin::gutter_decorations::on_glyph_margin_press);
        app.add_message::<crate::plugin::gutter_decorations::GlyphMarginClicked>();
        #[cfg(feature = "lsp")]
        {
            app.add_observer(crate::input::on_ctrl_click_goto_definition);
            app.add_observer(crate::input::on_pointer_move_for_hover);
            app.add_observer(crate::input::on_pointer_out_for_hover);
            app.add_systems(
                Update,
                crate::input::tick_lsp_hover_timer
                    .in_set(InputSet)
                    .after(ActionDispatchSet),
            );
        }

        register_handler_systems(app);

        app.add_systems(
            Update,
            crate::plugin::copy_highlight::handle_copy_with_highlighting.in_set(InputSet),
        );

        app.add_observer(crate::input::on_edit_invalidate_caches);

        app.add_systems(
            Update,
            ui_elements::auto_scroll_to_cursor
                .run_if(ui_elements::should_auto_scroll)
                .in_set(ApplyStateSet),
        );
    }
}

/// Full editor plugin group -- all sub-plugins in one add.
pub struct CodeEditorPlugins;

impl PluginGroup for CodeEditorPlugins {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>()
            .add(bevy_instanced_text::gpu::GlyphAtlasPlugin)
            .add(bevy_instanced_text::gpu::InstancedTextRenderPlugin)
            .add(bevy_instanced_text::view::plugin::InstancedTextPlugin)
            .add(bevy::input_focus::InputDispatchPlugin)
            .add(bevy_instanced_text_editor::InstancedTextEditPlugin::without_typing_observer())
            .add(leafwing_input_manager::plugin::InputManagerPlugin::<
                crate::input::EditorAction,
            >::default())
            .add(CodeEditorPlugin)
            .add(CursorPlugin)
            .add(syntax_highlighting::SyntaxPlugin)
            .add(FoldingPlugin)
            .add(BracketPlugin)
            .add(EditorUiPlugin)
            .add(ScrollAnimatorPlugin)
            .add(crate::display_map::DisplayMapPlugin);
        let group = group
            .add(crate::ui_kit::EditorTemperaPlugin)
            .add(crate::ui_kit::BevscodePalettePlugin);
        #[cfg(feature = "lsp")]
        let group = group
            .add(LspPlugin)
            .add(crate::lsp_ui_tempera::LspUiTemperaPlugin);
        group
    }
}

fn register_ide_action_events(app: &mut App) {
    use crate::input::action_events::*;

    macro_rules! register {
        ($($ty:ty),* $(,)?) => {
            $( app.add_message::<$ty>(); )*
        };
    }

    register!(
        GotoLineRequested,
        RequestCompletionRequested,
        GotoDefinitionRequested,
        RenameSymbolRequested,
        AddCursorAtNextOccurrenceRequested,
        AddCursorAboveRequested,
        AddCursorBelowRequested,
        ClearSecondaryCursorsRequested,
        ToggleFoldRequested,
        FoldRequested,
        UnfoldRequested,
        FoldAllRequested,
        UnfoldAllRequested,
    );
}

fn register_handler_systems(app: &mut App) {
    use crate::input::handlers::*;

    app.add_systems(
        Update,
        (
            multi_cursor::handle_add_cursor_at_next_occurrence,
            multi_cursor::handle_add_cursor_above,
            multi_cursor::handle_add_cursor_below,
            multi_cursor::handle_clear_secondary_cursors,
            file::handle_goto_line,
        )
            .in_set(InputSet)
            .after(ActionDispatchSet),
    );

    app.add_systems(
        Update,
        (
            folding::handle_toggle_fold,
            folding::handle_fold,
            folding::handle_unfold,
            folding::handle_fold_all,
            folding::handle_unfold_all,
        )
            .in_set(InputSet)
            .after(ActionDispatchSet),
    );

    app.add_systems(
        Update,
        folding::emit_fold_state_changed
            .in_set(InputSet)
            .after(folding::handle_unfold_all),
    );

    app.add_systems(
        Update,
        crate::syntax::language_swap::handle_set_language
            .in_set(InputSet)
            .after(ActionDispatchSet),
    );

    #[cfg(feature = "lsp")]
    app.add_systems(
        Update,
        (
            lsp::handle_request_completion,
            lsp::handle_goto_definition,
            lsp::handle_rename_symbol,
        )
            .in_set(InputSet)
            .after(ActionDispatchSet),
    );

    #[cfg(feature = "lsp")]
    app.add_systems(
        Update,
        crate::input::handlers::lsp_followup::lsp_followup
            .in_set(InputSet)
            .after(lsp::handle_request_completion),
    );
}

fn spawn_default_input_manager(
    mut commands: Commands,
    existing: Query<(), With<EditorInputManager>>,
) {
    if !existing.is_empty() {
        return;
    }
    commands.spawn((
        EditorInputManager,
        crate::input::default_input_map(),
        Name::new("EditorInputManager"),
    ));
}
