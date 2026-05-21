//! L4 full_window_render_smoke — `BACKCAST_E2E=1` の固定 fixture で実ウィンドウを起動したとき、
//! メニュー・サイドバー・フッター・主要パネル・チャートが欠落や重なりなく描画されることを保証する（kind:render）。
//!
//! ## headless 環境では実行不可
//!
//! このフローは `App::run()` を呼ぶため winit イベントループが必要であり、スクリーンバッファへの
//! GPU レンダリングが伴う。darwin CI（headless / no GPU）では実行できない。
//!
//! ### 実行ゲート
//! `BACKCAST_E2E=1` という環境変数が存在する **実ディスプレイ付きマシン**（ローカル開発・
//! 物理 CI ノード）でのみ手動または opt-in CI ジョブとして走らせること。
//!
//! ### 将来の実装指針（`BACKCAST_E2E=1` がある場合）
//! 1. `App::new()` に `DefaultPlugins`（実ウィンドウ）を add し、fixture 戦略と scenario を
//!    `BACKCAST_CACHE_DIR` / `BACKCAST_FIXTURE_PATH` 経由でロードさせる。
//! 2. 起動後数フレーム経ったら `screenshot` プラグインか bevy の `CaptureFrame` で
//!    スクリーンショットを取得する。
//! 3. `ui_dump::dump_panels` で構造ダンプを取り、メニュー / サイドバー / フッター /
//!    StrategyEditor / Chart の全領域が重複なく存在することを assert する。
//! 4. ピクセルレベルの blank チェック（全ピクセル同一色でないこと）を行う。
//!
//! skipped: kind:render — requires real winit window + GPU; not headless.
