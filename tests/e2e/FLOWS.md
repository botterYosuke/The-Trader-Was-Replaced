# E2E Flow Catalog — The-Trader-Was-Replaced

> issue #4「Add internal E2E test hooks for Bevy desktop UI」に対する、自動 E2E のテストケース一覧。
> Bevy ネイティブ UI のため Playwright 等は使わず、**Bevy の resource/event/system を直接駆動**して
> 観測可能な状態を assert する。OS のマウス座標クリックには一切依存しない。

## このファイルの使い方（編集ルール）

- 1 つの `- [ ]` 項目 = テストケース(flow)1 本。
- **チェックを入れた `- [x]` 項目を v1（ハーネスと同時に確立）に含める**。
- 追加・削除・並べ替え自由。優先度やバックエンドを変えたいときは行を直接編集する。
- 各 flow はいずれ `tests/e2e/flows/<id>.json` の宣言ファイルになる（このカタログはその索引）。

### wiki ↔ E2E 同期ルール（必須）

- このカタログは `docs/wiki/`（実アプリの操作説明書）と**対**になっている。
  wiki に書かれたユーザー可視の挙動は、原則ここに対応する flow を持つ。
- **`docs/wiki/` の操作挙動を変更・追加したら、必ずこのカタログを見直す**:
  新しい挙動 → flow 行を追加（実装可能なら `- [ ]`、観測不能なら「保留」注記）。
  挙動の削除・変更 → 対応 flow を削除・修正。
- backend→ECS seam を通らない挙動（クライアント側 gating / 純 UI / 描画依存 / backend 内部ガード）は
  flow にせず、末尾の「**E2E に*しない*もの**」節に理由付きで記録する。これも wiki との対応の一部。

### 凡例

- **seam** … 入力する縫い目（`TransportCommand` 列挙子 / `BackendEvent` / resource 更新）
- **観測** … assert 対象の resource とその状態遷移
- **be** … 想定バックエンド: `mock`（決定論的・CI 向き） / `real`（python -m engine・忠実度確認） / `none`
- **優先** … ★★★ 高 / ★★ 中 / ★ 低

### 駆動の縫い目（参照）

- 入力: `TransportCommandSender`（`mpsc<TransportCommand>`）にコマンドを直接送る → UI ボタン描画をバイパス
- 入力(イベント): backend → ECS の `BackendStatusUpdate` / `BackendEvent` をモックから注入
- 出力(観測): `LastRunResult.state`(RunState) / `PortfolioState` / `BackendStatus` /
  `VenueStatusRes` / `ExecutionModeRes` / `Tickers` / `TickersStatus` / `AvailableInstruments` /
  `LastPrices` / `SelectedSymbol` / `ReplaySpeed` / `TradingSession` / `ReplayStartupProgress`

---

## A. リプレイ・ライフサイクル（issue #4 の suggested first workflow / コア）

- [x] **A1 replay_runs_to_completion** ★★★ be:`mock`  ← v1
  - seam: `RunStrategy`
  - 観測: `RunState: Idle→Running→Completed`、`LastRunResult.summary_json` 充填
- [x] **A2 replay_pause_resume** ★★★ be:`mock`  ← v1
  - seam: `RunStrategy` → `Pause` → `Resume`
  - 観測: Running 中に `TradingSession.timestamp_ms` 停止 → 再進行 → `Completed`
- [x] **A3 replay_step_forward** ★★ be:`mock`
  - seam: `StepForward` × N
  - 観測: step ごとに `TradingSession.timestamp_ms` が 1 単位進む
- [x] **A4 replay_force_stop** ★★ be:`mock`
  - seam: Running 中に `ForceStop`
  - 観測: `RunState` が `Running` を抜ける
- [ ] **A5 replay_set_speed** ★ be:`mock`（保留: `BackendStatusUpdate` に speed ack variant が無く、backend→ECS seam で観測できない。transport task の lib 抽出=Phase A-full 待ち）（wiki: フッター速度ボタン 1x〜50x に対応）
  - seam: `SetSpeed(N)`
  - 観測: `ReplaySpeed.current == N`（backend ack）
