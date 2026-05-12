use bevy::prelude::*;

pub mod components;
pub mod window;
pub mod button;
pub mod systems;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<components::WindowManager>()
            .add_systems(Startup, window::setup_ui)
            .add_systems(Update, (
                systems::ui_update_system,
                systems::chart_rendering_system,
            ));
    }
}
