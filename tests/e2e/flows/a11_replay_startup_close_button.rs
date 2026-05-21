//! A11 replay_startup_close_button — Replay Startup のエラー/タイムアウト表示で Close を押すと、
//! エラー内容と進捗ウィンドウがユーザー操作で閉じることを保証する（kind:ui）。
//!
//! テストでは timeout / failed startup state と Close button interaction を注入し、`ReplayStartupProgress.visible` と error state を観測する。
