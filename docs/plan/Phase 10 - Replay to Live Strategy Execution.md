# Phase 10: Replay-to-Live Strategy Execution — Implementation Plan

> **前提**: Phase 9 (Live Account & Order API) が完了し、手動発注経路・口座同期・SecretVault が動作する状態を出発点とする。Phase 10 では **戦略コードからの自動発注** を初めて有効化し、Replay で検証した `Strategy` をファイル編集なしに Live 環境にプロモートする経路を完成させる。
>
> 上位計画 [Transparent Headless Replay](./archive/Tranceparent%20Headless%20Replay.md) の §Phase 10 「Replay-to-Live Strategy Execution」を具体化する。Phase 8 ADR「Replay と Live Auto のデータソース非対称性（Phase 10 への前提制約）」および Phase 9 §8 「Phase 10 への引き継ぎ事項」を引き継ぐ。

---

## 0′. グラウンドトゥルース訂正（レビュー反映、2026-05-20）

> Phase 9 Step 0 と同種の「計画書ドリフト」を着手前に潰すための注記。Nautilus ソースミラー (`.claude/skills/nautilus-trader/src/`) と実際の `python/engine/` ツリーで確認した事実に計画全体を合わせた。

1. **Nautilus に `StrategyEngine` は存在しない。** 戦略を管理するのは `Trader`（`nautilus_trader/trading/trader.py`、backtest/live 共通）であり、Live のホストは `TradingNode`（`nautilus_trader/live/node.py`、内部に `NautilusKernel` + `Trader` + `LiveDataEngine` + `LiveExecutionEngine` + `LiveRiskEngine` を持つ）。戦略は `add_strategy()` で attach する（実際 `strategy_runtime/engine_runner.py:150` が backtest でこれをやっている）。本計画の「StrategyEngine を有効化」はすべて「**`Trader` に Strategy を attach する**」と読み替える。
2. **環境依存の注入は `register()` 時にエンジンが行う。** `self.clock` / `self.cache` / `self.msgbus` は Strategy を `Trader` に登録した瞬間にエンジンが注入する（`common/actor.pyx:762-763`、登録前は `None`）。`StrategyConfig` が運ぶのは **venue / instrument_id / params** のみで、clock や data engine は config 経由では渡らない。Replay と Live で戦略が無分岐になるのは「Backtest エンジンか Live エンジンか」をエンジン側が決め、戦略は常に `self.clock` / `self.subscribe_bars()` を使うため——これは Nautilus の標準設計そのもの。
3. **Replay は既に本物の Nautilus `Bar` を流している。** Replay は `BacktestEngine` を streaming で回し、`on_bar` には catalog 由来の Nautilus `Bar` が届く（`engine_runner.py`）。一方 `live/aggregator.py` は **プロジェクト独自の `TickBarAggregator`**（`KlineUpdate` dataclass を生成して UI 用 `LiveEventBus` に流す）であって **Nautilus `BarBuilder` のラッパではない**し、エンジンには繋がっていない。したがって Phase 10 の本質的な課題は「Replay を BarBuilder 経由に変える」ことではなく、**Live 側に Nautilus エンジン（`Trader` + `LiveDataEngine` + Nautilus aggregation）を立て、live tick を Nautilus `Bar` に集約して Strategy の `on_bar` に届ける**ことである（§0.5 / §1.1 / §2.3 で具体化）。
4. **ファイルパス実体（§3 と一致させる）**: `strategy_runtime/strategy_loader.py`（`strategy_loader.py` 直下ではない）/ `strategy_runtime/engine_runner.py`（`replay_runner.py` は存在しない）/ `live/live_runner.py`（Phase 8 の adapter パイプライン `LiveRunner` クラス。Phase 9/10 の「ExecEngine を持つ live ホスト」とは別物・名前衝突に注意）。`live/risk_engine.py` は未存在（Phase 10 で作るとしても Nautilus 標準 RiskEngine の薄い hook 層に留める）。サンプル戦略は `mean_reversion_01` / `FakeBuyAndHold`（`ma_cross.py` は存在しない）。
5. **Phase 9 の実体はまだ Nautilus Live Engine 群ではない。** Phase 9 Step 2 は `ManualOrderFacade` が `OrderingVenueAdapter.submit_order/cancel_order/modify_order` を直接呼ぶ軽量 facade であり、`LiveExecutionEngine` / `LiveRiskEngine` / `Trader.add_strategy()` の本格 wiring は **Phase 10 で初めて導入する**（Phase 9 ADR §7）。したがって Phase 10 の `LiveStrategyHost` は、既存の venue login / SecretVault / `LiveRunner` / 実 venue `ExecutionClient` を二重起動せず、**アクティブな live session を単一所有者として共有または明示的に引き継ぐ**設計にする。別 login / 別 WebSocket / 別 order client を勝手に立てる案は、二重購読・二重発注・SecretVault bypass のリスクがあるため禁止。

---

## Goals

- **Strategy Portability**: Replay と Live Auto で **同一の `Strategy` サブクラス** を共有。環境依存の注入 (時刻ソース / データソース / Venue ID) は外部から行い、戦略本体は環境非依存に保つ
- **Promote to Live**: Strategy Editor で編集中の `.py` ファイルを `[Promote to Live]` ボタンで Live Auto モードにデプロイし、`StartLiveStrategy` RPC で起動できる
- **Safety Rails**: 未認証 / モード不一致 / 余力不足 / 注文金額超過 / 同一戦略の二重起動 を **構造的に防止**
- **Live Run Telemetry**: Live 実行中の fills / account-level position を既存 reducer 経由で UI に流し、run 別 PnL / position / log は `LiveRunPanel` の telemetry として表示する

## Non-Goals

- **複数の自動戦略 Live run の同時実行** — Phase 10 では「自動戦略 run は同時に 1 つ」に制約。複数戦略並列・同一戦略の複数銘柄並列は Phase 11
- **戦略の hot-reload** — Live 稼働中に `.py` を編集した場合、明示停止→再起動が必須。差分適用は対象外
- **Replay 中の戦略パラメータ最適化 (grid search / Optuna)** — Phase 11 以降
- **戦略パフォーマンスダッシュボード (KPI 集計 / シャープレシオ等)** — Phase 11 以降。Phase 10 では生イベント表示のみ

---

## 0. Feature Inventory

### 0.1 Strategy Loading

- `LoadStrategy(file_path, mode)` という **gRPC は現行 proto には存在しない**。Phase 7 の「戦略ロード」は Bevy UI 内の `StrategyFileLoadRequested` / `StrategyBuffer` と、Replay 起動時の `StartEngine.config.strategy_file` で実現されている。Phase 10 ではこの前提を曖昧にせず、Live 起動前に backend でパス検証済みハンドルを作る `RegisterLiveStrategy(strategy_file)` を新設する（§2.5）
  - 実体は `strategy_runtime/strategy_loader.load(path)`（`(module, scenario, strategy_cls)` を返す）。インスタンス化はランナー層の責務
  - `RegisterLiveStrategy` は canonical path が許可ディレクトリ配下かを検証し、`strategy_id` / `strategy_sha256` / `scenario` を返す。`StartLiveStrategy` は raw path を受け取らず、この `strategy_id` だけを受け取る（M9）
- `live_auto` モードでは `LiveStrategyHost` (新設) が Live 用 Nautilus エンジン（`Trader` + `LiveDataEngine` + `LiveExecutionEngine` + `LiveRiskEngine`、`TradingNode` 相当）に `add_strategy()` で Strategy を attach する（Nautilus に `StrategyEngine` は無い、§0′-1）
- 同じ `.py` モジュールが Replay ランナー（`strategy_runtime/engine_runner.py`、`BacktestEngine`）と Live ホスト（`LiveStrategyHost`、§2.2）の両方から `strategy_loader.load()` でロード可能であることを保証

### 0.2 Promote to Live フロー

- Strategy Editor (Phase 7.2) に `[Promote to Live]` ボタンを追加
- クリック時の前提条件チェック:
  1. Venue ログイン済み (`VenueState == CONNECTED` または `SUBSCRIBED`)
  2. ExecutionMode が `LiveAuto`、または `SetExecutionMode(LiveAuto)` が成功する状態
  3. 戦略ファイルがディスク保存済み (unsaved changes が無い)
  4. Safety Rails の事前検証 (position size 上限 / 注文金額上限が設定済み)
- すべて満たすと **2 段階確認モーダル** → Replay 結果サマリー (バックテスト KPI) を表示 → `[Confirm]` で `RegisterLiveStrategy` → 必要なら `SetExecutionMode(LiveAuto)` → `StartLiveStrategy` RPC を順に発射

### 0.3 Live Strategy Control

- `RegisterLiveStrategy(strategy_file)` — backend 側で戦略ファイルを検証し、Live 起動用 `strategy_id` を発行
- `StartLiveStrategy(strategy_id, instrument_id, venue, params, safety_limits)` — 検証済み戦略ハンドルから Live 戦略起動
- `StopLiveStrategy(run_id)` — graceful 停止 (在庫ポジションは残す、ユーザー判断で別途決済)
- `PauseLiveStrategy(run_id)` — run を paused にし、新規発注を backend で deny (market data callback 自体は継続し得る。既存注文は維持)
- `ResumeLiveStrategy(run_id)` — 再開
- `GetLiveStrategyStatus(run_id)` — 状態取得 (RUNNING / PAUSED / STOPPED / ERROR)

### 0.4 Strategy Portability Layer

- `python/engine/strategy_runtime/strategy_loader.py` は既に Replay 用のロード（`load() -> (module, scenario, strategy_cls)`）を持つ。Phase 10 では **ローダ自体は環境非依存のまま**にし、環境依存はランナー/ホスト層（`engine_runner.py` / `LiveStrategyHost`）が選んだエンジンに委ねる
- 環境依存の注入ポイント（**いずれもエンジンが提供し、Strategy は無分岐**）:
  - `Clock` — Replay は `TestClock`、Live は `LiveClock`（`common/component.pyx`）。Strategy には `register()` 時に注入され、戦略コードは常に `self.clock.utc_now()` を使う
  - データ供給 — Replay は `BacktestEngine` 内蔵の `DataEngine`（`BacktestDataEngine` というクラスは存在しない）、Live は `LiveDataEngine`。Strategy は常に `self.subscribe_bars(...)` / `on_bar(Bar)` を使う
  - `Venue` — Replay は scenario 由来（例 `TSE`）、Live は `TACHIBANA` / `KABU`。Strategy は config の `InstrumentId` から取得
  - `Instrument` registry — Replay は J-Quants 既製品 / catalog、Live は venue から取得した最新を `cache.add_instrument()`
- **環境非依存の根拠（§0′-2 を参照）**: `self.clock` / `self.cache` / `self.msgbus` は `Trader` への登録時にエンジンが注入する（`common/actor.pyx:762-763`）。`StrategyConfig` が運ぶのは venue / instrument_id / params（`bar_type_str` 等の戦略入力を含む）のみ。よって「Backtest エンジンか Live エンジンか」と「EXTERNAL / INTERNAL のどちらの `BarType` を購読させるか」をホスト層が選べば、戦略コードは 1 行も変えずに両モードで動く（これは Nautilus 標準の backtest↔live 可搬性そのもの）

### 0.5 Data Source 非対称性の吸収

Phase 8 ADR で定義された制約（venue 別に実体が異なる点を明記）:
- Replay: J-Quants OHLCV バー (分足含む) が既製品で存在、板情報なし。**Replay は `BacktestEngine` を回すため `on_bar` には既に本物の Nautilus `Bar` が届いている**（§0′-3）
- Live Auto (Tachibana): EVENT WebSocket の約定 (`EC`) / 板 (`FD` 系) が流れる → 約定列から tick 相当を構成できる
- Live Auto (kabu): **PUSH WebSocket は板情報のみで約定 tick の push は無い**（約定は `GET /orders` 1 秒 polling、Phase 9 §0.1）。よって kabu の分足/現在足は「板 push の `CurrentPrice` 更新」または「polling した約定」から構成するしかなく、Tachibana のような連続 tick 列は得られない。**aggregator の入力ソースは venue 別に明示する**（M2）

**Phase 10 で必要な追加実装の要点（C3 / §0′-3 の構造を解決する）**:
- **Live 側に Nautilus エンジンを立てるのが本丸**。Strategy が `on_bar(Bar)` を受けるには、live tick/板を **Nautilus `Bar` に集約して `LiveDataEngine` 経由で Strategy に届ける**必要がある。選択肢:
  - (推奨) live 受信を Nautilus の `TradeTick` / `QuoteTick` に変換し、Nautilus 標準 aggregation（`data/aggregation.pyx` の `TimeBarAggregator` / `TickBarAggregator`、`handle_trade_tick()`）を `LiveDataEngine` の internal aggregation で回す。これで Replay（catalog の `Bar`）と Live（aggregation 由来の `Bar`）が同じ `Bar` 型・同じ `BarSpecification` で `on_bar` に届く。`BarType.aggregation_source` は Replay が `EXTERNAL`、Live が `INTERNAL` になり得るため、完全一致を成功条件にしない
  - 既存 `live/aggregator.py` の独自 `TickBarAggregator`（`KlineUpdate` を吐く）は **UI 描画用 `LiveEventBus` 経路として継続**。Strategy への bar 供給とは別系統（混同しない。クラス名が Nautilus の `TickBarAggregator` と衝突している点に注意）
