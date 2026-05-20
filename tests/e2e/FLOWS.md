# E2E Flow Catalog — The-Trader-Was-Replaced

> issue #4「Add internal E2E test hooks for Bevy desktop UI」に対する、自動 E2E のテストケース一覧。
> Bevy ネイティブ UI のため Playwright 等は使わず、**Bevy の resource/event/system を直接駆動**して
> 観測可能な状態を assert する。OS のマウス座標クリックには一切依存しない。

## このファイルの使い方（編集ルール）

- 1 つの `- [ ]` 項目 = テストケース(flow)1 本。
- **チェックを入れた `- [x]` 項目を v1（ハーネスと同時に確立）に含める**。
- 追加・削除・並べ替え自由。優先度やバックエンドを変えたいときは行を直接編集する。
- 各 flow はいずれ `tests/e2e/flows/<id>.json` の宣言ファイルになる（このカタログはその索引）。

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
- [ ] **A3 replay_step_forward** ★★ be:`mock`
  - seam: `StepForward` × N
  - 観測: step ごとに `TradingSession.timestamp_ms` が 1 単位進む
- [ ] **A4 replay_force_stop** ★★ be:`mock`
  - seam: Running 中に `ForceStop`
  - 観測: `RunState` が `Running` を抜ける
- [ ] **A5 replay_set_speed** ★ be:`mock`
  - seam: `SetSpeed(N)`
  - 観測: `ReplaySpeed.current == N`（backend ack）
- [x] **A6 replay_failed_strategy** ★★★ be:`mock`  ← v1
  - seam: 壊れた strategy で `RunStrategy`
  - 観測: `RunStarted` → `RunFailed` → `RunState::Failed{error}`、error 文字列が surface
- [ ] **A7 replay_startup_progress** ★★ be:`mock`
  - seam: `RunStrategy`
  - 観測: `ReplayStartup` 4 stage（Resetting→Loading→Starting→WaitingFirstTick）が `ReplayStartupProgress` に反映
- [ ] **A8 stale_startup_id_ignored** ★★ be:`mock`（回帰しやすい）
  - seam: 旧 `startup_id` の status update を後追い注入
  - 観測: 古い update が新しい startup window を閉じない（相関 ID ロジック）

## B. ポートフォリオ / 実行結果

- [x] **B1 portfolio_populated_after_run** ★★★ be:`mock`  ← v1
  - seam: RunComplete 後の `PortfolioLoaded`
  - 観測: `PortfolioState.loaded == true`、positions/orders/equity 充填
- [ ] **B2 run_summary_parsed** ★★ be:`mock`
  - seam: `RunComplete{summary_json}`
  - 観測: `LastRunResult.parsed_summary`（fills_count, equity_points, total_pnl）が parse 済み

## C. 銘柄ユニバース / サイドバー

- [ ] **C1 list_instruments_replay** ★★ be:`mock`
  - seam: `ListInstruments(ReplayCatalogFallback)`
  - 観測: `TickersStatus: InFlight→Loaded`、`Tickers` 充填、source 設定
- [ ] **C2 list_instruments_failed** ★★ be:`mock`
  - seam: `ListInstruments` 失敗
  - 観測: `TickersStatus::Failed`、旧 list 保持（stale 表示）
- [ ] **C3 fetch_available_instruments** ★ be:`mock`
  - seam: `FetchAvailableInstruments(end_date)`
  - 観測: `AvailableInstruments.by_end_date[end_date]` 充填、`in_flight` クリア
- [ ] **C4 fetch_available_failed** ★ be:`mock`
  - seam: `FetchAvailableInstruments` 失敗
  - 観測: `AvailableInstruments.last_error` セット
- [ ] **C5 select_instrument** ★ be:`mock`
  - seam: `SelectedSymbol` 更新
  - 観測: 選択銘柄が反映

## D. Venue ライフサイクル（Live）

- [ ] **D1 venue_login_success** ★★ be:`mock`
  - seam: `VenueLogin`
  - 観測: `VenueState: Disconnected→Authenticating→Connected`
- [ ] **D2 venue_subscribed** ★ be:`mock`
  - seam: login 後
  - 観測: `Connected→Subscribed`、`instruments_loaded` 反映
- [ ] **D3 venue_login_error** ★★ be:`mock`
  - seam: login 失敗
  - 観測: `VenueState::Error`
- [ ] **D4 venue_logout** ★ be:`mock`
  - seam: `VenueLogout`
  - 観測: `→Disconnected`
- [ ] **D5 venue_logout_detected** ★ be:`mock`
  - seam: `BackendEvent::VenueLogoutDetected`
  - 観測: 外部ログアウトで状態クリア
- [ ] **D6 venue_reconnecting** ★★ be:`mock`
  - seam: `VenueChanged(Reconnecting)`
  - 観測: `Reconnecting` 表示（issue の network reconnect）
- [ ] **D7 live_universe_overwrite** ★★ be:`mock`
  - seam: Connected で `ListInstruments(LiveVenue)`
  - 観測: Live universe が Replay fallback を**上書き**（prune しない不変条件）

