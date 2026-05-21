# E2E Flow Catalog — The-Trader-Was-Replaced

> リリース前の最後の砦として、ユーザーが取りうる行動を原則すべて列挙し、自動テストの対象にする。
> 既存の Bevy resource/event/system 直接駆動ハーネスで観測できるものはこの方式で assert する。
> 直接駆動だけでは忠実に検証できない操作（描画依存、OS ダイアログ、キーボード入力、実 backend/環境依存）は
> 「対象外」にせず、代替方式（UI harness / smoke / integration / manual release gate）を明記する。

## このファイルの使い方（編集ルール）

- 1 つの `- [ ]` 項目 = テストケース(flow)1 本、または代替方式で実行する release-gate item 1 本。
- **チェックを入れた `- [x]` 項目を v1（ハーネスと同時に確立）に含める**。
- 追加・削除・並べ替え自由。優先度やバックエンドを変えたいときは行を直接編集する。
- 各 flow はいずれ `tests/e2e/flows/<id>.json` の宣言ファイルになる（このカタログはその索引）。

### wiki ↔ E2E 同期ルール（必須）

- このカタログは `docs/wiki/`（実アプリの操作説明書）と**対**になっている。
  wiki に書かれたユーザー可視の挙動は、原則ここに対応する flow を持つ。
- **`docs/wiki/` の操作挙動を変更・追加したら、必ずこのカタログを見直す**:
  新しい挙動 → flow 行を追加（実装可能なら `- [ ]`、観測不能なら「保留」注記）。
  挙動の削除・変更 → 対応 flow を削除・修正。
- backend→ECS seam を通らない挙動（クライアント側 gating / 純 UI / 描画依存 / backend 内部ガード）も
  カタログから除外しない。直接駆動ハーネスで不可能な場合は、末尾の「**直接駆動では不可能な場合の代替方式**」
  に方式と release gate を記録する。

### 凡例

- **seam** … 入力する縫い目（`TransportCommand` 列挙子 / `BackendEvent` / resource 更新）
- **観測** … assert 対象の resource とその状態遷移
- **be** … 想定バックエンド: `mock`（決定論的・CI 向き） / `real`（python -m engine・忠実度確認） / `none`
- **kind** … `state`（resource/event/system 直接駆動） / `ui`（Bevy UI harness: Interaction/Keyboard/MouseWheel/Pointer を注入） /
  `render`（実ウィンドウまたは画像/ログ smoke） / `integration`（backend/CLI/環境依存） / `manual-gate`（自動化不能時のリリース手順）
- **優先** … ★★★ 高 / ★★ 中 / ★ 低

### 駆動の縫い目（参照）

- 入力: `TransportCommandSender`（`mpsc<TransportCommand>`）にコマンドを直接送る → UI ボタン描画をバイパス
- 入力(イベント): backend → ECS の `BackendStatusUpdate` / `BackendEvent` をモックから注入
- 入力(UI): Bevy の `Interaction` / `ButtonInput<KeyCode>` / `MouseWheel` / pointer event / focused entity を直接注入
- 出力(観測): `LastRunResult.state`(RunState) / `PortfolioState` / `BackendStatus` /
  `VenueStatusRes` / `ExecutionModeRes` / `Tickers` / `TickersStatus` / `AvailableInstruments` /
  `LastPrices` / `SelectedSymbol` / `ReplaySpeed` / `TradingSession` / `ReplayStartupProgress`