- **Partial bar push**: 既存 `live/aggregator.py` には既に `build_now()`（進行中バーのスナップショット）がある。UI 用の partial push はこれを 1 秒間隔で叩けばよい。**ただし Strategy への partial bar は Nautilus 標準では `on_bar` に未確定バーを流さない**ため、必要なら別 API（`self.cache` への書き込み or カスタムデータ）で渡す設計を §2.3 で確定する
- **戦略の depth 参照可否を declare**:
  - Strategy クラスに `REQUIRES_DEPTH: ClassVar[bool] = False` を定義
  - `True` の戦略は Replay モードでロード時に warning `STRATEGY_REQUIRES_DEPTH_REPLAY_UNAVAILABLE` を表示 (動作はするが depth は空)
  - Live Auto モードでは `True` の戦略は depth subscription を自動有効化

### 0.6 Safety Rails

戦略起動前に登録される制約。**Phase 9 はまだ軽量 `ManualOrderFacade` であり Nautilus RiskEngine wiring は未導入**（§0′-5）。Phase 10 で live host を作る際は、Nautilus の live runtime では `LiveRiskEngineConfig`（内部で `RiskEngineConfig` に変換される）を使い、ネイティブで賄える項目と独自ロジックが要る項目を分ける（M1）:

| Safety Rail | default | 実装手段 |
| --- | --- | --- |
| `max_order_value_jpy` | 50 万円 | Nautilus `LiveRiskEngineConfig.max_notional_per_order`（Python live config）にマップ。pre-trade ネイティブ |
| `max_orders_per_minute` | 5 | Nautilus `LiveRiskEngineConfig.max_order_submit_rate`（例 `"5/00:01:00"`）にマップ。ネイティブ |
| `max_position_size_jpy` | 100 万円 | Nautilus には「JPY 建てポジション金額上限」の直接 config が無い → **`RiskEngine` サブクラス or pre-trade フック**で `cache.position()` + 新規注文金額を評価する独自チェック |
| `max_daily_loss_jpy` | 10 万円 | **post-trade 独自チェック**（標準 RiskEngine は pre-trade のみ）。§2.4 / Open Risk 2 参照。超過で `LiveStrategyStateMachine.error(...)` |
| `allowed_instruments` | 起動時指定の instrument_id のみ | pre-trade 独自チェック（ホワイトリスト照合） |

- ネイティブにマップできる項目（`max_order_value` / `max_orders_per_minute`）は live runtime 用の `LiveRiskEngineConfig` で構成し、独自コードを増やさない
- 独自ロジックが要る項目（`max_position_size_jpy` / `max_daily_loss_jpy` / `allowed_instruments`）は §2.4 の薄い独自層で実装する
- これらは `StartLiveStrategy` RPC の `safety_limits` パラメータで指定。Default 値は Bevy 側で設定 UI を提供。**構造的 bypass 不可（backend が責任、ADR §6）**

### 0.7 Run 管理

- `RunRegistry` (Python in-memory) で run_id (UUID) ベースに Live run を管理
- Phase 10 MVP では自動戦略 Live run は **全体で同時に 1 つ**に制限する (`StartLiveStrategy` は既存 RUNNING/PAUSED run があれば `LIVE_STRATEGY_ALREADY_RUNNING` で reject)。手動発注 (`MANUAL-001`) との同居は許可
  - **将来用の重複判定キー (M4)**: RunRegistry は Phase 11 の拡張に備えて **`(strategy_id, instrument_id)` の組** の索引も持つ（`strategy_id` は `RegisterLiveStrategy` が検証済みファイルに対して発行したハンドル、§2.5 / M9）。Phase 10 ではこのキーに加えて `max_active_live_auto_runs = 1` を強制し、同じ戦略を別銘柄で同時起動することも許可しない。各 Live run には一意な Nautilus **`StrategyId`**（例 `LIVE-{run_id 短縮}`）を採番し、`Trader` への attach と発注主体の識別（§2.9 / M6）に使う
- Replay run と Live run は別 namespace で管理 (Replay は session_id、Live は run_id)

---

## 1. Architecture / 構成

### 1.1 Process Layout (Phase 9 からの差分)

Phase 9 の発注経路は `ManualOrderFacade` + `OrderingVenueAdapter` の軽量 bespoke パイプラインであり、まだ `TradingNode` 相当の Nautilus live engine 群ではない（§0′-5）。Phase 10 ではこの既存 live session を二重起動せずに、Nautilus live host（`NautilusKernel` + `Trader` + `LiveDataEngine` + `LiveExecutionEngine` + `LiveRiskEngine` 相当）へ段階移行し、**`Trader.add_strategy()` による Strategy attach** を加える:

```
Live Nautilus エンジン（TradingNode 相当）
├── LiveDataEngine    (Phase 8 → Phase 10 で Nautilus aggregation を有効化し Bar を Strategy へ)
├── LiveExecutionEngine (Phase 10 で既存 OrderingVenueAdapter / ExecutionClient を bridge)
├── LiveRiskEngine    (Phase 10 で LiveRiskEngineConfig + 独自 pre/post hook §2.4)
└── Trader            (Phase 10 で live host に導入し Strategy を attach)
    └── add_strategy(Strategy)   ← Phase 10 [NEW]
        （Strategy は register() 時に self.clock=LiveClock / self.cache を注入される、§0′-2）

LiveStrategyHost (Phase 10 [NEW]、既存 LiveRunner/adapter session を単一所有者として利用または引き継ぎ、上記エンジンを起動/停止し state machine と RunRegistry を管理する薄いラッパ)
```

> ⚠️ **名前衝突注意**: 既存 `python/engine/live/live_runner.py` は Phase 8 の adapter→aggregator→`LiveEventBus`（UI 用）パイプライン `LiveRunner` クラスであって、上図の Nautilus エンジンホストではない。Phase 10 のホストは `LiveStrategyHost`（§2.2）として新設し、両者を取り違えない。
>
> ⚠️ **session 所有権**: `LiveStrategyHost` が独自に venue login / WebSocket / order client を作ると、Phase 9 の `ManualOrderFacade` と競合する。Start 時は server_grpc が保持する現在の `_live_runner` / `_order_facade` / adapter を検査し、未ログインなら `VENUE_LOGIN_REQUIRED`、別 host が所有中なら `LIVE_SESSION_ALREADY_OWNED` で reject する。実装方針は「共有」か「明示 handoff」のどちらかに固定し、同一 venue への二重接続をテストで禁止する。

### 1.2 State Machine

```
LiveStrategyStateMachine (Phase 10 [NEW])
  IDLE → LOADING → READY → RUNNING → (PAUSED) → STOPPING → STOPPED
                                  ↘ ERROR (safety rail violation / venue error)
```

- `READY` 状態: Strategy がロード済み、Safety Rails 設定済み、まだ market data を流していない
- `RUNNING` → `PAUSED`: **Nautilus に「外側の Host が attach 済み Strategy への `on_bar` だけを横取りして止める」汎用 hook は無い** (M5)。Phase 10 では「Pause = 新規発注ゲート」を安全仕様として採用する:
  - 実装方針: `LiveStrategyHost` / `live/safety_rails.py` が run_id 単位の paused flag を持ち、paused 中の新規注文を `STRATEGY_PAUSED` として deny する。market data callback は継続し得るが、発注は構造的に通らない
  - 既存注文の OrderEvent / PositionEvent は通常どおり Strategy / Cache / UI に届く
  - 「callback 自体も止める Pause」は Phase 11 候補。実装する場合は `strategy.stop()` / unsubscribe→resume 時 re-subscribe、または Strategy proxy を別途設計する
- `ERROR` 遷移時: `StopLiveStrategy` を内部発射 → **当該 `StrategyId` の** in-flight order のみ cancel（手動 / 他戦略の注文を巻き込まない、§2.9 / M6） → run を STOPPED に

### 1.3 Promote to Live フロー (高レベル)

```
[Strategy Editor]
   │ (1) ユーザー: [Promote to Live] クリック
   ↓
[Bevy UI: Pre-flight Check]
   │ Venue ログイン / mode / unsaved changes / safety limits を検証
   ↓
[Bevy UI: 2 段階確認モーダル]
   │ Replay KPI サマリー表示
   │ Safety Rails 設定 UI (max_position / max_order / max_daily_loss)
   │ [Confirm] クリック
   ↓
[gRPC: RegisterLiveStrategy(strategy_file)]
   │ canonical path / sha256 / scenario / strategy_cls を backend で検証
   │ → strategy_id を返す
   ↓
[gRPC: SetExecutionMode(LiveAuto)]  // 現在 LiveAuto でなければ必須。失敗時は中止
   ↓
[gRPC: StartLiveStrategy(strategy_id, instrument_id, venue, params, safety_limits)]
   ↓
[Python: LiveStrategyHost (live/strategy_host.py)]
   │ (a) StrategyRegistry から strategy_id→strategy_file を引き、strategy_runtime/strategy_loader.load() でロード
   │ (b) 既存 live session を検査・所有権確定後、Live Nautilus エンジンを起動し Trader.add_strategy(Strategy)
   │ (c) LiveRiskEngineConfig + safety_rails に safety_limits を登録
   │ (d) LiveDataEngine から該当 instrument_id の subscription + Bar aggregation を有効化
   │ (e) run_id 採番 + StrategyId 採番 → RunRegistry に登録
   │ (f) state: READY → RUNNING
   ↓
[SubscribeBackendEvents: BackendEvent.LiveStrategyEvent{run_id, strategy_id, status, ts_ms}]
   ↓
[Bevy UI: Footer に Run Badge 表示]
```

---

## 2. Tasks

### 2.1 Backend: Strategy Portability Layer

- `python/engine/strategy_runtime/strategy_loader.py` は既存 (`load()` 実装済み)。Phase 10 では **ローダは無改変が原則**で、Live ホスト (`LiveStrategyHost`) が同じ `load()` を呼ぶ
  - 環境依存 (clock / data engine / exec) は **Strategy を attach するエンジンが供給**する（`StrategyConfig` には入れない、§0′-2）。`StrategyConfig` には venue / instrument_id / params（`bar_type_str` 等の戦略入力を含む）のみ
  - Strategy 本体は `self.clock.utc_now()` / `self.subscribe_bars(...)` / `on_bar(Bar)` を使う（`register()` 後にエンジンが注入）。`self.data.subscribe_bars` ではなく `Strategy` 継承の `self.subscribe_bars` が正（`common/actor.pyx`）
- 既存サンプル戦略（**`mean_reversion_01` / `FakeBuyAndHold` 等、`ma_cross.py` は存在しない**）が Replay モード（`engine_runner.py` の `BacktestEngine`）で従来通り動くことを回帰確認した上で、同じ `strategy_cls` が Live ホストにも attach できることを確認

### 2.2 Backend: LiveStrategyHost

- `python/engine/live/strategy_host.py` を新設
- **Live 用 Nautilus エンジン（`TradingNode` 相当）の起動/停止**、`Trader.add_strategy()` / `remove_strategy()` による Strategy の attach / detach、state machine の管理（Nautilus に `StrategyEngine` は無い、§0′-1）
- **既存 live session との所有権を明示する（§0′-5 / §1.1）**:
  - `server_grpc` が保持する `_live_runner` が logged-in であることを Start 前提にする。未ログインなら `VENUE_LOGIN_REQUIRED`
  - `LiveStrategyHost` は同じ venue adapter / execution client / SecretVault を使うか、Start 時に明示 handoff する。いずれの場合も同一 venue への二重 login・二重 WebSocket・二重 order client は作らない
  - `ManualOrderFacade` と LiveAuto が同居する場合は、手動発注の `StrategyId` を `MANUAL-001` として同じ order event stream に載せる。LiveAuto 開始時に manual path を無効化する方針を選ぶなら、UI と RPC が `LIVE_AUTO_OWNS_SESSION` を返す
  - 回帰テスト: `StartLiveStrategy` が既存 session 不在で reject すること、同一 adapter に対して login が 2 回呼ばれないこと、SecretVault を bypass しないこと
- 戦略 lifecycle hook は Nautilus が呼ぶ（ホストが直接呼ぶのではない）。ホストの責務は state machine 遷移と RunRegistry 連携:
  - `on_start()` — Strategy 内。`READY → RUNNING` 遷移後に Nautilus が呼ぶ
  - `on_stop()` — Strategy 内。`STOPPING → STOPPED` 遷移時に Nautilus が呼ぶ
  - `on_bar()` / `on_quote_tick()` / `on_trade_tick()` — `LiveDataEngine` 由来。Phase 10 の PAUSE は callback 停止ではなく新規発注 deny（§1.2 / M5）
  - `on_order_filled()` / `on_order_canceled()` — `LiveExecutionEngine` 由来

### 2.3 Backend: Bar Builder 強化

**目的: Strategy の `on_bar` に Replay / Live で同じ Nautilus `Bar` 型が届くようにする**（§0′-3 / C3）。「Replay を BarBuilder に変える」のではなく「**Live に Nautilus aggregation を入れて Bar を作る**」のが本筋。

