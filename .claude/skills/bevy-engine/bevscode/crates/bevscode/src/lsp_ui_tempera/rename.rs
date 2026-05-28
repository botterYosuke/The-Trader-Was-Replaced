//! Inline rename input renderer.
//!
//! Renders the current rename text plus a thin caret bar at
//! `cursor_position`. Actual character input is funneled through
//! [`crate::lsp_ui::interceptors`], which updates [`LspRenamePopup`]
//! and the sync layer re-emits a fresh [`RenameInputData`].
//!
//! This is a tempera-styled visual on top of bevscode's existing input
//! capture path, not a `tempera::text_input` widget — that widget owns
//! its own keystroke pipeline, which would conflict with bevscode's
//! `FocusedInput` observer. The visuals (chrome, font, caret) match
//! tempera's input styling so the rename popup still looks identical to
//! tempera text inputs in the user's other apps.
//!
//! [`LspRenamePopup`]: crate::lsp_ui::state::LspRenamePopup

use bevy::prelude::*;

use crate::lsp_ui::components::RenameInputData;
use crate::lsp_ui::state::{PopupObserversAttached, RenameLifecycle};
use crate::ui_kit::PopupChrome;

use super::anchor::{PopupAnchor, PopupPlacement};
use super::chrome::{apply_chrome, attach_rename_observers, clear_children, PopupTarget};

pub fn update_rename_input(
    mut commands: Commands,
    mut popups: Query<
        (
            Entity,
            &RenameInputData,
            &mut Node,
            Option<&Children>,
            Has<PopupObserversAttached>,
        ),
        Changed<RenameInputData>,
    >,
    mut lifecycles: Query<&mut RenameLifecycle>,
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
            attach_rename_observers(&mut commands, entity, data.editor);
        }

        let fg = chrome.palette.popover_foreground;
        let caret = chrome.palette.ring;
        let caret_h = chrome.typography.base;

        commands.entity(entity).with_children(|p| {
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(1.0),
                ..default()
            })
            .with_children(|row| {
                let (pre, post) = split_at_char(&data.text, data.cursor_position);
                if !pre.is_empty() {
                    row.spawn((Text::new(pre), chrome.body_font(), TextColor(fg)));
                }
                row.spawn((
                    Node {
                        width: Val::Px(1.0),
                        height: Val::Px(caret_h),
                        ..default()
                    },
                    BackgroundColor(caret),
                ));
                if !post.is_empty() {
                    row.spawn((Text::new(post), chrome.body_font(), TextColor(fg)));
                }
            });
        });
    }
}

fn split_at_char(s: &str, char_index: usize) -> (&str, &str) {
    let split_byte = s
        .char_indices()
        .nth(char_index)
        .map(|(b, _)| b)
        .unwrap_or(s.len());
    s.split_at(split_byte)
}
