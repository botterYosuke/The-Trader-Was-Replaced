//! Code-actions popup renderer.
//!
//! Mirrors the completion popup row layout: a vertical list with one
//! row per action, the selected row highlighted with the tempera accent
//! background. Icons are pre-resolved Unicode glyphs from the sync
//! layer for now.

use bevy::prelude::*;

use crate::lsp_ui::components::{CodeActionItemData, CodeActionsPopupData};
use crate::lsp_ui::state::{CodeActionsLifecycle, PopupObserversAttached};
use crate::ui_kit::PopupChrome;

use super::anchor::{PopupAnchor, PopupPlacement};
use super::chrome::{apply_chrome, attach_code_actions_observers, clear_children, PopupTarget};

pub fn update_code_actions_popup(
    mut commands: Commands,
    mut popups: Query<
        (
            Entity,
            &CodeActionsPopupData,
            &mut Node,
            Option<&Children>,
            Has<PopupObserversAttached>,
        ),
        Changed<CodeActionsPopupData>,
    >,
    mut lifecycles: Query<&mut CodeActionsLifecycle>,
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
            attach_code_actions_observers(&mut commands, entity, data.editor);
        }

        let row_count = data.actions.len().clamp(1, 10);
        let row_height = data.height / row_count as f32;

        commands.entity(entity).with_children(|p| {
            for (i, action) in data.actions.iter().enumerate() {
                let selected = i == data.selected_index;
                spawn_action_row(p, action, selected, row_height, &chrome);
            }
        });
    }
}

fn spawn_action_row(
    parent: &mut ChildSpawnerCommands,
    action: &CodeActionItemData,
    selected: bool,
    height: f32,
    chrome: &PopupChrome,
) {
    let bg = if selected {
        chrome.palette.accent
    } else {
        Color::NONE
    };
    let fg = if selected {
        chrome.palette.accent_foreground
    } else {
        chrome.palette.popover_foreground
    };

    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(height),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::axes(
                    Val::Px(chrome.menu.item_padding_x),
                    Val::Px(chrome.spacing.xxs),
                ),
                column_gap: Val::Px(chrome.spacing.sm),
                border_radius: BorderRadius::all(Val::Px(chrome.spacing.corner_radius_tiny)),
                ..default()
            },
            BackgroundColor(bg),
        ))
        .with_children(|row| {
            row.spawn((Text::new(&action.icon), chrome.body_font(), TextColor(fg)));
            row.spawn((Text::new(&action.title), chrome.body_font(), TextColor(fg)));
        });
}
