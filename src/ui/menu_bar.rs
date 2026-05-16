use crate::trading::{StrategyRunConfig, TransportCommand, TransportCommandSender};
use crate::ui::components::ScenarioMetadata;
use crate::ui::components::{
    MenuBarRoot, MenuItem, MenuPopup, MenuTopLevel, OpenMenu, PanelKind, PanelSpawnRequested,
    PanelSpawnSource, PendingStrategyFragments, RedoMenuRequested, RegionKeyAllocator,
    StrategyBuffer, StrategyEditorSpawnSpec, StrategyFileLoadRequested, StrategyFragment,
    StrategyLoadMode, StrategyRunRequested, StrategyStatusLabel, UndoMenuRequested, WindowRoot,
};
use crate::ui::layout_persistence::{
    CacheRestoreRequested, LayoutLoadDialogRequested, LayoutLoadRequested, LayoutSaveAsRequested,
    LayoutSaveRequested, SidecarLayout,
};
use crate::ui::strategy_editor::split_py_into_fragments;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

const BTN_NORMAL: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BTN_HOVER: Color = Color::srgba(0.20, 0.20, 0.30, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.30, 0.30, 0.48, 1.0);

fn spawn_menu_item(parent: &mut ChildBuilder, label: &str, action: MenuItem) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(BTN_NORMAL),
            action,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(label),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.82, 0.82, 0.82)),
            ));
        });
}

pub fn spawn_menu_bar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(24.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(2.0),
                padding: UiRect::horizontal(Val::Px(4.0)),
                overflow: Overflow::visible(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.07, 0.11, 0.95)),
            MenuBarRoot,
        ))
        .with_children(|p| {
            // [ファイル ▾] トップレベルボタン
            p.spawn((
                Button,
                Node {
                    overflow: Overflow::visible(),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                    align_items: AlignItems::Center,
                    position_type: PositionType::Relative,
                    ..default()
                },
                BackgroundColor(BTN_NORMAL),
                MenuTopLevel::File,
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new("File(&F)"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.82, 0.82, 0.82)),
                ));
                // File popup
                p.spawn((
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        top: Val::Px(22.0),
                        left: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        min_width: Val::Px(200.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.10, 0.16, 0.98)),
                    GlobalZIndex(100),
                    MenuPopup(MenuTopLevel::File),
                ))
                .with_children(|p| {
                    spawn_menu_item(p, "Open (Ctrl+O)", MenuItem::LoadLayout);
                    spawn_menu_item(p, "Save (Ctrl+S)", MenuItem::SaveLayout);
                    spawn_menu_item(p, "Save As (Ctrl+Shift+S)", MenuItem::SaveLayoutAs);
                });
            });

            // [編集 ▾] トップレベルボタン
            p.spawn((
                Button,
                Node {
                    overflow: Overflow::visible(),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                    align_items: AlignItems::Center,
                    position_type: PositionType::Relative,
                    ..default()
                },
                BackgroundColor(BTN_NORMAL),
                MenuTopLevel::Edit,
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new("Edit(&E)"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.82, 0.82, 0.82)),
                ));
                // Edit popup
                p.spawn((
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        top: Val::Px(22.0),
                        left: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        min_width: Val::Px(160.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.10, 0.16, 0.98)),
                    GlobalZIndex(100),
                    MenuPopup(MenuTopLevel::Edit),
                ))
                .with_children(|p| {
                    spawn_menu_item(p, "Undo (Ctrl+Z)", MenuItem::Undo);
                    spawn_menu_item(p, "Redo (Ctrl+Y)", MenuItem::Redo);
                });
            });

            // spacer
            p.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

            // strategy status label
            p.spawn((
                Text::new("strategy: none"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.55, 0.55)),
                StrategyStatusLabel,
            ));
        });
}

