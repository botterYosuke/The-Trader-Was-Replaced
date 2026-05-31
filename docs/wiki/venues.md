# Venue 接続

The Trader Was Replaced は、日本株のライブ venue として **立花証券 e支店 (Tachibana)** と **kabuステーション (kabu、三菱UFJ eスマート証券 / 旧 auカブコム証券)** に接続できます。venue への接続・切断はメニューバーの **Venue** メニューから行います。

> 文中の `[D1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

関連ページ: [settings](settings.md) / [orders](orders.md) / [troubleshooting](troubleshooting.md) / [getting-started](getting-started.md)

## Venue メニュー

メニューバーの **Venue(&V)** をクリック、または **Alt+V** で開閉します。メニュー項目は以下のとおりです（`src/ui/menu_bar.rs`）。

| メニュー項目 | 接続先 |
|---|---|
| Connect Tachibana (Demo) | 立花証券 e支店 デモ環境 [D1] |
| Connect Tachibana (Prod) | 立花証券 e支店 本番環境 [D1]/[L3] |
| Connect kabuStation (Verify) | kabuステーション 検証環境 (localhost:18081) [D1] |
| Connect kabuStation (Prod) | kabuステーション 本番環境 (localhost:18080) [D1]/[L3] |
| Disconnect | 接続中の venue を切断 [D4] |

### メニューの挙動

- 起動時の `LIVE_VENUE`（`TACHIBANA` / `KABU`）で配線された venue 側の項目だけが有効です。配線されていない側の Connect 項目は自動的に非表示になります。 [L5]
- venue が「未接続でない」状態（AUTHENTICATING / CONNECTED / SUBSCRIBED / RECONNECTING / **ERROR**）のあいだは、すべての Connect 項目が無効化（半透明表示）されます。先に **Disconnect** してください。ERROR で固まった場合も、いったん Disconnect してから接続し直します（DISCONNECTED のときだけ Connect が有効）。 [D1]/[D3]/[D4]/[D6]

## venue 状態バッジ（フッター）

フッター右側に `Venue: <状態> (<venue_id>)` のバッジが表示されます（`src/ui/footer.rs`）。状態は次のように遷移します。

| 状態 | 意味 | 色 | E2E |
|---|---|---|---|
| DISCONNECTED | 未接続 | グレー | logout で復帰 [D4] |
| AUTHENTICATING | ログイン処理中 | 黄 | [D1] |
| CONNECTED | ログイン完了 | 水色 | [D1] |
| SUBSCRIBED | 銘柄購読中 | 緑 | [D2] |
| RECONNECTING | 再接続中 | 黄 | [D6] |
| ERROR | エラー | 赤 | login 失敗 [D3] |

接続フローは `Disconnected → Authenticating → Connected → Subscribed` と遷移します（成功時 [D1]/[D2]、失敗時 [D3]、外部要因の再接続 [D6]）。Prod 接続の二重ガード（環境変数未設定で遮断）は [D8]/[L3]。venue 側での外部ログアウト検知（VenueLogoutDetected）は [D5]、再ログイン通知の dismiss / Escape 優先順位は [K13]（#46 Slice B で `ModalLayer` スタック化 — Esc は専用 system ではなく汎用 `modal_layer_esc_system` が frontmost を dismiss）。Replay モードへの切替は venue を切断しない（VenueChanged{Disconnected} を伴わない限り Connected のまま） [D9]。

## Tachibana（立花証券 e支店）

- **デモ環境が既定**です。本番環境への接続には環境変数 `TACHIBANA_ALLOW_PROD=1` が必須で、未設定のまま Prod に接続しようとすると遮断されます（二重ガード）。 [L3]
- ログイン認証情報は環境変数で渡します。
  - `DEV_TACHIBANA_USER_ID` … ユーザー ID
  - `DEV_TACHIBANA_PASSWORD` … パスワード
  - `DEV_TACHIBANA_DEMO=true` … デモ環境を使う
- **第二暗証番号**はログイン時にプロンプトで入力し、キャッシュしません。注文系の操作でも第二暗証番号が要求され、バックエンドが `SecretRequired` を push すると SecretModal が開きます（入力値は SecretVault に短時間だけ保持、TTL で zeroize、永続化しない）。提出 / retry は [K8]、timeout zeroize / 空 submit ガードは [K15]。 [F5]
- 接続後、対象 venue の銘柄ユニバースが読み込まれます。 [C1]/[D7]

## kabu（kabuステーション）

- **検証環境 (localhost:18081) が既定**です。本番環境 (localhost:18080) への接続には環境変数 `KABU_ALLOW_PROD=1` が必須です（二重ガード）。 [L3]
- 認証は X-API-KEY 方式のトークンを発行して行います。API パスワードは環境変数 `DEV_KABU_API_PASSWORD` で渡します。
- kabuステーション本体（ローカルアプリ）が起動し、対象ポートで待ち受けている必要があります。
- **第二暗証番号は不要**です（API 設計が Tachibana と異なります）。
- 流量制限・上限があります。

| 種別 | 制限 |
|---|---|
| info（情報取得） | 10 req/s |
| order（注文） | 5 req/s |
| wallet（余力） | 10 req/s |
| 銘柄登録 | 最大 50 銘柄 |

## 銘柄購読とサイドバー価格表示

- サイドバーの **Instruments** セクションで銘柄を追加・選択します [J11]/[J13]。銘柄ユニバースの取得は [C1]（失敗時の stale 保持は [C2]、日付別の利用可能銘柄取得は [C3]/[C4]）。Live 接続時に Replay fallback リストを上書きする不変条件は [D7]。
- Live モード（Manual / Auto）で銘柄行をクリックすると、その銘柄の市場データ購読（SubscribeMarketData）が発火し、状態が SUBSCRIBED に進みます。[F1]（購読解除は [F2]）。銘柄の選択（`SelectedSymbol` 更新）は [C5]（保留中: UI 駆動のみで backend→ECS seam を通らない）。
- 各銘柄行には最新価格が表示されます（Replay / Live 共通）。価格更新は [F1]。
- 銘柄が未登録のときは `No instruments` と表示されます。各行の **x** ボタンで銘柄を削除できます。 [J13]

### 銘柄ピッカー（`+ Add`）

Instruments セクション下部の **+ Add** ボタンを押すと、検索ボックス付きのドロップダウン（銘柄ピッカー）が開きます。

- 検索欄に入力すると候補が絞り込まれ、最大 **15 行**まで表示されます。行をクリックすると Instruments に追加されます。ピッカーは開いたままなので連続追加でき、`Esc` で閉じます（同一銘柄の連続追加は約 100ms デバウンスされます）。 [J11]
- 候補の取得元は実行モードで変わります（Replay は日付指定の利用可能銘柄 [C3]/[C4]、Live は接続中 venue のユニバース [C1]）。状況に応じて次のプレースホルダが表示されます。

| 状況 | 表示 |
|---|---|
| Replay で `scenario.end` 未設定 | `Set scenario.end first` [J12] |
| 取得中 | `Loading...` [J12] |
| Live で venue 接続直後（銘柄マスタを初回ダウンロード中＝cold store） | `Loading...`（赤エラーではなくスピナー。マスタが揃うと自動で銘柄が並びます） [C6] |
| 取得失敗 | `Error: {メッセージ}` [J12] |
| Live で venue 未接続 | `Venue not connected` [J12] |
| 絞り込み結果が空 | `No matches` [J12] |

- サイドカーが `instruments_ref`（schema v3）を使う場合、Instruments は読み取り専用になり **+ Add** は無効化されます（`This sidecar uses 'instruments_ref' — read-only` の警告が出ます）。詳細は [strategy.md](strategy.md) を参照。 [J10]

## ライブ venue の選択

ライブ venue は `LIVE_VENUE` 環境変数で決まります。`backcast` がこれを読み取り、in-proc バックエンドへ転送します。

```text
LIVE_VENUE=TACHIBANA
LIVE_VENUE=KABU
```

未設定だと Replay 専用（ライブ venue なし）で起動します。起動は `run_inproc.ps1` 1 本で完結します（旧 `python -m engine --live-venue ...` の別プロセス起動は #64 / #68 で撤去済み）。詳細は [settings](settings.md) / ルート README.md §起動方法 を参照してください。
