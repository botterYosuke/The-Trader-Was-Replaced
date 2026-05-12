#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@fragment
fn fragment(
    in: VertexOutput,
) -> @location(0) vec4<f32> {
    // World position from vertex output
    let world_pos = in.world_position.xy;
    
    // Grid settings
    let cell_size = 100.0;
    let line_width = 1.0;
    
    // Calculate grid lines
    let grid = abs(fract(world_pos / cell_size - 0.5) - 0.5) / fwidth(world_pos / cell_size);
    let line = min(grid.x, grid.y);
    
    // Main grid lines
    let alpha = 1.0 - smoothstep(0.0, line_width, line);
    
    // Secondary grid (larger cells)
    let cell_size_large = 500.0;
    let grid_large = abs(fract(world_pos / cell_size_large - 0.5) - 0.5) / fwidth(world_pos / cell_size_large);
    let line_large = min(grid_large.x, grid_large.y);
    let alpha_large = 1.0 - smoothstep(0.0, line_width * 1.5, line_large);
    
    let color = vec4<f32>(0.1, 0.1, 0.2, 0.2 * alpha + 0.3 * alpha_large);
    
    if (color.a < 0.01) {
        discard;
    }
    
    return color;
}
