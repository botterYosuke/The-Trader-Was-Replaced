// This will all be rewritten soon, looking toward per-widget cursor control
// Rewrite should address issue #93 too

use crate::prelude::*;
use bevy::{
    input::mouse::MouseMotion,
    window::{CursorIcon, CursorOptions, PrimaryWindow, SystemCursorIcon},
};

pub(crate) struct CursorPlugin;

/// Unit resource whose existence in the world disables the cursor plugin systems.
#[derive(Resource)]
pub struct CursorPluginDisabled;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (crate::render_targets::hover_sprites, hover_ui),
                change_cursor,
            )
                .chain()
                .run_if(not(resource_exists::<CursorPluginDisabled>)),
        )
        .add_message::<TextHoverIn>()
        .register_type::<TextHoverIn>()
        .add_message::<TextHoverOut>()
        .register_type::<TextHoverOut>()
        .register_type::<HoverCursor>();
    }
}

/// What cursor icon to show when hovering over a widget
///
/// By default is [`CursorIcon::System(SystemCursorIcon::Text)`]
#[derive(Component, Reflect, Deref)]
pub struct HoverCursor(pub CursorIcon);

impl Default for HoverCursor {
    fn default() -> Self {
        Self(CursorIcon::System(SystemCursorIcon::Text))
    }
}

/// For use with custom cursor control
///
/// Event is emitted when cursor enters a text widget.
/// Event contains the cursor from the buffer's [`HoverCursor`]
#[derive(Message, Reflect, Deref, Debug, Clone)]
pub struct TextHoverIn(pub CursorIcon);

/// For use with custom cursor control
/// Event is emitted when cursor leaves a text widget
#[derive(Message, Reflect, Debug, Clone)]
pub struct TextHoverOut;

pub(crate) fn change_cursor(
    mut evr_hover_in: MessageReader<TextHoverIn>,
    evr_hover_out: MessageReader<TextHoverOut>,
    evr_text_changed: MessageReader<crate::events::CosmicTextChanged>,
    evr_mouse_motion: MessageReader<MouseMotion>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut windows: Query<(&mut CursorIcon, &mut CursorOptions), With<PrimaryWindow>>,
) {
    if windows.iter().len() == 0 {
        return;
    }
    let Ok((mut window_cursor_icon, mut cursor_options)) = windows.single_mut() else {
        return;
    };

    if let Some(ev) = evr_hover_in.read().last() {
        *window_cursor_icon = ev.0.clone();
    } else if !evr_hover_out.is_empty() {
        *window_cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
    }

    if !evr_text_changed.is_empty() {
        cursor_options.visible = false;
    }

    if mouse_buttons.get_just_pressed().len() != 0 || !evr_mouse_motion.is_empty() {
        cursor_options.visible = true;
    }
}

pub(crate) fn hover_ui(
    interaction_query: Query<
        (&Interaction, &HoverCursor),
        (With<CosmicEditBuffer>, Changed<Interaction>),
    >,
    mut evw_hover_in: MessageWriter<TextHoverIn>,
    mut evw_hover_out: MessageWriter<TextHoverOut>,
) {
    for (interaction, hover) in interaction_query.iter() {
        match interaction {
            Interaction::None => {
                evw_hover_out.write(TextHoverOut);
            }
            Interaction::Hovered => {
                evw_hover_in.write(TextHoverIn(hover.0.clone()));
            }
            _ => {}
        }
    }
}
