use crate::replay::{ReplayStartupPhase, ReplayStartupProgress};
use crate::trading::{
    CurrentRun, ExecutionMode, ExecutionModeRes, RunState, StrategyRunConfig, TradingSession,
    TransportCommand, TransportCommandSender, VenueStatusRes, is_venue_busy_for_menu,
};
use crate::ui::components::ScenarioMetadata;
use crate::ui::components::{
    InstrumentRegistry, MenuBarRoot, MenuItem, MenuPopup, MenuTopLevel, OpenMenu, PanelKind,
    PanelSpawnRequested, PanelSpawnSource, PendingStrategyFragments, RedoMenuRequested,
    RegionKeyAllocator, ScenarioReadTarget, ScenarioStartupParams, ScenarioWritebackPaths,
    StrategyBuffer, StrategyEditorSpawnSpec, StrategyFileLoadRequested, StrategyFragment,
    StrategyLoadMode, StrategyRunRequested, StepFromIdleRequested, StrategyStatusLabel,
    UndoMenuRequested, WindowRoot,
    flush_sidecars_now,
};
use crate::ui::layout_persistence::{
    CacheRestoreRequested, LayoutLoadDialogRequested, LayoutLoadMode, LayoutLoadRequested,
    LayoutSaveAsRequested, LayoutSaveRequested, SidecarLayout,
};
use crate::ui::settings::{SettingsModalRoot, spawn_settings_modal};
use crate::ui::strategy_editor::split_py_into_fragments;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

const BTN_NORMAL: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BTN_HOVER: Color = Color::srgba(0.20, 0.20, 0.30, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.30, 0.30, 0.48, 1.0);

fn spawn_menu_item(parent: &mut ChildSpawnerCommands, label: &str, action: MenuItem) {
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
                    spawn_menu_item(p, "New", MenuItem::FileNew);
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

            // [Venue ▾] トップレベルボタン
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
                MenuTopLevel::Venue,
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new("Venue(&V)"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.82, 0.82, 0.82)),
                ));
                // Venue popup
                p.spawn((
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        top: Val::Px(22.0),
                        left: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        min_width: Val::Px(240.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.10, 0.16, 0.98)),
                    GlobalZIndex(100),
                    MenuPopup(MenuTopLevel::Venue),
                ))
                .with_children(|p| {
                    spawn_menu_item(
                        p,
                        "Connect Tachibana (Demo)",
                        MenuItem::VenueConnectTachibanaDemo,
                    );
                    spawn_menu_item(
                        p,
                        "Connect Tachibana (Prod)",
                        MenuItem::VenueConnectTachibanaProd,
                    );
                    spawn_menu_item(
                        p,
                        "Connect kabuStation (Verify)",
                        MenuItem::VenueConnectKabuVerify,
                    );
                    spawn_menu_item(
                        p,
                        "Connect kabuStation (Prod)",
                        MenuItem::VenueConnectKabuProd,
                    );
                    spawn_menu_item(p, "Disconnect", MenuItem::VenueDisconnect);
                });
            });

            // [Help ▾] トップレベルボタン
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
                MenuTopLevel::Help,
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new("Help(&H)"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.82, 0.82, 0.82)),
                ));
                // Help popup
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
                    MenuPopup(MenuTopLevel::Help),
                ))
                .with_children(|p| {
                    spawn_menu_item(p, "Settings", MenuItem::HelpSettings);
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
    mut kb_events: ResMut<Messages<KeyboardInput>>,
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
    } else if keys.just_pressed(KeyCode::KeyV) {
        open_menu.0 = if open_menu.0 == Some(MenuTopLevel::Venue) {
            None
        } else {
            Some(MenuTopLevel::Venue)
        };
        true
    } else if keys.just_pressed(KeyCode::KeyH) {
        open_menu.0 = if open_menu.0 == Some(MenuTopLevel::Help) {
            None
        } else {
            Some(MenuTopLevel::Help)
        };
        true
    } else {
        false
    };
    if handled {
        kb_events.clear();
    }
}

fn send_venue_login(
    sender: &Option<Res<TransportCommandSender>>,
    venue_id: &str,
    environment_hint: &str,
) {
    let Some(sender) = sender.as_ref() else {
        warn!(
            "menu: Venue→Connect dropped (TransportCommandSender unavailable, venue={}, env={})",
            venue_id, environment_hint
        );
        return;
    };
    info!(
        "menu: Venue→Connect requested (venue={}, env={})",
        venue_id, environment_hint
    );
    if sender
        .tx
        .send(TransportCommand::VenueLogin {
            venue_id: venue_id.to_string(),
            credentials_source: "prompt".to_string(),
            environment_hint: environment_hint.to_string(),
        })
        .is_err()
    {
        error!(
            "menu: VenueLogin send failed (transport channel closed, venue={})",
            venue_id
        );
    }
}