- 出力(UI/描画): window/panel entity 構造、`Visibility`/`Display`/`Text`/`Style`、layout JSON、strategy cache、
  render/screenshot smoke、構造化ログ

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
- [x] **A9 replay_startup_timeout** ★ be:`mock`（timer 駆動。本番 `replay_startup_timeout_system` をハーネスに登録し、`Time<Real>` を `advance_real_time` で進めて駆動）（wiki: replay.md「Replay Startup 進捗ウィンドウ」のタイムアウト）
  - seam: startup 開始後（`arm_startup_timeout`）、first tick が来ないまま 60s 経過
  - 観測: 59s では error なし → 61s で `ReplayStartupProgress.error` セット（phase 不変・window は開いたまま）→ `close_startup_window` で error クリア+hide

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
- [x] **D5 venue_logout_detected** ★ be:`mock`
  - seam: `BackendEvent::VenueLogoutDetected{venue}`
  - 観測: `ReloginPrompt.active == Some(venue)`（外部ログアウト検知で ReloginModal が開く）。Phase 9 Step 7（health watchdog）マージで `backend_event_drain_system` が `ReloginPrompt` をセットするようになった。モーダルは通知のみで自身は再ログインしないため `VenueStatusRes` は意図的に不変
- [x] **D6 venue_reconnecting** ★★ be:`mock`
  - seam: `VenueChanged(Reconnecting)`
  - 観測: `Reconnecting` 表示（issue の network reconnect）
- [x] **D7 live_universe_overwrite** ★★ be:`mock`
  - seam: Connected で `ListInstruments(LiveVenue)`
  - 観測: Live universe が Replay fallback を**上書き**（prune しない不変条件）
- [ ] **D8 prod_guard_blocks_login** ★ be:`real`（保留: 二重ガードは backend 側 = `TACHIBANA_ALLOW_PROD` / `KABU_ALLOW_PROD` 未設定で Prod 接続を遮断する挙動。backend→ECS には login 失敗として現れるが、env ガードの忠実度は [L3] の backend integration / manual-gate で担保する）（wiki: venues.md「二重ガード」）
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
- [x] **F3 order_event** ★ be:`mock`
  - seam: `BackendEvent::OrderEvent`
  - 観測: `LiveOrders.orders` に `client_order_id` 一致レコードが現れ status/filled_qty/avg_price 反映。未知 id は static フィールド空で挿入され、同 id の後続イベントは in-place マージ
- [x] **F4 account_event** ★ be:`mock`
  - seam: `BackendEvent::AccountEvent`
  - 観測: `PortfolioState` の cash/buying_power/positions 更新・`loaded==true`・`equity == cash + Σ(qty*avg_price + unrealized_pnl)`
- [x] **F5 secret_required** ★ be:`mock`
  - seam: `BackendEvent::SecretRequired`
  - 観測: `SecretPrompt.active` が `Some` になり request_id/venue/kind/purpose 一致（第二暗証要求）

## H. 注文 RPC（Phase 9 ライブ発注 / status seam）

- [x] **H1 order_seeded** ★★ be:`mock`
  - seam: `BackendStatusUpdate::OrderSeeded`
  - 観測: `LiveOrders` に full レコード（symbol/side/qty/price 含む）を seed、`OrderFeedback.message` クリア
- [x] **H2 order_status_updated** ★★ be:`mock`
  - seam: seed 済みに `BackendStatusUpdate::OrderStatusUpdated`
  - 観測: `client_order_id` 一致レコードに status/fill をマージ（static フィールドは保持・重複挿入しない）
- [x] **H3 order_modified** ★★ be:`mock`
  - seam: seed 済みに `BackendStatusUpdate::OrderModified`
  - 観測: `apply_modify` で `Some` の qty/price のみ上書き・`None` は不変、status/fill 更新
- [x] **H4 order_rejected** ★★ be:`mock`
  - seam: `BackendStatusUpdate::OrderRejected{action,error_code}`
  - 観測: `OrderFeedback.message == Some("{action}が拒否されました ({error_code})")`
- [x] **H5 exec_mode_change_resets_portfolio** ★★ be:`mock`（回帰の肝）
  - seam: `BackendStatusUpdate::ExecutionModeChanged`
  - 観測: 実モード変更時に `PortfolioState` を default リセット（Live/Replay 口座データ混線防止）、同一モードは no-op

## G. バックエンド接続 / 自己修復（既存 supervisor テストと一部重複）

- [x] **G1 backend_connect_status** ★★ be:`mock`
  - seam: supervisor Ready → connect
  - 観測: `BackendStatus` Connected/Running true（footer `grpc: OK`）
