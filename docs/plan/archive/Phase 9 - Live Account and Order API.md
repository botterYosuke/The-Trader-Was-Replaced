# Phase 9: Live Account & Order API — Implementation Plan

> **前提**: Phase 8 (Live Venue & Market Data) が完了し、`LiveVenueAdapter` で read-only な市場接続・銘柄メタデータ・depth 購読が動作する状態を出発点とする。Phase 9 では **初めて発注経路を握る** ため、`ExecEngine` のインスタンス化、Tachibana 第二暗証番号の都度収集 UX、口座状態同期の 3 本柱を導入する（kabu は Password 不要）。
>
> 上位計画 [Transparent Headless Replay](./archive/Tranceparent%20Headless%20Replay.md) の §Phase 9 「口座情報の同期と注文機能の実装」を具体化する。

---

## 進捗 (Status)

> 最終更新: 2026-05-20

| Step | 内容 | 状態 |
| --- | --- | --- |
| 0 | Backend Event Transport (`SubscribeBackendEvents`) | ✅ 完了 — `96c7370c` + レビュー修正 `e221399e` |
| 1 | MockVenueAdapter 発注経路 + SecretVault + SubmitSecret RPC | ✅ 完了 (2026-05-20、未コミット) |
| 2 | 手動発注 facade + Order RPC + OrderEvent stream | ✅ 完了 (2026-05-20、未コミット) |
| 3 | OrderPanel UI + SecretModal UI | ✅ 完了 (2026-05-20、未コミット) |
| 4 | Account 同期 + Panel Live 対応 | ✅ 完了 (2026-05-21、未コミット) — account_sync + AccountEvent push + ModifyOrder(配線まで) + OrdersPanel 右クリックメニュー |
| 5 | TachibanaExecutionClient | ✅ 完了 (2026-05-21、未コミット) — CLMKabuNewOrder/Correct/Cancel + 第二暗証番号都度収集 + EC→OrderEvent。**EC 情報コードは e-station 参照実装で確定**。残課題: 口座レベル EC 購読 URL 構成 + `build_event_url` の comma %2C 問題（要 Demo 検証） |
| 6 | KabusapiExecutionClient | ✅ 完了 (2026-05-21、未コミット) — POST /sendorder + PUT /cancelorder + 訂正=取消→新規変換(補償) + GET /orders 1s polling→OrderEvent + fetch_account。**proto / Rust 変更ゼロ**。残課題: 実 verify 環境 smoke (§5.1 layer-3) + AccountType の config 化 |
| 7 | Venue Health Watchdog | ✅ 完了 (2026-05-21、未コミット) — kabu `check_health`(GET /apisoftlimit→4001007/4001017) + `live/health_watchdog.py`(poll型・debounce) + Tachibana SS=閉局 push 検知(⚠️TENTATIVE) + server_grpc 配線(`_publish_venue_logout`) + Rust ReloginModal(通知のみ) |
| 8 | Backend Auto-Restart + Idle Shutdown | ✅ 完了 (2026-05-21、未コミット) — **§3.7 Idle gRPC Shutdown ✅** (`live/idle_shutdown.py` ほか、既コミット)。**§3.8 Auto-Restart + in-flight re-sync ✅** — supervisor を crash-loop budget (60s/3回) の session loop に再構成 (`CrashBudget` + auto-respawn + 手動 `Restart` 機能化) + `GetOrders` proto RPC 新設 (facade.list_orders→稼働中注文) + Rust 再起動後 reconcile (`backend_restart_resync_system`→GetOrders→diff→`ReconcilePrompt`→通知モーダル) |
| 9 | Instruments Daily Refresh | ✅ 完了 (2026-05-21、未コミット) — `instruments_store.py`(atomic parquet write/read) + `instruments_scheduler.py`(login時persist + 営業日5:00 JST更新) + server_grpc 配線(store-first ListInstruments)。**ドリフト訂正**: J-Quants `/markets/trading_calendar` クライアントと「Phase 8 がログイン時に全置換する parquet store」は**いずれも未実装だった**ため、① 営業日カレンダーは持たず venue の fetch_instruments エラー/空に委ねる、② parquet store を本 Step で新設しフル結線 (ユーザー決定) |
| 10 | Polish | ✅ 完了 (2026-05-21、未コミット) — secret masking 再検証（**`mask_secrets` が proto wire field `second_secret` を伏字にしていなかった漏れを発見・修正** + SecretVault pickle/repr/TTL 失効テスト）+ drawio アーキ図 `docs/assets/phase9-architecture.drawio.svg` + Phase 10 引き継ぎ doc `docs/plan/phase9-to-phase10-handoff.md` |

### Step 0 完了サマリー (2026-05-20)

- **proto**: `SubscribeBackendEvents` server-streaming rpc + `BackendEvent` oneof（`secret_required` / `order_event` / `account_event` / `venue_logout_detected`、nested `AccountPosition`）
- **Python**: `live/backend_event_bus.py` 新設 — threadsafe `BackendEventBus`（`queue.Queue` fan-out）。`servicer.publish_backend_event()` + streaming handler（token 認証、`context.add_callback(sub.close)` で RPC teardown 時に `queue.get()` を解放し ThreadPool join ハングを防止）
- **Rust**: `trading::BackendEvent` ミラー enum + `AccountPosition` + `BackendEventChannel` resource + `backend_event_drain_system`（**現状ログ出力のみ**）。`setup_backend_connection` に再接続 subscriber タスク（独自 client・Ready-gated・connect/subscribe/stream-end の全失敗パスを 500ms backoff で self-heal）
- **テスト**: BackendEventBus unit 6 + gRPC streaming 2（token 認証 / push 配信 / cancel 後の subscription 除去）= Python 8 passed、`backcast` bin 22 passed（レビュー修正後に再確認）。初回コミット時に Python 全体 907 passed / 11 skipped を確認済み
- **レビュー修正** (`e221399e`): events 再接続の connect/subscribe 失敗も 500ms backoff で self-heal（永久 stall 回避）/ streaming テストの無限ハング回避（subscription 登録待ち + 5s deadline + cancel 後除去 assert）/ `BackendEventBus.subscriber_count()` 追加
- **⚠️ 計画書ドリフト訂正**: §3.12 / §4 / §5 を実装に合わせて訂正（① `asyncio.Queue`/`LiveEventBus` → threadsafe `BackendEventBus`、② `src/backend_client.rs` → `src/main.rs`、③ Step 0 はログのみで `on_account_event`/`on_order_event` 結線は Step 4）
- **次**: Step 1（MockVenueAdapter 発注経路 + SecretVault）

### Step 1 完了サマリー (2026-05-20)

- **SecretVault** (`python/engine/live/secret_vault.py` 新設): Tachibana 専用 secret 仲介。公開 API は `create_request(venue,purpose)->rid`（同期）/ `await wait_for(rid,timeout)->secret` / `submit(rid,secret)->None`（同期）/ `get(venue,purpose)->str|None`（同期）。`threading.Lock` で並行制御（gRPC は sync ThreadPool、`submit` は worker thread・`wait_for` は live-loop thread で走る cross-thread 構造のため `asyncio.Lock` 不可）。cross-thread の Future 解決は `future.get_loop().call_soon_threadsafe`、TTL 60s（保管時刻起点・再利用でリセットしない・purpose 別独立）。Future は `wait_for` が running loop 上で遅延生成（`create_request` は loop 非依存）。平文 secret は `_store` のみが保持し TTL で削除、`_pending` は timeout 時掃除（§6 整合）。unit test 11。
- **MockVenueAdapter.submit_order** (`mock_adapter.py`) + **OrderResult** (`python/engine/live/order_types.py` 新設): mock 発注経路。成功 `FILLED` / 失敗 `REJECTED` / 部分約定 `PARTIALLY_FILLED` を `set_next_order_outcome` で注入。async・kwargs 拡張可（§9）。`OrderResult` は proto `OrderEvent` と field 一致。`LiveVenueAdapter` Protocol は不変（mock 固有メソッド、発注は本来 ExecutionClient の責務）。unit test 12。
- **SubmitSecret RPC**: proto に `rpc SubmitSecret(SubmitSecretReq{token,request_id,secret}) returns (SubmitSecretRes{success,error_code})` 追加（secret は Req のみ・Res/ログに残さない、§1.3）。`server_grpc.py` に `SecretVault` の最小 wiring + handler（bad token → `UNAUTHENTICATED`、unknown request_id → `success=false`/`error_code="UNKNOWN_REQUEST_ID"`、正常 → `vault.submit` 後 `success=true`）。Rust mock (`tests/backend_integration.rs` の `MyDataEngine`) に `submit_secret` stub 追従。Python handler test 3 + Rust backend_integration 10 passed。
- **テスト**: 新規/変更分 26 passed（secret_vault 11 + mock_adapter 12 + submit_secret 3）。全体回帰 919 passed / 11 skipped。**pre-existing 失敗 4 件**（`test_grpc_shutdown` ×3 / `test_grpc_startup_sentinel` ×1）は Windows の `select.select` on pipe FD（`WinError 10038`）による test-harness 制約で本 Step と無関係（`python -m engine` 手動起動は `GRPC_LISTENING` で正常起動を確認）。
- **計画書ドリフト訂正**: §3.1 の `asyncio.Lock` → `threading.Lock`（cross-thread 構造のため）。
- **Step 2 への申し送り**: ① SecretVault に live-loop 参照を注入し、submit-先行/no-loop 経路でも TTL を arm（現状は本番 wait_for-先行 flow で arm されるため実害なし）。② `_targets`/`_ttl_armed` の長期掃除。③ grpc-server test fixture の `conftest.py` 共通化（任意）。
- **次**: Step 2（ExecEngine 有効化 + OrderEvent stream）

### Step 2 完了サマリー (2026-05-20)

- **アーキ判断 (ADR §7)**: §3.1 が留保していた「NautilusKernel フル起動 vs 個別 wire」は **軽量手動発注 facade（選択肢 B）** に確定。live パイプラインは bespoke のまま、真正 Nautilus ExecEngine/RiskEngine wiring は **Phase 10 / LiveAuto に延期**（移行順序: thin facade → LiveExecutionClient adapter 化 → full live engine）。
- **proto** (`engine.proto`): `PlaceOrder` / `CancelOrder` / `GetOrderStatus` rpc + `PlaceOrderReq/Res` `CancelOrderReq/Res` `GetOrderStatusReq/Res` message を追加。各 Res は `OrderEvent order_event` を inline で返す。`GetOrderStatus` は当初案 `returns (Order)` を `GetOrderStatusRes` に変更（`Order` message は `OrderEvent` 重複のため新設せず）。Python `engine_pb2`/`_grpc` 再生成（grpc_tools.protoc → 相対 import に修正）、Rust は build.rs(tonic) で自動再生。
- **`live/order_facade.py` [NEW]**: transport 非依存の `ManualOrderFacade`。`place()` / `cancel()`（adapter.submit_order / cancel_order 委譲）/ `get_status()`（in-memory order store、同期参照）。検証エラーと取消拒否は `OrderFacadeError(error_code)`。proto を import しない（変換と push は handler 責務）。`second_secret` は受理して無視（SecretVault 結線は Step 5、平文を adapter kwargs に転送しない）。
- **`order_types.py`**: `OrderEventData`（proto `OrderEvent` と field 一致の正規化モデル）追加。
- **`mock_adapter.py`**: `cancel_order` + `set_next_cancel_outcome` 追加（既定 CANCELED / 注入 REJECTED）。
- **`server_grpc.py`**: facade を live session lifetime で wire（`_start_live_components_async` で生成、teardown で None）。`PlaceOrder`/`CancelOrder`/`GetOrderStatus` handler を追加。write 系は `current_mode != Live*` を `EXECUTION_MODE_PRECONDITION`、facade 未起動を `VENUE_LOGIN_REQUIRED`（structured error、house style）で reject。成功時は `OrderEvent` を `publish_backend_event` で push しつつ unary response にも inline 返却。read 系 `GetOrderStatus` は Replay でも reject せず、session 無しは `NO_LIVE_SESSION`。
- **Rust** (`tests/backend_integration.rs`): proto 追加で tonic `DataEngine` trait に増えた `place_order`/`cancel_order`/`get_order_status` の mock stub を追従（Step 1 の submit_secret stub と同様）。`cargo check --tests` green。src 側は client のみ使用のため変更不要。
- **テスト**: 新規 38 passed（order_facade unit 9 + mock cancel 3 + grpc place/cancel/get 11 ＋ 既存 mock_adapter 等の再走分）。全体回帰 **986 passed / 35 skipped / 7 failed**。**7 失敗はすべて pre-existing**（`git stash` でクリーン HEAD でも同一に再現）: ① Windows pipe FD の `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1、② `StartEngine` の strategy_file 必須化（コミット `d4ca180a`）に未追従の `test_grpc_catalog_route`×1 / `test_jquants_to_catalog`×2。本 Step 由来の新規失敗は 0。
- **Step 3 への申し送り**: ① RPC は実装済みだが Rust UI（OrderPanel / SecretModal）からの発射経路は未配線（`TransportCommand` に `PlaceOrder`/`CancelOrder` variant 追加が必要、id=2434 メモ: 非 Copy payload で `#[derive(Clone)]` のみに）。② OrderEvent push は `backend_event_drain_system` まで届くがログ出力のみ（Snapshot Reducer 結線は §3.12 / Step 4）。③ `Replay` reject は `EXECUTION_MODE_PRECONDITION` の structured error（grpc abort ではない）。④ 2 段階確認モーダル・kabu 訂正警告バナーは Step 3 / Step 6。⑤ 新 error_code（`ORDER_NOT_CANCELABLE` / `PLACE_TIMEOUT` / `CANCEL_TIMEOUT` / `INVALID_INSTRUMENT` / `INVALID_VENUE`）の UI トースト文言を用意すること（下記レビュー修正で追加）。
- **レビュー修正 (2026-05-20、Step 2 後段)**: parallel review（nautilus / 並行性 + tachibana/kabu forward-compat）で検出した Medium 以上を修正:
  - **TOCTOU**: `PlaceOrder`/`CancelOrder`/`GetOrderStatus` handler で `self._order_facade` を local に snapshot（teardown が並行で None 化する race。特に `GetOrderStatus` は try/except 無しで `AttributeError` が gRPC INTERNAL として漏れていた → 一貫して `NO_LIVE_SESSION`/`VENUE_LOGIN_REQUIRED` を返す）。
  - **取消の終端ガード**: `ManualOrderFacade.cancel` が終端注文（FILLED/CANCELED/REJECTED/EXPIRED/DENIED）を venue に無駄打ちし、矛盾イベント（FILLED 注文が filled_qty 全量のまま "CANCELED" 化）を publish していた → `ORDER_NOT_CANCELABLE` で事前 reject（plan §1.2 OrderStateMachine 準拠）。
  - **入力検証**: NaN/Inf qty/price は `<= 0` を素通りするため `math.isfinite` を追加。空 `instrument_id`/`venue` も reject。
  - **status 契約固定**: `OrderResult.status` を Nautilus OrderStatus 14 名に pydantic validator で限定（実 venue adapter の typo `"CANCELLED"` 等が UI へ素通りするのを境界で防ぐ）。
  - **timeout 区別**: `future.result` の `TimeoutError` を `PLACE_TIMEOUT`/`CANCEL_TIMEOUT` で区別（mock では発生しないが、実 venue では「注文成立だが応答 timeout」を UI に明示する。reconcile は Step 8）。
  - テスト: Step 2 スイート 45 passed（+7: 終端ガード / NaN / 空 instrument / status validator 等）。proto・Rust は不変。
- **Step 5/6（実 venue）への申し送り — forward-compat（今回は実装せず記録のみ。proto field 追加は後続でも非破壊）**:
  - **Tachibana `CLMKabuCancelOrder` は `sOrderNumber` + `sEigyouDay`（営業日）の 2 識別子が必須**。現 `OrderEvent`/`OrderEventData`/`OrderResult` は `venue_order_id`（→`sOrderNumber`）のみで `eigyou_day`（注文日）を運ぶ field が無い。Step 5 で発注時に `CLMKabuNewOrder` 応答の `sEigyouDay` を取得し取消で再供給する必要があるため、`OrderEvent` に `optional string order_date`（または汎用 `venue_ids` map）を additive 追加する。kabu は `OrderID` のみで取消するため空で良い。
  - **venue 固有発注パラメータ**（cash/margin、口座種別、市場コード、`sZyoutoekiKazeiC` 等）の経路: `OrderingVenueAdapter.submit_order/cancel_order` は `**extra` を持つが、`PlaceOrderReq`/`CancelOrderReq` proto と facade は generic 7 field しか通さない。`PlaceOrderReq` に `map<string,string> venue_params` を足し handler→facade→`**extra` で透過させる方式を Step 5 で確定する。
  - **secret チャネルの一本化**: `PlaceOrderReq.second_secret` と `SecretRequired`→`SubmitSecret`（§1.3 SecretVault、TTL reuse）の 2 経路が併存。facade は `second_secret` を意図的に終端し adapter に渡さないため、実経路は SecretVault 側になる。Step 5 着手時にどちらを正とするか決め、冗長/競合チャネルを排除する。
- **次**: Step 3（OrderPanel UI + SecretModal UI）

### Step 3 完了サマリー (2026-05-20)

