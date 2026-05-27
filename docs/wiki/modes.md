# モード（Replay / Manual / Auto）

> 文中の `[E1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

The Trader Was Replaced は **3 つの実行モード**を持ち、フッター左端のセグメントトグルで切り替える。Bevy 0.15 GUI フロントエンドと Python gRPC エンジン（NautilusTrader ベース）で構成される。

> モードを切り替える CLI フラグ（`--mode` など）は**存在しない**。モードは GUI のフッターからのみ切り替える。ヘッドレスでの過去データ実行は [backtest.md](backtest.md) を参照。

![モードと状態遷移](assets/modes-state.drawio.svg)

## モード切替 UI

フッター（画面下部のバー）の左端に 3 つのボタンが並ぶ。 [I4]

| ボタン | `ExecutionMode` | 説明 |
|---|---|---|
| `Replay` | `Replay` | 過去データ再生・仮想実行（既定） |
| `Manual` | `LiveManual` | ライブ市場・手動発注 |
| `Auto` | `LiveAuto` | ライブ市場・戦略自動発注 |

選択中のモードはハイライト表示される。**起動直後は Replay** が選択されている。 [E1]

切替は「クリック → backend へ `SetExecutionMode` を送信 → backend からの状態通知でローカル状態を更新」という流れになっており、ローカルで先行して状態を変えない。これにより backend が拒否したときの UI と backend の不整合を防いでいる。[E1]

### 切替の前提条件（クライアント側）

クリックしても前提条件を満たさなければ切替リクエストは送られない（クライアント側 gating）。 [I4]

| 切替先 | 前提条件 | 満たさない場合 |
|---|---|---|
| `Manual` / `Auto`（ライブ） | Venue が接続済み（Disconnected / Error 以外） | 切替不可 [I4] |
| `Replay` | なし（常に到達可能なホームモード） | — 戦略未ロード・replay `IDLE` でも切替可 [E1][I4] |

Venue（証券会社）の接続はメニューバーの **Venue** メニューから行う。詳細は [venues.md](venues.md) を参照。

## 各モードの説明

### Replay（過去データ再生）

- 過去のバーデータ（J-Quants 由来の ParquetDataCatalog）を再生し、戦略を**仮想実行**する。
- **実発注は行わない。** 約定はエンジン内のシミュレーションで生成される。
- フッターにトランスポート操作（先頭へ / 1 バック / Play・Pause / 1 進む / 強制停止）と再生速度（1x〜50x）が表示される。 [A1]/[A2]/[A3]/[A4]/[A5]
- **Startup ウィンドウ**（開始日・終了日・粒度・初期資金）が表示される。閉じる `×` ボタンを持たないフローティングウィンドウで [M7]、表示・非表示は実行モードが決める [M8]。フィールド検証・Run は [J7]/[J8]。
- 操作フローの詳細は [replay.md](replay.md)。

### Manual（ライブ・手動発注）

- **ライブ市場**に接続し、ユーザーが Order ウィンドウ（発注フォーム）から**手動で発注**する。新規・訂正・取消、2 段階 confirm、第二暗証番号モーダル（Tachibana）を備える。詳細は [orders.md](orders.md)。 [K7]/[K9]/[K10]/[K12]
- トランスポート操作・再生速度・Startup ウィンドウは非表示になる（過去データ再生の概念がないため）。 [I4]
- **戦略エディタ（Strategy Editor）も非表示**になる。Manual は手動発注のみで戦略編集を使わないため、エディタウィンドウとサイドバーの「Strategy Editor」ボタンを隠す（Replay / Auto では表示）。 [M12]

### Auto（ライブ・戦略自動発注）

- **ライブ市場**に接続し、Replay で検証した戦略が**自動で発注**する。
- **Auto への切替自体に `--live-venue` 起動が要る**。`--live-venue` 無しではログインできず Auto モードに入れない（＝Auto に入れている時点で venue identity は確定済み）。
- **Live Auto の起動（issue #40）**: 専用の **「Promote to Live」ボタン**と **Safety Rails モーダル**は撤去され、起動経路はフッターの transport（再生）**▶** ボタンへ統合された。**Auto** に切替 → `[File]▸[Open]` で戦略を読み込み → フッターの **▶** を押すと `RegisterLiveStrategy` → `StartLiveStrategy` を直列発行して Live Auto run を起動する（戦略の scenario に銘柄があり・venue ライブ接続・戦略 cache が揃ったときのみ送出、いずれか欠けると未送出。起動銘柄は scenario から導出＝Replay と対称で、複数銘柄ではサイドバー選択が scenario 内ならそれを・無ければ先頭を使う）。`SetExecutionMode` は ▶ から再送しない（モードは backend 権威）。Live 実行エンジンと起動 RPC（`StartLiveStrategy` 等）は温存。前提（instrument・venue ライブ接続・strategy ロード）が欠けると ▶ はサイレント無反応で終わらず、Run Result パネルに理由（"No instrument selected" / "Venue not connected" / "No strategy loaded (open a strategy file)" 等）を赤字表示する。起動経路と Safety Rails は [strategy.md](strategy.md)。 [N5]/[N7]
- 起動時に **Safety Rails**（`max_position_size_jpy` / `max_order_value_jpy` / `max_daily_loss_jpy` / `max_orders_per_minute` / `allowed_instruments`、`0` は無効）で発注量・建玉・当日損失・流量に上限をかける（バックエンドが pre/post-trade で強制）。
- 実行中の run の lifecycle / PnL / order / fill は **Run Result** パネルにリアルタイム表示される。Safety Rail 違反は Footer トーストで通知される。[N1]/[N2]/[N3]/[N4]

## モードによる挙動差

| 項目 | Replay | Manual / Auto（ライブ） |
|---|---|---|
| トランスポートボタン（`|<` `<` `▶/||` `>` `■`） | 表示 [A1]/[A2]/[A3]/[A4] | 非表示（Auto は **▶** のみ表示）[I4]/[N5] |
| 再生速度（1x〜50x） | 表示 [A5] | 非表示 [I4] |
| Startup ウィンドウ（フローティング） | 表示 [M8] | 非表示 [M8]/[I4] |
| フッターの `time:` 表示 | リプレイ時刻（JST、`(replay)`） [A2] | 実時刻（`(live)`） [I4] |
| 実発注 | なし（仮想） | あり（手動 / 自動） |
| Venue 接続 | 不要（接続中でも切断しない） [D9] | 必須 |
| live 価格の panel 混入 | なし（live kline は Replay 中は reducer に流れない） [D22] | live 価格をそのまま表示 |
| live 口座の portfolio panel 混入 | なし（live AccountEvent は Replay 中は backend stream に流れない） [D23] | live 余力・建玉をそのまま表示 |

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
