//! M5 panel_duplicate_policy — StrategyEditor は複数 region を開けるが、
//! BuyingPower / RunResult / Positions / Orders は同種 1 枚だけに制限されることを
//! 保証する（kind:ui）。
//!
//! `panel_button_system` の duplicate ガード:
//! ```
//! let allow_multi = matches!(kind, PanelKind::StrategyEditor);
//! if allow_multi || !existing_kinds.iter().any(|k| k == kind) {
//!     spawn_events.send(PanelSpawnRequested { … });
//! }
//! ```
//! を本番 system チェーンで通す。

use bevy::prelude::*;
use bevy::transform::TransformPlugin;

use backcast::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PendingStrategyFragments,
    RegionKeyAllocator, StrategyBuffer, WindowManager,
};
use backcast::ui::editor_history::{ActiveDrag, AppHistory};
use backcast::ui::floating_window::panel_spawn_dispatcher_system;
use backcast::ui::layout_persistence::AutoSaveState;
use backcast::ui::sidebar::panel_button_system;

use crate::ui_dump::{dump_panels, panels_of};

#[test]
fn m5_panel_duplicate_policy() {
    let mut app = App::new();
    app.add_plugins(TransformPlugin);

    app.insert_resource(WindowManager::default())
        .insert_resource(RegionKeyAllocator::default())
        .insert_resource(AppHistory::default())
        .insert_resource(ActiveDrag::default())
        .insert_resource(AutoSaveState::default())
        .insert_resource(PendingStrategyFragments::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(InstrumentRegistry::default());

    app.add_message::<PanelSpawnRequested>();

    app.add_systems(
        Update,
        (panel_button_system, panel_spawn_dispatcher_system).chain(),
    );

    // BackgroundColor は panel_button_system の &mut BackgroundColor クエリに必要。
    // ── BuyingPower を 2 回押す → 1 枚だけ spawn されること ──
    // 1 回目
    app.world_mut()
        .spawn((Button, Interaction::Pressed, PanelKind::BuyingPower, BackgroundColor::default()));
    app.update();
    app.update();
    // 2 回目（new entity を spawn してフレームを跨がせる）
    app.world_mut()
        .spawn((Button, Interaction::Pressed, PanelKind::BuyingPower, BackgroundColor::default()));
    app.update();
    app.update();

    let panels = dump_panels(app.world_mut());
    assert_eq!(
        panels_of(&panels, "Buying Power").len(),
        1,
        "BuyingPower は 2 回押しても 1 枚のはず (panels={panels:#?})"
    );

    // ── StrategyEditor を 2 回押す → 2 枚 spawn されること ──
    app.world_mut()
        .spawn((Button, Interaction::Pressed, PanelKind::StrategyEditor, BackgroundColor::default()));
    app.update();
    app.update();
    app.world_mut()
        .spawn((Button, Interaction::Pressed, PanelKind::StrategyEditor, BackgroundColor::default()));
    app.update();
    app.update();

    let panels = dump_panels(app.world_mut());
    assert_eq!(
        panels_of(&panels, "Strategy Editor").len(),
        2,
        "StrategyEditor は 2 回押すと 2 枚になるはず (panels={panels:#?})"
    );

    // ── StrategyEditor の 2 枚は region_key が異なること ──
    let editors = panels_of(&panels, "Strategy Editor");
    let key0 = editors[0].region_key.as_deref().expect("region_key があるはず");
    let key1 = editors[1].region_key.as_deref().expect("region_key があるはず");
    assert_ne!(
        key0, key1,
        "2 枚の StrategyEditor は異なる region_key を持つはず"
    );

    // ── RunResult / Positions / Orders も同様に singleton チェック ──
    for kind in [PanelKind::Positions, PanelKind::Orders] {
        app.world_mut()
            .spawn((Button, Interaction::Pressed, kind, BackgroundColor::default()));
        app.update();
        app.update();
        app.world_mut()
            .spawn((Button, Interaction::Pressed, kind, BackgroundColor::default()));
        app.update();
        app.update();
    }

    let panels = dump_panels(app.world_mut());
    assert_eq!(
        panels_of(&panels, "Positions").len(),
        1,
        "Positions は 1 枚のはず"
    );
    assert_eq!(
        panels_of(&panels, "Orders").len(),
        1,
        "Orders は 1 枚のはず"
    );
}