- **アーキ判断 (ユーザー選択)**: OrderPanel/モーダルは **Bevy UI Node + Interaction** 流派で実装（instrument_picker / menu_bar と同系統）。表示専用の world-space sprite floating window（buying_power 等）とは別。よって `PanelKind` / `panel_spawn_dispatcher_system` は経由せず、**Startup で 1 度 spawn し `Display` で出し入れ**する（sidebar/footer/menu と同じ）。⇒ 計画書 §3.9 の「floating window + WindowLayout 永続化」前提から**ドリフト訂正**（egui は Phase 7 で完全撤去済みのため、操作系 UI は UI Node が唯一の選択肢）。
- **`src/ui/order_panel.rs` [NEW]**: `OrderForm`（side/type/qty/price/tif）+ `OrderConfirm`（2 段階確認の pending）resource。pure fn `validate_order`（数量＝売買単位の倍数 / 指値の価格＝呼値の倍数 / 銘柄未選択を弾く。`tick_size` は価格刻みで数量検証に使わない）/ `build_draft`（成行は price を落とす）/ `estimated_notional`（指値=価格・成行=直近約定価格）。`[発注]`→検証→`OrderConfirm.pending` セット→中央オーバーレイの確認モーダル（銘柄/売買/数量/価格/概算約定額を再表示）→`[Confirm]` で初めて `TransportCommand::PlaceOrder` 発射、`[Cancel]` は破棄。`ExecutionMode == LiveManual` のときだけ `Display::Flex`。**売買単位 100 / 呼値 1.0 はデフォルト定数**（実 instrument metadata が Rust 側 state に未流入のため。実連動は Step 4/5 の TODO）。
- **`src/ui/secret_modal.rs` [NEW]**: `SecretRequired`（main の drain が `SecretPrompt.active` にセット）で開くモーダル。入力は **keyboard イベント drain → `Zeroizing<String>` バッファ**（picker_searchbox と同じ drain。cosmic_edit ではない）。`[送信]`/Enter→`SubmitSecret` 発射＋バッファ zeroize＋close、`[キャンセル]`/Esc→破棄、**25s timeout** で auto-close。マスク表示は `•`×len。⇒ §3.10 の「cosmic-edit 1 行 password モード」から**ドリフト訂正**（UI-node 向け cosmic password の実績が無く buffer zeroize も困難。実 AC §6「明示保持しない / zeroize / 平文を残さない」は keyboard-drain + `Zeroizing` の方が確実に満たす）。
- **`src/trading.rs`**: `TransportCommand::{PlaceOrder, CancelOrder, SubmitSecret}` 追加（非 Copy・Clone のみ、id=2434 メモ準拠）。`RedactedSecret`（`Zeroizing<String>` ラッパ、`Debug` を `RedactedSecret(***)` に伏字化＝コマンドの `{:?}` で平文が漏れない）。`LiveOrders`/`LiveOrder`（UI が握る Live 注文簿、`client_order_id` キー、`upsert_full`＝発注応答の静的 field seed / `apply_event`＝OrderEvent の status・fill マージで静的 field 保全）。`SecretPrompt`/`SecretPromptRequest`。`BackendStatusUpdate::OrderUpserted`。
- **`src/main.rs`**: 発注 RPC の dispatch arm 3 種（token はトランスポートタスクが注入）。`PlaceOrder`/`CancelOrder` 応答の `OrderEvent`（ids+status+fill のみ）を**コマンドの静的 field（symbol/side/qty/price）とマージ**して `OrderUpserted` を `status_tx` で送出（OrderEvent に symbol 等が無いため）。`backend_event_drain_system` を**ログのみ→結線**に: `SecretRequired`→`SecretPrompt.active`、`OrderEvent`→`LiveOrders.apply_event`（§3.12 entry 側の最小 reducer。Account/Position は Step 4）。`apply_status_update` に `OrderUpserted` arm（symbol 空=Cancel 応答→status マージ / 非空=Place 応答→full seed）。
- **`src/ui/orders.rs`**: Live mode は `LiveOrders` を、Replay は従来 `PortfolioState.orders` を表示（成行は価格列 "MKT"）。
- **`Cargo.toml`**: `zeroize = "1"`。
- **テスト**: 新規 lib unit 約 25（trading: LiveOrders upsert/merge 5 + RedactedSecret 伏字 1、order_panel: validate/build/notional 9 + system 5、secret_modal: lifecycle/submit/cancel/timeout/mask 6）。全体回帰 **lib 449 / bin 22 / integration 10 passed, 0 failed**。`cargo check --all-targets` green。clippy は本 Step 由来の新規は doc 整形 1 件のみ修正（type_complexity / too_many_arguments は既存 house style と同型・既存ベースラインに 80 件あり -D 非運用）。
- **計画書ドリフト訂正**: ① OrderPanel/モーダルは UI Node 流派（PanelKind/dispatcher 不使用、Startup spawn + Display）。② SecretModal は cosmic_edit でなく keyboard-drain + `Zeroizing`。③ `OrderEvent`→OrdersPanel の**最小 reducer を Step 3 で実装**（Step 2 申し送り②「ログのみ／reducer は Step 4」を entry 側だけ前倒し。Account/Position reducer は Step 4 のまま）。
- **Step 4 への申し送り**: ① `AccountEvent`→`PortfolioState`（cash/buying_power/positions）reducer 結線（drain は現状 AccountEvent をログのみ）。② OrdersPanel 右クリック→[取消]/[訂正] コンテキストメニュー（`CancelOrder` コマンドは配線済み、UI トリガ未）。③ 実 instrument metadata（売買単位/呼値）を Rust state に流して order_panel の定数 100/1.0 を置換。④ 銘柄手動 override（現状 `SelectedSymbol` 連動のみ）。⑤ 概算手数料テーブル（現状 notional のみ・「手数料概算は未対応」表示）。⑥ secret_modal_input の keyboard 消費が menu/picker と競合し得る順序（モーダル稀・Tachibana 専用のため未調整、`.before(InputSet)` のみ）。⑦ **E2E（mock LiveManual で 発注→約定→OrdersPanel 表示）は未実施**（unit/system test でロジックは網羅。GUI+backend E2E は e2e-testing で別途）。⑧ kabu 訂正警告バナー（§2.3 / §3.11）は Step 6。
- **次**: Step 4（Account 同期 + PositionsPanel/OrdersPanel Live 対応）

### Step 4 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（Python バックエンド + Rust フロントエンドの両半 + proto `ModifyOrder` 追加が統合され全テスト緑、2026-05-21、未コミット）。Rust **506 passed / 0 failed**（lib 469 + bin 27 + backend_integration 10）、Python **1024 passed / 35 skipped / 7 failed**（7 失敗はすべて pre-existing baseline = Windows pipe FD `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1、`strategy_file` 必須化未追従 `test_grpc_catalog_route`×1 / `test_jquants_to_catalog`×2。本 Step 由来の新規失敗 0、Step 2 比 +38 passed）。`cargo check --all-targets` 緑。

- **A. AccountEvent → PortfolioState reducer 結線** (`src/main.rs`): ✅
  - `backend_event_drain_system` に `mut portfolio: ResMut<PortfolioState>` を追加し、`BackendEvent::AccountEvent` をログのみ→pure fn `apply_account_event(&mut portfolio, cash, buying_power, positions)` に結線。`cash`/`buying_power` をセット、`AccountPosition`→`PortfolioPosition` map、`loaded=true`。
  - **equity 計算式**: `AccountEvent` に `equity` field が無いため、`equity = cash + Σ(qty as f64 * avg_price + unrealized_pnl)` で導出（建玉時価 ≈ 取得簿価 `qty*avg_price` + 評価損益 `unrealized_pnl`。venue の `unrealized_pnl` が同じ `avg_price` 基準で計算されていれば真の時価と一致する近似）。pure fn 化して unit test 2 件（複数建玉の equity 検算 / 建玉ゼロ時 equity==cash）。
  - **Res 追加の波及**: 本番 `main.rs` の `insert_resource(PortfolioState::default())` は既存。`backend_event_drain_system` を踏む既存テストは無く、追従漏れ panic は発生しなかった（`status_update_system` 経由は別 system）。
- **B. ModifyOrder コマンド経路** (`src/trading.rs` + `src/main.rs`): ✅
  - `src/trading.rs`: `TransportCommand::ModifyOrder { venue, client_order_id, new_qty: Option<f64>, new_price: Option<f64>, second_secret: Option<RedactedSecret> }`（Step 3 の derive・secret 伏字流儀踏襲）。`BackendStatusUpdate::OrderModified { client_order_id, venue_order_id, new_qty, new_price, status, filled_qty, avg_price, ts_ms }`。`LiveOrders::apply_modify(...)`（既存レコード検索 → symbol/side 保持、`new_qty`/`new_price` が Some なら上書き、status/fill/venue_order_id/ts_ms 更新、空 venue_order_id は維持、**未知 id は no-op**）。unit test 3 件。
  - `src/main.rs`: `TransportCommand::ModifyOrder` dispatch arm（`engine::ModifyOrderReq` を組み `new_price`/`new_qty` を Option→proto optional、`client.modify_order(req)`、成功時 `OrderEvent`（ids+status+fill）とコマンドの `new_qty`/`new_price` をマージして `OrderModified` を `status_tx` 送出、失敗時 `OrderRejected{action:"訂正"}`）。`apply_status_update` に `OrderModified` arm → `live_orders.apply_modify(...)`。
  - **設計判断（qty/price マージ）**: `OrderEvent` は qty/price を運ばないため、Modify の qty/price は **`ModifyOrder` コマンド由来の値を transport task でマージ**して `OrderModified` に載せる（Step 3 の Place/Cancel と同じ「コマンド静的 field をマージ」方針）。
- **C. OrdersPanel 右クリック → コンテキストメニュー [取消]/[訂正]** (`src/ui/orders.rs` + `src/ui/order_context_menu.rs` [NEW]): ✅
  - **設計判断（実装方式）**: OrdersPanel は world-space Sprite/Text2d パネル。各データ行に**透明 Sprite のヒット領域**（`OrdersRowHit{row}`、`Color::srgba(_,_,_,0.0)` + `custom_size`、0.15 は bounds picking で alpha 無関係＝pickable）を `content_area` 子として spawn し、`.observe(Pointer<Down>)` で **Secondary ボタンのみ**反応（`down.event().button != PointerButton::Secondary { return }`）。Live モード時のみ・対象行に注文があるときのみ `OrderContextMenu`（`open`/`client_order_id`/`venue`/`screen_pos`）resource にセット。
  - メニュー本体は **Bevy UI Node オーバーレイ**（`order_context_menu.rs`、`GlobalZIndex(220)`、`Display::Flex/None`、`screen_pos`=`Pointer<Down>.pointer_location.position` でカーソル付近に配置）。[取消] / [訂正] は Button + Interaction。backdrop クリック / Esc で閉じる。
  - [取消] → `TransportCommand::CancelOrder { venue, order_id: client_order_id, second_secret: None }`（Step 3 で配線済み、venue は `VenueStatusRes.venue_id` から解決）。[訂正] → Modify モーダルを `client_order_id`/`venue` 付きで open。system test 4 件。
- **D. Modify モーダル** (`src/ui/modify_modal.rs` [NEW]): ✅
  - Bevy UI Node 中央オーバーレイ（`GlobalZIndex(250)`、order_panel/secret_modal 流儀）。新数量/新価格は **keyboard drain**（数字 `.` のみ受理、Tab でフォーカス切替、Enter=Confirm、Esc=破棄）。空欄は「変更しない（None）」扱い、`parse_buf` は空白/非有限/<=0 を None に。
  - **kabu 警告バナー**（§2.3 / §3.11）: `venue_capabilities::for_venue(venue).supports_order_correction == false`（=kabu）のとき上部に警告バナー（「kabuステーションには訂正 API がありません。取消→新規発注の 2 段階で訂正します。途中失敗で元注文のみ取消になることがあります。」）と同意チェックを表示し、**`ack_kabu` が ON になるまで Confirm を弾く**（`can_confirm()` = 変更あり ∧ (kabu でない ∨ ack 済み)）。Tachibana/MOCK は警告不要・チェック不要（`for_venue` で判定、文字列直書き回避）。
  - new_qty/new_price 両方空の場合は Confirm を弾きトースト相当（`OrderFeedback`）。`[Confirm]` → `TransportCommand::ModifyOrder { ..., second_secret: None }`（Step 4 は常に None、Tachibana secret 結線は Step 5）→ close。`[Cancel]`/Esc → 破棄。状態は `ModifyForm` resource。unit/system test 9 件。
  - keyboard drain は secret_modal と同じく `.before(InputSet)` / `.before(picker_searchbox_input_system)` / `.before(menu_keyboard_system)`。
- **E. mod.rs wiring + tests/backend_integration.rs**: ✅
  - `src/ui/mod.rs`: 新 module（`modify_modal` / `order_context_menu`）の Startup spawn（`spawn_order_context_menu` / `spawn_modify_modal`）+ Update systems を**新規 `add_systems` ブロック**に登録（既存 Phase 9 ブロックが 13 system で、追加 9 を足すと 20 tuple 上限超過のため分割）。`OrderContextMenu` / `ModifyForm` を `init_resource`。
  - `tests/backend_integration.rs`: mock `DataEngine` に `modify_order` スタブ追加（place/cancel と同形、`ModifyOrderRes{success:true,...}`）。
- **テスト (Rust)**: `cargo test --lib` 469 + `--bins` 27 + `--test backend_integration` 10 = **506 passed / 0 failed**（新規 16: trading apply_modify 3 + modify_modal 9 + order_context_menu 4 ＋ main の AccountEvent reducer 2）。`cargo check --all-targets` 緑。clippy の本 Step 由来 type_complexity 4 件（modify_modal ×2 / order_context_menu ×2）は order_panel/secret_modal と同型の multi-filter Query で house-style baseline（対象外）。
- **self-review (simplify)**: `context_menu_item_system` の `handled` フラグ + `break` を loop 内 early-return に簡素化。kabu 判定は文字列直書きでなく `venue_capabilities::for_venue` を再利用。redundant state なし。`apply_account_event` は引数 4 で閾値内・pure fn。

#### Python バックエンド (account_sync + AccountEvent push + ModifyOrder)

- **A. 口座データ型** (`python/engine/live/order_types.py`): `AccountPositionData`（symbol/qty/avg_price/unrealized_pnl）+ `AccountSnapshot`（cash/buying_power/positions、**ts_ms は持たない** — snapshot 等価判定から時刻を排除し push 時に handler が採番）。両方 frozen pydantic。
- **B. Adapter Protocol 拡張** (`adapter.py` `OrderingVenueAdapter`): `async def fetch_account() -> AccountSnapshot` + `async def modify_order(*, venue, order_id, new_price=None, new_qty=None, **extra) -> OrderResult`。Tachibana=CLMKabuCorrectOrder（atomic）/ kabu=取消→新規変換は Step 6 adapter の責務、Step 4 は mock のみ。
- **C. MockVenueAdapter** (`mock_adapter.py`): `fetch_account`（`set_account_snapshot` 注入、既定 cash=0/bp=0/positions=()）/ `modify_order`（`set_next_modify_outcome` 注入、既定 `ACCEPTED`）。
- **D. `live/account_sync.py` [NEW]**: `AccountSync(adapter, on_account_event, interval_s=30.0)`。transport 非依存（proto 非 import、reducer_bridge 思想）。**起動直後に 1 回必ず emit**（初期ロード = §3.12「初期ロード/手動リフレッシュ」を push で満たす ⇒ **新規 `GetAccount` RPC は追加せずドリフト訂正**）。以降は `interval_s` 毎 fetch し前回 emit snapshot と異なるときだけ emit（frozen pydantic `==`）。**fetch_account / callback 例外は warning ログ + `last_error` 記録してループ継続**（口座同期は best-effort、1 回失敗で永久停止させない）。`CancelledError` のみ正常終了。
- **E. `order_facade.py` `modify`**: 検証（UNKNOWN_ORDER_ID / 終端 `ORDER_NOT_MODIFIABLE` / 両方 None `NOTHING_TO_MODIFY` / NaN・Inf・<=0 の `INVALID_PRICE`/`INVALID_QTY`）→ `adapter.modify_order` → REJECTED は `MODIFY_REJECTED`（store 不変）→ 成功は status/fill/venue_order_id を更新した `OrderEventData` で store 更新。**`OrderEvent` は qty/price を持たない設計のため facade は qty/price を載せない**（UI 反映は Rust 側が `ModifyOrder` コマンド由来でマージ、上記 B 設計判断と対）。`second_secret` は受理して無視（Step 5 で結線）。
- **F. `server_grpc.py` 結線**: `_start_live_components_async` で facade 生成直後に `AccountSync(interval_s=30.0)` を生成・start。コールバック `_publish_account_snapshot` が `AccountSnapshot`→`engine_pb2.AccountEvent`（positions 詰め替え + `ts_ms=int(time.time()*1000)`）→ `publish_backend_event`（`BackendEventBus` は threadsafe queue ⇒ live loop thread から直接 publish して安全）。`_teardown_live_components_async` で `account_sync.stop()` を await、`_teardown_live_components` finally で `self._account_sync=None`、`__init__` で初期化。`ModifyOrder` handler を `CancelOrder` 雛形で実装（Replay→`EXECUTION_MODE_PRECONDITION`、facade None→`VENUE_LOGIN_REQUIRED`、`OrderFacadeError`→error_code、timeout→`MODIFY_TIMEOUT`、その他→`MODIFY_FAILED`、成功→publish + inline）。
- **テスト (Python)**: `test_account_sync.py` 5（初回 forced emit / 差分 emit / stop / fetch 例外継続 + last_error）+ facade modify 拡張 + mock fetch_account/modify_order + grpc ModifyOrder handler。本 Step 由来の新規失敗 0。

#### proto: `ModifyOrder` RPC 追加（オーケストレーター実施）

- `python/proto/engine.proto` に `rpc ModifyOrder (ModifyOrderReq) returns (ModifyOrderRes)` + `ModifyOrderReq{token=1,venue=2,order_id=3,new_price=4 opt double,new_qty=5 opt double,second_secret=6 opt string}` / `ModifyOrderRes{success=1,error_code=2,OrderEvent order_event=3}` を追加。Python pb2 を `grpc_tools.protoc` で regen し相対 import に手修正（`from . import engine_pb2`）、Rust は build.rs(tonic) で自動再生。`AccountEvent`/`AccountPosition` は Step 0 で既に proto 凍結済みのため追加不要。

#### レビュー修正 (2026-05-21、Step 4 後段) — parallel review (bevy / nautilus / tachibana+kabu) で検出した Medium 以上を修正

3 ドメイン並行レビューで検出した実バグ 5 件を修正（すべて Step 4 の納品コード内の欠陥。verify 済み・全テスト緑）:

- **[High] kabu 訂正警告バナーが実行時に死んでいた（venue id の大小文字不一致）** — `venue_capabilities::for_venue` は小文字 `"kabu"`/`"tachibana"` のみ match していたが、実行時に `ModifyForm.venue` へ届く `VenueStatusRes.venue_id` は backend が**大文字** (`"KABU"`/`"TACHIBANA"`) で報告する（`menu_bar.rs` の gating が `v == "TACHIBANA"` で比較しているのが傍証）。結果 `for_venue("KABU")→None→requires_kabu_ack()=false` となり、kabu の **§2.3 警告バナー・同意チェック・Confirm ゲートが一切出ない**安全バイパスだった（テストは小文字 `"kabu"` を手で組んでいたため素通り）。`for_venue` を `to_ascii_lowercase()` で case-insensitive 化。回帰テスト追加（`for_venue_is_case_insensitive` / `kabu_gate_works_with_backend_uppercase_venue_id`）。
- **[Medium] AccountSync が login 前に起動 → 初回 forced emit が必ず失敗** — `_start_live_components`（→`AccountSync.start()`）は VenueLogin handler 内で `adapter.login()` **より前**に走るため、起動直後の forced `fetch_account()` が未ログイン adapter で例外になり、唯一保証されるはずの初回 emit が握り潰されていた（UI に余力・建玉が最大 30s 出ない）。`_start_live_components_async` では AccountSync を**生成のみ**にし、`_attempt` の login 成功直後（CONNECTED 遷移後）に `start()` する構成へ変更。
- **[Medium] emit リトライの毒化（`_last_emitted` を callback 成功前に更新）** — 配信失敗した snapshot を「emit 済み」と誤記録し、値が変わるまで二度と再送しなかった（特に初回ロードが永久欠落しうる）。`account_sync.py` で `_last_emitted` 更新を `on_account_event` 成功後のみに移動。
- **[Medium] Live 口座データが Replay ビューに滲み出す** — `apply_account_event` は共有 `PortfolioState` に cash/buying_power/positions/equity/`loaded=true` を書き、BuyingPower/Positions パネルは `loaded` だけで mode 無関係に表示するため、Live→Replay 切替（replay 実行前）で Live 余力・建玉が残留した。`ExecutionModeChanged` reducer で **mode が実際に変わったとき** `*portfolio = default()` にリセット（新 mode が改めて埋め直す）。回帰テスト `mode_change_resets_portfolio_to_prevent_live_replay_bleed` 追加。
- **[Medium] OrdersPanel 右クリックがカメラを pan させる** — PanCam が `grab_buttons: [Right, Middle]` のため、コンテキストメニューを開く右クリック自体が pan を誘発し screen-space メニューと world-space パネルがずれる。`pancam_suppression_over_editor_system` に `OrderContextMenu` を渡し、**メニュー表示中は `dragging` を上書きして PanCam を強制無効化**。

