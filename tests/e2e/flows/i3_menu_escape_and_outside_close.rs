//! I3 menu_escape_and_outside_close — メニュー表示中に Escape またはメニュー外クリックで
//! 開いているメニューが閉じることを保証する（kind:ui）。
//!
//! **このフローは現在 SKIP（未実装）。**
//!
//! `src/ui/menu_bar.rs` には Escape キーやメニュー外クリックで `OpenMenu` を閉じる system が存在しない。
//! 該当する関数名・キーワード（"Escape", "outside", "close_menu", "menu_escape"）を grep しても
//! ヒットなし（2026-05-21 時点）。
//!
//! 将来 production 側に実装が入ったら、以下の seam を使って E2E test を追加する:
//! - Escape: `ButtonInput<KeyCode>::just_pressed(KeyCode::Escape)` を検出する system を追加し、
//!   `OpenMenu(None)` をセットする。
//! - Outside click: `Pointer<Down>` on non-menu entities を観測し `OpenMenu(None)` をセットする。
//! どちらも headless で `ButtonInput` または `Pointer` イベント注入で駆動できる。
