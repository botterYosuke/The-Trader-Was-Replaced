# Phase 10: Replay-to-Live Strategy Execution — Implementation Plan

> **前提**: Phase 9 (Live Account & Order API) が完了し、手動発注経路・口座同期・SecretVault が動作する状態を出発点とする。Phase 10 では **戦略コードからの自動発注** を初めて有効化し、Replay で検証した `Strategy` をファイル編集なしに Live 環境にプロモートする経路を完成させる。
>
> 上位計画 [Transparent Headless Replay](./archive/Tranceparent%20Headless%20Replay.md) の §Phase 10 「Replay-to-Live Strategy Execution」を具体化する。Phase 8 ADR「Replay と Live Auto のデータソース非対称性（Phase 10 への前提制約）」および Phase 9 §8 「Phase 10 への引き継ぎ事項」を引き継ぐ。

---

## Goals

- **Strategy Portability**: Replay と Live Auto で **同一の `Strategy` サブクラス** を共有。環境依存の注入 (時刻ソース / データソース / Venue ID) は外部から行い、戦略本体は環境非依存に保つ
- **Promote to Live**: Strategy Editor で編集中の `.py` ファイルを `[Promote to Live]` ボタンで Live Auto モードにデプロイし、`StartLiveStrategy` RPC で起動できる
- **Safety Rails**: 未認証 / モード不一致 / 余力不足 / 注文金額超過 / 同一戦略の二重起動 を **構造的に防止**
- **Live Run Telemetry**: Live 実行中のイベント (fills / position / PnL) を Replay と同じ Snapshot Reducer 経由で UI に流し、両モードで同じ UI コードを使う

## Non-Goals

- **複数戦略の同時 Live 実行** — Phase 10 では「1 戦略 1 Live run」に制約。複数戦略並列は Phase 11
- **戦略の hot-reload** — Live 稼働中に `.py` を編集した場合、明示停止→再起動が必須。差分適用は対象外
- **Replay 中の戦略パラメータ最適化 (grid search / Optuna)** — Phase 11 以降
- **戦略パフォーマンスダッシュボード (KPI 集計 / シャープレシオ等)** — Phase 11 以降。Phase 10 では生イベント表示のみ

---

## 0. Feature Inventory

### 0.1 Strategy Loading

- `LoadStrategy(file_path, mode)` (Phase 7 既存) を拡張: `mode` パラメータに `"replay"` / `"live_auto"` を許可
- `live_auto` モードでは `LiveStrategyHost` (新設) が Nautilus `StrategyEngine` を有効化して Strategy を attach
- 同じ `.py` モジュールが `replay_runner.py` / `live_runner.py` の両方からロード可能であることを保証

### 0.2 Promote to Live フロー

- Strategy Editor (Phase 7.2) に `[Promote to Live]` ボタンを追加
- クリック時の前提条件チェック:
  1. Venue ログイン済み (`VenueState == CONNECTED`)
  2. ExecutionMode が `LiveAuto` または切替可能
  3. 戦略ファイルがディスク保存済み (unsaved changes が無い)
  4. Safety Rails の事前検証 (position size 上限 / 注文金額上限が設定済み)
- すべて満たすと **2 段階確認モーダル** → Replay 結果サマリー (バックテスト KPI) を表示 → `[Confirm]` で `StartLiveStrategy` RPC 発射

### 0.3 Live Strategy Control

- `StartLiveStrategy(strategy_id, instrument_id, venue, params, safety_limits)` — Live 戦略起動
- `StopLiveStrategy(run_id)` — graceful 停止 (在庫ポジションは残す、ユーザー判断で別途決済)
- `PauseLiveStrategy(run_id)` — 戦略の `on_bar` / `on_tick` 呼び出しを一時停止 (新規発注は止まるが既存注文は維持)
- `ResumeLiveStrategy(run_id)` — 再開
- `GetLiveStrategyStatus(run_id)` — 状態取得 (RUNNING / PAUSED / STOPPED / ERROR)

### 0.4 Strategy Portability Layer

- `python/engine/strategy_loader.py` を Replay / Live 両対応に拡張
- 環境依存の注入ポイント:
  - `Clock` — Replay は `TestClock`、Live は `LiveClock`
  - `DataEngine` instance — Replay は `BacktestDataEngine`、Live は `LiveDataEngine`
  - `Venue` — Replay は `SIM`、Live は `TACHIBANA` / `KABUCOM`
  - `Instrument` registry — Replay は J-Quants 既製品、Live は venue から取得した最新
