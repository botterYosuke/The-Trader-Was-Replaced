//! Editor UI plugin for rendering editor visual elements

use bevy::prelude::*;
use bevy_instanced_text::{MonoCellWidth, TextOverlays, TextUnderlays};

use crate::settings::*;
use crate::types::{
    BracketMatchRects, CaretRects, CodeEditor, CursorLineRects, FoldHighlightRects,
    IndentGuideRects, RulerRects, SelectionRects, Separator, WhitespaceRects,
};

use super::gutter_decorations::{
    drive_chevron_rotation, setup_icon_atlas, sync_fold_chevron_icons, sync_gutter_decoration_bars,
    sync_gutter_icons, update_glyph_margin_overlays, update_line_decoration_overlays,
    GlyphMarginRects, LineDecorationRects,
};
use super::links::{update_link_overlays, LinkRects};
use super::{
    setup_gutter_text_view, sync_gutter_container, sync_gutter_text_font, sync_gutter_text_view,
    to_bevy_coords_left_aligned, update_cursor_line_highlight, update_fold_highlights,
    update_indent_guides, update_rulers, update_selection_highlight, update_whitespace_markers,
    EditorSetupSet,
};
use bevy_instanced_text::gpu::GlyphAtlas;

use super::{update_bracket_highlight, update_bracket_match};

#[derive(Default)]
pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_editor_ui.after(EditorSetupSet));
        if app.is_plugin_added::<bevy::image::ImagePlugin>() {
            if !app.is_plugin_added::<bevy_resvg::plugin::SvgPlugin>() {
                app.add_plugins(bevy_resvg::plugin::SvgPlugin);
            }
            app.add_systems(PreStartup, setup_icon_atlas);
            app.add_systems(
                Update,
                (
                    sync_gutter_icons,
                    sync_gutter_decoration_bars,
                    sync_fold_chevron_icons,
                    drive_chevron_rotation.after(sync_fold_chevron_icons),
                )
                    .after(setup_gutter_text_view),
            );
        }

        app.add_systems(Update, setup_gutter_text_view);

        app.add_systems(Update, sync_node_from_window);

        app.add_observer(sync_indent_config_on_change);
        app.add_observer(disable_scroll_beyond_last_line);

        app.add_observer(detect_indentation_on_buffer_insert);
        app.add_observer(detect_indentation_on_first_edit);
        app.add_systems(Update, sync_cursor_icon);
        app.add_systems(Update, sync_automatic_layout);

        app.add_systems(
            Update,
            update_separator_on_resize.run_if(viewport_or_gutter_changed),
        );

        app.add_systems(
            PostUpdate,
            (
                resolve_gutter_layout,
                apply_editor_padding.after(resolve_gutter_layout),
                sync_gutter_container.after(resolve_gutter_layout),
                sync_gutter_text_font.after(resolve_gutter_layout),
                sync_gutter_text_view.after(sync_gutter_text_font),
            )
                .before(bevy_instanced_text::LayoutProduceSet),
        );

        app.add_systems(
            PostUpdate,
            update_font_metrics
                .run_if(bevy_instanced_text::gpu::atlas_ready)
                .in_set(super::RenderingSet),
        );

        app.add_systems(
            PostUpdate,
            (update_selection_highlight, update_cursor_line_highlight).in_set(super::RenderingSet),
        );

        app.add_systems(
            PostUpdate,
            (
                update_indent_guides,
                update_rulers,
                update_fold_highlights,
                update_link_overlays,
                update_whitespace_markers,
                update_glyph_margin_overlays,
                update_line_decoration_overlays,
            )
                .in_set(super::RenderingSet),
        );

        #[cfg(feature = "lsp")]
        app.add_systems(
            Update,
            super::gutter_decorations::sync_lsp_glyph_markers.in_set(super::ApplyStateSet),
        );

        #[cfg(feature = "lsp")]
        app.add_systems(
            PostUpdate,
            super::diagnostic_underlines::update_diagnostic_underlines.in_set(super::RenderingSet),
        );

        app.add_systems(Update, update_bracket_match.in_set(super::ApplyStateSet));
        app.add_systems(
            PostUpdate,
            update_bracket_highlight
                .after(update_indent_guides)
                .in_set(super::RenderingSet),
        );

        app.add_systems(
            PostUpdate,
            merge_overlay_components
                .after(super::RenderingSet)
                .before(bevy_instanced_text::TextViewRenderSet),
        );
    }
}

use bevy::ecs::query::QueryData;