- [x] **A6 replay_failed_strategy** ★★★ be:`mock`  ← v1
  - seam: 壊れた strategy で `RunStrategy`
  - 観測: `RunStarted` → `RunFailed` → `RunState::Failed{error}`、error 文字列が surface
- [x] **A7 replay_startup_progress** ★★ be:`mock`
  - seam: `RunStrategy`
  - 観測: `ReplayStartup` 4 stage（Resetting→Loading→Starting→WaitingFirstTick）が `ReplayStartupProgress` に反映
- [x] **A8 stale_startup_id_ignored** ★★ be:`mock`（回帰しやすい）
  - seam: 旧 `startup_id` の status update を後追い注入
  - 観測: 古い update が新しい startup window を閉じない（相関 ID ロジック）
- [ ] **A9 replay_startup_timeout** ★ be:`mock`（保留: 60s ソフトタイムアウト→error+`Close` を観測したいが、`ReplayStartupProgress` にタイムアウト状態の seam が無く timer 駆動。要 timer 注入 / progress resource 拡張）（wiki: replay.md「Replay Startup 進捗ウィンドウ」のタイムアウト）
  - seam: startup 開始後、first tick が来ないまま 60s 経過
  - 観測: `ReplayStartupProgress` がタイムアウトエラー表示へ遷移し、`Close` で閉じられる

## B. ポートフォリオ / 実行結果

- [x] **B1 portfolio_populated_after_run** ★★★ be:`mock`  ← v1
  - seam: RunComplete 後の `PortfolioLoaded`
  - 観測: `PortfolioState.loaded == true`、positions/orders/equity 充填
- [x] **B2 run_summary_parsed** ★★ be:`mock`
  - seam: `RunComplete{summary_json}`
  - 観測: `LastRunResult.parsed_summary`（fills_count, equity_points, total_pnl）が parse 済み

## C. 銘柄ユニバース / サイドバー

- [x] **C1 list_instruments_replay** ★★ be:`mock`
  - seam: `ListInstruments(ReplayCatalogFallback)`
  - 観測: `TickersStatus: InFlight→Loaded`、`Tickers` 充填、source 設定
- [x] **C2 list_instruments_failed** ★★ be:`mock`
  - seam: `ListInstruments` 失敗
  - 観測: `TickersStatus::Failed`、旧 list 保持（stale 表示）
- [x] **C3 fetch_available_instruments** ★ be:`mock`
  - seam: `FetchAvailableInstruments(end_date)`
  - 観測: `AvailableInstruments.by_end_date[end_date]` 充填、`in_flight` クリア
- [x] **C4 fetch_available_failed** ★ be:`mock`
  - seam: `FetchAvailableInstruments` 失敗
  - 観測: `AvailableInstruments.last_error` セット
- [ ] **C5 select_instrument** ★ be:`mock`（保留: UI 駆動のみ — `SelectedSymbol` は backend→ECS seam を通らない）
  - seam: `SelectedSymbol` 更新
  - 観測: 選択銘柄が反映

## D. Venue ライフサイクル（Live）

- [x] **D1 venue_login_success** ★★ be:`mock`
  - seam: `VenueLogin`
  - 観測: `VenueState: Disconnected→Authenticating→Connected`
- [x] **D2 venue_subscribed** ★ be:`mock`
  - seam: login 後
  - 観測: `Connected→Subscribed`、`instruments_loaded` 反映
- [x] **D3 venue_login_error** ★★ be:`mock`
  - seam: login 失敗
  - 観測: `VenueState::Error`
- [x] **D4 venue_logout** ★ be:`mock`
  - seam: `VenueLogout`
  - 観測: `→Disconnected`
