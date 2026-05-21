//! I2 menu_keyboard_alt_shortcuts — Alt+F / Alt+E / Alt+V で File / Edit / Venue メニューを
//! キーボードから開閉できることを保証する（kind:ui）。
//!
//! テストでは `ButtonInput<KeyCode>` に Alt と対象キーを注入し、本番 `menu_keyboard_system` が
//! `OpenMenu` をトグルすることを観測する。`menu_keyboard_system` は `ButtonInput<KeyCode>` と
//! `Events<KeyboardInput>` を読む（KeyboardInput は handled 時に clear するため ResMut 必須）。
//!
//! 注意: bare `App` には input プラグインが無く `ButtonInput` はフレーム境界で自動 clear されない。
//! `just_pressed` が前フレームから残り、既に pressed のキーへの再 `press()` は no-op になるため、
//! 各フェーズで `reset_all()` してから押し直して `just_pressed` を確実に作り直す。

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

use backcast::ui::components::{MenuTopLevel, OpenMenu};
use backcast::ui::menu_bar::menu_keyboard_system;

#[test]
fn i2_menu_keyboard_alt_shortcuts() {
    let mut app = App::new();

    app.insert_resource(OpenMenu(None));
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.add_event::<KeyboardInput>();
    app.add_systems(Update, menu_keyboard_system);

    // Alt を押しつつ `key` を just_pressed にして 1 フレーム回す。
    let press_alt = |app: &mut App, key: KeyCode| {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::AltLeft);
        keys.press(key);
        app.update();
    };

    // ── Alt+F → File が開く ──
    press_alt(&mut app, KeyCode::KeyF);
    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::File),
        "Alt+F で OpenMenu::File になるはず"
    );

    // ── Alt+F を再度 → File が閉じる（トグル）──
    press_alt(&mut app, KeyCode::KeyF);
    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        None,
        "Alt+F を再度押すと OpenMenu::None（閉じる）になるはず"
    );

    // ── Alt+E → Edit が開く ──
    press_alt(&mut app, KeyCode::KeyE);
    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::Edit),
        "Alt+E で OpenMenu::Edit になるはず"
    );

    // ── Alt+V → 開いていた Edit から Venue へ切り替わる ──
    press_alt(&mut app, KeyCode::KeyV);
    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::Venue),
        "Alt+V で OpenMenu::Venue に切り替わるはず"
    );

    // ── Alt 無しでは何も起きない（早期 return）──
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::KeyF);
        app.update();
    }
    assert_eq!(
        app.world().resource::<OpenMenu>().0,
        Some(MenuTopLevel::Venue),
        "Alt を押していなければトグルされず Venue のままのはず"
    );
}
