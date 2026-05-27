//! Line-decoration bars: thin coloured rectangles in the gutter's
//! line-decoration strip (VCS added / modified indicators, severity
//! bars). Host-facing surface is [`GutterDecorations`]. Deleted-row
//! entries are filtered here and rendered as glyph-margin icons
//! instead (see `markers.rs`).
//!
//! Bars are *child UI Nodes* of the editor (`BackgroundColor`-tinted),
//! not GPU overlays — so Taffy clips them to the gutter region and
//! they cannot spill into the code area.

use bevy::prelude::*;
use bevy_instanced_text::RectOverlay;
use crate::types::{CodeEditor, GutterContainer};
use crate::ui_kit::GutterTokens;

use super::common::{diff_place, group_pools_by_editor, RowGeometry};

/// One bar in the line-decoration strip.
#[derive(Clone, Debug, Reflect)]
#[reflect(Debug)]
pub struct LineDecoration {
    pub line: usize,
    pub kind: DecorationKind,
    pub color: Color,
}

/// Categorisation for a [`LineDecoration`]. Bars render for
/// [`Added`](DecorationKind::Added), [`Modified`](DecorationKind::Modified),
/// [`DiagnosticBar`](DecorationKind::DiagnosticBar), and
/// [`Custom`](DecorationKind::Custom). [`Deleted`](DecorationKind::Deleted)
/// instead places a small triangle icon in the glyph margin at the
/// deletion row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum DecorationKind {
    Added,
    Modified,
    Deleted,
    DiagnosticBar,
    Custom,
}

/// Per-editor list of line-decoration bars.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct GutterDecorations(pub Vec<LineDecoration>);

/// Marker for a line-decoration bar child node.
#[derive(Component, Reflect, Clone, Copy)]
#[reflect(Component)]
pub struct GutterDecorationBar {
    pub editor: Entity,
    pub line: usize,
}

/// Stub Component preserved for the merge-overlay pipeline; no
/// longer populated (bars render as child Nodes now). Removable
/// once every merge-overlay consumer stops listing it.
#[derive(Component, Default, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct LineDecorationRects(pub Vec<RectOverlay>);

pub(crate) fn update_line_decoration_overlays(
    mut q: Query<&mut LineDecorationRects, With<CodeEditor>>,
) {
    for mut rects in q.iter_mut() {
        if !rects.0.is_empty() {
            rects.0.clear();
        }
    }
}

pub(crate) fn sync_gutter_decoration_bars(
    mut commands: Commands,
    tokens: Option<Res<GutterTokens>>,
    editors: Query<
        (
            Entity,
            &GutterDecorations,
            crate::settings::GutterLayoutView,
        ),
        With<CodeEditor>,
    >,
    mut existing: Query<(
        Entity,
        &GutterDecorationBar,
        &mut Node,
        &mut BackgroundColor,
        &mut Visibility,
    )>,
    containers: Query<(Entity, &GutterContainer)>,
) {
    let tokens = tokens.map(|t| t.clone()).unwrap_or_default();
    let mut by_editor = group_pools_by_editor(
        existing.iter().map(|(id, bar, ..)| (id, bar)),
        |bar: &GutterDecorationBar| bar.editor,
    );

    for (editor_entity, decorations, gl) in editors.iter() {
        if gl.gutter.bar.is_empty() {
            continue;
        }

        let bar_width: f32 = tokens.bar_width.min(gl.gutter.bar.width);
        // Bar centered inside its sub-column at the right edge of the
        // decorations band. Centering keeps the bar in place when the
        // chevron sub-column toggles on/off (folding controls visible).
        let bar_left = gl.gutter.bar.place_square(bar_width).round().max(0.0);
        let bar_radius = tokens.bar_radius.max(0.5);

        let active: Vec<&LineDecoration> = decorations
            .0
            .iter()
            .filter(|d| !matches!(d.kind, DecorationKind::Deleted))
            .collect();

        let pool = by_editor.entry(editor_entity).or_default();

        // Pool slot N corresponds to `active[N]` permanently across
        // frames. Hidden lines (collapsed by a fold) keep their slot
        // hidden in place rather than skipped, so the remaining slots
        // never need to swap colour or kind mid-frame. (Consistency
        // with `markers.rs` / `chevrons.rs` — keeps decoration
        // identity tied to the host's input ordering.)
        for (idx, dec) in active.iter().enumerate() {
            let geom = RowGeometry::compute(dec.line, gl.font, gl.line_height, gl.padding, gl.layout);
            let line = dec.line;
            let color = dec.color;

            if let Some(geom) = geom {
                let height_px = geom.line_height_px.round();
                let bar_color = color.with_alpha(color.alpha() * tokens.bar_alpha);
                if let Some(&entity) = pool.get(idx) {
                    if let Ok((_, _bar, mut node, mut bg, mut vis)) = existing.get_mut(entity) {
                        diff_place(&mut node, bar_left, geom.top_px, bar_width, height_px);
                        if bg.0 != bar_color {
                            bg.0 = bar_color;
                        }
                        if *vis != Visibility::Inherited {
                            *vis = Visibility::Inherited;
                        }
                        commands.entity(entity).insert(GutterDecorationBar {
                            editor: editor_entity,
                            line,
                        });
                    }
                } else {
                    let id = commands
                        .spawn((
                            GutterDecorationBar {
                                editor: editor_entity,
                                line,
                            },
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(bar_left),
                                top: Val::Px(geom.top_px),
                                width: Val::Px(bar_width),
                                height: Val::Px(height_px),
                                border_radius: BorderRadius::all(Val::Px(bar_radius)),
                                ..default()
                            },
                            BackgroundColor(bar_color),
                            bevy::picking::Pickable::IGNORE,
                            Name::new("GutterDecorationBar"),
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
            } else if let Some(&entity) = pool.get(idx) {
                if let Ok((_, _, _, _, mut vis)) = existing.get_mut(entity) {
                    if *vis != Visibility::Hidden {
                        *vis = Visibility::Hidden;
                    }
                }
            }
        }

        // Any pool entries past `active.len()` came from a previous
        // frame with more bars; hide them.
        for &entity in pool.iter().skip(active.len()) {
            if let Ok((_, _, _, _, mut vis)) = existing.get_mut(entity) {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}
