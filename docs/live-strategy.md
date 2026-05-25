# Live Strategy — Replay で検証した戦略を Live Auto で動かす

Phase 10 で「戦略コードからの自動発注（Live Auto）」が解禁された。Replay
（[strategy-replay.md](./strategy-replay.md)）で検証した **同一の `Strategy` サブクラス** を、
ファイル編集なしに Live にプロモートして動かせる。本ドキュメントは戦略開発者向けに
「移植可能な戦略の書き方」「bar 供給の仕組み」「Safety Rails の指針」「Promote 〜 制御」をまとめる。

アーキテクチャ全体図: [`assets/phase10-architecture.drawio.svg`](./assets/phase10-architecture.drawio.svg)

---

## 1. 移植性 — 戦略はモード分岐しない

**原則: 戦略は Replay か Live かを知らない。** 環境依存（時刻ソース・データ供給・発注先）は
すべてエンジン側が供給し、戦略は常に同じ Nautilus API を使う。

| 環境依存 | Replay | Live Auto | 戦略が使う API（無分岐） |
| --- | --- | --- | --- |
| 時刻 | `TestClock` | `LiveClock` | `self.clock.utc_now()` / `set_timer` |
| データ | `BacktestEngine` の `DataEngine` | `LiveDataEngine` | `self.subscribe_bars(...)` / `on_bar(Bar)` |
| 発注 | backtest matching engine | `LiveExecutionEngine` → venue | `self.submit_order(...)` |
| Venue | scenario 由来（例 `TSE`） | `TACHIBANA` / `KABU` | config の `InstrumentId` から |

`self.clock` / `self.cache` / `self.msgbus` は **`Trader.register()` 時にエンジンが注入する**
（`common/actor.pyx`、登録前は `None`）。`StrategyConfig` が運ぶのは venue / instrument_id /
params だけで、clock や data engine は config 経由では渡らない。これは Nautilus 標準の
backtest↔live 可搬性そのもの。

> ❌ `if mode == "replay":` のような分岐を戦略内に書かない。書いた時点で移植性が壊れる
> （Success Criteria は grep で分岐ゼロを要求する）。`time.sleep` / `asyncio.sleep` /
> 壁時計 API も使わない（determinism が壊れる）— スケジュールは `self.clock.set_timer`。

---

## 2. Live にプロモートできる戦略の書き方

ローダ（`strategy_runtime/strategy_loader.load()`）はクラスを返すだけで、
Replay ランナー（`engine_runner.py`）も Live ホスト（`engine_controller.py`）も同じ `load()` を呼ぶ。
**戦略コンストラクタは 2 形式どちらでもよい**:

### (a) kwargs 形式（推奨・サンプル戦略と同形）

Live ホストは `instrument_id` と `bar_type_str` を **キーワード引数で** 渡し、`params`（gRPC の
`StartLiveStrategy.params`）を上乗せする。

```python
class MeanReversion(Strategy):
    def __init__(self, instrument_id: str, bar_type_str: str, lookback: int = 20) -> None:
        super().__init__()
        self._iid = InstrumentId.from_str(instrument_id)
        self._bar_type = BarType.from_str(bar_type_str)
        self._lookback = lookback

    def on_start(self) -> None:
        self.subscribe_bars(self._bar_type)   # ← 購読は on_start で（__init__ ではない）

    def on_bar(self, bar: Bar) -> None:
        ...   # self.submit_order(...) で発注
```

### (b) `StrategyConfig` 形式

```python
class FakeBuyAndHoldConfig(StrategyConfig, frozen=True):
    instrument_id: str = "1301.TSE"
    bar_type: str = "1301.TSE-1-DAY-LAST-EXTERNAL"

class FakeBuyAndHold(Strategy):
    def on_start(self) -> None:
        self.subscribe_bars(BarType.from_str(self.config.bar_type))
```

> ⚠️ `on_start` で subscribe する（`__init__` ではない）。コンストラクタ時点では msgbus /
> data engine が未配線。

### bar_type は EXTERNAL を書いておけばよい