const BTN_DISABLED: Color = Color::srgba(0.20, 0.20, 0.20, 0.5);
const TEXT_NORMAL: Color = Color::srgb(0.82, 0.82, 0.82);
const TEXT_DISABLED: Color = Color::srgba(0.40, 0.40, 0.40, 0.5);

fn venue_connect_is_tachibana(item: &MenuItem) -> bool {
    matches!(
        item,
        MenuItem::VenueConnectTachibanaDemo | MenuItem::VenueConnectTachibanaProd
    )
}

fn venue_connect_is_kabu(item: &MenuItem) -> bool {
    matches!(
        item,
        MenuItem::VenueConnectKabuVerify | MenuItem::VenueConnectKabuProd
    )
}

/// Returns true if the given Venue→Connect MenuItem should be disabled in the
/// current VenueStatusRes (occupied slot — same or opposite venue is busy).
fn venue_connect_disabled(item: &MenuItem, status: &VenueStatusRes) -> bool {
    let is_connect = venue_connect_is_tachibana(item) || venue_connect_is_kabu(item);
    if !is_connect {
        return false;
    }
    is_venue_busy_for_menu(status.state)
}

/// Drives the disabled / normal background+text color of Venue→Connect buttons.
///
/// Runs every frame (no `is_changed()` gate) so that the Hover/None color
/// updates inside `menu_item_system` — which fire on `Changed<Interaction>` —
/// cannot leave a disabled button stuck on the hover color on later frames.
pub fn gate_venue_menu_items_system(
    mut btn_q: Query<(&MenuItem, &Interaction, &mut BackgroundColor, &Children), With<Button>>,
    mut text_q: Query<&mut TextColor>,
    status: Res<VenueStatusRes>,
) {
    for (item, interaction, mut bg, children) in &mut btn_q {
        let is_connect = venue_connect_is_tachibana(item) || venue_connect_is_kabu(item);
        if !is_connect {
            continue;
        }
        let disabled = is_venue_busy_for_menu(status.state);
        // Pressed lasts one frame and is handled by `menu_item_system`; leave
        // its visual indicator (BTN_PRESSED) alone here.
        if !matches!(interaction, Interaction::Pressed) {
            let target_bg = if disabled { BTN_DISABLED } else { BTN_NORMAL };
            if bg.0 != target_bg {
                bg.0 = target_bg;
            }
        }
        let target_text = if disabled { TEXT_DISABLED } else { TEXT_NORMAL };
        for child in children.iter() {
            if let Ok(mut tc) = text_q.get_mut(child) {
                if tc.0 != target_text {
                    tc.0 = target_text;
                }
            }
        }
    }
}

pub fn hide_unconfigured_venue_items_system(
    status: Res<VenueStatusRes>,
    mut btn_q: Query<(&MenuItem, &mut Node), With<Button>>,
) {
    if !status.is_changed() {
        return;
    }
    let configured = status
        .configured_venue
        .as_deref()
        .map(|s| s.to_ascii_uppercase());
    for (item, mut node) in &mut btn_q {
        let is_tachibana = venue_connect_is_tachibana(item);
        let is_kabu = venue_connect_is_kabu(item);
        if !is_tachibana && !is_kabu {
            continue;
        }
        let target_display = match &configured {
            Some(v) if v == "TACHIBANA" && is_kabu => Display::None,
            Some(v) if v == "KABU" && is_tachibana => Display::None,
            _ => Display::Flex,
        };
        if node.display != target_display {
            node.display = target_display;
        }
    }
}

