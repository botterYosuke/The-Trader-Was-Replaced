# Phase 9: Live Account & Order API — Implementation Plan

> **前提**: Phase 8 (Live Venue & Market Data) が完了し、`LiveVenueAdapter` で read-only な市場接続・銘柄メタデータ・depth 購読が動作する状態を出発点とする。Phase 9 では **初めて発注経路を握る** ため、`ExecEngine` のインスタンス化、第二暗証番号 / 取引パスワードの収集 UX、口座状態同期の 3 本柱を導入する。
>
> 上位計画 [Transparent Headless Replay](./archive/Tranceparent%20Headless%20Replay.md) の §Phase 9 「口座情報の同期と注文機能の実装」を具体化する。

---

## Goals

- Live venue (Tachibana / kabuステーション) の **口座残高・保有ポジション・約定履歴** を同期し、既存 `TradingState` reducer 経由で UI に反映する
- LiveManual モードで **手動発注**（新規・取消・訂正）が可能になる。`OrderPanel` (Phase 8 で繰り越し) を新設し、`PlaceOrder` / `CancelOrder` / `ModifyOrder` の 3 RPC を追加
- **第二暗証番号 (Tachibana) / 取引パスワード (kabu)** を発注時のみ収集・メモリ保持・idle forget するワンタイム収集 UX を導入
- Phase 8 で繰り越した運用系項目（kabu 本体早朝ログアウト後の自動回復、Instruments 日次更新、idle gRPC shutdown、backend 自動再起動）を片付け、Phase 10 (Promote to Live) の土台を整える

## Non-Goals

- **アルゴリズム発注 / 戦略からの自動発注は Phase 10**。Phase 9 はあくまで **人間がボタンを押す** 経路のみ。`StartLiveStrategy` RPC は導入しない
- 複数 Venue への同時発注、IFD/OCO 等の特殊注文タイプは Phase 9 のスコープ外
- 信用取引・先物・オプション・夜間 PTS は対象外（現物のみ）
- 税制計算・確定申告レポートは対象外

---

## 0. Feature Inventory / バックエンド機能一覧

### 0.1 発注経路

- `PlaceOrder(venue, instrument_id, side, qty, price, order_type, time_in_force, second_secret?)` — 新規発注
- `CancelOrder(venue, order_id, trade_password?)` — 取消（kabu は取引パスワード必須）
- `ModifyOrder(venue, order_id, new_price?, new_qty?, second_secret?)` — 訂正
  - **kabu には訂正 API が無い** → adapter 内部で「取消 → 新規発注」に変換（atomicity は保証されない旨を UI に表示）
  - Tachibana は `CLMKabuChangeOrder` を直接発射
- `GetOrderStatus(venue, order_id)` — 単発取得（polling 用）
- `StreamOrderEvents(venue)` — 約定・キャンセル・期限切れの push streaming（kabu は WebSocket、Tachibana は EVENT WebSocket / EventDownload）

### 0.2 口座情報

- `GetAccount(venue)` — 預り金・買付余力・評価額・建玉一覧の単発取得
- `StreamAccountEvents(venue)` — 残高変化・ポジション変化の push streaming
- `ListExecutions(venue, from, to)` — 約定履歴（日付範囲指定）

### 0.3 機密情報の都度収集

- `RequestSecret(venue, kind, purpose)` — Python 側が UI に「第二暗証番号を入力してください」モーダルを開かせる RPC（reverse direction: server → UI）
  - 既存の双方向 streaming `EventStream` のイベント種別として実装（新規 server-streaming RPC を追加しない）
  - UI は `SubmitSecret(request_id, secret)` で応答。secret は Rust 側で **保持しない**（即 backend に転送して破棄）
- backend 側で受領した secret は `SecretVault`（メモリのみ）に `request_id` 紐付けで保管し、対応 RPC 完了時または **60 秒 idle で消去**

### 0.4 Watchdog & 運用

