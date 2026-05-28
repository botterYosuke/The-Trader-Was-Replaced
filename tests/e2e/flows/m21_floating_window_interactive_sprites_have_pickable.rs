//! M21 floating_window_interactive_sprites_have_pickable — `spawn_floating_window` が生成する
//! インタラクティブな Sprite（title_bar / close_button / resize_handle / window_root）に
//! `Pickable` コンポーネントが付いていることを保証する（kind:ui）。
//!
//! Bevy 0.18 の `sprite_picking` バックエンドは `&Pickable` を必須クエリ引数とするため、
//! `Pickable` を持たない Sprite は picking 対象にならず `Pointer<Drag>` / `Pointer<Click>`
//! が一切発火しない。本 flow は issue #52 で発覚した「タイトルバードラッグ無反応」の
//! 再発を防ぐ構造回帰ガード。

use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{CloseButton, TitleBar, WindowManager, WindowRoot};
use backcast::ui::editor_history::AppHistory;
use backcast::ui::floating_window::{FloatingWindowChildren, FloatingWindowSpec, spawn_floating_window};

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);
    app.insert_resource(WindowManager::default());
    app.insert_resource(AppHistory::default());
    app
}

#[test]
fn m21_title_bar_has_pickable() {
    let mut app = build_app();
    {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "TEST".to_string(),
                size: Vec2::new(200.0, 150.0),
                position: Vec2::ZERO,
                accent: Color::WHITE,
                closeable: false,
                resizable: false,
            },
        );
    }
    app.world_mut().flush();

    let count = app
        .world_mut()
        .query_filtered::<Entity, (With<TitleBar>, With<Pickable>)>()
        .iter(app.world())
        .count();
    assert_eq!(count, 1, "TitleBar は Pickable を持つはず（Drag observer が機能するために必須）");
}

#[test]
fn m21_window_root_has_pickable() {
    let mut app = build_app();
    {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "TEST".to_string(),
                size: Vec2::new(200.0, 150.0),
                position: Vec2::ZERO,
                accent: Color::WHITE,
                closeable: false,
                resizable: false,
            },
        );
    }
    app.world_mut().flush();

    let count = app
        .world_mut()
        .query_filtered::<Entity, (With<WindowRoot>, With<Pickable>)>()
        .iter(app.world())
        .count();
    assert_eq!(count, 1, "WindowRoot は Pickable を持つはず（Press observer が機能するために必須）");
}

#[test]
fn m21_close_button_has_pickable() {
    let mut app = build_app();
    {
        let mut commands = app.world_mut().commands();
        spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "TEST".to_string(),
                size: Vec2::new(200.0, 150.0),
                position: Vec2::ZERO,
                accent: Color::WHITE,
                closeable: true,
                resizable: false,
            },
        );
    }
    app.world_mut().flush();

    let count = app
        .world_mut()
        .query_filtered::<Entity, (With<CloseButton>, With<Pickable>)>()
        .iter(app.world())
        .count();
    assert_eq!(count, 1, "CloseButton は Pickable を持つはず（Click observer が機能するために必須）");
}

#[test]
fn m21_resize_handles_have_pickable() {
    let mut app = build_app();
    let root = {
        let mut commands = app.world_mut().commands();
        let (root, _, _) = spawn_floating_window(
            &mut commands,
            FloatingWindowSpec {
                title: "TEST".to_string(),
                size: Vec2::new(200.0, 150.0),
                position: Vec2::ZERO,
                accent: Color::WHITE,
                closeable: false,
                resizable: true,
            },
        );
        root
    };
    app.world_mut().flush();

    let (resize_right, resize_bottom, resize_corner) = {
        let ch = app
            .world()
            .get::<FloatingWindowChildren>(root)
            .expect("root は FloatingWindowChildren を持つはず");
        (ch.resize_right, ch.resize_bottom, ch.resize_corner)
    };

    for (name, handle_opt) in [
        ("resize_right", resize_right),
        ("resize_bottom", resize_bottom),
        ("resize_corner", resize_corner),
    ] {
        let handle = handle_opt.unwrap_or_else(|| panic!("{name} は Some のはず（resizable:true）"));
        assert!(
            app.world().get::<Pickable>(handle).is_some(),
            "{name} は Pickable を持つはず（Drag/Over/Out observer が機能するために必須）",
        );
    }
}