#[derive(QueryData)]
struct OverlaySourcesView {
    sel: &'static SelectionRects,
    guides: &'static IndentGuideRects,
    rulers: &'static RulerRects,
    fold_hl: &'static FoldHighlightRects,
    cursor_line: &'static CursorLineRects,
    carets: &'static CaretRects,
    brackets: &'static BracketMatchRects,
    links: &'static LinkRects,
    whitespace: &'static WhitespaceRects,
    glyph_margin: &'static GlyphMarginRects,
    line_dec: &'static LineDecorationRects,
}

fn assemble_overlays(
    src: &OverlaySourcesViewItem<'_, '_>,
    underlays: &mut TextUnderlays,
    overlays: &mut TextOverlays,
) {
    underlays.0.clear();
    underlays.0.extend_from_slice(&src.guides.0);
    underlays.0.extend_from_slice(&src.rulers.0);
    underlays.0.extend_from_slice(&src.fold_hl.0);
    underlays.0.extend_from_slice(&src.sel.0);
    underlays.0.extend_from_slice(&src.line_dec.0);

    overlays.0.clear();
    overlays.0.extend_from_slice(&src.cursor_line.0);
    overlays.0.extend_from_slice(&src.carets.0);
    overlays.0.extend_from_slice(&src.brackets.0);
    overlays.0.extend_from_slice(&src.links.0);
    overlays.0.extend_from_slice(&src.whitespace.0);
    overlays.0.extend_from_slice(&src.glyph_margin.0);
}

#[cfg(not(feature = "lsp"))]
fn merge_overlay_components(
    mut query: Query<
        (OverlaySourcesView, &mut TextUnderlays, &mut TextOverlays),
        (
            With<CodeEditor>,
            Or<(
                Changed<SelectionRects>,
                Changed<IndentGuideRects>,
                Changed<RulerRects>,
                Changed<FoldHighlightRects>,
                Changed<CursorLineRects>,
                Changed<CaretRects>,
                Changed<BracketMatchRects>,
                Changed<LinkRects>,
                Changed<WhitespaceRects>,
                Changed<GlyphMarginRects>,
                Changed<LineDecorationRects>,
            )>,
        ),
    >,
) {
    for (src, mut underlays, mut overlays) in &mut query {
        assemble_overlays(&src, &mut underlays, &mut overlays);
    }
}

#[cfg(feature = "lsp")]
fn merge_overlay_components(
    mut query: Query<
        (
            OverlaySourcesView,
            &super::diagnostic_underlines::DiagnosticUnderlineRects,
            &mut TextUnderlays,
            &mut TextOverlays,
        ),
        (
            With<CodeEditor>,
            Or<(
                Changed<SelectionRects>,
                Changed<IndentGuideRects>,
                Changed<RulerRects>,
                Changed<FoldHighlightRects>,
                Changed<CursorLineRects>,
                Changed<CaretRects>,
                Changed<BracketMatchRects>,
                Changed<LinkRects>,
                Changed<WhitespaceRects>,
                Changed<GlyphMarginRects>,
                Changed<LineDecorationRects>,
                Changed<super::diagnostic_underlines::DiagnosticUnderlineRects>,
            )>,
        ),
    >,
) {
    for (src, diag_underlines, mut underlays, mut overlays) in &mut query {
        assemble_overlays(&src, &mut underlays, &mut overlays);
        overlays.0.extend_from_slice(&diag_underlines.0);
    }
}

/// Opt-in marker: editors with this component have their `Node` automatically
/// sized to the primary window via a full-screen `Val::Percent(100.0)` node.
/// Hosts that manage layout themselves (multi-pane, render-to-texture) omit this.
#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
pub struct AutoResizeViewport;

fn sync_node_from_window(
    mut editors: Query<&mut Node, (With<CodeEditor>, With<AutoResizeViewport>)>,
) {
    let target_w = Val::Vw(100.0);
    let target_h = Val::Vh(100.0);
    for mut node in editors.iter_mut() {
        if node.width != target_w {
            node.width = target_w;
        }
        if node.height != target_h {
            node.height = target_h;
        }
    }
}

/// Defaults `scroll_beyond_last_line` to `false` -- in a scrollbar-less
/// embed the Monaco default (`true`) scrolls into nothing.
fn disable_scroll_beyond_last_line(
    trigger: On<bevy::ecs::lifecycle::Insert, CodeEditor>,
    mut editors: Query<&mut bevy_instanced_text_editor::ScrollConfig>,
) {
    let Ok(mut cfg) = editors.get_mut(trigger.event().entity) else {
        return;
    };
    if cfg.scroll_beyond_last_line {
        cfg.scroll_beyond_last_line = false;
    }
}