- **Venue Health Watchdog** — kabu の `/token` を 30 秒間隔で軽量 ping し `4001001`（本体ログアウト）検出 → modal 経由で再ログイン誘導 → `/token` 再発行 → 購読自動再開
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
└── RiskEngine        (Phase 9 で軽量実装、pre-trade check のみ)
```

`LiveExecutionClient` は Nautilus 標準の `LiveExecutionClient` を venue 別に 1 実装ずつ:

- `TachibanaExecutionClient` — `python/engine/exchanges/tachibana_exec.py` (新設)
- `KabusapiExecutionClient` — `python/engine/exchanges/kabusapi_exec.py` (新設)

### 1.2 State Machines

```
OrderStateMachine (Phase 9 [NEW])
  CREATED → PENDING_SUBMIT → SUBMITTED → ACCEPTED → (PARTIALLY_FILLED) → FILLED
                                                  ↘ CANCELED / REJECTED / EXPIRED
                                                  ↘ PENDING_CANCEL → CANCELED
                                                  ↘ PENDING_MODIFY → ACCEPTED (with new price/qty)
```

Nautilus 標準の `OrderStatus` をそのまま採用。`PENDING_MODIFY` は kabu の「取消 → 新規発注」変換時にもサイクルする。

### 1.3 SecretVault

```
SecretVault (in-memory dict)
  key: request_id (UUID)
  value: { secret: str, requested_at: float, purpose: str, ttl: 60s }

flow:
  1. ExecutionClient が secret を必要とする RPC を発射しようとする
  2. SecretVault に該当 venue + purpose の secret が無ければ
     → EventStream に SecretRequired{request_id, venue, kind, purpose} を push
  3. UI が iced/Bevy modal で入力 → SubmitSecret(request_id, secret) で応答
  4. SecretVault に保管 → ExecutionClient が取り出して RPC 発射
  5. RPC 完了 or 60s idle で削除（whichever first）
```

**Rust 側に secret を 1 ミリ秒も滞留させない**: `SubmitSecret` RPC は UI モーダルの入力フィールド (cosmic-edit) から直接 grpc バイト列を構築し、Rust 側のヒープに `String` として残さない（`zeroize` でモーダル閉時に明示消去）。

---

## 2. Venue 固有の取り扱い

### 2.1 Tachibana 発注

> 詳細は `.claude/skills/tachibana/SKILL.md` の発注セクション参照。

- 新規発注: `CLMKabuNewOrder` — `sSecondPassword` 必須（第二暗証番号、§3.3.1 で都度収集）
- 訂正: `CLMKabuChangeOrder` — 単価 / 数量変更可能、`sOrderNumber` で対象指定
- 取消: `CLMKabuCancelOrder` — 第二暗証番号不要（取消は本人確認済み前提）
- 約定通知: EVENT WebSocket の `EC` (約定通知) / `KP` (株価) / `SS` (システム状態) を購読
- **`p_no` 採番**: 既存の `tachibana_session.json` ベースのカウンタを継続使用。プロセス再起動を跨いでも連番が破綻しないよう atomic write を維持
- `sJsonOfmt`: 通常 REQUEST は `"5"` 固定 (Phase 8 既決)

### 2.2 kabuステーション 発注

> 詳細は `.claude/skills/kabusapi/SKILL.md` の発注セクション参照。

- 新規発注: `POST /sendorder` — `Password` (取引パスワード) 必須、第二暗証番号は kabu には無い
- **訂正 API は存在しない** → adapter 内部で「`PUT /cancelorder` → 結果待ち → 新規 `POST /sendorder`」のトランザクション。失敗時の補償:
  - 取消成功 + 新規失敗 → UI に「訂正失敗。元注文は取消済み。新規発注を再試行してください」モーダル
  - 取消失敗 → UI に「訂正失敗。元注文はそのまま残っています」モーダル
- 取消: `PUT /cancelorder` — `Password` 必須
- 約定通知: PUSH WebSocket は **板情報のみ**、約定通知は無いので `GET /orders` を 1 秒間隔 polling
- **流量制限**: 発注系 5 req/s を `kabusapi_ratelimit.OrderBucket` で事前抑制 (Phase 8 既決)
- エラー対応:

  | コード   | 意味                  | Phase 9 ポリシー                                                                            |
  | -------- | --------------------- | ------------------------------------------------------------------------------------------- |
  | 4001003  | 取引パスワード不一致  | SecretVault から該当 secret を破棄 → 再 `RequestSecret` → 1 回まで自動 retry                |
  | 4002001  | 注文数量上限超過      | UI に明示 (translate to `ORDER_QTY_EXCEEDED`)                                               |
  | 4002005  | 余力不足              | `GetAccount` を再取得して UI に最新の買付余力を表示                                         |
  | 4001005  | トークン失効          | `/token` 再発行後 1 回 retry (Phase 8 既存ロジック)                                         |

### 2.3 訂正発注の atomicity 表示

UI 上で訂正注文を出す際、kabu の場合は **必ず警告バナー** を出す:

> 「kabuステーションには訂正 API がありません。`取消 → 新規発注` の 2 段階で訂正します。途中で失敗した場合は元注文が取消のみ済むことがあります。」

Tachibana の場合は警告不要（`CLMKabuChangeOrder` で atomic）。

---

## 3. Tasks

### 3.1 Backend: ExecEngine 有効化 & 基盤

- `live_runner.py` に `ExecEngine` / `RiskEngine` のインスタンス化を追加。`LiveVenueAdapter` から `LiveExecutionClient` を取り出して `ExecEngine.register_client()`
- Nautilus `OrderFactory` を Strategy 外から呼べる薄い wrapper (`live/order_facade.py`) を追加。Phase 9 は手動発注のみなので Strategy 経由ではなく gRPC handler から直接 facade を叩く
- `live/secret_vault.py` を新設。`asyncio.Lock` で並行アクセス制御、TTL チェックは `asyncio.get_event_loop().call_later(60, ...)`

### 3.2 Backend: 発注 RPC 追加

```proto
service DataEngine {
  // 既存 RPC...

  // Phase 9
  rpc PlaceOrder(PlaceOrderReq) returns (PlaceOrderRes);
  rpc CancelOrder(CancelOrderReq) returns (CancelOrderRes);
  rpc ModifyOrder(ModifyOrderReq) returns (ModifyOrderRes);
  rpc GetOrderStatus(GetOrderStatusReq) returns (Order);
  rpc GetAccount(GetAccountReq) returns (Account);
  rpc ListExecutions(ListExecutionsReq) returns (stream Execution);

  // SecretVault に対する UI 応答
  rpc SubmitSecret(SubmitSecretReq) returns (SubmitSecretRes);
}

