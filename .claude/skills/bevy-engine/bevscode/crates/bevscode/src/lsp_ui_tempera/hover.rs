//! Hover tooltip renderer.
//!
//! LSP hover responses are CommonMark per the spec, so the popup body
//! is a `bevy_markdown::Markdown` child entity. Plain-text-only servers
//! still render correctly: prose without markdown syntax becomes plain
//! paragraphs through the same parser. Fenced code blocks get
//! tree-sitter syntax highlighting via the `MarkdownHighlighter`
//! resource installed by [`super::LspUiTemperaPlugin`].

use bevy::picking::events::Scroll;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_markdown::Markdown;

use crate::lsp_ui::components::HoverPopupData;
use crate::lsp_ui::state::{HoverLifecycle, PopupObserversAttached};
use crate::plugin::ScrollAnimator;
use crate::ui_kit::{markdown_theme_from_chrome, PopupChrome};

use super::anchor::{PopupAnchor, PopupPlacement};
use super::chrome::{apply_chrome, attach_hover_observers, PopupTarget};

pub fn update_hover_popup(
    mut commands: Commands,
    mut popups: Query<
        (
            Entity,
            &HoverPopupData,
            &mut Node,
            Option<&Children>,
            Has<PopupObserversAttached>,
        ),
        Changed<HoverPopupData>,
    >,
    mut markdown_children: Query<&mut Markdown>,
    mut lifecycles: Query<&mut HoverLifecycle>,
    anchor: PopupAnchor,
    chrome: PopupChrome,
) {
    for (entity, data, mut node, children, observers_attached) in popups.iter_mut() {
        let placed = apply_chrome(
            &mut commands,
            entity,
            &mut node,
            &anchor,
            &chrome,
            &PopupTarget {
                editor: data.editor,
                line: data.line,
                character: data.character,
                size: Vec2::new(data.width, data.height),
                placement: PopupPlacement::PreferBelow,
            },
        );
        if placed.is_none() {
            continue;
        }

        // Hover is the one popup whose content can exceed the chrome
        // height (long rust-analyzer docs). Override the shared
        // chrome's `Overflow::clip()` with vertical scroll so capped
        // content stays reachable.
        node.overflow = Overflow::scroll_y();

        // Register the chrome entity on the editor's hover lifecycle so
        // the dismiss-grace tick + the hover trigger observer can both
        // tell "popup is currently up" without re-querying for the
        // marker component. Idempotent on every frame the hover stays
        // visible.
        if let Ok(mut hover_lc) = lifecycles.get_mut(data.editor) {
            if hover_lc.popup_entity != Some(entity) {
                hover_lc.popup_entity = Some(entity);
            }
        }
        if !observers_attached {
            attach_hover_observers(&mut commands, entity, data.editor);
            // Reuse the editor's smooth-scroll animator on the popup.
            // The handler writes `target`; `drive_scroll_animator`
            // tweens `ScrollPosition` toward it with cubic-out easing.
            commands.entity(entity).insert(ScrollAnimator::smooth());
            // Scroll wheel input on the popup must drive the popup's
            // `ScrollPosition` and *not* propagate up to the editor's
            // `on_pointer_scroll` observer (which would scroll the
            // buffer instead).
            commands.entity(entity).observe(
                |mut trigger: On<Pointer<Scroll>>,
                 mut q: Query<(&mut ScrollAnimator, &ScrollPosition, &ComputedNode)>,
                 children_q: Query<&Children>,
                 child_nodes: Query<&ComputedNode>| {
                    let popup = trigger.entity;
                    let Ok((mut anim, sp, popup_node)) = q.get_mut(popup) else {
                        return;
                    };
                    // `Pointer<Scroll>` deltas come in lines for wheel
                    // mice and pixels for trackpads. Convert lines to
                    // a fixed pixel step matching one body-text line.
                    const SCROLL_LINE_PX: f32 = 18.0;
                    let unit_y = match trigger.event().unit {
                        bevy::input::mouse::MouseScrollUnit::Line => SCROLL_LINE_PX,
                        bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
                    };
                    let unit_x = unit_y;
                    let dy = trigger.event().y * unit_y;
                    let dx = trigger.event().x * unit_x;

                    // Clamp the target so flicking the wheel hard
                    // doesn't queue an animation that overshoots past
                    // the bottom of the content.
                    let viewport = popup_node.size() * popup_node.inverse_scale_factor();
                    let content_h = children_q
                        .get(popup)
                        .map(|c| {
                            c.iter()
                                .filter_map(|child| child_nodes.get(child).ok())
                                .map(|n| n.size().y * n.inverse_scale_factor())
                                .fold(0.0_f32, |acc, h| acc + h)
                        })
                        .unwrap_or(0.0);
                    let max_y = (content_h - viewport.y).max(0.0);
                    // Anchor to the current scroll position rather than
                    // the in-flight animator target so each wheel tick
                    // moves a consistent distance regardless of how far
                    // the eased animation has progressed.
                    let base_y = sp.0.y;
                    let base_x = sp.0.x;
                    anim.target.y = (base_y - dy).clamp(0.0, max_y);
                    anim.target.x = (base_x - dx).max(0.0);
                    trigger.propagate(false);
                },
            );
        }

        // The popup data Component is overwritten every frame the hover
        // is visible (see `sync::sync_hover_popup`), so always despawning
        // and respawning the Markdown child would churn its grandchildren
        // every frame and leave the popup visually empty. Reuse the
        // existing child and only mutate its `source` when the content
        // actually changed.
        let existing_md = children
            .into_iter()
            .flat_map(|c| c.iter())
            .find(|c| markdown_children.get(*c).is_ok());

        if let Some(child) = existing_md {
            if let Ok(mut md) = markdown_children.get_mut(child) {
                if md.source != data.content {
                    md.source = data.content.clone();
                }
            }
        } else {
            let (fonts, colors, spacing, scales) = markdown_theme_from_chrome(&chrome);
            commands.entity(entity).with_children(|p| {
                p.spawn((
                    Markdown {
                        source: data.content.clone(),
                    },
                    fonts,
                    colors,
                    spacing,
                    scales,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        max_width: Val::Px(data.width - chrome.spacing.sm),
                        ..default()
                    },
                ));
            });
        }
    }
}
