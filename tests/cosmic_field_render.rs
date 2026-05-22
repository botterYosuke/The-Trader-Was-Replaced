use bevy_cosmic_edit::cosmic_text::{
    Attrs, Buffer, Color as CosmicColor, FontSystem, Metrics, Shaping,
};

/// The GUI shows dark/near-black input text on a near-black bg.
/// render_texture (crates/bevy_cosmic_edit/src/render.rs:213) draws each glyph
/// from its per-glyph `color_opt` (fallback DefaultAttrs). This buffer-only test
/// (no Window/Camera/render pipeline, no pub(crate) types) confirms that after
/// seeding text via the same `set_text(.., Shaping::Advanced)` path the GUI uses,
/// every shaped glyph carries the light color (220,220,220) — isolating the
/// visible bug to the render/clip/DPI layer rather than a color regression.
#[test]
fn shaped_glyphs_carry_light_color() {
    let mut font_system = FontSystem::new();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(9.0, 11.0));
    let light = Attrs::new().color(CosmicColor::rgb(220, 220, 220));
    buffer.set_text(&mut font_system, "2025-01-01", light, Shaping::Advanced);

    let mut glyph_count = 0usize;
    let mut dark_glyphs = 0usize;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            glyph_count += 1;
            let c = glyph.color_opt.unwrap_or(CosmicColor::rgb(0, 0, 0));
            if c.r() < 120 && c.g() < 120 && c.b() < 120 {
                dark_glyphs += 1;
            }
        }
    }

    assert!(
        glyph_count >= 10,
        "expected all glyphs shaped, got {glyph_count}"
    );
    assert_eq!(
        dark_glyphs, 0,
        "{dark_glyphs} glyph(s) carry dark color despite light Attrs"
    );
}
