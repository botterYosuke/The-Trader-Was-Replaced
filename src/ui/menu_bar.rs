use crate::trading::{StrategyRunConfig, TransportCommand, TransportCommandSender};
use crate::ui::app_state::{load_app_state, save_app_state, AppState};
use crate::ui::components::ScenarioMetadata;
use crate::ui::components::{
    MenuBarRoot, MenuButton, OpenStrategyRequested, StrategyBuffer, StrategyRunRequested,
    StrategyStatusLabel,
};
use crate::ui::layout_persistence::{
    LayoutLoadDialogRequested, LayoutSaveAsRequested, LayoutSaveRequested,
};
use bevy::prelude::*;
use rfd::FileDialog;
use sha2::{Digest, Sha256};

const BTN_NORMAL: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BTN_HOVER: Color = Color::srgba(0.20, 0.20, 0.30, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.30, 0.30, 0.48, 1.0);

fn spawn_menu_btn(parent: &mut ChildBuilder, label: &str, action: MenuButton) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
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
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.07, 0.11, 0.95)),
            MenuBarRoot,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("File"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.65, 0.65, 0.65)),
            ));

            spawn_menu_btn(p, "Open Strategy...", MenuButton::OpenStrategy);
            spawn_menu_btn(p, "Save (Ctrl+S)", MenuButton::SaveLayout);
            spawn_menu_btn(p, "Save As (Ctrl+Shift+S)", MenuButton::SaveLayoutAs);
            spawn_menu_btn(p, "Load (Ctrl+O)", MenuButton::LoadLayout);

            p.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

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

#[allow(clippy::type_complexity)]
pub fn menu_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &MenuButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut open_strategy_events: EventWriter<OpenStrategyRequested>,
    mut save_ev: EventWriter<LayoutSaveRequested>,
    mut save_as_ev: EventWriter<LayoutSaveAsRequested>,
    mut load_ev: EventWriter<LayoutLoadDialogRequested>,
) {
    for (interaction, mut bg, action) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                match action {
                    MenuButton::OpenStrategy => {
                        info!("menu: open strategy requested");
                        if let Some(path) = FileDialog::new()
                            .add_filter("Python strategy", &["py"])
                            .set_directory("python/tests/data")
                            .pick_file()
                        {
                            info!("menu: selected strategy: {:?}", path);
                            open_strategy_events.send(OpenStrategyRequested { path });
                        } else {
                            info!("menu: open strategy canceled");
                        }
                    }
                    MenuButton::SaveLayout => {
                        info!("menu: save layout requested");
                        save_ev.send(LayoutSaveRequested);
                    }
                    MenuButton::SaveLayoutAs => {
                        info!("menu: save layout as requested");
                        save_as_ev.send(LayoutSaveAsRequested);
                    }
                    MenuButton::LoadLayout => {
                        info!("menu: load layout requested");
                        load_ev.send(LayoutLoadDialogRequested);
                    }
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

/// 起動時に app_state.json を読み、`last_strategy_path` が存在すれば
/// `OpenStrategyRequested` を発火してストラテジーを自動復元する。
/// Startup schedule で 1 回だけ実行される。
pub fn restore_last_strategy_system(mut events: EventWriter<OpenStrategyRequested>) {
    let state = load_app_state();
    if let Some(path) = state.last_strategy_path {
        if path.exists() {
            info!("restore_last_strategy: firing OpenStrategyRequested for {:?}", path);
            events.send(OpenStrategyRequested { path });
        } else {
            info!("restore_last_strategy: path {:?} not found, skipping", path);
        }
    }
}

pub fn log_open_strategy_requested_system(mut events: EventReader<OpenStrategyRequested>) {
    for event in events.read() {
        info!("open strategy selected: {:?}", event.path);
    }
}

fn strategy_cache_path(original: &std::path::Path) -> Option<std::path::PathBuf> {
    let abs = original.canonicalize().ok()?;
    let hash_bytes = {
        let mut h = Sha256::new();
        h.update(abs.to_string_lossy().as_bytes());
        h.finalize()
    };
    let hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let prefix = &hash[..16];
    let filename = original.file_name()?.to_string_lossy();
    let cache_name = format!("{}__{}", prefix, filename);

    let dir = dirs::cache_dir()?
        .join("the-trader-was-replaced")
        .join("strategy_buffers");
    Some(dir.join(cache_name))
}

pub fn update_strategy_status_label_system(
    buffer: Res<StrategyBuffer>,
    mut query: Query<&mut Text, With<StrategyStatusLabel>>,
) {
    if !buffer.is_changed() {
        return;
    }

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
            let dirty = if buffer.dirty { " *" } else { "" };
            format!("strategy: {}{}{}", name, cache, dirty)
        }
        None => "strategy: none".to_string(),
    };

    for mut text in &mut query {
        text.0 = label.clone();
    }
}