fn sync_indent_config_on_change(
    trigger: On<bevy::ecs::lifecycle::Insert, crate::settings::Indentation>,
    mut editors: Query<
        (
            &crate::settings::Indentation,
            &mut bevy_instanced_text_editor::IndentConfig,
        ),
        With<CodeEditor>,
    >,
) {
    let Ok((indent, mut cfg)) = editors.get_mut(trigger.event().entity) else {
        return;
    };
    let next_tab = indent.indent_size.resolve(indent.tab_size);
    if cfg.tab_width != next_tab {
        cfg.tab_width = next_tab;
    }
    if cfg.use_spaces != indent.insert_spaces {
        cfg.use_spaces = indent.insert_spaces;
    }
    if cfg.use_tab_stops != indent.use_tab_stops {
        cfg.use_tab_stops = indent.use_tab_stops;
    }
    if cfg.sticky_tab_stops != indent.sticky_tab_stops {
        cfg.sticky_tab_stops = indent.sticky_tab_stops;
    }
    if cfg.trim_whitespace_on_delete != indent.trim_whitespace_on_delete {
        cfg.trim_whitespace_on_delete = indent.trim_whitespace_on_delete;
    }
}

#[derive(Component, Default)]
struct DetectIndentationDone;

fn detect_indentation_on_buffer_insert(
    trigger: On<
        bevy::ecs::lifecycle::Insert,
        bevy_instanced_text::TextBuffer<bevy_instanced_text_editor::RopeBuffer>,
    >,
    commands: Commands,
    editors: Query<
        (
            &bevy_instanced_text::TextBuffer<bevy_instanced_text_editor::RopeBuffer>,
            &mut crate::settings::Indentation,
        ),
        (With<CodeEditor>, Without<DetectIndentationDone>),
    >,
) {
    run_detect_indentation(trigger.event().entity, commands, editors);
}

fn detect_indentation_on_first_edit(
    trigger: On<bevy_instanced_text_editor::OnEdit>,
    commands: Commands,
    editors: Query<
        (
            &bevy_instanced_text::TextBuffer<bevy_instanced_text_editor::RopeBuffer>,
            &mut crate::settings::Indentation,
        ),
        (With<CodeEditor>, Without<DetectIndentationDone>),
    >,
) {
    run_detect_indentation(trigger.event().entity, commands, editors);
}

fn run_detect_indentation(
    entity: Entity,
    mut commands: Commands,
    mut editors: Query<
        (
            &bevy_instanced_text::TextBuffer<bevy_instanced_text_editor::RopeBuffer>,
            &mut crate::settings::Indentation,
        ),
        (With<CodeEditor>, Without<DetectIndentationDone>),
    >,
) {
    let Ok((buffer, mut indent)) = editors.get_mut(entity) else {
        return;
    };
    if buffer.len_chars() == 0 {
        return;
    }
    commands.entity(entity).insert(DetectIndentationDone);
    if !indent.detect_indentation {
        return;
    }

    let mut tab_count = 0usize;
    let mut space_widths: std::collections::HashMap<u32, usize> = Default::default();
    let line_count = buffer.len_lines().min(100);
    for line_idx in 0..line_count {
        let line = buffer.line(line_idx);
        let mut spaces = 0u32;
        let mut starts_with_tab = false;
        for c in line.chars() {
            match c {
                '\t' => {
                    starts_with_tab = true;
                    break;
                }
                ' ' => spaces += 1,
                _ => break,
            }
        }
        if starts_with_tab {
            tab_count += 1;
        } else if spaces > 0 {
            *space_widths.entry(spaces).or_insert(0) += 1;
        }
    }

    let total_space_runs: usize = space_widths.values().copied().sum();
    if tab_count > total_space_runs {
        indent.insert_spaces = false;
    } else if total_space_runs > 0 {
        indent.insert_spaces = true;
        let common = [2u32, 4, 8]
            .into_iter()
            .max_by_key(|w| {
                space_widths
                    .iter()
                    .filter(|(s, _)| *s % w == 0)
                    .map(|(_, c)| *c)
                    .sum::<usize>()
            })
            .unwrap_or(4);
        indent.tab_size = common;
    }
}

