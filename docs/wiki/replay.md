# Replay モード

> 文中の `[A1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

Replay モードは、過去のバーデータを再生して戦略を**仮想実行**するモードである（既定モード）。実発注は行わず、約定はエンジン内のシミュレーションで生成される。

ヘッドレス（GUI なし）で同等の実行を行う方法は [backtest.md](backtest.md) を参照。

![Replay シーケンス](assets/replay-sequence.drawio.svg)

## 全体の流れ

1. フッター左端のモードトグルが **Replay** になっていることを確認（既定）。 [E1]
2. メニューバー **File → Open (Ctrl+O)** で戦略の **サイドカー JSON（`<strategy>.json`）** を選択する（ダイアログは `.json` のみ表示。同名の `.py` が自動で読み込まれ Strategy Editor が開く）。 [I5]/[I9]
3. サイドバーの **Startup** パネルで開始日・終了日・粒度・初期資金を確認／編集する。 [J7]/[J8]
4. フッター中央の **▶** ボタンで Run を開始する。 [A1]/[J8]
5. Replay Startup 進捗ウィンドウが表示され、再生が始まると自動的に消える。 [A7]
6. トランスポートボタンと速度ボタンで再生を制御する。 [A2]/[A3]/[A4]/[A5]

## Startup パネル（サイドバー）

サイドバーの **Startup** パネルでシナリオの実行条件を編集する。フィールドは戦略のサイドカー JSON（`<strategy>.json` の `scenario` キー）と同期し、編集はキャッシュサイドカーへ書き戻される。

| ラベル | 内容 | 検証 |
|---|---|---|
| `Start` | 開始日（`YYYY-MM-DD`） | 空・不正な日付はエラー [J7]。編集内容の commit は [J16] |
| `End` | 終了日（`YYYY-MM-DD`） | 空・不正な日付はエラー。`Start` は `End` 以前であること [J7]。編集内容の commit は [J16] |
| `Granularity` | 粒度。`Daily` / `Minute` の 2 ボタンから選択 | 未選択・未知の値はエラー [J7]。ボタン commit は [J16] |
| `Initial cash` | 初期資金（正の整数） | 整数でない・0 以下はエラー [J7]。編集内容の commit は [J16] |

入力エラーは各フィールドの下に赤字で表示される。エラーが残っている間は Run できない。

> **パネルが無効化される条件**: 再生の起動中（Startup 進捗ウィンドウ表示中）や、書き戻し先のキャッシュサイドカーが未設定のときは、パネルが薄く表示され編集できなくなる。

Startup パネルは **Replay モードのときだけ表示**される（Manual / Auto では非表示）。

## トランスポートボタン（フッター）

フッター中央のボタンで再生を制御する。Replay モードのときだけ表示される。

| ボタン | 動作 | E2E |
|---|---|---|
| `\|<` | 先頭へ（実際には強制停止して初期状態へ戻す） | [A4] |
| `<` | 1 バック（現状未配線。`PAUSED` 中のみ意味を持つ想定） | no-op [A10] |
| `▶` / `\|\|` | Play / Pause。`IDLE` / `LOADED` のときは Run（実行開始）、`RUNNING` のときは `\|\|`（Pause）、`PAUSED` のときは `▶`（Resume） | Run [A1] / Pause・Resume [A2] |
| `>` | 1 進む（`PAUSED` 中のみ有効。1 バーずつステップ実行） | [A3] |
| `■` | 強制停止（`RUNNING` / `PAUSED` / `LOADED` のとき有効） | [A4] |

> **▶ ボタンが半透明のとき**: `IDLE` / `LOADED` 状態で戦略のキャッシュパスが未設定だと Run できず半透明になる。Strategy Editor で戦略を開いてキャッシュを生成すると有効になる。Pause / Resume はキャッシュパス不要なので常に有効。

## 再生速度（フッター）

トランスポートボタンの右に速度ボタンが並ぶ。Replay モードのときだけ表示される。[A5]

| ボタン | 倍率 |
|---|---|
| `1x` | 等倍（既定・起動時に選択） |
| `2x` | 2 倍 |
| `5x` | 5 倍 |
| `10x` | 10 倍 |
| `50x` | 50 倍 |

選択中の速度はハイライト表示される。

## Replay Startup 進捗ウィンドウ

Run を開始すると画面中央に **Starting replay** 進捗ウィンドウが表示される。段階ラベルと不確定（左右に往復する）進捗バーを持つ。[A7]（古い `startup_id` を無視する相関ロジックは [A8]）

段階ラベルは以下の順に遷移する。

| 段階 | ラベル |
|---|---|
| コマンド受理 | `Starting replay command...` |
| 前回のリプレイをリセット | `Resetting previous replay...` |
| データ読み込み | `Loading replay data...` |
| 戦略起動 | `Starting Python strategy...` |
| 最初のティック待ち | `Waiting for first replay tick...` |

再生が始まる（`state: RUNNING` になる、またはリプレイ時刻が進む）と、ウィンドウは**自動的に閉じる**。

- **タイムアウト**: 60 秒経っても起動が完了しないと、ソフトタイムアウトのエラーメッセージを表示し、`Close` ボタンが現れる。backend のログを確認するか、`■`（強制停止）を試す。[A9]/[A11]
- **エラー時**: エラーが表示された場合は `Close` ボタンで閉じる。エラーはユーザーが閉じるまで残る。[A6]/[A11]

## フッターの状態表示

| ラベル | 内容 |
|---|---|
| `time:` | リプレイ時刻（JST、末尾に `(replay)`） |
| `state:` | `IDLE` / `LOADED` / `RUNNING` / `PAUSED` |
| `grpc:` | backend 接続（`DISABLED` / `OK` / `ERR` / `...`） |

## SCENARIO サイドカー

Startup パネルが編集する開始日・終了日・粒度・初期資金は、戦略 `.py` と**同名の `<strategy>.json`** の `scenario` キーに保存される。SCENARIO の全キー仕様・スキーマバージョン・JSON 例は [strategy.md](strategy.md) を参照。

## 関連ページ

- [modes.md](modes.md) — 3 モードの概要
- [backtest.md](backtest.md) — ヘッドレス CLI 実行
- [strategy.md](strategy.md) — 戦略の書き方・SCENARIO・Strategy Editor
- [chart.md](chart.md) — チャート表示