- **Live (Strategy 供給経路) [本丸]**: live の約定/板を Nautilus `TradeTick` / `QuoteTick` に変換し、`LiveDataEngine` の internal bar aggregation（`data/aggregation.pyx` の `TimeBarAggregator.handle_trade_tick()` / `TickBarAggregator`、`BarType` の 5 番目を `INTERNAL` 指定）で `Bar` を生成して Strategy の `on_bar` に届ける。これで Replay の catalog `Bar`（`EXTERNAL`）と同じ `Bar` 型・同じ `BarSpecification` に揃う。`bar_type_str` を直接受け取る既存戦略には、Replay runner が `...-EXTERNAL`、Live host が `...-INTERNAL` を渡すことで戦略コード変更を避ける
  - **venue 別の入力 (M2)**: Tachibana は `EC`（約定）を `TradeTick` 化。kabu は **約定 tick が無い**ため、板 push の `CurrentPrice` か `GET /orders` polling から `TradeTick` 相当を構成する（精度限界を §8 Open Risk に明記）
- **Live (UI 描画経路) [既存維持]**: `python/engine/live/aggregator.py` の独自 `TickBarAggregator`（`KlineUpdate` を `LiveEventBus` に流す）はそのまま。partial bar push は既存 `build_now()` を 1 秒間隔で叩く形で実装（メソッドは実装済み、追加は push のスケジューリングのみ）
- **Replay 側は無改変**: 既に `BacktestEngine` が本物の `Bar` を `on_bar` に流している。J-Quants OHLCV → `Bar` 変換は catalog ローダ (`nautilus_catalog_loader.py` 等) が担当済み
- ⚠️ Strategy への **未確定（partial）bar** は Nautilus 標準では `on_bar` に流れない。必要なら別経路（カスタムデータ or cache）で渡す設計を本ステップで確定する

### 2.4 Backend: Safety Rails (LiveRiskEngineConfig の構成 + 独自 hook)

> `python/engine/live/risk_engine.py` は **未存在**。Phase 9 は軽量 `ManualOrderFacade` であり、Nautilus live RiskEngine wiring は Phase 10 で初導入する（§0′-5 / M1）。Live runtime では `LiveRiskEngineConfig` を使う。`RiskEngineConfig` を live kernel に直接渡すと環境不一致になるため禁止。

- **ネイティブ `LiveRiskEngineConfig` で構成（独自コードを増やさない）**:
  - `max_order_value_jpy` → per-instrument `max_notional_per_order`
  - `max_orders_per_minute` → `max_order_submit_rate`（例 `"5/00:01:00"`）
- **独自薄層（`live/safety_rails.py` 新設）で pre-trade フック**:
  - `max_position_size_jpy`: `cache.position()` の既存ポジション + 新規注文後の合計金額が上限以内か
  - `allowed_instruments`: instrument_id がホワイトリスト内か
- **独自薄層で post-trade チェック**（標準 RiskEngine は pre-trade のみ）:
  - `max_daily_loss_jpy`: 当日の realized + unrealized P&L が上限を下回ったら `LiveStrategyStateMachine.error("MAX_DAILY_LOSS_EXCEEDED")` を発射（mark-to-market の評価タイミングは保守的に、§8 Open Risk 2）
- pre-trade 違反は Nautilus が `OrderDenied`（`RiskEngine` reject）として戦略に通知。UI には `SubscribeBackendEvents` の `BackendEvent.oneof` 経由で `SafetyRailViolation{run_id, kind, detail, ts_ms}` を push（§2.5 / M8）

### 2.5 Backend: gRPC RPC 追加

```proto
service DataEngine {
  // 既存 RPC...

  // Phase 10（すべて unary。Phase 9 ADR の通り汎用 streaming transport は
  // SubscribeBackendEvents の 1 本のみで、それ以外に stream RPC は増やさない、M7）
  rpc RegisterLiveStrategy(RegisterLiveStrategyReq) returns (RegisterLiveStrategyRes);
  rpc StartLiveStrategy(StartLiveStrategyReq) returns (StartLiveStrategyRes);
  rpc StopLiveStrategy(StopLiveStrategyReq) returns (LiveStrategyControlRes);
  rpc PauseLiveStrategy(PauseLiveStrategyReq) returns (LiveStrategyControlRes);
  rpc ResumeLiveStrategy(ResumeLiveStrategyReq) returns (LiveStrategyControlRes);
  rpc GetLiveStrategyStatus(GetLiveStrategyStatusReq) returns (GetLiveStrategyStatusRes);
  rpc ListLiveStrategies(ListLiveStrategiesReq) returns (ListLiveStrategiesRes); // unary repeated（streaming にしない、M7）
}

message RegisterLiveStrategyReq {
  string token = 1;
  string request_id = 2;
  string strategy_file = 3;  // UI が保存済み path。backend が resolve/canonicalize して許可ディレクトリ配下か検証する
  string expected_sha256 = 4; // UI が確認モーダルに表示した内容との TOCTOU 防止。空なら backend 計算値のみ返す
}

message RegisterLiveStrategyRes {
  bool success = 1;
  string request_id = 2;
  string error_code = 3;
  string strategy_id = 4;      // StartLiveStrategy に渡す opaque handle
  string strategy_sha256 = 5;  // Promote 確認時の再照合用
  string display_name = 6;
}

message StartLiveStrategyReq {
  // 任意ホストパスを backend に exec させない（live 自動発注経路の RCE / FS 共有前提を避ける、M9）。
  // RegisterLiveStrategy が検証済みの strategy_id（許可ディレクトリ配下に限定）を渡す。
  string token = 1;
  string request_id = 2;
  string strategy_id = 3;     // RegisterLiveStrategy で検証済みのハンドル（生パスではない）
  string instrument_id = 4;
  string venue = 5;
  map<string, string> params = 6;
  SafetyLimits safety_limits = 7;
}

message StartLiveStrategyRes {
  bool success = 1;
  string request_id = 2;
  string error_code = 3;
  string run_id = 4;
  LiveStrategyStatus status = 5;
}

message StopLiveStrategyReq { string token = 1; string request_id = 2; string run_id = 3; }
message PauseLiveStrategyReq { string token = 1; string request_id = 2; string run_id = 3; }
message ResumeLiveStrategyReq { string token = 1; string request_id = 2; string run_id = 3; }
message GetLiveStrategyStatusReq { string token = 1; string request_id = 2; string run_id = 3; }
message ListLiveStrategiesReq { string token = 1; string request_id = 2; }

message LiveStrategyControlRes {
  bool success = 1;
  string request_id = 2;
  string error_code = 3;
  LiveStrategyStatus status = 4;
}

message GetLiveStrategyStatusRes {
  bool success = 1;
  string request_id = 2;
  string error_code = 3;
  LiveStrategyStatus status = 4;
}

message LiveStrategyStatus {
  string run_id = 1;
  string strategy_id = 2;           // RegisterLiveStrategy の opaque handle
  string nautilus_strategy_id = 3;  // 発注主体識別用 StrategyId
  string instrument_id = 4;
  string venue = 5;
  string status = 6;                // READY/RUNNING/PAUSED/STOPPED/ERROR
  int64 ts_ms = 7;
}

message ListLiveStrategiesRes {
  bool success = 1;
  string request_id = 2;
  string error_code = 3;
  repeated LiveStrategyStatus strategies = 4;
}

message SafetyLimits {
  int64 max_position_size_jpy = 1;   // 独自 pre-trade（M1）
  int64 max_order_value_jpy = 2;     // → LiveRiskEngineConfig.max_notional_per_order（ネイティブ）
  int64 max_daily_loss_jpy = 3;      // 独自 post-trade
  int32 max_orders_per_minute = 4;   // → LiveRiskEngineConfig.max_order_submit_rate（ネイティブ）
  repeated string allowed_instruments = 5;  // 独自 pre-trade
}

// 新規イベントは Phase 9 Step 0 の SubscribeBackendEvents の BackendEvent.oneof に
// 追加する（別 stream を作らない＝Phase 9 ADR「single channel」維持、M8）:
//   - LiveStrategyEvent{run_id, strategy_id, status, ts_ms}
//   - SafetyRailViolation{run_id, kind, detail, ts_ms}
//   - StrategyLogMessage{run_id, level, message, ts_ms}  // Strategy 内 self.log.info() の中継
//
// 既存 OrderEvent には strategy_id が無い（2026-05-21 時点の engine.proto は
// order_id/venue_order_id/client_order_id/status/filled_qty/avg_price/ts_ms のみ）。
// Phase 10 で発注主体フィルタを行うため、次を additive に追加する:
//   optional string strategy_id = 8;  // Nautilus StrategyId。手動は "MANUAL-001"
// Python OrderEventData / _order_event_to_proto / Rust BackendEvent::OrderEvent / LiveOrder も同時に mirror する。
```

- **認証/認可の必須条件**: Phase 10 で追加する全 RPC は既存 RPC と同じ `token` を必須にし、token 不一致は `UNAUTHENTICATED` abort。`StartLiveStrategy` / `Pause` / `Resume` / `Stop` の write 系は token 検証後に `ExecutionMode == LiveAuto` と live session 所有権を検査し、失敗時は structured `success=false` / `error_code`（`EXECUTION_MODE_PRECONDITION`, `VENUE_LOGIN_REQUIRED`, `LIVE_SESSION_ALREADY_OWNED`）で返す
- **path 検証**: `RegisterLiveStrategy` は受け取った path を backend 側で `resolve(strict=True)` し、許可 root 配下・通常ファイル・`.py` 拡張子・symlink escape なしを確認してから `strategy_loader.load()` を呼ぶ。`expected_sha256` が指定されていれば resolve 後の実ファイル hash と一致しない場合 `STRATEGY_HASH_MISMATCH`

### 2.6 Backend: RunRegistry

- `python/engine/live/run_registry.py` を新設
- `register(run_id, strategy_id, instrument_id, nautilus_strategy_id, ...)` / `unregister(run_id)` / `get(run_id)` / `list_active()`
- Phase 10 では active automated run が 1 件でもあれば新規 `StartLiveStrategy` を reject する。加えて将来拡張用に `(strategy_id, instrument_id)` → run_id の索引を持つ（§0.7 / M4）。各 run は一意な Nautilus `StrategyId` を保持し、発注主体識別（§2.9 / M6）に使う
- 永続化なし (in-memory)。プロセス再起動時は全 run が消える (戦略本体は venue 側に注文が残る可能性あり、要 UI 警告)

### 2.7 UI: Strategy Editor `[Promote to Live]` ボタン

- `src/ui/strategy_editor.rs` (Phase 7.2) にボタン追加
- 前提条件チェック (§0.2) → 失敗時はエラートースト
- 成功時に Safety Rails 設定モーダル (`src/ui/safety_rails_modal.rs` 新設) を開く
- モーダルで Safety Rails 入力 + Replay KPI サマリー表示 (直近 Replay 結果を Cache から取得) → `[Confirm]` で `RegisterLiveStrategy` → 必要なら `SetExecutionMode(LiveAuto)` → `StartLiveStrategy` RPC

### 2.8 UI: Live Run Panel

- `src/ui/live_run_panel.rs` を新設
- アクティブな Live run の一覧表示 (Phase 10 は automated run 1 件制約だが UI は将来複数対応を想定)
- 各 run の状態 (RUNNING / PAUSED / ERROR)、起動時刻、累積 P&L、発注数、約定数
- `[Pause]` / `[Resume]` / `[Stop]` ボタン

### 2.9 UI: 既存 OrdersPanel の発注主体フィルタ / PositionsPanel の扱い

- Phase 9 では「Live で発生した全 Order / account position」を表示するだけだったが、Phase 10 では複数の発注主体 (手動 / Strategy A / Strategy B) が並ぶ可能性がある
- **発注主体の識別は Nautilus `StrategyId` で行う（M6）**: Cython の `Order` / `OrderFilled` は immutable で任意フィールドを後付けできないため、`source_run_id` という新フィールドは持たせない。代わりに各 Live run に一意な `StrategyId`（`LIVE-{run_id 短縮}`、手動発注は Phase 9 §3.1 の `MANUAL-001`）を割り当て、`OrderEvent` proto に **additive field `optional string strategy_id = 8`** を追加して区別する。RunRegistry が `StrategyId ↔ run_id` を対応付ける
- OrdersPanel（実ファイルは `src/ui/orders.rs`）に「絞り込み: All / Manual / Strategy: XXX」ドロップダウンを追加（フィルタは `OrderEvent.strategy_id` / `LiveOrder.strategy_id` で行う）
- PositionsPanel（実ファイルは `src/ui/positions.rs`）は Phase 9 の `AccountEvent`→`PortfolioState` reducer を使う **口座全体の net position 表示**であり、現行 `AccountEvent` / `AccountPosition` には `strategy_id` が無い。Phase 10 では PositionsPanel 自体の strategy filter は実装しない。run 別の position / PnL が必要な場合は `LiveStrategyEvent`（または新規 `LiveStrategyTelemetry`）に `run_id` 付きの戦略別 telemetry を載せ、`Live Run Panel` に表示する

### 2.10 UI: SafetyRailViolation トースト

- `SafetyRailViolation` イベントを受信したら Footer 右下に warning トースト
- 違反種別ごとに色分け (max_daily_loss は赤、max_orders_per_minute は黄等)

---

## 3. File Layout

