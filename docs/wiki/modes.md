# モード（Replay / Manual / Auto）

The Trader Was Replaced は **3 つの実行モード**を持ち、フッター左端のセグメントトグルで切り替える。Bevy 0.15 GUI フロントエンドと Python gRPC エンジン（NautilusTrader ベース）で構成される。

> モードを切り替える CLI フラグ（`--mode` など）は**存在しない**。モードは GUI のフッターからのみ切り替える。ヘッドレスでの過去データ実行は [backtest.md](backtest.md) を参照。

![モードと状態遷移](assets/modes-state.drawio.svg)

## モード切替 UI

フッター（画面下部のバー）の左端に 3 つのボタンが並ぶ。

| ボタン | `ExecutionMode` | 説明 |
|---|---|---|
| `Replay` | `Replay` | 過去データ再生・仮想実行（既定） |
| `Manual` | `LiveManual` | ライブ市場・手動発注（Phase 9 で開発中） |
| `Auto` | `LiveAuto` | ライブ市場・戦略自動発注（将来） |

選択中のモードはハイライト表示される。**起動直後は Replay** が選択されている。

切替は「クリック → backend へ `SetExecutionMode` を送信 → backend からの状態通知でローカル状態を更新」という流れになっており、ローカルで先行して状態を変えない。これにより backend が拒否したときの UI と backend の不整合を防いでいる。

### 切替の前提条件（クライアント側）

クリックしても前提条件を満たさなければ切替リクエストは送られない。

| 切替先 | 前提条件 | 満たさない場合 |
|---|---|---|
| `Manual` / `Auto`（ライブ） | Venue が接続済み（Disconnected / Error 以外） | 切替不可 |
| `Replay` | 戦略がロード済み | 切替不可 |

Venue（証券会社）の接続はメニューバーの **Venue** メニューから行う。詳細は [venues.md](venues.md) を参照。

## 各モードの説明

### Replay（過去データ再生）

- 過去のバーデータ（J-Quants 由来の ParquetDataCatalog）を再生し、戦略を**仮想実行**する。
- **実発注は行わない。** 約定はエンジン内のシミュレーションで生成される。
- フッターにトランスポート操作（先頭へ / 1 バック / Play・Pause / 1 進む / 強制停止）と再生速度（1x〜50x）が表示される。
- サイドバーに **Startup** パネル（開始日・終了日・粒度・初期資金）が表示される。
- 操作フローの詳細は [replay.md](replay.md)。

### Manual（ライブ・手動発注）

- **ライブ市場**に接続し、ユーザーが**手動で発注**する。
- Phase 9 で開発中。
- トランスポート操作・再生速度・Startup パネルは非表示になる（過去データ再生の概念がないため）。

### Auto（ライブ・戦略自動発注）

- **ライブ市場**に接続し、戦略が**自動で発注**する。
- 将来実装（Phase 9 以降）。
- 安全装置（`max_qty` / `max_notional_jpy`）で発注量・約定代金に上限をかける。詳細は [strategy.md](strategy.md)。

## モードによる挙動差

| 項目 | Replay | Manual / Auto（ライブ） |
|---|---|---|
| トランスポートボタン（`|<` `<` `▶/||` `>` `■`） | 表示 | 非表示 |
| 再生速度（1x〜50x） | 表示 | 非表示 |
| Startup パネル（サイドバー） | 表示 | 非表示 |
| フッターの `time:` 表示 | リプレイ時刻（JST、`(replay)`） | 実時刻（`(live)`） |
| 実発注 | なし（仮想） | あり（手動 / 自動） |
| Venue 接続 | 不要 | 必須 |

チャートの **Ladder（板）ペイン**などライブ専用の表示はライブモードでのみ意味を持つ。チャートの詳細は [chart.md](chart.md) を参照。

## フッターの状態表示

フッター右側には現在の状態が表示される。

| ラベル | 意味 |
|---|---|
| `time:` | リプレイ時刻（Replay）または実時刻（ライブ） |
| `state:` | エンジンの状態（`IDLE` / `LOADED` / `RUNNING` / `PAUSED`） |
| `Venue:` | Venue 接続状態（`DISCONNECTED` / `CONNECTED` / `SUBSCRIBED` 等） |
| `grpc:` | backend との gRPC 接続状態（`DISABLED` / `OK` / `ERR` / `...`） |

## 関連ページ

- [replay.md](replay.md) — Replay モードの操作フロー
- [backtest.md](backtest.md) — ヘッドレス CLI での過去データ実行
- [strategy.md](strategy.md) — 戦略の書き方と Strategy Editor
- [venues.md](venues.md) — Venue（証券会社）接続
