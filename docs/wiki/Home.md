# The Trader Was Replaced

**The Trader Was Replaced** は、日本株を対象とした戦略リプレイ／バックテスト／ライブ閲覧のためのデスクトップアプリです。Bevy 0.15（Rust）製のフローティングウィンドウ GUI（バイナリ `backcast.exe`）と、NautilusTrader 1.226+ をベースとした Python gRPC エンジンを別プロセスで組み合わせて動作します。対応する証券会社は立花証券 e支店 と kabuステーション（三菱UFJ eスマート証券）です。

![アーキテクチャ](assets/architecture.drawio.svg)

## 3本柱

| 柱 | 概要 | 詳細 |
|---|---|---|
| Replay 実行 | GUI 上で戦略 `.py` を読み込み、過去データ（ParquetDataCatalog）をリプレイしながら戦略を実行する。フッターの再生コントロールで開始・一時停止・ステップ・速度変更ができる。 | [Replay 実行](replay.md) |
| バックテスト（CLI） | GUI を使わず、`python -m engine.strategy_replay` または `scripts/run_replay.ps1` でヘッドレスにリプレイを実行し、run-buffer（`meta.json` / `fills.jsonl` / `equity.jsonl` / `summary.json`）を出力する。 | [バックテスト](backtest.md) |
| ライブ閲覧 | 立花証券 e支店 / kabuステーション に接続し、銘柄の最新気配・口座情報を閲覧する。 | [取引所接続](venues.md) |

ライブ発注・第二暗証番号入力・口座同期は **Phase 9 で開発中** です。詳細は [注文](orders.md) / [取引所接続](venues.md) を参照してください。

## 構成

本アプリは 2 つのプロセスで構成されます。

- **GUI（Rust / Bevy）**: `backcast.exe`。画面表示・操作・チャート描画を担当。
- **バックエンド（Python / gRPC）**: NautilusTrader ベースのエンジン。既定ポート `19876`。データ供給・戦略実行を担当。

GUI からバックエンドへは gRPC で接続します。接続に成功するとフッター右下に `state: IDLE  grpc: OK` と表示されます。

## ページ一覧

| ページ | 内容 |
|---|---|
| [はじめに](getting-started.md) | 前提環境、バックエンド／GUI の起動、最初の Replay 実行までの最短手順 |
| [画面構成](screen-layout.md) | メニューバー・サイドバー・フッター・フローティングウィンドウの全体マップ |
| [ウィンドウとパネル](windows-and-panels.md) | 各フローティングウィンドウ（パネル）の役割 |
| [チャート](chart.md) | チャートウィンドウの表示内容 |
| [実行モード](modes.md) | Replay / Manual / Auto モードの切り替え |
| [Replay 実行](replay.md) | GUI 上での戦略リプレイ手順 |
| [バックテスト](backtest.md) | CLI でのヘッドレス実行 |
| [戦略](strategy.md) | 戦略 `.py` と SCENARIO（サイドカー JSON）の扱い |
| [取引所接続](venues.md) | 立花証券 / kabuステーション への接続 |
| [注文](orders.md) | 注文関連（Phase 9 で開発中） |
| [File メニュー](file-menu.md) | レイアウトの保存・読み込み・新規作成 |
| [設定](settings.md) | テーマ・バックエンド・レイアウト関連 |
| [トラブルシューティング](troubleshooting.md) | よくある症状と対処 |
