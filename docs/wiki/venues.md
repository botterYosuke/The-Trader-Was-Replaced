# Venue 接続

The Trader Was Replaced は、日本株のライブ venue として **立花証券 e支店 (Tachibana)** と **kabuステーション (kabu、三菱UFJ eスマート証券 / 旧 auカブコム証券)** に接続できます。venue への接続・切断はメニューバーの **Venue** メニューから行います。

> 文中の `[D1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

関連ページ: [settings](settings.md) / [orders](orders.md) / [troubleshooting](troubleshooting.md) / [getting-started](getting-started.md)

## Venue メニュー

メニューバーの **Venue(&V)** をクリック、または **Alt+V** で開閉します。メニュー項目は以下のとおりです（`src/ui/menu_bar.rs`）。

| メニュー項目 | 接続先 |
|---|---|
| Connect Tachibana (Demo) | 立花証券 e支店 デモ環境 |
| Connect Tachibana (Prod) | 立花証券 e支店 本番環境 |
| Connect kabuStation (Verify) | kabuステーション 検証環境 (localhost:18081) |
| Connect kabuStation (Prod) | kabuステーション 本番環境 (localhost:18080) |
| Disconnect | 接続中の venue を切断 |

### メニューの挙動

- バックエンドの起動時 `--live-venue {TACHIBANA,KABU}` フラグで配線された venue 側の項目だけが有効です。配線されていない側の Connect 項目は自動的に非表示になります。
- いずれかの venue が接続処理中・接続中（AUTHENTICATING / CONNECTED / SUBSCRIBED / RECONNECTING）のあいだは、すべての Connect 項目が無効化（半透明表示）されます。先に Disconnect してください。

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

接続フローは `Disconnected → Authenticating → Connected → Subscribed` と遷移します（成功時 [D1]/[D2]、失敗時 [D3]、外部要因の再接続 [D6]）。Prod 接続の二重ガード（環境変数未設定で遮断）は [D8]（保留中・backend 側）。venue 側での外部ログアウト検知（VenueLogoutDetected）は [D5]（Phase 9 で開発中）。

## Tachibana（立花証券 e支店）

- **デモ環境が既定**です。本番環境への接続には環境変数 `TACHIBANA_ALLOW_PROD=1` が必須で、未設定のまま Prod に接続しようとすると遮断されます（二重ガード）。
- ログイン認証情報は環境変数で渡します。
  - `DEV_TACHIBANA_USER_ID` … ユーザー ID
  - `DEV_TACHIBANA_PASSWORD` … パスワード
  - `DEV_TACHIBANA_DEMO=true` … デモ環境を使う
- **第二暗証番号**はログイン時にプロンプトで入力し、キャッシュしません。注文系の操作で第二暗証番号を要求する仕組み（SecretVault、60 秒 TTL）は **Phase 9 で開発中**です。
- 接続後、対象 venue の銘柄ユニバースが読み込まれます。

## kabu（kabuステーション）

- **検証環境 (localhost:18081) が既定**です。本番環境 (localhost:18080) への接続には環境変数 `KABU_ALLOW_PROD=1` が必須です（二重ガード）。
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

- サイドバーの **Instruments** セクションで銘柄を追加・選択します。銘柄ユニバースの取得は [C1]（失敗時の stale 保持は [C2]、日付別の利用可能銘柄取得は [C3]/[C4]）。Live 接続時に Replay fallback リストを上書きする不変条件は [D7]。
- Live モード（Manual / Auto）で銘柄行をクリックすると、その銘柄の市場データ購読（SubscribeMarketData）が発火し、状態が SUBSCRIBED に進みます。[F1]（購読解除は [F2]）
- 各銘柄行には最新価格が表示されます（Replay / Live 共通）。価格更新は [F1]。

## バックエンドの venue 配線

ライブ venue はバックエンド起動時の `--live-venue` フラグで決まります。

```text
python -m engine --live-venue TACHIBANA
python -m engine --live-venue KABU
```

フラグを省略すると Replay 専用（ライブ venue なし）で起動します。詳細は [settings](settings.md) を参照してください。