```
python/engine/
├── strategy_runtime/
│   ├── strategy_loader.py          # 既存。原則無改変（load() を Live ホストも呼ぶ）
│   └── engine_runner.py            # 既存 Replay (BacktestEngine)。原則無改変
├── live/
│   ├── live_runner.py              # 既存 Phase 8 adapter パイプライン（UI 用、別物・改変なし）
│   ├── strategy_host.py    [NEW]   # LiveStrategyHost: Live Nautilus エンジン起動 + Trader.add_strategy + state machine
│   ├── run_registry.py     [NEW]   # in-memory run 管理 + (strategy_id,instrument_id) 索引 + StrategyId 対応
│   ├── safety_rails.py     [NEW]   # 独自 pre/post-trade hook（max_position / max_daily_loss / allowed_instruments）
│   └── aggregator.py               # 既存（UI 用 build_now() partial push のスケジューリングのみ追加）
│   # Strategy 供給用の Nautilus aggregation は LiveDataEngine の internal aggregation で構成（新ファイル不要）

src/ui/
├── strategy_editor.rs              # [Promote to Live] ボタン追加
├── safety_rails_modal.rs   [NEW]   # Safety Rails 設定 + Replay KPI 表示
├── live_run_panel.rs       [NEW]   # アクティブ run 一覧 + 制御
├── positions.rs                    # 既存。口座全体の net position 表示を維持（strategy_id フィルタは入れない）
└── orders.rs                       # strategy_id フィルタ追加（M6）
```

---

## 4. Implementation Order

各 Step 完了時点で `cargo run` 可能を維持。Mock 経由で発注テストできるよう、Step 1 で MockVenueAdapter にも戦略 attach の経路を通す。

1. **Step 1 — Strategy Portability 確認 + Live Bar 供給の設計確定**
   - `strategy_runtime/strategy_loader.load()` が Replay/Live 両方から呼べることを確認（ローダ改変は最小）
   - Live 側に Nautilus aggregation（`LiveDataEngine` internal aggregation で `Bar` 生成）を入れる経路を設計・PoC（§2.3 / C3）
   - 既存サンプル戦略（`mean_reversion_01` 等）が Replay モードで従来通り動作することを回帰確認
2. **Step 2 — LiveStrategyHost + RunRegistry**
   - `live/strategy_host.py` 実装、state machine 単体テスト
   - `live/run_registry.py` 実装
3. **Step 3 — gRPC RPC + `BackendEvent` oneof 拡張（M8）**
   - `StartLiveStrategy` / `StopLiveStrategy` / `Pause` / `Resume` / `GetStatus` / `ListLiveStrategies`（全 unary）実装
   - 新イベント（`LiveStrategyEvent` / `SafetyRailViolation` / `StrategyLogMessage`）を既存 `SubscribeBackendEvents` の `BackendEvent.oneof` に追加
   - MockVenueAdapter で疎通テスト
4. **Step 4 — Safety Rails (ネイティブ config + 独自 hook)**
   - `max_order_value` / `max_orders_per_minute` を `LiveRiskEngineConfig` で構成（ネイティブ）
   - `max_position_size_jpy` / `allowed_instruments` (pre-trade) / `max_daily_loss` (post-trade) を `live/safety_rails.py` で実装
   - 違反を `SubscribeBackendEvents` の `BackendEvent.oneof`（`SafetyRailViolation`）に push
5. **Step 5 — Bevy UI: Safety Rails Modal + Promote to Live ボタン**
   - `safety_rails_modal.rs` 新設、Replay KPI サマリー表示
   - Strategy Editor から `[Promote to Live]` 経路の E2E (Mock)
6. **Step 6 — Bevy UI: Live Run Panel**
   - `live_run_panel.rs` 新設
   - Pause / Resume / Stop ボタンの動作確認
7. **Step 7 — OrdersPanel の strategy_id フィルタ + LiveRun telemetry**
   - `OrderEvent` proto に `optional string strategy_id = 8` を additive 追加し、Python / Rust mirror を更新（Order Cython 型には新フィールドを足さない、M6）
   - `orders.rs` にドロップダウン UI 追加（All / Manual / Strategy: XXX）
   - `positions.rs` は口座全体表示のまま維持。run 別 position / PnL は `LiveRunPanel` の telemetry として表示する
8. **Step 8 — Partial Bar Push（UI 経路）+ Live Bar 供給の検証**
   - `aggregator.py` の `build_now()` を 1 秒間隔で `LiveEventBus` に push（UI 用）
   - Strategy 供給経路は Nautilus aggregation 由来の `Bar` が `on_bar` に届くことを確認
   - Replay (catalog `Bar`) / Live (aggregation `Bar`) で同じ `BarSpecification`・同じ OHLCV になる回帰テスト（`aggregation_source` は EXTERNAL / INTERNAL で異なり得る）
9. **Step 9 — Live E2E (Demo / Verify)**
   - Tachibana Demo + 簡単な戦略 (`mean_reversion_01` 等) を 1 営業日 Live 稼働
   - kabu Verify でも同様に E2E（kabu は約定 tick 無し前提の精度限界を確認、§8 Open Risk 5）
10. **Step 10 — Polish**
    - drawio アーキ図 `phase10-architecture.drawio.svg`
    - Strategy 開発者向けドキュメント (Portability の使い方、Safety Rails の指針)
    - Phase 11 への引き継ぎ事項を docs にまとめる

---

## 5. Success Criteria

### Strategy Portability

- 既存 Replay 用サンプル戦略 (`mean_reversion_01` / `FakeBuyAndHold` 等、`ma_cross.py` は存在しない) が **コード変更ゼロ** で Live Auto モードで起動できる
- Strategy 内に `if mode == "replay":` のような分岐が存在しない (grep で確認)
- `Strategy.on_bar()` に渡るのが Replay (catalog `Bar`) / Live (Nautilus aggregation `Bar`) のいずれでも同じ `Bar` 型・同じ `BarSpecification` であることを type test で確認（`BarType.aggregation_source` の完全一致は要求しない）

### Promote to Live

- Strategy Editor で `.py` を編集 → `[Promote to Live]` → Safety Rails モーダル → Live 起動、までが手動 E2E で通る
- Venue 未ログイン / unsaved changes / safety limits 未設定 のいずれかが NG なら `[Promote to Live]` ボタンが disabled になる
- 2 段階確認モーダルで Replay KPI が表示される。**現行 `summary.py` が算出するのは `total_pnl` / `max_drawdown` / `trade_count` / `win_rate` / `fee_total`**。`Sharpe` / 累積リターン% は未算出（M3）→ Phase 10 で算出を追加するか、モーダル表示項目から外すかを Step 5 着手時に決める（既存項目のみで進めるのが既定）

### Safety Rails

- `max_position_size_jpy` 超過 → 独自 pre-trade hook が `OrderDenied`、UI トースト表示 (unit test + Mock E2E)
- `max_order_value_jpy` 超過 → ネイティブ `LiveRiskEngineConfig.max_notional_per_order` が `OrderDenied`
- `max_orders_per_minute` 超過 → ネイティブ `max_order_submit_rate` で抑制 (unit test)
- `max_daily_loss_jpy` 超過 → 戦略が自動 STOPPED 状態に、**当該 `StrategyId` の** in-flight order が cancel (unit test + Mock E2E)
- `allowed_instruments` 外への発注 → 独自 pre-trade hook が `OrderDenied`

### Live Run Telemetry

- Live 稼働中の fills は OrdersPanel に、account-level position は既存 PositionsPanel に表示される
- run 別 position / PnL は `LiveRunPanel` の telemetry で表示される
- 複数 run (手動 + Strategy) が同居しても OrdersPanel は `strategy_id` フィルタで分離表示できる
- `SafetyRailViolation` トーストが Footer 右下に出る

### 構造的安全性

- `ExecutionMode != LiveAuto` で `StartLiveStrategy` を呼ぶと `EXECUTION_MODE_PRECONDITION` で reject (unit test)
- Phase 10 の全 RPC は bad token で `UNAUTHENTICATED` abort、token 正常だが precondition NG なら structured `success=false` / `error_code` を返す (unit test)
- active automated run が既にある状態の `StartLiveStrategy` が `LIVE_STRATEGY_ALREADY_RUNNING` で reject され、将来用の `(strategy_id, instrument_id)` 索引も重複を検出する (unit test、M4)
- Replay run と Live run が同時に走っているとき、UI の Run Badge で両方が独立に表示される

### Bar 供給の一致

- Replay (catalog `Bar`) と Live (Nautilus aggregation 由来 `Bar`) で、同じ tick 列から生成される `Bar` の OHLCV が一致する (unit test)
- UI 用 partial bar push が 1 秒間隔で `LiveEventBus` に発火する。**Strategy への未確定バー供給は別経路（§2.3、Nautilus 標準の `on_bar` は確定バーのみ）であることを明記**

---

## 6. ADRs

### ADR: 環境依存はエンジンが供給し、Strategy は無分岐（`StrategyConfig` には clock/data を入れない）

選択肢:
- **A. Strategy 内で `if self.mode == "replay"` のような分岐** — コード重複、保守性低下
- **B. Backtest エンジンか Live エンジンかをホスト層が選び、Strategy はエンジンが注入する `self.clock` / `self.cache` / `self.subscribe_bars` を使う（無分岐）** ← **採用**

採用理由: これは Nautilus 標準の backtest↔live 可搬性そのもの。`self.clock` / `self.cache` は `Trader` への `register()` 時にエンジンが注入する（`common/actor.pyx:762-763`）。**`StrategyConfig` が運ぶのは venue / instrument_id / params のみで、clock や data engine は config 経由では渡らない**（§0′-2）。当初案の「環境依存を `StrategyConfig` 経由で注入」は Nautilus の実際の仕組みと異なるため訂正。

### ADR: Live 側に Nautilus aggregation を入れて `Bar` を作る（Replay は無改変）

選択肢:
- **A. Live の独自 `KlineUpdate` を Strategy にも流す** — Replay (Nautilus `Bar`) と型が違い Strategy が分岐する
- **B. Live tick を Nautilus `TradeTick` 化し `LiveDataEngine` の internal aggregation で `Bar` を生成、Strategy の `on_bar(Bar)` に届ける** ← **採用**

採用理由: Replay は既に `BacktestEngine` 経由で本物の `Bar` を `on_bar` に流している（§0′-3）。当初案「Replay 側も BarBuilder を経由する」は前提が逆で、改修すべきは Live 側。Live を Nautilus aggregation に揃えれば Replay/Live で同じ `Bar` 型・同じ `BarSpecification` になり、プロモート時の挙動差分を構造的に小さくできる。`BarType.aggregation_source` は Replay が `EXTERNAL`、Live が `INTERNAL` になり得るため、Strategy が source を直書きしないよう runner/host が `bar_type_str` を供給する。既存 `live/aggregator.py`（独自 `TickBarAggregator` → `KlineUpdate`）は UI 描画専用として残す。

### ADR: Safety Rails は backend で実装（ネイティブ `LiveRiskEngineConfig` + 独自薄層）

選択肢:
- **A. UI 側 (Rust) で Safety Rails チェック** — bypass されるリスク (RPC を直接叩けば回避可能)
- **B. backend で実装。ネイティブで賄える項目は `LiveRiskEngineConfig`、不足分のみ独自薄層** ← **採用**

採用理由: Safety Rails は **構造的に bypass 不可能** であるべき。`max_order_value`→`max_notional_per_order`、`max_orders_per_minute`→`max_order_submit_rate` は live runtime のネイティブ config で構成し（`LiveRiskEngineConfig`、§0′-5 / M1）、`max_position_size_jpy` / `max_daily_loss_jpy` / `allowed_instruments` のみ `live/safety_rails.py` の独自 pre/post-trade hook で実装する。UI は値入力 layer に留める。

### ADR: Phase 10 は automated Live run を同時 1 件に制約

選択肢:
- **A. 複数 automated run 許可** — 戦略のロジック次第で二重発注リスク、run 別 PnL 配賦も Phase 10 では未成熟
- **B. automated Live run は同時 1 件に制約** ← **採用**

採用理由: Phase 10 段階では戦略の多重化が想定外。手動発注との同居は `StrategyId` で分離できるが、自動戦略を複数同時に走らせると二重発注・資金配賦・停止時 cancel 範囲が一気に難しくなる。RunRegistry には `(strategy_id, instrument_id)` 索引を持たせて Phase 11 で `instance_id` / 複数 run へ拡張できる余地を残す。

### ADR: 戦略の hot-reload は対象外、明示停止 → 再起動を要求する

選択肢:
- **A. `.py` 編集を検知して自動再起動** — 編集中の半端な状態で起動するリスク
- **B. 明示停止 → 編集 → 再起動を要求** ← **採用**

採用理由: 誤発注リスクと UI 状態の整合性。Phase 11 以降で「safe reload」(現在の position をそのまま引き継いで新戦略インスタンスに移行) を別途設計する。

### ADR: 発注主体は Nautilus `StrategyId` で識別する（Cython Order に新フィールドを足さない）

選択肢:
- **A. `Order` / `OrderFilled` に `source_run_id` フィールドを追加** — Cython の immutable 型は後付けフィールド不可（`OrderFilled` をサブクラスできない）
- **B. 各 Live run に一意な `StrategyId` を割り当て、transport には `OrderEvent.strategy_id` を additive 追加して識別** ← **採用**