pub fn open_strategy_buffer_system(
    mut events: EventReader<OpenStrategyRequested>,
    mut buffer: ResMut<StrategyBuffer>,
) {
    for event in events.read() {
        match std::fs::read_to_string(&event.path) {
            Ok(source) => {
                buffer.original_path = Some(event.path.clone());
                buffer.source = source.clone();
                buffer.dirty = false;

                match strategy_cache_path(&event.path) {
                    Some(cache_path) => {
                        let cache_dir = cache_path.parent().unwrap();
                        if let Err(err) = std::fs::create_dir_all(cache_dir) {
                            error!("failed to create cache dir {:?}: {}", cache_dir, err);
                            buffer.cache_path = None;
                        } else if let Err(err) = std::fs::write(&cache_path, &source) {
                            error!("failed to write cache file {:?}: {}", cache_path, err);
                            buffer.cache_path = None;
                        } else {
                            info!(
                                "strategy buffer loaded: original={:?}, cache={:?}, bytes={}",
                                event.path,
                                cache_path,
                                buffer.source.len()
                            );
                            // F7 対応: GUI Run は cache_path を backend に渡すため、
                            // backend の load_scenario(cache_path) が <hash>__foo.json を
                            // 探せるよう元 sidecar JSON を cache にもコピーする。
                            // stale cleanup → コピーの順序が重要（元 sidecar 削除後の再 Open でも安全）。
                            let original_sidecar = event.path.with_extension("json");
                            let cache_sidecar = cache_path.with_extension("json");
                            copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);
                            buffer.cache_path = Some(cache_path);
                        }
                    }
                    None => {
                        error!("failed to compute cache path for {:?}", event.path);
                        buffer.cache_path = None;
                    }
                }

                // app_state.json に last_strategy_path を永続化する
                let state = AppState {
                    last_strategy_path: Some(event.path.clone()),
                    ..AppState::default()
                };
                if let Err(e) = save_app_state(&state) {
                    error!("failed to save app_state: {e}");
                }
            }
            Err(err) => {
                error!("failed to read strategy file {:?}: {}", event.path, err);
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

        assert!(cache_sidecar.exists(), "cache sidecar should exist after copy");
        let content = std::fs::read_to_string(&cache_sidecar).unwrap();
        let orig_content = std::fs::read_to_string(&original_sidecar).unwrap();
        assert_eq!(content, orig_content, "cache sidecar content should match original");
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
        assert_eq!(content, updated, "cache sidecar should reflect updated original");
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
        assert!(cache_sidecar.exists(), "cache sidecar should exist after first copy");

        // 元 sidecar を削除してから再 Open を模倣
        std::fs::remove_file(&original_sidecar).unwrap();
        copy_sidecar_to_cache(&original_sidecar, &cache_sidecar);

        assert!(
            !cache_sidecar.exists(),
            "stale cache sidecar should be removed when original is deleted"
        );
    }
}
