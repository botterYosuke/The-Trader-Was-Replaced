//! Tempera glue layer.
//!
//! Bevscode's chrome — every LSP popup, the gutter, the editor background —
//! reads its colors and metrics from tempera's `ColorPalette`, `Spacing`,
//! `Typography`, `FontHandle`, and `MenuTokens` resources. That is
//! the same surface tempera uses to style buttons, dropdowns, dialogs in
//! the user's other apps, so swapping the palette here flips the editor in
//! lockstep with the rest of the UI.
//!
//! Pieces:
//!
//! - [`BevscodePalettePlugin`] installs tempera's [`ThemePlugin`] (idempotent)
//!   and runs `sync_palette_into_editor_theme` every time the palette
//!   changes, mapping the shadcn-aligned tokens onto each `EditorTheme`.
//! - [`palette_to_editor_theme`] is the single mapping function. Editor-
//!   specific fields (selection background, line numbers, bracket pair
//!   palette, fold marker, …) have no shadcn equivalent and are left alone.
//! - [`PopupChrome`] is the `SystemParam` bundle popup renderers read so
//!   they pull one parameter instead of five.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use tempera::theme::{ColorPalette, FontHandle, MenuTokens, Spacing, ThemePlugin, Typography};

use crate::settings::EditorTheme;
use crate::types::CodeEditor;

pub mod markdown_theme;
pub use markdown_theme::markdown_theme_from_chrome;

/// App-wide diagnostic palette + squiggle metrics. Lives alongside
/// tempera's [`MenuTokens`] so a host can recolour every diagnostic
/// surface (gutter icon, decoration bar, wavy underline) by swapping
/// one resource. Per-editor overrides go on
/// [`crate::settings::DiagnosticColors`].
#[derive(Resource, Clone, Debug)]
pub struct DiagnosticTokens {
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub hint: Color,
    /// Alpha multiplier applied to the squiggle pills so the wave
    /// recedes slightly under the text.
    pub squiggle_alpha: f32,
    /// Stroke thickness (logical px) of each squiggle pill before DPI
    /// scaling.
    pub squiggle_thickness: f32,
}

impl Default for DiagnosticTokens {
    fn default() -> Self {
        let palette = ColorPalette::dark();
        Self {
            error: palette.destructive,
            // Warning / info / hint have no shadcn token. Tuned to read
            // as a system with `destructive` (similar saturation, sit
            // legibly on the dark popover surface).
            warning: Color::srgb(0.918, 0.706, 0.282),
            info: Color::srgb(0.349, 0.706, 0.929),
            hint: Color::srgb(0.631, 0.631, 0.671),
            squiggle_alpha: 0.85,
            squiggle_thickness: 1.5,
        }
    }
}

/// App-wide gutter metrics: decoration-bar geometry, glyph-icon scale,
/// shared bar alpha. Built from [`Spacing`] so the gutter sits on the
/// same dimension scale as tempera widgets.
#[derive(Resource, Clone, Debug)]
pub struct GutterTokens {
    /// Width (logical px) of a `GutterDecorations` bar.
    pub bar_width: f32,
    /// Corner radius of the bar — defaults to `Spacing::corner_radius_micro`.
    pub bar_radius: f32,
    /// Alpha multiplier applied to the bar's source colour.
    pub bar_alpha: f32,
    /// Glyph-margin icon size as a fraction of line height.
    pub glyph_icon_scale: f32,
    /// Fold-chevron size as a fraction of line height.
    pub chevron_scale: f32,
}

impl Default for GutterTokens {
    fn default() -> Self {
        let spacing = Spacing::default();
        Self {
            bar_width: 3.0,
            bar_radius: spacing.corner_radius_micro,
            bar_alpha: 0.85,
            glyph_icon_scale: 0.6,
            chevron_scale: 0.6,
        }
    }
}

/// Installs tempera's theme resources and keeps every `EditorTheme` in sync
/// with the active [`ColorPalette`].
pub struct BevscodePalettePlugin;

impl Plugin for BevscodePalettePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<ThemePlugin>() {
            app.add_plugins(ThemePlugin);
        }
        app.init_resource::<DiagnosticTokens>()
            .init_resource::<GutterTokens>()
            .add_systems(Update, sync_palette_into_editor_theme);
        #[cfg(feature = "lsp")]
        app.add_systems(Update, sync_diagnostic_tokens_into_editor_colors);
    }
}

