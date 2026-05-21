//! I1 menu_click_open_close — メニューバーの File / Edit / Venue をクリックすると
//! 対応するメニューが開き、同じトップレベルまたは別メニュークリックで開閉状態が切り替わることを保証する（kind:ui）。
//!
//! テストでは menu button の `Interaction::Pressed` を注入し、`OpenMenu` と popup entity 表示を観測する。
//! `menu_top_level_system` は `Changed<Interaction>` でトリガーするため、
//! spawn 時に `Interaction::Pressed` をセットして 1 フレーム回すだけで駆動できる。

use bevy::prelude::*;

use backcast::ui::components::{MenuPopup, MenuTopLevel, OpenMenu};
use backcast::ui::menu_bar::menu_top_level_system;

#[test]
fn i1_menu_click_open_close() {
    let mut app = App::new();

    app.insert_resource(OpenMenu(None));
    app.add_systems(Update, menu_top_level_system);

    // File / Edit / Venue のトップレベルボタンと対応するポップアップを spawn する。
    // ポップアップは実 spawn_menu_bar と同じ Display::None 初期値にする。
    let popup_file = app
        .world_mut()
        .spawn((Node { display: Display::None, ..default() }, MenuPopup(MenuTopLevel::File)))
        .id();
    let popup_edit = app
        .world_mut()
        .spawn((Node { display: Display::None, ..default() }, MenuPopup(MenuTopLevel::Edit)))
        .id();
    let popup_venue = app
        .world_mut()
        .spawn((Node { display: Display::None, ..default() }, MenuPopup(MenuTopLevel::Venue)))
        .id();

    // ── Phase A: File ボタンを押す → File メニューが開く ──
    let btn_file = app
        .world_mut()
        .spawn((
            Button,
            Interaction::Pressed,
            BackgroundColor(Color::WHITE),
            MenuTopLevel::File,
        ))
        .id();
    app.update();

    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::File),
        "File ボタンを押すと OpenMenu::File になるはず"
    );

    // ── Phase B: 同じ File ボタンを再度押す → File メニューが閉じる ──
    // Interaction をリセットしてから再 Pressed にする（Changed<Interaction> を発火させるため）。
    app.world_mut()
        .entity_mut(btn_file)
        .get_mut::<Interaction>()
        .map(|mut i| *i = Interaction::None);
    app.update();
    app.world_mut()
        .entity_mut(btn_file)
        .get_mut::<Interaction>()
        .map(|mut i| *i = Interaction::Pressed);
    app.update();

    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        None,
        "File ボタンを再度押すと OpenMenu::None（閉じる）になるはず"
    );

    // ── Phase C: Edit ボタンを押す → Edit メニューが開く ──
    app.world_mut().spawn((
        Button,
        Interaction::Pressed,
        BackgroundColor(Color::WHITE),
        MenuTopLevel::Edit,
    ));
    app.update();

    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::Edit),
        "Edit ボタンを押すと OpenMenu::Edit になるはず"
    );

    // ── Phase D: Venue ボタンを押す → Venue メニューが開く ──
    app.world_mut().spawn((
        Button,
        Interaction::Pressed,
        BackgroundColor(Color::WHITE),
        MenuTopLevel::Venue,
    ));
    app.update();

    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::Venue),
        "Venue ボタンを押すと OpenMenu::Venue になるはず"
    );

    // ポップアップ entity 自体が存在することを確認（表示制御は sync_menu_popup_visibility_system 担当）。
    assert!(
        app.world().get_entity(popup_file).is_ok(),
        "File popup entity が存在するはず"
    );
    assert!(
        app.world().get_entity(popup_edit).is_ok(),
        "Edit popup entity が存在するはず"
    );
    assert!(
        app.world().get_entity(popup_venue).is_ok(),
        "Venue popup entity が存在するはず"
    );
}
