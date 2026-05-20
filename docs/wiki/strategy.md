# 戦略の書き方と Strategy Editor

戦略は NautilusTrader の `Strategy` サブクラスとして Python で記述する。実行条件（銘柄・期間・粒度・初期資金など）は戦略 `.py` と**同名のサイドカー JSON**（`<strategy>.json`）の `scenario` キーに書く。

## 戦略 `.py`

`nautilus_trader.trading.strategy.Strategy` を継承したクラスを定義し、`on_start` でバーを購読し、`on_bar` などのコールバックで売買する。

```python
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


class BuyAndHoldStrategy(Strategy):
    def __init__(self, *, instrument_id: str = "1301.TSE", lot_size: int = 100) -> None:
        super().__init__(config=StrategyConfig(strategy_id="buy-and-hold"))
        self.instrument_id = InstrumentId.from_str(instrument_id)
        self.lot_size = int(lot_size)
        self._bought = False

    def on_start(self) -> None:
        self.subscribe_bars(BarType.from_str(f"{self.instrument_id}-1-DAY-LAST-EXTERNAL"))

    def on_bar(self, bar: Bar) -> None:
        if self._bought:
            return
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(self.lot_size),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)
        self._bought = True
```

`__init__` の引数は SCENARIO の `strategy_init_kwargs`、または CLI の `--strategy-param KEY=VALUE` で上書きできる。

### リポジトリ内のサンプル

| ファイル | 内容 |
|---|---|
| `python/tests/data/test_strategy_daily.py` | 1301.TSE / Daily / バイアンドホールド |
| `python/tests/data/test_strategy_minute.py` | Minute 版 |
| `python/tests/data/test_strategy_7203_daily.py` | 7203.TSE / Daily |
| `python/tests/data/pair_trade_minute.py` | 2 銘柄（schema v2）/ Minute / ペアトレード |

## SCENARIO（サイドカー JSON）

実行条件は戦略 `.py` と同じディレクトリにある同名 `<strategy>.json` の `scenario` キーに書く。例えば `test_strategy_daily.py` に対しては `test_strategy_daily.json`。

```json
{
  "scenario": {
    "schema_version": 1,
    "instrument": "1301.TSE",
    "start": "2025-01-06",
    "end": "2025-03-31",
    "granularity": "Daily",
    "initial_cash": 1000000
  }
}
```

複数銘柄（schema v2）の例:

```json
{
  "scenario": {
    "schema_version": 2,
    "instruments": ["1301.TSE", "7203.TSE"],
    "start": "2025-01-06",
    "end": "2025-01-10",
    "granularity": "Minute",
    "initial_cash": 1000000
  }
}
```

### キー

| キー | 必須 | 内容 |
|---|---|---|
| `schema_version` | 必須 | `1` / `2` / `3` |
| `instrument` | v1 で必須 | 単一銘柄の文字列（例 `"1301.TSE"`） |
| `instruments` | v2 / v3 で必須 | 銘柄の文字列リスト（空不可） |
| `start` | 必須 | 開始日（`YYYY-MM-DD`） |
| `end` | 必須 | 終了日（`YYYY-MM-DD`） |
| `granularity` | 必須 | `"Daily"` または `"Minute"`（大文字小文字を厳密に区別） |
| `initial_cash` | 必須 | 初期資金（整数） |
| `strategy_init_kwargs` | 任意（v3） | 戦略 `__init__` に渡す kwargs |

> `granularity` は `"Daily"` / `"Minute"` 以外（`"daily"`, `" Daily "`, `"DAILY"` 等）を受け付けない。

### レガシー: `.py` 内 SCENARIO

戦略 `.py` 内に `SCENARIO` 定数を書く旧形式は、**Python CLI からのみ** フォールバックで動く（WARN ログが出る）。**GUI（Bevy アプリ）からは実行できない**。新規の戦略はサイドカー JSON を使うこと。

## Strategy Editor（GUI）

メニューバー **File → Open (Ctrl+O)** で戦略 `.py` を開くと、フローティングウィンドウの Strategy Editor が開く。

### 編集機能

| 機能 | 操作 |
|---|---|
| Python シンタックスハイライト | 自動 |
| 行番号ガター | 左端に表示 |
| スクロールバー | 右端に表示 |
| 検索・置換 | `Ctrl+F` で Find/Replace パネルを開く（`Esc` で閉じる）。パネル内に検索欄・置換欄と Replace / Replace All ボタンがある |
| Tab インデント | `Tab` キーでスペースに展開 |
| オートインデント | `Enter` で前行のインデントを引き継ぐ |
| 括弧オートクローズ | 開き括弧を入力すると閉じ括弧を補完 |
| Undo / Redo | `Ctrl+Z`（Undo）/ `Ctrl+Y` または `Ctrl+Shift+Z`（Redo） |
| 自動保存 | 編集を止めて約 1 秒後にキャッシュへ自動保存（デバウンス） |

> 検索・置換は `Ctrl+F` で開く 1 つのパネルにまとまっている（置換専用の `Ctrl+H` ショートカットはない）。

## 安全装置（max_qty / max_notional_jpy）

ライブ発注（Manual / Auto）時の暴走を防ぐため、SCENARIO に発注の上限を設定できる。

| キー | 内容 |
|---|---|
| `max_qty` | 1 注文あたりの最大数量 |
| `max_notional_jpy` | 1 注文あたりの最大約定代金（円） |

ライブモードは Phase 9 で開発中。ライブ発注の詳細は [orders.md](orders.md) を参照。

## 関連ページ

- [replay.md](replay.md) — GUI Replay の操作フロー
- [backtest.md](backtest.md) — ヘッドレス CLI 実行
- [modes.md](modes.md) — 3 モードの概要
- [orders.md](orders.md) — 注文