/// Gated adapter for [`tempera::TemperaPlugin`].
///
/// `TemperaPlugin::build` unconditionally calls `add_plugins` on every
/// tempera widget plugin, so re-registering it panics when a downstream
/// app has already added tempera itself. `PluginGroupBuilder::add` can't
/// be made conditional from inside a `PluginGroup`, so [`CodeEditorPlugins`]
/// adds this thin wrapper instead — it forwards to `TemperaPlugin` only
/// when tempera hasn't already been installed.
///
/// [`CodeEditorPlugins`]: crate::plugin::CodeEditorPlugins
pub struct EditorTemperaPlugin;

impl Plugin for EditorTemperaPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<tempera::TemperaPlugin>() {
            app.add_plugins(tempera::TemperaPlugin);
        }
    }
}

/// Map the shadcn-aligned slots of `palette` onto the slots of `theme`
/// that have a direct equivalent. Editor-specific colors (selection,
/// bracket pairs, fold, whitespace, link) keep their existing values —
/// they're tuned per-editor and don't belong to the popover/chrome
/// vocabulary.
pub fn palette_to_editor_theme(palette: &ColorPalette, theme: &mut EditorTheme) {
    theme.background = palette.background;
    theme.foreground = palette.foreground;
    theme.separator = palette.border;
    theme.placeholder_color = palette.muted_foreground;
    // Gutter digits + fold chevrons read this; shadcn's muted_foreground
    // is the same grey it uses for de-emphasised body text, which is
    // what we want for line numbers.
    theme.line_numbers = palette.muted_foreground;
}

fn sync_palette_into_editor_theme(
    palette: Res<ColorPalette>,
    mut themes: ParamSet<(
        Query<&mut EditorTheme, With<CodeEditor>>,
        Query<&mut EditorTheme, (With<CodeEditor>, Added<EditorTheme>)>,
    )>,
) {
    if palette.is_changed() {
        for mut theme in themes.p0().iter_mut() {
            palette_to_editor_theme(&palette, &mut theme);
        }
    } else {
        for mut theme in themes.p1().iter_mut() {
            palette_to_editor_theme(&palette, &mut theme);
        }
    }
}

/// Map the four shadcn-aligned diagnostic colours onto every editor's
/// per-entity [`crate::settings::DiagnosticColors`]. Mirrors
/// [`sync_palette_into_editor_theme`] — touch every editor when the
/// resource changes, and freshly-spawned editors on the next tick.
#[cfg(feature = "lsp")]
fn sync_diagnostic_tokens_into_editor_colors(
    tokens: Res<DiagnosticTokens>,
    mut colors: ParamSet<(
        Query<&mut crate::settings::DiagnosticColors, With<CodeEditor>>,
        Query<
            &mut crate::settings::DiagnosticColors,
            (With<CodeEditor>, Added<crate::settings::DiagnosticColors>),
        >,
    )>,
) {
    let apply = |c: &mut crate::settings::DiagnosticColors| {
        c.error = tokens.error;
        c.warning = tokens.warning;
        c.info = tokens.info;
        c.hint = tokens.hint;
    };
    if tokens.is_changed() {
        for mut c in colors.p0().iter_mut() {
            apply(&mut c);
        }
    } else {
        for mut c in colors.p1().iter_mut() {
            apply(&mut c);
        }
    }
}

/// Read-only slice of tempera tokens consumed by popup renderers. Pulls
/// the five resources every chrome path touches in one `SystemParam`.
#[derive(SystemParam)]
pub struct PopupChrome<'w> {
    pub palette: Res<'w, ColorPalette>,
    pub spacing: Res<'w, Spacing>,
    pub typography: Res<'w, Typography>,
    pub font: Res<'w, FontHandle>,
    pub menu: Res<'w, MenuTokens>,
}

impl PopupChrome<'_> {
    /// Body-row font (typography.sm). Matches tempera's command palette
    /// and dropdown row sizes.
    #[must_use]
    pub fn body_font(&self) -> TextFont {
        self.font.text_font(self.typography.sm)
    }

    /// Bold variant of the body font — used for active signature
    /// parameters.
    #[must_use]
    pub fn body_font_bold(&self) -> TextFont {
        self.font.text_font_bold(self.typography.sm)
    }

    /// Smaller secondary font (typography.xs) — pagers, detail spans.
    #[must_use]
    pub fn small_font(&self) -> TextFont {
        self.font.text_font(self.typography.xs)
    }
}