- [x] **G2 backend_reconnect_selfheal** ★★ be:`mock`
  - seam: `Error` で接続断 → `Connected(true)` で復帰
  - 観測: `Error` で `connected=false`+`last_error` 記録 → 再 `Connected(true)` で `connected` 復旧（self-heal commit の回帰防止）
- [x] **G3 backend_disabled_sim** ★ be:`none`
  - seam: `backend_enabled = false`（`Harness::new_backend_disabled()`）
  - 観測: `backend_enabled == false`（footer `grpc: DISABLED`）、`backend_update_system` が early-return し replay clock push が no-op（`timestamp_ms` 不変）

---

## I. メニュー / モード / レイアウト（ユーザー操作）

- [ ] **I1 file_menu_open_close_keyboard** ★★ kind:`ui` be:`none`
  - seam: `Alt+F` / `Escape` / menu button `Interaction::Pressed`
  - 観測: `OpenMenu` が File に遷移 → close、メニュー entity の `Display` / `Visibility` が同期
- [ ] **I2 edit_menu_undo_redo_window_ops** ★★ kind:`ui` be:`none`
  - seam: window move/close/spawn → `Ctrl+Z` / `Ctrl+Y` / Edit menu item
  - 観測: `AppHistory` と `WindowRoot` の位置・存在が undo/redo で戻る
- [ ] **I3 venue_menu_connect_disconnect_gating** ★★ kind:`ui` be:`mock`
  - seam: Venue menu item click
  - 観測: configured venue に応じた表示/非表示、接続中の Connect disabled、`VenueLogin` / `VenueLogout` command 発行
- [ ] **I4 mode_toggle_precondition_gating** ★★★ kind:`ui` be:`mock`
  - seam: Replay / Manual / Auto segment click
  - 観測: Venue 未接続では Manual/Auto command を送らない、戦略未ロードでは Replay command を送らない、許可時のみ `SetExecutionMode` 発行
- [ ] **I5 file_open_loads_sidecar_and_strategy** ★★★ kind:`integration` be:`none`
  - seam: file dialog をバイパスして selected path event/resource を注入
  - 観測: `.json` sidecar 読み込み、`strategy_path` 解決、Strategy Editor spawn、Scenario metadata 反映
- [ ] **I6 file_new_resets_workspace** ★★ kind:`ui` be:`none`
  - seam: File → New
  - 観測: ロード済み strategy / scenario / panels が新規状態に戻る
- [ ] **I7 layout_save_and_restore** ★★★ kind:`integration` be:`none`
  - seam: window move/resize/close → Save → fresh app → load
  - 観測: sidecar JSON の `viewport` / `windows[]` / `strategy_path` / `scenario` と復元後 entity が一致
- [ ] **I8 layout_autosave_debounce** ★★ kind:`ui` be:`none`
  - seam: window drag end + time advance
  - 観測: 1s 未満では保存なし、1s 後に layout dirty が flush される

## J. Strategy Editor / Startup / 銘柄ピッカー（ユーザー入力）

- [ ] **J1 editor_keyboard_text_entry** ★★★ kind:`ui` be:`none`
  - seam: focused cosmic editor に文字入力 / paste
  - 観測: `StrategyFragment` と cache file が更新され、履歴に text edit が積まれる
- [ ] **J2 editor_tab_enter_autoclose** ★★ kind:`ui` be:`none`
  - seam: `Tab` / `Enter` / `(` `[` `{` `"` `'`
  - 観測: 4-space indent、autoindent、bracket autoclose、closer 直前では補完しない
- [ ] **J3 editor_find_replace_panel** ★★ kind:`ui` be:`none`
  - seam: `Ctrl+F` / query 入力 / `F3` / `Shift+F3` / `Repl` / `Repl All` / `Esc`
  - 観測: panel open/close、match count、current match、case toggle、replace 結果、history
- [ ] **J4 editor_undo_redo_autosave** ★★ kind:`ui` be:`none`
  - seam: edit → `Ctrl+Z` / `Ctrl+Y` → time advance
  - 観測: buffer が戻る/進む、debounce 後の cache 内容が一致