検出されたが**意図的に Step 5/6 へ送る** forward-compat 指摘（Step 4 は mock 専用のため現状の挙動欠陥ではない。下記「Step 5/6 への申し送り」に集約）: ① `ModifyOrderReq`/`OrderEvent` が Tachibana `CLMKabuCorrectOrder`/`CLMKabuCancelOrder` の `sEigyouDay`（営業日）を運べない（Step 2 申し送りの cancel 版と同根）、② venue 固有発注パラメータ用の `map<string,string> venue_params` 不在、③ kabu 訂正=取消→新規変換で**新 `client_order_id`/`venue_order_id` が採番される**が facade.modify / Rust `apply_modify` は同一 id を in-place 更新する設計（§1.2「元注文 CANCELED 終端＋別 client_order_id で新規」と矛盾）、④ `second_secret` の二重チャネル（`*Req.second_secret` 3 箇所が inert／正は SecretVault）、⑤ `unrealized_pnl` の venue 非対称（Tachibana `CLMGenbutuKabuList` は取得簿価ベースで live 評価損益を持たない → equity 導出が cost-basis に化ける）。

#### Step 5/6 への申し送り（上記レビューで再確認した forward-compat。proto 追加は additive で非破壊なので Step 5 着手時にまとめて）

- **`sEigyouDay`/order date チャネル**: `OrderEvent` に `optional string order_date`（または `map<string,string> venue_ids`）を additive 追加し、`ModifyOrderReq`/`CancelOrderReq` に Tachibana の営業日を供給できるようにする（kabu は空で良い）。
- **`venue_params` map**: `PlaceOrderReq`/`ModifyOrderReq`/`CancelOrderReq` に `map<string,string> venue_params` を足し handler→facade→adapter `**extra` へ透過（Tachibana `sCondition`/`sOrderExpireDay`、kabu `Exchange`/`AccountType`/`CashMargin`/`FrontOrderType` 等）。
- **kabu の id 再採番**: kabu modify は別 `client_order_id`/`venue_order_id` を生む。`ModifyOrderRes`/`OrderEvent` に `optional string new_client_order_id` を足し、Rust `apply_modify` を「旧注文を CANCELED 終端＋新注文 insert」に拡張できる形にする（§1.2 整合）。Step 4 の in-place モデルは Tachibana atomic 専用と明記。
- **`second_secret` 一本化**: SecretVault（`SecretRequired`→`SubmitSecret`）を正とし、`PlaceOrderReq`/`CancelOrderReq`/`ModifyOrderReq` の `second_secret` は deprecated/reserved として facade 永久 inert を明記（Step 5 で modify だけ誤って復活させない）。
- **`unrealized_pnl` optional 化**: proto `optional double` にし、欠損時は Rust equity 導出を `qty*avg_price` fallback＋「時価評価不可」表示に。Tachibana `fetch_account` は価格 feed と join するか absent 報告するかを Step 5 で確定。`cash` と `buying_power` の取得 API（`CLMZanKaiKanougaku` 等）対応も同時に確認。

#### ⚠️ プロセス上の教訓（並行エージェントの非分離 working dir 競合）

- **事象**: Step 4 を Python / Rust の 2 サブエージェントで**同一 working dir（worktree 分離なし）に並行起動**したところ、両エージェントが「自分の編集が revert された」「proto が消えた」「QUARANTINE stash に退避された」と報告して中断した。**実際には**: ① git hook は標準の Git LFS のみで quarantine/revert 機構は存在しない、② `git stash list` に QUARANTINE stash は存在しない（古い phase8 stash のみ）、③ 最終ツリーは proto + Python + Rust + whole-tree `cargo fmt` ノイズを含めて**正しく再収束し全テスト緑**だった。原因は **2 エージェントが同一ディレクトリで相互の書き込み・`cargo fmt`・git 操作を踏み合った干渉**（parallel-agent-dev 失敗パターン #7「feature ブランチで worktree／非分離並行」）。エージェントの「revert/quarantine」報告は競合中の中間状態の誤読だった。
- **教訓**: feature ブランチで複数エージェントを並行させるときは **ファイル単位の厳密分離だけでは不十分**で、`cargo fmt` の全ツリー実行・git 操作・lib 非コンパイル中間状態が相互干渉する。**proto のような共有 artifact 変更はオーケストレーターが直列で凍結・コミットしてから**サブエージェントを起動するか、各エージェント完了の都度オーケストレーターが直列で統合検証する運用が安全（本 Step は後者で復旧）。

### Step 5 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（Python のみ・**proto / Rust 変更ゼロ**、2026-05-21、未コミット）。Python `-m "not slow"` **1035 passed / 11 skipped / 4 failed**（4 失敗はすべて pre-existing Windows pipe FD baseline = `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1。本 Step 由来の新規失敗 0）。新規テスト約 50（tachibana_orders 35 + secret_provider 5 + tachibana_ws EC 1 + tachibana_adapter 実行系 12 + grpc 結線 4）。

- **アーキ判断（proto / Rust 変更ゼロ）**: Step 2 申し送りの「`OrderEvent` に `order_date`（`sEigyouDay`）追加」「`venue_params` map 追加」は **いずれも不要と判断しドリフト訂正**。理由: ① 取消/訂正に必要な `sOrderNumber`+`sEigyouDay` は **adapter 内部レジストリ**（`client_order_id → _TachibanaOrderRef`）で保持し、facade は client_order_id のみ渡す現契約のまま動く。② venue 固有パラメータ（`sCondition`/`sZyoutoekiKazeiC`/`sSizyouC`/`sGenkinShinyouKubun`）は既存 proto field（`time_in_force`）+ login session + 現物 MVP 定数から導出できる。⇒ **Step 4 を中断させた共有 artifact（proto）競合を構造的に回避**。Rust 側は Step 0-4 の OrderEvent/SecretRequired/SubmitSecret drain で充足、変更不要。
- **`exchanges/tachibana_orders.py` [NEW]**: 純粋関数。`build_new_order_payload`/`build_correct_order_payload`/`build_cancel_order_payload`（`sSecondPassword` 必須・取消/訂正は 2 識別子・side→`sBaibaiKubun`(BUY=3/SELL=1)・TIF→`sCondition`(DAY=0/OPENING=2/CLOSING=4)・MARKET→`sOrderPrice="0"`・`sZyoutoekiKazeiC` は session 流用）。`parse_order_response`（R6 2 段判定: `p_errno` 接続エラーは raise、`sResultCode`!=0 業務リジェクトは `OrderAck(rejected=True)` で REJECTED 注文化）。`parse_ec_frame`（EC→`ExecutionReport`）。
- **`live/secret_provider.py` [NEW]**: `SecondSecretResolver(vault, push_secret_required)`。`resolve(venue,purpose)`= `vault.get` → 無ければ `create_request`→`SecretRequired` push→`wait_for(30s)`。timeout は `SecretTimeoutError("SECRET_TIMEOUT")`。transport 非依存（push callback を server が注入）。連続発注は cache hit で push しない（TTL reuse、リセットなし）。
- **`exchanges/tachibana.py`**: `TachibanaAdapter` を `OrderingVenueAdapter` 化。`submit_order`(CLMKabuNewOrder→ACCEPTED、約定は EC 後追い)/`cancel_order`(CLMKabuCancelOrder)/`modify_order`(CLMKabuCorrectOrder、atomic)/`fetch_account`(CLMZanKaiKanougaku の `sSummaryGenkabuKaituke`=buying_power、CLMGenbutuKabuList の `aGenbutuKabuList`=positions。**cash は買付可能額を proxy**＝現物口座近似)。`set_execution_hooks(secret_resolver, on_order_event)` で結線。**口座レベル EC WS** を login で 1 本起動・logout で停止（FD per-ticker hub とは別。`p_evt_cmd=ST,KP,EC,SS,US`）。再 login は EC + レジストリを stop/clear。EC 重複再送は `_last_ec_report` で dedup。
- **`exchanges/tachibana_ws.py`**: `TachibanaEventWs._recv_loop` に EC/SS/US dispatch を追加（旧実装は EC を 'other' に落として捨てていた）。
- **`server_grpc.py`**: `_start_live_components_async` で `SecondSecretResolver` + `_publish_order_event` を adapter に注入（`hasattr(set_execution_hooks)` で mock/kabu はスキップ）。`_publish_secret_required`/`_publish_order_event` 追加。**注文 write RPC は `_order_timeout_s=40s`**（secret は発注呼び出しの内側で 30s 待つため `_live_timeout_s=5s` だと PLACE_TIMEOUT 誤発火→orphan order になる）。3 ハンドラに `except SecretTimeoutError → error_code` を追加。`PlaceOrderReq.second_secret` は facade で終端し続け、実 secret は SecretVault 経路に一本化（Step 2 申し送りの二重チャネル解消）。
- **secret 経路 E2E**: `test_grpc_tachibana_secret.py` で PlaceOrder→SecretRequired push→SubmitSecret(別 worker thread)→cross-thread に live loop を起こして resolve→ACCEPTED、を実 RPC 往復で検証。SECRET_TIMEOUT 経路も検証。
- **EC 情報コードは e-station 参照実装で確定**（`C:\Users\sasai\Documents\e-station` の `python/engine/exchanges/tachibana_event.py` + architecture.md §6、2026-05-21 ユーザー指示で確認）。当初 `api_event_if.xlsx` 未同梱で TENTATIVE だった推測値を**正値に差し替え済み**: `p_NO`=注文番号 / `p_EDA`=約定枝番(重複検知) / `p_NT`=通知種別(1受付/2約定/3取消/4失効) / `p_DH`=約定単価 / `p_DSU`=約定数量 / `p_ZSU`=残数量(0=全約定) / `p_OD`=約定日時(JST→UTC ms)。status は `ec_status(p_NT, leaves_qty)` で導出（約定は残数量>0 で PARTIALLY_FILLED）。累計約定数量は **発注数量 - 残数量**（adapter の `_TachibanaOrderRef.qty` から導出。EC は side/issue/原数量を持たない）。重複検知は `(venue_order_id, trade_id, notification_type)` の seen-set（e-station C-H3 流儀。再接続の全件再送を弾く）。
- **残る未確定（2 点、要 Demo 検証 = §5.1 layer-3）**: ① **口座レベル EC 購読 URL の構成** — EC は口座スコープだが、e-station は EC を per-ticker FD 接続に相乗りさせる設計（本実装は専用の issue 非依存接続）。issue/行番号パラメータ無しの接続が有効かは未検証。② **`build_event_url` の comma エンコード問題** — 本実装の `build_event_url` は `,`→`%2C` するが、e-station `build_ws_url` は「サーバが `%2C` を認識しない」として **raw comma** を送る（docstring 明記）。これは EC URL だけでなく **Phase 8 の FD 購読 URL（`p_evt_cmd=ST%2CKP%2CFD`）にも影響しうる潜在バグ**。Demo で実フレーム受信を確認する際に両方を検証する。送信側（CLMKabu*）と secret 経路は確定。
- **§5.1 layer-3（実 Demo smoke）は未実施**: credential 未供給・実発注を伴うため。CI/mock の layer-1/2（決定論・人間 0）で網羅。EC 受信の URL 構成のみ Demo 確認が残る。
- **self-review (simplify)**: 3 並行レビューで検出した correctness 2 件（再 login での EC/レジストリ leak、EC 全件再送の重複 push）を修正。REJECTED OrderResult の 3 重コピペを `_rejected_result` に集約、`_to_float`/`parse_float` の重複を `tachibana_orders.parse_float` に統合。per-call httpx client（fetch_instruments と同型）・dict eviction（facade の no-evict 既決と整合）・専用 thread pool（1 注文ずつの手動 UX 前提）は据え置き判断。

### Step 6 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（Python のみ・**proto / Rust 変更ゼロ**、2026-05-21、未コミット）。Python `-m "not slow"` **1094 passed / 11 skipped / 4 failed**（4 失敗はすべて pre-existing Windows pipe FD baseline = `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1。本 Step 由来の新規失敗 0）。新規テスト 51（kabusapi_orders 純粋関数 31 + kabusapi_exec adapter 20）。

- **アーキ判断（proto / Rust 変更ゼロ）**: Step 5（Tachibana）と同じく additive proto 変更は不要と判断。理由: ① kabu の取消/訂正は venue 採番 `OrderId` のみで成立し、`client_order_id → _KabuOrderRef`（venue OrderId + 元注文パラメータ）を **adapter 内部レジストリ**で保持して facade 契約（client_order_id のみ）のまま動く。② 約定通知は GET /orders polling 由来で `OrderEvent`（既存）に詰める。③ kabu は Password 不要（R3）なので SecretVault / SecretRequired 経路を一切使わない。`set_execution_hooks(secret_resolver=..., on_order_event=...)` は Tachibana と同じ呼び出し口を維持し（server_grpc 無改修）、kabu 実装は `secret_resolver` を受理して無視する。
- **`exchanges/kabusapi_orders.py` [NEW]**: 純粋関数。`build_send_order_payload`（現物 CashMargin=1・買=DelivType2/FundType"AA"・売=DelivType0/FundType"  "・MARKET→Price=0・**Password フィールド無し**）/ `build_cancel_order_payload`（`{"OrderID":...}` のみ）/ `front_order_type`（order_type×TIF→FrontOrderType: 成行10/指値20/寄成13/寄指21/引成16/引指24。OPENING=前場寄・CLOSING=後場引の MVP 解釈）/ `parse_send_order_response`（**Result フィールド = 発注エラーの二段目判定**。HTTP/Code は呼び出し側 check_response 担当、§2.2 / R7）/ `parse_order_status`（GET /orders の State+CumQty+Details RecType から Nautilus OrderStatus 導出。約定明細から数量加重平均価格を算出）。
- **`exchanges/kabusapi.py` `KabuStationAdapter`**: `OrderingVenueAdapter` 化。`submit_order`（POST /sendorder→ACCEPTED、Result≠0 は REJECTED、**Result=-1 異常終了は KabuApiError 伝播**、§2.2）/ `cancel_order`（PUT /cancelorder、OrderID のみ）/ `modify_order`（**取消→確定待ち→新規発注**の補償シーケンス。kabu に訂正 API 無し §2.2）/ `fetch_account`（GET /wallet/cash の `StockAccountWallet`=buying_power[cash も proxy]・GET /positions?product=1&addinfo=true の `LeavesQty`/`Price`/`ProfitLoss`→positions）。`set_execution_hooks` で OrderEvent push を結線、**最初の submit で GET /orders 1s polling task を遅延起動**（idle polling 回避）。流量は R5 token-bucket（発注5/余力10/情報10）。
- **訂正（取消→新規変換）の補償を facade 契約で表現**（proto 非変更の制約下、Step 5 方針踏襲）: 取消失敗→`REJECTED`（元注文 live のまま）/ 取消成功+新規失敗→`CANCELED`（同一 client_order_id を CANCELED 終端化＝UI に「原注文取消済・要再発注」を正しく反映）/ 取消確定待ちタイムアウト→`REJECTED`（新規見送り・polling が後追い）/ 全成功→`ACCEPTED`（**同一 client_order_id に新 OrderId を再マップ**＝Tachibana atomic と同じ in-place モデル）。kabu 警告バナー＋同意ゲートは Step 4 `modify_modal` で実装済み（`venue_capabilities` の `supports_order_correction==false`）。
- **self-review (simplify) で検出・修正した correctness 1 件**: 訂正の取消→新規の隙間（asyncio await 点）で **background polling が「取消確定」を spurious な CANCELED として push / unregister する race**。`_modifying` set で当該 client_order_id の polling を一時抑止して解消（回帰テスト `test_poll_suppressed_while_modifying`）。併せて終端注文を push 後に registry から外す最適化（`test_poll_unregisters_terminal_order`、全注文終端化で polling が HTTP を叩かなくなる）を追加。REJECTED 正規化は `_rejected_result` に集約（Tachibana と同型）。
- **残課題 / forward-compat**: ① **AccountType（一般2/特定4/法人12）は MVP 既定 = 特定(4) 定数**。kabu は login 応答に口座種別を載せない（Tachibana の sZyoutoekiKazeiC のような流用元が無い）ため。一般/法人運用が要るときは venue_params 経路で上書き（Step 4/5 handoff「venue 固有発注パラメータ」）。② **§5.1 layer-3（実 verify 環境 smoke）は未実施**（kabuステーション本体 = Windows GUI 必須・CI 不可、R1/S1）。CI/mock の httpx-mock 層（決定論・人間 0）で発注/取消/訂正/口座/polling の不変条件を網羅。③ 逆指値・信用・先物 OP・OCO は Phase 9 非対象。

#### レビュー反映 (2026-05-21, ラウンド 1) — 3 並行レビュー (kabu spec+silent-failure / tachibana+asyncio / type-design+plan) の Medium 以上を全消化

並行レビューで検出した HIGH 1 + MEDIUM 8 を修正（全テスト緑、exchanges+live 671 passed / 2 skipped、新規回帰テスト +15）:

- **[HIGH] 訂正 (取消→新規) が部分約定後に full qty を再発注して over-fill**（`kabusapi.py` modify_order）— 取消は約定に勝てないことがあり（40/100 約定して残り 60 が取消）、原数量 100 をそのまま再発注すると約定済み 40 と二重建玉になり**最大 140 株の実弾発注事故**になっていた。`_await_order_terminal` を `bool`→`OrderStatusReport | None` に変更し、確定時点の `filled_qty` を取得。`merged_qty = (new_qty or ref.qty) - already_filled` で残数量のみ再発注、`merged_qty<=0`（目標数量まで約定済み）は再発注せず終端状態（FILLED/CANCELED）を返す。新原数量 `ref.qty=merged_qty`。回帰: `test_modify_after_partial_fill_resubmits_only_remainder` / `test_modify_when_already_filled_to_target_skips_resubmit`。
- **[MEDIUM] 再発注の `Result=-1`（システムエラー）が握り潰され CANCELED に丸められていた** — `submit_order` は `-1` を `KabuApiError` 伝播するのに modify 内の再発注脚だけ素通りだった（§2.2 不整合）。再発注 `new_ack.reject_code=="-1"` で `KabuApiError` 伝播（原注文は取消済みなので unregister せず polling に CANCELED 後追いさせる）。回帰: `test_modify_resubmit_system_error_propagates`。
- **[MEDIUM] 部分約定→失効の終端が CANCELED に誤分類**（`kabusapi_orders.order_status`）— `cum_qty>0 && State==5` を無条件 CANCELED にし Details を見ていなかった。`_terminal_remainder_status(details)` を新設（残りが失効/期限切れ RecType 3/7→EXPIRED、取消 6 を含むその他→CANCELED、取消優先）。`_confirmed_rectypes` を共通化。回帰: `test_order_status_partial_then_expired_is_expired` / `..._canceled_via_details`。
- **[MEDIUM] `parse_order_status` の欠損/範囲外 State が 0→ACCEPTED に化け masking**（終端検知漏れ→レジストリ leak＋無限 polling）— `state not in {1..5}` は行をスキップ（debug ログ）。回帰: `test_parse_order_status_invalid_state_returns_none` / `..._missing_state_returns_none`。
- **[MEDIUM] orders polling が認証断で 1Hz hot-loop**（`_run_orders_poll`）— 連続失敗を指数バックオフ（初回 1s→2/4/…上限 `_POLL_MAX_BACKOFF_S=30s`）、成功で 1s に復帰。`last_error` 記録は維持。回帰: `test_poll_loop_backs_off_on_repeated_failure`。**注**: `4001007/4001017` の `VenueLogoutDetected` 連動は Step 7 (Watchdog) の責務（本 Step はバックオフのみ）。
- **[MEDIUM] `logout` / `_stop_orders_poll` が task 例外を `BaseException: pass` で全握り潰し** — `CancelledError` のみ pass、それ以外は `logger.warning`（shutdown 時バグの可視化）。
- **[MEDIUM] `build_event_url` の値 allowlist `[A-Za-z0-9,]*` が空/退化値を黙認**（`tachibana_url.py`）— `""`/`","`/`"ST,,KP"` は「何も購読しない」silent failure になり、raw-comma 境界の趣旨に反する。`[A-Za-z0-9]+(?:,[A-Za-z0-9]+)*`（非空トークンのカンマ連結）に厳格化。回帰: `test_build_event_url_rejects_degenerate_evt_cmd` / `..._accepts_valid_token_lists`。
- **[MEDIUM] `server_grpc.py` のコメントが「kabu は set_execution_hooks 無し」と誤記**（実際は実装あり・OrderEvent push が稼働）— 保守者を誤誘導するため訂正（mock のみ hooks 無し）。
- **[MEDIUM/plan] §1.2 と Step 6 の in-place remap の不整合を解消（下記）。** review H2（cancel 応答は fill-blind だが polling が CumQty を補正）は設計どおりで、回帰 `test_cancel_then_poll_reconciles_partial_fill` で「cancel→poll が真の約定量を後追い反映」を固定。
- **LOW（対応不要・記録のみ）**: `int(qty)` 切り捨て（OrderPanel が呼値・売買単位の倍数を検証済み）/ `front_order_type` の OPENING/CLOSING MVP 解釈（DAY 以外の実運用前に OpenAPI 再確認）/ diagnostic の private `_env` reach-in（コンストラクタで `environment="demo"` をハードコードしておりフェイルクローズ）。

**§1.2 / Step 4 申し送りとの整合（plan-fidelity 訂正）**: §1.2 は「kabu 訂正は元注文 CANCELED 終端＋**別** client_order_id で新規」を規定し、Step 4 申し送りは「in-place remap は Tachibana atomic 専用」と書いていた。Step 6 は **proto 非変更の制約下で同一 client_order_id の in-place remap を意図的に採用**（`new_client_order_id` proto field 追加を回避）。当初この in-place モデルは「部分約定分の帰属が壊れる」懸念があったが、上記 HIGH 修正（残数量のみ再発注）により **新 OrderId が運ぶのは未約定の残数量だけ**になり over-fill が解消、約定済み分は polling が元 client_order_id の OrderEvent として別途反映する。よって §1.2 の「別 client_order_id」要件は **Step 6 では in-place remap ＋残数量再発注で代替する**ことをここで明記し、ドリフトを解消する（proto に `new_client_order_id` を足す案は将来 Phase 10 で algo 発注を入れる際に再検討）。

#### レビュー反映 (2026-05-21, ラウンド 2) — fix 由来の silent failure 再走（silent-failure-hunter）

ラウンド 1 fix が新たな握り潰し/不整合を生んでいないか再レビュー。CRITICAL/HIGH 0、検出 MEDIUM 2（いずれも edge / コメント精度、happy-path・回帰テストには非該当）を消化。コア fix（残数量再発注 / `-1` 伝播 / `finally` での `_modifying` 解除 / backoff の cap・reset / except 限定 / regex 厳格化）は clean と確認:

- **[MEDIUM] `parse_order_status` の None ガードのコメントが過剰主張** — 「レジストリ leak + 無限 polling を防ぐ」と書いたが、None を返しても *恒久的に* 解釈不能な行は unregister されず leak は残る（ただし kabu State は OpenAPI 上 1-5 のみなので実発生しない）。コメントを実態に合わせて訂正（誤 ACCEPTED 回避が目的・一時異常は次回 poll で解消、と明記）。挙動変更なし。
- **[MEDIUM] `_run_orders_poll` の自己終了判定が sleep の後** — backoff が伸びた状態（最大 30s）で全注文終端になると、空判定まで最大 1 backoff 分 task が居残る。`if not self._orders_ref: return` を**ループ先頭（sleep 前）へ移動**して即終了化（`_poll_orders_once` 自体も空ガードを持つので余計な HTTP は出ない）。回帰テスト（`test_poll_loop_self_terminates_when_no_orders` / `test_poll_loop_backs_off_on_repeated_failure`）は緑のまま。
- **収束**: exchanges+live **671 passed / 2 skipped**、全体 `-m "not slow"` **1111 passed / 11 skipped / 4 failed**（4 失敗は既知 baseline = Windows pipe FD `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1、本作業と無関係）。MEDIUM 以上ゼロで収束。

#### simplify パス (2026-05-21) — reuse / quality / efficiency 3 並行レビューの polish

MEDIUM 以上収束後の品質パス。検出のうち効果のあるもののみ反映（exchanges+live+secret **675 passed / 2 skipped**）:

- **[efficiency] `_poll_orders_once` が口座の全注文を毎 tick parse** → 安価な `ID` 照合を先に行い未追跡注文を `parse_order_status`（Details 走査・約定平均・時刻変換）の前に弾く。per-tick コストを O(口座注文数)→O(追跡注文数) に。
- **[quality] leaky: adapter が private `_orders._DEFAULT_ACCOUNT_TYPE` を跨いで参照** → public `DEFAULT_ACCOUNT_TYPE` に改名。
- **[efficiency] `_modifying` が login/logout で未クリア**（他レジストリは clear 済み）→ 対称性・防御のため両所に `clear()` 追加。
- **[quality] `server_grpc.py` の hooks コメントが venue roster を列挙して腐りやすい** → load-bearing な WHY（kabu は secret 不要で resolver を無視・mock は hooks 無し）のみ残して整理。
- **据え置き判断**: ① 6 箇所の GET/POST/PUT boilerplate を `_api` ヘルパーに集約する案は、quality reviewer が「verb/bucket/payload が異なり parameter sprawl・GET/PUT 意図が埋もれる」と判断（reuse reviewer と意見相違）、収束後の 6 メソッド改修リスクに見合わないため見送り。② `parse_float` / JST 時刻変換の kabu↔Tachibana 重複は sentinel 仕様差による意図的並行で非共有。③ `fetch_account` は既に `asyncio.gather`（wallet/info 別 bucket）で並行取得済み・`_last_pushed` dedup キー `(status, filled_qty)` は約定が動けば filled_qty も動くため avg_price 無視で正。

#### レビュー反映 (2026-05-21, ラウンド 3) — Step 6 コミット後の 3 並行レビュー (silent-failure / asyncio-concurrency / kabu-spec+type+plan-fidelity)

R1/R2/simplify 収束後にコミットした Step 6 (`558eabb1`) を改めて 3 観点並行レビュー。kabu-spec+type+plan-fidelity は OpenAPI yaml と公式サンプルに対し発注 payload を field 単位で照合し **指摘ゼロ**（Side 文字列・CashMargin/DelivType/FundType 買売非対称・market Price=0・cancel `OrderID` casing・State/RecType 意味・remainder-only 再発注すべて正）。silent-failure / concurrency が検出した **HIGH 1 + MEDIUM 1** を修正（全テスト緑、exchanges+live **677 passed / 2 skipped**、新規回帰テスト +6）:

- **[HIGH] 訂正中に約定した数量が OrderEvent stream から消える（filled_qty 過少報告）** — kabu 訂正の in-place remap は旧 venue OrderId を `_order_id_to_cid` から外す（= polling が旧 leg を二度と読まない）。一方で R1 の over-fill 修正により再発注は残数量のみ（merged_qty）になり、新 leg O2 の `CumQty` は 0 から始まる。結果、原数量 100 で 40 約定 (O1) → 残 60 を O2 で再発注 → O2 が 60 約定、という論理注文の累計約定は本来 100 だが、stream は O2 の 60 しか報告できず**約定済み 40 が永久に欠落**する（売り訂正では「売れた量」を過少表示する実弾事故）。R1 over-fill 修正の裏面で、約定済み分の帰属が壊れていた。
  - **修正**: `_KabuOrderRef` に `filled_base` / `notional_base`（訂正で捨てた旧 leg の累計約定数量・約定代金）を追加。`modify_order` は `total_target = new_qty or (filled_base + qty)` / `total_filled = filled_base + already_filled` / `merged_qty = total_target − total_filled` で残数量を算出し、remap 成功時に `ref.filled_base = total_filled`（telescoping）。`_poll_orders_once` は `filled_base + 現 leg CumQty` を論理累計として push（加重平均価格つき）、`filled_base>0` かつ現 leg ACCEPTED のときは論理 `PARTIALLY_FILLED` に持ち上げる。merged_qty<=0 / 新規業務リジェクト CANCELED / ACCEPTED の全 return path で `filled_qty=total_filled` を載せる（旧 leg を捨てるため polling 後追い不可）。**論理総目標 = `filled_base + qty` 不変条件**を N 回の remap で保つ。回帰: `test_modify_carries_filled_baseline_so_remainder_poll_reports_cumulative` / `test_poll_after_modify_reports_partial_when_baseline_but_remainder_unfilled` / `test_modify_new_failed_after_partial_reports_filled` / `test_double_modify_telescopes_filled_baseline`（連続訂正で baseline 累積・総目標 100 維持・加重平均検算）。
- **[MEDIUM] `cancel_order` / `modify_order` が訂正進行中の同一注文に対する re-entrancy 無防備** — 取消→新規の suppression window (`_modifying`) 中に、同一 client_order_id へ並行 cancel または二本目の modify が走ると、後者の `finally: _modifying.discard` が先行 modify の suppression を先に畳み、polling が先行 leg の中間状態（取消確定）を spurious な CANCELED として push / unregister し、remap した live 注文を孤児化させうる。現状の単一 live-loop / 1 注文ずつの手動 UX 下では latent だが、対称ガードで閉じる。
  - **修正**: `cancel_order` と `modify_order` の双方の冒頭で `if order_id in self._modifying: return REJECTED(reject_reason="MODIFY_IN_PROGRESS")`（mutation 前に early-return、suppression は先行 modify が保持し続ける）。回帰: `test_cancel_order_rejected_while_modifying` / `test_modify_order_rejected_while_modifying`。
- **LOW（対応不要・記録のみ）**: ① remap 直後の初回 poll が同一 total を再 push しうる（idempotent）が、実際は modify-ACCEPTED 応答の status を polling が `PARTIALLY_FILLED` に補正する**意味のある重複**なので pop のまま維持。② `_run_orders_poll` の `_last_error` は Watchdog (Step 7) 連動まで health に出ないが log 済み（仕様）。③ `_parse_transact_time_ms` は実 ISO `TransactTime` でも `digits[:14]` で正しく JST→UTC ms 化する（compact 形式前提の docstring は実装上は ISO も頑健）。④ 注文 write path の `resp.json()` は非 JSON body で `JSONDecodeError` を上層 (gRPC error_code) に伝播するが silent ではない。

**収束**: CRITICAL/HIGH/MEDIUM **ゼロ**。silent-failure / concurrency / kabu-spec の 3 観点で R3（実装由来 HIGH 1）→ R4（fix 由来 MEDIUM 1）→ 収束のカーブ。proto / Rust 変更ゼロは維持（baseline は adapter 内に閉じる）。

---

## Goals

- Live venue (Tachibana / kabuステーション) の **口座残高・保有ポジション・約定履歴** を同期し、既存 `TradingState` reducer 経由で UI に反映する
- LiveManual モードで **手動発注**（新規・取消・訂正）が可能になる。`OrderPanel` (Phase 8 で繰り越し) を新設し、`PlaceOrder` / `CancelOrder` / `ModifyOrder` の 3 RPC を追加
- **第二暗証番号 (Tachibana のみ)** を新規・訂正・**取消**のすべてで収集・メモリ保持・idle forget するワンタイム収集 UX を導入（kabu は sendorder / cancelorder とも Password 不要）
- Phase 8 で繰り越した運用系項目（kabu 本体早朝ログアウト後の自動回復、Instruments 日次更新、idle gRPC shutdown、backend 自動再起動）を片付け、Phase 10 (Promote to Live) の土台を整える

## Non-Goals

- **アルゴリズム発注 / 戦略からの自動発注は Phase 10**。Phase 9 はあくまで **人間がボタンを押す** 経路のみ。`StartLiveStrategy` RPC は導入しない
- 複数 Venue への同時発注、IFD/OCO 等の特殊注文タイプは Phase 9 のスコープ外
- 信用取引・先物・オプション・夜間 PTS は対象外（現物のみ）
- 税制計算・確定申告レポートは対象外

### Step 7 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（Python + Rust、proto 変更ゼロ、2026-05-21、未コミット）。Python `-m "not slow"` **1138 passed / 11 skipped / 4 failed**（4 失敗はすべて pre-existing Windows pipe FD baseline = `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1。本 Step 由来の新規失敗 0）。Rust `cargo build --lib`/`--bins --tests` 緑、新規 lib unit 6（relogin_modal 4 + backend_sync drain 1 + ※kabu/tachibana/watchdog の Python 側は下記）。**Rust 既存 2 失敗（`backend_supervisor::classify_child_exit_*`）は Windows で Unix `true`/`false` が無いための pre-existing 環境差**で本 Step 無関係。Python 新規テスト 約 21（health_watchdog 6 + kabu check_health 6 + tachibana SS 7 + grpc 配線 2）。

- **アーキ判断（proto / 既存 transport 変更ゼロ）**: `VenueLogoutDetected` は Step 0 で proto 凍結済み・Rust drain も配線済み（ログのみ）。本 Step は ① Python に検知ロジック ② Rust drain をログのみ→ReloginModal 結線、の 2 点のみ。proto も既存 `BackendEvent`/`set_execution_hooks` 契約も非破壊。
- **kabu（poll 型）**: `KabuStationAdapter.check_health()` [NEW] = `GET /apisoftlimit`（info 系・最軽量・副作用なし。`HEAD` は `4001014` で不可、新規 `/token` は本体負荷のため不使用）。`4001007`/`4001017`（本体ログアウト/未ログイン）→ `False`、それ以外のエラー（流量 429・接続断）は **transient として伝播**（watchdog が握る。誤って False を返すと spurious な再ログイン modal が出るため必ず raise）。未ログイン（teardown race）は `False` でなく `RuntimeError`。`_VENUE_LOGGED_OUT_CODES` 定数化。
- **`live/health_watchdog.py` [NEW]**: `VenueHealthWatchdog(adapter, venue_id, on_venue_logout, interval_s=30)`。transport/venue 非依存（account_sync と同思想、venue 固有エラー型を知らず bool 契約のみ）。**初回 forced tick しない**（login 直後は healthy なはず、変化検知が目的）。ログアウト検出は **debounce**（復旧を観測するまで 1 回だけ通知）、`check_health` の例外は warning + `last_error` でループ継続。`CancelledError` のみ終了。
- **Tachibana（push 型）**: poll watchdog は使わず、EVENT WS の **SS=システムステータス**フレームで検知（`set_execution_hooks(on_venue_logout=...)` 追加）。`_handle_system_status` = `CLMSystemStatus` の `sSystemStatus`(0:閉局/1:開局/2:一時停止) と `sLoginKyokaKubun`(0:不許可/1:許可/2:時間外/9:管理者) を読み、閉局 or ログイン不許可を「要再ログイン」とみなす。open→closed 遷移時のみ通知（SS は接続毎初回再送のため debounce）。login/logout で `_last_system_open` をリセット。⚠️ **TENTATIVE（要 Demo 検証 = §5.1 layer-3）**: EVENT フレームでの `sSystemStatus`/`sLoginKyokaKubun` の **prefix**（`s*` か `p_*` 変種か）は実 Demo 未確認（EC URL / comma エンコードと同じ Demo-pending）。判別フィールド欠落時は安全側（通知しない）。フィールド名・値域は `mfds_json_api_ref_text.html` の CLMSystemStatus で確定。
- **`server_grpc.py` 配線**: `_publish_venue_logout(venue)` [NEW]（VenueLogoutDetected を BackendEventBus に push）を kabu/Tachibana 両方の検知 callback として注入。watchdog は `hasattr(adapter,"check_health")` で gate（kabu のみ・mock/Tachibana は None）。`set_execution_hooks(on_venue_logout=self._publish_venue_logout)` を両 adapter に渡す（kabu は受理して無視＝secret_resolver と同じ accept-and-ignore）。watchdog は account_sync と同じく **login 前に生成・login 成功後に start**（`_start_health_watchdog_after_login`）、teardown で最初に stop。
- **Rust UI（`src/ui/relogin_modal.rs` [NEW] + `ReloginPrompt` resource）**: `backend_event_drain_system`（backend_sync.rs）の `VenueLogoutDetected` arm を**ログのみ→`relogin_prompt.active = Some(venue)`** に結線。ReloginModal は UI Node オーバーレイ（GlobalZIndex 260、secret_modal 流儀）で venue 名 + 再ログイン案内 + [閉じる]/Esc を表示。
  - **設計判断（drift note, §3.5）**: 計画書は「再ログイン modal → ログイン完了で購読再開」と書くが、本モーダルは **通知に徹し自身は `VenueLogin` を発射しない**。理由: ① 検知時点で backend `venue_sm` はまだ `CONNECTED`（検知は push で状態遷移ではない）→ 直接 `VenueLogin` は busy slot に衝突。② 環境（demo/verify/prod）選択は Venue メニューが所有 → モーダルから推測発射すると**誤った環境への再接続**になりうる。よって実再ログインは既存 Venue メニュー（Disconnect→Connect）を通し、購読再開もその既存ログインフローが担う。
- **残課題（Step 8/Demo へ）**: ① Tachibana SS frame の prefix を Demo で確認し TENTATIVE を確定。② kabu polling の `4001007/4001017` バックオフ（Step 6 で実装済み）と watchdog の VenueLogoutDetected を連動させる余地（現状は独立。watchdog 検知で UI 誘導、polling はバックオフ継続）。③ 再ログイン完了後の購読自動再開の E2E（既存ログインフロー依存・Step 8 の auto-restart re-sync と合わせて検証）。

### Step 8 (一部) 完了サマリー (2026-05-21) — §3.7 Idle gRPC Shutdown のみ

> **状態**: 🔶 **§3.7 完了 / §3.8 未着手**。Python `tests/live` + grpc 関連 **215 passed**、`cargo build --lib` 緑。idle_shutdown 新規テスト 7。