採用理由: Cython 型の制約上 A は実現不可（§0′ / M6）。各 run = 一意 `StrategyId`（手動は `MANUAL-001`、Phase 9 §3.1）で `cache.orders(strategy_id=...)` 等のネイティブ API がそのまま使え、戦略停止時に「その `StrategyId` の in-flight order だけ cancel」が安全にできる。現行 `engine.proto` の `OrderEvent` には `strategy_id` が無いため、Phase 10 Step 3 で `optional string strategy_id = 8` を追加し、Python `OrderEventData` / Rust `BackendEvent::OrderEvent` / `LiveOrder` へ同時に mirror する。RunRegistry が `StrategyId ↔ run_id` を対応付ける。

### ADR: Nautilus エンジン群を Phase 8 → 9 → 10 で段階有効化（Phase 10 は Strategy を attach）

Phase 8 → 9 → 10 で段階的に発注能力を解禁する設計（Nautilus に `StrategyEngine` は無く、戦略は `Trader.add_strategy()` で attach する、§0′-1）:
- Phase 8: `DataEngine` のみ (read-only)
- Phase 9: bespoke `LiveRunner` + `ManualOrderFacade` + `OrderingVenueAdapter`（手動発注のみ。Nautilus live engine 群は未導入）
- Phase 10: Nautilus live host を導入し、既存 adapter / execution client を bridge した `LiveExecutionEngine` / `LiveRiskEngine` 相当 + **`Trader.add_strategy(Strategy)`**（戦略が `submit_order()` を呼ぶ）+ Safety Rails 強化

これにより各 Phase で発生し得る障害範囲が明確に区切られる。

### Open Question: 戦略の永続化と再起動時の復旧

backend が crash → 自動再起動 (Phase 9) した場合、稼働中の Live Strategy はどうするか:
- **案 A**: 自動再起動時に最後の run 設定を `app_state.json` 等から復元
- **案 B**: 再起動時はすべての Live run を停止状態にして、ユーザーに再起動判断を委ねる ← **Phase 10 では採用**

理由: 自動復元は意図しないタイミングで戦略が再起動するリスクがある。Phase 10 段階では「クラッシュ時は人間判断」を採用し、永続化は Phase 11 で再評価。

### Open Question: Live Strategy のログ出力先

Strategy 内 `self.log.info(...)` の出力先:
- Phase 10: backend のログファイルにのみ出力 + `StrategyLogMessage` イベントで Live Run Panel に直近 100 行表示
- Phase 11 候補: 専用ログビューアパネル

---

## 7. Phase 11 への引き継ぎ事項

| 項目 | Phase 10 での状態 | Phase 11 候補 |
| --- | --- | --- |
| **複数 automated Live run 同時実行** | 自動戦略 run は同時 1 件に制約 | `instance_id` / 複数 run 拡張 |
| **戦略の hot-reload** | 明示停止 → 再起動 | safe reload (position 引き継ぎ) |
| **戦略パラメータ最適化** | 非対象 | Grid search / Optuna 統合 |
| **戦略パフォーマンスダッシュボード** | 生イベント表示のみ | KPI 集計 / Sharpe / Calmar 自動計算 |
| **Live Strategy の永続化と自動復旧** | 再起動時は停止状態 | app_state 経由の復元 |
| **専用ログビューアパネル** | Live Run Panel の最終 100 行のみ | フィルタ機能付きログビューア |
| **戦略のバージョン管理** | 非対象 | git 連携 / strategy_id にハッシュ付与 |
| **複数 Venue 同時接続** (Phase 8 Open Question) | 非対象 | venue 別 `Trader` / `TradingNode` |

---

## 8. Open Risks

1. **Replay と Live で `Bar` 出力の微差** — Replay は catalog の確定 `Bar`、Live は Nautilus aggregation 由来の `Bar`。tick の集約方式・タイムスタンプ境界で OHLCV が ±1 tick ずれる可能性。Step 8 で徹底的に regression test
2. **Safety Rails の loophole** — `max_daily_loss` の計算における unrealized P&L 評価タイミング (mark-to-market) のずれで判定が遅延する可能性。標準 RiskEngine は pre-trade のみのため post-trade は独自層、実装時に保守的に評価
3. **Strategy 内で例外発生時の挙動** — Nautilus 標準では `on_bar` の例外で戦略が落ちる。Phase 10 では `LiveStrategyStateMachine.error("STRATEGY_EXCEPTION")` に遷移させ、`SafetyRailViolation` イベントで UI に通知 + 当該 `StrategyId` の in-flight order を cancel
4. **Promote to Live の Replay KPI 信頼性** — 直近 Replay 結果を取得するが、戦略パラメータ変更後に Replay 未実行のまま `[Promote to Live]` を押されるとサマリーが古い。前提条件チェックに「直近 Replay 結果のパラメータが現在と一致しているか（params のハッシュ突合）」を含める
5. **kabu は約定 tick feed が無い** — kabu の PUSH は板情報のみ、約定は 1 秒 polling（Phase 9 §0.1）。よって ① 戦略への `Bar` 供給は板 `CurrentPrice` か polling 約定からの構成となり Tachibana より精度が落ちる、② 「直近約定を見て次の判断」をする戦略は最大 1〜2 秒遅延する。Phase 11 で push 化を venue にリクエストするか、kabu 戦略に polling 前提の設計指針を出す

---

## 9. 進捗トラッカー (Implementation Progress)