fn sync_automatic_layout(
    mut commands: Commands,
    editors: Query<
        (Entity, &crate::settings::Misc, Has<AutoResizeViewport>),
        (With<CodeEditor>, Changed<crate::settings::Misc>),
    >,
) {
    for (entity, misc, has_marker) in editors.iter() {
        match (misc.automatic_layout, has_marker) {
            (true, false) => {
                commands.entity(entity).insert(AutoResizeViewport);
            }
            (false, true) => {
                commands.entity(entity).remove::<AutoResizeViewport>();
            }
            _ => {}
        }
    }
}

fn sync_cursor_icon(
    mut commands: Commands,
    input_focus: Res<bevy::input_focus::InputFocus>,
    editors: Query<
        (
            &crate::settings::Misc,
            &crate::types::HoveredInGutter,
            &crate::plugin::HoveredLink,
        ),
        With<CodeEditor>,
    >,
    windows: Query<(Entity, Option<&bevy::window::CursorIcon>), With<bevy::window::PrimaryWindow>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let Some(entity) = input_focus.get() else {
        return;
    };
    let Ok((misc, in_gutter, hovered_link)) = editors.get(entity) else {
        return;
    };
    let Ok((window_entity, current)) = windows.single() else {
        return;
    };
    let ctrl_held = keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight)
        || keyboard.pressed(KeyCode::SuperLeft)
        || keyboard.pressed(KeyCode::SuperRight);
    let target = if in_gutter.0 {
        bevy::window::SystemCursorIcon::Default
    } else if misc.links && hovered_link.0.is_some() && ctrl_held {
        bevy::window::SystemCursorIcon::Pointer
    } else {
        match misc.mouse_style {
            crate::settings::MouseStyle::Text => bevy::window::SystemCursorIcon::Text,
            crate::settings::MouseStyle::Default => bevy::window::SystemCursorIcon::Default,
            crate::settings::MouseStyle::Copy => bevy::window::SystemCursorIcon::Copy,
        }
    };
    let next = bevy::window::CursorIcon::System(target);
    if current.map(|c| *c != next).unwrap_or(true) {
        commands.entity(window_entity).insert(next);
    }
}

/// Resolve gutter column widths and left-edges in one left-to-right pass.
/// Layout: `[ pad_l | glyph | numbers | decorations(chevron|bar) | pad_r ]`.
fn resolve_gutter_layout(
    mut editors: Query<
        (
            &mut GutterConfig,
            &MonoCellWidth,
            &TextFont,
            &bevy::text::LineHeight,
            &EditorUi,
            &crate::settings::Folding,
            &bevy_instanced_text::TextBuffer<bevy_instanced_text_editor::RopeBuffer>,
        ),
        With<CodeEditor>,
    >,
) {
    for (mut cfg, mono, font, line_height, ui, folding, buffer) in editors.iter_mut() {
        let show_numbers = !matches!(ui.line_numbers, crate::settings::LineNumbers::Off);
        let folding_enabled = matches!(
            folding.show_controls,
            crate::settings::ShowFoldingControls::Always
                | crate::settings::ShowFoldingControls::Mouseover
        );
        let line_count = buffer.len_lines().max(1) as u32;
        let digit_count = digit_count_for(line_count);
        let min_chars = digit_count.max(ui.line_numbers_min_chars).max(1) as f32;
        let line_height_px = bevy_instanced_text::resolve_line_height(*line_height, font.font_size);

        // 1) Widths — each band independent, then a chevron-bearing
        //    decorations band combining the chevron sub-column and the
        //    user-configured bar width.
        let glyph_w = if ui.glyph_margin {
            // Monaco sizes the glyph margin to line-height; `ui.glyph_margin_width`
            // is a floor so small text never squashes icons below readability.
            line_height_px.max(ui.glyph_margin_width)
        } else {
            0.0
        };
        let numbers_w = if show_numbers {
            mono.px * min_chars
        } else {
            0.0
        };
        // Chevron column scales with line-height (Monaco does the same)
        // with a 16 px floor for tiny font sizes.
        let chevron_w = if folding_enabled {
            line_height_px.max(16.0)
        } else {
            0.0
        };
        let bar_w = ui.line_decorations_width.max(0.0);
        let decorations_w = chevron_w + bar_w;
        let bands_w = glyph_w + numbers_w + decorations_w;
        let gutter_width = if bands_w > 0.0 {
            ui.gutter_padding_left + ui.gutter_padding_right + bands_w
        } else {
            0.0
        };

        // 2) Left chain — single pass, every band starts where the
        //    previous ended.
        let mut x = ui.gutter_padding_left;
        let glyph = GutterBand {
            left: x,
            width: glyph_w,
        };
        x += glyph_w;
        let numbers = GutterBand {
            left: x,
            width: numbers_w,
        };
        x += numbers_w;
        let decorations = GutterBand {
            left: x,
            width: decorations_w,
        };
        let chevron = GutterBand {
            left: x,
            width: chevron_w,
        };
        x += chevron_w;
        let bar = GutterBand {
            left: x,
            width: bar_w,
        };

        let next = GutterConfig {
            gutter_width,
            editor_padding_left: gutter_width + ui.code_margin_left,
            line_height_px,
            glyph,
            numbers,
            decorations,
            chevron,
            bar,
        };
        // PartialEq is exact, but every input is rounded or clamped
        // before arriving here (font-size px, mono.px which is itself
        // measured + dirty-thresholded). Avoids the change-detection
        // flicker of the old per-field epsilon comparison.
        if *cfg != next {
            *cfg = next;
        }
    }
}