- [ ] **D5 venue_logout_detected** ★ be:`mock`（保留: event seam — `backend_event_drain_system` が resource を変えない。実装には `src/backend_sync.rs` 拡張が必要）
  - seam: `BackendEvent::VenueLogoutDetected`
  - 観測: 外部ログアウトで状態クリア
- [x] **D6 venue_reconnecting** ★★ be:`mock`
  - seam: `VenueChanged(Reconnecting)`
  - 観測: `Reconnecting` 表示（issue の network reconnect）
- [x] **D7 live_universe_overwrite** ★★ be:`mock`
  - seam: Connected で `ListInstruments(LiveVenue)`
  - 観測: Live universe が Replay fallback を**上書き**（prune しない不変条件）
- [ ] **D8 prod_guard_blocks_login** ★ be:`real`（保留: 二重ガードは backend 側 = `TACHIBANA_ALLOW_PROD` / `KABU_ALLOW_PROD` 未設定で Prod 接続を遮断する挙動。backend→ECS には login 失敗として現れるが、env ガードの忠実度は backend 単体テスト向き）（wiki: venues.md「二重ガード」）
  - seam: env 未設定で Prod venue へ `VenueLogin`
  - 観測: 接続が拒否され `VenueState::Error`（または login が送られない）

## E. 実行モード

- [x] **E1 set_execution_mode** ★★ be:`mock`
  - seam: `SetExecutionMode(LiveManual)`
  - 観測: backend authoritative → `ExecutionModeChanged` → `ExecutionModeRes.mode == LiveManual`

## F. ライブ市場データ / 注文・口座イベント（Phase 9/10）

- [x] **F1 subscribe_market_data** ★ be:`mock`
  - seam: `SubscribeMarketData(id)`
  - 観測: `LastPricesUpdated` → `LastPrices` 充填
- [x] **F2 unsubscribe_market_data** ★ be:`mock`
  - seam: `UnsubscribeMarketData(id)`
  - 観測: price 更新停止
- [ ] **F3 order_event** ★ be:`mock`（保留: event seam — `backend_event_drain_system` が resource を変えない。`src/backend_sync.rs` 拡張が必要）
  - seam: `BackendEvent::OrderEvent`
  - 観測: `PortfolioState.orders` に fill 反映
- [ ] **F4 account_event** ★ be:`mock`（保留: event seam — 同上）
  - seam: `BackendEvent::AccountEvent`
  - 観測: `PortfolioState` の cash/buying_power/positions 更新
- [ ] **F5 secret_required** ★ be:`mock`（保留: event seam — pending-secret resource 自体が未実装）
  - seam: `BackendEvent::SecretRequired`
  - 観測: pending-secret 状態（第二暗証要求）

## G. バックエンド接続 / 自己修復（既存 supervisor テストと一部重複）

- [x] **G1 backend_connect_status** ★★ be:`mock`
  - seam: supervisor Ready → connect
  - 観測: `BackendStatus` Connected/Running true（footer `grpc: OK`）
- [x] **G2 backend_reconnect_selfheal** ★★ be:`mock`
  - seam: `Error` で接続断 → `Connected(true)` で復帰
  - 観測: `Error` で `connected=false`+`last_error` 記録 → 再 `Connected(true)` で `connected` 復旧（self-heal commit の回帰防止）
- [ ] **G3 backend_disabled_sim** ★ be:`none`
  - seam: `backend_enabled = false`
  - 観測: `grpc: DISABLED`、send は no-op

---

## E2E に**しない**もの（ユニットテスト / 既存の手動 e2e-testing 担当）

- `LiveAuto` が Phase 8 で選択不可（メニュー gating）→ UI ロジックの単体テスト
- **モード切替の前提条件 gating**（wiki: modes.md）→ クライアント側で切替リクエストを抑止する純 UI ロジックで
  backend→ECS seam を通らない。単体テスト向き:
  - Manual / Auto への切替は venue 接続済み（Disconnected / Error 以外）でないとリクエストを送らない
  - Replay への切替は戦略ロード済みでないと送らない