- [ ] **J5 startup_panel_validation_blocks_run** ★★★ kind:`ui` be:`mock`
  - seam: Start / End / Granularity / Initial cash を空・不正値・`start > end` に編集して Run
  - 観測: 赤字 error label、Run command 未送信、正常値に戻すと `RunStrategy` 発行
- [ ] **J6 startup_panel_writeback_to_sidecar** ★★ kind:`integration` be:`none`
  - seam: Startup fields edit + debounce
  - 観測: cache sidecar の `scenario.start/end/granularity/initial_cash` が更新される
- [ ] **J7 instruments_ref_fail_closed** ★★★ kind:`integration` be:`none`
  - seam: `instruments_ref` が欠落・破損・空の sidecar を load
  - 観測: `ScenarioLoadedFromFile` 不発、Run disabled、error 表示。正常 ref では instruments 解決
- [ ] **J8 instrument_picker_search_add_remove** ★★★ kind:`ui` be:`mock`
  - seam: `+ Add` → search 入力 → candidate click → row remove
  - 観測: 100ms debounce、最大 15 行、`Tickers` / `AvailableInstruments` 由来候補、scenario instruments 更新、Chart spawn/despawn
- [ ] **J9 instrument_picker_placeholders_and_readonly** ★★ kind:`ui` be:`mock`
  - seam: Replay end 未設定 / Live venue 未接続 / loading / error / no matches / `instruments_ref`
  - 観測: `Set scenario.end first` / `Venue not connected` / `Loading...` / `Error:` / `No matches`、readonly 時に追加削除不可

## K. Chart / Panels / Orders UI（描画・フォーム操作）

- [ ] **K1 panel_spawn_close_z_order_drag** ★★ kind:`ui` be:`none`
  - seam: sidebar Panels buttons / close / drag / focus
  - 観測: Strategy Editor / Buying Power / Run Result / Positions / Orders が spawn、close、z-order 更新、drag 位置反映
- [ ] **K2 chart_zoom_pan_reset_autoscale** ★★★ kind:`ui` be:`mock`
  - seam: wheel / Ctrl+wheel / left drag / right or middle drag / double click
  - 観測: `ChartViewState` の time/price scale、camera pan/zoom、autoscale enabled が期待値に遷移
- [ ] **K3 chart_ladder_live_mode_depth_render_state** ★★ kind:`state` be:`mock`
  - seam: Live mode + `InstrumentTradingData.depth` 更新
  - 観測: Ladder pane spawn、21 行固定、ask/bid/last/no-depth placeholder、Replay に戻すと despawn
- [ ] **K4 order_panel_place_confirm_cancel_modify** ★★★ kind:`ui` be:`mock`
  - seam: order form 入力 → submit → confirm → context menu cancel/modify
  - 観測: `PlaceOrder` / `CancelOrder` / `ModifyOrder` command、confirm modal、modify modal、feedback line
- [ ] **K5 secret_modal_submit_retry_timeout_escape** ★★★ kind:`ui` be:`mock`
  - seam: `SecretRequired` → secret 入力 → submit / reject / timeout / Escape
  - 観測: secret は resource/log に残らない、`SubmitSecret` 発行、`SecretSubmitFailed` は modal error、retry 可能
- [ ] **K6 reconcile_modal_after_backend_restart** ★★ kind:`state` be:`mock`
  - seam: working orders がある状態で backend crash→ready→`OrdersReconciled`
  - 観測: `GetOrders` 発行、unknown orders が `ReconcilePrompt` に入り、dismiss で消える
- [ ] **K7 run_result_positions_orders_buying_power_render_state** ★★ kind:`ui` be:`mock`
  - seam: `PortfolioLoaded` / `AccountEvent` / `RunComplete`
  - 観測: 各 panel text/table が resource と一致、空状態も崩れない

## L. CLI / 実 backend / リリース smoke

