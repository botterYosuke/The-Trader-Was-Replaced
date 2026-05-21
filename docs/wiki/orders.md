# 注文と口座

> **ライブ発注（新規 / 訂正 / 取消）・第二暗証番号モーダル・口座同期は Phase 9 で開発中です。** 本ページの「将来像」節は未実装の設計であり、現行ビルドでは動作しません。

> 文中の `[B1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。

関連ページ: [venues](venues.md) / [settings](settings.md) / [replay](replay.md) / [troubleshooting](troubleshooting.md)

## 現状（実装済み）

現在、注文・建玉・余力は **Replay（仮想実行）の結果として読み取り専用で表示**されます。実際の証券会社へは発注しません。

| パネル | 内容 |
|---|---|
| Orders | Replay 中に発生した仮想注文の一覧 |
| Positions | 仮想建玉（数量・平均取得単価・含み損益） |
| Buying Power | 仮想ポートフォリオの cash / equity / buying power |

これらは戦略を Replay 実行した結果がバックエンドから届き、パネルに反映されたものです。ユーザーがこの画面から発注・取消・訂正を行うことはできません。Replay 完了後にポートフォリオ（positions / orders / equity）が反映されることは [B1]、サマリ（fills_count / total_pnl 等）の parse は [B2] で保証されます。

## 将来像（Phase 9 で開発中）

Phase 9 では、ライブ venue に対する実発注パイプラインを開発中です。以下は実装中の設計であり、本ビルドでは利用できません。

- **Manual モードでの発注**: 新規注文・訂正・取消。
- **第二暗証番号モーダル**: Tachibana で注文時に第二暗証番号を要求するモーダル。入力値は短時間だけ保持（SecretVault、60 秒 TTL）し、永続化しません。kabu は第二暗証番号を必要としません。
- **注文 → 約定 → ポートフォリオ反映パイプライン**: 発注した注文の状態（受付・約定・拒否など）と約定結果が口座に反映されます。発注成功時は full レコードが `LiveOrders` に seed され [H1]、以後のステータス/約定更新がマージされます [H2]/[F3]。訂正は qty/price の差分のみ上書きされ [H3]、拒否は OrderPanel のエラー行に整形メッセージが出ます [H4]。構造化 reject ではない注文 notice も同じフィードバック行に表示されます [H6]。第二暗証番号の提出失敗は SecretModal 側の retry 可能 error として表示されます [H7]。実行モードを切り替えると口座スナップショットは一旦リセットされ、Live/Replay のデータが混ざらないようになっています [H5]。
- **口座同期**: venue 側の現金残高・建玉を取得して Positions / Buying Power に反映します [F4]。

### 注文フロー（開発中）

![注文フロー(開発中)](assets/order-flow.drawio.svg)

> 上図の点線部分は **Phase 9 で開発中**の経路を表します。実線部分（Replay の読み取り表示）のみが現行ビルドで動作します。

## バックエンドイベント（gRPC）の概念

Phase 9 では、バックエンドから UI へ向けたサーバー送信ストリーム `SubscribeBackendEvents` を通じて、`BackendEvent` が push されます。`BackendEvent` は次のいずれか 1 つを運びます（`python/proto/engine.proto`）。

| イベント種別 | 意味 | E2E |
|---|---|---|
| SecretRequired | 第二暗証番号の入力要求（Tachibana のみ。kabu は送出しない） | [F5] |
| OrderEvent | 注文状態の更新（ステータス・約定数量・平均約定価格など） | [F3] |
| AccountEvent | 口座更新（現金・買付余力・建玉一覧） | [F4] |
| VenueLogoutDetected | venue 側でのログアウト検知 | [D5] |

> 4 つの event seam はいずれも Phase 9 マージで `backend_event_drain_system` に reducer が入り、E2E で観測可能になりました。SecretRequired → `SecretPrompt`（[F5]）、OrderEvent → `LiveOrders`（[F3]）、AccountEvent → `PortfolioState`（[F4]）、VenueLogoutDetected → `ReloginPrompt`（[D5]、Step 7 health watchdog）。

ユーザー視点では、SecretRequired を受けると第二暗証番号モーダルが表示され、OrderEvent / AccountEvent によって Orders / Positions / Buying Power が更新され、VenueLogoutDetected を受けると ReloginModal が開いて再ログインを促します（モーダルは通知のみで、再ログイン自体は Venue メニューから行います）。

backend 再起動後の注文 reconcile では、backend が追跡していない working orders だけが ReconcileModal に表示され、terminal orders は無視されます [K6]。
