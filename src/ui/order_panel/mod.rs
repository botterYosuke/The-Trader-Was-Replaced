//! Phase 9 §3.9 — OrderPanel (LiveManual 専用、手動発注フォーム)。
//!
//! Slice 2: world-space sprite floating window (ORDER) 内のフォーム。
//! `panel_spawn_dispatcher_system` → `spawn_order_form_in_window` でコンテンツエリアを埋める。
//!
//! ボタン操作は `OrderButtonPressed` イベント経由（sprite の Pointer<Click> observer が送信）。
//!
//! 2 段階確認: `[発注]` で `OrderConfirm.pending` をセット → 中央オーバーレイの確認モーダルに
//! 内容 (銘柄/売買/数量/価格/概算約定額) を再表示 → `[Confirm]` で初めて
//! `TransportCommand::PlaceOrder` を発射する (§3.9)。
//!
//! 第二暗証番号 (Tachibana) は別モジュール `secret_modal.rs` が `SecretRequired` イベントで
//! 収集する。OrderPanel は `second_secret` を載せない (mock/kabu は不要、Tachibana は Step 5)。

use bevy::prelude::*;

mod confirm_modal;
mod form;

// ── 共有配色 (form / confirm_modal 双方が参照) ──────────────────────────────
// Colors are now sourced from Theme (see usage sites)

pub use confirm_modal::{
    ConfirmButton, ConfirmModalRoot, OrderConfirm, confirm_modal_button_system,
    confirm_modal_reconcile_system, confirm_modal_sync_system, confirm_modal_visibility_system,
    spawn_confirm_modal,
};
pub use form::{
    OrderButton, OrderButtonPressed, OrderDraft, OrderForm, OrderType, OrderValidationError,
    Side, TimeInForce, order_form_button_system, order_panel_sync_system,
    order_submit_button_system, order_window_despawn_system, spawn_order_form_in_window,
    validate_order,
};