- Strategy 本体は `self.config` から `clock`, `data_engine`, `venue` を取得するため、コード変更なしで両モードで動作

### 0.5 Data Source 非対称性の吸収

Phase 8 ADR で定義された制約:
- Replay: J-Quants OHLCV バー (分足含む) が既製品で存在、板情報なし
- Live Auto: tick / board depth のみ、分足は `aggregator.py` が tick から集約

Phase 10 で必要な追加実装:
- **Bar Builder の精度向上** — Phase 8 の `live/aggregator.py` (Nautilus `BarBuilder` ラッパ) を強化
  - Partial bar push (バー完成前の現在足を 1 秒間隔で push) を有効化
  - Replay 側も J-Quants バーをそのまま流すのではなく `BarBuilder` を経由させ、Live と同じイベント形状にする
- **戦略の depth 参照可否を declare**:
  - Strategy クラスに `REQUIRES_DEPTH: ClassVar[bool] = False` を定義
  - `True` の戦略は Replay モードでロード時に warning `STRATEGY_REQUIRES_DEPTH_REPLAY_UNAVAILABLE` を表示 (動作はするが depth は空)
  - Live Auto モードでは `True` の戦略は depth subscription を自動有効化

### 0.6 Safety Rails

戦略起動前に Python `RiskEngine` に登録される制約:
- `max_position_size_jpy` — 1 銘柄あたりの最大ポジション金額 (default: 100 万円)
- `max_order_value_jpy` — 1 注文あたりの最大金額 (default: 50 万円)
- `max_daily_loss_jpy` — 1 日あたりの最大損失額 (超過で戦略を自動停止、default: 10 万円)
- `max_orders_per_minute` — 流量制限 (default: 5)
- `allowed_instruments` — 取引可能銘柄のホワイトリスト (default: 戦略起動時に指定した instrument_id のみ)

これらは `StartLiveStrategy` RPC の `safety_limits` パラメータで指定。Default 値は Bevy 側で設定 UI を提供。

### 0.7 Run 管理

- `RunRegistry` (Python in-memory) で run_id (UUID) ベースに Live run を管理
- 1 戦略あたりの同時 Live インスタンスは **1 つに制限** (重複 `StartLiveStrategy` は `STRATEGY_ALREADY_RUNNING` で reject)
- Replay run と Live run は別 namespace で管理 (Replay は session_id、Live は run_id)

---

## 1. Architecture / 構成

### 1.1 Process Layout (Phase 9 からの差分)

```
live_runner.py (Phase 9 で発注経路を持つ)
├── DataEngine        (Phase 8)
├── ExecEngine        (Phase 9)
├── RiskEngine        (Phase 9 軽量 → Phase 10 で Safety Rails 強化)
└── StrategyEngine    (Phase 10 [NEW]) ← Strategy インスタンスを attach
    └── LiveStrategyHost  (Phase 10 [NEW])
        ├── Strategy   (ユーザー定義、Replay と共有)
        ├── Clock      (LiveClock)
        └── Cache      (Live venue 由来の Order / Position / Account)
```

### 1.2 State Machine

```
LiveStrategyStateMachine (Phase 10 [NEW])
  IDLE → LOADING → READY → RUNNING → (PAUSED) → STOPPING → STOPPED
                                  ↘ ERROR (safety rail violation / venue error)
```

- `READY` 状態: Strategy がロード済み、Safety Rails 設定済み、まだ market data を流していない
- `RUNNING` → `PAUSED`: `on_bar` / `on_tick` 呼び出しを停止 (既存注文の管理は継続)
- `ERROR` 遷移時: `StopLiveStrategy` を内部発射 → 全 in-flight order を cancel → run を STOPPED に

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
[gRPC: StartLiveStrategy(strategy_id, instrument_id, venue, params, safety_limits)]
   ↓
[Python: live_runner.py]
   │ (a) strategy_loader.py で .py を Live コンテキストでロード
   │ (b) StrategyEngine に attach
   │ (c) RiskEngine に safety_limits を登録
   │ (d) DataEngine から該当 instrument_id の subscription を有効化
   │ (e) run_id 採番 → RunRegistry に登録
   │ (f) state: READY → RUNNING
   ↓
[EventStream: LiveStrategyEvent{run_id, status, ts_ms}]
   ↓