- メニュー開閉 / Alt+F / レイアウト永続化 / cosmic_edit 入力 → 描画依存。ユニット + 既存の手動 E2E に残す
- **Startup パネルの入力検証 gating**（wiki: replay.md / strategy.md）→ 空・不正な日付 / granularity 未選択 /
  initial_cash ≤ 0 / `start > end` のとき Run を抑止し赤字エラーを出すのはクライアント側の純 UI 検証で、
  backend→ECS seam を通らない。`scenario_startup_panel.rs` の単体テスト向き。
- **`instruments_ref`（schema v3）の fail-closed**（wiki: strategy.md「instruments_ref」）→ 参照先 JSON が
  欠落・破損・空のとき `ScenarioLoadedFromFile` を発火させず Run を半透明のままにする挙動。
  file-watch 駆動（`scenario_parser.rs`）で backend→ECS seam を通らないため、`scenario_parser.rs` の
  既存単体テスト（`parse_resolves_instruments_ref_to_instruments` 等）でカバー。
- **銘柄ピッカー（`+ Add`）の挙動**（wiki: venues.md「銘柄ピッカー」）→ 検索絞り込み / 最大 15 行 /
  プレースホルダ（`Set scenario.end first` / `Venue not connected` / `Loading...` / `Error:` / `No matches`）/
  100ms デバウンス / `instruments_ref` 時の読み取り専用化は描画依存の純 UI（`instrument_picker.rs`）。
  ユニットテスト向き。ピッカーが消費する銘柄ユニバース取得自体は [C1]〜[C4] でカバー済み。

---

## 実装状況（2026-05-20 時点 / branch: feat/e2e-test-harness）

- ✅ **ECS 同期層の lib 抽出済み**: `apply_status_update` / `status_update_system` /
  `apply_available_loaded` / `apply_available_failed` / `BackendEventChannel` /
  `backend_event_drain_system` / `StatusUpdateChannel`（`rx` を `pub` 化）を
  `main.rs`（バイナリ）から `src/backend_sync.rs`（lib）へ移動。`main.rs` は
  `backcast::backend_sync::*` を import する形に変更。`cargo check --tests` は通る
  （残る warning は **既存のもの**: `instruments_universe_prune.rs:163/165`、
  `restore.rs:41`、`main.rs:33 UnsubscribeRequest`。いずれも本作業と無関係）。
- ✅ **v1 ハーネス + 4 本実装済み（A1/A2/A6/B1、`cargo test --test e2e_replay` で 4 passed）**:
  1. `tests/e2e/support/mod.rs` に `Harness` を実装済み。`MinimalPlugins` の headless `App` に
     `StatusUpdateChannel`/`BackendChannel`/`BackendEventChannel` + 全 trading resource +
     `ReplayStartupProgress` を insert し、`backend_update_system`/`status_update_system`/
     `backend_event_drain_system` を `Update` に登録。`status_tx`/`backend_tx`/`event_tx` を保持し、
     `send_status()`/`send_event()`/`push_state(ts)`/`tick()`/`run_state()`/`last_run()`/
     `portfolio()`/`timestamp_ms()` helper を提供。`TradingSettings` は `backend_enabled: true` で
     明示構築（`from_env()` 不使用）。`push_state` は `BackendTradingState` を serde_json で最小構築。
  2. テストバイナリ `tests/e2e_replay.rs` を作成済み（`#[path = "e2e/support/mod.rs"] mod support;`）。
  3. v1 の 4 本実装済み: **A1**=RunStarted→Running→RunComplete→Completed+parsed_summary、
     **A6**=RunStarted→RunFailed→Failed{error}、**B1**=PortfolioLoaded→loaded+equity/positions/orders、
     **A2**=RunStarted→push_state(1000)→pause(tick 不変)→push_state(2000)→RunComplete。
     ※ A2 の pause/resume は backend が押し出す replay clock のミラー検証に留める（gRPC 経由の
     `Pause`/`Resume` は transport task 依存 = Phase A-full）。
