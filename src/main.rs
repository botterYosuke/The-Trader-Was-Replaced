mod trading;
mod ui;
mod grid;
mod camera;

use bevy::prelude::*;
use bevy_pancam::PanCamPlugin;
use trading::{TradingData, price_simulation_system};
use ui::UiPlugin;
use grid::grid_system;
use camera::setup_camera;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Trader Dashboard - Premium Infinite Canvas".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(PanCamPlugin)
        .add_plugins(UiPlugin)
        .insert_resource(TradingData::default())
        .add_systems(Startup, setup_camera)
        .add_systems(Update, (
            price_simulation_system,
            grid_system,
        ))
        .run();
}