## E. 実行モード

- [ ] **E1 set_execution_mode** ★★ be:`mock`
  - seam: `SetExecutionMode(LiveManual)`
  - 観測: backend authoritative → `ExecutionModeChanged` → `ExecutionModeRes.mode == LiveManual`

## F. ライブ市場データ / 注文・口座イベント（Phase 9/10）

- [ ] **F1 subscribe_market_data** ★ be:`mock`
  - seam: `SubscribeMarketData(id)`
  - 観測: `LastPricesUpdated` → `LastPrices` 充填
- [ ] **F2 unsubscribe_market_data** ★ be:`mock`
  - seam: `UnsubscribeMarketData(id)`
  - 観測: price 更新停止
- [ ] **F3 order_event** ★ be:`mock`
  - seam: `BackendEvent::OrderEvent`
  - 観測: `PortfolioState.orders` に fill 反映
- [ ] **F4 account_event** ★ be:`mock`
  - seam: `BackendEvent::AccountEvent`
  - 観測: `PortfolioState` の cash/buying_power/positions 更新
- [ ] **F5 secret_required** ★ be:`mock`
  - seam: `BackendEvent::SecretRequired`
  - 観測: pending-secret 状態（第二暗証要求）

## G. バックエンド接続 / 自己修復（既存 supervisor テストと一部重複）

- [ ] **G1 backend_connect_status** ★★ be:`mock`
  - seam: supervisor Ready → connect
  - 観測: `BackendStatus` Connected/Running true（footer `grpc: OK`）
- [ ] **G2 backend_reconnect_selfheal** ★★ be:`mock`
  - seam: 接続断 → 復帰
  - 観測: status トグル後に復旧（self-heal commit の回帰防止）
- [ ] **G3 backend_disabled_sim** ★ be:`none`
  - seam: `backend_enabled = false`
  - 観測: `grpc: DISABLED`、send は no-op

---

## E2E に**しない**もの（ユニットテスト / 既存の手動 e2e-testing 担当）

- `LiveAuto` が Phase 8 で選択不可（メニュー gating）→ UI ロジックの単体テスト
- メニュー開閉 / Alt+F / レイアウト永続化 / cosmic_edit 入力 → 描画依存。ユニット + 既存の手動 E2E に残す

---

## 実装状況（2026-05-20 時点 / branch: feat/e2e-test-harness）

- ✅ **ECS 同期層の lib 抽出済み**: `apply_status_update` / `status_update_system` /
  `apply_available_loaded` / `apply_available_failed` / `BackendEventChannel` /
  `backend_event_drain_system` / `StatusUpdateChannel`（`rx` を `pub` 化）を
  `main.rs`（バイナリ）から `src/backend_sync.rs`（lib）へ移動。`main.rs` は
  `backcast::backend_sync::*` を import する形に変更。`cargo check --tests` は通る
  （残る warning は **既存のもの**: `instruments_universe_prune.rs:163/165`、
  `restore.rs:41`、`main.rs:33 UnsubscribeRequest`。いずれも本作業と無関係）。
- ⬜ **未着手（次の担当へ）**:
  1. `tests/e2e/support/mod.rs` に `Harness` を実装。`MinimalPlugins` の headless `App` を組み、
     `StatusUpdateChannel`/`BackendChannel`/`BackendEventChannel` と全 trading resource +
     `ReplayStartupProgress` を insert し、`status_update_system` + `backend_update_system` +
     `backend_event_drain_system` を `Update` に追加。`status_tx`/`backend_tx`/`event_tx` を保持し、
     `send_status()` / `push_state()` / `tick()`(=`app.update()`) / `run_state()` 等の helper を生やす。
     ⚠️ `TradingSettings` は **`backend_enabled: true` で明示構築**（`backend_update_system` が
     disabled だと早期 return する）。`from_env()` は env 依存なので使わない。
  2. テストバイナリ `tests/e2e_replay.rs` を作り、`#[path = "e2e/support/mod.rs"] mod support;`
     で取り込む（`tests/` 直下の `.rs` だけが test crate になる。`tests/e2e/` 配下はビルドされない）。
  3. v1 の 4 本（A1/A2/A6/B1）を実装:
     - **A1**: `RunStarted`→assert `RunState::Running`→`RunComplete{summary_json}`→assert
       `Completed` + `parsed_summary`(fills_count/total_pnl)。
     - **A6**: `RunStarted`→`RunFailed{error}`→assert `RunState::Failed{error}`。
     - **B1**: `PortfolioLoaded{..}`→assert `PortfolioState.loaded==true` + equity/positions。
     - **A2**: `RunStarted`→`push_state(ts=1000)`→assert `TradingSession.timestamp_ms==1000`→
       (state を送らない=pause) tick して不変→`push_state(ts=2000)`→assert 2000→`RunComplete`。
       ※ pause/resume の本来のセマンティクス（`Pause`/`Resume` を gRPC で送る）は transport task
       依存。v1 では「backend が押し出す replay clock を UI が忠実にミラーする」ことの検証に留める。
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
