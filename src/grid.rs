use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

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

#[allow(clippy::type_complexity)]
fn update_grid_position(
    camera_query: Query<
        (&Transform, &Projection),
        (
            With<Camera2d>,
            Or<(Changed<Transform>, Changed<Projection>)>,
        ),
    >,
    mut grid_query: Query<&mut Transform, (With<MainGrid>, Without<Camera2d>)>,
) {
    if let (Ok((camera_transform, projection)), Ok(mut grid_transform)) =
        (camera_query.single(), grid_query.single_mut())
    {
        grid_transform.translation.x = camera_transform.translation.x;
        grid_transform.translation.y = camera_transform.translation.y;

        // Ensure the grid quad is always large enough to cover the screen
        // 100000.0 is the base size. We scale it by the camera's zoom scale.
        let scale = if let Projection::Orthographic(proj) = projection {
            proj.scale
        } else {
            1.0
        };
        grid_transform.scale = Vec3::splat(scale.max(1.0));
    }
}