- ✅ **v2 で 19 本追加（`cargo test --test e2e_replay` で 23 passed）**: A3/A4/A7/A8、B2、
  C1/C2/C3/C4、D1/D2/D3/D4/D6/D7、E1、F1/F2、G1。`tests/e2e/support/mod.rs` に観測アクセサ
  （`venue()`/`exec_mode()`/`tickers()`/`available()`/`last_prices()`/`startup_progress()`/
  `backend_connected()`/`backend_running()`）と、startup window を開く `begin_startup(id)` を追加。
  A3 は A2 と同じく backend が押し出す replay clock のミラー検証。A4 は force-stop が
  backend→ECS では `RunComplete` として現れる（`Running` を抜けることを観測）。A8 は古い
  `startup_id` の `RunComplete` が startup window を閉じないことの回帰テスト。D7 は Live universe が
  Replay fallback list を wholesale 上書きする不変条件。
- ✅ **G2 backend_reconnect_selfheal 追加（24 passed）**: `Error` で `connected=false`+`last_error`
  記録 → 再 `Connected(true)` で `connected` が復帰することを観測（連続断・復旧が status seam だけで
  決定論的に書けるため、supervisor task に依存せず実装できた）。`backend_last_error()` アクセサを追加。
- ⬜ **保留（同ハーネスでは観測不能）**:
  - **A5 set_speed**: `BackendStatusUpdate` に speed ack variant が無い（Phase A-full 待ち）。
  - **C5 select_instrument**: `SelectedSymbol` は UI 駆動のみで backend→ECS seam を通らない。
  - **D5/F3/F4/F5（event seam）**: `backend_event_drain_system` が `info!` するだけで resource を
    変えないため assert 不可。実装には `src/backend_sync.rs` の event drain に
    `PortfolioState`/`VenueStatusRes`/pending-secret resource 反映を足し、本番 `main.rs` の挙動と
    一致させる必要がある（スコープ拡大 = 依頼者確認待ち）。
  - **G3 backend_disabled**: `backend_enabled=false` が前提だが `Harness` は `true` 固定で構築する。
    観測には disabled 版ハーネス（または builder 引数）の追加が必要。
- 📌 **設計メモ**: 本 v1 は backend→ECS の片側（`BackendStatusUpdate` 注入）を駆動する。
  反対側（`TransportCommand`→gRPC→`BackendStatusUpdate`）は既に `tests/backend_integration.rs`
  が mock tonic サーバでカバー済み。**両者で end-to-end をカバー**する構図。完全な単一プロセス
  ループ（`TransportCommand` 注入→mock gRPC→`RunState` 観測）は transport task（`main.rs`
  `setup_backend_connection` の ~600 行）の lib 抽出が必要で、これは Phase A-full の別タスク。

## ハーネス計画（参考・別途実装）

- **Phase A**: App 組み立てとトランスポートタスクを `main.rs` から lib へ抽出 → `MinimalPlugins` の
  ヘッドレス App を `tests/e2e/` から起動。flow JSON を読み、`TransportCommand` を注入し
  `app.update()` ループで resource を assert。mock gRPC（`backend_integration.rs` の `MyDataEngine`
  を `tests/e2e/support/` に共有抽出）を使用。CI 向き。
- **Phase B**: `--e2e` / `BACKCAST_E2E=1` のウィンドウ実行モード（固定ウィンドウ・固定パス・
  flow JSON 読み込み・構造化ログ）。同じ flow JSON を実描画で smoke 実行。

### 想定ディレクトリ

```
tests/e2e/
├── FLOWS.md          ← このカタログ（索引）
├── flows/            ← 宣言的 flow ファイル *.json（各 flow 本体）
├── fixtures/         ← strategy .py / scenario sidecar JSON など素材
└── support/          ← 共有 Rust ヘルパ（mock engine / headless app builder）
```
