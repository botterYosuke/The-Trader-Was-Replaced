# 画面構成

> 文中の `[I1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

![画面構成](assets/screen-layout.drawio.svg)

The Trader Was Replaced の画面は、上部のメニューバー、左のサイドバー、下部のフッター、そして中央のフローティングウィンドウ群で構成されます。

| エリア | 位置 | 役割 |
|---|---|---|
| [メニューバー](#メニューバー) | 上 | File / Edit / Venue のドロップダウンメニュー [I1]/[I2]/[I3] |
| [サイドバー](#サイドバー) | 左 | 銘柄リスト＋価格、パネルを開くボタン、Settings 表示 [J11]/[J13]/[M1]/[M6] |
| [フッター](#フッター) | 下 | 実行モードトグル、再生コントロール、速度、Venue 状態、gRPC 状態 [I4]/[A1]/[A2]/[A3]/[A4]/[G1] |
| [フローティングウィンドウ](#フローティングウィンドウ) | 中央 | ドラッグ可能なパネル群 [M1]/[M2]/[M3]/[M4] |

## メニューバー

画面上端に配置されます。左から **File(&F)** / **Edit(&E)** / **Venue(&V)** の 3 つのトップレベルメニューがあり、右端には現在の戦略状態（例: `strategy: none`）が表示されます。Alt + F / E / V でも開けます。 [I1]/[I2]

| メニュー | 項目 |
|---|---|
| File(&F) | New / Open (Ctrl+O) / Save (Ctrl+S) / Save As (Ctrl+Shift+S) |
| Edit(&E) | Undo (Ctrl+Z) / Redo (Ctrl+Y) |
| Venue(&V) | Connect Tachibana (Demo) / Connect Tachibana (Prod) / Connect kabuStation (Verify) / Connect kabuStation (Prod) / Disconnect |

File メニューの各項目はレイアウト（および戦略一式）の保存・読み込みを扱います。詳細は [File メニュー](file-menu.md) を参照してください。Venue メニューは取引所への接続・切断です。詳細は [取引所接続](venues.md) を参照してください。

## サイドバー

画面左に固定表示されます。上から順に以下のセクションがあります。

| セクション | 内容 |
|---|---|
| Instruments | 登録銘柄の一覧。各行に銘柄 ID、最新価格、削除用の **x** ボタン。下部に **+ Add** ボタン [J11]/[J12]/[J13] |
| Panels | 開けるパネルのボタン群: **Strategy Editor** / **Buying Power** / **Run Result** / **Positions** / **Orders** [M1]/[M5] |
| Settings | テーマ・バックエンド URL・レイアウト状態の表示（例: `Theme: Dark` / `Backend: localhost:19876`） [M6] |

Instruments セクションでは、行をクリックすると選択銘柄が切り替わります [J13]。銘柄が未登録のときは `No instruments` と表示されます [J13]。**+ Add** で開く検索付き銘柄ピッカーや、`instruments_ref` 使用時の読み取り専用化は [取引所接続](venues.md#銘柄ピッカー-add) を参照してください [J11]/[J12]/[J10]。Panels ボタンを押すと対応するフローティングウィンドウが開きます [M1]。Settings の詳細は [設定](settings.md) を参照してください [M6]。

## フッター

画面下端に配置され、左から順に以下が並びます。

| 要素 | 内容 |
|---|---|
| 実行モードトグル | **Replay** / **Manual** / **Auto** のセグメント切り替え [I4]/[E1] |
| 再生コントロール | `\|<`（先頭へ）/ `<`（ステップ戻し）/ **▶**（Run・一時停止中は再開、実行中は **\|\|** で一時停止）/ `>`（ステップ送り）/ **■**（強制停止） [A1]/[A2]/[A3]/[A4] |
| 速度セレクタ | `1x` / `2x` / `5x` / `10x` / `50x`（既定 `1x`） [A5] |
| 時刻ラベル | `time: ...`（Replay は JST のリプレイ時刻、Live は現在時刻） [A2]/[I4] |
| 状態バッジ | `state: IDLE` / `RUNNING` / `PAUSED` / `LOADED` [A1]/[A2]/[A3]/[A4] |
| Venue バッジ | `Venue: DISCONNECTED` / `AUTHENTICATING` / `CONNECTED` / `SUBSCRIBED` / `RECONNECTING` / `ERROR` [D1]/[D2]/[D3]/[D4]/[D6] |
| gRPC ステータス | `grpc: DISABLED` / `OK` / `ERR` / `...` [G1]/[G2]/[G3] |

再生コントロールと速度セレクタは **Replay** モードのときだけ表示されます（Manual / Auto では非表示）。実行モードの違いは [実行モード](modes.md)、再生操作の詳細は [Replay 実行](replay.md) を参照してください。 [I4]

## フローティングウィンドウ

中央の作業領域に表示される、ドラッグで移動できるパネル群です。サイドバーの Panels ボタン、または戦略の読み込み時に生成されます。 [M1]/[M2]/[I5]（**Startup ウィンドウは例外**で、Panels ボタンからは開かず、実行モードが表示を制御します [M8]。詳細は [ウィンドウとパネル](windows-and-panels.md)。）

| パネル | 役割 |
|---|---|
| Strategy Editor | 戦略 `.py` の編集 |
| Startup | リプレイ実行条件（開始日・終了日・粒度・初期資金）。Replay のみ表示・`×` 無し [M7]/[M8] |
| Chart | ローソク足チャートの表示 |
| Buying Power | 買付余力・資金情報 |
| Positions | 保有ポジション |
| Orders | 注文（読み取り専用一覧） |
| Run Result | 実行結果・Live Auto run の状態（lifecycle / PnL / order / fill）。ExecutionMode 所有・自動 spawn。 [M13]/[M14]/[N1]/[N2] |
| Order | LiveManual 限定のサイドバー Order ボタンから開くスプライト製ウィンドウ（新規・訂正・取消、2 段階 confirm） [K7]/[K10] |

Order は LiveManual 限定のサイドバー **Order** ボタンから開くワールド空間のスプライト製フローティングウィンドウ（タイトル `ORDER`）で、ドラッグ移動・パン追随・`×` で閉じる操作に対応し、LiveManual を離れると despawn します（レイアウト永続化の対象外）。 [K10]。各パネルの詳細は [ウィンドウとパネル](windows-and-panels.md)、チャートの内容は [チャート](chart.md) を参照してください。