| Step | 内容 | 状態 |
| --- | --- | --- |
| 1 | Strategy Portability 確認 + Live Bar 供給の設計確定 | ✅ 完了 (2026-05-21) |
| 2 | LiveStrategyHost + RunRegistry | ✅ 完了 (2026-05-21) — host shell (lifecycle / 所有権 / RunRegistry 連携 / 戦略ロード)。Nautilus engine bridge は seam として Step 3+ に委譲 |
| 3 | gRPC RPC + `BackendEvent` oneof 拡張 (M8) | ✅ 完了 (2026-05-21) — 7 unary RPC + LiveStrategyEvent/SafetyRailViolation/StrategyLogMessage + OrderEvent.strategy_id。engine bridge は placeholder（実発注なし）で mock 疎通。 |
| 4 | Safety Rails (ネイティブ config + 独自 hook) | ✅ 完了 (2026-05-21) — **Nautilus live engine bridge を実装**（NautilusKernel + 実 LiveExecutionClient over OrderingVenueAdapter）。ネイティブ rail = LiveRiskEngineConfig、独自 rail = safety_rails.py。mock で全 rail 検証。bar 供給は Step 8。 |
| 5 | Bevy UI: Safety Rails Modal + Promote to Live | ✅ 完了 (2026-05-21) — `safety_rails_modal.rs` (UI-node trigger + modal、± ステッパー、Replay KPI)。`PromoteToLive` 1 コマンドが transport で Register→SetExecutionMode(LiveAuto)→Start を順次 await。結果は `LiveStrategyPromoteResult`→`PromoteFeedback`。14 新規 test、lib 515 / bin 30 green。GUI mock E2E は未実施（要手動 verify）。 |
| 6 | Bevy UI: Live Run Panel | ✅ 完了 (2026-05-21) — `live_run_panel.rs` (UI-node、`LiveRuns` resource を `LiveStrategyEvent` から駆動、status/ids/起動時刻 + Pause/Resume/Stop、状態 gating)。3 制御コマンド + main.rs dispatch。telemetry(PnL/件数)は Step 7。lib 526 / bin 30 green。 |
| 7 | OrdersPanel strategy_id フィルタ + LiveRun telemetry | ✅ 完了 (2026-05-21) — manual→`MANUAL-001` / auto→`LIVE-{run}` タグ（kernel msgbus bridge + `change_id`/`change_order_id_tag` で StrategyId 強制）、`LiveStrategyTelemetry`(oneof#8) 新設、orders.rs 循環フィルタ、live_run_panel に PnL/件数。parallel-agent-dev 2 並列（PY/RS）。workspace 全緑（lib547/bin30/integ10、backend_integration の pre-existing Step3 breakage も修復）。 |
| 8 | Partial Bar Push + Live Bar 供給の検証 | ✅ 完了 (2026-05-21) — **本丸**: `NautilusVenueDataClient`(LiveMarketDataClient) を kernel に register し、`LiveRunner` の tick tap 経由で live 約定を `TradeTick` 化 → `LiveDataEngine` internal aggregation → `on_bar`。実 kernel full-path test で OHLCV 一致・spec=INTERNAL を確認。UI partial push は `LiveRunner` 1s `build_now()`→bus。live+strategy_runtime+grpc 489 passed。GUI/実 venue E2E は Step 9。 |
| 9 | Live E2E (Demo / Verify) | ⬜ — 実 venue 認証情報 + 市場稼働時間が必要（自動化不可、要手動）。手順は [phase11-handoff.md](../phase11-handoff.md) §3 |
| 10 | Polish (drawio / docs / Phase 11 引き継ぎ) | ✅ 完了 (2026-05-21) — `assets/phase10-architecture.drawio.svg`（drawio skill / 編集可能 native XML + dark theme 静的 SVG）、`docs/live-strategy.md`（戦略開発者向け: 移植性 / bar 供給 / Safety Rails / Promote・制御）、`docs/phase11-handoff.md`（§7 + Step 4/7/8 既知の限界を集約）。 |

### Step 1 完了サマリー (2026-05-21)

- **成果物**:
  - `python/engine/live/bar_supply.py` [NEW] — `to_internal_bar_type()` / `live_bar_type()`。
    Replay の EXTERNAL `BarType` を Live の INTERNAL に読み替える変換を 1 箇所に集約（ADR-B / §2.3）。
    aggregation 本体は Nautilus 標準 (`data/aggregation.pyx`) を使うため新規実装なし（File Layout の「新ファイル不要」を踏襲）。
  - `python/tests/live/test_live_bar_supply.py` [NEW] — 設計ロック用 PoC + 回帰（7 tests, green）。
- **設計確定 (PoC で構造的に検証)**:
  - 戦略は同一 `BarSpecification`（step/aggregation/price_type）を購読し続け、変わるのは
    `aggregation_source`（EXTERNAL→INTERNAL）のみ。`BarType` 完全一致は成功条件にしない（§5）。
  - Nautilus 標準 `TimeBarAggregator`(INTERNAL) に同一 `TradeTick` 列を流すと、確定 `Bar` の OHLCV が
    手計算（open=最初 / high=最大 / low=最小 / close=最後 / volume=合計）と一致する。
  - `strategy_loader.load()` はクラスを返すだけ（インスタンス化・clock/data 束縛なし）→
    Replay/Live 双方から同じロードを使える（§0′-2 / §0.4）。
- **回帰**: 既存コード無改変（新規ファイル追加のみ）。Replay 系 (`tests/strategy_runtime/`) は baseline 通り green。
- **次の手 (Step 2)**: `LiveStrategyHost` が `to_internal_bar_type` で戦略 `bar_type` を読み替え、
  `LiveDataEngine` に INTERNAL を subscribe させる + `Trader.add_strategy()` で attach。
- **TDD baseline**: Python `-m "not slow"` の pre-existing 失敗は 4 件
  (`test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1、Windows pipe FD 由来)。Step 1 で増減なし。

### Step 2 完了サマリー (2026-05-21)

- **完了した成果物**:
  - `python/engine/live/strategy_state_machine.py` [NEW] — `LiveStrategyStateMachine` (§1.2)。
    IDLE→LOADING→READY→RUNNING→(PAUSED)→STOPPING→STOPPED / ↘ERROR。`is_running`（新規発注ゲート,
    PAUSED は False）/ `is_active` / `is_terminal` / `error(code)`。venue 用 `VenueStateMachine` とは別物。
  - `python/engine/live/run_registry.py` [NEW] — `RunRegistry` (§2.6 / M4 / M6)。
    `max_active_live_auto_runs=1` の単一 run 制約、`(strategy_id, instrument_id)` 重複検出、
    `nautilus_strategy_id → run_id` 逆引き（発注主体識別）。in-memory・永続化なし。
  - `python/engine/live/strategy_host.py` [NEW] — `LiveStrategyHost` の **host shell**。
    `start_run` / `pause_run` / `resume_run` / `stop_run` / `fail_run` が state machine と
    RunRegistry を駆動する。`LiveStrategyHostError(error_code)` で
    `VENUE_LOGIN_REQUIRED` / `STRATEGY_LOAD_FAILED` / `LIVE_STRATEGY_ALREADY_RUNNING` /
    `DUPLICATE_STRATEGY_INSTRUMENT` / `STRATEGY_ATTACH_FAILED` / `UNKNOWN_RUN` を構造化。
  - tests: `test_live_strategy_state_machine.py` (10) / `test_run_registry.py` (10) /
    `test_strategy_host.py` (17)、全 green（live スイート 279 passed、回帰なし）。
- **設計確定**:
  - **session 所有権 = 共有（採用）**: host は `session_provider()` で既存 Phase 9 live
    session（`_live_runner` / adapter）を借用し、未ログインなら `VENUE_LOGIN_REQUIRED`。
    新しい login / WebSocket / order client は作らない（§1.1 ⚠️ の二択を「共有」に固定）。
    手動発注（`MANUAL-001`）と LiveAuto は同じ order stream に同居する（§2.2）。
  - **engine attach は seam に委譲**: host は `LiveEngineController` Protocol
    （`attach` / `detach` / `cancel_inflight_orders`）だけに依存。戦略インスタンス化と
    EXTERNAL→INTERNAL `bar_type` 読み替えは controller の責務（engine_runner が backtest で
    `strategy_cls(**kwargs); add_strategy()` するのと同じ分担）。`fail_run` / `stop_run` の
    in-flight cancel は **当該 `StrategyId` のみ**（§1.3 / M6）。
  - **transport 非依存**: proto を import しない。`token` / `ExecutionMode` 検証と
    strategy_id↔file 解決・path 検証は gRPC layer（Step 3 / §2.5）の責務。
- **未完（次の手 = Step 3 で結線）**: `LiveEngineController` の **実体**——既存
  `OrderingVenueAdapter` を Nautilus `LiveExecutionClient` / `LiveDataClient` に bridge し、
  `Trader` + `LiveDataEngine` + `LiveExecutionEngine` + `LiveRiskEngine` を `_live_runner` の
  live loop 上で起動して `add_strategy()` する controller。これは Phase 10 最大の実装で、
  Step 3（gRPC / RegisterLiveStrategy の strategy_id↔file レジストリ）・Step 4（RiskEngine /
  safety_rails）・Step 8（bar 供給検証）に跨る。async 統合 + server_grpc 周辺の競合に注意。

### Step 3 完了サマリー (2026-05-21)

- **完了した成果物**:
  - `python/proto/engine.proto` — 7 unary RPC（`RegisterLiveStrategy` / `StartLiveStrategy` /
    `StopLiveStrategy` / `PauseLiveStrategy` / `ResumeLiveStrategy` / `GetLiveStrategyStatus` /
    `ListLiveStrategies`）+ message 群（`SafetyLimits` / `LiveStrategyStatus` / 各 Req/Res）。
    `BackendEvent.oneof` に `LiveStrategyEvent`(5) / `SafetyRailViolation`(6) /
    `StrategyLogMessage`(7) を additive 追加。`OrderEvent` に `optional string strategy_id = 8`
    を additive 追加（M6 / M8）。Python pb2/grpc 再生成 + 相対 import 手修正。Rust は build.rs(tonic)
    で自動再生。
  - `python/engine/live/strategy_registry.py` [NEW] — `StrategyRegistry`（§2.5）。
    `register(file, expected_sha256)→StrategyHandle` / `resolve(strategy_id)`。
    `strategy_id = strat-{sha256[:16]}`（内容ハッシュ由来で再登録は冪等）。`StrategyRegistryError`
    で `STRATEGY_FILE_NOT_FOUND` / `STRATEGY_NOT_A_FILE` / `STRATEGY_LOAD_FAILED` /
    `STRATEGY_HASH_MISMATCH` / `UNKNOWN_STRATEGY_ID` を構造化。
  - `python/engine/live/engine_controller.py` [NEW] — `NoopLiveEngineController`（**Step 3
    placeholder**）。Nautilus engine に繋がず attach/detach/cancel を記録するのみ。**注文経路に
    繋がっていないため StartLiveStrategy 成功でも実発注は発生しない**（構造的に安全）。実 bridge は
    Step 3+/4/8。
  - `server_grpc.py` — `RunRegistry` / `StrategyRegistry` / `LiveStrategyHost`(placeholder
    controller) を servicer lifetime で配線。7 handler 実装。`_live_strategy_lock` で単一 run
    スロットの TOCTOU を防止。`_publish_live_strategy_event` で lifecycle 遷移を push。
    `_order_event_to_proto` に `strategy_id` 引数を additive 追加（Step 3 は "" のまま、Step 7 で populate）。
  - `src/trading.rs` / `src/main.rs` / `src/backend_sync.rs` — `BackendEvent` mirror に 3 新 variant
    + `OrderEvent.strategy_id` + `LiveOrder.strategy_id` を additive 追加。main.rs の
    proto→mirror 変換に 3 payload arm 追加。backend_sync の reducer は Step 3 では log のみ
    （UI panel は Step 5-7）。
  - tests: `python/tests/test_grpc_live_strategy.py` [NEW]（12 tests, green）。
- **設計確定（ドリフト訂正）**:
  - **path 検証ポリシー = Replay と同じ**（ユーザー決定）: §2.5 の「許可ディレクトリ配下」allow-list
    は導入せず、Replay の `StartEngine` と同じく `resolve()` + `strategy_loader.load()` で検証する。
    Replay と非対称な root 制約を作らない。`expected_sha256` の TOCTOU ガードは維持。
  - **write 系の mode gate**: `StartLiveStrategy` のみ `ExecutionMode == LiveAuto` を強制
    （`EXECUTION_MODE_PRECONDITION`）。`Pause`/`Resume`/`Stop` は run 存在（`UNKNOWN_RUN`）だけを
    条件にし mode で hard gate しない——runaway を常に止められるようにするための安全寄り判断
    （§2.5 の「write 系すべて LiveAuto 検査」からの意図的ドリフト）。
  - **`strategy_id` の値**: Step 3 は proto/Rust への mirror（field 配線）まで。manual→`MANUAL-001` /
    auto→`LIVE-{run}` の populate と OrdersPanel フィルタは Step 7。
- **回帰**: live スイート 315 passed / Step 2 + strategy_runtime 190 passed / Rust lib 501 passed。
  pre-existing 失敗（`test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1, Windows pipe FD 由来）は
  Step 3 で増減なし。
- **次の手 (Step 4)**: `LiveRiskEngineConfig`（`max_order_value`→`max_notional_per_order` /
  `max_orders_per_minute`→`max_order_submit_rate`）+ `live/safety_rails.py`(独自 pre/post-trade hook)。
  違反を `SafetyRailViolation` event で push（proto/transport は Step 3 で配線済み）。
  これは `LiveEngineController` 実体（Nautilus engine bridge）と一体で進む。

### Step 4 完了サマリー (2026-05-21)

> ユーザー決定: **Nautilus live engine bridge を Step 4 で着手**（Step 3 の placeholder を実体化）。
> Step 3 完了サマリーの「次の手」で予告した「Phase 10 最大の実装」を本 Step で実装した。

- **完了した成果物**:
  - `python/engine/live/safety_rails.py` [NEW] — `SafetyRails` / `SafetyLimits`（transport 非依存）。
    `to_live_risk_engine_config()`（ネイティブ rail）/ `check_pre_trade()`（max_position_size /
    allowed_instruments）/ `check_post_trade()`（max_daily_loss）。`0 = その rail 無効`（数値ポリシーは
    Bevy UI 由来、§0.6）。純粋ロジック（13 unit tests, green）。
  - `python/engine/live/nautilus_exec_client.py` [NEW] — `NautilusVenueExecClient`（`LiveExecutionClient`
    実体）。既存 `OrderingVenueAdapter`（submit/cancel/modify/fetch_account）を Nautilus 発注パイプラインに
    bridge。`_submit_order` は **SUBMITTED 前**に独自 pre-trade rails を評価し、違反なら
    `generate_order_denied` + `on_safety_violation`（venue に送らない）。約定は `OrderResult` →
    `generate_order_submitted/accepted/filled/rejected` に正規化。`_connect` で `generate_account_state`
    を seed（RiskEngine free-balance / Portfolio 用、account_id を採番）。
  - `python/engine/live/engine_controller.py` — `NautilusLiveEngineController` [NEW] を追加（`NoopLiveEngineController`
    は Step 3 plumbing テスト用に残置）。`attach()` で **run ごとに** `NautilusKernel`（Trader +
    LiveExecutionEngine + LiveRiskEngine + LiveDataEngine + Cache + Portfolio + MessageBus + LiveClock）を
    live loop 上に組み、exec client を `register_client`、instrument を cache 登録、`strategy_cls(**kwargs)`
    を `add_strategy`、`kernel.start()`。`loop_provider` / `adapter_provider` で server_grpc の runtime
    resource を共有借用（新規 login / WebSocket は作らない、§1.1）。`detach` は `kernel.stop_async()`。
  - `server_grpc.py` — 既定 controller を `NautilusLiveEngineController` に切替（providers 注入）。
    `StartLiveStrategy` が proto `SafetyLimits` → `SafetyRails` を組み `StartParams.safety_rails` で attach に
    渡す。`_publish_safety_rail_violation`（pre-trade 違反 / post-trade 損失）。post-trade
    `max_daily_loss` は `_publish_account_snapshot`（AccountSync callback, live loop thread）で評価し、
    違反時は **別スレッド**で `fail_run`（同 loop への blocking round-trip 自己デッドロック回避）。
  - `strategy_host.py` — `attach` seam に `safety_rails` を追加（`StartParams.safety_rails` を素通し）。
  - tests: `test_safety_rails.py`（13）/ `test_nautilus_live_exec.py`（5: 実 kernel で within-limit fill /
    native max_notional deny / 独自 position-size deny / allowed_instruments deny / controller lifecycle）/
    `test_grpc_live_strategy.py` に post-trade max_daily_loss 2 件追加。
- **設計確定 / 学び**:
  - **最小 live stack = `NautilusKernel`**（`TradingNodeConfig` で直接構築、`TradingNode` は不要）。live は
    `bypass_logging` 禁止 → `LoggingConfig(log_level="ERROR", log_level_file="OFF")` で cwd への
    `*.log` 散布を防ぐ。
  - **OrderDenied は INITIALIZED からのみ有効**: 独自 rail の deny は `generate_order_submitted` の**前**に
    行う（後だと SUBMITTED→DENIED 不正遷移で注文が SUBMITTED 固着）。ネイティブ rail は RiskEngine が
    submit 前に弾くので問題なし。
  - **発注主体 StrategyId の正確なタグ付け（"LIVE-{run}"）と config= 形式戦略の StrategyConfig 構築は Step 7/8**。
    現状 controller は kwargs 形式戦略（instrument_id / bar_type_str）で attach する。
- **既知の限界 / seam**:
  - market data / bar 供給は Step 8。MARKET 注文の参照価格が cache に無い場合 notional は 0 にフォールバックし
    position-size チェックを保守的にスキップ（テストは LIMIT 注文で notional を確定）。
  - 同一プロセスで live kernel（非 bypass logging）を先に初期化すると、後続の backtest（`bypass_logging=True`）の
    dispose ログ（`InvalidStateTrigger RUNNING->DISPOSE`）が console に漏れる（Nautilus の global logging が
    once 初期化のため）。production は replay / live が別プロセスなので無影響。test の console ノイズのみ。
- **回帰**: live + grpc + strategy_runtime `-m "not slow"` 498 passed / 11 skipped。新規 20 tests green。

#### Step 4 レビュー remediation (2026-05-21)

実装完了後の横断レビューで **Medium ×2 + 潜在バグ ×1** を修正した（Medium 以上ゼロまで反復）。

- **Finding 1 (Medium) — post-trade 評価のデッドロック窓**: `_evaluate_post_trade_loss` は live loop
  thread（AccountSync callback）から走るのに `_live_strategy_lock` を取得していた。同 lock は
  `StopLiveStrategy` / `_fail_run_for_loss` が **live loop への blocking round-trip（`kernel.stop_async`/cancel）
  中ずっと保持**する。teardown 中に account snapshot が来ると「live loop は lock 待ち / lock 保持側は live loop
  待ち」で相互デッドロックし、`.result(timeout=5/10)` のタイムアウトまでハング（しかも timeout した
  `stop_async` は runaway kernel を片付け損ねる）。→ rails dict 専用の `_run_rails_lock`（blocking round-trip を
  伴わない軽量 lock）を新設し、live loop 経路はそちらだけを取得。**不変条件「live loop callback は
  `_live_strategy_lock` を取らない」** をコードコメントで明文化（2 lock は同時保持しないので lock 順序起因の
  別デッドロックも無い）。回帰テスト `test_post_trade_eval_does_not_block_on_live_strategy_lock`。
- **Finding 2 (Medium) — `cancel_inflight_orders` が暗黙 no-op**: `strategy.cancel_all_orders(strategy.instrument_id)`
  を `hasattr(strategy, "instrument_id")` ガードで呼んでいたが、サンプル戦略は `self._instrument_id`（private）や
  `config` 形式で public `instrument_id` を持たず、ガードが常に False → §5 の「max_daily_loss → 当該 StrategyId の
  in-flight order を cancel」が**黙って何もしない**。→ `kernel.cache.orders_open(strategy_id=strategy.id)` で当該 run の
  open 注文を引き `strategy.cancel_order(order)`（instrument 属性に非依存・型差に頑健）。回帰テスト
  `test_cancel_inflight_orders_cancels_by_strategy_id`。
- **Finding 3 (潜在) — controller が起動 instrument_id を無視**: `attach` は kernel cache / RiskEngine に request の
  `instrument_id` を登録するのに、戦略 kwargs は `default_strategy_init_kwargs(scenario)`（**scenario の既定銘柄** +
  EXTERNAL bar_type）で組んでいた。両者が食い違うと戦略が cache 未登録銘柄で発注し破綻し得る。また ADR-B /
  host docstring が定める「controller が INTERNAL bar_type を供給」も未実施（`bar_supply.live_bar_type` が未使用）。
  → kwargs を **request の instrument_id** と `live_bar_type()`（INTERNAL）で組む。回帰テスト
  `test_attach_uses_request_instrument_not_scenario`。
- **simplify パス（3 review agent: reuse / quality / efficiency, medium）**:
  - 死コード `engine_runner.default_strategy_init_kwargs` を削除（Finding 3 で controller が使わなくなり全 caller 消滅）。
  - `cancel_inflight_orders` を open 注文に加え **inflight（submit 済み未 ack）** も cancel するよう拡張
    （teardown 中の取りこぼし防止。両 index は排他で二重 cancel しない）。
  - `_modify_order` の未使用 `instrument` 変数 / `X if X is not None else None` の no-op 三項を除去。
  - rails+baseline の対 pop を `_release_run_rails_locked()` に集約（両 dict を必ず一緒に外す不変条件を担保）。
  - test の background-loop 定型を `_bg_loop`/`_stop_bg_loop` に共通化（lifecycle test も移行）。
- **回帰（remediation + simplify 後）**: live + grpc + strategy_runtime `-m "not slow"` 474 passed / 11 skipped（新規 3 tests 込み）。

- **次の手 (Step 5)**: Bevy UI（Safety Rails モーダル + Promote to Live ボタン）。`SafetyLimits` の入力 UI と
  Replay KPI サマリー（既存 `summary.py` の項目のみ、§5 M3）。

### Step 5 完了サマリー (2026-05-21)

- **完了した成果物**:
  - `src/ui/safety_rails_modal.rs` [NEW] — Promote-to-Live トリガー + Safety Rails モーダル（**Bevy UI Node 流派**、
    `order_panel.rs` / `secret_modal.rs` を手本に Startup spawn + `Node.display` で出し入れ）。トリガーは pre-flight
    （戦略ロード済み + venue 接続 + 銘柄選択）で enabled/disabled（greyed）。押下で Run と同じ merge→`flush_strategy_cache`
    を行い canonical `.py` path を確定してモーダルを開く。モーダルは **± ステッパー**（text 入力ではなく
    `order_panel` の qty/price 流儀、`0 = OFF`）で 4 レール（max_order_value / max_position / max_daily_loss / max_orders_per_min）、
    Replay KPI サマリー（**既存 `summary.py` 項目のみ**: total_pnl / fills / equity_points / status、§5 M3）、
    `allowed_instruments` は起動銘柄 1 件に固定表示。Esc/Cancel で閉じ、Confirm で `TransportCommand::PromoteToLive` を 1 本発射。
    純関数（`preflight_blocker` / `build_safety_limits` / `format_limit_jpy` / `replay_kpi_summary`）+ 14 unit tests。
  - `src/trading.rs` — `SafetyLimitsInput`（transport 非依存ミラー、`0 = rail 無効`）/ `TransportCommand::PromoteToLive`
    （`strategy_file` / `expected_sha256` / `instrument_id` / `venue` / `params` / `safety_limits` / `ensure_live_auto`）/
    `BackendStatusUpdate::LiveStrategyPromoteResult` / `PromoteFeedback` resource を additive 追加。
  - `src/main.rs` — `PromoteToLive` dispatch arm。transport task で **RegisterLiveStrategy → SetExecutionMode(LiveAuto) →
    StartLiveStrategy を順次 await**（3 本を独立コマンドにすると LiveAuto 反映前に Start が走るレースになるため 1 コマンド集約）。
    各段の reject / transport error を `LiveStrategyPromoteResult` で surface。`PromoteFeedback` を insert。
  - `src/backend_sync.rs` — `apply_status_update` / `status_update_system` に `PromoteFeedback` param 追加、
    `LiveStrategyPromoteResult` arm（成功 = run id、reject = error code を notice 文字列に）。
  - `src/ui/mod.rs` — module 登録 + `init_resource`（`PromotePrompt` / `SafetyRailsForm`、`PromoteFeedback` は main.rs）+
    Startup spawn + 専用 Update ブロック（`safety_rails_modal_button_system.before(secret_modal_input_system)` で
    §3.10 Escape determinism を踏襲）。
- **設計確定 / 学び**:
  - **ボタン配置**: 計画書 §2.7 は「strategy_editor.rs にボタン」だが、Replay の Run トリガーは実際には
    `footer.rs` にある（world-space cosmic editor に Interaction/Button を載せない流儀）。Promote も同じ判断で
    **UI-node 常駐ボタン**（自己完結モジュール内）にし、editor 本体は無改変。
  - **結果 surface の二重経路**: 成功は backend が push する `LiveStrategyEvent{status:"RUNNING"}`（Step 6 panel が消費）と
    unary 応答の両方で来る。reject は unary のみなので `PromoteFeedback`（LiveAuto でも見える、OrderFeedback は
    LiveManual 専用）に集約。
  - **mode gate**: Confirm 時に `ensure_live_auto=true` で SetExecutionMode を chain に含め、`StartLiveStrategy` の
    backend `ExecutionMode == LiveAuto` precondition（Step 3）を構造的に満たす。
- **未実施 / 次の手 (Step 6)**: ① **GUI mock E2E**（`[Promote to Live]` → モーダル → Live 起動を実アプリで通す）は未実施＝
  要手動 verify。② `live_run_panel.rs`（アクティブ run 一覧 + Pause/Resume/Stop、`LiveStrategyEvent` を消費）。
  Step 5 で配線済みの `LiveStrategyEvent` / `PromoteFeedback` をパネルが読む。
- **回帰**: Rust lib 515 passed / bin 30 passed（新規 14 込み）。Python は無改変（Step 3/4 の RPC をそのまま叩く）。
  proto 変更なし（RPC は Step 3 で追加済み、Rust は build.rs で既に生成済み）。

### Step 5 後追い (2026-05-21)

- **Promote フィードバック行を常駐表示**: モーダルは Confirm で閉じるため RPC chain の async な
  成功/拒否をモーダル内で出せない。`safety_rails_modal.rs` に `PromoteFeedbackText`（起動ボタン直下の
  常駐行）+ `promote_feedback_sync_system` を追加し、pre-flight ブロック理由 /「起動中…」/ 成功 (run id) /
  構造化 reject (error_code) を `PromoteFeedback.message` から差分表示する。

### Step 6 完了サマリー (2026-05-21)

- **完了した成果物**:
  - `src/ui/live_run_panel.rs` [NEW] — Live Run Panel（**Bevy UI Node 流派**: 制御ボタンを持つので
    world-space sprite ではなく UI-node。Startup spawn + パネル全体を `Node.display` gate）。`LiveRuns`
    resource を行ソースに、各行 status（色分け）/ strategy・run id（短縮）/ 起動時刻（`HH:MM:SS`）+
    `[Pause]`/`[Resume]`/`[Stop]` を表示。状態 gating（Pause=RUNNING のみ / Resume=PAUSED のみ /
    Stop=非終端のみ、無効ボタンはグレー）。Phase 10 は automated run 1 件だが UI は固定 3 行で複数対応の前提。
    純関数（`status_color` / `format_hms` / `short_id` / `can_pause`/`can_resume`/`can_stop`）+ 6 unit tests。
  - `src/trading.rs` — `LiveRunRecord` / `LiveRuns` resource（`apply_event` upsert、`started_ts` は初回固定、
    空 strategy_id は既知値を消さない、`MAX_LIVE_RUNS=8` cap、`active_run_id()`）/ `is_terminal_run_status()` /
    `TransportCommand::{Pause,Resume,Stop}LiveStrategy{run_id}` を additive 追加。4 unit tests。
  - `src/backend_sync.rs` — `backend_event_drain_system` に `LiveRuns` param 追加、`LiveStrategyEvent` arm を
    log のみ → `live_runs.apply_event(...)` に結線（既存テストに `init_resource::<LiveRuns>()` 追従）。
  - `src/main.rs` — `Pause/Resume/StopLiveStrategy` の 3 dispatch arm（spawn して RPC、結果は backend が push する
    `LiveStrategyEvent` で panel に反映＝fire-and-log）。`LiveRuns` を insert。
  - `src/ui/mod.rs` — module 登録 + Startup spawn + 専用 Update ブロック（5 systems）。`LiveRuns` は main.rs insert
    （`LiveOrders` と同じ＝transport-facing）。
- **設計確定**:
  - **telemetry は Step 7**: `LiveStrategyEvent` は {run_id, strategy_id, status, ts_ms} のみで PnL/発注数/約定数を
    運ばない。§2.9 の通り run 別 telemetry は別イベント（Step 7）。Step 6 は lifecycle + 制御に限定。
  - **制御の mode gate なし**: Pause/Resume/Stop は run 存在のみを条件にする（§2.5、runaway を常に止められる）。
    client 側は状態 gating（UX）で送信を間引くが、backend が最終ゲート。
- **回帰**: Rust lib 526 passed / bin 30 passed（新規 10 + Step 5 後追いの feedback 行）。proto/Python 無改変。
- **次の手 (Step 7)**: `OrderEvent.strategy_id` を populate（manual→`MANUAL-001` / auto→`LIVE-{run}`、Python 側）+
  `orders.rs` に strategy_id フィルタ + run 別 telemetry イベント（PnL/件数）を `LiveRunPanel` に表示。

### Step 7 完了サマリー (2026-05-21)

> オーケストレーション: **parallel-agent-dev** で proto 凍結（直列）→ PY（`python/`）/ RS（`src/`）の 2 エージェント並列。
> 言語境界で完全分離（ファイル非重複）。共有 artifact（proto/pb2）はオーケストレーターが先に凍結・再生成・相対 import 修正。

- **完了した成果物**:
  - `python/proto/engine.proto` — `LiveStrategyTelemetry`（run_id/strategy_id/realized_pnl/unrealized_pnl/
    order_count/fill_count/ts_ms）を `BackendEvent.oneof` field 8 に additive 追加（§2.9）。pb2 再生成 + 相対 import 手修正。
    Rust は build.rs(tonic) で自動再生。
  - `python/engine/live/engine_controller.py` — **(B) StrategyId 強制**: `NautilusLiveEngineController._do_attach` で
    `add_strategy` の**前**に `strategy.change_id(StrategyId(nautilus_strategy_id))` + `change_order_id_tag(run短縮)` を呼ぶ。
    **学び**: `StrategyConfig.strategy_id` 単独では `Strategy.__init__` が `{ClassName}-{tag}` を必ず付けて不一致。
    さらに `Trader.add_strategy` は `order_id_tag in (None,"None")` のとき id を `{head}-{NNN}` に**再採番**する
    （trader.py:407-413、"LIVE-ab12cd34"→"LIVE-000"）ため `change_order_id_tag` も必須。これで `str(strategy.id)==nautilus_strategy_id`。
    Replay（engine_runner）は change_id を呼ばないので従来の ClassName 由来のまま（id 強制は Live 経路に閉じる）。
    **(C) kernel→UI bridge**: `kernel.msgbus.subscribe(topic=f"events.order.{nautilus_strategy_id}", handler=...)`。
    handler は OrderEvent を受け `cache.order(client_order_id)` から `OrderEventData` を組み `on_order_event(ev, strategy_id)` を呼ぶ。
    best-effort（例外は log のみ）、live loop thread・外側 lock 非取得（Step4 不変条件）。
    **(D) telemetry 算出**: order_count=`cache.orders()` 件数、fill_count=filled_qty>0 件数（単一 kernel=単一 run, §0.7）、
    realized/unrealized=`portfolio.realized_pnls/unrealized_pnls(venue, target_currency=JPY)` を JPY 合算（空 dict→0.0）。
  - `python/engine/server_grpc.py` — **(A) タグ規則**: 定数 `MANUAL_STRATEGY_ID="MANUAL-001"`。manual unary 応答 5 経路
    （PlaceOrder/CancelOrder/ModifyOrder/GetOrderStatus/GetOrders）を `_order_event_to_proto(ev, strategy_id=MANUAL_STRATEGY_ID)` でタグ。
    adapter EC stream（`_publish_order_event`）は `""` のまま（共有 adapter、UI 側「非空が勝つ」マージに委ねる）。
    `_on_auto_order_event`（bridge → `LIVE-{run}` タグ push）/ `_on_auto_telemetry`（`run_id_for_nautilus_strategy` で逆引き →
    `LiveStrategyTelemetry` push）を controller に注入。
  - `src/trading.rs` — `BackendEvent::LiveStrategyTelemetry` variant、`LiveRunRecord` に telemetry 4 フィールド、
    `LiveRuns::apply_telemetry`（lifecycle 前後不問 upsert・非空 strategy_id が勝つ・started 初回固定）、
    `LiveOrders::apply_event` に `strategy_id` 引数（**非空が勝つ・空は既知値を消さない**＝venue_order_id と同パターン）、
    `LiveOrders::filtered`/`distinct_strategy_ids`、`OrdersFilter` resource + 純関数（`order_matches_filter`/`next_filter`/
    `filter_label`/共有 `short_id`）、`BackendStatusUpdate::OrderSeeded` に `strategy_id`。
  - `src/main.rs` — proto→mirror の `LiveStrategyTelemetry` arm（網羅 match 維持）、PlaceOrder 応答→`OrderSeeded` に
    `strategy_id: ev.strategy_id.unwrap_or_default()`。
  - `src/backend_sync.rs` — `OrderEvent` arm で `strategy_id` を `apply_event` に渡す、`LiveStrategyTelemetry` arm →
    `apply_telemetry`、`OrderSeeded` arm で受領 strategy_id を使用。
  - `src/ui/orders.rs` — フィルタラベルセル + クリック Sprite（**world-space パネルなので bevy-engine 流儀に従い
    UI-node Button ではなく `OrdersRowHit` と同流の透明 Sprite + `Pointer<Down>` 観測子で循環切替**）。
    **正しさ**: 表示 (`orders_panel_system`) と右クリックメニュー (`OrdersRowHit` 観測子) の両方が `LiveOrders::filtered(&filter)` を
    index するので、フィルタ後も行 N→注文 N がズレない。Replay 経路（`PortfolioState.orders`）はフィルタ非適用で挙動不変。
  - `src/ui/live_run_panel.rs` — `LiveRunRow` を 3 行構成に拡張（status/ids/started → telemetry(PnL 符号色 + `o:n f:m`) → 制御ボタン）。
    `format_pnl`/`pnl_color`/`format_counts`。
  - `tests/backend_integration.rs` — **(オーケストレーター修復)** Step 3 が proto に追加した 7 live-strategy RPC を mock
    `MyDataEngine` が未実装で `cargo test --workspace` が Step 3 以来コンパイル不能だった（lib/bin 個別実行では露見せず）。
    token-gate スタブ 7 メソッドを追加して workspace を緑に戻した。
- **設計確定 / タグ規則（不変条件）**:
  - OrderEvent.strategy_id = manual→`"MANUAL-001"` / auto→`"LIVE-{run[:8]}"`（= run の Nautilus StrategyId、`LiveRunRecord.strategy_id` と同値）/
    未タグ EC stream→`""`。UI マージは **非空が勝つ・空は既知値を消さない**（unary 応答/auto bridge がタグした行を後続の空 EC イベントが上書きしない）。
- **既知の限界 / 次工程**:
  - **unrealized_pnl**: 市場データ未供給（Step 8 前）では建玉の mark-to-market 不可 → 0.0。Step 8 で bar/価格供給後に意味を持つ。
  - **二重報告**: 実 venue では同一約定が共有 adapter EC stream（空 strategy_id）でも届き得るが client_order_id マージ + 非空優先で
    `LIVE-{run}` を保持（mock は EC stream 非発火のため Step 7 テスト範囲では二重化しない）。Step 9 の実 venue E2E で要確認。
  - telemetry は order event 都度更新（account snapshot 連動の unrealized 再計算は Step 8 で追加が自然）。
- **回帰（オーケストレーター実地検証）**: Python `python/tests/{live,strategy_runtime}` + `test_grpc_live_strategy` = **483 passed / 33 skipped / 0 failed**
  （pre-existing flaky `test_secret_vault::test_double_submit...` は再実行で緑 = タイミング由来・Step 7 無関係。除外対象の
  `test_grpc_shutdown`×3 / `test_grpc_startup_sentinel`×1 は非対象）。Rust `cargo test --workspace` = **lib 547 / bin 30 /
  backend_integration 10、全緑**。clippy: Step 7 delta はクリーン（残存警告は rust-1.93 toolchain 由来の pre-existing で
  ~30 ファイルに分散＝Step 7 以前から `-D warnings` は workspace 全体で失敗状態、別途 toolchain cleanup タスク）。
- **未実施 / 次の手 (Step 8)**: ① **GUI mock E2E**（OrdersPanel フィルタ + LiveRunPanel telemetry を実アプリで目視）は要手動 verify。
  ② Partial Bar Push（`aggregator.py` `build_now()` を 1 秒間隔で `LiveEventBus` に push）+ Strategy 供給経路（Nautilus
  aggregation 由来 `Bar` が `on_bar` に届く）の検証 + Replay/Live で同 `BarSpecification`・OHLCV 一致の回帰テスト。
  Step 7 で配線した telemetry の unrealized は Step 8 の価格供給後に意味を持つ。

### Step 8 完了サマリー (2026-05-21)

> Phase 10 の **本丸**: Step 2–4 で seam に委ねていた「live tick → Nautilus `Bar` → 戦略 `on_bar`」を結線した。
> proto / Rust / engine.proto は無改変（純 Python）。

- **完了した成果物**:
  - `python/engine/live/bar_supply.py` — `trades_update_to_trade_tick(trade, instrument, seq)` を追加
    （venue `TradesUpdate` → Nautilus `TradeTick`）。price/size は `make_price`/`make_qty` で precision 丸め、
    aggressor は buy/sell→BUYER/SELLER（欠落/不明は NO_AGGRESSOR）、`trade_id={ts_ns}-{seq}`（同 ns 衝突回避）。純関数。
  - `python/engine/live/nautilus_data_client.py` [NEW] — `NautilusVenueDataClient`(`LiveMarketDataClient`)。
    `_connect`/`_disconnect`/`_subscribe_trade_ticks`/`_subscribe_quote_ticks` は no-op（共有 session は新規接続しない、§1.1）。
    `feed_trades_update(trade)` が cache の instrument を引いて `TradeTick` 化し `_handle_data()`
    （= `msgbus.send("DataEngine.process", tick)`、接続状態に非依存）。engine が trades topic に publish →
    aggregator が確定 `Bar` を組み戦略の `on_bar` へ。
  - `python/engine/live/live_runner.py` — ① **tick tap**: `add_tick_listener`/`remove_tick_listener`。
    `_run` が各 `TradesUpdate` を bus publish 後に listener へ同期配布（best-effort、live loop thread）。
    ② **partial bar push**: `partial_push_interval_s>0` のとき `build_now()` のスナップショットを一定間隔で bus に publish
    する `_partial_push` タスク（`start` で起動 / `stop` で cancel）。**既定 0（無効）**で既存テスト不変、production は
    server_grpc が **1.0s** を渡す（§2.3 UI 用 partial bar）。
  - `python/engine/live/engine_controller.py` — `NautilusLiveEngineController` に `runner_provider` を追加。
    `_do_attach` で **kernel.start() の前**に `NautilusVenueDataClient` を `data_engine.register_client`
    （戦略 on_start の `subscribe_bars(<...-INTERNAL>)` が SubscribeTradeTicks を発行する時点で client 必須）。
    start 後に `await runner.subscribe(instrument_id)`（idempotent）+ instrument フィルタ付き tick listener を登録。
    detach/teardown で listener を外す。runner が無い構成（テスト直結 kernel）では tap を張らない。
  - `python/engine/server_grpc.py` — controller に `runner_provider=self._live_runner_provider` を注入、
    `LiveRunner(..., partial_push_interval_s=1.0)`。
  - tests: `test_live_data_client.py` [NEW]（変換 ×2 / 実 kernel full-path で `on_bar` OHLCV 一致 + spec=INTERNAL ×2(slow) /
    controller wiring が runner に listener を張り feed する ×1）+ `test_live_runner.py` に partial-push ×1。
- **設計確定 / 学び**:
  - **INTERNAL bar 供給の必須前提**: 戦略が INTERNAL `BarType` を購読すると `DataEngine` が aggregator を作り、
    当該 venue 宛に `SubscribeTradeTicks` を発行する（`data/engine.pyx:_subscribe_bar_aggregator`）。よって
    **その venue の data client が register 済みでないと aggregator が作られず `on_bar` が永遠に来ない**。
    Step 4 まで data client 未登録だったため auto 戦略の `on_bar` は不発だった（= Step 8 が解決した本質）。
  - **`_handle_data` は接続非依存**（`data/client.pyx:1247` = `msgbus.send` のみ）→ kernel start 済みなら
    tick 注入が確定バーまで届く。`subscribe_trade_ticks` の coroutine は no-op でよい（INTERNAL 集約は engine が駆動）。
  - **時間バーの close は LiveClock タイマー駆動**（tick の ts_event ではない）。決定論的な full-path テストは
    SECOND バー + 実時間 settle で行い、`build_with_no_updates=True` 由来の空バーと区別するため「volume>0 の bar」を
    検証対象にする。MINUTE の OHLCV 正しさは Step 1 の TestClock テストが別途ロック済み。
  - **venue 別入力 (M2)**: Tachibana の EC 約定はそのまま `TradesUpdate`。kabu は約定 tick が無く板 `CurrentPrice`/
    polling から `TradesUpdate` 相当を構成する必要がある（精度限界 §8 Open Risk 5）。本経路は `TradesUpdate` を
    受ければ venue 非依存に動く。
- **回帰**: Python `tests/{live,strategy_runtime}` + `test_grpc_live_strategy` = **489 passed / 33 skipped / 0 failed**
  （Step 7 の 483 + 新規 6）。Rust / proto は無改変（純 Python のため `cargo` 再検証不要）。`InvalidStateTrigger
  RUNNING→DISPOSE` の ERROR 行は Step 4 既知の console ノイズ（同一プロセスで live kernel が global logger を
  once 初期化 → 後続 backtest の dispose ログが漏れる）で失敗ではない。
- **未実施 / 次の手 (Step 9)**: ① Tachibana Demo + `mean_reversion_01` 等で 1 営業日 Live 稼働、② kabu Verify でも E2E
  （約定 tick 無し前提の精度限界を確認）、③ GUI mock E2E（Promote→bar→on_bar→発注→OrdersPanel/LiveRunPanel）の手動 verify。
  実 venue では同一約定が共有 adapter EC stream（空 strategy_id）でも届き得る二重報告を要確認（Step 7 既知）。

#### Step 8 review remediation (2026-05-21)

実装完了後の横断レビュー（code-review skill = simplify 相当、3 agent: reuse / quality / efficiency）で
efficiency findings を修正した（reuse は「既存 bridge/converter パターン踏襲で clean」）。

- **per-tick の関数内 import を撤去**: `bar_supply` の `TradeTick` / `TradeId` / `AggressorSide` を module scope に
  hoist（`catalog_data_loader` 経由で既に nautilus が読まれるため新規 import コスト無し）。tick ごとの import lookup を消す。
- **per-tick の `InstrumentId.from_str` をメモ化**: `NautilusVenueDataClient` に `_iid_cache`（venue id → InstrumentId）を
  追加し、ホットパスの parse を 1 回に。
- **partial bar push の no-op flood を抑止**: `LiveRunner._partial_push` に変更検出ガード（`_last_partial`、
  (instrument_id, interval 順位) → KlineUpdate）を入れ、静かな相場で同一スナップショットを毎秒 publish しないようにした。
  併せて `_partial_push` の `except BaseException` を `except Exception` に絞り CancelledError 以外の system 例外を握り潰さない。
- **skip した findings**: `self._runner` 保持（add/remove を同一 runner で対称にする方が listener lifecycle 上安全）、
  `self._strategy_id_str`（Step 4/7 の既存コードで scope 外）、コメントの Step/§ 参照（リポジトリの house style に準拠）。
- **回帰**: refactor 後 `tests/live` + `test_grpc_live_strategy` = **335 passed**。

### Step 10 完了サマリー (2026-05-21)

> ユーザー決定: Step 9（実 venue E2E）は demo/verify 認証情報 + 市場稼働時間を要し自動実行不可のため後回しにし、
> 自律実行可能な Step 10（Polish）を先に完了。

- **完了した成果物**:
  - `docs/assets/phase10-architecture.drawio.svg` [NEW] — **drawio skill** で生成（編集可能な native
    `mxGraphModel` を `content` に埋め込み + phase9 と同じ dark theme の静的 SVG レンダリング。単一ソース
    （node/edge 定義）から両表現を生成して同期）。UI / 制御（gRPC → LiveStrategyHost + RunRegistry/StrategyRegistry）
    / Events single channel / NautilusKernel（Trader+Strategy / LiveDataEngine / LiveExecutionEngine / LiveRiskEngine+safety_rails）
    / bar 供給（Replay EXTERNAL vs Live: Venue→LiveRunner tap→DataClient→INTERNAL aggregation→on_bar）を図示。
    凡例: 青=command / 緑=event / 橙=bar・tick データ / 紫破線=UI 専用 partial bar。
  - `docs/live-strategy.md` [NEW] — 戦略開発者向け: ①移植性（モード無分岐 / register 時注入）、②Live 可搬な戦略の
    書き方（kwargs 形式 / StrategyConfig 形式 / bar_type は EXTERNAL を書けばホストが INTERNAL 読み替え）、
    ③bar 供給（on_bar は確定バーのみ / partial は UI 専用 / venue 別精度）、④Safety Rails（ネイティブ + 独自、`0=OFF`）、
    ⑤Promote フロー + run 制御、⑥やってはいけないこと chk。`REQUIRES_DEPTH` は未実装と明記（vaporware を書かない）。
  - `docs/phase11-handoff.md` [NEW] — §7 の handoff 表 + Step 4/7/8 で判明した既知の限界（kabu 約定 tick 無し /
    unrealized PnL の account 連動 / 共有 EC stream の二重報告 / ログ汚染）+ 未実施（Step 9 / GUI E2E / clippy cleanup）+
    Phase 10 で確立した設計資産を集約。
- **設計確定 / 学び**:
  - **drawio `.drawio.svg` は単一ソース生成が正解**: phase9 は静的 SVG に title だけの最小 content だったが、
    本図は memory note「editable native XML を使う」に従い **完全な mxGraphModel を埋め込み**つつ、静的 SVG と
    乖離しないよう 1 つの node/edge 定義から両方を生成した（手書き二重管理を回避）。
  - **CLAUDE.md mandated `simplify` skill が不在**: `.claude/skills/` に `simplify` ディレクトリが無く、
    機能同等の `code-review` で代替した。CLAUDE.md の指示と実体が乖離（要 skill 新規作成 or CLAUDE.md 修正、別件）。
- **Phase 10 全体の状態**: Step 1–8 + Step 10 完了。**Step 9（実 venue E2E）と GUI mock E2E のみ未実施**
  （いずれも認証情報・市場稼働時間・人手 verify を要するため自動化不可、引き継ぎ済み）。
