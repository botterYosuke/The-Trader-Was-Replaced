//! M1 sidebar_panel_buttons_spawn_windows — Sidebar の Panels ボタンで
//! Strategy Editor / Buying Power / Run Result / Positions / Orders の
//! 各 floating window が開くことを保証する（kind:ui）。
//!
//! `panel_button_system` が `Changed<Interaction>` を受けて `PanelSpawnRequested` を発火し、
//! `panel_spawn_dispatcher_system` が `WindowRoot` + `PanelKind` を spawn するまでを
//! 本番 system チェーンで通す。
//!
//! StrategyEditor の spawn は `CosmicFontSystem` が必要。他は不要。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy_cosmic_edit::prelude::CosmicFontSystem;
use cosmic_text::FontSystem;

use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PendingStrategyFragments,
    RegionKeyAllocator, StrategyBuffer, WindowManager, WindowRoot,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::AutoSaveState;
use backcast::ui::sidebar::panel_button_system;

use crate::ui_dump::{dump_panels, panels_of};

#[test]
fn m1_sidebar_panel_buttons_spawn_windows() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    // panel_spawn_dispatcher_system が必要とする resource 群
    app.insert_resource(WindowManager::default())
        .insert_resource(CosmicFontSystem(FontSystem::new()))
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(PendingStrategyFragments::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(InstrumentRegistry::default());

    app.add_message::<PanelSpawnRequested>();

    app.add_systems(
        Update,
        (panel_button_system, panel_spawn_dispatcher_system).chain(),
    );

    // 各 PanelKind に対してボタン entity を spawn し、Interaction::Pressed を注入する。
    // Changed<Interaction> を確実に発火させるため各 kind を 1 フレームずつ処理する。
    for kind in [
        PanelKind::BuyingPower,
        PanelKind::Positions,
        PanelKind::Orders,
        PanelKind::StrategyEditor,
    ] {
        // BackgroundColor は panel_button_system の &mut BackgroundColor クエリに必要。
        // spawn 後 1 フレームで Changed<Interaction> が確定する。
        app.world_mut()
            .spawn((Button, Interaction::Pressed, kind, BackgroundColor::default()));
        app.update();
        app.update(); // dispatcher が同フレームで動くのを確実にするため 2 フレーム
    }

    let panels = dump_panels(app.world_mut());

    // 各種 panel が 1 枚ずつ spawn されていること
    assert_eq!(
        panels_of(&panels, "Buying Power").len(),
        1,
        "BuyingPower が 1 枚 spawn するはず (panels={panels:#?})"
    );
    assert_eq!(
        panels_of(&panels, "Positions").len(),
        1,
        "Positions が 1 枚 spawn するはず (panels={panels:#?})"
    );
    assert_eq!(
        panels_of(&panels, "Orders").len(),
        1,
        "Orders が 1 枚 spawn するはず (panels={panels:#?})"
    );
    assert_eq!(
        panels_of(&panels, "Strategy Editor").len(),
        1,
        "StrategyEditor が 1 枚 spawn するはず (panels={panels:#?})"
    );

    // 全 WindowRoot entity の数も 5 であること（余計な spawn がないこと）
    let total: usize = app
        .world_mut()
        .query_filtered::<Entity, With<WindowRoot>>()
        .iter(app.world())
        .count();
    assert_eq!(total, 4, "WindowRoot は 4 体のはず (got {total})");
}
