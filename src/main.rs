use bevy::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};
use bevy_prototype_lyon::prelude::*;
use rand::Rng;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Trader Dashboard - Infinite Canvas".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(PanCamPlugin)
        .add_plugins(ShapePlugin)
        .insert_resource(TradingData::default())
        .add_systems(Startup, (setup_camera, setup_ui))
        .add_systems(Update, (
            price_simulation_system,
            chart_update_system,
            ui_update_system,
            grid_system,
        ))
        .run();
}

#[derive(Resource)]
struct TradingData {
    price: f32,
    history: Vec<f32>,
    timer: Timer,
}

impl Default for TradingData {
    fn default() -> Self {
        Self {
            price: 100.0,
            history: vec![100.0],
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
        }
    }
}

#[derive(Component)]
struct WindowRoot;

#[derive(Component)]
struct TitleBar;

#[derive(Component)]
struct PriceDisplay;

#[derive(Component)]
struct ChartLine;

#[derive(Component, Clone, Copy)]
enum TradeButton {
    Buy,
    Sell,
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PanCam {
            grab_buttons: vec![MouseButton::Right, MouseButton::Middle],
            ..default()
        },
    ));
}

fn setup_ui(
    mut commands: Commands,
) {
    // Window Root
    let window_id = commands.spawn((
        Sprite {
            color: Color::srgba(0.05, 0.05, 0.1, 0.9),
            custom_size: Some(Vec2::new(400.0, 500.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        WindowRoot,
    )).id();

    // Title Bar (Draggable area)
    let title_bar = commands.spawn((
        Sprite {
            color: Color::srgba(0.1, 0.1, 0.2, 1.0),
            custom_size: Some(Vec2::new(400.0, 40.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 230.0, 1.0),
        TitleBar,
    ))
    .observe(|drag: Trigger<Pointer<Drag>>, mut query: Query<&mut Transform, With<WindowRoot>>, parent_query: Query<&Parent>| {
        // Dragging the title bar moves the whole window
        if let Ok(parent) = parent_query.get(drag.entity()) {
            if let Ok(mut transform) = query.get_mut(parent.get()) {
                transform.translation.x += drag.event().delta.x;
                transform.translation.y -= drag.event().delta.y;
            }
        }
    })
    .id();

    // Title Text
    let title_text = commands.spawn((
        Text2d::new("TRADER DASHBOARD"),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, 1.0),
    )).id();

    commands.entity(title_bar).add_child(title_text);

    // Price Display
    let price_text = commands.spawn((
        Text2d::new("$100.00"),
        TextFont {
            font_size: 60.0,
            ..default()
        },
        TextColor(Color::srgb(0.0, 1.0, 0.5)),
        Transform::from_xyz(0.0, 120.0, 1.0),
        PriceDisplay,
    )).id();

    // Chart Area
    let chart_line = commands.spawn((
        ShapeBundle {
            path: GeometryBuilder::build_as(&lines(&[Vec2::ZERO, Vec2::ZERO])),
            transform: Transform::from_xyz(-180.0, -50.0, 1.0),
            ..default()
        },
        Stroke::new(Color::srgb(0.0, 0.8, 1.0), 2.0),
        ChartLine,
    )).id();

    // Buttons
    let buy_button = spawn_button(&mut commands, "BUY", Color::srgb(0.0, 0.8, 0.4), Vec2::new(-80.0, -180.0), TradeButton::Buy);
    let sell_button = spawn_button(&mut commands, "SELL", Color::srgb(0.8, 0.2, 0.2), Vec2::new(80.0, -180.0), TradeButton::Sell);

    commands.entity(window_id).add_child(title_bar);
    commands.entity(window_id).add_child(price_text);
    commands.entity(window_id).add_child(chart_line);
    commands.entity(window_id).add_child(buy_button);
    commands.entity(window_id).add_child(sell_button);
}

fn spawn_button(
    commands: &mut Commands,
    label: &str,
    color: Color,
    position: Vec2,
    action: TradeButton,
) -> Entity {
    let btn = commands.spawn((
        Sprite {
            color,
            custom_size: Some(Vec2::new(120.0, 50.0)),
            ..default()
        },
        Transform::from_xyz(position.x, position.y, 1.0),
        action,
    ))
    .observe(move |_: Trigger<Pointer<Click>>, mut data: ResMut<TradingData>| {
        match action {
            TradeButton::Buy => data.price += 1.5,
            TradeButton::Sell => data.price -= 1.5,
        }
    })
    .id();

    let text = commands.spawn((
        Text2d::new(label),
        TextFont {
            font_size: 24.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, 1.0),
    )).id();

    commands.entity(btn).add_child(text);
    btn
}

fn price_simulation_system(
    time: Res<Time>,
    mut data: ResMut<TradingData>,
) {
    data.timer.tick(time.delta());
    if data.timer.just_finished() {
        let mut rng = rand::thread_rng();
        let change = rng.gen_range(-0.5..0.6);
        data.price += change;
        let price = data.price;
        data.history.push(price);
        if data.history.len() > 50 {
            data.history.remove(0);
        }
    }
}

fn chart_update_system(
    data: Res<TradingData>,
    mut query: Query<&mut Path, With<ChartLine>>,
) {
    if data.is_changed() {
        for mut path in &mut query {
            if data.history.len() < 2 { continue; }

            let max_price = data.history.iter().cloned().fold(f32::NEG_INFINITY, f32::max).max(105.0);
            let min_price = data.history.iter().cloned().fold(f32::INFINITY, f32::min).min(95.0);
            let range = (max_price - min_price).max(1.0);

            let x_step = 360.0 / (data.history.len() - 1) as f32;
            let mut points = Vec::new();

            for (i, &p) in data.history.iter().enumerate() {
                let x = i as f32 * x_step;
                let y = (p - min_price) / range * 150.0;
                points.push(Vec2::new(x, y));
            }

            *path = GeometryBuilder::build_as(&lines(&points));
        }
    }
}

fn ui_update_system(
    data: Res<TradingData>,
    mut query: Query<(&mut Text2d, &mut TextColor), With<PriceDisplay>>,
) {
    for (mut text, mut color) in &mut query {
        text.0 = format!("${:.2}", data.price);
        color.0 = if data.price >= 100.0 {
            Color::srgb(0.0, 1.0, 0.5)
        } else {
            Color::srgb(1.0, 0.2, 0.2)
        };
    }
}

fn lines(points: &[Vec2]) -> Path {
    let mut path_builder = PathBuilder::new();
    if points.len() >= 2 {
        path_builder.move_to(points[0]);
        for &p in &points[1..] {
            path_builder.line_to(p);
        }
    }
    path_builder.build()
}

fn grid_system(mut gizmos: Gizmos) {
    let color = Color::srgba(0.1, 0.1, 0.2, 0.3);
    for i in -20..=20 {
        let x = i as f32 * 100.0;
        gizmos.line_2d(Vec2::new(x, -2000.0), Vec2::new(x, 2000.0), color);
        let y = i as f32 * 100.0;
        gizmos.line_2d(Vec2::new(-2000.0, y), Vec2::new(2000.0, y), color);
    }
}