戦略コードには `...-LAST-EXTERNAL` を書いておけば、**Live ホストが自動で INTERNAL に読み替える**
（`bar_supply.to_internal_bar_type` / `live_bar_type`）。戦略は同じ `BarSpecification`
（step / aggregation / price_type）を購読し続け、変わるのは `aggregation_source` だけ。
コード変更ゼロで Replay↔Live 可搬になる。

---

## 3. bar 供給 — `on_bar` に何が届くか

戦略の `on_bar(Bar)` には、Replay でも Live でも **同じ Nautilus `Bar` 型・同じ `BarSpecification`**
が届く。供給経路だけが異なる:

- **Replay**: catalog の確定 `Bar`（`EXTERNAL`）を `BacktestEngine` が `on_bar` に流す。
- **Live**: venue の約定/板を Nautilus `TradeTick` 化し、`LiveDataEngine` の internal aggregation
  （`TimeBarAggregator`、`INTERNAL`）が確定 `Bar` を組んで `on_bar` に届ける（Phase 10 Step 8）。

```
venue tick ─ LiveRunner(tick tap) ─ NautilusVenueDataClient.feed_trades_update
           └→ TradeTick → _handle_data → LiveDataEngine 内 TimeBarAggregator(INTERNAL)
              → 確定 Bar → Strategy.on_bar
```

注意点:

- **`on_bar` に届くのは確定バーのみ。** Nautilus 標準では未確定（partial）バーは `on_bar` に流れない。
  時間バーの確定は **`LiveClock` タイマー駆動**（tick の `ts_event` ではない）。
- **UI 用の partial bar は別系統。** `LiveRunner` が 1 秒間隔で進行中バーのスナップショット
  （`build_now()`）を `LiveEventBus` に流し、UI チャートが描く。戦略には届かない。
- **venue 別の精度（重要）**:
  - **Tachibana**: EVENT WS の `EC`（約定）が実 tick になる → bar の精度が高い。
  - **kabu**: PUSH は板情報のみで **約定 tick が無い**。bar は板 `CurrentPrice` か `GET /orders`
    polling から構成するしかなく、連続 tick 列が得られない（Tachibana より精度が落ち、
    「直近約定を見て次の判断」をする戦略は 1〜2 秒遅延し得る）。kabu 向け戦略は polling 前提で設計する。

---

## 4. Safety Rails — 構造的に bypass 不可能

Safety Rails は **backend で強制する**（UI は値入力 layer に過ぎず、RPC を直叩きしても回避できない）。
2 段構え:

| Rail | 実装 | 既定 (`SafetyLimits`) |
| --- | --- | --- |
| `max_order_value_jpy` | ネイティブ `LiveRiskEngineConfig.max_notional_per_order`（pre-trade deny） | `0` = OFF |
| `max_orders_per_minute` | ネイティブ `max_order_submit_rate = "N/00:01:00"` | `0` = OFF |
| `max_position_size_jpy` | 独自 pre-trade（既存ポジション金額 + 新規注文金額 ≤ 上限） | `0` = OFF |
| `allowed_instruments` | 独自 pre-trade（ホワイトリスト照合） | `()` = 起動 instrument のみ |
| `max_daily_loss_jpy` | 独自 post-trade（当日 P&L が上限割れ → run を `ERROR` に） | `0` = OFF |

- **`0 = その rail 無効`**。数値ポリシー（50万/100万/10万/5 等）は Bevy UI 側が既定値として提供する。
- pre-trade 違反は Nautilus が `OrderDenied` として戦略に通知し、UI には `SafetyRailViolation` が push される
  （Footer トースト）。venue には注文を送らない。
- `max_daily_loss_jpy` 超過は post-trade で検知して run を自動 `STOPPED`（`ERROR` 経由）にし、
  **当該 `StrategyId` の in-flight 注文だけ** を cancel する（手動・他戦略を巻き込まない）。
- 注文金額（notional）は参照価格が必要。market data 供給後（Step 8）は last price から算出するが、
  価格未供給時は保守的に position-size チェックをスキップする場合がある。

