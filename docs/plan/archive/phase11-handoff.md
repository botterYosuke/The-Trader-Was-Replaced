# Phase 11 への引き継ぎ事項

Phase 10（[Replay-to-Live Strategy Execution](./plan/Phase%2010%20-%20Replay%20to%20Live%20Strategy%20Execution.md)）
完了時点で、意図的に Phase 11 以降に送った機能と、実装中に判明した既知の限界をまとめる。
各項目は Phase 10 の計画 §7 / ADR / 各 Step 完了サマリーが出典。

---

## 1. 計画的に Phase 11 送りにした機能（Non-Goals）

| 項目 | Phase 10 での状態 | Phase 11 候補 |
| --- | --- | --- |
| **複数 automated Live run 同時実行** | 自動戦略 run は同時 **1 件** に制約（`RunRegistry.max_active_live_auto_runs=1`）。手動 `MANUAL-001` との同居は可 | `instance_id` 導入で複数 run / 同戦略複数銘柄。RunRegistry に `(strategy_id, instrument_id)` 索引は実装済み（拡張余地あり） |
| **戦略の hot-reload** | 明示停止 → 編集 → 再起動が必須 | safe reload（現 position を引き継いで新インスタンスへ移行） |
| **戦略パラメータ最適化** | 非対象 | Grid search / Optuna 統合 |
| **戦略パフォーマンスダッシュボード** | 生イベント表示のみ（PnL/件数の telemetry まで） | KPI 集計 / Sharpe / Calmar 自動計算 |
| **Live Strategy の永続化・自動復旧** | crash 再起動時は **停止状態**（人間判断）。venue 側に注文が残り得る | `app_state` 経由の復元 |
| **専用ログビューア** | Live Run Panel に直近ログ（`StrategyLogMessage`）のみ | フィルタ付きログビューアパネル |
| **戦略のバージョン管理** | `strategy_id = strat-{sha256[:16]}`（内容ハッシュ）まで | git 連携 / バージョン履歴 |
| **複数 Venue 同時接続** | 非対象 | venue 別 `Trader` / `TradingNode` |
| **戦略の depth 参照宣言** (`REQUIRES_DEPTH`) | **未実装**（計画 §0.5 のみ）。Replay は depth 空・Live は venue 依存 | クラス `ClassVar` 宣言 → Live で depth subscription 自動有効化、Replay で warning |

---

## 2. 実装中に判明した既知の限界（要 Phase 11 で対処 or 検証）

### 2.1 bar 供給の精度（venue 非対称、Step 8 / §8 Open Risk 5）
- **kabu は約定 tick feed が無い**。PUSH は板情報のみ、約定は 1 秒 polling。よって戦略への `Bar` 供給は
  板 `CurrentPrice` / polling 約定からの構成となり、Tachibana（EVENT WS `EC`）より精度が落ちる。
  「直近約定を見て次の判断」をする戦略は最大 1〜2 秒遅延し得る。
  → Phase 11 で push 化を venue にリクエストするか、kabu 戦略に polling 前提の設計指針を出す。
- Replay（catalog の確定 `Bar`、EXTERNAL）と Live（aggregation 由来 `Bar`、INTERNAL）は
  tick の集約方式・タイムスタンプ境界で OHLCV が ±1 tick ずれ得る。Step 1/8 で回帰テスト済みだが、
  実 venue tick 列での検証は Step 9（未実施）。

### 2.2 telemetry の unrealized P&L（Step 7/8）
- `LiveStrategyTelemetry.unrealized_pnl` は建玉の mark-to-market に市場価格が要る。Step 8 で価格供給が
  入ったので意味を持つようになったが、account snapshot 連動の再計算は order event 都度更新に留まる。
  → Phase 11 で account snapshot 連動の unrealized 再計算を追加するのが自然。

### 2.3 注文の二重報告（Step 7、要実 venue 確認）
- auto 戦略の約定は kernel msgbus bridge 経由で `LIVE-{run}` タグ付きで届くが、実 venue では同一約定が
  共有 adapter の EC stream（strategy_id 空）でも届き得る。UI は client_order_id でマージし「非空が勝つ」
  規則で `LIVE-{run}` を保持する設計だが、**mock では EC stream が発火しない**ため Phase 10 のテスト範囲では
  二重化を実検証できていない。→ Step 9 の実 venue E2E で要確認。

### 2.4 同一プロセスのログ汚染（Step 4、無害）
- 同一プロセスで live kernel（非 bypass logging）を先に初期化すると、後続の backtest
  （`bypass_logging=True`）の dispose ログ（`InvalidStateTrigger RUNNING->DISPOSE`）が console に漏れる
  （Nautilus の global logger が once 初期化のため）。**production は replay/live が別プロセスなので無影響**、
  test の console ノイズのみ。

---

## 3. Phase 10 で未実施 → Phase 11（または手動）で要対応

| 項目 | 内容 |
| --- | --- |
| **Step 9: Live E2E（Demo / Verify）** | Tachibana Demo + サンプル戦略を 1 営業日 Live 稼働 / kabu Verify でも E2E（約定 tick 無し前提の精度限界を確認）。**実 venue 認証情報と市場稼働時間が必要**で自動化不可 |
| **GUI mock E2E** | Promote → Safety Rails モーダル → bar → on_bar → 発注 → OrdersPanel フィルタ / Live Run Panel telemetry / **Safety Rail 違反トースト（Footer 右下）/ Strategy ログ tail（`emit_strategy_log`→msgbus→`StrategyLogMessage`）** を実アプリで目視（Step 5/6/7 + Step 9 remediation で配線済み、目視 verify が未実施）。ログ tail を出すサンプル戦略は `emit_strategy_log` を呼ぶ必要がある（素の `self.log.*` は UI 非中継） |
| **clippy / toolchain cleanup** | Rust `-D warnings` は workspace 全体で pre-existing 警告（rust-1.93 toolchain 由来、~30 ファイル）により失敗状態。Phase 10 の delta はクリーン。別途 toolchain cleanup タスクとして切り出す |

---

## 4. Phase 10 で確立した設計資産（Phase 11 が前提にできるもの）

- **移植性レイヤ**: `strategy_loader.load()` は環境非依存（クラスを返すだけ）。Replay/Live 双方が同じロードを使う。
  EXTERNAL→INTERNAL bar_type 読み替えは `bar_supply.py` に集約。
- **Live host stack**: `NautilusKernel`（直接構築、`TradingNode` 不要）+ `NautilusVenueExecClient`
  （`OrderingVenueAdapter` bridge）+ `NautilusVenueDataClient`（tick → INTERNAL aggregation）。
- **発注主体識別**: 各 run = 一意 `StrategyId`（`LIVE-{run}`、手動は `MANUAL-001`）。
  `change_id` + `change_order_id_tag` で強制（Cython Order に新フィールドを足さない）。
- **single channel transport**: `SubscribeBackendEvents` の `BackendEvent.oneof` のみ
  （`LiveStrategyEvent` / `SafetyRailViolation` / `StrategyLogMessage` / `LiveStrategyTelemetry`
  / `OrderEvent.strategy_id`）。新 stream を増やさない原則。
- **Safety Rails 二段構え**: ネイティブ `LiveRiskEngineConfig` + 独自薄層 `safety_rails.py`。backend 強制で bypass 不可。
- 詳細は [`live-strategy.md`](./live-strategy.md) と [`assets/phase10-architecture.drawio.svg`](./assets/phase10-architecture.drawio.svg)。
