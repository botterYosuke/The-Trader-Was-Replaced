use bevy::prelude::*;
use bevy_cosmic_edit::prelude::CosmicPrimaryCamera;
use bevy_pancam::{DirectionKeys, PanCam};

pub fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        CosmicPrimaryCamera,
        PanCam {
            grab_buttons: vec![MouseButton::Right, MouseButton::Middle],
            move_keys: DirectionKeys::NONE, // disable AWSD pan — conflicts with cosmic_edit
            ..default()
        },
    ));
}
