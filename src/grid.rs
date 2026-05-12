use bevy::prelude::*;

pub fn grid_system(
    mut gizmos: Gizmos,
    camera_query: Query<&OrthographicProjection, With<Camera2d>>,
) {
    let Ok(projection) = camera_query.get_single() else { return; };
    let scale = projection.scale;
    
    // Dynamic grid spacing based on zoom scale
    let spacing = if scale < 0.5 {
        50.0
    } else if scale < 2.0 {
        100.0
    } else if scale < 5.0 {
        500.0
    } else {
        1000.0
    };

    let color = Color::srgba(0.1, 0.1, 0.2, 0.3);
    let secondary_color = Color::srgba(0.1, 0.1, 0.2, 0.1);
    
    let range = 20;
    for i in -range..=range {
        let x = i as f32 * spacing;
        gizmos.line_2d(Vec2::new(x, -5000.0), Vec2::new(x, 5000.0), if i % 5 == 0 { color } else { secondary_color });
        
        let y = i as f32 * spacing;
        gizmos.line_2d(Vec2::new(-5000.0, y), Vec2::new(5000.0, y), if i % 5 == 0 { color } else { secondary_color });
    }
}