- **§3.7 Idle gRPC Shutdown（✅ 完了）**: `live/idle_shutdown.py` [NEW] = `LastRequestClock`(threading.Lock + monotonic) + `RequestActivityInterceptor`(grpc.ServerInterceptor、全 RPC で `touch()`) + `IdleShutdownMonitor`(daemon thread、`check_interval_s` 毎に idle 確認、`idle_timeout_s`=60 超過で `on_idle` を 1 回呼んで終了) + `should_enable_idle_shutdown(environ)`(純粋 gate)。`serve()` に配線: interceptor を `grpc.server(interceptors=[...])` に登録、standalone 時のみ monitor を start し `on_idle = process_lifecycle.start_shutdown(grace=2)`（既存 shutdown 経路が teardown→kabu unregister/all→`server.stop` を担う）。**Bevy supervisor は spawn 時に `BACKEND_SUPERVISED=1` を渡す**（`backend_supervisor.rs::spawn_python_backend` に `.env("BACKEND_SUPERVISED","1")`）→ 配下では monitor を起動しない。
  - **ドリフト訂正（§3.7）**: 計画書の「asyncio.Lock / background asyncio task」は本番が同期 `grpc.server(ThreadPoolExecutor)` のため **threading ベース**に変更（SecretVault と同根。単一 asyncio loop は存在しない）。
- **§3.8 Auto-Restart + in-flight re-sync（✅ 完了、別セッション 2026-05-21）**: 下記 Step 8 §3.8 完了サマリー参照。

### Step 8 §3.8 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（Rust + proto + Python、2026-05-21、未コミット）。Rust `cargo build` 緑、`cargo test` **lib 488 / bins 28 / integration 10 passed, 0 failed**（新規: CrashBudget 4 + reconcile pure 4 + reconcile_modal 4 + ほか）。Python `-m "not slow"` **1167 passed / 11 skipped / 4 failed**（4 は pre-existing Windows pipe FD baseline。新規 GetOrders handler 5）。**※環境注記**: 本セッションの Windows では Git Bash の `true`/`false` が PATH 上にあり、Step 7 サマリーが pre-existing 失敗とした `backend_supervisor::classify_child_exit_*` は **緑**だった。

- **A. Backend Auto-Restart（`src/backend_supervisor.rs`）**: 上位 `run_supervisor` を **`'session: loop`** に再構成。各イテレーションが 1 回の probe→(attach|spawn)→monitor。
  - **`CrashBudget`**（純・time 注入で unit テスト可）: 60s sliding window / 3-strike。`record_and_allows_restart(now)` は古い crash を prune→push→`len() < max`（3 回目で exhausted=false）。`reset()` は手動 Restart / 健全復帰でクリア。定数 `AUTO_RESTART_WINDOW=60s` / `AUTO_RESTART_MAX=3`（ADR 準拠、テストで固定）。
  - **`run_post_ready_monitor` を `-> MonitorOutcome`** 化（`Crashed`/`Stopped`/`RestartRequested`/`ShutdownComplete`/`ChannelClosed`）。**spawn パスのみ auto-restart**（own_process）: Crashed→budget 内なら再 spawn / 尽きたら `wait_for_command_after_terminal` で手動 `Restart`（=budget reset 後 再 spawn）待ち。attach パスの crash は再 spawn せず手動待ち（外部管理 backend の巻き添え kill 回避）。`SpawnOutcome`（spawn ブロックを `spawn_and_handshake` に抽出）。
  - **`Restart` を no-op stub から機能化**: monitor 内で受けたら **`handle_shutdown` で現行 child を先に reap してから** `RestartRequested` を返す（Child handle を drop しても OS プロセスは死なない → 二重 backend 事故を防止）。
  - 既存 2 テスト（autospawn-zero / venv-not-found）は新 session loop でも terminal に収束（dropped cmd sender→`recv()`=None→Terminal）。hot-spin 無し（loop 先頭が常に await 付き TCP probe）。
- **B. `GetOrders` RPC（proto + Python）**: `engine.proto` に `rpc GetOrders(GetOrdersReq{token,venue}) returns (GetOrdersRes{success,error_code,repeated OrderEvent orders})` 追加（オーケストレーターが直列で凍結・Python pb2 regen + 相対 import 再パッチ・Rust tonic 自動再生・`tests/backend_integration.rs` mock stub 追従）。`ManualOrderFacade.list_orders()`（**非終端**注文の snapshot）+ `GetOrders` handler（GetOrderStatus 雛形・read 系で mode reject しない・facade None→`NO_LIVE_SESSION`）。**再起動直後の fresh backend は facade 空 → orders=[]** ＝ UI の楽観的注文を全件「状態不明」に炙り出す reconcile primitive。
- **C. In-flight re-sync（Rust）**: `TransportCommand::GetOrders` + `BackendStatusUpdate::OrdersReconciled{backend_client_order_ids}` + `ReconcilePrompt`/`ReconcileUnknownOrder` resource + 純関数 `reconcile_unknown_orders(live, backend_ids)`（UI 稼働中注文のうち backend 未追跡のものを抽出）+ `is_terminal_order_status`。`backend_restart_resync_system`（`Local` で「Crashed の後に Ready へ遷移」を 1 回だけ検知。初回起動 Ready では発火しない）が稼働中注文ありのとき `GetOrders` 発射 → dispatch が `client.get_orders`（失敗/空は ids=[]）→ `apply_status_update` の `OrdersReconciled` arm が diff して `ReconcilePrompt.unknown` を埋める → `src/ui/reconcile_modal.rs`（relogin_modal 流儀の通知専用 UI Node モーダル・GlobalZIndex 262・[確認した]/Esc で dismiss）が一覧表示。
  - **設計判断（通知に徹する）**: 再起動直後 backend は venue 未ログインなので、モーダルから自動取消/再送はしない（二重発注リスク, ADR §3.8）。ユーザーは Venue メニューで再ログインし venue 側で実状態を確認する（relogin_modal と同方針）。
- **D. ドリフト訂正（「[Restart Backend] disabled UI」）**: 計画書 §3.8 は budget 超過時に「既存の [Restart Backend] disabled UI に格下げ」と書くが、**その Restart ボタン UI は未実装だった**（`SupervisorCommand::Restart` は UI から一度も送られていない）。本 Step は budget 超過時に lifecycle を `Crashed` のまま留め（footer が表示）supervisor は手動 `Restart` を待ち受ける形にし、`Restart` コマンド自体を機能化した（将来ボタンを足せば即動く）。専用トースト「Backend を再起動しました」は lifecycle 遷移 Crashed→Spawning→Ready が footer に出るため見送り（forward-compat）。
- **self-review (simplify)**: 3 並行レビュー（reuse / quality / correctness）で **correctness バグ 0**（auto-restart 7 シナリオ全 trace 済み）。検出は LOW/MEDIUM の house-style 指摘のみ（reconcile_modal↔relogin_modal の並行＝意図的・既存 secret_modal と同型、terminal-status の Rust↔Python 重複＝wire 定数で正当、`apply_status_update` 13 引数＝Step 4/6 から続く `#[allow(too_many_arguments)]` baseline）→ いずれも修正せず据え置き（bundle struct / match arm 抽出は確立パターンを崩す churn で見送り）。
- **残課題 / forward-compat**: ① **venue-truth GetOrders**（kabu `GET /orders` / Tachibana `CLMOrderList` で venue 実注文を引く richer reconcile）は未実装。現状は facade in-memory store（再起動で空）を返す MVP で「再起動で状態不明」AC は満たすが、再ログイン後の自動 reconcile は将来。② [Restart Backend] ボタン UI 本体 + auto-restart トースト。③ §5.1 layer-3（実 taskkill /F → 自動再起動 + reconcile モーダルの E2E）は未実施（CrashBudget 純テスト + supervisor シナリオ trace で網羅）。

### Step 10 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（2026-05-21、未コミット）。Python 新規テスト 4（secret masking 再検証）。`mask_secrets` の漏れ修正に伴う回帰なし（test_logging 緑）。

- **secret masking 再検証 — ⚠️ 実バグ発見・修正**: `mask_secrets`（`live/logging.py`）の secret-key regex は `second[_-]?password`（"password" token 必須）だったため、**Phase 9 の proto wire field `second_secret`（PlaceOrder/Cancel/ModifyOrderReq）が伏字にならず素通りしていた**。`PlaceOrderReq` 等の dict をログに出すと平文が漏れる経路（実際は facade 終端 + 未ログインで低リスクだが §6 AC 違反）。regex に `|secret`（"secret" を含む任意 key を伏字）を追加してフェイルセーフ化。回帰テスト `test_secret_masking_phase9.py` 4 件（`second_secret`/`sSecondPassword` 伏字 / SecretVault repr に平文なし / pickle スナップショットに平文なし＝Lock/Future で直列化不可 / TTL `_expire` 後に `_store` から平文消失）。Rust `RedactedSecret` の Debug 伏字は trading.rs unit test で既出。
- **drawio アーキ図**: `docs/assets/phase9-architecture.drawio.svg`（phase7 と同じ host="app.diagrams.net" SVG 流儀）。Rust(Bevy GUI: UI panels/modals + backend_sync + supervisor + transport task) ↔ gRPC(:19876, GetOrders ★) ↔ Python(server_grpc + ManualOrderFacade + SecretVault + background components + BackendEventBus/IdleShutdown + venue adapters) ↔ 立花/kabu API の全体像。★=Phase 9 新規、青=command、緑=event/push を凡例化。
- **Phase 10 引き継ぎ doc**: `docs/plan/phase9-to-phase10-handoff.md`。§8 引き継ぎ表 + 各 Step の forward-compat（ExecEngine wiring 移行順序 / secret 単一チャネル維持 / kabu in-place remap / unrealized_pnl 非対称 / AccountType / venue-truth GetOrders / [Restart Backend] ボタン / J-Quants trading_calendar / §5.1 layer-3 Demo 検証 / セキュリティ不変条件）を集約。

### Step 9 完了サマリー (2026-05-21)

> **状態**: ✅ **完了**（Python のみ・**proto / Rust 変更ゼロ**、2026-05-21、未コミット）。Python `-m "not slow"` **1162 passed / 11 skipped / 4 failed**（4 失敗はすべて pre-existing Windows pipe FD baseline = `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1。本 Step 由来の新規失敗 0、Step 8 比 +24 passed）。新規テスト 16（instruments_store 6 + instruments_scheduler 10）。

- **⚠️ 着手時に判明した計画書ドリフト（2 件）**: §3.6 が前提とする ① **J-Quants `/markets/trading_calendar` HTTP クライアント**（`JQuantsLoader` はローカルファイル読み込み専用で API クライアントではない・credentials フロー無し）と ② **「Phase 8 がログイン時に全置換する instruments parquet store」**（`_list_instruments_live` は都度 `fetch_instruments_blocking` で UI に返すだけで parquet を**一切永続化していなかった**）が **どちらも未実装**だった。ユーザー判断で: ① **営業日カレンダーは作らず venue の `fetch_instruments` エラー/空に委ねる**（非営業日/閉局は venue がエラー → スキップして翌 5:00 に再試行）、② **parquet store を本 Step で新設しログイン時 persist も含めフル結線**、に確定。
- **`live/instruments_store.py` [NEW]**: `instruments_path(venue)`（`INSTRUMENTS_CACHE_DIR` env override → 既定 `LOCALAPPDATA|~/.cache / the-trader-was-replaced/instruments/<venue>.parquet`、`tachibana_file_store` と同型。ファイル名は venue 小文字化で大小ブレ吸収）+ `write_instruments`（pyarrow Table → tmp → `os.replace` の **atomic**、`server_grpc._write_artifact_atomic` 流儀。空リストも空 parquet 可）+ `read_instruments`（parquet→`list[InstrumentRaw]`、`FileNotFoundError` は store miss として None）。pyarrow は server_grpc が既に使う依存（追加なし）。schema はモジュール singleton（lazy build）。
- **`live/instruments_scheduler.py` [NEW]**: `InstrumentsScheduler(adapter, venue_id, *, persist=write_instruments, next_delay_s=…, now_fn=…)`。account_sync と同思想（transport / store 非依存・`persist`/`next_delay_s` 注入で 5:00 JST 実待ちなしに検証）。**起動直後に 1 回 fetch+persist（= ログイン時 persist / 初期ロード）**、以降は `seconds_until_next_5am_jst(now)`（純関数・別途 unit test）で次の 5:00 JST まで sleep して再 fetch+persist。fetch/persist 例外は warning + `last_error` 記録して **前回 parquet を保持しループ継続**（best-effort）、空リストは「adapter 非対応（kabu MVP）」で persist しない。`CancelledError` のみ正常終了。
- **`server_grpc.py` 配線**: `_start_live_components_async` で scheduler を生成（account_sync/watchdog と同じく login 前に生成・**login 成功後に `_start_instruments_scheduler_after_login`** で start ＝初期 persist が logged-in adapter を叩く）、teardown で最初に stop（parquet のみ触るので安価）、finally で None。`_list_instruments_live` を **store-first** に変更（persist 済み parquet があればそれを返し venue を叩かない・miss/read 例外時のみ live fetch して best-effort persist）。`LiveRunner.venue_id` プロパティ追加（store キー、reach-in を 1 箇所に集約）。
- **self-review (simplify)**: 3 並行レビューで検出した worthwhile 3 件を反映（① venue_id の getattr reach-in 2 箇所を `LiveRunner.venue_id` プロパティ使用に統一、② pyarrow schema を per-write 再構築 → lazy singleton 化、③ `read_instruments` の `path.exists()` TOCTOU を `try/except FileNotFoundError` に）。**据え置き判断**: cache-dir helper / atomic-write helper / scheduler 基底クラス / time_helpers の共通化は **shared artifact（tachibana_file_store / run_buffer 等）を巻き込む out-of-scope な churn**で、account_sync との lifecycle mirror も計画書記載の意図的並行のため見送り（並行エージェント churn 教訓と整合）。
- **残課題 / forward-compat**: ① **J-Quants `/markets/trading_calendar` 連携**は未実装（`next_delay_s` / business-day gate の差し替えで将来足せる形にしてある）。現状の「venue エラーに委ねる」は非営業日に venue がエラーを返す前提に依存する。② instruments store を読む consumer は現状 `_list_instruments_live` のみ。③ §5.1 layer-3（実 Demo で 5:00 JST 実走）は未実施（時刻 mock の決定論テストで網羅）。

### Phase 9 全体レビュー反映 (2026-05-21, 横断ラウンド) — 未コミット review-fix delta の再レビュー

> Phase 9 (Step 0–10) コミット後に working-tree に積まれていた未コミットの review-fix delta（Finding 1/3・MEDIUM-1..5・items 5–9）を、5 観点並行レビュー（kabu+tachibana spec / Python gRPC+silent-failure / Bevy ECS / Rust core+supervisor / 各 venue skill）で再検証し、検出した **HIGH 2 + MEDIUM 8 + clippy correctness 1 + LOW 1** を全消化。検証緑（Rust lib 500 / bin 30 / integration 10、Python exchanges+live+grpc 848 passed / 2 skipped、`cargo clippy` correctness error 0）。

- **[HIGH] Escape クロストークの修正がフレーム非決定的だった** (`src/ui/mod.rs`) — SecretModal は Escape を `Events<KeyboardInput>` drain で消費、4 つの通知/確認 reader は `ButtonInput::just_pressed` で読むため、両者に順序制約が無く「scheduler 順次第で 1 回の Escape が両モーダルを閉じる」窓があった（テストは偶然の順序で通っていた）。reader が「上位モーダルがフラグをクリアする前」にガードを評価できるよう、`confirm_modal_button_system.before(secret_modal_input_system)` と通知 3 reader の `.before(secret_modal_input_system).before(confirm_modal_button_system)` を付与して決定化（優先度 lattice: secret > confirm > 通知/コンテキスト、cycle 無し）。
- **[HIGH] 失敗ハンドシェイク child の reap が async 上で blocking `child.wait()` だった** (`src/backend_supervisor.rs`) — `spawn_and_handshake` は `async fn` なので素の `Child::wait()` が Tokio worker を stall させる。`handle_shutdown` の前例どおり `spawn_blocking` + `wait_timeout(500ms)` に変更。
- **[MEDIUM] `mask_secrets` に再帰/循環ガードが無く、再帰が `try` の外** (`logging.py`) — cyclic payload で `RecursionError`（= ログ時クラッシュ）。`_mask(payload, depth)` + `_MAX_MASK_DEPTH=25`（超過で `"<max-depth>"` sentinel）に再構成。
- **[MEDIUM] `mask_secrets` の `model_dump` duck-type が広すぎ** (`logging.py`) — `isinstance(BaseModel)` に厳格化（副作用ある任意 `model_dump` を呼ばない）。
- **[MEDIUM] 再接続 flush が order/secret コマンドまで保持していた** (`src/main.rs`) — preserve 対象を **reconcile primitive の `GetOrders` のみ**に絞り、stale な PlaceOrder/Cancel/Modify と旧 session の `SubmitSecret`（平文 secret の再送）を drop（§3.8 ADR / §3.10 secret hygiene）。
- **[MEDIUM] `apply_modify` の fill が単調でない** (`src/trading.rs`) — `apply_event` と同じ `filled_qty >= existing.filled_qty` ガードを追加（modify ACK の filled=0 が EC partial を潰すのを防止）。
- **[MEDIUM] Tachibana EC seen-set を隔離 callback の *前* に mark していた** (`tachibana.py`) — callback が raise すると実弾 fill が seen 済みになり再接続再送でも永久 dedup → fill 消失。mark を **成功後**に移動（失敗時は mark せず再送に委ねる、downstream 冪等）。
- **[MEDIUM] SS `sSystemStatus=="2"`（一時停止）の logout 判定ドリフト** — 実装は「真の閉局 `"0"` / 真の不許可 `"0"` のみ logout」で正しいが `event_protocol.md:87` の不変条件と乖離していた → doc を実挙動に更新＋`test_ss_suspended_status2_does_not_fire` で固定。
- **[MEDIUM] `SecretPrompt.error` のクリア漏れ余地** (`src/trading.rs`/`secret_modal.rs`) — `SecretPrompt::close()`（active+error を同時 null）を新設し閉じる経路を集約。
- **[MEDIUM] secret 失敗エラー行が 320px カードを溢れうる** (`secret_modal.rs`) — info ノードに `width: 100%` を付け wrap を保証。
- **[clippy correctness / pre-existing] `wait_for_command_after_terminal` の `while let` が `never_loop`** (`src/backend_supervisor.rs`) — 両 arm が return する degenerate loop。`if let` に変更（挙動同一・`cargo clippy` の deny-by-default correctness error を解消）。
- **[LOW] kabu modify の merged_qty<=0 で CANCELED 丸めが広すぎ** (`kabusapi.py`) — `!= "FILLED"` → `== "REJECTED"` に絞り、明細由来の正当な EXPIRED を保つ。
- **[doc / MEDIUM-C] idle-shutdown「UI 接続中は発火しない」セマンティクスを §3.7 に明文化**（挙動はテスト固定済み・バグではない）。

**ラウンド 2（fix 由来の silent-failure 再走、2 観点並行）**: Python silent-failure と Rust+Bevy correctness を再投入。**新規 CRITICAL/HIGH/MEDIUM 0**。Rust reviewer は H1 の `.before` 方向の正しさ（interleaving trace）と ordering グラフの非循環を独立確認。検出は Python の理論上 MEDIUM 1（pydantic import 失敗時の fallthrough = hard 依存ゆえ到達不能・pre-existing）→ fail-safe な duck-type fallback で塞ぎ、Rust の LOW 2 件を消化:
- **[L1] flush の drop 範囲をコメントで網羅**（`GetOrders` のみ保持＝VenueLogin/SubscribeMarketData/… 全 variant を drop する旨を明記＋回帰テストに `VenueLogout` drop を追加）。
- **[L2] Escape ordering の schedule レベル回帰テストを新設**（`reconcile_modal.rs::schedule_orders_reconcile_before_secret_drain_so_one_escape_closes_only_secret`）。per-modal テストは誤順序でも通るため、両 system を本番 `.before` で組み・実 Escape（event drain + ButtonInput）を 1 回流して「上位モーダルのみ閉じる」を固定。`.before`→`.after` flip / cycle を CI で検出可能に。

**収束**: CRITICAL/HIGH/MEDIUM **ゼロ**。検証緑（Rust lib 501 / bin 30 / integration 10、`cargo clippy` correctness error 0、Python 全体 `-m "not slow"` 1238 passed / 11 skipped、failing 4 は既知の Windows pipe-FD baseline = `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1 で本作業と無関係）。