- [ ] **L1 cli_strategy_replay_outputs_run_buffer** ★★★ kind:`integration` be:`real`
  - seam: `python -m engine.strategy_replay run` / `scripts/run_replay.ps1`
  - 観測: stdout summary、`meta.json` / `fills.jsonl` / `equity.jsonl` / `summary.json` 生成
- [ ] **L2 grpc_backend_real_startup_and_health** ★★★ kind:`integration` be:`real`
  - seam: `.venv/bin/python -m engine` 起動 → Hello/GetState/ListInstruments
  - 観測: Ready、schema version、health、shutdown
- [ ] **L3 live_venue_prod_guard_release_gate** ★★★ kind:`integration` be:`real`
  - seam: `TACHIBANA_ALLOW_PROD` / `KABU_ALLOW_PROD` 未設定で Prod login
  - 観測: backend が接続拒否、UI は `VenueState::Error` または login command 抑止。CI では env isolated smoke、実口座接続は manual-gate
- [ ] **L4 full_window_render_smoke** ★★ kind:`render` be:`mock`
  - seam: `BACKCAST_E2E=1` で固定 fixture 起動
  - 観測: 主要 panel、footer、menu、chart、modal のスクリーンショット/構造化ログが baseline と一致

---

## 直接駆動では不可能な場合の代替方式

この節は「テストしないもの」ではない。既存の resource/event/system 直接駆動ハーネスだけではユーザー操作の忠実度が足りない場合に、採用する代替方式を定義する。

| 対象 | 直接駆動だけで不足する理由 | 代替方式 | release gate |
|---|---|---|---|
| メニュー開閉 / Alt+F/E/V | backend seam を通らず、keyboard focus と UI entity 表示が本体 | `kind:ui`。`ButtonInput<KeyCode>` と `Interaction` を注入し `OpenMenu` / entity 表示を assert | I1/I3 必須 |
| モード切替 gating | command を送らないことが仕様で、backend ack では観測できない | `kind:ui`。送信 channel を監視し「未送信」を assert | I4 必須 |
| OS ファイルダイアログ | CI で OS native dialog を安定操作しにくい | dialog 自体はバイパスし、選択済み path event/resource を注入。別途 smoke で起動確認 | I5 + L4 |
| レイアウト永続化 | ファイル I/O と debounce が主対象 | temp dir fixture で `Save/Load` を integration 実行し JSON と復元 entity を assert | I7/I8 必須 |
| cosmic_edit 入力 | text editor plugin の focus/keyboard 処理が主対象 | `kind:ui`。focused entity と keyboard/text input を注入。必要なら最小実ウィンドウ smoke を追加 | J1-J4 必須 |
| Startup パネル入力検証 | Run command を送らない UI gating が仕様 | `kind:ui`。field editor state、error label、transport channel 未送信/送信を assert | J5/J6 必須 |
| `instruments_ref` fail-closed | file-watch / parser / writeback の連携 | temp sidecar/ref file を使う integration。破損・空・正常の fixture を固定 | J7 必須 |
| 銘柄ピッカー | searchbox、debounce、候補表示、readonly が純 UI | `kind:ui`。time advance と text/entity assert。取得 seam は C1-C4 と組み合わせる | J8/J9 必須 |
| Chart 操作 | wheel/drag/double click と render state が主対象 | `kind:ui` で `ChartViewState` / camera を assert。描画崩れは `kind:render` smoke | K2/K3 + L4 |
| 注文フォーム / modal / context menu | 2 段階 confirm、focus、Escape 優先順位が主対象 | `kind:ui`。command channel、modal visibility、feedback resource を assert | K4/K5/K6 必須 |
| Prod guard / 実 venue | CI で実口座・外部環境に依存 | env isolated backend integration で guard を確認。実接続はリリース時 manual-gate に残す | L3 必須 |
| 画面全体の見た目 | headless resource assert では重なり・欠落を検出しづらい | `BACKCAST_E2E=1` 固定 fixture 起動、スクリーンショットまたは構造化 UI dump の smoke | L4 必須 |

---