// EventStream (既存) に追加されるイベント種別:
//   - SecretRequired{request_id, venue, kind ("second_secret" | "trade_password"), purpose}
//   - OrderEvent{order_id, status, filled_qty, avg_price, ts_ms}
//   - AccountEvent{cash, buying_power, positions, ts_ms}
```

すべての RPC は `ExecutionMode` を server 側で検証し、`Replay` モード時は **構造的に reject** (`EXECUTION_MODE_PRECONDITION`)。

### 3.3 Backend: ExecutionClient 実装

#### 3.3.1 TachibanaExecutionClient

- `submit_order()` の冒頭で SecretVault に第二暗証番号がなければ `SecretRequired` を push → 取得を待つ (max 30s timeout、超過で `SECRET_TIMEOUT`)
- `CLMKabuNewOrder` の組み立てに既存 `tachibana_url_builder.py` (Phase 8) を流用
- EVENT WebSocket からの `EC` を `OrderFilled` / `PositionChanged` イベントに変換し `ExecEngine` に push
- 第二暗証番号は **使用後即削除** (RPC 完了時に SecretVault から pop)

#### 3.3.2 KabusapiExecutionClient

- `submit_order()` の冒頭で SecretVault に取引パスワードがなければ `SecretRequired` を push
- `modify_order()` は内部で `cancel_order()` → wait `OrderCanceled` → `submit_order(new_params)` のシーケンス。失敗時の補償は §2.2 参照
- 約定確認は 1 秒間隔 polling (`GET /orders?id=...`) を `asyncio.Task` で回す
- `4001003` (取引パスワード不一致) を受けたら SecretVault から削除して 1 回再要求 retry

### 3.4 Backend: 口座同期

- `live/account_sync.py` 新設
  - 起動時 + 30 秒間隔で `GetAccount` 相当を venue API に発射 (`kabusapi: GET /wallet/cash` + `GET /positions`、Tachibana: `CLMZanKabuList` + `CLMHenkanInfoZyoutoueki`)
  - 差分があれば `AccountEvent` を EventStream に push
- ポジションは Nautilus `Cache` の `position()` API に流し込み、Snapshot Reducer の既存経路で UI 表示（PositionsPanel は Phase 8 で実装済み、Phase 9 で初めて Live データが流れる）

### 3.5 Backend: Venue Health Watchdog (Phase 8 繰り越し)

- `live/health_watchdog.py` 新設
- 30 秒間隔で kabu の `/token` を軽量 ping (`HEAD /board/{symbol}@1` で代替、新規 token 発行は避ける)
- `4001001` 検出 → EventStream に `VenueLogoutDetected{venue}` push → UI が再ログイン modal を開く → ログイン完了後に既存購読を `kabusapi_register` から再発行
- Tachibana 側は EVENT WebSocket の disconnect で検知 (auto-reconnect は Phase 8 で実装済み、Phase 9 は SS=01 閉局検出ロジックを追加)

### 3.6 Backend: Instruments Daily Refresh (Phase 8 繰り越し)

- `live/instruments_scheduler.py` 新設
- `apscheduler` または素の `asyncio.create_task` + `asyncio.sleep_until_next(5:00 JST)` で営業日判定 → 全銘柄 fetch → `artifacts/instrument-lists/listed-symbols-YYYY-MM-DD.json` を atomic rename で更新
- 取引日カレンダーは J-Quants の `/markets/trading_calendar` を使用

### 3.7 Backend: Idle gRPC Shutdown (Phase 8 繰り越し)

- gRPC interceptor で `last_request_ts` を AtomicF64 に記録
- background task が 5 秒間隔で確認、`now - last_request_ts > 60s` かつ **独立起動モード** (= Bevy supervisor 配下でない) なら:
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
  - 数量（100 株単位、`tick_size` 検証）
  - 価格（成行 / 指値、指値の場合は `tick_size` 整合チェック）
  - 執行条件（寄付 / 引け / 不成 / 当日中）
- **2 段階確認モーダル**: `[発注]` ボタンクリック → モーダルで「銘柄・数量・価格・推定約定額・概算手数料」を再表示 → `[Confirm]` で初めて RPC 発射
- 推定約定額は `qty * price` の単純計算（手数料は venue 別の概算テーブルから引く、誤差は明示）
- レイアウト保存先は `live_manual_layout.json` (Phase 8 で概念定義済み)

### 3.10 UI: SecretRequired モーダル

- `src/ui/secret_modal.rs` を新設（既存 `ModalLayer` 機構を流用）
- EventStream で `SecretRequired{request_id, venue, kind, purpose}` を受信 → モーダル open
- フィールド: cosmic-edit 1 行、`password` モード（マスク表示）
- 入力後 `SubmitSecret` RPC 発射 → cosmic-edit buffer を `zeroize` で破棄、モーダル close
- タイムアウト 25 秒（backend の 30 秒タイムアウトより少し短く設定）でモーダル auto-close + `SECRET_INPUT_CANCELED` をエラートーストに

### 3.11 UI: 訂正発注の警告バナー (kabu のみ)

- `OrderPanel` で対象 venue が kabu かつ `[訂正]` ボタンが押された場合 → モーダル上部に warning バナー表示
- ユーザーが `[理解した上で訂正する]` チェックボックスを ON にして初めて `[Confirm]` が enabled になる

### 3.12 UI: PositionsPanel / OrdersPanel の Live 対応

- 既存パネルは Phase 8 で Snapshot Reducer 経由で表示される設計だが、Phase 8 段階では Live ポジションは常に空だった
- Phase 9 で `AccountEvent` / `OrderEvent` が流れ始める → 既存パネルが自動で Live データを描画
- 追加実装: OrdersPanel に **右クリック → [取消] / [訂正]** コンテキストメニュー（コンテキストメニュー自体は新規実装、`bevy_egui` で簡易実装）

---

## 4. File Layout

```
python/engine/
├── live_runner.py                  # ExecEngine + RiskEngine 有効化
├── live/
│   ├── secret_vault.py     [NEW]   # メモリのみ secret 保管、60s TTL
│   ├── order_facade.py     [NEW]   # OrderFactory wrapper、手動発注 entry point
│   ├── account_sync.py     [NEW]   # 30s 間隔の余力・ポジション同期
│   ├── health_watchdog.py  [NEW]   # 30s 間隔の /token ping、4001001 検出
│   ├── instruments_scheduler.py [NEW]  # 5:00 JST に全銘柄再フェッチ
│   └── aggregator.py               # (Phase 8 で実装、Phase 9 では変更なし)
├── exchanges/
│   ├── tachibana_exec.py   [NEW]   # CLMKabuNewOrder / Change / Cancel + EC 約定通知
│   ├── kabusapi_exec.py    [NEW]   # /sendorder / /cancelorder + GET /orders polling
│   └── (Phase 8 既存ファイル)
└── server_grpc.py                  # PlaceOrder / CancelOrder / ModifyOrder / SubmitSecret / 口座 RPC を追加