**simplify パス（reuse / quality / efficiency 3 並行）**: efficiency CLEAN（`mask_secrets` は `isinstance` 化で per-node コストむしろ低下）。reuse + quality が共通指摘した **MEDIUM 1 のみ反映** — 単調 fill ガードが `apply_event` と `apply_modify` でバイト同一複製になっていたため `LiveOrder::advance_fill(&mut self, filled_qty, avg_price)` に抽出（実弾不変条件を 1 箇所に集約・drift 防止）。pydantic fallback（pragma:no-cover の fail-safe）と reap idiom の 3 重化（`handle_shutdown` 既決ミラー・スコープ外）は据え置き。

---

## 0. Feature Inventory / バックエンド機能一覧

### 0.1 発注経路

- `PlaceOrder(venue, instrument_id, side, qty, price, order_type, time_in_force, second_secret?)` — 新規発注（`second_secret?` は Tachibana のみ、kabu は不要）
- `CancelOrder(venue, order_id, second_secret?)` — 取消（kabu は `OrderId` のみ・Password 不要。**Tachibana は `CLMKabuCancelOrder` に `sSecondPassword` 必須**、マニュアル確認済み。`second_secret?` は Tachibana のみ）
- `ModifyOrder(venue, order_id, new_price?, new_qty?, second_secret?)` — 訂正（`second_secret?` は Tachibana のみ）
  - **kabu には訂正 API が無い** → adapter 内部で「取消 → 新規発注」に変換（atomicity は保証されない旨を UI に表示）
  - Tachibana は `CLMKabuCorrectOrder` を直接発射
- `GetOrderStatus(venue, order_id)` — 単発取得（polling 用）
- `GetOrders(venue)` — 稼働中注文の一覧取得（venue 側真実。backend 再起動後の reconcile に使う、§3.8）。kabu は `GET /orders`、Tachibana は `CLMOrderList` にマップ
- `OrderEvent` (SubscribeBackendEvents stream 経由 push) — 約定・キャンセル・期限切れの push 通知（`SubscribeBackendEvents` が配信する `BackendEvent.oneof` のひとつ、§3.2 参照）
  - **kabu**: WebSocket PUSH は板情報のみ（約定通知は未配信）。`GET /orders` を 1 秒間隔 polling で代替、結果を `OrderEvent` に変換して push（§3.3.2 参照）
  - **Tachibana**: EVENT WebSocket の `EC` (約定通知) を `OrderEvent` に変換して push

### 0.2 口座情報

- `GetAccount(venue)` — 預り金・買付余力・評価額・建玉一覧の単発取得
- `AccountEvent` (SubscribeBackendEvents stream 経由 push) — 残高変化・ポジション変化の push 通知（`SubscribeBackendEvents` が配信する `BackendEvent.oneof` のひとつ、§3.2 参照）
- `ListExecutions(venue, from, to)` — 約定履歴（日付範囲指定）

### 0.3 機密情報の都度収集

- `SecretRequired` イベント (`SubscribeBackendEvents` stream 経由、§3.2) — Python 側が UI に「第二暗証番号を入力してください」モーダルを開かせるイベント種別（reverse direction: server → UI）
  - **Tachibana のみ使用**。kabu は sendorder / cancelorder とも Password 不要なので発動しない
  - §3.2 / Step 0 で新設する server-streaming RPC `SubscribeBackendEvents` の `BackendEvent.oneof` の 1 種別として実装する（**現行 proto に streaming RPC は存在しないため、Step 0 でこの RPC を新設するのが前提**。専用 RPC `SubscribeSecretRequests` は追加しない — ADR §7 参照）
  - UI は `SubmitSecret(request_id, secret)` RPC で応答。secret は Rust 側で **保持しない**（即 backend に転送して破棄）
- backend 側で受領した secret は `SecretVault`（メモリのみ）に `(venue, purpose)` 紐付けで保管し、**保管時刻から 60 秒の TTL で消去**する（連続発注では再利用するが TTL はリセットしない。API 呼び出しごとの即時 pop はしない — §1.3 ADR）

### 0.4 Watchdog & 運用

- **Venue Health Watchdog** — kabu を 30 秒間隔で軽量 ping し `4001007`/`4001017`（本体ログアウト）検出 → modal 経由で再ログイン誘導 → `/token` 再発行 → 購読自動再開
- **Instruments Daily Refresh** — 営業日 5:00 JST に全銘柄メタデータを再フェッチ（既存 parquet を atomic rename で更新）
- **Backend Idle Shutdown** — 独立起動 backend は 60 秒どの RPC も来なければ `unregister/all` を打って自己 shutdown
- **Backend Auto-Restart** — `backend_supervisor` のクラッシュループカウンタ (Phase 8 で仕込み済み) を活かし、3 回未満なら自動再起動、3 回以上は手動 (`[Restart Backend]` disabled) に格下げ

---

## 1. Architecture / 構成

### 1.1 ExecEngine の有効化

Phase 8 で意図的に **インスタンス化しなかった** Nautilus `ExecEngine` を Phase 9 で初めて有効化する。`live_runner.py` を以下構成に拡張:

```
live_runner.py
├── DataEngine        (Phase 8 で有効)
├── ExecEngine        (Phase 9 で新規有効化) ← LiveExecutionClient を 1 個 attach
└── RiskEngine        (Phase 9 で有効化。Nautilus 標準 RiskEngine を pre-trade check のみの設定で使う＝自前実装ではなく構成)
```

`LiveExecutionClient` は Nautilus 標準の `LiveExecutionClient` を venue 別に 1 実装ずつ:

- `TachibanaExecutionClient` — `python/engine/exchanges/tachibana_exec.py` (新設)
- `KabusapiExecutionClient` — `python/engine/exchanges/kabusapi_exec.py` (新設)

### 1.2 State Machines

```
OrderStateMachine (Phase 9 [NEW]) — Nautilus 標準 OrderStatus に準拠
  INITIALIZED → SUBMITTED → ACCEPTED → (PARTIALLY_FILLED) → FILLED
              ↘ DENIED (RiskEngine pre-trade reject)        ↘ CANCELED / REJECTED / EXPIRED
  ACCEPTED → PENDING_CANCEL → CANCELED
  ACCEPTED → PENDING_UPDATE → ACCEPTED (新価格/数量)   ※ Tachibana CLMKabuCorrectOrder のみ
```

Nautilus 標準の `OrderStatus`（`INITIALIZED` / `SUBMITTED` / `ACCEPTED` / `PENDING_UPDATE` / `PENDING_CANCEL` / `PARTIALLY_FILLED` / `FILLED` / `CANCELED` / `REJECTED` / `EXPIRED` / `DENIED`、`.claude/skills/nautilus_trader/src/nautilus_trader/core/rust/model.pxd:352` 確認済み）をそのまま採用する。`CREATED` / `PENDING_SUBMIT` / `PENDING_MODIFY` という状態は Nautilus に**存在しない**ため使わない（`INITIALIZED` から `SUBMITTED` へ直接遷移する）。

- **`PENDING_UPDATE` を経由するのは Tachibana の `CLMKabuCorrectOrder`（単価/数量変更）のみ。**
- **kabu の訂正は「取消 → 新規発注」変換のため、元注文は `CANCELED` で終端し、新規注文が別 `client_order_id` で `INITIALIZED` から開始する**（同一注文が `PENDING_UPDATE` をサイクルするわけではない。§2.2 / ADR §7 の `client_order_id` / `venue_order_id` 分離を参照）。

### 1.3 SecretVault

```
SecretVault (in-memory dict)  ※ Tachibana 専用。kabu は Password 不要のため使用しない
  primary_key: (venue, purpose) → request_id + secret + requested_at + ttl(60s)
    └─ 連続発注キャッシュ用。TTL 内の既存 secret は再利用し SecretRequired を発行しない
  pending: request_id → asyncio.Future[str]
    └─ SubmitSecret が届くまで TachibanaExecutionClient が await する Future

flow (Tachibana のみ、NewOrder / CorrectOrder / CancelOrder すべて同じ経路):
  1. TachibanaExecutionClient が CLMKabu* を発射しようとする
  2. SecretVault.get(venue, purpose) で TTL 内の secret を探す
     → あれば即返却（SecretRequired を発行しない）
     → なければ request_id = UUID を発行
       → SubscribeBackendEvents stream に SecretRequired{request_id, venue, kind, purpose} を push
       → asyncio.Future を pending に登録して await（max 30s timeout）
  3. UI が Bevy modal で入力 → SubmitSecret(request_id, secret) RPC で応答
  4. Python gRPC handler が pending[request_id].set_result(secret) → Future が解決
     → SecretVault.store(venue, purpose, secret) で TTL 付き保管
  5. TachibanaExecutionClient が secret を取り出して API 呼び出し実行
  6. **保管時刻から 60s TTL 満了で削除**（連続発注では step 2 で再利用するため、API 呼び出しごとの即時 pop はしない）
     ※ ADR: 連続発注で TTL はリセットされない（purpose 別に独立 TTL。保管時の 1 回だけ `call_later(60, ...)` を仕掛ける）
```

**Rust 側での secret 保持方針**: `SubmitSecret` RPC は tonic/prost が必然的に Rust の `String` としてデシリアライズする（これを避けることは不可能）。現実的な目標は「**明示保持しない**」「**cosmic-edit buffer を `zeroize` で消去**」「**ログ・ファイル・状態 resource に平文で残さない**」の3点とする。gRPC layer の一時的なデシリアライズ文字列は対象外とし、handler 関数の戻り後に GC される前提とする。

---

## 2. Venue 固有の取り扱い

### 2.1 Tachibana 発注

> 詳細は `.claude/skills/tachibana/SKILL.md` の発注セクション参照。

- 新規発注: `CLMKabuNewOrder` — `sSecondPassword` 必須（第二暗証番号、§3.3.1 で都度収集）
- 訂正: `CLMKabuCorrectOrder` — 単価 / 数量変更可能、`sOrderNumber` で対象指定
- 取消: `CLMKabuCancelOrder` — **`sSecondPassword` 必須**（マニュアルのリクエスト例 `{"sCLMID":"CLMKabuCancelOrder","sOrderNumber":"...","sEigyouDay":"...","sSecondPassword":"pswd"}` で確認済み。「取消は本人確認済み前提で不要」という旧記述は誤り）
- 約定通知: EVENT WebSocket の `EC` (約定通知) / `KP` (キープアライブ) / `SS` (システム状態) を購読（時価情報は `FD`、株価更新は Phase 8 DataEngine が担う）
- **`p_no` 採番**: 既存の `tachibana_session.json` ベースのカウンタを継続使用。プロセス再起動を跨いでも連番が破綻しないよう atomic write を維持
- `sJsonOfmt`: 通常 REQUEST は `"5"` 固定 (Phase 8 既決)

### 2.2 kabuステーション 発注

> 詳細は `.claude/skills/kabusapi/SKILL.md` の発注セクション参照。

- 新規発注: `POST /sendorder` — Password 不要（認証は X-API-KEY ヘッダのみ）。第二暗証番号も kabu には存在しない
- **訂正 API は存在しない** → adapter 内部で「`PUT /cancelorder` → 結果待ち → 新規 `POST /sendorder`」のトランザクション。失敗時の補償:
  - 取消成功 + 新規失敗 → UI に「訂正失敗。元注文は取消済み。新規発注を再試行してください」モーダル
  - 取消失敗 → UI に「訂正失敗。元注文はそのまま残っています」モーダル
- 取消: `PUT /cancelorder` — `OrderId` のみ。Password は不要（OpenAPI `RequestCancelOrder` スキーマ確認済み）
- 約定通知: PUSH WebSocket は **板情報のみ**、約定通知は無いので `GET /orders` を 1 秒間隔 polling
- **流量制限**: 発注系 5 req/s を `kabusapi_ratelimit.OrderBucket` で事前抑制 (Phase 8 既決)
- エラー対応:

  > **注意**: kabuステーション API のエラーは 2 層構造。①HTTP レスポンス body `Code` フィールド（`4001xxx` / `4002xxx` 系、リクエストチェックエラー）と ② `/sendorder` レスポンスの `Result` フィールド（発注エラー、整数コード）を区別して処理する。
  > **認証方式**: sendorder / cancelorder とも X-API-KEY ヘッダのみ。Password フィールドは一切不要（`RequestCancelOrder` / `RequestSendOrder` スキーマに Password なし）。

  | コード / 種別 | 意味（`ptal/error.html` 準拠） | Phase 9 ポリシー |
  | --- | --- | --- |
  | `4001009` (Messageコード) | APIキー不一致 | `/token` 再発行後 1 回 retry |
  | `4001007` / `4001017` (Messageコード) | ログイン認証エラー（kabuステーション本体ログアウト） | `VenueLogoutDetected` push → 再ログイン modal |
  | `4001013` (Messageコード) | APIパスワード不正（`/token` 発行失敗時） | ログイン UI に再入力を促す |
  | `Result=21` (発注エラー) | 可能額不正エラー（余力不足） | `GetAccount` を再取得して UI に最新の買付余力を表示 |
  | `Result=16` (発注エラー) | 取引数量不正エラー | UI に明示 (translate to `ORDER_QTY_INVALID`) |
  | `Result=-1` (発注エラー) | 異常終了コード（システムエラー） | `KabuApiError` を上層へ伝播し UI にエラートーストを表示 |
  | `4002001` (Messageコード) | 銘柄が見つからない | UI に明示、銘柄コード / 市場コードの組合せを確認させる |
  | HTTP 429 | スロットリング制限（流量超過） | `kabusapi_ratelimit.OrderBucket` で事前抑制 (Phase 8 既決)、超過した場合は backoff retry |

### 2.3 訂正発注の atomicity 表示

UI 上で訂正注文を出す際、kabu の場合は **必ず警告バナー** を出す:

> 「kabuステーションには訂正 API がありません。`取消 → 新規発注` の 2 段階で訂正します。途中で失敗した場合は元注文が取消のみ済むことがあります。」

Tachibana の場合は警告不要（`CLMKabuCorrectOrder` で atomic）。

---

## 3. Tasks

### 3.1 Backend: ExecEngine 有効化 & 基盤

- `live_runner.py` に `ExecEngine` / `RiskEngine` のインスタンス化を追加。`LiveVenueAdapter` から `LiveExecutionClient` を取り出して `ExecEngine.register_client()`。標準 RiskEngine は pre-trade check のみの構成で有効化（自前実装ではない、§1.1）
  - **依存コンポーネント**: `ExecEngine` / `RiskEngine` は `MessageBus` / `Cache` / `Clock` / `Portfolio` を要求する。Phase 8 で `DataEngine` 用に立てたこれらを共有する（`NautilusKernel` をフル起動するか個別 wire するかは Step 2 実装時に確定。`StrategyEngine` だけは依然無効）
- Nautilus `OrderFactory` を Strategy 外から呼べる薄い wrapper (`live/order_facade.py`) を追加。Phase 9 は手動発注のみなので Strategy 経由ではなく gRPC handler から直接 facade を叩く
  - **`OrderFactory` は `trader_id` + `strategy_id` を必須引数に取る**（`.claude/skills/nautilus_trader/src/nautilus_trader/common/factories.pyx:73` 確認済み）。手動発注には Strategy が無いため、合成 ID（例 `StrategyId("MANUAL-001")` / 既存 `TraderId`）を facade が固定で与える
  - **発注は `ExecEngine.submit_order()` ではなく `SubmitOrder` コマンドを生成して `ExecEngine.execute(command)` に渡す**（`ExecutionEngine` に `submit_order()` メソッドは存在しない。エントリ点は `execute(TradingCommand)`、`.../execution/engine.pyx:866` 確認済み。取消/訂正も同様に `CancelOrder` / `ModifyOrder` コマンド → `execute()`）
- `live/secret_vault.py` を新設。**`threading.Lock`** で並行アクセス制御する（gRPC servicer は sync ThreadPool で `submit` は worker thread・`wait_for` は live loop thread で走る cross-thread 構造のため `asyncio.Lock` は使えない）。cross-thread の Future 解決は `future.get_loop().call_soon_threadsafe(future.set_result, secret)`、TTL は `loop.call_soon_threadsafe(loop.call_later, 60, ...)` で loop thread 上に仕掛ける。**【ドリフト訂正 — 当初案 `asyncio.Lock` / `asyncio.get_event_loop().call_later` を Step 1 実装で訂正】**

### 3.2 Backend: 発注 RPC 追加

```proto
service DataEngine {
  // 既存 RPC (Phase 8 まで)...

  // Phase 9 Step 0: Backend Event Transport（発注 RPC より先に実装する） ✅ 実装済み (96c7370c)
  // 現行 proto に streaming RPC は存在しない。この RPC を新設してから他の Phase 9 RPC を実装する
  rpc SubscribeBackendEvents(SubscribeBackendEventsReq) returns (stream BackendEvent);
  //   BackendEvent.oneof payload:
  //     - SecretRequired{request_id, venue, kind ("second_secret"), purpose}  ※ Tachibana のみ
  //     - OrderEvent{order_id, venue_order_id, client_order_id, status, filled_qty, avg_price, ts_ms}
  //     - AccountEvent{cash, buying_power, positions, ts_ms}
  //     - VenueLogoutDetected{venue}  ※ kabu Watchdog が 4001007/4001017 を検知したときに push（§3.5）

  // Phase 9 Step 2 以降: 発注 RPC
  rpc PlaceOrder(PlaceOrderReq) returns (PlaceOrderRes);
  rpc CancelOrder(CancelOrderReq) returns (CancelOrderRes);    // Tachibana は second_secret 必須
  rpc ModifyOrder(ModifyOrderReq) returns (ModifyOrderRes);    // Tachibana は second_secret 必須
  rpc GetOrderStatus(GetOrderStatusReq) returns (Order);
  rpc GetOrders(GetOrdersReq) returns (GetOrdersRes);          // 稼働中注文の一覧（再起動後の reconcile / 楽観的状態突合に使う、§3.8）
  rpc GetAccount(GetAccountReq) returns (AccountSnapshot);
  rpc ListExecutions(ListExecutionsReq) returns (ListExecutionsRes);  // unary paged（streaming 不使用）

  // SecretVault に対する UI 応答
  rpc SubmitSecret(SubmitSecretReq) returns (SubmitSecretRes);
}
```