## 実装状況（2026-05-21 時点 / branch: docs/wiki）

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
- ✅ **G3 backend_disabled_sim 追加（25 passed）**: `Harness::with_backend_enabled(bool)` に切り出し、
  `new_backend_disabled()` で `backend_enabled=false` のハーネスを構築。`backend_enabled()` アクセサで
  footer `grpc: DISABLED` 条件を、`push_state` 後の `timestamp_ms` 不変で `backend_update_system` の
  early-return（send no-op）を観測。
- ✅ **A9 replay_startup_timeout 追加（26 passed）**: 本番 `replay_startup_timeout_system` をハーネスの
  `Update` に登録し、`arm_startup_timeout(id)` で startup window を開いて `started_at_elapsed` を採番、
  `advance_real_time(dur)` で headless `Time<Real>` を進めて駆動。59s で error なし→61s で
  `error` セット（phase 不変）→ `close_startup_window()` でクリアを観測。
- ✅ **D5 venue_logout_detected 追加（`cargo test --test e2e_replay` で 35 passed）**: Phase 9 Step 7
  （health watchdog）マージで `backend_event_drain_system` が `VenueLogoutDetected` 受信時に新規
  `ReloginPrompt.active = Some(venue)` をセットするようになり、保留中だった D5 が観測可能になった。
  ハーネスに `ReloginPrompt` resource を insert し `relogin_prompt()` アクセサを追加。モーダルは通知に
  徹し自身は再ログインしないため `VenueStatusRes` は意図的に不変（本番の drift note 参照）。
- ✅ **event seam 3 本 + 注文 RPC 5 本追加（34 passed）**: Phase 9
  ライブ口座・発注 API のマージで `backend_event_drain_system` / `apply_status_update` に reducer が
  入り、event seam が観測可能になった。`tests/e2e/support/mod.rs` に `live_orders()` /
  `order_feedback()` / `secret_prompt()` アクセサを追加。
  - **F3 order_event**=`OrderEvent` → `LiveOrders.apply_event`（未知 id は static 空で挿入、同 id は
    in-place マージ）。**F4 account_event**=`AccountEvent` → `apply_account_event`（cash/buying_power/
    positions/loaded、`equity = cash + Σ(qty*avg_price + unrealized_pnl)`）。**F5 secret_required**=
    `SecretRequired` → `SecretPrompt.active` が `Some`。
  - **H1 order_seeded**=`OrderSeeded` → full レコード seed + `OrderFeedback` クリア。
    **H2 order_status_updated**=`OrderStatusUpdated` → `apply_event` マージ（static 保持）。
    **H3 order_modified**=`OrderModified` → `apply_modify`（`Some` のみ上書き・`None` 不変）。
    **H4 order_rejected**=`OrderRejected` → `OrderFeedback.message` に整形メッセージ。
    **H5 exec_mode_change_resets_portfolio**=実モード変更で `PortfolioState` を default リセット
    （同一モードは no-op）— Live/Replay 口座データ混線防止の回帰の肝。
- ⬜ **既存 state ハーネスでは未完（代替方式で release gate 化）**:
  - **A5 set_speed**: `BackendStatusUpdate` に speed ack variant が無い。transport task の lib 抽出後に state flow、当面は footer UI の `SetSpeed` command 発行を UI harness で検証する。
  - **C5 select_instrument**: `SelectedSymbol` は UI 駆動のみで backend→ECS seam を通らない。[J8]/[K2] の UI harness で銘柄選択から chart 更新まで検証する。
  - **D8 prod_guard**: env 二重ガードは backend 側ロジック。[L3] の backend integration とリリース時 manual-gate で担保する。
- 📌 **設計メモ**: 現行 v1 state harness は backend→ECS の片側（`BackendStatusUpdate` 注入）を駆動する。
  反対側（`TransportCommand`→gRPC→`BackendStatusUpdate`）は既に `tests/backend_integration.rs`
  が mock tonic サーバでカバー済み。今後はこれに `kind:ui` / `kind:render` / `kind:integration`
  を足し、ユーザー行動ベースの release gate として end-to-end を完成させる。完全な単一プロセス
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
