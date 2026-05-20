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
| 3 | OrderPanel UI + SecretModal UI | ⬜ 未着手 |
| 4 | Account 同期 + Panel Live 対応 | ⬜ 未着手 |
| 5 | TachibanaExecutionClient | ⬜ 未着手 |
| 6 | KabusapiExecutionClient | ⬜ 未着手 |
| 7 | Venue Health Watchdog | ⬜ 未着手 |
| 8 | Backend Auto-Restart + Idle Shutdown | ⬜ 未着手 |
| 9 | Instruments Daily Refresh | ⬜ 未着手 |
| 10 | Polish | ⬜ 未着手 |

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
- **Step 3 への申し送り**: ① RPC は実装済みだが Rust UI（OrderPanel / SecretModal）からの発射経路は未配線（`TransportCommand` に `PlaceOrder`/`CancelOrder` variant 追加が必要、id=2434 メモ: 非 Copy payload で `#[derive(Clone)]` のみに）。② OrderEvent push は `backend_event_drain_system` まで届くがログ出力のみ（Snapshot Reducer 結線は §3.12 / Step 4）。③ `Replay` reject は `EXECUTION_MODE_PRECONDITION` の structured error（grpc abort ではない）。④ 2 段階確認モーダル・kabu 訂正警告バナーは Step 3 / Step 6。
- **次**: Step 3（OrderPanel UI + SecretModal UI）

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
   - `CLMKabuNewOrder` / `CLMKabuCorrectOrder` / `CLMKabuCancelOrder` 実装
   - 第二暗証番号都度収集の E2E (Demo 環境)
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

---

## 6. Success Criteria

### 発注経路

- LiveManual モードで OrderPanel から `1 株 / 成行 / 当日中` の手動発注ができ、約定後に PositionsPanel / OrdersPanel に反映される (両 venue の Demo / Verify 環境で E2E)
- 取消・訂正が両 venue で動作する。kabu の訂正は「取消 → 新規」の 2 段階であることが UI 警告バナーで明示される
- **Tachibana** 第二暗証番号が **SecretVault 保管から 60 秒以内にメモリから消える**（連続発注で再利用しても TTL リセットなし。debug log で確認、または `gc.get_objects()` 走査の unit test）
- **Tachibana** 第二暗証番号が **明示保持されない** ことを確認: (a) cosmic-edit buffer の `zeroize` 動作確認、(b) ログ・session ファイル・状態 resource に平文が出現しないこと（`process memory dump` での完全消去は tonic/prost のデシリアライズ一時文字列を対象外とする現実的な目標に変更）
- **Tachibana** 取消 (`CLMKabuCancelOrder`) でも第二暗証番号収集が正しく動作することをテストで確認（旧仕様「取消は不要」は廃止）
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
