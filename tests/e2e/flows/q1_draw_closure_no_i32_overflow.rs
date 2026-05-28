//! Q1 draw_closure i32 overflow — issue #60
//!
//! ## 概要
//! macOS Retina (render_scale ≈ 2.0) 環境で LiveManual 切替直後に backend から
//! `AccountEvent cash=20000000 buying_power=20000000` が届いたとき、
//! `bevy_cosmic_edit::render::render_texture` の `draw_closure` 内で
//! `x + col + pad_x - scroll_x` が i32 オーバーフローしてパニックする。
//!
//! ## 修正
//! `draw_coord_x` / `draw_coord_y` ヘルパーを抽出し、i64 中間計算 + clamp に変更。
//! 単体テストは `crates/bevy_cosmic_edit/src/render.rs` の `tests` モジュールで実装済み。
//!
//! ## kind: render / manual-gate
//! `render_texture` は GPU/ウィンドウ依存のため E2E harness (headless) では自動化不可。
//! 代替方式: `crates/bevy_cosmic_edit/src/render.rs` の単体テスト（draw_coord_x / draw_coord_y）
//! で overflow しないことを debug ビルドで確認する。
//!
//! ## リリースゲート（手動確認手順）
//! 1. `--live-venue TACHIBANA` で起動
//! 2. Venue → Connect Tachibana（demo 認証）して `grpc: OK` を確認
//! 3. フッターで ExecutionMode を LiveManual に切り替え
//! 4. backend から `AccountEvent cash=20000000 buying_power=20000000` が届いても
//!    クラッシュしないことを確認する
//! 5. Strategy Editor の文字入力（J1〜J8 相当の操作）が正常に動作することを確認
//!
//! ## 回帰ガード
//! - `cargo test -p bevy_cosmic_edit draw_coord` が green であれば overflow ガード済み
//! - 全体: `cargo test --test e2e_replay` で J1〜J8 等の cosmic_edit 入力系が回帰しないこと
