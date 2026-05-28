//! Signature help renderer.
//!
//! Renders the active overload's label with the active-parameter range
//! emphasized (bold + foreground color), plus a `1/N` pager when multiple
//! overloads exist.

use bevy::prelude::*;

use crate::lsp_ui::components::SignatureHelpPopupData;
use crate::lsp_ui::state::{PopupObserversAttached, SignatureLifecycle};
use crate::ui_kit::PopupChrome;

use super::anchor::{PopupAnchor, PopupPlacement};
use super::chrome::{apply_chrome, attach_signature_observers, clear_children, PopupTarget};

pub fn update_signature_help_popup(
    mut commands: Commands,
    mut popups: Query<
        (
            Entity,
            &SignatureHelpPopupData,
            &mut Node,
            Option<&Children>,
            Has<PopupObserversAttached>,
        ),
        Changed<SignatureHelpPopupData>,
    >,
    mut lifecycles: Query<&mut SignatureLifecycle>,
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
                placement: PopupPlacement::PreferAbove,
            },
        );
        clear_children(&mut commands, children);
        if placed.is_none() {
            continue;
        }

        if let Ok(mut lc) = lifecycles.get_mut(data.editor) {
            if lc.popup_entity != Some(entity) {
                lc.popup_entity = Some(entity);
            }
        }
        if !observers_attached {
            attach_signature_observers(&mut commands, entity, data.editor);
        }

        let fg = chrome.palette.popover_foreground;
        let muted = chrome.palette.muted_foreground;
        let label = &data.label;
        let active_range = data.parameter_ranges.get(data.active_parameter).copied();

        commands.entity(entity).with_children(|p| {
            if data.total_signatures > 1 {
                p.spawn((
                    Text::new(format!(
                        "{}/{}",
                        data.current_index + 1,
                        data.total_signatures
                    )),
                    chrome.small_font(),
                    TextColor(muted),
                ));
            }

            // Split the label into [pre][active][post] so the active
            // parameter renders bold in the popover foreground while the
            // rest stays muted. Falls back to a single span when no
            // active range applies.
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(0.0),
                ..default()
            })
            .with_children(|row| match active_range {
                Some((s, e)) if s < e && e <= label.len() => {
                    let pre = &label[..s];
                    let active = &label[s..e];
                    let post = &label[e..];
                    if !pre.is_empty() {
                        row.spawn((Text::new(pre), chrome.body_font(), TextColor(muted)));
                    }
                    row.spawn((Text::new(active), chrome.body_font_bold(), TextColor(fg)));
                    if !post.is_empty() {
                        row.spawn((Text::new(post), chrome.body_font(), TextColor(muted)));
                    }
                }
                _ => {
                    row.spawn((Text::new(label), chrome.body_font(), TextColor(fg)));
                }
            });
        });
    }
}