[Bevy UI: Footer に Run Badge 表示]
```

---

## 2. Tasks

### 2.1 Backend: Strategy Portability Layer

- `python/engine/strategy_loader.py` を拡張
  - 既存 Replay 経路 (Phase 6) を維持しつつ `mode="live_auto"` を追加
  - 環境依存の注入を `StrategyConfig` dataclass にまとめ、Strategy `__init__` に渡す
  - Strategy 本体が `self.clock.utc_now()` / `self.data.subscribe_bars(...)` のように依存を経由するパターンを推奨
- 既存の Phase 7 サンプル戦略 (`scenarios/ma_cross.py` 等) を Portability Layer 経由でロードできるよう refactor (互換性破壊は許容、Phase 7 段階では未稼働の想定)

### 2.2 Backend: LiveStrategyHost

- `python/engine/live/strategy_host.py` を新設
- `StrategyEngine` のインスタンス化、`Strategy` の attach / detach、state machine の管理
- 戦略 lifecycle hook の橋渡し:
  - `on_start()` — `READY → RUNNING` 遷移時
  - `on_stop()` — `STOPPING → STOPPED` 遷移時
  - `on_bar()` / `on_tick()` — DataEngine からのイベント
  - `on_order_filled()` / `on_order_canceled()` — ExecEngine からのイベント

### 2.3 Backend: Bar Builder 強化

- `python/engine/live/aggregator.py` (Phase 8) に partial bar push を追加
- Replay 側も `BarBuilder` 経由に統一: `python/engine/replay_runner.py` に J-Quants OHLCV を `BarBuilder.handle_trade_tick()` 相当に変換するアダプタを追加
- これにより Replay / Live で `Strategy.on_bar()` に渡るイベント形状が完全に一致する

### 2.4 Backend: RiskEngine 強化 (Safety Rails)

- `python/engine/live/risk_engine.py` (Phase 9 軽量実装) を拡張
- 事前チェック (pre-trade):
  - `max_position_size_jpy`: 既存ポジション + 新規注文後の合計金額が上限以内か
  - `max_order_value_jpy`: 1 注文の金額が上限以内か
  - `max_orders_per_minute`: token bucket で流量制限
  - `allowed_instruments`: instrument_id がホワイトリスト内か
- 事後チェック (post-trade):
  - `max_daily_loss_jpy`: 当日の realized + unrealized P&L が上限を下回ったら `LiveStrategyStateMachine.error("MAX_DAILY_LOSS_EXCEEDED")` を発射
- 違反は `OrderRejected` イベントで戦略に通知、UI には `EventStream: SafetyRailViolation{run_id, kind, detail}` を push

### 2.5 Backend: gRPC RPC 追加

```proto
service DataEngine {
  // 既存 RPC...

  // Phase 10
  rpc StartLiveStrategy(StartLiveStrategyReq) returns (StartLiveStrategyRes);
  rpc StopLiveStrategy(StopLiveStrategyReq) returns (StopLiveStrategyRes);
  rpc PauseLiveStrategy(PauseLiveStrategyReq) returns (PauseLiveStrategyRes);
  rpc ResumeLiveStrategy(ResumeLiveStrategyReq) returns (ResumeLiveStrategyRes);
  rpc GetLiveStrategyStatus(GetLiveStrategyStatusReq) returns (LiveStrategyStatus);
  rpc ListLiveStrategies(google.protobuf.Empty) returns (stream LiveStrategyStatus);
}

message StartLiveStrategyReq {
  string strategy_file = 1;
  string instrument_id = 2;
  string venue = 3;
  map<string, string> params = 4;
  SafetyLimits safety_limits = 5;
}

message SafetyLimits {
  int64 max_position_size_jpy = 1;
  int64 max_order_value_jpy = 2;
  int64 max_daily_loss_jpy = 3;
  int32 max_orders_per_minute = 4;
  repeated string allowed_instruments = 5;
}

