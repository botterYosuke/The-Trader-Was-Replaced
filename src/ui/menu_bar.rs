use bevy::prelude::*;
use rfd::FileDialog;
use sha2::{Digest, Sha256};
use crate::ui::components::{MenuBarRoot, MenuButton, OpenStrategyRequested, StrategyBuffer, StrategyStatusLabel, StrategyRunRequested};

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
                TextFont { font_size: 12.0, ..default() },
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
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.65, 0.65, 0.65)),
            ));

            spawn_menu_btn(p, "Open Strategy...", MenuButton::OpenStrategy);

            p.spawn(Node { flex_grow: 1.0, ..default() });

            p.spawn((
                Text::new("strategy: none"),
                TextFont { font_size: 12.0, ..default() },
                TextColor(Color::srgb(0.55, 0.55, 0.55)),
                StrategyStatusLabel,
            ));
        });
}

pub fn menu_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &MenuButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut open_strategy_events: EventWriter<OpenStrategyRequested>,
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
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}

pub fn log_open_strategy_requested_system(
    mut events: EventReader<OpenStrategyRequested>,
) {
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
            let cache = if buffer.cache_path.is_some() { " cached" } else { "" };
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
                            buffer.cache_path = Some(cache_path);
                        }
                    }
                    None => {
                        error!("failed to compute cache path for {:?}", event.path);
                        buffer.cache_path = None;
                    }
                }
            }
            Err(err) => {
                error!("failed to read strategy file {:?}: {}", event.path, err);
            }
        }
    }
}

pub fn log_strategy_run_requested_system(
    mut events: EventReader<StrategyRunRequested>,
) {
    for event in events.read() {
        info!("strategy run requested: {:?}", event.cache_path);
    }
}
