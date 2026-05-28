//! Fold-chevron SVG icons rendered in the gutter's chevron column.
//! A single `chevron-down` handle is used for every chevron; folded
//! chevrons rotate −90° around the icon centre over [`CHEVRON_ANIM_SECS`]
//! using a cubic-out easing curve. The set of visible chevrons depends
//! on `Folding::show_controls`:
//! - `Always`: one chevron per foldable region.
//! - `Mouseover`: only the hovered foldable line.
//! - `Never`: nothing rendered (width is 0).

use std::f32::consts::FRAC_PI_2;

use bevy::math::curve::{Curve, EaseFunction, EasingCurve};
use bevy::prelude::*;
use bevy::ui::UiTransform;
use bevy_resvg::prelude::*;

use crate::settings::EditorTheme;
use crate::types::{CodeEditor, FoldState, GutterContainer, HoveredGutterLine};
use crate::ui_kit::GutterTokens;

use super::common::{diff_place, group_pools_by_editor, RowGeometry};
use super::icons::IconAtlas;

/// Marker for a fold-chevron icon child node.
#[derive(Component, Reflect, Clone, Copy)]
#[reflect(Component)]
pub struct GutterFoldChevron {
    pub editor: Entity,
    pub line: usize,
}

/// Per-chevron rotation state. `target` flips between 0 (expanded,
/// pointing down) and −π/2 (folded, pointing right). The animator
/// eases from `start` to `target` over [`CHEVRON_ANIM_SECS`].
#[derive(Component, Default)]
pub(crate) struct ChevronRotation {
    /// Angle the current ease started at (captures mid-flight handoff).
    start: f32,
    /// Latest sampled angle — written every frame by the animator.
    current: f32,
    /// Goal angle. Setting this resets `elapsed` and re-anchors `start`.
    target: f32,
    elapsed: f32,
}

/// Animation length for the chevron fold toggle, in seconds.
const CHEVRON_ANIM_SECS: f32 = 0.12;

/// Resolve the per-frame list of `(buffer_line, is_folded)` chevrons
/// to render for an editor, given the user's `show_controls` choice
/// and the currently-hovered gutter line.
fn desired_chevrons(
    fold: &FoldState,
    show: crate::settings::ShowFoldingControls,
    hovered_line: Option<usize>,
) -> Vec<(usize, bool)> {
    use crate::settings::ShowFoldingControls::*;
    match show {
        Always => fold
            .regions
            .iter()
            .map(|r| (r.start_line, r.is_folded))
            .collect(),
        Mouseover => hovered_line
            .filter(|&line| fold.is_foldable_line(line))
            .map(|line| vec![(line, fold.is_folded_line(line))])
            .unwrap_or_default(),
        Never => Vec::new(),
    }
}