pub fn menu_top_level_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &MenuTopLevel),
        (Changed<Interaction>, With<Button>),
    >,
    mut open_menu: ResMut<OpenMenu>,
) {
    for (interaction, mut bg, top) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                open_menu.0 = if open_menu.0 == Some(*top) {
                    None
                } else {
                    Some(*top)
                };
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

pub fn sync_menu_popup_visibility_system(
    open_menu: Res<OpenMenu>,
    mut popup_q: Query<(&MenuPopup, &mut Node)>,
) {
    if !open_menu.is_changed() {
        return;
    }
    for (popup, mut node) in &mut popup_q {
        node.display = if open_menu.0 == Some(popup.0) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub fn menu_keyboard_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut open_menu: ResMut<OpenMenu>,
    mut kb_events: ResMut<Events<KeyboardInput>>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !alt {
        return;
    }
    let handled = if keys.just_pressed(KeyCode::KeyF) {
        open_menu.0 = if open_menu.0 == Some(MenuTopLevel::File) {
            None
        } else {
            Some(MenuTopLevel::File)
        };
        true
    } else if keys.just_pressed(KeyCode::KeyE) {
        open_menu.0 = if open_menu.0 == Some(MenuTopLevel::Edit) {
            None
        } else {
            Some(MenuTopLevel::Edit)
        };
        true
    } else {
        false
    };
    if handled {
        kb_events.clear();
    }
}

pub fn menu_item_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &MenuItem),
        (Changed<Interaction>, With<Button>),
    >,
    mut open_menu: ResMut<OpenMenu>,
    mut save_ev: EventWriter<LayoutSaveRequested>,
    mut save_as_ev: EventWriter<LayoutSaveAsRequested>,
    mut load_ev: EventWriter<LayoutLoadDialogRequested>,
    mut undo_ev: EventWriter<UndoMenuRequested>,
    mut redo_ev: EventWriter<RedoMenuRequested>,
) {
    for (interaction, mut bg, item) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                open_menu.0 = None;
                match item {
                    MenuItem::SaveLayout => {
                        info!("menu: save layout requested");
                        save_ev.send(LayoutSaveRequested);
                    }
                    MenuItem::SaveLayoutAs => {
                        info!("menu: save layout as requested");
                        save_as_ev.send(LayoutSaveAsRequested);
                    }
                    MenuItem::LoadLayout => {
                        info!("menu: load layout requested");
                        load_ev.send(LayoutLoadDialogRequested);
                    }
                    MenuItem::Undo => {
                        info!("menu: undo requested");
                        undo_ev.send(UndoMenuRequested);
                    }
                    MenuItem::Redo => {
                        info!("menu: redo requested");
                        redo_ev.send(RedoMenuRequested);
                    }
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

/// 起動時に固定 cache `%LocalAppData%/.../app_state.json` を読み、
/// `CacheRestoreRequested` を発火して復元処理に流す。
/// Startup schedule で 1 回だけ実行される。
pub fn restore_last_strategy_system(mut events: EventWriter<CacheRestoreRequested>) {
    let Some((cache_json, _)) = cache_state_paths() else {
        info!("restore_from_cache: cache_dir not found, skipping");
        return;
    };
    if !cache_json.exists() {
        info!(
            "restore_from_cache: cache JSON {:?} not found, skipping",
            cache_json
        );
        return;
    }

    let text = match std::fs::read_to_string(&cache_json) {
        Ok(text) => text,
        Err(e) => {
            error!("restore_from_cache: failed to read {:?}: {e}", cache_json);
            return;
        }
    };
    let layout = match serde_json::from_str::<SidecarLayout>(&text) {
        Ok(layout) => layout,
        Err(e) => {
            error!("restore_from_cache: failed to parse {:?}: {e}", cache_json);
            return;
        }
    };

    info!(
        "restore_from_cache: firing CacheRestoreRequested from {:?}",
        cache_json
    );
    events.send(CacheRestoreRequested { layout });
}

pub fn log_strategy_file_load_requested_system(mut events: EventReader<StrategyFileLoadRequested>) {
    for event in events.read() {
        info!(
            "strategy file load requested: path={:?} mode={:?}",
            event.path, event.mode
        );
    }
}

pub(crate) fn cache_state_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let dir = dirs::cache_dir()?.join("the-trader-was-replaced");
    Some((dir.join("app_state.json"), dir.join("app_state.py")))
}

pub fn update_strategy_status_label_system(
    buffer: Res<StrategyBuffer>,
    fragments: Query<&StrategyFragment>,
    mut query: Query<&mut Text, With<StrategyStatusLabel>>,
) {
    let fragment_count = fragments.iter().count();
    let dirty_count = fragments.iter().filter(|f| f.dirty).count();
    let total_lines: usize = fragments
        .iter()
        .map(|f| {
            if f.source.is_empty() {
                0
            } else {
                f.source.matches('\n').count() + 1
            }
        })
        .sum();

    let label = match &buffer.original_path {
        Some(path) => {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<unnamed>");
            let cache = if buffer.cache_path.is_some() {
                " cached"
            } else {
                ""
            };
            let dirty_marker = if dirty_count > 0 { " *" } else { "" };
            format!(
                "strategy: {}{}{} [{} region{}, {} line{}{}]",
                name,
                cache,
                dirty_marker,
                fragment_count,
                if fragment_count == 1 { "" } else { "s" },
                total_lines,
                if total_lines == 1 { "" } else { "s" },
                if dirty_count > 0 {
                    format!(", {} dirty", dirty_count)
                } else {
                    String::new()
                },
            )
        }
        None if fragment_count > 0 => {
            let dirty_marker = if dirty_count > 0 { " *" } else { "" };
            format!(
                "strategy: untitled{} [{} region{}, {} line{}]",
                dirty_marker,
                fragment_count,
                if fragment_count == 1 { "" } else { "s" },
                total_lines,
                if total_lines == 1 { "" } else { "s" },
            )
        }
        None => "strategy: none".to_string(),
    };

    for mut text in &mut query {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_strategy_file_load_system(
    mut commands: Commands,
    mut events: EventReader<StrategyFileLoadRequested>,
    mut buffer: ResMut<StrategyBuffer>,
    mut allocator: ResMut<RegionKeyAllocator>,
    mut pending: ResMut<PendingStrategyFragments>,
    mut spawn_ev: EventWriter<PanelSpawnRequested>,
    mut layout_ev: EventWriter<LayoutLoadRequested>,
    existing_roots: Query<(Entity, &PanelKind), With<WindowRoot>>,
) {
    for event in events.read() {
        let source = match std::fs::read_to_string(&event.path) {
            Ok(s) => s,
            Err(err) => {
                error!("failed to read strategy file {:?}: {}", event.path, err);
                continue;
            }
        };

        let outcome = split_py_into_fragments(&source);
        for w in &outcome.warnings {
            warn!("strategy split warning ({:?}): {}", event.path, w);
        }

        buffer.original_path = Some(event.path.clone());
        buffer.last_merged_source = None;

        match cache_state_paths() {
            Some((_, cache_py)) => {
                if let Some(cache_dir) = cache_py.parent() {
                    if let Err(err) = std::fs::create_dir_all(cache_dir) {
                        error!("failed to create cache dir {:?}: {}", cache_dir, err);
                        buffer.cache_path = None;
                    } else {
                        buffer.cache_path = Some(cache_py.clone());
                        info!(
                            "strategy file loaded: original={:?}, cache={:?}, regions={}",
                            event.path,
                            cache_py,
                            outcome.fragments.len()
                        );
                    }
                } else {
                    buffer.cache_path = None;
                }
            }
            None => {
                error!("failed to compute cache state paths");
                buffer.cache_path = None;
            }
        }

        allocator.bump_to_at_least(outcome.max_numeric_suffix);

        for (entity, kind) in &existing_roots {
            if matches!(kind, PanelKind::StrategyEditor) {
                commands.entity(entity).despawn_recursive();
            }
        }

        pending.by_region_key.clear();
        pending.loaded_for_path = Some(event.path.clone());
        for (key, body) in &outcome.fragments {
            pending.by_region_key.insert(key.clone(), body.clone());
        }

        let sidecar_path = event.path.with_extension("json");
        let sidecar_exists = sidecar_path.exists();

        // sidecar が「scenario-only」(windows キー不在) の場合、layout だけに委ねると
        // どのパネルも spawn されない。peek して windows が無ければ fragments を直接 spawn。
        let sidecar_has_windows = sidecar_exists
            && std::fs::read_to_string(&sidecar_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("windows").cloned())
                .map(|w| !w.is_null())
                .unwrap_or(false);

        match (event.mode, sidecar_exists, sidecar_has_windows) {
            (StrategyLoadMode::LayoutRestore, _, _) => {}
            (_, true, true) => {
                info!(
                    "strategy load: sidecar present with windows, delegating spawn to layout {:?}",
                    sidecar_path
                );
                layout_ev.send(LayoutLoadRequested { path: sidecar_path });
            }
            (_, true, false) => {
                info!(
                    "strategy load: sidecar present but scenario-only (no windows), spawning fragments directly and firing layout for scenario metadata {:?}",
                    sidecar_path
                );
                for (key, body) in &outcome.fragments {
                    spawn_ev.send(PanelSpawnRequested {
                        kind: PanelKind::StrategyEditor,
                        source: PanelSpawnSource::LayoutLoad,
                        strategy_spec: Some(StrategyEditorSpawnSpec {
                            region_key: Some(key.clone()),
                            source: Some(body.clone()),
                            layout_source: PanelSpawnSource::LayoutLoad,
                        }),
                    });
                }
                layout_ev.send(LayoutLoadRequested { path: sidecar_path });
            }
            (_, false, _) => {
                for (key, body) in &outcome.fragments {
                    spawn_ev.send(PanelSpawnRequested {
                        kind: PanelKind::StrategyEditor,
                        source: PanelSpawnSource::LayoutLoad,
                        strategy_spec: Some(StrategyEditorSpawnSpec {
                            region_key: Some(key.clone()),
                            source: Some(body.clone()),
                            layout_source: PanelSpawnSource::LayoutLoad,
                        }),
                    });
                }
            }
        }

        if matches!(
            event.mode,
            StrategyLoadMode::UserOpen | StrategyLoadMode::LayoutRestore
        ) {
            if let Err(e) = sync_to_cache(&event.path) {
                error!("failed to sync strategy to cache: {e}");
            }
        }
    }
}

pub fn log_strategy_run_requested_system(mut events: EventReader<StrategyRunRequested>) {
    for event in events.read() {
        info!("strategy run requested: {:?}", event.cache_path);
    }
}

pub fn handle_strategy_run_system(
    mut events: EventReader<StrategyRunRequested>,
    scenario: Res<ScenarioMetadata>,
    sender: Option<Res<TransportCommandSender>>,
) {
    for event in events.read() {
        if scenario.instruments.is_empty() {
            error!("Run blocked: SCENARIO has no instruments");
            continue;
        }
        let Some(ref start) = scenario.start else {
            error!("Run blocked: SCENARIO has no start date");
            continue;
        };
        let Some(ref end) = scenario.end else {
            error!("Run blocked: SCENARIO has no end date");
            continue;
        };
        let Some(ref granularity) = scenario.granularity else {
            error!("Run blocked: SCENARIO has no granularity");
            continue;
        };

        let run_config = StrategyRunConfig {
            instruments: scenario.instruments.clone(),
            start: start.clone(),
            end: end.clone(),
            granularity: granularity.clone(),
            initial_cash: scenario.initial_cash,
        };

        info!(
            "strategy run: RunStrategy strategy_file={:?} instruments={:?} start={:?} end={:?} granularity={:?}",
            event.cache_path,
            run_config.instruments,
            run_config.start,
            run_config.end,
            run_config.granularity
        );

        let cmd = TransportCommand::RunStrategy {
            strategy_file: event.cache_path.clone(),
            config: run_config,
        };
        if let Some(sender) = &sender {
            if let Err(e) = sender.tx.send(cmd) {
                error!("failed to send RunStrategy command: {}", e);
            }
        } else {
            error!("RunStrategy: TransportCommandSender is None — backend not connected");
        }
    }
}

pub(crate) fn sync_to_cache(original_py: &std::path::Path) -> std::io::Result<()> {
    let Some((cache_json, cache_py)) = cache_state_paths() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cache_dir not found",
        ));
    };

    if let Some(cache_dir) = cache_py.parent() {
        std::fs::create_dir_all(cache_dir)?;
    }

    std::fs::copy(original_py, &cache_py)?;

    let original_sidecar = original_py.with_extension("json");
    copy_sidecar_to_cache(&original_sidecar, &cache_json);

    Ok(())
}