pub fn menu_item_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &MenuItem),
        (Changed<Interaction>, With<Button>),
    >,
    mut open_menu: ResMut<OpenMenu>,
    mut save_ev: MessageWriter<LayoutSaveRequested>,
    mut save_as_ev: MessageWriter<LayoutSaveAsRequested>,
    mut load_ev: MessageWriter<LayoutLoadDialogRequested>,
    mut undo_ev: MessageWriter<UndoMenuRequested>,
    mut redo_ev: MessageWriter<RedoMenuRequested>,
    sender: Option<Res<TransportCommandSender>>,
    execution_mode: Res<ExecutionModeRes>,
    venue_status: Res<VenueStatusRes>,
    mut commands: Commands,
    existing_settings: Query<(), With<SettingsModalRoot>>,
) {
    for (interaction, mut bg, item) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                open_menu.0 = None;
                match item {
                    MenuItem::SaveLayout => {
                        info!("menu: save layout requested");
                        save_ev.write(LayoutSaveRequested);
                    }
                    MenuItem::SaveLayoutAs => {
                        info!("menu: save layout as requested");
                        save_as_ev.write(LayoutSaveAsRequested);
                    }
                    MenuItem::LoadLayout => {
                        // Phase 8 §3.5.1 / §3.6.1「File→Open Strategy の execution_mode 連動」:
                        //   現在 Live (LiveManual or LiveAuto) なら、ダイアログ発火前に
                        //   SetExecutionMode(LiveAuto) を送って Live Auto に遷移させる。
                        //   Replay モードの場合は既存挙動どおりダイアログのみ。
                        // 既存の sidecar JSON 経由 .py 間接ロード経路 (apply_layout_system →
                        //   StrategyFileLoadRequested) は不変。本 Step では mode 連動のみ追加する。
                        if matches!(
                            execution_mode.mode,
                            ExecutionMode::LiveManual | ExecutionMode::LiveAuto
                        ) {
                            if let Some(sender) = sender.as_ref() {
                                info!("menu: File→Open in Live mode → SetExecutionMode(LiveAuto)");
                                if sender
                                    .tx
                                    .send(TransportCommand::SetExecutionMode {
                                        mode: ExecutionMode::LiveAuto,
                                    })
                                    .is_err()
                                {
                                    error!(
                                        "menu: SetExecutionMode(LiveAuto) send failed (transport channel closed)"
                                    );
                                }
                            } else {
                                warn!(
                                    "menu: File→Open in Live mode but TransportCommandSender unavailable; skipping SetExecutionMode"
                                );
                            }
                        }
                        info!("menu: load layout requested");
                        load_ev.write(LayoutLoadDialogRequested);
                    }
                    MenuItem::Undo => {
                        info!("menu: undo requested");
                        undo_ev.write(UndoMenuRequested);
                    }
                    MenuItem::Redo => {
                        info!("menu: redo requested");
                        redo_ev.write(RedoMenuRequested);
                    }
                    MenuItem::FileNew => {
                        // Phase 8 §3.5 / §3.6「起動・New の挙動」:
                        //   SetExecutionMode(LiveManual) を発行してロード中の戦略を破棄。Live Manual に戻る。
                        // TODO(Phase 8 follow-up): 未保存サイドカー .json の自動保存 / 戦略 buffer の
                        //   unload (StrategyBuffer.original_path のクリア等)。
                        //   本 Step では SetExecutionMode 発火のみ実装し、副作用群は別 Step に繰り越す。
                        let Some(sender) = sender.as_ref() else {
                            warn!("menu: File→New dropped (TransportCommandSender unavailable)");
                            continue;
                        };
                        // Phase 8 §3.5 / §3.6「New 時に Replay 稼働中なら停止を先行発火」:
                        //   backend 側で冪等扱い (既に Idle なら error_code 返却 → main.rs:380-399 で
                        //   error ログ 1 行)。UI は RunState を見ずに無条件で送る。
                        info!("menu: File→New → ForceStop (precede SetExecutionMode)");
                        if sender.tx.send(TransportCommand::ForceStop).is_err() {
                            error!("menu: ForceStop send failed (transport channel closed)");
                        }
                        info!("menu: File→New requested (SetExecutionMode(LiveManual))");
                        if sender
                            .tx
                            .send(TransportCommand::SetExecutionMode {
                                mode: ExecutionMode::LiveManual,
                            })
                            .is_err()
                        {
                            error!(
                                "menu: SetExecutionMode(LiveManual) send failed (transport channel closed)"
                            );
                        }
                    }
                    MenuItem::VenueConnectTachibanaDemo
                    | MenuItem::VenueConnectTachibanaProd
                    | MenuItem::VenueConnectKabuVerify
                    | MenuItem::VenueConnectKabuProd => {
                        if is_venue_busy_for_menu(venue_status.state) {
                            warn!(
                                "menu: VenueConnect blocked (venue busy, state={:?})",
                                venue_status.state
                            );
                            continue;
                        }
                        let (venue, env) = match item {
                            MenuItem::VenueConnectTachibanaDemo => ("tachibana", "demo"),
                            MenuItem::VenueConnectTachibanaProd => ("tachibana", "prod"),
                            MenuItem::VenueConnectKabuVerify => ("kabu", "verify"),
                            _ => ("kabu", "prod"),
                        };
                        send_venue_login(&sender, venue, env);
                    }
                    MenuItem::VenueDisconnect => {
                        let Some(sender) = sender.as_ref() else {
                            warn!(
                                "menu: Venue→Disconnect dropped (TransportCommandSender unavailable)"
                            );
                            continue;
                        };
                        info!("menu: Venue→Disconnect requested");
                        if sender.tx.send(TransportCommand::VenueLogout).is_err() {
                            error!("menu: VenueLogout send failed (transport channel closed)");
                        }
                    }
                    MenuItem::HelpSettings => {
                        info!("menu: Help→Settings requested");
                        if existing_settings.is_empty() {
                            spawn_settings_modal(&mut commands);
                        }
                    }
                }
            }
            Interaction::Hovered => {
                let target = if venue_connect_disabled(item, &venue_status) {
                    BTN_DISABLED
                } else {
                    BTN_HOVER
                };
                if bg.0 != target {
                    bg.0 = target;
                }
            }
            Interaction::None => {
                let target = if venue_connect_disabled(item, &venue_status) {
                    BTN_DISABLED
                } else {
                    BTN_NORMAL
                };
                if bg.0 != target {
                    bg.0 = target;
                }
            }
        }
    }
}