pub(crate) fn sync_fold_chevron_icons(
    mut commands: Commands,
    atlas: Option<Res<IconAtlas>>,
    tokens: Option<Res<GutterTokens>>,
    editors: Query<
        (
            Entity,
            &FoldState,
            crate::settings::GutterLayoutView,
            &crate::settings::Folding,
            &EditorTheme,
            &HoveredGutterLine,
        ),
        With<CodeEditor>,
    >,
    mut existing: Query<(
        Entity,
        &GutterFoldChevron,
        &mut Node,
        &mut SvgColor,
        &mut ChevronRotation,
        &mut Visibility,
    )>,
    containers: Query<(Entity, &GutterContainer)>,
) {
    let Some(atlas) = atlas else {
        return;
    };
    let tokens = tokens.map(|t| t.clone()).unwrap_or_default();

    let mut by_editor = group_pools_by_editor(
        existing.iter().map(|(id, ch, ..)| (id, ch)),
        |ch: &GutterFoldChevron| ch.editor,
    );

    for (editor_entity, fold, gl, folding, theme, hovered) in editors.iter() {
        if gl.gutter.chevron.is_empty() {
            continue;
        }
        let desired = desired_chevrons(fold, folding.show_controls, hovered.0);

        let color = theme.line_numbers;
        let pool = by_editor.entry(editor_entity).or_default();

        for (idx, (line, folded)) in desired.iter().enumerate() {
            let geom = RowGeometry::compute(*line, gl.font, gl.line_height, gl.padding, gl.layout);
            let line = *line;
            let target_angle = if *folded { -FRAC_PI_2 } else { 0.0 };

            let Some(geom) = geom else {
                if let Some(&entity) = pool.get(idx) {
                    if let Ok((_, _, _, _, _, mut vis)) = existing.get_mut(entity) {
                        if *vis != Visibility::Hidden {
                            *vis = Visibility::Hidden;
                        }
                    }
                }
                continue;
            };

            let icon_size = (gl.gutter.chevron.width.min(geom.line_height_px) * tokens.chevron_scale)
                .round()
                .max(8.0);
            let optical_lift = (geom.line_height_px * 0.05).round();
            let icon_left = gl.gutter.chevron.place_square(icon_size).round();
            // Bias the icon slightly above geometric centre so it
            // tracks the digits' optical centre (which sits above
            // the row's mid-line because of the descender).
            let icon_top =
                (geom.top_px + (geom.line_height_px - icon_size) * 0.5 - optical_lift).round();

            if let Some(&entity) = pool.get(idx) {
                if let Ok((_, _ch, mut node, mut svg_color, mut rot, mut vis)) =
                    existing.get_mut(entity)
                {
                    diff_place(&mut node, icon_left, icon_top, icon_size, icon_size);
                    if svg_color.0 != color {
                        svg_color.0 = color;
                    }
                    if (rot.target - target_angle).abs() > f32::EPSILON {
                        rot.start = rot.current;
                        rot.target = target_angle;
                        rot.elapsed = 0.0;
                    }
                    if *vis != Visibility::Inherited {
                        *vis = Visibility::Inherited;
                    }
                    commands.entity(entity).insert(GutterFoldChevron {
                        editor: editor_entity,
                        line,
                    });
                }
            } else {
                let id = commands
                    .spawn((
                        GutterFoldChevron {
                            editor: editor_entity,
                            line,
                        },
                        UiSvg(atlas.chevron_down.clone()),
                        SvgColor(color),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(icon_left),
                            top: Val::Px(icon_top),
                            width: Val::Px(icon_size),
                            height: Val::Px(icon_size),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        ChevronRotation {
                            start: target_angle,
                            current: target_angle,
                            target: target_angle,
                            elapsed: CHEVRON_ANIM_SECS,
                        },
                        UiTransform::from_rotation(Rot2::radians(target_angle)),
                        (
                            bevy::picking::Pickable::IGNORE,
                            Name::new("GutterFoldChevron"),
                        ),
                    ))
                    .id();
                if let Some(parent) = containers
                    .iter()
                    .find_map(|(eid, c)| (c.editor == editor_entity).then_some(eid))
                {
                    commands.entity(parent).add_child(id);
                }
                pool.push(id);
            }
        }

        // Any pool entries past `desired.len()` came from a previous
        // frame with more chevrons (e.g. user just collapsed a region
        // making one disappear); hide them.
        for &entity in pool.iter().skip(desired.len()) {
            if let Ok((_, _, _, _, _, mut vis)) = existing.get_mut(entity) {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

/// Per-frame: advance each chevron's `current` toward `target` along a
/// cubic-out curve over [`CHEVRON_ANIM_SECS`], writing the result as a
/// [`UiTransform`] rotation. Rotation pivots around the node centre
/// because the UI layout pass applies the affine before the
/// node-centre offset.
pub(crate) fn drive_chevron_rotation(
    time: Res<Time>,
    mut q: Query<(&mut ChevronRotation, &mut UiTransform)>,
) {
    let dt = time.delta_secs();
    for (mut rot, mut transform) in q.iter_mut() {
        if rot.elapsed >= CHEVRON_ANIM_SECS {
            continue;
        }
        rot.elapsed = (rot.elapsed + dt).min(CHEVRON_ANIM_SECS);
        let t = (rot.elapsed / CHEVRON_ANIM_SECS).clamp(0.0, 1.0);
        let eased =
            EasingCurve::new(rot.start, rot.target, EaseFunction::CubicOut).sample_clamped(t);
        rot.current = eased;
        transform.rotation = Rot2::radians(eased);
    }
}

#[cfg(test)]
mod tests {
    //! Headless tests for chevron placement. We don't invoke the full
    //! `sync_fold_chevron_icons` system (it needs `IconAtlas` + the
    //! resvg pipeline) — instead we exercise `desired_chevrons` and
    //! the per-row math directly. That's where the bug would live if
    //! a chevron's reported line disagrees with its painted y.
    use super::*;
    use crate::types::fold::{FoldKind, FoldRegion};

    fn region(start: usize, end: usize, kind: FoldKind) -> FoldRegion {
        FoldRegion {
            start_line: start,
            end_line: end,
            is_folded: false,
            kind,
            indent_level: 0,
        }
    }

    /// The fold regions tree-sitter reported for `editor_syntax.rs`
    /// at the moment the user observed the line-16 → digit-14 drift.
    /// Order matches the logged output (parse-tree traversal order,
    /// **not** sorted by start_line).
    fn editor_syntax_regions() -> FoldState {
        FoldState {
            regions: vec![
                region(7, 11, FoldKind::Class),     // struct Person
                region(5, 6, FoldKind::Comment),    // comments before `use`
                region(13, 32, FoldKind::Class),    // impl Person
                region(15, 21, FoldKind::Function), // pub fn new
                region(14, 15, FoldKind::Comment),  // /// Create a new person
                region(16, 20, FoldKind::Other),    // Self { ... }
                region(24, 26, FoldKind::Function), // add_tag
                region(23, 24, FoldKind::Comment),
                region(29, 31, FoldKind::Function), // is_adult
                region(28, 29, FoldKind::Comment),
                region(34, 62, FoldKind::Function), // fn main
                region(47, 49, FoldKind::Block),
                region(47, 49, FoldKind::Block),
                region(52, 56, FoldKind::Block),
            ],
            content_version: 0,
        }
    }

    /// Compute the painted y (inside the gutter container) for each
    /// chevron the renderer would emit, given a `FoldState`.
    fn placements(fold: &FoldState, line_height_px: f32) -> Vec<(usize, f32)> {
        // Build the same desired list `sync_fold_chevron_icons` does in
        // Always mode. Math mirrors `RowGeometry::compute` without
        // pulling in TextFont / LineHeight Bevy types.
        let mut out = Vec::new();
        for r in &fold.regions {
            if fold.is_line_hidden(r.start_line) {
                continue;
            }
            let display_row = fold.actual_to_display_line(r.start_line) as f32;
            let top_px = (display_row * line_height_px - 0.0).round();
            out.push((r.start_line, top_px));
        }
        out
    }

    /// With no folds applied, every chevron should land at the y of
    /// its own buffer line — `actual_to_display_line` is the identity.
    #[test]
    fn chevron_y_equals_start_line_when_unfolded() {
        let fold = editor_syntax_regions();
        let lh = 21.0_f32;
        let placed = placements(&fold, lh);

        // The chevron for `fn new` (region (15, 21)) should land at y=15*21=315,
        // which is display row 15 (= displayed digit "16").
        let fn_new = placed
            .iter()
            .find(|(line, _)| *line == 15)
            .expect("chevron for fn new (start_line=15) should be present");
        assert!(
            (fn_new.1 - 15.0 * lh).abs() < 0.5,
            "fn new chevron should land at y=315 (display row 15); got y={}",
            fn_new.1,
        );

        // Spot-check every region: each chevron at start_line N should
        // be drawn at y = N * line_height (no folds → display_row = N).
        for (line, y) in &placed {
            let expected = (*line as f32) * lh;
            assert!(
                (y - expected).abs() < 0.5,
                "chevron(start_line={line}) at y={y}, expected y={expected}",
            );
        }
    }

    /// The user reports clicking a chevron at *digit 14* folds `fn new`
    /// (whose `start_line` is 15 → digit 16). If the math is right,
    /// this test fails to reproduce — meaning the bug isn't in
    /// `desired_chevrons` / `RowGeometry`, and we should look elsewhere
    /// (e.g. parent layout or `ComputedNode` resolution).
    #[test]
    fn fn_new_chevron_does_not_land_at_digit_14() {
        let fold = editor_syntax_regions();
        let lh = 21.0_f32;
        let placed = placements(&fold, lh);

        let fn_new = placed
            .iter()
            .find(|(line, _)| *line == 15)
            .expect("fn new chevron present");
        let digit_14_y = 13.0 * lh; // 0-indexed display row 13 = displayed digit "14"

        assert!(
            (fn_new.1 - digit_14_y).abs() > lh * 0.5,
            "fn new chevron lands at y={} which IS digit 14's row (y={}). \
             If this assertion fails, the bug IS in chevron row math.",
            fn_new.1,
            digit_14_y,
        );
    }

    /// `desired_chevrons` iterates `regions` in vec order (parse-tree
    /// order), not sorted by `start_line`. If anything downstream
    /// assumes sorted-by-line order, this is where it'd bite.
    #[test]
    fn desired_chevrons_preserves_parse_tree_order() {
        let fold = editor_syntax_regions();
        let desired = desired_chevrons(&fold, crate::settings::ShowFoldingControls::Always, None);

        let lines: Vec<usize> = desired.iter().map(|(l, _)| *l).collect();
        // Reflect the logged order exactly:
        assert_eq!(
            lines,
            vec![7, 5, 13, 15, 14, 16, 24, 23, 29, 28, 34, 47, 47, 52]
        );
    }

    /// Sanity: with `fn new` folded, lines 16..=20 are hidden, so the
    /// `Self { ... }` chevron at start_line=16 must NOT render (it's
    /// inside the folded function).
    #[test]
    fn folding_fn_new_hides_nested_chevrons() {
        let mut fold = editor_syntax_regions();
        // Fold the `fn new` region (15, 21).
        fold.regions
            .iter_mut()
            .find(|r| r.start_line == 15 && r.end_line == 21)
            .unwrap()
            .is_folded = true;

        let lh = 21.0_f32;
        let placed = placements(&fold, lh);

        // Inside-fold chevrons must be dropped.
        assert!(
            !placed.iter().any(|(l, _)| *l == 16),
            "chevron at start_line=16 (Self block) should be hidden inside the folded fn new",
        );

        // The fn new chevron itself stays visible at its own row.
        let fn_new = placed
            .iter()
            .find(|(l, _)| *l == 15)
            .expect("fn new chevron stays visible");
        assert!(
            (fn_new.1 - 15.0 * lh).abs() < 0.5,
            "fn new chevron y should still be 15*lh after folding self; got {}",
            fn_new.1,
        );

        // Regions starting *after* the folded range collapse up by
        // (21 - 15) = 6 rows: e.g. `add_tag` at start_line=24 → display row 18.
        let add_tag = placed
            .iter()
            .find(|(l, _)| *l == 24)
            .expect("add_tag chevron still present");
        let expected_y = 18.0 * lh;
        assert!(
            (add_tag.1 - expected_y).abs() < 0.5,
            "add_tag chevron y should collapse to row 18 (y={expected_y}); got y={}",
            add_tag.1,
        );
    }

    /// And finally: when we drop comment folds entirely (option (b)
    /// from earlier), the line-15 chevron is the `fn new` one — and
    /// clicking at digit 16's y should resolve to buffer line 15.
    /// This documents the "no comment folds" behavior so the next
    /// reader can see what changing it would do.
    #[test]
    fn dropping_comment_folds_leaves_fn_new_at_start_line_15() {
        let mut fold = editor_syntax_regions();
        fold.regions.retain(|r| r.kind != FoldKind::Comment);

        let chevrons_on_line_15: Vec<_> =
            fold.regions.iter().filter(|r| r.start_line == 15).collect();
        assert_eq!(
            chevrons_on_line_15.len(),
            1,
            "after dropping comment folds, exactly one chevron at start_line=15",
        );
        assert_eq!(chevrons_on_line_15[0].kind, FoldKind::Function);
    }
}
