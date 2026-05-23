//! I14 save_without_path_falls_back_to_dialog — `original_path` 未確定の状態で Save (Ctrl+S) を
//! 実行すると、既存パスへ無言で書き込まず Save As 相当の保存ダイアログにフォールバックすることを
//! 保証する（kind:integration）。
//!
//! # シナリオ
//! 起動時に cache（`app_state.json` / `app_state.py`）から復元したが、`SidecarLayout.strategy_path`
//! が `None`（一度も実ファイルに Save していない）なら `apply_cache_restore_system`
//! （`src/ui/layout_persistence.rs:464`）は `StrategyBuffer.original_path = None` のまま復元する。
//! この状態で `LayoutSaveRequested`（Ctrl+S）を発火すると、`handle_save_layout_system`
//! （`src/ui/layout_persistence.rs:708-721`）は path 未確定分岐に入り、保存ダイアログを起動する。
//!
//! # SKIP: headless 不可（i8 と同 seam）
//!
//! path 未確定分岐は Save As (`handle_save_as_layout_system`) と同じく
//! `AsyncComputeTaskPool::get().spawn` で `rfd::AsyncFileDialog::save_file()` を起動する:
//!
//! ```ignore
//! // layout_persistence.rs:708-721
//! let Some(orig) = &buffer.original_path else {
//!     if pending.is_active() { continue; }
//!     let task = AsyncComputeTaskPool::get().spawn(async move {
//!         rfd::AsyncFileDialog::new()
//!             .add_filter("Layout JSON", &["json"])
//!             .save_file()
//!             .await
//!             .map(|h| h.path().to_path_buf())
//!     });
//!     pending.begin(FileDialogKind::Save, task);
//!     continue;
//! };
//! ```
//!
//! 外部からパスを注入できる seam が無く、i8（Save As）と同じ理由で headless 駆動できない。
//! さらに bare `App` には `TaskPoolPlugin` が無いため `AsyncComputeTaskPool::get()` が
//! 未初期化パニックし、pool を初期化すると今度は実 rfd ダイアログを開こうとする。
//!
//! ## 代替策（将来対応）
//! i8 と共通のリファクタで両方が headless テスト可能になる:
//! - path 未確定分岐を「`LayoutSaveAsRequested` を emit して Save As へ委譲」に変えると、
//!   `original_path = None` で Ctrl+S → `LayoutSaveAsRequested` が 1 回発火、を event 観測で assert できる。
//! - または「ダイアログ要求」と「パス確定後の書き込み」を別イベント / `Local<Option<PathBuf>>`
//!   override に分割する。
//!
//! ## 現状
//! このファイルはスタブとして残し、CI では `#[ignore]` 扱いとする。
//! path 確定済みの Save 書き込みは i7 が、Ctrl+S→`LayoutSaveRequested` の発火は i9 がカバー済み。

#[test]
#[ignore = "path 未確定 Save は rfd ダイアログを直接呼ぶため headless 駆動不可（i8 と同 seam, i14 SKIP）"]
fn i14_save_without_path_falls_back_to_dialog() {
    // このテストは headless 不可のためスキップする。
    // 詳細はファイル上部のコメントを参照。
}