/// cache `.py` 書き込み直後に呼ぶ sidecar コピーロジック。
///
/// # 契約
/// - `cache_sidecar` を**無条件に削除**してから（stale cleanup）、
///   `original_sidecar` が存在する場合だけコピーする。
/// - この順序により「元 sidecar 削除 → 再 Open」時に stale cache が残らない。
pub(crate) fn copy_sidecar_to_cache(
    original_sidecar: &std::path::Path,
    cache_sidecar: &std::path::Path,
) {
    // (1) stale cache 削除 — 元 sidecar が無くなったケースも cover
    if cache_sidecar.exists() {
        let _ = std::fs::remove_file(cache_sidecar).map_err(|err| {
            warn!(
                "failed to remove stale cache sidecar {:?}: {}",
                cache_sidecar, err
            );
        });
    }

    // (2) 元 sidecar があればコピー
    if original_sidecar.exists() {
        match std::fs::copy(original_sidecar, cache_sidecar) {
            Ok(_) => info!(
                "strategy sidecar cached: {:?} -> {:?}",
                original_sidecar, cache_sidecar
            ),
            Err(err) => warn!(
                "failed to copy sidecar JSON {:?}: {}",
                original_sidecar, err
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tmp に `foo.py` + `foo.json` を作り、copy_sidecar_to_cache を呼ぶと
    /// cache に `<hash>__foo.json` が存在し内容が一致することを検証
    #[test]
    fn test_copy_sidecar_copies_json_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let original_sidecar = tmp.path().join("foo.json");
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_sidecar = cache_dir.path().join("abc123__foo.json");

        std::fs::write(&original_sidecar, r#"{"scenario": {"schema_version": 1}}"#).unwrap();

        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);

        assert!(
            cache_sidecar.exists(),
            "cache sidecar should exist after copy"
        );
        let content = std::fs::read_to_string(&cache_sidecar).unwrap();
        let orig_content = std::fs::read_to_string(&original_sidecar).unwrap();
        assert_eq!(
            content, orig_content,
            "cache sidecar content should match original"
        );
    }

    /// sidecar が存在しない場合でも、エラーにならず cache sidecar も作られない
    #[test]
    fn test_copy_sidecar_no_copy_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let original_sidecar = tmp.path().join("no_such.json");
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_sidecar = cache_dir.path().join("abc123__no_such.json");

        // sidecar 不在でも panic しない
        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);

        assert!(
            !cache_sidecar.exists(),
            "cache sidecar should NOT exist when original is absent"
        );
    }

    /// 元 sidecar を編集して再度 copy_sidecar_to_cache を呼ぶと cache sidecar も更新される
    #[test]
    fn test_copy_sidecar_overwrites_stale_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let original_sidecar = tmp.path().join("foo.json");
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_sidecar = cache_dir.path().join("abc123__foo.json");

        // 1 回目: 古い内容でコピー
        std::fs::write(&original_sidecar, r#"{"scenario": {"schema_version": 1}}"#).unwrap();
        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);

        // 元 sidecar を更新
        let updated = r#"{"scenario": {"schema_version": 1, "instrument": "7203.TSE"}}"#;
        std::fs::write(&original_sidecar, updated).unwrap();

        // 2 回目: 更新後の内容でコピー
        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);

        let content = std::fs::read_to_string(&cache_sidecar).unwrap();
        assert_eq!(
            content, updated,
            "cache sidecar should reflect updated original"
        );
    }

    /// 元 sidecar を削除して再度 copy_sidecar_to_cache を呼ぶと
    /// stale な cache sidecar が削除される（stale 残留防止）
    #[test]
    fn test_copy_sidecar_removes_stale_when_original_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let original_sidecar = tmp.path().join("foo.json");
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_sidecar = cache_dir.path().join("abc123__foo.json");

        // 1 回目: sidecar ありでコピー
        std::fs::write(&original_sidecar, r#"{"scenario": {}}"#).unwrap();
        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);
        assert!(
            cache_sidecar.exists(),
            "cache sidecar should exist after first copy"
        );

        // 元 sidecar を削除してから再 Open を模倣
        std::fs::remove_file(&original_sidecar).unwrap();
        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);

        assert!(
            !cache_sidecar.exists(),
            "stale cache sidecar should be removed when original is deleted"
        );
    }
}
