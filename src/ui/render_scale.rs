use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_cosmic_edit::CosmicRenderScale;

/// cosmic-edit スプライトの CosmicRenderScale を
/// (window DPI × camera ズーム supersample) で駆動する共有コンポーネント。
/// editor 専用だった ZoomResponsiveEditor を置き換え、Startup フィールドも同じ経路に乗せる。
#[derive(Component)]
pub struct RenderScaleResponsive {
    pub max_supersample: f32,
    last_applied: f32,
}

impl RenderScaleResponsive {
    pub fn new(max_supersample: f32) -> Self {
        Self {
            max_supersample,
            last_applied: 0.0,
        }
    }
}

/// window.scale_factor() と camera zoom の両方から目標 render_scale を求め、
/// CosmicRenderScale に書く。metrics や CosmicEditor 内部 buffer には触れないので
/// set_initial_scale(First) / add_editor_to_focused(PostUpdate) の競合とは無関係。
pub fn update_cosmic_render_scale_system(
    window_q: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<&Projection, With<Camera2d>>,
    mut q: Query<(&mut RenderScaleResponsive, &mut CosmicRenderScale)>,
) {
    let dpi = window_q
        .get_single()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0)
        .max(1.0);
    let camera_scale = camera_q
        .get_single()
        .map(|p| {
            if let Projection::Orthographic(proj) = p { proj.scale } else { 1.0 }
        })
        .unwrap_or(1.0)
        .max(0.01);
    let zoom = (1.0 / camera_scale).max(1.0);

    for (mut responsive, mut render_scale) in &mut q {
        let target = (dpi * zoom).clamp(1.0, responsive.max_supersample);
        if (responsive.last_applied - target).abs() < 0.01 {
            continue;
        }
        responsive.last_applied = target;
        render_scale.0 = target;
    }
}
