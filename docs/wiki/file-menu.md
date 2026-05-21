# ファイルメニューとレイアウト永続化

> 文中の `[I5]` などは、その挙動を保証する E2E flow の ID（実体は `tests/e2e/flows/<id>.rs`）。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

画面上端のメニューバーから、ファイル操作・編集操作・接続先（Venue）の選択を行います。メニューバーには `File(&F)` / `Edit(&E)` / `Venue(&V)` の 3 つのトップレベルメニューがあり、右端にロード中の戦略を示すステータスラベル（`strategy: none` 等）が表示されます。

## メニュー項目

### File（ファイル）

| 項目 | ショートカット | 動作 |
| --- | --- | --- |
| `New` | — | ロード中の戦略を破棄し、新規状態（Live Manual）に戻します。 |
| `Open (Ctrl+O)` | Ctrl+O | レイアウト／戦略を開きます（下記「開く」を参照）。 |
| `Save (Ctrl+S)` | Ctrl+S | レイアウトを `.json` サイドカーに保存します。 |
| `Save As (Ctrl+Shift+S)` | Ctrl+Shift+S | 保存先を指定してレイアウトを保存します。 |

### Edit（編集）

| 項目 | ショートカット | 動作 |
| --- | --- | --- |
| `Undo (Ctrl+Z)` | Ctrl+Z | 直前の操作を取り消します。 |
| `Redo (Ctrl+Y)` | Ctrl+Y | 取り消した操作をやり直します。 |

### Venue（接続先）

接続先証券会社の選択・切断を行います。詳細は [接続先（Venue）](venues.md) を参照してください。

| 項目 | 動作 |
| --- | --- |
| `Connect Tachibana (Demo)` | 立花証券（デモ環境）に接続。 |
| `Connect Tachibana (Prod)` | 立花証券（本番環境）に接続。 |
| `Connect kabuStation (Verify)` | kabuステーション（検証環境）に接続。 |
| `Connect kabuStation (Prod)` | kabuステーション（本番環境）に接続。 |
| `Disconnect` | 接続を切断。 |

- 接続処理中・接続中は Connect 系の項目が無効化されます。設定済みの接続先に応じて、対向の Venue の項目は非表示になります。

## メニューの開閉

- 各トップレベルボタンをクリックするとメニューが開閉します。
- キーボードでも開閉できます。

| キー | 対象 |
| --- | --- |
| Alt+F | File メニュー |
| Alt+E | Edit メニュー |
| Alt+V | Venue メニュー |

## 開く（Open）

- `Open (Ctrl+O)` はファイルダイアログを表示し、`.json` のレイアウトサイドカーファイルを選択します。
- サイドカー JSON を開くと、その `windows[]` に応じてパネル（Strategy Editor など）が spawn し、scenario の銘柄に対応するチャートが開きます [I5]。
- サイドカー JSON は戦略 `.py` ファイルへの参照（`strategy_path`）を含むことができ、その場合は対応する戦略も読み込まれます。
- Live モード中に Open を実行すると、ダイアログ表示前に自動的に Live Auto モードへ遷移します。

## レイアウト永続化

`Save` で、現在のレイアウトを `.json` サイドカーファイルに保存します。保存される主な内容は次のとおりです。

| フィールド | 内容 |
| --- | --- |
| `schema_version` | サイドカーのスキーマバージョン。 |
| `viewport` | カメラの pan（`pan_x` / `pan_y`）と zoom。 |
| `windows[]` | 各フローティングウィンドウの `kind` / `position` / `size` / `z` / `region_key`。 |
| `strategy_path` | 復元する戦略 `.py` のパス。 |
| `scenario` | SCENARIO 設定（保存時に既存 JSON から保持して書き戻し）。 |

- ウィンドウを移動すると、約 1 秒のデバウンス後にレイアウトが自動保存されます。
- ウィンドウの移動・クローズは Undo / Redo の対象です（AppHistory による管理）。
- チャートウィンドウはレイアウト永続化の対象外で、サイドバーで選択中の銘柄から導出されます。

## キーボードショートカット一覧

| ショートカット | 機能 |
| --- | --- |
| Ctrl+O | 開く（Open） |
| Ctrl+S | 保存（Save） |
| Ctrl+Shift+S | 名前を付けて保存（Save As） |
| Ctrl+Z | 元に戻す（Undo） |
| Ctrl+Y | やり直す（Redo） |
| Alt+F / Alt+E / Alt+V | メニュー開閉 |
| Ctrl+F | エディタ内の検索・置換パネルを開く（置換も同じパネル。詳細は [戦略エディタ](strategy.md)） |

## プラットフォーム上の注意

- Linux: メニューバーはアプリ内ウィジェットとして描画されます（GTK ネイティブのメニューではありません）。
- macOS: Cmd+Q による終了は、未保存確認ダイアログを出さずにアプリを終了する既知の制約があります。