---

## 5. Live Auto の起動と run 制御

### 起動フロー（RPC チェーン）

> **UI 入口（issue #40）**: 専用の「Promote to Live」ボタンと Safety Rails モーダルは撤去済み。
> 以下の RPC チェーンと前提条件は backend 側に温存されており、UI からの起動入口は
> フッターの transport（再生）ボタンへ再配線する予定（Auto に切替 → `[File]▸[Open]` → ▶）。

起動の前提条件:

1. Venue ログイン済み（`CONNECTED` / `SUBSCRIBED`）
2. ExecutionMode が `LiveAuto`（または `SetExecutionMode(LiveAuto)` 可能）
3. 戦略ファイルがディスク保存済み（unsaved changes 無し）
4. Safety Rails が設定済み

UI 入口の再配線時は、呼び出し側が **`RegisterLiveStrategy` → `SetExecutionMode(LiveAuto)`
→ `StartLiveStrategy`** を順に await する必要がある（LiveAuto 反映前に Start が走るレースを防ぐため）。
`RegisterLiveStrategy` はファイルを canonicalize + `sha256` 検証して `strategy_id`
（`strat-{sha256[:16]}`、内容ハッシュ由来で冪等）を発行し、`StartLiveStrategy` は生パスではなく
この `strategy_id` を受け取る。

### run 制御（Live Run Panel）

| RPC | 効果 |
| --- | --- |
| `StopLiveStrategy(run_id)` | graceful 停止。在庫ポジションは残す（ユーザー判断で別途決済） |
| `PauseLiveStrategy(run_id)` | **新規発注ゲート**。市場データ callback は継続し得るが新規注文を `STRATEGY_PAUSED` で deny。既存注文は維持 |
| `ResumeLiveStrategy(run_id)` | 再開 |
| `GetLiveStrategyStatus` / `ListLiveStrategies` | 状態取得 |

- **Phase 10 は自動戦略 run を同時 1 件に制約**。既に RUNNING/PAUSED の auto run があると
  `StartLiveStrategy` は `LIVE_STRATEGY_ALREADY_RUNNING` で reject（手動発注 `MANUAL-001` との同居は可）。
- Pause/Resume/Stop は **mode で hard gate しない**（runaway を常に止められるよう、run 存在のみが条件）。
- 各 run には一意な Nautilus `StrategyId`（`LIVE-{run 短縮}`）が採番され、OrdersPanel の
  発注主体フィルタ（All / Manual / Strategy）と run 別 telemetry に使われる。
- **crash 復旧**: backend crash → 自動再起動時、Live run は復元されず停止状態になる
  （意図しない再起動を避けるため人間判断に委ねる）。venue 側に注文が残る可能性があるので UI で要確認。

---

## 6. やってはいけないこと（チェックリスト）

- ❌ `if mode == "replay"` 等のモード分岐（移植性が壊れる）
- ❌ `__init__` での `subscribe_bars`（msgbus 未配線）→ `on_start` で
- ❌ `time.sleep` / `asyncio.sleep` / `datetime.now()`（determinism 破壊）→ `self.clock`
- ❌ `print` / `logging` 直叩き → `self.log.info(...)`（Nautilus 構造化ログ = stdout/file に乗る）。
  **UI（Live Run Panel）に出したい行は `emit_strategy_log(self, message, level)`**
  （`engine.live.strategy_log`）を使う — `self.log` ミラー + msgbus 経由で `StrategyLogMessage` 中継に乗る。
  ※ 素の `self.log.*` は UI には中継されない（Nautilus に Python log sink が無いため、§Open Question）。
- ❌ Live 稼働中の `.py` 編集による hot-reload 期待 → 明示停止 → 編集 → 再起動が必須（Phase 10）
- ⚠️ depth を参照する戦略: depth 宣言フック（`REQUIRES_DEPTH`）は **未実装**（Phase 11 候補、§0.5）。
  現状 Replay は depth 空、Live は venue 依存。