src/
├── backend_supervisor.rs           # auto-restart ロジック有効化（crash loop カウンタは Phase 8 既存）
└── ui/
    ├── order_panel.rs      [NEW]   # LiveManual 専用、手動発注 UI（Phase 8 から繰り越し）
    ├── secret_modal.rs     [NEW]   # 第二暗証番号 / 取引パスワード入力モーダル
    └── (Phase 8 既存ファイル)
```

---

## 5. Implementation Order

各ステップ完了時点で `cargo run` できる状態を維持する。Phase 8 と同じく、本番 venue に接続しなくても **MockVenueAdapter の発注経路** で UI → backend の往復をテストできるよう Step 1 で mock を先に拡張する。

1. **Step 1 — MockVenueAdapter の発注経路 + SecretVault**
   - `MockVenueAdapter.submit_order()` を追加（成功・失敗・部分約定の各パターンを返せる）
   - `live/secret_vault.py` を実装し SecretVault unit test
   - `SubmitSecret` RPC + `SecretRequired` イベントの protobuf 追加
2. **Step 2 — ExecEngine 有効化 + OrderEvent stream**
   - `live_runner.py` に `ExecEngine` / `RiskEngine` インスタンス化を追加
   - `PlaceOrder` / `CancelOrder` / `GetOrderStatus` RPC を実装（mock 経由で疎通確認）
   - EventStream に `OrderEvent` を push する経路を実装
3. **Step 3 — OrderPanel UI + SecretModal UI**
   - `src/ui/order_panel.rs` 新設、2 段階確認モーダル含む
   - `src/ui/secret_modal.rs` 新設、`zeroize` 連携
   - Mock 経由で「発注 → 第二暗証番号入力 → 約定通知 → OrdersPanel に表示」が通る
4. **Step 4 — Account 同期 + PositionsPanel/OrdersPanel Live 対応**
   - `live/account_sync.py` 実装、`AccountEvent` push
   - 既存パネルへの Live データ流入確認
   - OrdersPanel の右クリックコンテキストメニュー (取消 / 訂正) 追加
5. **Step 5 — TachibanaExecutionClient**
   - `CLMKabuNewOrder` / `CLMKabuChangeOrder` / `CLMKabuCancelOrder` 実装
   - 第二暗証番号都度収集の E2E (Demo 環境)
6. **Step 6 — KabusapiExecutionClient**
   - `POST /sendorder` / `PUT /cancelorder` 実装
   - **訂正は「取消 → 新規発注」変換** + 補償ロジック + UI 警告バナー
   - 取引パスワード都度収集の E2E (Verify 環境)
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
    - secrets masking ログフィルタの再検証 (第二暗証番号 / 取引パスワードがログに出ないこと)
    - drawio アーキ図 `phase9-architecture.drawio.svg`
    - Phase 10 (Promote to Live) への引き継ぎ事項を docs にまとめる

---

## 6. Success Criteria

### 発注経路

- LiveManual モードで OrderPanel から `1 株 / 成行 / 当日中` の手動発注ができ、約定後に PositionsPanel / OrdersPanel に反映される (両 venue の Demo / Verify 環境で E2E)
- 取消・訂正が両 venue で動作する。kabu の訂正は「取消 → 新規」の 2 段階であることが UI 警告バナーで明示される
- 第二暗証番号 / 取引パスワードが **使用後 60 秒以内にメモリから消える** (debug log で確認、または `gc.get_objects()` 走査の unit test)
- 第二暗証番号 / 取引パスワードが **Rust 側のヒープに一切残らない** (cosmic-edit buffer の `zeroize` 動作確認、`process memory dump` で平文検索)
- `Replay` モードで `PlaceOrder` RPC を発射すると `EXECUTION_MODE_PRECONDITION` で reject される (unit test)

### 口座同期

- 起動時に kabu / Tachibana の買付余力・保有ポジションが正しく取得され BuyingPowerPanel / PositionsPanel に表示される
- 約定後 30 秒以内に PositionsPanel が更新される
- 余力不足 (`4002005` / Tachibana 相当) で発注した際、`GetAccount` 再取得後に最新余力が UI に出る

### 運用系 (Phase 8 繰り越し)

- kabu 本体を手動ログアウト → 30 秒以内に Venue Health Watchdog が検知 → 再ログイン modal → ログイン完了で購読自動再開 (E2E)
- 営業日 5:00 JST に Instruments parquet が atomic 更新される (時刻 mock test)
- 独立起動 backend が 60 秒 idle で `unregister/all` 発射後に自己 shutdown する (unit test + manual)
- Bevy supervisor 配下では idle shutdown が無効化される (環境変数判定 unit test)
- backend を `taskkill /F` で殺すと 60 秒以内 3 回未満なら自動再起動、3 回以上で `[Restart Backend]` disabled に格下げ (Phase 8 のクラッシュループカウンタ流用)

### セキュリティ

- 全文 grep でログ・コアダンプ・session ファイルに第二暗証番号・取引パスワードが平文で出現しない
- `SecretVault` を `pickle.dumps()` した結果に平文が含まれない（メモリスナップショット採取テスト）

---

## 7. Open Questions & ADRs

### ADR: Phase 9 で初めて ExecEngine をインスタンス化する

Phase 8 では `DataEngine` のみホストして発注経路を構造的に遮断していた。Phase 9 で `ExecEngine` を有効化する際:

1. **Venue 別に 1 `LiveExecutionClient` を attach** — Nautilus 標準の構造に従う
2. **`StrategyEngine` は依然として無効** — 戦略からの自動発注は Phase 10。Phase 9 の発注は gRPC handler から `order_facade` 経由で直接 `ExecEngine.submit_order()` を叩く
3. **読み取り専用モード fallback は持たない** — `ExecutionMode == Replay` のとき backend は ExecEngine の `submit_order` を呼ばない RPC 層で reject する。ExecEngine 自体は LiveManual / LiveAuto モードでは常時稼働

### ADR: 第二暗証番号 / 取引パスワードは発注時都度収集・メモリのみ保持・60s idle で破棄

Phase 8 ADR を継承。Phase 9 で UI 実装が入る:

1. **ファイル / keyring に書かない** — 漏洩窓を最小化
2. **Rust 側に滞留させない** — cosmic-edit buffer から直接 gRPC バイト列、応答後 `zeroize`
3. **Python 側 SecretVault に保管、TTL 60s** — 連続発注時の入力負担を緩和するための最小限の cache
4. **連続発注で TTL がリセットされない** — purpose 別に独立 TTL (連射時に時計がずるずる延びるのを避ける)

### ADR: kabu の訂正は「取消 → 新規発注」変換を adapter 層で行い、atomicity 非保証を UI で明示する

kabu API には訂正エンドポイントが無い。選択肢:

- **A. 訂正機能を提供しない** — UX が悪い (取消 + 新規を 2 操作で要求)
- **B. adapter 層で取消 + 新規を変換、atomicity 非保証を UI で明示** ← **採用**
- **C. adapter 層で同上、atomicity を 2PC 風に保証** — kabu API では実現不可能

採用理由: ユーザーの操作回数を減らす UX 価値が、atomicity 喪失のリスクより大きい。リスクは UI 警告バナー + チェックボックスで explicit consent を取ることで mitigation。

### ADR: SecretRequired は新規 RPC ではなく既存 EventStream の追加イベント

Server → UI の reverse-direction 通信が必要。選択肢:

- **A. 新規 server-streaming RPC `SubscribeSecretRequests`** — 別チャンネル管理が増える
- **B. 既存 `EventStream` のイベント種別として追加** ← **採用**

採用理由: イベント駆動 UI の既存パターンに乗る方が一貫性が高く、UI 側のイベントハンドラに 1 アームを足すだけで済む。

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

1. **kabu の取引パスワード誤入力ロック** — 3 回連続失敗で本体側がロックされる可能性。SecretVault 失敗時の retry を 1 回に制限し、それ以上は明示的に再入力を要求する設計とする
2. **Tachibana の第二暗証番号失敗回数制限** — 同上。失敗時は SecretVault から破棄 + UI に「残り試行回数注意」warning
3. **kabu の訂正失敗時の整合性** — §2.2 / §2.3 の補償ロジックを E2E test で網羅
4. **EVENT WebSocket の `EC` 取りこぼし** — Tachibana の EVENT WebSocket が disconnect 中に約定が起きた場合、復旧時に `CLMOrderList` で差分取得して reconcile
5. **時刻ずれによる Instruments Daily Refresh の二重実行** — supervisor 配下で複数プロセスが起動する場合は backend singleton (Phase 8 の Named Mutex) で防止
