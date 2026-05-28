use crate::widget::CosmicPadding;
use crate::{cosmic_edit::ReadOnly, prelude::*, widget::WidgetSet};
use crate::{cosmic_edit::*, CosmicWidgetSize};
use bevy::render::render_resource::Extent3d;
use cosmic_text::{Color as CosmicColor, Edit, Editor as CosmicTextEditor, Metrics};
use image::{imageops::FilterType, GenericImageView};

/// System set for cosmic text rendering systems. Runs in [`PostUpdate`]
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RenderSet;

pub(crate) struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<SwashCache>() {
            app.insert_resource(SwashCache::default());
        } else {
            debug!("Skipping inserting `SwashCache` resource");
        }
        app.add_systems(Update, blink_cursor).add_systems(
            PostUpdate,
            (render_texture,).in_set(RenderSet).after(WidgetSet),
        );
    }
}

pub(crate) fn blink_cursor(mut q: Query<&mut CosmicEditor, Without<ReadOnly>>, time: Res<Time>) {
    for mut e in q.iter_mut() {
        e.cursor_timer.tick(time.delta());
        if e.cursor_timer.just_finished() {
            e.cursor_visible = !e.cursor_visible;
            e.set_redraw(true);
        }
    }
}

/// Maps the x coordinate for a draw call: translates by column offset, padding, and scroll.
/// Uses i64 intermediate arithmetic to prevent i32 overflow on Retina/HiDPI displays
/// where cosmic_text may return large scaled pixel coordinates (issue #60).
fn draw_coord_x(x: i32, col: i32, pad_x: i32, scroll_x: i32) -> i32 {
    (x as i64 + col as i64 + pad_x as i64 - scroll_x as i64)
        .clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Maps the y coordinate for a draw call: translates by row offset and padding.
/// Uses i64 intermediate arithmetic to prevent i32 overflow (issue #60).
fn draw_coord_y(y: i32, row: i32, pad_y: i32) -> i32 {
    (y as i64 + row as i64 + pad_y as i64)
        .clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn draw_pixel(buffer: &mut [u8], width: i32, height: i32, x: i32, y: i32, color: CosmicColor) {
    let a_a = color.a() as u32;
    if a_a == 0 {
        // Do not draw if alpha is zero
        return;
    }

    if y < 0 || y >= height {
        // Skip if y out of bounds
        return;
    }

    if x < 0 || x >= width {
        // Skip if x out of bounds
        return;
    }

    let offset = (y as usize * width as usize + x as usize) * 4;

    let bg = bevy::prelude::Color::srgba_u8(
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    );

    // TODO: if alpha is 100% or bg is empty skip blending

    let fg = Srgba::rgba_u8(color.r(), color.g(), color.b(), color.a());

    let premul = (fg * fg.alpha).with_alpha(color.a() as f32 / 255.0);

    let out = premul + (bg.to_srgba() * (1.0 - fg.alpha));

    buffer[offset + 2] = (out.blue * 255.0) as u8;
    buffer[offset + 1] = (out.green * 255.0) as u8;
    buffer[offset] = (out.red * 255.0) as u8;
    buffer[offset + 3] = (out.alpha * 255.0) as u8;
}

/// Prepares a cloned [`cosmic_text::Buffer`] for high-res (`render_scale > 1.0`) rendering.
///
/// Scales **only** metrics and the buffer size — the GPU texture is enlarged and
/// glyphs are rasterized at higher resolution, but the *logical layout* (which lines
/// are visible, where wrapping happens) is unchanged. The shadow buffer is a
/// high-resolution viewport; the scroll state that decides *which logical line to
/// start drawing from* is copied from the original buffer as-is.
///
/// The scaled metrics are derived from **this buffer's own current metrics** ×
/// `render_scale`, NOT from the caller's `CosmicEditBuffer`. When focused, the
/// source clone is the `editor`'s internal buffer (e.g. font 14 / line 18, no DPI
/// applied); when unfocused, it's `CosmicEditBuffer` (e.g. font 28 / line 36,
/// already DPI-scaled by `set_initial_scale`). Scaling each clone against its own
/// metrics avoids the double-scale bug where the focused path would otherwise be
/// handed the DPI-scaled 28/36 baseline.
///
/// `Buffer.scroll` is deliberately **left untouched**. `Scroll::vertical` is a pixel
/// offset *within* `Scroll::line` (see `cosmic_text::Scroll`), normally 0 for a
/// line-stepped editor. `shape_until_scroll` advances `scroll.line` whenever
/// `layout_height < scroll.vertical`. Scaling `scroll.vertical` by `render_scale`
/// would balloon a tiny logical offset past the (also scaled) `layout_height` at
/// high zoom, so `scroll.line` would jump by a zoom-dependent amount — that was the
/// real cause of "the focused editor starts at a different line per zoom level".
///
/// Uses `set_metrics_and_size` (single call): `set_metrics` and `set_size` each
/// delegate to it internally and each triggers its own `relayout` +
/// `shape_until_scroll`, so the split form shapes the buffer twice for no benefit.
fn scale_buffer_for_render(
    buffer: &mut cosmic_text::Buffer,
    font_system: &mut cosmic_text::FontSystem,
    render_scale: f32,
    render_size: Vec2,
) {
    let m = buffer.metrics();
    let scaled_metrics = Metrics::new(m.font_size * render_scale, m.line_height * render_scale);
    buffer.set_metrics_and_size(
        font_system,
        scaled_metrics,
        Some(render_size.x),
        Some(render_size.y),
    );
}

/// Renders to the [CosmicRenderOutput]
#[allow(unused_mut)] // for .set_redraw(false) commented out
fn render_texture(
    mut query: Query<(
        Option<&mut CosmicEditor>,
        &mut CosmicEditBuffer,
        &DefaultAttrs,
        &CosmicBackgroundImage,
        &CosmicBackgroundColor,
        &CursorColor,
        &SelectionColor,
        Option<&SelectedTextColor>,
        &CosmicRenderOutput,
        CosmicWidgetSize,
        &CosmicPadding,
        &XOffset,
        Option<&ReadOnly>,
        &CosmicTextAlign,
        Option<&CosmicRenderScale>,
    )>,
    mut font_system: ResMut<CosmicFontSystem>,
    mut images: ResMut<Assets<Image>>,
    mut swash_cache_state: ResMut<SwashCache>,
) {
    for (
        editor,
        mut buffer,
        attrs,
        background_image,
        fill_color,
        cursor_color,
        selection_color,
        selected_text_color_option,
        canvas,
        size,
        padding,
        x_offset,
        readonly_opt,
        position,
        render_scale_opt,
    ) in query.iter_mut()
    {
        let Ok(logical_size) = size.logical_size() else {
            continue;
        };

        // avoids a panic
        if logical_size.x == 0. || logical_size.y == 0. {
            debug!(
                message = "Size of buffer is zero, skipping",
                // once = "This log only appears once"
            );
            continue;
        }

        // CosmicRenderScale > 1.0: render texture at higher resolution than logical size.
        // Sprite.custom_size (= logical_size) and Transform stay unchanged so hit-testing
        // and text layout are unaffected. Only the GPU texture is enlarged.
        let render_scale = render_scale_opt.map(|r| r.0).unwrap_or(1.0).max(1.0);
        let render_size = logical_size * render_scale;

        // Draw background at render_size
        let mut pixels = vec![0; render_size.x as usize * render_size.y as usize * 4];
        if let Some(bg_image) = background_image.0.clone() {
            if let Some(image) = images.get(&bg_image) {
                let mut dynamic_image = image.clone().try_into_dynamic().unwrap();
                if image.size() != render_size.as_uvec2() {
                    dynamic_image = dynamic_image.resize_to_fill(
                        render_size.x as u32,
                        render_size.y as u32,
                        FilterType::Triangle,
                    );
                }
                for (i, (_, _, rgba)) in dynamic_image.pixels().enumerate() {
                    if let Some(p) = pixels.get_mut(i * 4..(i + 1) * 4) {
                        p[0] = rgba[0];
                        p[1] = rgba[1];
                        p[2] = rgba[2];
                        p[3] = rgba[3];
                    }
                }
            }
        } else {
            let bg = fill_color.0.to_cosmic();
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[0] = bg.r();
                pixel[1] = bg.g();
                pixel[2] = bg.b();
                pixel[3] = bg.a();
            }
        }

        let font_color = attrs
            .0
            .color_opt
            .unwrap_or(cosmic_text::Color::rgb(0, 0, 0));

        let min_pad = match position {
            CosmicTextAlign::Center { padding } => *padding as f32,
            CosmicTextAlign::TopLeft { padding } => *padding as f32,
            CosmicTextAlign::Left { padding } => *padding as f32,
        };

        // draw_closure always works in render_size coordinates.
        // When render_scale == 1.0 render_size == logical_size so offsets are identical.
        // When render_scale > 1.0 padding and x_offset are scaled proportionally.
        let pad_x = (padding.x.max(min_pad) * render_scale) as i32;
        let pad_y = (padding.y * render_scale) as i32;
        let scroll_x = (x_offset.left as f32 * render_scale) as i32;

        let draw_closure = |x, y, w, h, color| {
            for row in 0..h as i32 {
                for col in 0..w as i32 {
                    draw_pixel(
                        &mut pixels,
                        render_size.x as i32,
                        render_size.y as i32,
                        draw_coord_x(x, col, pad_x, scroll_x),
                        draw_coord_y(y, row, pad_y),
                        color,
                    );
                }
            }
        };

        if render_scale > 1.001 {
            // High-res path: clone the buffer, apply scaled metrics + size, shape, draw.
            // Original buffer/editor are NOT modified, so layout and hit-testing stay correct.

            if let Some(mut editor) = editor {
                if !editor.redraw() {
                    continue;
                }

                let original_cursor = editor.cursor();

                // Clone buffer from editor's internal state (focused path)
                let shadow_buf = editor.with_buffer(|b| {
                    let mut clone = b.clone();
                    scale_buffer_for_render(
                        &mut clone,
                        &mut font_system.0,
                        render_scale,
                        render_size,
                    );
                    clone
                });

                // Wrap in temporary Editor so cursor + selection are drawn at scaled coords
                let mut shadow_ed = CosmicTextEditor::new(shadow_buf);
                shadow_ed.set_cursor(original_cursor);

                let c_color = cursor_color.0;
                let c_opacity = if editor.cursor_visible && readonly_opt.is_none() {
                    c_color.alpha()
                } else {
                    0.
                };
                let c_cosmic = c_color.with_alpha(c_opacity).to_cosmic();
                let s_cosmic = selection_color.0.to_cosmic();
                let st_cosmic = selected_text_color_option
                    .map(|c| c.0.to_cosmic())
                    .unwrap_or(font_color);

                shadow_ed.draw(
                    &mut font_system.0,
                    &mut swash_cache_state.0,
                    font_color,
                    c_cosmic,
                    s_cosmic,
                    st_cosmic,
                    draw_closure,
                );
            } else {
                if !buffer.redraw() {
                    continue;
                }

                let mut shadow = buffer.0.clone();
                scale_buffer_for_render(
                    &mut shadow,
                    &mut font_system.0,
                    render_scale,
                    render_size,
                );

                shadow.draw(
                    &mut font_system.0,
                    &mut swash_cache_state.0,
                    font_color,
                    draw_closure,
                );
            }
        } else {
            // Normal rendering path (render_scale == 1.0)
            if let Some(mut editor) = editor {
                if !editor.redraw() {
                    continue;
                }

                let cursor_color = cursor_color.0;
                let cursor_opacity = if editor.cursor_visible && readonly_opt.is_none() {
                    cursor_color.alpha()
                } else {
                    0.
                };
                let cursor_color = cursor_color.with_alpha(cursor_opacity).to_cosmic();
                let selection_color = selection_color.0.to_cosmic();
                let selected_text_color = selected_text_color_option
                    .map(|c| c.0.to_cosmic())
                    .unwrap_or(font_color);

                editor.draw(
                    &mut font_system.0,
                    &mut swash_cache_state.0,
                    font_color,
                    cursor_color,
                    selection_color,
                    selected_text_color,
                    draw_closure,
                );
            } else {
                if !buffer.redraw() {
                    continue;
                }
                buffer.draw(
                    &mut font_system.0,
                    &mut swash_cache_state.0,
                    font_color,
                    draw_closure,
                );
            }
        }

        if let Some(prev_image) = images.get_mut(&canvas.0) {
            if let Some(data) = prev_image.data.as_mut() {
                data.clear();
                data.extend_from_slice(pixels.as_slice());
            } else {
                prev_image.data = Some(pixels);
            }
            prev_image.resize(Extent3d {
                width: render_size.x as u32,
                height: render_size.y as u32,
                depth_or_array_layers: 1,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{draw_coord_x, draw_coord_y};

    /// Regression guard for issue #60: draw_closure must not overflow i32 on Retina/HiDPI.
    /// On macOS Retina (render_scale≈2) cosmic_text returns large pixel coords that, combined
    /// with pad_x/col, used to exceed i32::MAX → panic in debug mode.
    #[test]
    fn draw_coord_x_clamps_for_large_retina_values() {
        // (i32::MAX - 10) + 5 + 10 - 0 would exceed i32::MAX without fix
        let px = draw_coord_x(i32::MAX - 10, 5, 10, 0);
        assert_eq!(px, i32::MAX);
    }

    #[test]
    fn draw_coord_y_clamps_for_large_retina_values() {
        // (i32::MAX - 5) + 3 + 10 would exceed i32::MAX without fix
        let py = draw_coord_y(i32::MAX - 5, 3, 10);
        assert_eq!(py, i32::MAX);
    }

    #[test]
    fn draw_coord_x_normal_case_unchanged() {
        // Normal (non-overflow) values must not be affected by the fix
        assert_eq!(draw_coord_x(10, 5, 3, 2), 16);
    }

    #[test]
    fn draw_coord_y_normal_case_unchanged() {
        assert_eq!(draw_coord_y(10, 5, 3), 18);
    }
}