/// 起動時に固定 cache `%LocalAppData%/.../app_state.json` を読み、
/// `CacheRestoreRequested` を発火して復元処理に流す。
/// Startup schedule で 1 回だけ実行される。
pub fn restore_last_strategy_system(mut events: MessageWriter<CacheRestoreRequested>) {
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

    let text = match crate::ui::layout_persistence::read_json_with_bom_strip(&cache_json) {
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
    events.write(CacheRestoreRequested { layout });
}

pub fn log_strategy_file_load_requested_system(mut events: MessageReader<StrategyFileLoadRequested>) {
    for event in events.read() {
        info!(
            "strategy file load requested: path={:?} mode={:?}",
            event.path, event.mode
        );
    }
}

pub(crate) fn cache_state_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    // BACKCAST_CACHE_DIR が設定されていればそれを使う（headless テストが実アプリ共有の
    // cache を汚さず strategy ロードを検証するための隔離フック。未設定なら従来どおり OS cache dir）。
    let dir = match std::env::var_os("BACKCAST_CACHE_DIR") {
        Some(d) => std::path::PathBuf::from(d),
        None => dirs::cache_dir()?.join("the-trader-was-replaced"),
    };
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
    mut events: MessageReader<StrategyFileLoadRequested>,
    mut buffer: ResMut<StrategyBuffer>,
    mut allocator: ResMut<RegionKeyAllocator>,
    mut pending: ResMut<PendingStrategyFragments>,
    mut spawn_ev: MessageWriter<PanelSpawnRequested>,
    mut layout_ev: MessageWriter<LayoutLoadRequested>,
    existing_roots: Query<(Entity, &PanelKind), With<WindowRoot>>,
    mut scenario_target: ResMut<ScenarioReadTarget>, // ← ADD
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
        // ↓ ADD: sidecar path を ScenarioReadTarget にセット（parse_scenario_system がここを読む）
        let sidecar_path_for_target = event.path.with_extension("json");
        scenario_target.0 = Some(sidecar_path_for_target);
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
                commands.entity(entity).despawn();
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
        let sidecar_has_windows = crate::ui::layout_persistence::sidecar_has_windows(&sidecar_path);

        match (event.mode, sidecar_exists, sidecar_has_windows) {
            (StrategyLoadMode::LayoutRestore, _, _) => {}
            (_, true, true) => {
                info!(
                    "strategy load: sidecar present with windows, delegating spawn to layout {:?}",
                    sidecar_path
                );
                layout_ev.write(LayoutLoadRequested {
                    path: sidecar_path,
                    mode: LayoutLoadMode::ApplySidecarForPy,
                });
            }
            (_, true, false) => {
                info!(
                    "strategy load: sidecar present but scenario-only (no windows), spawning fragments directly and firing layout for scenario metadata {:?}",
                    sidecar_path
                );
                for (key, body) in &outcome.fragments {
                    spawn_ev.write(PanelSpawnRequested {
                        kind: PanelKind::StrategyEditor,
                        source: PanelSpawnSource::LayoutLoad,
                        strategy_spec: Some(StrategyEditorSpawnSpec {
                            region_key: Some(key.clone()),
                            source: Some(body.clone()),
                            layout_source: PanelSpawnSource::LayoutLoad,
                        }),
                    });
                }
                layout_ev.write(LayoutLoadRequested {
                    path: sidecar_path,
                    mode: LayoutLoadMode::ApplySidecarForPy,
                });
            }
            (_, false, _) => {
                for (key, body) in &outcome.fragments {
                    spawn_ev.write(PanelSpawnRequested {
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

pub fn log_strategy_run_requested_system(mut events: MessageReader<StrategyRunRequested>) {
    for event in events.read() {
        info!("strategy run requested: {:?}", event.cache_path);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_strategy_run_system(
    mut events: MessageReader<StrategyRunRequested>,
    mut step_events: MessageReader<StepFromIdleRequested>,
    scenario: Res<ScenarioMetadata>,
    sender: Option<Res<TransportCommandSender>>,
    registry: Res<InstrumentRegistry>,
    paths: Res<ScenarioWritebackPaths>,
    mut progress: ResMut<ReplayStartupProgress>,
    trading_data: Res<TradingSession>,
    real_time: Res<Time<Real>>,
    mut current_run: ResMut<CurrentRun>,
    startup_params: Res<ScenarioStartupParams>,
) {
    for event in events.read() {
        if startup_params.errors.any() {
            error!("Run blocked: scenario startup params have errors");
            continue;
        }
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

        // 計画書 §3.5: Run 直前 inline flush。
        if registry.editable {
            if let Err(e) =
                flush_sidecars_now(registry.as_slice(), None, paths.cache_sidecar.as_deref())
            {
                error!("Run blocked: sidecar flush failed: {}", e);
                continue;
            }
        }

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

        let startup_id = progress.next_startup_id;

        let cmd = TransportCommand::RunStrategy {
            strategy_file: event.cache_path.clone(),
            config: run_config,
            startup_id,
        };

        let Some(sender) = sender.as_ref() else {
            error!("RunStrategy: TransportCommandSender is None — backend not connected");
            continue;
        };
        if let Err(e) = sender.tx.send(cmd) {
            error!("failed to send RunStrategy command: {}", e);
            continue;
        }

        // 送信成功後に progress を更新（失敗時は触らない）
        let detail = event
            .cache_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        progress.next_startup_id = startup_id.wrapping_add(1);
        progress.startup_id = startup_id;
        progress.visible = true;
        progress.phase = ReplayStartupPhase::CommandAccepted;
        progress.detail = detail;
        progress.error = None;
        progress.started_at_elapsed = Some(real_time.elapsed());
        progress.baseline_timestamp_ms = Some(trading_data.timestamp_ms);
        progress.start_engine_accepted = false;
        current_run.state = RunState::Running;
    }

    // #61: IDLE から ▶| を押したときの StepFromIdleRequested → LoadAndStep
    for event in step_events.read() {
        if startup_params.errors.any() {
            error!("StepFromIdle blocked: scenario startup params have errors");
            continue;
        }
        if scenario.instruments.is_empty() {
            error!("StepFromIdle blocked: SCENARIO has no instruments");
            continue;
        }
        let Some(ref start) = scenario.start else {
            error!("StepFromIdle blocked: SCENARIO has no start date");
            continue;
        };
        let Some(ref end) = scenario.end else {
            error!("StepFromIdle blocked: SCENARIO has no end date");
            continue;
        };
        let Some(ref granularity) = scenario.granularity else {
            error!("StepFromIdle blocked: SCENARIO has no granularity");
            continue;
        };

        if registry.editable {
            if let Err(e) =
                flush_sidecars_now(registry.as_slice(), None, paths.cache_sidecar.as_deref())
            {
                error!("StepFromIdle blocked: sidecar flush failed: {}", e);
                continue;
            }
        }

        let run_config = StrategyRunConfig {
            instruments: scenario.instruments.clone(),
            start: start.clone(),
            end: end.clone(),
            granularity: granularity.clone(),
            initial_cash: scenario.initial_cash,
        };

        let startup_id = progress.next_startup_id;
        let cmd = TransportCommand::LoadAndStep {
            config: run_config,
            startup_id,
        };

        let Some(sender) = sender.as_ref() else {
            error!("LoadAndStep: TransportCommandSender is None — backend not connected");
            continue;
        };
        if let Err(e) = sender.tx.send(cmd) {
            error!("failed to send LoadAndStep command: {}", e);
            continue;
        }

        let detail = event
            .cache_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        progress.next_startup_id = startup_id.wrapping_add(1);
        progress.startup_id = startup_id;
        progress.visible = true;
        progress.phase = ReplayStartupPhase::CommandAccepted;
        progress.detail = detail;
        progress.error = None;
        progress.started_at_elapsed = Some(real_time.elapsed());
        progress.baseline_timestamp_ms = Some(trading_data.timestamp_ms);
        progress.start_engine_accepted = false;
        current_run.state = RunState::Running;
    }
}

pub(crate) fn sync_to_cache(original_py: &std::path::Path) -> std::io::Result<()> {
    // `.py` 以外（cache 復元直後の replay 突入で渡る scenario `.json` sidecar 等）を
    // app_state.py に copy すると cache を JSON で自己破壊する（i15）。Python ソース以外は何もしない。
    if original_py.extension().and_then(|e| e.to_str()) != Some("py") {
        warn!(
            "sync_to_cache: refusing to sync non-.py source {:?} into app_state.py",
            original_py
        );
        return Ok(());
    }
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
    use crate::ui::components::ScenarioStartupParamsErrors;

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

    // --- Step D tests ---

    fn make_valid_scenario() -> ScenarioMetadata {
        ScenarioMetadata {
            schema_version: Some(1),
            instruments: vec!["7203.TSE".to_string()],
            start: Some("2024-01-01".to_string()),
            end: Some("2024-01-02".to_string()),
            granularity: Some("1m".to_string()),
            initial_cash: Some(1_000_000),
        }
    }

    fn build_app_for_run(
        scenario: ScenarioMetadata,
        with_sender: bool,
    ) -> (
        App,
        Option<tokio::sync::mpsc::UnboundedReceiver<TransportCommand>>,
    ) {
        let mut app = App::new();
        app.init_resource::<Time<Real>>();
        app.insert_resource(scenario);
        app.insert_resource(InstrumentRegistry::default());
        app.insert_resource(ScenarioWritebackPaths::default());
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<ScenarioStartupParams>();
        app.insert_resource(TradingSession::default());
        app.insert_resource(CurrentRun::default());
        app.add_message::<StrategyRunRequested>();
        app.add_message::<StepFromIdleRequested>();
        app.add_systems(Update, handle_strategy_run_system);

        let rx = if with_sender {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();
            app.insert_resource(TransportCommandSender { tx });
            Some(rx)
        } else {
            None
        };
        (app, rx)
    }

    #[test]
    fn test_handle_strategy_run_writes_progress_on_send_success() {
        let (mut app, mut rx) = build_app_for_run(make_valid_scenario(), true);
        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/foo.py"),
        });
        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(progress.visible, "progress should be visible after success");
        assert_eq!(progress.phase, ReplayStartupPhase::CommandAccepted);
        assert_eq!(progress.startup_id, 0);
        assert_eq!(progress.next_startup_id, 1);
        assert!(progress.error.is_none());
        assert!(!progress.start_engine_accepted);

        let current_run = app.world().resource::<CurrentRun>();
        assert!(matches!(current_run.state, RunState::Running));

        let rx = rx.as_mut().unwrap();
        let cmd = rx.try_recv().expect("RunStrategy command should be sent");
        match cmd {
            TransportCommand::RunStrategy { startup_id, .. } => {
                assert_eq!(startup_id, 0);
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn test_handle_strategy_run_no_sender_keeps_progress_idle() {
        let (mut app, _rx) = build_app_for_run(make_valid_scenario(), false);
        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/foo.py"),
        });
        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);

        let current_run = app.world().resource::<CurrentRun>();
        assert!(matches!(current_run.state, RunState::Idle));
    }

    #[test]
    fn test_handle_strategy_run_invalid_scenario_keeps_progress_idle() {
        let (mut app, _rx) = build_app_for_run(ScenarioMetadata::default(), true);
        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/foo.py"),
        });
        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);
    }

    #[test]
    fn handle_strategy_run_system_blocks_when_startup_params_have_errors() {
        let (mut app, mut rx) = build_app_for_run(make_valid_scenario(), true);
        // errors を設定して Run を block させる
        app.world_mut()
            .resource_mut::<ScenarioStartupParams>()
            .errors = ScenarioStartupParamsErrors {
            granularity: Some("unknown granularity".to_string()),
            ..Default::default()
        };
        app.world_mut().write_message(StrategyRunRequested {
            cache_path: std::path::PathBuf::from("/tmp/foo.py"),
        });
        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible, "Run must be blocked when errors.any()");
        assert_eq!(progress.phase, ReplayStartupPhase::Idle);

        let rx = rx.as_mut().unwrap();
        assert!(
            rx.try_recv().is_err(),
            "no RunStrategy command should be sent"
        );

        let current_run = app.world().resource::<CurrentRun>();
        assert!(matches!(current_run.state, RunState::Idle));
    }

    // Venue menu gating: while is_venue_busy_for_menu(state) is true, all
    // Connect items (both venues) become disabled. menu_item_system applies
    // the same guard at press time so the gRPC call is also suppressed.

    use crate::trading::{VenueState, VenueStatusRes};

    fn build_app_for_menu_gating(
        state: VenueState,
        venue_id: Option<&str>,
    ) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<OpenMenu>();
        app.insert_resource(VenueStatusRes {
            state,
            venue_id: venue_id.map(|s| s.to_string()),
            instruments_loaded: 0,
            configured_venue: None,
        });
        app.add_systems(Update, gate_venue_menu_items_system);

        let text_t = app
            .world_mut()
            .spawn((
                Text::new("Connect Tachibana (Demo)"),
                TextColor(TEXT_NORMAL),
            ))
            .id();
        let btn_t = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                BackgroundColor(BTN_NORMAL),
                MenuItem::VenueConnectTachibanaDemo,
            ))
            .add_child(text_t)
            .id();

        let text_k = app
            .world_mut()
            .spawn((
                Text::new("Connect kabuStation (Verify)"),
                TextColor(TEXT_NORMAL),
            ))
            .id();
        let btn_k = app
            .world_mut()
            .spawn((
                Button,
                Interaction::None,
                BackgroundColor(BTN_NORMAL),
                MenuItem::VenueConnectKabuVerify,
            ))
            .add_child(text_k)
            .id();

        app.update();
        (app, btn_t, btn_k)
    }

    #[test]
    fn test_gate_venue_menu_disables_kabu_when_tachibana_authenticating() {
        let (app, _btn_t, btn_k) =
            build_app_for_menu_gating(VenueState::Authenticating, Some("tachibana"));
        let bg = app.world().get::<BackgroundColor>(btn_k).unwrap();
        assert_eq!(
            bg.0, BTN_DISABLED,
            "kabu Connect should be disabled while tachibana is AUTHENTICATING"
        );
    }

    #[test]
    fn test_gate_venue_menu_disables_kabu_when_tachibana_connected() {
        let (app, _btn_t, btn_k) =
            build_app_for_menu_gating(VenueState::Connected, Some("tachibana"));
        let bg = app.world().get::<BackgroundColor>(btn_k).unwrap();
        assert_eq!(
            bg.0, BTN_DISABLED,
            "kabu Connect should be disabled while tachibana is CONNECTED"
        );
    }

    #[test]
    fn test_gate_venue_menu_disables_same_venue_when_authenticating() {
        let (app, btn_t, _btn_k) =
            build_app_for_menu_gating(VenueState::Authenticating, Some("tachibana"));
        let bg = app.world().get::<BackgroundColor>(btn_t).unwrap();
        assert_eq!(
            bg.0, BTN_DISABLED,
            "same-venue Connect (tachibana) must also be disabled while AUTHENTICATING"
        );
    }

    #[test]
    fn test_gate_venue_menu_enables_all_when_disconnected() {
        let (app, btn_t, btn_k) = build_app_for_menu_gating(VenueState::Disconnected, None);
        let bg_t = app.world().get::<BackgroundColor>(btn_t).unwrap();
        let bg_k = app.world().get::<BackgroundColor>(btn_k).unwrap();
        assert_eq!(
            bg_t.0, BTN_NORMAL,
            "tachibana Connect should be normal when DISCONNECTED"
        );
        assert_eq!(
            bg_k.0, BTN_NORMAL,
            "kabu Connect should be normal when DISCONNECTED"
        );
    }

    fn build_app_for_menu_press(
        state: VenueState,
        item: MenuItem,
    ) -> (
        App,
        Entity,
        tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    ) {
        let mut app = App::new();
        app.init_resource::<OpenMenu>();
        app.insert_resource(VenueStatusRes {
            state,
            venue_id: Some("tachibana".to_string()),
            instruments_loaded: 0,
            configured_venue: None,
        });
        app.insert_resource(ExecutionModeRes::default());
        app.add_message::<LayoutSaveRequested>();
        app.add_message::<LayoutSaveAsRequested>();
        app.add_message::<LayoutLoadDialogRequested>();
        app.add_message::<UndoMenuRequested>();
        app.add_message::<RedoMenuRequested>();
        app.add_message::<PanelSpawnRequested>();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<TransportCommand>();
        app.insert_resource(TransportCommandSender { tx });

        app.add_systems(Update, menu_item_system);

        let btn = app
            .world_mut()
            .spawn((
                Button,
                Interaction::Pressed,
                BackgroundColor(BTN_NORMAL),
                item,
            ))
            .id();

        app.update();
        (app, btn, rx)
    }

    #[test]
    fn test_venue_connect_pressed_during_other_venue_busy_is_ignored() {
        let (_app, _btn, mut rx) =
            build_app_for_menu_press(VenueState::Connected, MenuItem::VenueConnectKabuVerify);
        assert!(
            rx.try_recv().is_err(),
            "VenueConnect on opposite venue must NOT send VenueLogin when slot is busy"
        );
    }

    #[test]
    fn test_venue_connect_same_venue_pressed_during_authenticating_is_ignored() {
        let (_app, _btn, mut rx) = build_app_for_menu_press(
            VenueState::Authenticating,
            MenuItem::VenueConnectTachibanaDemo,
        );
        assert!(
            rx.try_recv().is_err(),
            "VenueConnect on same venue must NOT send VenueLogin while AUTHENTICATING"
        );
    }

    fn build_app_for_hide(configured: Option<&str>) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.insert_resource(VenueStatusRes {
            configured_venue: configured.map(|s| s.to_string()),
            ..Default::default()
        });
        app.add_systems(Update, hide_unconfigured_venue_items_system);
        let btn_t = app
            .world_mut()
            .spawn((Button, Node::default(), MenuItem::VenueConnectTachibanaDemo))
            .id();
        let btn_k = app
            .world_mut()
            .spawn((Button, Node::default(), MenuItem::VenueConnectKabuVerify))
            .id();
        app.update();
        (app, btn_t, btn_k)
    }

    #[test]
    fn test_hide_unconfigured_none_shows_both() {
        let (app, btn_t, btn_k) = build_app_for_hide(None);
        assert_eq!(
            app.world().get::<Node>(btn_t).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(btn_k).unwrap().display,
            Display::Flex
        );
    }

    #[test]
    fn test_hide_unconfigured_tachibana_hides_kabu() {
        let (app, btn_t, btn_k) = build_app_for_hide(Some("TACHIBANA"));
        assert_eq!(
            app.world().get::<Node>(btn_t).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(btn_k).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn test_hide_unconfigured_kabu_hides_tachibana() {
        let (app, btn_t, btn_k) = build_app_for_hide(Some("KABU"));
        assert_eq!(
            app.world().get::<Node>(btn_k).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(btn_t).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn test_hide_unconfigured_restores_when_cleared() {
        let (mut app, _btn_t, btn_k) = build_app_for_hide(Some("TACHIBANA"));
        assert_eq!(
            app.world().get::<Node>(btn_k).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<VenueStatusRes>()
            .configured_venue = None;
        app.update();
        assert_eq!(
            app.world().get::<Node>(btn_k).unwrap().display,
            Display::Flex
        );
    }

    #[test]
    fn test_help_settings_spawns_modal() {
        let (mut app, _btn, _rx) =
            build_app_for_menu_press(VenueState::Disconnected, MenuItem::HelpSettings);
        let count = app
            .world_mut()
            .query_filtered::<Entity, With<SettingsModalRoot>>()
            .iter(app.world())
            .count();
        assert_eq!(count, 1, "HelpSettings press must spawn exactly one SettingsModalRoot");
    }

    #[test]
    fn test_help_settings_dedup_prevents_second_spawn() {
        let (mut app, _btn, _rx) =
            build_app_for_menu_press(VenueState::Disconnected, MenuItem::HelpSettings);
        let count = app
            .world_mut()
            .query_filtered::<Entity, With<SettingsModalRoot>>()
            .iter(app.world())
            .count();
        assert_eq!(count, 1, "after first press, exactly one SettingsModalRoot expected");

        // 2 回目: Changed<Interaction> を確実に発火させるため新エンティティで模擬
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            BackgroundColor(BTN_NORMAL),
            MenuItem::HelpSettings,
        ));
        app.update();

        let count = app
            .world_mut()
            .query_filtered::<Entity, With<SettingsModalRoot>>()
            .iter(app.world())
            .count();
        assert_eq!(
            count, 1,
            "dedup guard must prevent second SettingsModalRoot from spawning"
        );
    }
}
