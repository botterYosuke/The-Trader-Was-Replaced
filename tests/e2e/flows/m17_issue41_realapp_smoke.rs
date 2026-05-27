//! M17 issue41_realapp_smoke — issue #41 実アプリ smoke: RUN RESULT × 無し /
//! Replay 可視 / Manual 非表示 / Auto 再表示 / サイドバーボタン無し（kind:manual-gate）。
//!
//! # 確認済み: 2026-05-27
//! 起動オプション: `--live-venue TACHIBANA` 付き（Manual モード切替に venue 接続が必要）。
//!
//! ## 確認手順
//! 1. backend を `--live-venue TACHIBANA` で起動。
//! 2. GUI 起動 → フッター `state: IDLE  grpc: OK` 確認。
//! 3. M13: RUN RESULT ウィンドウに × ボタンが**ない**こと。
//! 4. M14: Replay モードで RUN RESULT が**表示**されていること。
//! 5. M15: サイドバーに「Run Result」ボタンが**ない**こと。
//! 6. M14: Venue → Connect Tachibana → 接続後、ExecutionMode を Manual に切替
//!    → RUN RESULT が**消える**こと。
//! 7. M14: ExecutionMode を Auto に切替 → RUN RESULT が**再表示**されること。
//!
//! ## 結果
//! 全項目 pass。自動 smoke は M13–M16 が担う。headless では Manual モード切替に
//! venue 接続が必要なため `kind:manual-gate`。
//!
//! **テストコード無し**（headless 自動化不可・doc stub）。
