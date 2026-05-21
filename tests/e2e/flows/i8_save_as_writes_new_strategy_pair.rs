//! I8 save_as_writes_new_strategy_pair — Save As が新しい `.json` / `.py` のペアを作成し、
//! 以後の original path と cache/writeback state を新しい保存先へ切り替えることを保証する（kind:integration）。
//!
//! # SKIP: headless 不可
//!
//! `handle_save_as_layout_system`（`src/ui/layout_persistence.rs` ~689行）は常に
//! `FileDialog::new().add_filter(...).save_file()` を呼ぶ。I7 の Save と異なり、
//! `original_path` の有無に関わらず**毎回** rfd ダイアログを開く設計になっている。
//!
//! ```
//! // layout_persistence.rs:689
//! let json_path = match FileDialog::new()
//!     .add_filter("Layout JSON", &["json"])
//!     .save_file()
//! {
//!     Some(p) => p,
//!     None => { continue; } // キャンセル扱い
//! };
//! ```
//!
//! 外部からパスを注入できる seam（イベント / リソース）が存在しないため、
//! headless テストとして駆動する手段がない。
//!
//! ## 代替策（将来対応）
//! `handle_save_as_layout_system` に `save_as_path_override: Local<Option<PathBuf>>`
//! のような headless seam を追加すれば駆動可能になる。
//! またはシステムを 2 段に分割し「ダイアログ要求」と「パス確定後の書き込み」を
//! 別イベントで繋ぐリファクタリングでも対応できる。
//!
//! ## 現状
//! このファイルはスタブとして残し、CI では `#[ignore]` 扱いとする。

#[test]
#[ignore = "Save As は rfd ダイアログを直接呼ぶため headless 駆動不可（i8 SKIP）"]
fn i8_save_as_writes_new_strategy_pair() {
    // このテストは headless 不可のためスキップする。
    // 詳細はファイル上部のコメントを参照。
}
