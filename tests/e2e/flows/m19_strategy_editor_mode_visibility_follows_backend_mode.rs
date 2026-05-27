//! M19 strategy_editor_mode_visibility_follows_backend_mode — strategy editor の
//! `apply_strategy_editor_mode_visibility_system` が **本番の mode writer（`status_update_system`）
//! 経由の backend mode 変化に同一フレームで追従する**ことを保証する（kind:state）。
//!
//! # 背景
//! `apply_strategy_editor_mode_visibility_system` は `is_changed()` ガード無しで毎フレーム
//! mode を読む。`.after(status_update_system)` 制約が無いため、同フレームで前に走ると
//! `status_update_system` が mode を書く前の古い値で判断し、1 フレーム遅延する。
//! （issue #41 codex review の Medium・strategy_editor.rs:172）
//!
//! M16・M18 同様、本フローは `status_update_system` 経由に `ExecutionModeChanged` を流し、
//! mode 確定と同一 tick で `PanelKind::StrategyEditor` の `Visibility` が反映されることを
//! 保証する。production の `.after(status_update_system)` と同じ順序制約をハーネス側にも
//! 張り、「mode 確定 → editor 可視性反映」が単一フレームで閉じる契約を固定する。
//!
//! ウィンドウ (`WindowRoot` の `Visibility`) に加え、サイドバー "Panels" の Strategy Editor
//! ボタン (`Button` + `PanelKind::StrategyEditor`) の `Node.display` 経路も同時に検証する
//! （`apply_strategy_editor_mode_visibility_system` の `btn_q` path。issue #41）。

use bevy::prelude::*;

use crate::support::Harness;
use backcast::backend_sync::status_update_system;
use backcast::trading::{BackendStatusUpdate, ExecutionMode};
use backcast::ui::components::{PanelKind, WindowRoot};
use backcast::ui::strategy_editor::apply_strategy_editor_mode_visibility_system;

#[test]
fn m19_strategy_editor_mode_visibility_follows_backend_mode() {
    let mut h = Harness::new();
    h.app.add_systems(
        Update,
        apply_strategy_editor_mode_visibility_system.after(status_update_system),
    );

    let editor = h
        .app
        .world_mut()
        .spawn((WindowRoot, PanelKind::StrategyEditor, Visibility::Inherited))
        .id();
    // サイドバー "Panels" の Strategy Editor ボタン（btn_q path）。
    let editor_btn = h
        .app
        .world_mut()
        .spawn((Button, PanelKind::StrategyEditor, Node::default()))
        .id();

    // 初期 Replay: editor もボタンも表示
    h.tick();
    assert_eq!(
        *h.app.world().get::<Visibility>(editor).unwrap(),
        Visibility::Inherited,
        "初期 Replay では Strategy Editor は表示されるはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(editor_btn).unwrap().display,
        Display::Flex,
        "初期 Replay ではサイドバーの Strategy Editor ボタンは表示されるはず"
    );

    // backend が LiveManual を echo → 同一 tick で Hidden
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::LiveManual,
    });
    assert_eq!(
        h.exec_mode().mode,
        ExecutionMode::LiveManual,
        "前提: status_update_system が mode を LiveManual に確定する"
    );
    assert_eq!(
        *h.app.world().get::<Visibility>(editor).unwrap(),
        Visibility::Hidden,
        "LiveManual echo と同一フレームで Strategy Editor は非表示になるはず（1 フレーム遅延しない）"
    );
    assert_eq!(
        h.app.world().get::<Node>(editor_btn).unwrap().display,
        Display::None,
        "LiveManual echo と同一フレームでサイドバーの Strategy Editor ボタンも非表示になるはず"
    );

    // Replay へ戻す → 同一フレームで退避値（Inherited）へ復元
    h.send_status(BackendStatusUpdate::ExecutionModeChanged {
        mode: ExecutionMode::Replay,
    });
    assert_eq!(
        *h.app.world().get::<Visibility>(editor).unwrap(),
        Visibility::Inherited,
        "Replay 復帰と同一フレームで Strategy Editor は再表示されるはず"
    );
    assert_eq!(
        h.app.world().get::<Node>(editor_btn).unwrap().display,
        Display::Flex,
        "Replay 復帰と同一フレームでサイドバーの Strategy Editor ボタンも再表示されるはず"
    );
}
