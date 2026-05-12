use bevy::prelude::*;
use bevy_pancam::PanCam;

pub fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PanCam {
            grab_buttons: vec![MouseButton::Right, MouseButton::Middle],
            ..default()
        },
    ));
}
