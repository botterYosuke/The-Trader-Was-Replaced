# Phase 9 → Phase 10 引き継ぎ事項

> Phase 9 (Live Account & Order API) 完了時点 (2026-05-21) で **意図的に Phase 10 へ送った項目**と、
> 各 Step で記録した forward-compat / 残課題を 1 箇所に集約する。詳細な根拠は各 Step 完了サマリー
> ([Phase 9 plan](./Phase%209%20-%20Live%20Account%20and%20Order%20API.md)) を参照。Phase 10 着手時は
> このファイルと [Phase 10 plan](./Phase%2010%20-%20Replay%20to%20Live%20Strategy%20Execution.md) を突き合わせること。

## 1. Phase 10 本丸（Phase 9 で構造的に遮断した機能）

| 項目 | Phase 9 での状態 | Phase 10 での期待実装 |
| --- | --- | --- |
| **Strategy からの自動発注** | 非実装。`order_facade` は gRPC handler 専用（人間がボタンを押す経路のみ） | `StrategyEngine` 有効化、Strategy の `submit_order()` 呼び出しを `ExecEngine` に流す |
| **Promote to Live API** | 非実装（`StartLiveStrategy` RPC は導入しない） | `StartLiveStrategy(strategy_id, instrument_id, venue, params)` / `StopLiveStrategy(run_id)` |
| **Strategy Portability** | 非対象 | `replay_runner.py` と `live_runner.py` の双方が同じ Strategy モジュールをロード可能なエントリ点 |
| **データソース非対称性の吸収** | Phase 8 の `aggregator.py` のみ | tick→分足の精度向上、partial bar push、Replay 戦略の Live 動作保証 |
| **Safety Rails** | 手動 2 段階確認モーダルのみ | Position size 上限、注文金額上限、1 戦略 1 Live インスタンス制約 |

## 2. ExecEngine wiring の移行順序（ADR §7）

Phase 9 Step 2 は **軽量 `ManualOrderFacade`（選択肢 B）** を採用し、真正 Nautilus `ExecEngine` /
`RiskEngine` / `MessageBus` / `Cache` / `Portfolio` / `LiveExecutionClient` の wiring は延期した。
現状の live パイプラインは `LiveRunner → LiveEventBus → LiveReducerBridge` の bespoke 構成。

推奨移行順序: **thin facade（Phase 9）→ `LiveExecutionClient` adapter 化 → full live engine（Phase 10 / LiveAuto）**。
Phase 10 で `ExecEngine.execute(SubmitOrder コマンド)` 経由に切り替える（`ExecutionEngine` に `submit_order()`
は無く `execute(TradingCommand)` がエントリ点）。`OrderFactory` は `trader_id` + `strategy_id` 必須。

## 3. 発注 / 約定経路の forward-compat（Step 5/6）

- **`second_secret` の単一チャネル化（済・維持）**: 実 secret 経路は **SecretVault**（`SecretRequired` push →
  `SubmitSecret` RPC、TTL 60s reuse）に一本化済み。proto `PlaceOrderReq/CancelOrderReq/ModifyOrderReq` の
  `second_secret` field は **facade で永久 inert**（adapter に渡さない）。Phase 10 で algo 発注を足す際も
  この終端を崩さないこと（誤って復活させると二重チャネル + 平文漏洩面が再発する）。
- **kabu 訂正の in-place remap（Tachibana atomic と非対称）**: kabu は訂正 API が無く「取消→残数量のみ新規」
  変換。同一 `client_order_id` に新 `OrderId` を再マップし、`filled_base`/`notional_base` で累計約定を telescoping
  保持する。§1.2 の「別 client_order_id で新規」要件はこの in-place + 残数量再発注で代替済み。Phase 10 で algo
  発注を入れるなら `new_client_order_id` proto field 追加を再検討（現状は人間の 1 注文ずつ UX 前提）。
