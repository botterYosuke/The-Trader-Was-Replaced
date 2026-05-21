# 戦略の書き方と Strategy Editor

> 文中の `[J1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

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

実行条件は戦略 `.py` と同じディレクトリにある同名 `<strategy>.json` の `scenario` キーに書く。例えば `test_strategy_daily.py` に対しては `test_strategy_daily.json`。 [J14]

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
| `schema_version` | 必須 | `1` / `2` / `3` [J14] |
| `instrument` | v1 で必須 | 単一銘柄の文字列（例 `"1301.TSE"`） [J14] |
| `instruments` | v2 / v3 | 銘柄の文字列リスト（空不可） [J14] |
| `instruments_ref` | v3 で `instruments` の代替 | 外部 JSON への参照（`"<path>"` または `"<path>#<json-pointer>"`、サイドカーからの相対パス）。下記参照 [J9]/[J10] |
| `start` | 必須 | 開始日（`YYYY-MM-DD`） [J14]/[J16] |
| `end` | 必須 | 終了日（`YYYY-MM-DD`） [J14]/[J16] |
| `granularity` | 必須 | `"Daily"` または `"Minute"`（大文字小文字を厳密に区別） [J14]/[J16] |
| `initial_cash` | 必須 | 初期資金（整数） [J14]/[J16] |
| `strategy_init_kwargs` | 任意 | 戦略 `__init__` に渡す kwargs |

> `granularity` は `"Daily"` / `"Minute"` 以外（`"daily"`, `" Daily "`, `"DAILY"` 等）を受け付けない。 [J14]

### `instruments_ref`（schema v3 / 外部ユニバース参照）

v3 では `instruments` を直接書く代わりに、`instruments_ref` で銘柄リストを外部 JSON ファイルから読み込める。

- 形式は `"universe.json"`（ファイル全体）または `"universe.json#/path/to/list"`（JSON ポインタで配列を指定）で、パスはサイドカー JSON からの相対で解決される。
- **解決は fail-closed**: 参照先ファイルが無い・JSON が壊れている・ポインタが不正・リストが空のいずれかの場合、シナリオは読み込まれず（`ScenarioLoadedFromFile` が発火せず）、Run ボタンは半透明のまま有効化されない。 [J9]
- `instruments_ref` を使うサイドカーを開くと、**サイドバーの Instruments は読み取り専用**になる（`+ Add` ボタンが無効化され、銘柄の追加・削除ができない）。サイドバーには `This sidecar uses 'instruments_ref' — read-only` の警告が表示される。手動で銘柄を編集したい場合は `instruments` を直書きする v2 / v3 サイドカーを使う。 [J10]

### レガシー: `.py` 内 SCENARIO

戦略 `.py` 内に `SCENARIO` 定数を書く旧形式は、**Python CLI からのみ** フォールバックで動く（WARN ログが出る）。**GUI（Bevy アプリ）からは実行できない**。新規の戦略はサイドカー JSON を使うこと。

## Strategy Editor（GUI）

メニューバー **File → Open (Ctrl+O)** で戦略の **サイドカー JSON（`<strategy>.json`）** を選択すると（ファイルダイアログは `.json` のみを表示する。同名の `<strategy>.py` が自動で読み込まれる）、フローティングウィンドウの Strategy Editor が開く。 [I5]

開いている sidecar JSON が変更された場合は、mtime の変化に応じて scenario が再読み込みされ、Startup / Instruments の表示へ反映されます。 [J15]

### 編集機能

| 機能 | 操作 |
|---|---|
| Python シンタックスハイライト | 自動 [L4] |
| 行番号ガター | 左端に表示 [L4] |
| スクロールバー | 右端に表示 [L4] |
| 検索・置換 | `Ctrl+F` で `FIND / REPLACE` パネルを開く（`Esc` で閉じる）。詳細は下記 [J5]/[J6] |
| Tab インデント | `Tab` キーでスペース（4 つ）に展開 [J2] |
| オートインデント | `Enter` で前行のインデントを引き継ぐ [J3] |
| 括弧オートクローズ | 開き括弧（`(` `[` `{` `"` `'`）を入力すると閉じ括弧を補完（直後が閉じ括弧のときは補完しない） [J4] |
| Undo / Redo | `Ctrl+Z`（Undo）/ `Ctrl+Y` または `Ctrl+Shift+Z`（Redo） [I11] |
| 自動保存 | 編集を止めて約 1 秒後にキャッシュへ自動保存（デバウンス） [J1] |

### 検索・置換パネル（`FIND / REPLACE`）

`Ctrl+F` で検索と置換が 1 つのパネルにまとまって開く（置換専用の `Ctrl+H` ショートカットはない）。`Esc` で閉じる。 [J5]

| 要素 | 内容 |
|---|---|
| 検索欄 / 置換欄 | 上段が検索クエリ、下段が置換文字列。検索は**部分一致**（正規表現ではない） [J5] |
| `<` / `>` ボタン | 前のマッチ / 次のマッチへ移動 [J5] |
| `Repl` / `Repl All` ボタン | 現在のマッチを置換 / すべて置換 [J6] |
| マッチ件数表示 | `現在 / 全体` 形式でヒット数を表示 [J5] |
| `F3` / `Shift+F3` | 次 / 前のマッチへ移動（検索欄にフォーカス中は `Enter` ではなく `F3` で移動） [J5] |
| 大文字小文字 | 既定は区別しない（case-insensitive）。トグルで切り替え [J6] |

## Promote to Live（戦略の Live Auto 昇格）

Replay で検証した戦略を、ライブ venue での**自動発注**に昇格させる導線が **Promote to Live** です。

1. 戦略をロードし、Venue を接続する（Promote ボタンは戦略未ロード / venue 未接続のあいだは無効・グレー表示）。
2. `[Promote to Live]` を押すと、エディタの内容がキャッシュ `.py`（Replay の Run と同じパス）へ flush され、**Safety Rails モーダル**が開く。
3. モーダルで Safety Rails の上限を確認・調整し、Confirm すると `PromoteToLive` が送られる（バックエンドは Register → SetExecutionMode(LiveAuto) → Start を連鎖実行する）。
4. 結果はモーダル / Promote 表示にフィードバックされる（成功 = 新 run id、拒否 = 構造化 error_code: `EXECUTION_MODE_PRECONDITION` / `VENUE_LOGIN_REQUIRED` / `LIVE_STRATEGY_ALREADY_RUNNING` / `STRATEGY_LOAD_FAILED` / `STRATEGY_HASH_MISMATCH`）。 [N5]

起動後の run は **Live Run Panel** に表示され、`[Pause]` / `[Resume]` / `[Stop]` で制御し、PnL / order / fill のテレメトリと戦略ログ tail を確認できる。詳細は [orders.md](orders.md) / [windows-and-panels.md](windows-and-panels.md)。 [N1]/[N2]/[N4]

## Safety Rails（ライブ自動発注の安全装置）

Live Auto の暴走を防ぐため、Promote 時に発注の上限を設定する。**`0` はそのレールを無効**にする（課さない）。Safety Rails モーダルは数値 4 項目を ± ステッパーで編集し（既定値: max_position_size_jpy=1,000,000 / max_order_value_jpy=500,000 / max_daily_loss_jpy=100,000 / max_orders_per_minute=5）、`allowed_instruments` のホワイトリストはバックエンドが起動時銘柄に対して強制する。

| レール | 種別 | 内容 |
|---|---|---|
| `max_order_value_jpy` | pre-trade（ネイティブ） | 1 注文あたりの最大約定代金（円）。Nautilus `max_notional_per_order` |
| `max_orders_per_minute` | pre-trade（ネイティブ） | 1 分あたりの最大発注回数。Nautilus `max_order_submit_rate` |
| `max_position_size_jpy` | pre-trade（独自） | \|既存ポジション金額\| + 新規注文金額 の上限（円） |
| `allowed_instruments` | pre-trade（独自） | 発注を許可する銘柄ホワイトリスト（空なら制限なし） |
| `max_daily_loss_jpy` | post-trade（独自） | 当日 P&L が `-上限` を割ると run を ERROR にし in-flight order を cancel |

pre/post-trade のいずれかが違反すると `SafetyRailViolation` が push され、Footer トーストで通知される。 [N3]

ライブ発注の詳細は [orders.md](orders.md)、モードの概要は [modes.md](modes.md) を参照。

## 関連ページ

- [replay.md](replay.md) — GUI Replay の操作フロー
- [backtest.md](backtest.md) — ヘッドレス CLI 実行
- [modes.md](modes.md) — 3 モードの概要
- [orders.md](orders.md) — 注文