fn apply_editor_padding(
    mut editors: Query<
        (&mut Node, &GutterConfig, &crate::settings::Padding),
        (
            With<CodeEditor>,
            Or<(Changed<GutterConfig>, Changed<crate::settings::Padding>)>,
        ),
    >,
) {
    for (mut node, cfg, padding) in editors.iter_mut() {
        let l = Val::Px(cfg.editor_padding_left);
        let t = Val::Px(padding.top);
        let b = Val::Px(padding.bottom);
        if node.padding.left != l {
            node.padding.left = l;
        }
        if node.padding.top != t {
            node.padding.top = t;
        }
        if node.padding.bottom != b {
            node.padding.bottom = b;
        }
    }
}

fn setup_editor_ui(
    mut commands: Commands,
    editor_query: Query<
        (
            &ComputedNode,
            &GutterConfig,
            &EditorTheme,
            &EditorUi,
            Option<&bevy::camera::visibility::RenderLayers>,
        ),
        With<CodeEditor>,
    >,
) {
    for (computed, gutter, theme, ui, render_layers) in editor_query.iter() {
        let inv = computed.inverse_scale_factor();
        let logical = computed.size() * inv;
        let viewport_width = logical.x;
        let viewport_height = logical.y;

        if ui.show_separator {
            let mut cmds = commands.spawn((
                Sprite {
                    color: theme.separator,
                    custom_size: Some(Vec2::new(1.0, viewport_height)),
                    ..default()
                },
                Transform::from_translation(to_bevy_coords_left_aligned(
                    gutter.gutter_width,
                    viewport_height / 2.0,
                    viewport_width,
                    viewport_height,
                    0.0,
                )),
                Separator,
                Name::new("Separator"),
            ));
            if let Some(layers) = render_layers {
                cmds.insert(layers.clone());
            }
        }
    }
}

fn viewport_or_gutter_changed(
    query: Query<
        (),
        (
            With<CodeEditor>,
            Or<(Changed<ComputedNode>, Changed<GutterConfig>)>,
        ),
    >,
) -> bool {
    !query.is_empty()
}

fn update_separator_on_resize(
    viewport_query: Query<(&ComputedNode, &GutterConfig), With<CodeEditor>>,
    mut separator_query: Query<(&mut Sprite, &mut Transform), With<Separator>>,
) {
    let Some((computed, gutter)) = viewport_query.iter().next() else {
        return;
    };

    let inv = computed.inverse_scale_factor();
    let logical = computed.size() * inv;
    let viewport_width = logical.x;
    let viewport_height = logical.y;

    for (mut sprite, mut transform) in separator_query.iter_mut() {
        sprite.custom_size = Some(Vec2::new(1.0, viewport_height));
        transform.translation = to_bevy_coords_left_aligned(
            gutter.gutter_width,
            viewport_height / 2.0,
            viewport_width,
            viewport_height,
            0.0,
        );
    }
}

fn update_font_metrics(
    mut editors: Query<(&TextFont, &mut MonoCellWidth), With<CodeEditor>>,
    mut atlas: ResMut<GlyphAtlas>,
    fonts: Res<Assets<bevy::text::Font>>,
) {
    for (font, mut mono) in editors.iter_mut() {
        let font_id = atlas.ensure_font(&font.font, &fonts);
        let width = atlas.shape_line("0", font.font_size, font_id).width;
        if width > 0.0 && (mono.px - width).abs() > 0.01 {
            info!(
                "Updating char_width from {:.3} to {:.3} (measured)",
                mono.px, width
            );
            mono.px = width;
        }
    }
}

fn digit_count_for(n: u32) -> u32 {
    let mut digits = 1;
    let mut v = n;
    while v >= 10 {
        v /= 10;
        digits += 1;
    }
    digits
}
