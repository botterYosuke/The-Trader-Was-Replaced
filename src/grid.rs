use bevy::prelude::*;
use bevy::sprite::{Material2d, Material2dPlugin, AlphaMode2d};
use bevy::render::render_resource::{AsBindGroup, ShaderRef};

pub struct GridPlugin;

impl Plugin for GridPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<GridMaterial>::default())
            .add_systems(Startup, setup_grid)
            .add_systems(Update, update_grid_position);
    }
}

#[derive(Component)]
pub struct MainGrid;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GridMaterial {}

impl Material2d for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/grid.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

fn setup_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    // Spawn a quad for the grid. Size doesn't need to be huge if it follows the camera,
    // but should be large enough to cover the screen even when zoomed out.
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(100000.0, 100000.0))),
        MeshMaterial2d(materials.add(GridMaterial {})),
        Transform::from_xyz(0.0, 0.0, -1.0),
        MainGrid,
    ));
}

fn update_grid_position(
    camera_query: Query<(&Transform, &OrthographicProjection), (With<Camera2d>, Or<(Changed<Transform>, Changed<OrthographicProjection>)>)>,
    mut grid_query: Query<&mut Transform, (With<MainGrid>, Without<Camera2d>)>,
) {
    if let Ok((camera_transform, projection)) = camera_query.get_single() {
        if let Ok(mut grid_transform) = grid_query.get_single_mut() {
            grid_transform.translation.x = camera_transform.translation.x;
            grid_transform.translation.y = camera_transform.translation.y;
            
            // Ensure the grid quad is always large enough to cover the screen
            // 100000.0 is the base size. We scale it by the camera's zoom scale.
            grid_transform.scale = Vec3::splat(projection.scale.max(1.0));
        }
    }
}