// EventStream に追加されるイベント:
//   - LiveStrategyEvent{run_id, status, ts_ms}
//   - SafetyRailViolation{run_id, kind, detail, ts_ms}
//   - StrategyLogMessage{run_id, level, message, ts_ms}  // Strategy 内 self.log.info() の中継
```

### 2.6 Backend: RunRegistry

- `python/engine/live/run_registry.py` を新設
- `register(run_id, strategy_file, ...)` / `unregister(run_id)` / `get(run_id)` / `list_active()`
- 永続化なし (in-memory)。プロセス再起動時は全 run が消える (戦略本体は venue 側に注文が残る可能性あり、要 UI 警告)

### 2.7 UI: Strategy Editor `[Promote to Live]` ボタン

- `src/ui/strategy_editor.rs` (Phase 7.2) にボタン追加
- 前提条件チェック (§0.2) → 失敗時はエラートースト
- 成功時に Safety Rails 設定モーダル (`src/ui/safety_rails_modal.rs` 新設) を開く
- モーダルで Safety Rails 入力 + Replay KPI サマリー表示 (直近 Replay 結果を Cache から取得) → `[Confirm]` で `StartLiveStrategy` RPC

### 2.8 UI: Live Run Panel

- `src/ui/live_run_panel.rs` を新設
- アクティブな Live run の一覧表示 (Phase 10 は 1 戦略制約だが UI は将来複数対応を想定)
- 各 run の状態 (RUNNING / PAUSED / ERROR)、起動時刻、累積 P&L、発注数、約定数
- `[Pause]` / `[Resume]` / `[Stop]` ボタン

### 2.9 UI: 既存 PositionsPanel / OrdersPanel の run_id フィルタ

- Phase 9 では「Live で発生した全 Order / Position」を表示するだけだったが、Phase 10 では複数の発注主体 (手動 / Strategy A / Strategy B) が並ぶ可能性がある
- 各 Order / Position に `source_run_id` メタデータを付与 (手動発注は `null`)
- PositionsPanel / OrdersPanel に「絞り込み: All / Manual / Strategy: XXX」ドロップダウンを追加

### 2.10 UI: SafetyRailViolation トースト

- `SafetyRailViolation` イベントを受信したら Footer 右下に warning トースト
- 違反種別ごとに色分け (max_daily_loss は赤、max_orders_per_minute は黄等)

---

## 3. File Layout

```
python/engine/
├── live_runner.py                  # StrategyEngine 有効化
├── strategy_loader.py              # mode="live_auto" 対応に拡張
├── replay_runner.py                # BarBuilder 経由に統一
├── live/
│   ├── strategy_host.py    [NEW]   # LiveStrategyHost (state machine + attach/detach)
│   ├── run_registry.py     [NEW]   # in-memory run 管理
│   ├── risk_engine.py              # Phase 9 拡張 (Safety Rails 強化)
│   └── aggregator.py               # partial bar push 追加

