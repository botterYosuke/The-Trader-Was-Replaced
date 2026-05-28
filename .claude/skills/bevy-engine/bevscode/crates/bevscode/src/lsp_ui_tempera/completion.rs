//! Completion popup renderer.
//!
//! Reads [`CompletionPopupData`] and (re)builds a vertical list of
//! `Node` items under the popup entity. Each row mirrors tempera's
//! context-menu item layout (label left, optional detail right,
//! `MenuTokens.item_height` tall, `item_padding_x` horizontal padding)
//! so completion lists look identical to the dropdowns elsewhere in
//! the user's tempera apps.

use bevy::prelude::*;

use crate::lsp_ui::components::{CompletionItemData, CompletionPopupData};
use crate::lsp_ui::state::{CompletionLifecycle, PopupObserversAttached};
use crate::ui_kit::PopupChrome;

use super::anchor::{PopupAnchor, PopupPlacement};
use super::chrome::{apply_chrome, attach_completion_observers, clear_children, PopupTarget};

pub fn update_completion_popup(
    mut commands: Commands,
    mut popups: Query<
        (
            Entity,
            &CompletionPopupData,
            &mut Node,
            Option<&Children>,
            Has<PopupObserversAttached>,
        ),
        Changed<CompletionPopupData>,
    >,
    mut lifecycles: Query<&mut CompletionLifecycle>,
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
            attach_completion_observers(&mut commands, entity, data.editor);
        }

        let item_height = data.height / data.max_visible.max(1) as f32;

        commands.entity(entity).with_children(|p| {
            let start = data.scroll_offset;
            let end = (start + data.max_visible).min(data.items.len());
            for (i, item) in data.items[start..end].iter().enumerate() {
                let absolute = start + i;
                let selected = absolute == data.selected_index;
                spawn_item(p, item, selected, item_height, &chrome);
            }
        });
    }
}

fn spawn_item(
    parent: &mut ChildSpawnerCommands,
    item: &CompletionItemData,
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
    let muted = chrome.palette.muted_foreground;

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
            row.spawn((Text::new(&item.label), chrome.body_font(), TextColor(fg)));
            if let Some(detail) = &item.detail {
                row.spawn((Text::new(detail), chrome.body_font(), TextColor(muted)));
            }
        });
}