- **`unrealized_pnl` の venue 非対称**: Tachibana `CLMGenbutuKabuList` は取得簿価ベースで live 評価損益を持たない
  → equity 導出が cost-basis に化ける。proto `optional double` 化 + 価格 feed join or absent 報告を Phase 10 で確定。
- **kabu AccountType（一般2/特定4/法人12）は MVP 既定 = 特定(4) 定数**。kabu login 応答に口座種別が無いため。
  一般/法人運用は `venue_params` 経路（未実装）で上書きする設計余地。

## 4. 運用系の残課題（Step 7/8/9）

- **§3.8 venue-truth GetOrders（未実装・MVP は facade in-memory）**: 現 `GetOrders` は backend の `ManualOrderFacade`
  in-memory store（再起動で空）を返す。「再起動で状態不明」AC は満たすが、kabu `GET /orders` / Tachibana
  `CLMOrderList` で **venue 実注文**を引く richer reconcile（再ログイン後の自動突合）は Phase 10 で。
- **[Restart Backend] ボタン UI 本体 + auto-restart トースト（未実装）**: §3.8 で `SupervisorCommand::Restart` を
  no-op stub から機能化したが、それを送る **UI ボタンは未配線**（lifecycle 遷移 Crashed→Spawning→Ready は footer に
  出る）。専用トースト「Backend を再起動しました」も同様に未実装。
- **§3.6 J-Quants `/markets/trading_calendar` クライアント（未実装）**: instruments 日次更新スケジューラは営業日
  カレンダーを持たず、非営業日/閉局は venue の `fetch_instruments` エラー/空に委ねている。trading_calendar 連携は
  `InstrumentsScheduler.next_delay_s` / business-day gate の差し替えで将来足せる形にしてある。
- **instruments parquet store の consumer**: 現状 `_list_instruments_live`（store-first read）のみ。

## 5. ⚠️ 実 Demo 検証が残る項目（§5.1 layer-3、CI 不可・人手 env 1 度設定）

Phase 9 は CI/mock の layer-1/2（決定論・人間 0）で網羅したが、実 venue を叩く以下は **Demo / Verify 環境で
まとめて検証**する（credential / 実発注を伴うため CI から除外）:

- **Tachibana EVENT WS の URL 構成 / comma エンコード**: `build_event_url` は `,`→`%2C` するが、e-station 参照実装は
  「サーバが `%2C` を認識しない」として **raw comma** を送る。EC（約定通知）URL と Phase 8 の FD 購読 URL
  （`p_evt_cmd=ST,KP,FD`）の両方で実フレーム受信を確認する（TENTATIVE）。
- **口座レベル EC 購読 URL**: EC は口座スコープだが本実装は issue 非依存の専用接続。issue/行番号パラメータ無しの
  接続が有効かは未検証。
- **Tachibana SS（システムステータス）フレームの prefix**: 閉局検知の `sSystemStatus`/`sLoginKyokaKubun` の prefix
  （`s*` か `p_*` 変種か）は実 Demo 未確認。判別フィールド欠落時は安全側（通知しない）に倒してある（TENTATIVE）。
- **発注/取消/訂正の実 API 疎通**: Tachibana（1株/成行/当日中 → EC → 取消）、kabu（X-API-KEY のみで発注/取消完結）。
- **§3.8 実 taskkill /F → 自動再起動 + reconcile モーダル**の E2E。

## 6. セキュリティ不変条件（Phase 10 でも維持）

- 第二暗証番号は **Tachibana 専用**・SecretVault メモリのみ・**保管から 60s TTL で確実に削除**（再利用でリセットしない）。
- 平文を repo / ログ / session ファイル / スナップショットに残さない。`mask_secrets` は Phase 9 で wire field
  `second_secret`（"password" token を持たず従来漏れていた）も伏字化するよう拡張済み。Rust は `RedactedSecret`
  （`Zeroizing` + Debug 伏字 `RedactedSecret(***)`）で command 経由の平文ログを防ぐ。
- kabu は sendorder / cancelorder とも Password 不要（X-API-KEY のみ）。SecretVault は使わない。