src/ui/
├── strategy_editor.rs              # [Promote to Live] ボタン追加
├── safety_rails_modal.rs   [NEW]   # Safety Rails 設定 + Replay KPI 表示
├── live_run_panel.rs       [NEW]   # アクティブ run 一覧 + 制御
├── positions_panel.rs              # source_run_id フィルタ追加
└── orders_panel.rs                 # source_run_id フィルタ追加
```

---

## 4. Implementation Order

各 Step 完了時点で `cargo run` 可能を維持。Mock 経由で発注テストできるよう、Step 1 で MockVenueAdapter にも戦略 attach の経路を通す。

1. **Step 1 — Strategy Portability Layer + BarBuilder 統一**
   - `strategy_loader.py` の `mode="live_auto"` 対応
   - Replay 側を `BarBuilder` 経由に refactor
   - 既存 Phase 6 サンプル戦略が Replay モードで従来通り動作することを回帰確認
2. **Step 2 — LiveStrategyHost + RunRegistry**
   - `live/strategy_host.py` 実装、state machine 単体テスト
   - `live/run_registry.py` 実装
3. **Step 3 — gRPC RPC + EventStream イベント追加**
   - `StartLiveStrategy` / `StopLiveStrategy` / `Pause` / `Resume` / `GetStatus` 実装
   - MockVenueAdapter で疎通テスト
4. **Step 4 — Safety Rails (RiskEngine 強化)**
   - max_position / max_order / max_orders_per_minute (pre-trade) 実装
   - max_daily_loss (post-trade) 実装
   - 違反イベントを EventStream に push
5. **Step 5 — Bevy UI: Safety Rails Modal + Promote to Live ボタン**
   - `safety_rails_modal.rs` 新設、Replay KPI サマリー表示
   - Strategy Editor から `[Promote to Live]` 経路の E2E (Mock)
6. **Step 6 — Bevy UI: Live Run Panel**
   - `live_run_panel.rs` 新設
   - Pause / Resume / Stop ボタンの動作確認
7. **Step 7 — PositionsPanel / OrdersPanel の source_run_id フィルタ**
   - Order / Position メタデータ拡張
   - ドロップダウン UI 追加
8. **Step 8 — Partial Bar Push**
   - `aggregator.py` 拡張、Strategy が現在進行中バーを参照可能に
   - Replay / Live で同じイベント形状の確認テスト
9. **Step 9 — Live E2E (Demo / Verify)**
   - Tachibana Demo + 簡単な戦略 (MA cross) を 1 営業日 Live 稼働
   - kabu Verify でも同様に E2E
10. **Step 10 — Polish**
    - drawio アーキ図 `phase10-architecture.drawio.svg`
    - Strategy 開発者向けドキュメント (Portability Layer の使い方、Safety Rails の指針)
    - Phase 11 への引き継ぎ事項を docs にまとめる

---

## 5. Success Criteria

### Strategy Portability

- Phase 6 の Replay 用サンプル戦略 (`ma_cross.py` 等) が **コード変更ゼロ** で Live Auto モードで起動できる
- Strategy 内に `if mode == "replay":` のような分岐が存在しない (grep で確認)
- `Strategy.on_bar()` に渡るイベントが Replay と Live で同じ型・同じフィールドであることを type test で確認

### Promote to Live

- Strategy Editor で `.py` を編集 → `[Promote to Live]` → Safety Rails モーダル → Live 起動、までが手動 E2E で通る
- Venue 未ログイン / unsaved changes / safety limits 未設定 のいずれかが NG なら `[Promote to Live]` ボタンが disabled になる
- 2 段階確認モーダルで Replay KPI (累積リターン / 最大 DD / Sharpe / 約定回数) が表示される

### Safety Rails

- `max_position_size_jpy` 超過 → `OrderRejected` で戦略に通知、UI トースト表示 (unit test + Mock E2E)
- `max_order_value_jpy` 超過 → 同上
- `max_orders_per_minute` 超過 → token bucket で発注遅延 (unit test)
- `max_daily_loss_jpy` 超過 → 戦略が自動 STOPPED 状態に、in-flight order が全 cancel (unit test + Mock E2E)
- `allowed_instruments` 外への発注 → `OrderRejected`

### Live Run Telemetry

- Live 稼働中の fills / position / PnL が PositionsPanel / OrdersPanel に Replay と同等の粒度で表示される
- 複数 run (手動 + Strategy) が同居しても source_run_id フィルタで分離表示できる
- `SafetyRailViolation` トーストが Footer 右下に出る

### 構造的安全性

- `ExecutionMode != LiveAuto` で `StartLiveStrategy` を呼ぶと `EXECUTION_MODE_PRECONDITION` で reject (unit test)
- 同一戦略の二重 `StartLiveStrategy` が `STRATEGY_ALREADY_RUNNING` で reject (unit test)
- Replay run と Live run が同時に走っているとき、UI の Run Badge で両方が独立に表示される

### Bar Builder 精度

- Replay / Live で同じ tick データを `BarBuilder` に流すと、生成される `Bar` の OHLCV が一致する (unit test)
- Partial bar push が 1 秒間隔で発火し、Strategy が `self.bar.close` で現在進行中バーの最新値を取得できる

---

## 6. ADRs

### ADR: Strategy Portability Layer は環境依存を `StrategyConfig` 経由で注入する

選択肢:
- **A. Strategy 内で `if self.mode == "replay"` のような分岐** — コード重複、保守性低下
- **B. 環境依存を `StrategyConfig` 経由で注入、Strategy 本体は環境非依存** ← **採用**

採用理由: Nautilus 標準の Strategy パターン (`config` 経由の依存注入) に従う。テスタビリティと将来の Promote to Live の構造的整合性が高い。

### ADR: Replay 側も BarBuilder を経由する

選択肢:
- **A. Replay は J-Quants OHLCV をそのまま `on_bar` に渡す** — Live と Strategy 側コードが分岐する可能性
- **B. Replay 側も `BarBuilder` を経由させ、Live と同じイベント形状にする** ← **採用**

採用理由: Strategy 本体が完全に環境非依存になる。Replay → Live のプロモートで挙動差分が出るリスクを構造的に排除。

### ADR: Safety Rails は Python `RiskEngine` 層で実装する

選択肢:
- **A. UI 側 (Rust) で Safety Rails チェック** — bypass されるリスク (RPC を直接叩けば回避可能)
- **B. Python `RiskEngine` 層で実装** ← **採用**

採用理由: Safety Rails は **構造的に bypass 不可能** であるべき。UI は単に値を入力させる layer に留め、検証は backend が責任を持つ。

### ADR: 1 戦略 1 Live インスタンス制約

選択肢:
- **A. 複数インスタンス許可** — 戦略のロジック次第で二重発注リスク
- **B. 1 戦略 1 Live インスタンスに制約** ← **採用**

採用理由: Phase 10 段階では戦略の多重化が想定外。Phase 11 で需要が出た段階で `instance_id` パラメータを追加して制約を緩める拡張余地を残す。

### ADR: 戦略の hot-reload は対象外、明示停止 → 再起動を要求する

選択肢:
- **A. `.py` 編集を検知して自動再起動** — 編集中の半端な状態で起動するリスク
- **B. 明示停止 → 編集 → 再起動を要求** ← **採用**

採用理由: 誤発注リスクと UI 状態の整合性。Phase 11 以降で「safe reload」(現在の position をそのまま引き継いで新戦略インスタンスに移行) を別途設計する。

### ADR: Live Strategy の Order / Position に source_run_id を付与

選択肢:
- **A. メタデータなし (手動と Strategy 発注を区別しない)** — 戦略停止時の clean-up が困難
- **B. source_run_id を付与し、UI でフィルタ可能に** ← **採用**

採用理由: 戦略停止時に「この戦略由来の in-flight order だけ cancel する」操作が安全にできる。手動ポジションを誤って巻き込まない。

### ADR: ExecEngine は Phase 9 で初有効化、StrategyEngine は Phase 10 で初有効化

Phase 8 → 9 → 10 で段階的に Nautilus エンジン群を有効化する設計:
- Phase 8: DataEngine のみ (read-only)
- Phase 9: DataEngine + ExecEngine (手動発注のみ)
- Phase 10: DataEngine + ExecEngine + StrategyEngine (戦略自動発注)

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
| **複数戦略同時 Live 実行** | 1 戦略 1 Live に制約 | `instance_id` 拡張 |
| **戦略の hot-reload** | 明示停止 → 再起動 | safe reload (position 引き継ぎ) |
| **戦略パラメータ最適化** | 非対象 | Grid search / Optuna 統合 |
| **戦略パフォーマンスダッシュボード** | 生イベント表示のみ | KPI 集計 / Sharpe / Calmar 自動計算 |
| **Live Strategy の永続化と自動復旧** | 再起動時は停止状態 | app_state 経由の復元 |
| **専用ログビューアパネル** | Live Run Panel の最終 100 行のみ | フィルタ機能付きログビューア |
| **戦略のバージョン管理** | 非対象 | git 連携 / strategy_id にハッシュ付与 |
| **複数 Venue 同時接続** (Phase 8 Open Question) | 非対象 | venue 別 StrategyEngine |

---

## 8. Open Risks

1. **Replay と Live で BarBuilder 出力の微差** — partial bar push のタイミング、tick の集約方式によって OHLCV が ±1 tick ずれる可能性。Step 8 で徹底的に regression test
2. **Safety Rails の loophole** — `max_daily_loss` の計算における unrealized P&L 評価タイミング (mark-to-market) のずれで判定が遅延する可能性。実装時に保守的に評価
3. **Strategy 内で例外発生時の挙動** — Nautilus 標準では `on_bar` の例外で戦略が落ちる。Phase 10 では `LiveStrategyStateMachine.error("STRATEGY_EXCEPTION")` に遷移させ、`SafetyRailViolation` イベントで UI に通知 + 全 in-flight order cancel
4. **Promote to Live の Replay KPI 信頼性** — 直近 Replay 結果を `Cache` から取得するが、戦略パラメータ変更後に Replay 未実行のまま `[Promote to Live]` を押されるとサマリーが古い。前提条件チェックに「直近 Replay 結果のパラメータが現在と一致しているか」を含める
5. **kabu の polling 遅延と戦略の意思決定タイミング** — kabu は約定通知が 1 秒 polling のため、戦略が「直近約定を見て次の判断」をする場合に遅延発生。Phase 11 で WebSocket 経由の push 化を venue にリクエストするか、戦略側で polling 前提の設計指針を出す