> **注**: `ListExecutions` は当初 `returns (stream Execution)` としていたが、現行 transport に streaming が存在しない状態で実装するとリスクが高い。Phase 9 では `ListExecutionsRes { repeated Execution executions; string next_cursor; }` の unary paged response とし、streaming は Phase 10 以降で検討する。

**注文を変化させる write 系 RPC**（`PlaceOrder` / `CancelOrder` / `ModifyOrder`）は `ExecutionMode` を server 側で検証し、`Replay` モード時は **構造的に reject** (`FAILED_PRECONDITION` + error code `EXECUTION_MODE_PRECONDITION`)。`SubscribeBackendEvents`（transport 本体、Rust 受信タスクが全モードで張る）/ `SubmitSecret` / 読み取り系（`GetOrderStatus` / `GetOrders` / `GetAccount` / `ListExecutions`）は reject 対象外（Replay モードでは単に Live データが流れないだけ）。

### 3.3 Backend: ExecutionClient 実装

#### 3.3.1 TachibanaExecutionClient

- `submit_order()` / `cancel_order()` / `modify_order()` の冒頭でそれぞれ SecretVault に第二暗証番号がなければ `SecretRequired` を push → 取得を待つ (max 30s timeout、超過で `SECRET_TIMEOUT`)
  - **CLMKabuCancelOrder も `sSecondPassword` 必須**（マニュアル確認済み）。取消だけ SecretVault を bypass しない
- `CLMKabuNewOrder` / `CLMKabuCorrectOrder` / `CLMKabuCancelOrder` の組み立てに既存 `tachibana_url.py` (Phase 8) を流用
- EVENT WebSocket からの `EC` を `OrderEvent` に変換し `SubscribeBackendEvents` ストリーム経由で UI に push
- 第二暗証番号は **保管から 60s TTL で破棄**（§1.3 の reuse cache 方針。連続発注のため API 呼び出しごとの即時 pop はしない。TTL は再利用でリセットしない）

#### 3.3.2 KabusapiExecutionClient

- `submit_order()` / `cancel_order()` は Password 不要。X-API-KEY ヘッダは `kabusapi_auth.py` が自動付与するため、SecretVault / SecretRequired の発動は kabu では発生しない
- `modify_order()` は内部で `cancel_order()` → wait `OrderCanceled` → `submit_order(new_params)` のシーケンス。失敗時の補償は §2.2 参照
- 約定確認は 1 秒間隔 polling (`GET /orders?id=...`) を `asyncio.Task` で回す
- 発注エラー `Result=-1`（異常終了コード）を受けたら `KabuApiError` を上層へ伝播し UI にエラートーストを表示（注: 4001xxx 系 Message コードと発注エラー Result コードは別系統、§2.2 エラーテーブル参照）

### 3.4 Backend: 口座同期

- `live/account_sync.py` 新設
  - 起動時 + 30 秒間隔で `GetAccount` 相当を venue API に発射 (`kabusapi: GET /wallet/cash` + `GET /positions`、Tachibana: `CLMGenbutuKabuList` (現物保有銘柄一覧) + `CLMZanKaiKanougaku` (買余力))
  - 差分があれば `AccountEvent` を `SubscribeBackendEvents` stream に push
- ポジションは Nautilus `Cache` に登録する（`cache.position()` は **getter**。登録は `cache.add_position(position, oms_type)`、`.../cache/cache.pyx:2348` 確認済み）。Live では本来 `LiveExecutionClient` の reconciliation（`PositionStatusReport` 生成）で Cache を同期するのが Nautilus 流儀のため、Phase 9 では venue から取得した建玉を `add_position` で直接入れるか reconciliation 経由にするかを Step 4 実装時に確定する。いずれも Snapshot Reducer の既存経路で UI 表示（PositionsPanel は Phase 8 で実装済み、Phase 9 で初めて Live データが流れる）

### 3.5 Backend: Venue Health Watchdog (Phase 8 繰り越し)

- `live/health_watchdog.py` 新設
- 30 秒間隔で kabu を軽量 ping (`GET /apisoftlimit` を使用。`HEAD` は `4001014 許可されていないHTTPメソッド` で失敗するため使用禁止。新規 `/token` 発行も避ける)
- `4001007` または `4001017`（ログイン認証エラー、kabuステーション本体ログアウト）検出 → `SubscribeBackendEvents` stream に `VenueLogoutDetected{venue}` push → UI が再ログイン modal を開く → ログイン完了後に既存購読を `kabusapi_register` から再発行
- Tachibana 側は EVENT WebSocket の disconnect で検知 (auto-reconnect は Phase 8 で実装済み、Phase 9 は SS=01 閉局検出ロジックを追加)

### 3.6 Backend: Instruments Daily Refresh (Phase 8 繰り越し)

- `live/instruments_scheduler.py` 新設
- 純粋な `asyncio.create_task` + `asyncio.sleep(next_5am_jst - now)` で実装（`apscheduler` は未確立の外部依存のため使用しない。`asyncio.sleep_until_next` は存在しない）。JST 5:00 までの秒数を `datetime` で算出してスリープ → 営業日判定 → 全銘柄 fetch → **Live universe メタデータ parquet（`cache_dir/the-trader-was-replaced/instruments/<venue>.parquet`、Phase 8 がログイン時に全置換する store）を atomic rename で更新**
  - **対象アーティファクトの区別**: 更新するのは上記の Live venue 銘柄メタ parquet（§0.4 / §6 の「Instruments parquet」）。Phase 7.5b の Replay 用シンボルリスト `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` とは**別物**で、こちらは本スケジューラの対象外
- 取引日カレンダーは J-Quants の `/markets/trading_calendar` を使用

### 3.7 Backend: Idle gRPC Shutdown (Phase 8 繰り越し)

- Python gRPC interceptor で `last_request_ts: float` を `asyncio.Lock` で保護して記録（Python サーバは asyncio 単一スレッド前提。Rust の `AtomicU64` は不要・使用不可）
- background asyncio task が 5 秒間隔で確認、`time.monotonic() - last_request_ts > 60.0` かつ **独立起動モード** (= Bevy supervisor 配下でない) なら:
  1. `unregister/all` を best-effort で発射
  2. `server.stop(grace=2.0)`
- Bevy supervisor 配下では `BACKEND_SUPERVISED=1` 環境変数で判定し idle shutdown を無効化
- **確定セマンティクス (レビュー M-C)**: 開きっぱなしの `SubscribeBackendEvents` ストリームは「アクティビティ」として扱う（per-stream heartbeat スレッドが idle clock を周期 touch する）。Rust UI は全モードでこのストリームを常時張るため、**UI が接続している間は idle shutdown が発火しない**（= idle shutdown は「UI も他クライアントも居ない、CLI 単独起動・非 supervised の放置 backend」だけを self-terminate する）。これは §3.7 の「60s 無 RPC で自己終了」を意図的に狭めたもので、`test_open_subscribe_stream_keeps_clock_fresh` で固定。idle なストリーム購読だけで誤って backend を落とすのを防ぐのが目的。

### 3.8 Backend: Auto-Restart (Phase 8 繰り越し)

- `src/backend_supervisor.rs` の crash loop カウンタ (Phase 8) を活用
- 直近 60s で 1〜2 回クラッシュ → 即座に再 spawn（ユーザー操作不要、トーストで「Backend を再起動しました」のみ通知）
- 3 回以上 → 既存の `[Restart Backend]` disabled UI に格下げ（Phase 8 既決）
- in-flight 注文の保護: 再起動直後に `GetOrders` を発射し、UI 側の楽観的 order list と diff → 不整合があれば「order_id=XXX の状態が backend 再起動で不明になりました」モーダル

### 3.9 UI: OrderPanel 新設

- `src/ui/order_panel.rs` を新設（Phase 8 で `[Phase 9]` マーク済みのファイル）
- 表示条件: `ExecutionMode == LiveManual` のときのみ
- フィールド:
  - 銘柄 (Sidebar 選択の `SelectedSymbol` と連動、手動 override 可)
  - 売買区分（買 / 売）ラジオ
  - 数量（**売買単位 (lot size) の倍数**を検証。`tick_size` は価格刻みであって数量検証には使わない。現物は 100 株単位が一般的だが銘柄メタデータの売買単位を参照する）
  - 価格（成行 / 指値、指値の場合は `tick_size`〔呼値〕整合チェック）
  - 執行条件（寄付 / 引け / 不成 / 当日中）
- **2 段階確認モーダル**: `[発注]` ボタンクリック → モーダルで「銘柄・数量・価格・推定約定額・概算手数料」を再表示 → `[Confirm]` で初めて RPC 発射
- 推定約定額は `qty * price` の単純計算（手数料は venue 別の概算テーブルから引く、誤差は明示）
- レイアウト保存先は `live_manual_layout.json` (Phase 8 で概念定義済み)

### 3.10 UI: SecretRequired モーダル（Tachibana のみ）

- `src/ui/secret_modal.rs` を新設（既存 `ModalLayer` 機構を流用）
- `SubscribeBackendEvents` stream で `SecretRequired{request_id, venue, kind, purpose}` を受信 → モーダル open
  - **kabu は Password 不要のため `SecretRequired` が発行されることはない**。モーダルは Tachibana の第二暗証番号収集専用
- フィールド: cosmic-edit 1 行、`password` モード（マスク表示）
- 入力後 `SubmitSecret` RPC 発射 → cosmic-edit buffer を `zeroize` で破棄、モーダル close
- タイムアウト 25 秒（backend の 30 秒タイムアウトより少し短く設定）でモーダル auto-close + `SECRET_INPUT_CANCELED` をエラートーストに

### 3.11 UI: 訂正発注の警告バナー (kabu のみ)

- `OrderPanel` で対象 venue が kabu かつ `[訂正]` ボタンが押された場合 → モーダル上部に warning バナー表示
- ユーザーが `[理解した上で訂正する]` チェックボックスを ON にして初めて `[Confirm]` が enabled になる

### 3.12 UI: PositionsPanel / OrdersPanel の Live 対応

現行 Rust 側は `GetPortfolio` unary RPC 後に `PortfolioLoaded` イベントを経由して描画する経路が中心で、Push 受信タスクは存在しない。Phase 9 で以下 2 経路を追加する：

- **初期ロード / 手動リフレッシュ**: `GetAccount` RPC（新設）を呼んで口座スナップショットを取得し、`PortfolioLoaded` と同じ Snapshot Reducer に流す。既存パネル描画ロジックの変更は不要
- **差分 push**: `SubscribeBackendEvents` gRPC server-streaming を Rust 受信タスク (`tokio::task`) で購読し、`AccountEvent` / `OrderEvent` を受けるたびに既存 Snapshot Reducer の `on_account_event` / `on_order_event` ハンドラに渡す
  - Rust 受信タスクは **Step 0 で実装済み**（`src/main.rs` の `setup_backend_connection` 内 → `BackendEventChannel` mpsc → `backend_event_drain_system`）。**Step 0 時点では各イベントをログ出力するのみ**で、`on_account_event` / `on_order_event` の Snapshot Reducer 結線は本ステップ (Step 4) で追加する
- 追加実装: OrdersPanel に **右クリック → [取消] / [訂正]** コンテキストメニュー（コンテキストメニュー自体は新規実装、`bevy_egui` で簡易実装）

---

## 4. File Layout

```
python/engine/
├── live_runner.py                  # ExecEngine + RiskEngine 有効化
├── live/
│   ├── backend_event_bus.py [DONE] # Step 0: SubscribeBackendEvents 用 threadsafe fan-out (queue.Queue)
│   ├── secret_vault.py     [NEW]   # メモリのみ secret 保管、60s TTL
│   ├── order_facade.py     [NEW]   # OrderFactory wrapper、手動発注 entry point
│   ├── account_sync.py     [NEW]   # 30s 間隔の余力・ポジション同期
│   ├── health_watchdog.py  [NEW]   # 30s 間隔の GET /apisoftlimit ping、4001007/4001017 検出
│   ├── instruments_scheduler.py [NEW]  # 5:00 JST に全銘柄再フェッチ
│   └── aggregator.py               # (Phase 8 で実装、Phase 9 では変更なし)
├── exchanges/
│   ├── tachibana_exec.py   [NEW]   # CLMKabuNewOrder / Change / Cancel + EC 約定通知
│   ├── kabusapi_exec.py    [NEW]   # /sendorder / /cancelorder + GET /orders polling
│   └── (Phase 8 既存ファイル)
└── server_grpc.py                  # PlaceOrder / CancelOrder / ModifyOrder / SubmitSecret / 口座 RPC を追加

src/
├── main.rs                         # [DONE] Step 0: backend-event subscriber tokio task (setup_backend_connection) + BackendEventChannel + backend_event_drain_system
├── trading.rs                      # [DONE] Step 0: trading::BackendEvent ミラー enum + AccountPosition
├── backend_supervisor.rs           # auto-restart ロジック有効化（crash loop カウンタは Phase 8 既存）
└── ui/
    ├── order_panel.rs      [NEW]   # LiveManual 専用、手動発注 UI（Phase 8 から繰り越し）
    ├── secret_modal.rs     [NEW]   # Tachibana 第二暗証番号入力モーダル（kabu は Password 不要）
    └── (Phase 8 既存ファイル)
```

---

## 5. Implementation Order

各ステップ完了時点で `cargo run` できる状態を維持する。Phase 8 と同じく、本番 venue に接続しなくても **MockVenueAdapter の発注経路** で UI → backend の往復をテストできるよう Step 1 で mock を先に拡張する。

0. **Step 0 — Backend Event Transport 新設（`SubscribeBackendEvents`）** ✅ **完了** (`96c7370c` + レビュー修正 `e221399e`、2026-05-20)
   - `engine.proto` に `rpc SubscribeBackendEvents` と `BackendEvent` oneof message を追加 ✅
   - Python 側: `server_grpc.py` に server-streaming handler を追加 ✅
     - **【ドリフト訂正 — 当初案: `LiveEventBus` から `asyncio.Queue` 経由】** 実装は **新設 `live/backend_event_bus.py` の `BackendEventBus`（`queue.Queue` + `threading.Lock` の threadsafe fan-out）**。gRPC servicer は sync ThreadPool（handler は `def`）で、streaming handler は worker thread でブロックし publish は別 thread から呼ばれるため、asyncio ではなく threadsafe queue が正しい（市場データ用 `LiveEventBus` とは別物）。`servicer.publish_backend_event()` + `context.add_callback(sub.close)` で RPC teardown 時に blocked な `queue.get()` を解放
   - Rust 側: gRPC server-streaming 受信タスク (`tokio::task`) を追加 ✅
     - **【ドリフト訂正 — 当初案: `src/backend_client.rs`（存在しない）】** 実装は **`src/main.rs` の `setup_backend_connection`** 内。独自 client + Ready-gated 再接続ループ（connect/subscribe/stream-end の全失敗パスを `events_reconnect_backoff()` で 500ms backoff self-heal）
     - **【スコープ注記】** Step 0 では受信イベントを `BackendEventChannel`（`mpsc`）経由で `backend_event_drain_system` に渡し**ログ出力のみ**。`EventWriter` → Snapshot Reducer の結線（`on_account_event` / `on_order_event`）は **Step 3/4 に延期**（§3.12）
   - この Step が完了してから Step 1 以降に進む（`SubscribeBackendEvents` stream 前提の機能がすべてここに依存する）
1. **Step 1 — MockVenueAdapter の発注経路 + SecretVault**
   - `MockVenueAdapter.submit_order()` を追加（成功・失敗・部分約定の各パターンを返せる）
   - `live/secret_vault.py` を実装し SecretVault unit test
   - `SubmitSecret` RPC の protobuf 追加（`SecretRequired` 自体は Step 0 で `BackendEvent.oneof` として定義済みなので、ここでは応答側 RPC のみ）
2. **Step 2 — 手動発注 facade + OrderEvent stream**（ADR §7「Step 2 は手動発注 facade」参照。当初の「ExecEngine 有効化」は Phase 10 / LiveAuto に延期）
   - `live/order_facade.py` 新設（transport 非依存の手動発注 dispatch。`adapter.submit_order` / `cancel_order` 委譲 + in-memory order store）
   - `PlaceOrder` / `CancelOrder` / `GetOrderStatus` RPC を実装（mock 経由で疎通確認。Replay は `EXECUTION_MODE_PRECONDITION` reject、runner 未起動は `VENUE_LOGIN_REQUIRED`）
   - `SubscribeBackendEvents` stream に `OrderEvent` を push する経路を実装（unary response にも同じ `OrderEvent` を返す）
3. **Step 3 — OrderPanel UI + SecretModal UI**
   - `src/ui/order_panel.rs` 新設、2 段階確認モーダル含む
   - `src/ui/secret_modal.rs` 新設、`zeroize` 連携
   - Mock 経由で「発注 → 第二暗証番号入力 → 約定通知 → OrdersPanel に表示」が通る
4. **Step 4 — Account 同期 + PositionsPanel/OrdersPanel Live 対応**
   - `live/account_sync.py` 実装、`AccountEvent` push
   - 既存パネルへの Live データ流入確認
   - OrdersPanel の右クリックコンテキストメニュー (取消 / 訂正) 追加
5. **Step 5 — TachibanaExecutionClient**
   - `CLMKabuNewOrder` / `CLMKabuCorrectOrder` / `CLMKabuCancelOrder` 実装 + EVENT WebSocket `EC`→`OrderEvent` 変換 + SecretVault 結線（NewOrder/Correct/Cancel すべてで第二暗証番号を要求）
   - **E2E は今マージした headless ハーネス（`src/backend_sync.rs` の lib 抽出 + `tests/e2e/`）で実装し、人間の介在を極力 0 にする**（詳細は下記「§5.1 Step 5 E2E 方針」）。実 Demo 環境を人手で叩く旧 UX 確認は、env 供給の credential による**任意・gated な smoke 1 本**に縮退させる
6. **Step 6 — KabusapiExecutionClient**
   - `POST /sendorder` / `PUT /cancelorder` 実装
   - **訂正は「取消 → 新規発注」変換** + 補償ロジック + UI 警告バナー
   - Password 不要の確認 E2E（kabu は X-API-KEY のみで発注・取消が完結すること） (Verify 環境)
7. **Step 7 — Venue Health Watchdog (kabu 早朝ログアウト自動回復)**
   - `live/health_watchdog.py` 実装
   - kabu 本体強制ログアウト → modal → 再ログイン → 購読自動再開の E2E
8. **Step 8 — Backend Auto-Restart + Idle Shutdown**
   - `backend_supervisor.rs` の 60s 内 3 回未満 → 自動再起動を有効化
   - 独立起動モードの idle 60s shutdown を実装
   - in-flight 注文の re-sync ロジック実装
9. **Step 9 — Instruments Daily Refresh**
   - `live/instruments_scheduler.py` 実装
   - 営業日 5:00 JST 動作確認 (test では時刻 mock で検証)
10. **Step 10 — Polish**
    - secrets masking ログフィルタの再検証 (Tachibana 第二暗証番号がログに出ないこと)
    - drawio アーキ図 `phase9-architecture.drawio.svg`
    - Phase 10 (Promote to Live) への引き継ぎ事項を docs にまとめる

### 5.1 Step 5 E2E 方針（マージ済み headless ハーネス活用・人間の介在を極力 0 に）

