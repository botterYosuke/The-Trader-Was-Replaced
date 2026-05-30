//! I23 (kind:ui + kind:manual-gate): cache 復元後の editor が `original=None`（破損 cache）の状態で
//! File→Open すると、in-place fragment update は実行されるが、その後 StrategyFragment.source が
//! 0 lines にリセットされて「0 lines ↔ N lines」チカチカが再発する。
//!
//! 関連: [I22] [I21] [I20]
//!
//! ## 根本原因と fix（issue #72）
//!
//! `sync_strategy_fragment_to_bevscode_system` が `SetTextRequested` と同フレームで
//! `SetLanguageRequested` を送ると、bevscode が `SetLanguageRequested` 処理時に rope を一時クリアし
//! `Changed<TextBuffer<RopeBuffer>>` が 0 lines で立つ。
//! `sync_bevscode_to_strategy_fragment_system` がこれを受けて `fragment.source = ""` に上書きし、
//! oscillation が発生する（`original=None` 起動 = 空 `TextBuffer` で特に顕在化）。
//!
//! fix: `PendingLanguageReRequest` marker component を追加し、`SetLanguageRequested` を翌フレームに
//! 遅延（`flush_pending_language_request_system`。`sync_bevscode_to_strategy_fragment_system` の後に登録）。
//!
//! ## 自動ガード（unit テスト、GREEN）
//!
//! - `i23_content_replace_defers_language_request_via_marker`:
//!   `sync_strategy_fragment_to_bevscode_system` が content replace 時に `SetLanguageRequested` を送らず
//!   `PendingLanguageReRequest` を entity に insert することを assert（`cargo test --lib`）。
//! - `i23_flush_pending_language_request_sends_lang_and_removes_marker`:
//!   `flush_pending_language_request_system` が翌フレームに `SetLanguageRequested` を送り
//!   `PendingLanguageReRequest` を remove することを assert（`cargo test --lib`）。
//!
//! ## manual-gate 確認手順
//!
//! 1. `C:\Users\<user>\AppData\Local\the-trader-was-replaced\app_state.json` を破損させる
//!    （`end: "not-a-date"` 等）か、`app_state.py` を空にして起動する。
//! 2. cache restore: `original=None`, editor = "# dummy" または空 で起動する。
//! 3. File → Open → `examples/test_strategy_daily.json` を選択。
//! 4. status bar が `[1 region, N lines]`（N > 0）で安定していることを確認する。
//!    「0 lines ↔ N lines」チカチカが発生すれば BUG 再現。
//!
//! fix 適用済み（issue #72）。
