//! Gutter line-number rendering via a child `TextView` entity.
//!
//! Each `CodeEditor` gets a [`GutterContainer`] child Node sized to the
//! computed `gutter_width`, and the line-number `TextView` plus every
//! decoration child (icons, bars, chevrons) live underneath that
//! container. Taffy clips the container so children can never spill
//! into the code area, even when their absolute coordinates briefly
//! disagree with the band math.
//!
//! Vertical alignment: the container is offset from the editor's top
//! by `Padding::top` so that its content-box origin (y=0 in container
//! local space) lines up with text row 0 in the code area — the
//! renderer reads the same `padding.top` via `content_inset()`. With
//! that shared anchor, gutter decorations at `top: row * line_height
//! - scroll.y` land on the exact y as text row `row`, without any
//! per-decoration `text_top` term.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::text::{Justify, TextLayout};
use bevy::ui::ScrollPosition;
use bevy_instanced_text::{
    FormattedSpan, HiddenLines, LineStyles, MonoFontFaces, TextBuffer, TextFormat, TextSpan,
};
use bevy_instanced_text_editor::RopeBuffer;

use crate::settings::*;
use crate::types::*;

/// Spawn a [`GutterContainer`] (and its child `GutterTextView`) for
/// each new `CodeEditor`. Idempotent — re-running on an editor that
/// already has a container is a no-op.
pub(crate) fn setup_gutter_text_view(
    mut commands: Commands,
    editors: Query<
        (
            Entity,
            &TextFont,
            &MonoFontFaces,
            &EditorTheme,
            &bevy::text::LineHeight,
            Option<&bevy::camera::visibility::RenderLayers>,
        ),
        With<CodeEditor>,
    >,
    existing_containers: Query<&GutterContainer>,
) {
    for (editor_entity, font, faces, theme, line_height, render_layers) in editors.iter() {
        if existing_containers
            .iter()
            .any(|g| g.editor == editor_entity)
        {
            continue;
        }

        let mut container_cmds = commands.spawn((
            GutterContainer {
                editor: editor_entity,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(0.0),
                overflow: Overflow::clip(),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
            Name::new("GutterContainer"),
        ));
        if let Some(layers) = render_layers {
            container_cmds.insert(layers.clone());
        }
        let container_id = container_cmds.id();
        commands.entity(editor_entity).add_child(container_id);

        let mut gutter_cmds = commands.spawn((
            GutterTextView {
                editor: editor_entity,
            },
            TextBuffer::<TextSpan>::default(),
            font.clone(),
            faces.clone(),
            *line_height,
            TextLayout {
                justify: Justify::Right,
                ..default()
            },
            bevy_instanced_text::TextColor(theme.line_numbers),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
            Name::new("GutterTextView"),
        ));
        if let Some(layers) = render_layers {
            gutter_cmds.insert(layers.clone());
        }
        let gutter_id = gutter_cmds.id();
        commands.entity(container_id).add_child(gutter_id);
    }
}

/// Keeps each gutter `TextView` in sync with its editor.
///
/// Runs in PostUpdate before `LayoutProduceSet`.
pub(crate) fn sync_gutter_text_view(
    editor_query: Query<
        (
            Entity,
            &SelectionState,
            &TextBuffer<RopeBuffer>,
            &ScrollPosition,
            &GutterConfig,
            Ref<FoldState>,
            &EditorTheme,
            &EditorUi,
            &crate::settings::RenderSettings,
            &crate::settings::Folding,
            Ref<HoveredGutterLine>,
        ),
        (With<CodeEditor>, Without<GutterTextView>),
    >,
    mut gutter_query: Query<
        (
            &GutterTextView,
            &mut TextBuffer<TextSpan>,
            &mut ScrollPosition,
            &mut HiddenLines,
            &mut LineStyles,
            &mut Node,
            &mut Visibility,
            &mut bevy_instanced_text::TextColor,
        ),
        Without<CodeEditor>,
    >,
) {
    use crate::settings::LineNumbers as LineNumbersMode;
    use crate::settings::RenderFinalNewline;
    use crate::settings::ShowFoldingControls;
    for (
        editor_entity,
        sel,
        buffer,
        editor_scroll,
        gutter,
        fold_state,
        theme,
        ui,
        render,
        folding,
        hovered,
    ) in editor_query.iter()
    {
        let Some((
            _,
            mut g_buffer,
            mut g_scroll,
            mut g_hidden,
            mut g_styles,
            mut g_node,
            mut g_vis,
            mut g_color,
        )) = gutter_query
            .iter_mut()
            .find(|(g, ..)| g.editor == editor_entity)
        else {
            continue;
        };

        let show_numbers = !matches!(ui.line_numbers, crate::settings::LineNumbers::Off);
        let target_vis = if show_numbers {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *g_vis != target_vis {
            *g_vis = target_vis;
        }

        if !show_numbers {
            continue;
        }

        let zero = Val::Px(0.0);
        if g_node.padding.top != zero {
            g_node.padding.top = zero;
        }

        let target_left = Val::Px(gutter.numbers.left);
        if g_node.padding.left != target_left {
            g_node.padding.left = target_left;
        }
        let target_right = Val::Px(gutter.gutter_width - gutter.numbers.right());
        if g_node.padding.right != target_right {
            g_node.padding.right = target_right;
        }

        let default_color = bevy_instanced_text::TextColor(theme.line_numbers);
        if g_color.0 != default_color.0 {
            *g_color = default_color;
        }

        if (g_scroll.y - editor_scroll.y).abs() > 1e-4 {
            g_scroll.y = editor_scroll.y;
        }

        let raw_line_count = buffer.len_lines();
        let trailing_empty = raw_line_count > 0
            && bevy_instanced_text::TextContent::line(&**buffer, raw_line_count - 1)
                .trim()
                .is_empty();
        let strip_trailing =
            trailing_empty && matches!(render.render_final_newline, RenderFinalNewline::Off);
        let line_count = if strip_trailing {
            raw_line_count.saturating_sub(1)
        } else {
            raw_line_count
        }
        .max(1);

        let cursor_line = sel
            .selections
            .primary()
            .head_offset()
            .min(buffer.len_chars());
        let cursor_line_idx = buffer.char_to_line(cursor_line);

        let mode = ui.line_numbers;
        let mouseover_chevrons = matches!(folding.show_controls, ShowFoldingControls::Mouseover);
        let always_chevrons = matches!(folding.show_controls, ShowFoldingControls::Always);
        let old_count = if g_buffer.0 .0.is_empty() {
            0
        } else {
            bevy_instanced_text::TextContent::line_count(&g_buffer.0)
        };
        let count_stale = old_count != line_count;
        let needs_full_rebuild = count_stale
            || matches!(mode, LineNumbersMode::Relative)
            || (always_chevrons && fold_state.is_changed())
            || (mouseover_chevrons && (hovered.is_changed() || fold_state.is_changed()));

        if needs_full_rebuild {
            let mut text = String::with_capacity(line_count * 6);
            for i in 0..line_count {
                if i > 0 {
                    text.push('\n');
                }
                let label = match mode {
                    LineNumbersMode::Relative => {
                        if i == cursor_line_idx {
                            (i + 1).to_string()
                        } else {
                            (i as isize - cursor_line_idx as isize)
                                .unsigned_abs()
                                .to_string()
                        }
                    }
                    LineNumbersMode::Interval => {
                        let n = i + 1;
                        if n % 10 == 0 || i == cursor_line_idx {
                            n.to_string()
                        } else {
                            String::new()
                        }
                    }
                    _ => (i + 1).to_string(),
                };
                text.push_str(&label);
            }
            g_buffer.0 = TextSpan(text);
        }

        if fold_state.is_changed() || count_stale {
            let mut hidden: HashSet<usize> = HashSet::new();
            for region in &fold_state.regions {
                if !region.is_folded {
                    continue;
                }
                let start = region.start_line.saturating_add(1);
                let end = region.end_line.min(line_count.saturating_sub(1));
                for line in start..=end {
                    hidden.insert(line);
                }
            }
            *g_hidden = HiddenLines::new(hidden);
        }

        let cursor_lines: HashSet<usize> = sel
            .selections
            .iter()
            .map(|s| {
                let pos = s.head_offset().min(buffer.len_chars());
                buffer.char_to_line(pos)
            })
            .collect();

        let current_active: HashSet<usize> = g_styles.by_line.keys().map(|&k| k as usize).collect();

        if cursor_lines != current_active
            || count_stale
            || (always_chevrons && fold_state.is_changed())
            || (mouseover_chevrons && (hovered.is_changed() || fold_state.is_changed()))
        {
            let active_color = theme.line_numbers_active;
            let mut by_line: HashMap<u32, Vec<FormattedSpan>> = HashMap::new();
            for &line in &cursor_lines {
                if line < line_count {
                    let payload = (line + 1).to_string();
                    let byte_len = payload.len();
                    by_line.insert(
                        line as u32,
                        vec![FormattedSpan {
                            text: payload,
                            format: TextFormat::fg(0..byte_len, active_color),
                            is_virtual: false,
                        }],
                    );
                }
            }
            *g_styles = LineStyles::new(by_line);
        }
    }
}

/// Mirror the editor's [`bevy::text::LineHeight`] / [`TextFont`] onto
/// the [`GutterTextView`]. Without this, the gutter inherits the
/// renderer's default `LineHeight` and digits drift relative to
/// chevrons / decorations — those decoration systems read the
/// editor's `LineHeight` directly, so a mismatch causes a per-row
/// stride divergence that compounds with line number.
pub(crate) fn sync_gutter_text_font(
    editors: Query<(&TextFont, &bevy::text::LineHeight), With<CodeEditor>>,
    mut gutter: Query<
        (&GutterTextView, &mut TextFont, &mut bevy::text::LineHeight),
        Without<CodeEditor>,
    >,
) {
    for (view, mut g_font, mut g_lh) in gutter.iter_mut() {
        let Ok((font, lh)) = editors.get(view.editor) else {
            continue;
        };
        if g_font.font_size != font.font_size || g_font.font != font.font {
            *g_font = font.clone();
        }
        if *g_lh != *lh {
            *g_lh = *lh;
        }
    }
}

/// Track the [`GutterContainer`]'s `Node::width` to the resolved
/// `GutterConfig::gutter_width` for its editor, and its `Node::top`
/// to the editor's `Padding::top` — the single source of truth for
/// the row-0 anchor shared with the code text view (see module docs).
/// Runs each frame in PostUpdate before line-number layout so the
/// container is sized + placed before its TextView child paints.
pub(crate) fn sync_gutter_container(
    editors: Query<(&GutterConfig, &crate::settings::Padding), With<CodeEditor>>,
    mut containers: Query<(&GutterContainer, &mut Node)>,
) {
    for (container, mut node) in containers.iter_mut() {
        let Ok((gutter, padding)) = editors.get(container.editor) else {
            continue;
        };
        let target_w = Val::Px(gutter.gutter_width);
        if node.width != target_w {
            node.width = target_w;
        }
        let target_top = Val::Px(padding.top);
        if node.top != target_top {
            node.top = target_top;
        }
    }
}

#[cfg(test)]
mod tests {
    //! Regression tests for the gutter / editor `LineHeight` mirror.
    //!
    //! Without the mirror, the `GutterTextView` falls back to the
    //! renderer's default `LineHeight` (typically
    //! `RelativeToFont(1.2)`), while the chevrons / decorations read
    //! the editor's `LineHeight` directly. The two diverge per-row,
    //! producing a compounding offset that looks like ~2 line-heights
    //! of drift around buffer line 13 for 14 px fonts.
    use super::*;
    use bevy::text::LineHeight;

    fn spawn_minimal_editor(app: &mut App, line_height: LineHeight) -> Entity {
        // Spawn only the Components `setup_gutter_text_view` reads —
        // the test does not exercise the full editor cascade.
        app.world_mut()
            .spawn((
                CodeEditor,
                TextFont::from_font_size(14.0),
                MonoFontFaces::default(),
                EditorTheme::default(),
                line_height,
            ))
            .id()
    }

    fn find_gutter_view(app: &mut App, editor: Entity) -> Entity {
        let mut q = app.world_mut().query::<(Entity, &GutterTextView)>();
        q.iter(app.world())
            .find(|(_, gv)| gv.editor == editor)
            .map(|(e, _)| e)
            .expect("GutterTextView spawned")
    }

    #[test]
    fn setup_clones_editor_line_height_onto_gutter_view() {
        let mut app = App::new();
        app.add_systems(Update, setup_gutter_text_view);

        let editor = spawn_minimal_editor(&mut app, LineHeight::Px(21.0));
        app.update();

        let view = find_gutter_view(&mut app, editor);
        let lh = *app
            .world()
            .entity(view)
            .get::<LineHeight>()
            .expect("GutterTextView should carry LineHeight cloned from the editor");
        assert_eq!(
            lh,
            LineHeight::Px(21.0),
            "GutterTextView LineHeight must match editor (21.0); got {lh:?}",
        );
    }

    #[test]
    fn sync_propagates_editor_line_height_changes() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (setup_gutter_text_view, sync_gutter_text_font).chain(),
        );

        let editor = spawn_minimal_editor(&mut app, LineHeight::Px(21.0));
        app.update();

        // Host mutates the editor's LineHeight after spawn — common
        // pattern when settings change at runtime.
        let mut e = app.world_mut().entity_mut(editor);
        *e.get_mut::<LineHeight>().unwrap() = LineHeight::Px(28.0);
        app.update();

        let view = find_gutter_view(&mut app, editor);
        let lh = *app.world().entity(view).get::<LineHeight>().unwrap();
        assert_eq!(
            lh,
            LineHeight::Px(28.0),
            "sync_gutter_text_font should propagate editor LineHeight changes",
        );
    }
}