> `feat/e2e-test-harness` をマージ済み（merge commit `a94882bf`）。backend↔ECS 同期層を
> `src/backend_sync.rs`（lib）へ抽出し、`MinimalPlugins` の headless `App` に
> `StatusUpdateChannel` / `BackendEventChannel` を insert → `BackendEvent` / `BackendStatusUpdate` /
> `TransportCommand` を縫い目から注入 → `app.update()` で resource を assert できる。**GUI 描画にも
> OS マウス座標にも一切依存しない。** カタログは `tests/e2e/FLOWS.md`（F3 order_event / F4
> account_event / F5 secret_required が Step 5 の種）。反対側（`TransportCommand`→gRPC→
> `BackendStatusUpdate`）は `tests/backend_integration.rs` の mock tonic サーバが既にカバー。

Step 5 の E2E は **3 層**に分け、人間が毎回操作する箇所を 0 にする。CI で回る決定論的層を主とし、実
Demo 環境は任意・gated の smoke 1 本だけに留める。

1. **UI / reducer 層（`be: mock`・CI・人間 0）** — headless ハーネスで完結:
   - `BackendEvent::SecretRequired` を注入 → `SecretPrompt.active == Some(..)` を assert（F5 を Step 5 用に拡張）。
   - **第二暗証番号の応答は GUI モーダルに人が打たず、`TransportCommand::SubmitSecret { request_id, RedactedSecret(env 由来) }` をハーネスから直接送る**（UI ボタン/keyboard drain をバイパス。secret は `TACHIBANA_TEST_SECOND_SECRET` 等の env / fixture 由来のダミー）。送出後に `SecretPrompt.active == None`（モーダル close）を assert。
   - 発注ラウンドトリップ: `TransportCommand::PlaceOrder/CancelOrder/ModifyOrder` 注入 →（mock tonic 経由で）`OrderSeeded`/`OrderStatusUpdated`/`OrderModified` → `LiveOrders` 反映を assert。`BackendEvent::OrderEvent`/`AccountEvent` 注入 → `LiveOrders` / `PortfolioState` 反映を assert（F3/F4）。
   - secret timeout（25s auto-close）・キャンセルも擬似時計/縫い目で検証（人間の待機不要）。
2. **backend ExecutionClient 層（`be: mock`・CI・人間 0）** — **fake Tachibana transport** に対して検証（実 Demo を叩かない）:
   - `tachibana_url.py`（Phase 8）で組んだ `CLMKabuNewOrder` / `CLMKabuCorrectOrder` / `CLMKabuCancelOrder` のリクエスト JSON を、HTTP/WebSocket を差し替え可能な fake（記録 & 定型応答）に流し、**送信ペイロードの不変条件**（`sCLMID`・`sSecondPassword` の付与・`sOrderNumber`+`sEigyouDay` の 2 識別子・`p_no` 採番）を assert。
   - SecretVault の TTL（保管 60s・連続発注で再利用・リセットなし）と、`SecretRequired` push → `SubmitSecret` → API 実行の cross-thread Future 解決を、env 由来ダミー secret で自動検証（Step 1 の secret_vault unit test を E2E 経路まで延伸）。
   - EVENT WebSocket の `EC`（約定通知）フレームを fake から流し込み → `OrderEvent` 変換 → `publish_backend_event` を assert。
3. **実 Demo 環境 smoke（`be: real`・任意・gated・人間ほぼ 0）** — CI から外し、`TACHIBANA_DEMO_E2E=1` 等の env フラグでのみ起動する **1 本**に限定:
   - login credential は Phase 8 の `credentials_source`（`env` / `session_cache`）から、第二暗証番号も `TACHIBANA_TEST_SECOND_SECRET` 等の env から供給し、**対話プロンプト・GUI 入力を踏まない**。人間の作業は「env / session_cache を 1 度設定する」だけで、テスト実行ごとの介在は 0。
   - 目的は実 API との疎通忠実度確認（`1 株 / 成行 / 当日中` の発注→`EC`→取消）。失敗しても CI を割らない（nightly / 手動）。

**不変条件**: secret は env/fixture 由来のダミーのみを CI に置き、平文を repo・ログ・スナップショットに残さない（§6 セキュリティ）。実口座の本物の第二暗証番号は CI/E2E に投入しない（Demo 専用 credential を使う）。`tests/e2e/FLOWS.md` に Step 5 の flow（`F5+`/新規 `H. Tachibana 発注`）を追記してから実装する。

---

## 6. Success Criteria

### 発注経路

- LiveManual モードで OrderPanel から `1 株 / 成行 / 当日中` の手動発注ができ、約定後に PositionsPanel / OrdersPanel に反映される。**検証は §5.1 の headless ハーネス（`be: mock`・CI・人間 0）を主とし**、実 Demo / Verify 環境は env 供給 credential による任意・gated smoke（CI 外）に留める
- 取消・訂正が両 venue で動作する。kabu の訂正は「取消 → 新規」の 2 段階であることが UI 警告バナーで明示される
- **Tachibana** 第二暗証番号が **SecretVault 保管から 60 秒以内にメモリから消える**（連続発注で再利用しても TTL リセットなし。debug log で確認、または `gc.get_objects()` 走査の unit test）
- **Tachibana** 第二暗証番号が **明示保持されない** ことを確認: (a) cosmic-edit buffer の `zeroize` 動作確認、(b) ログ・session ファイル・状態 resource に平文が出現しないこと（`process memory dump` での完全消去は tonic/prost のデシリアライズ一時文字列を対象外とする現実的な目標に変更）
- **Tachibana** 取消 (`CLMKabuCancelOrder`) でも第二暗証番号収集が正しく動作することをテストで確認（旧仕様「取消は不要」は廃止）。検証は §5.1 layer 1/2（headless ハーネス + fake transport、env 由来ダミー secret）で人手なしに自動化する
- **kabu** は sendorder / cancelorder とも Password 不要（X-API-KEY のみ）なので SecretVault は使用されないことをテストで確認
- `Replay` モードで `PlaceOrder` RPC を発射すると `EXECUTION_MODE_PRECONDITION` で reject される (unit test)

### 口座同期

- 起動時に kabu / Tachibana の買付余力・保有ポジションが正しく取得され BuyingPowerPanel / PositionsPanel に表示される
- 約定後 30 秒以内に PositionsPanel が更新される
- 余力不足（kabu: 発注エラー `Result=21` / Tachibana 相当）で発注した際、`GetAccount` 再取得後に最新余力が UI に出る

### 運用系 (Phase 8 繰り越し)

- kabu 本体を手動ログアウト → 30 秒以内に Venue Health Watchdog が検知 → 再ログイン modal → ログイン完了で購読自動再開 (E2E)
- 営業日 5:00 JST に Instruments parquet が atomic 更新される (時刻 mock test)
- 独立起動 backend が 60 秒 idle で `unregister/all` 発射後に自己 shutdown する (unit test + manual)
- Bevy supervisor 配下では idle shutdown が無効化される (環境変数判定 unit test)
- backend を `taskkill /F` で殺すと 60 秒以内 3 回未満なら自動再起動、3 回以上で `[Restart Backend]` disabled に格下げ (Phase 8 のクラッシュループカウンタ流用)

### セキュリティ

- 全文 grep でログ・コアダンプ・session ファイルに Tachibana 第二暗証番号が平文で出現しない
- `SecretVault` を `pickle.dumps()` した結果に平文が含まれない（メモリスナップショット採取テスト、Tachibana 専用）

---

## 7. Open Questions & ADRs

### ADR: Phase 9 で初めて ExecEngine をインスタンス化する

Phase 8 では `DataEngine` のみホストして発注経路を構造的に遮断していた。Phase 9 で `ExecEngine` を有効化する際:

1. **Venue 別に 1 `LiveExecutionClient` を attach** — Nautilus 標準の構造に従う
2. **`StrategyEngine` は依然として無効** — 戦略からの自動発注は Phase 10。Phase 9 の発注は gRPC handler から `order_facade` 経由で `SubmitOrder` コマンドを生成し `ExecEngine.execute(command)` に渡す（`ExecutionEngine` に `submit_order()` は無く `execute(TradingCommand)` がエントリ点。§3.1 参照）
3. **読み取り専用モード fallback は持たない** — `ExecutionMode == Replay` のとき backend は `execute()` を呼ぶ前に RPC 層で reject する。ExecEngine 自体は LiveManual / LiveAuto モードでは常時稼働

### ADR: Step 2 は「手動発注 facade」。フル Nautilus ExecEngine wiring は Phase 10 / LiveAuto に延期

> 追記 2026-05-20（Step 2 実装時に確定。§3.1 の「NautilusKernel フル起動 vs 個別 wire」の留保への回答）。

現状の live パイプライン（`python/engine/live/`）は `LiveRunner → LiveEventBus → LiveReducerBridge` の bespoke 構成で、Nautilus の `TradingNode` / `LiveExecutionEngine` / `RiskEngine` / `MessageBus` / `Cache` / `Portfolio` を一切持たない（Nautilus `BacktestEngine` を使うのは replay/backtest 経路の `strategy_runtime/engine_runner.py` のみ）。

Phase 9 の目的は **手動発注経路を開ける** こと。ここでフル Nautilus live wiring を入れると Step 2 が「注文 RPC」ではなく「Live エンジン基盤導入」になり、Phase 10（Promote to Live / 戦略自動発注）の本丸と責務が重なる。Step 0/1 でも計画書の重量級 Nautilus 前提を bespoke 構成へドリフト訂正してきた経緯と一貫する。

選択肢:

- **A. フル Nautilus ExecEngine + RiskEngine + Cache + Portfolio + MessageBus + LiveExecutionClient + OrderFactory を live にも導入** — コード量と Nautilus 固有複雑性が大きく、Phase 10 の責務を先食いする
- **B. 軽量 `order_facade`: execution mode 検証 → `adapter.submit_order()` → `OrderResult` を proto `OrderEvent` に変換 → `publish_backend_event` + unary response にも同じ `OrderEvent` を返す** ← **採用**

採用理由: 「手動でボタンを押す経路を開ける」という Phase 9 のスコープに必要十分。`adapter.submit_order` は既に存在（Step 1）。現実的な移行順序は **thin facade → `LiveExecutionClient` adapter 化 → full live engine** とし、真正 Nautilus ExecEngine wiring は Phase 10 / LiveAuto で導入する。

実装ノート:

- `order_facade` は transport 非依存（proto を import しない）。proto 変換と `publish_backend_event` は gRPC handler（`server_grpc.py`）の責務。
- write 系 RPC（`PlaceOrder` / `CancelOrder`）は `current_mode == "Replay"`（または mode 未設定）を `error_code="EXECUTION_MODE_PRECONDITION"`（structured error、house style: `context.abort` は token/INVALID_ARGUMENT のみ）で reject。runner 未起動は `VENUE_LOGIN_REQUIRED`。
- `GetOrderStatus` は当初案 `returns (Order)` を `returns (GetOrderStatusRes{success, error_code, OrderEvent order_event})` に変更（`Order` message は `OrderEvent` と重複するため新設しない）。facade が in-memory に発注を track し参照する。
- Tachibana 第二暗証番号 / `SecretRequired` の facade 結線は **Step 5** で追加（mock / kabu は不要）。Step 2 では `second_secret` フィールドのみ proto に用意し facade は受理して無視する。

### ADR: Tachibana 第二暗証番号は NewOrder / CorrectOrder / CancelOrder すべてで都度収集・メモリのみ保持・60s idle で破棄（kabu は非該当）

Phase 8 ADR を継承。Phase 9 で UI 実装が入る。**Tachibana は CLMKabuCancelOrder でも `sSecondPassword` 必須**（マニュアル確認済み。Phase 8 の「取消は不要」記述は誤り）。**kabu は sendorder / cancelorder とも Password 不要（OpenAPI `RequestCancelOrder` / `RequestSendOrder` スキーマ確認済み）のため SecretVault は Tachibana 専用となる**:

1. **ファイル / keyring に書かない** — 漏洩窓を最小化
2. **Rust 側に滞留させない** — cosmic-edit buffer から直接 gRPC バイト列、応答後 `zeroize`
3. **Python 側 SecretVault に保管、TTL 60s** — 連続発注時の入力負担を緩和するための最小限の cache
4. **連続発注で TTL がリセットされない** — purpose 別に独立 TTL (連射時に時計がずるずる延びるのを避ける)

### ADR: kabu の Password 不要は Phase 8 ADR / skill 本文の旧記述を上書きする

Phase 8 ADR および tachibana/kabusapi skill 本文には「`PUT /cancelorder` body に `Password` フィールドが必要」という記述が残っている。**Phase 9 はこれを上書きする**。根拠は `kabu_STATION_API.yaml` の `RequestCancelOrder` スキーマ（`OrderId` のみ required、`Password` フィールドなし）。実装者は Phase 8 ADR / skill の旧記述ではなく Phase 9 計画書と OpenAPI を参照すること。

### ADR: kabu の訂正は「取消 → 新規発注」変換を adapter 層で行い、atomicity 非保証を UI で明示する

kabu API には訂正エンドポイントが無い。選択肢:

- **A. 訂正機能を提供しない** — UX が悪い (取消 + 新規を 2 操作で要求)
- **B. adapter 層で取消 + 新規を変換、atomicity 非保証を UI で明示** ← **採用**
- **C. adapter 層で同上、atomicity を 2PC 風に保証** — kabu API では実現不可能

採用理由: ユーザーの操作回数を減らす UX 価値が、atomicity 喪失のリスクより大きい。リスクは UI 警告バナー + チェックボックスで explicit consent を取ることで mitigation。

### ADR: SecretRequired は `SubscribeBackendEvents` の `BackendEvent.oneof` として実装する

Server → UI の reverse-direction 通信が必要。選択肢:

- **A. 新規 server-streaming RPC `SubscribeSecretRequests`** — 別チャンネル管理が増える
- **B. `SubscribeBackendEvents` の `BackendEvent.oneof` として追加** ← **採用**

採用理由: OrderEvent / AccountEvent / VenueLogoutDetected と同一チャンネルで配信することで Rust 受信タスクが 1 つで済み、一貫性が高い。

### ADR: Backend Auto-Restart は 60s 内 3 回未満を自動、それ以上を手動

Phase 8 では crash loop カウンタを仕込んでおき、Phase 9 で自動再起動経路を有効化する。

1. **発注経路を持つ Phase 9 で自動再起動が必要** — Phase 8 の「手動再起動」は read-only 前提だったが、Phase 9 では発注 in-flight 中のクラッシュ復旧が UX 上必要
2. **二重発注リスク** — 自動再起動直後に `GetOrders` で venue 側真実を取得し、UI の楽観的状態と diff → ユーザー確認モーダル
3. **3 回以上は人間介入** — クラッシュループ時は原因切り分けを優先

### ADR: 独立起動 backend は idle 60s で `unregister/all` + 自己 shutdown、supervisor 配下では無効

Phase 8 で繰り越した項目。判定は環境変数 `BACKEND_SUPERVISED=1` (supervisor が起動時にセット) で行う。

### Open Question: 信用取引・先物への拡張

Phase 9 は現物のみ。信用 / 先物 / オプションは:

- 注文タイプが増える（建玉指定、返済方法、貸借区分）
- API parameter が拡張される（kabu の `MarginTradeType` / Tachibana の `sBaibaiKubun` 値域）
- ポジション計算が複雑化（評価益 / 評価損のキャッシュ反映）

Phase 11 以降で別 spec として切り出す。Phase 9 の `submit_order()` シグネチャは将来拡張可能な形 (kwargs dict) で設計しておく。

### ADR: 注文 ID は `client_order_id` (client 生成 UUID) と `venue_order_id` (venue 採番) を分離する

kabu の「取消 → 新規発注」訂正ではブローカー側の `OrderId` が変わる。backend 再起動復旧時も venue 側の ID を再取得して突合する。統一した追跡には 2 つの ID が必要:
- `client_order_id`: gRPC handler が `PlaceOrder` 受信時に生成する UUID。UI の楽観的状態追跡に使う
- `venue_order_id`: 各 venue が返す採番文字列（Tachibana: `sOrderNumber`、kabu: `OrderId`）。再起動後の reconcile に使う

`OrderEvent` message は両フィールドを持つ（§3.2 proto 参照）。

### ADR: `ListExecutions` は unary paged response とする（streaming は Phase 10 以降）

当初 `returns (stream Execution)` としていたが、現行 transport には streaming RPC が存在しない（Phase 9 Step 0 で追加する `SubscribeBackendEvents` はサーバ push 専用）。`ListExecutions` は `returns (ListExecutionsRes)` の unary paged response とし、`next_cursor` フィールドで続きを取得する設計にする。streaming 化は Phase 10 以降で判断する。

### Open Question: 約定通知の遅延上限

kabu は polling (1s) のため最大 1 秒遅延が発生する。Tachibana は EVENT WebSocket で push される。SLA としてどこまで保証するか:

- Phase 9 では「最大 2 秒遅延を許容、それ以上は warning ログ」とする
- Phase 10 (Promote to Live) で戦略が約定確認を待つ場合は適宜タイムアウト設計を見直す

---

## 8. Phase 10 への引き継ぎ事項

Phase 9 で意図的に Phase 10 へ送る項目:

| 項目                          | Phase 9 での状態                         | Phase 10 での期待実装                                                                              |
| ----------------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------------------- |
| **Strategy からの自動発注**   | 非実装。`order_facade` は gRPC handler 専用 | `StrategyEngine` 有効化、Strategy の `submit_order()` 呼び出しを `ExecEngine` に流す               |
| **Promote to Live API**       | 非実装                                   | `StartLiveStrategy(strategy_id, instrument_id, venue, params)` / `StopLiveStrategy(run_id)`        |
| **Strategy Portability**      | 非対象                                   | `replay_runner.py` と `live_runner.py` の双方が同じ Strategy モジュールをロード可能なエントリ点    |
| **データソース非対称性の吸収** | Phase 8 の `aggregator.py` のみ          | tick → 分足の精度向上、partial bar push、Replay 戦略の Live 動作保証                               |
| **Safety Rails**              | 手動 2 段階確認のみ                      | Position size 上限、注文金額上限、1 戦略 1 Live インスタンス制約                                   |

---

## 9. Open Risks

1. **Tachibana の第二暗証番号失敗回数制限** — 失敗回数上限あり。失敗時は SecretVault から破棄 + UI に「残り試行回数注意」warning。SecretVault 失敗時の retry を 1 回に制限し、それ以上は明示的に再入力を要求する設計とする
2. **kabu 側の Password 不要確認** — sendorder / cancelorder とも `RequestCancelOrder` スキーマに Password フィールドが存在しない（OpenAPI 確認済み）。将来 API バージョンアップで変更があれば対応が必要
3. **kabu の訂正失敗時の整合性** — §2.2 / §2.3 の補償ロジックを E2E test で網羅
4. **EVENT WebSocket の `EC` 取りこぼし** — Tachibana の EVENT WebSocket が disconnect 中に約定が起きた場合、復旧時に `CLMOrderList` で差分取得して reconcile
5. **時刻ずれによる Instruments Daily Refresh の二重実行** — supervisor 配下で複数プロセスが起動する場合は backend singleton (Phase 8 の Named Mutex) で防止
