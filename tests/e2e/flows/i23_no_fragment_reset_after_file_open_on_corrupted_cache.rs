//! I23 (kind:ui + kind:manual-gate): cache 復元後の editor が `original=None`（破損 cache）の状態で
//! File→Open すると、in-place fragment update は実行されるが、その後 StrategyFragment.source が
//! 0 lines にリセットされて「0 lines ↔ N lines」チカチカが再発する。
//!
//! 関連: [I22] [I21] [I20]
//!
//! ## 再現手順（manual-gate）
//!
//! 1. `C:\Users\<user>\AppData\Local\the-trader-was-replaced\app_state.json` を破損させる
//!    （`end: "not-a-date"` 等）か、app_state.py を空にして起動する。
//! 2. cache restore: `original=None`, editor = "# dummy" または空 で起動する。
//! 3. File → Open → `examples/test_strategy_daily.json` を選択。
//! 4. status bar が `[1 region, N lines]`（N > 0）で安定していることを確認する。
//!    「0 lines ↔ N lines」チカチカが発生すれば BUG 再現。
//!
//! ## 調査済みの根本原因候補
//!
//! - `sync_strategy_fragment_to_bevscode_system`（`strategy_editor.rs`）が
//!   `SetTextRequested{text: N lines}` と同時に `SetLanguageRequested` を送る（I20 fix）。
//! - bevscode が `SetLanguageRequested` 処理時に rope を一時クリアすると
//!   `Changed<TextBuffer<RopeBuffer>>` が 0 lines で立ち、
//!   `sync_bevscode_to_strategy_fragment_system` が `fragment.source = ""` に上書きする。
//! - `original=None` 起動では bevscode peer の `TextBuffer` が空のため、上書き→復元の
//!   oscillation が発生しやすい（通常起動では TextBuffer に既存コンテンツがありスキップされる）。
//!
//! ## 自動テスト計画（headless 不可 → 代替）
//!
//! - bevscode のメッセージ処理順序（`SetTextRequested` vs `SetLanguageRequested` の順序）が
//!   headless テストで再現困難なため、`kind:manual-gate` が主軸。
//! - 可能な自動ガード: `sync_strategy_fragment_to_bevscode_system` が rope クリア経路を通る際、
//!   `SetTextRequested` を `SetLanguageRequested` より必ず後に処理されるよう ordering を追加。
//!   unit test で「SetTextRequested を送った後に Changed<TextBuffer> が正コンテンツを持つ」を assert。
//!
//! fix 後に `- [x]` に更新し、unit / manual ゲート結果を記録する（issue #72）。
//! RED＝回帰ガード・fix は #72 後に green
