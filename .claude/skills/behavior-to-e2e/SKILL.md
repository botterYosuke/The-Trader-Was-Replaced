---
name: behavior-to-e2e
description: >-
  The-Trader-Was-Replaced（Bevy + Python gRPC）で、ユーザーが「こう動いてほしい / こうなったら困る /
  この挙動を保証して」と挙動を言葉で説明したとき、それを **ヘッドレス自動 E2E テスト** に変換するスキル。
  既存ハーネス（`tests/e2e/support/mod.rs` の `Harness`）の上に `tests/e2e_replay.rs` へ `#[test]` を足し、
  backend→ECS seam（`BackendStatusUpdate` / `BackendEvent` / replay clock を注入し resource を assert）で検証する。
  必ず起動する場面: 「この挙動をテストにして」「〜したら〜になることを保証したい」「〜が壊れてないか自動で確認したい」
  「リプレイが完走することをテスト」「Run が失敗したら error が出るのを担保」「ポートフォリオが反映されるか」
  「venue ログインの状態遷移をテスト」「銘柄リストの取得失敗をテスト」「回帰テストを追加」「E2E を1本足して」
  「FLOWS.md の flow を実装」「backend からこのイベントが来たら UI がこうなる、をテスト」と言われたとき。
  GUI を実際に起動して目視する手動検証は本スキルではなく `e2e-testing` を使う。本スキルは描画に依存しない
  resource レベルの自動テスト専用。Rust の一般的なユニットテスト作法は `rust-testing` を併用する。
---

# behavior-to-e2e — 挙動の言葉を E2E テストに変える

ユーザーが日本語で語った「こうあってほしい挙動」を、`tests/e2e_replay.rs` の `#[test]` に落とす。
このプロジェクトの E2E は **OS マウス操作にも描画にも依存しない**。`MinimalPlugins` のヘッドレス Bevy
`App` に、backend が押し出すメッセージ（縫い目 = seam）を注入し、結果の **resource 状態を assert** する。
これが issue #4「Add internal E2E test hooks」の核。`tests/e2e/FLOWS.md` が全 flow の索引。

## なぜこの形なのか（先に理解する）

本アプリの状態機械は「backend → ECS」の一方向で動く。backend（Python gRPC、replay/live runner）が
状態を push し、Rust 側の 3 つの drain system がそれを resource に反映する。**ユーザーに見える状態遷移は
ほぼ全てこの seam を通る**。だからウィンドウを開かなくても、seam にメッセージを流して resource を見れば
「ユーザーが画面で見るはずの挙動」を決定論的に検証できる。逆に言うと、**seam を通らない挙動
（純粋な UI クリック → 描画）は本スキルの対象外**で、`e2e-testing`（手動目視）に回す。

## ワークフロー

1. **挙動を 1 文の不変条件に言い換える**。「Run したら完了する」→「`RunStarted` の後 `RunComplete` を受けると
   `RunState` が `Running`→`Completed` になり `parsed_summary` が埋まる」。ユーザーの言葉のままでなく、
   観測可能な resource 遷移に翻訳する。曖昧なら何を観測すれば「動いた」と言えるかをユーザーに確認する。
2. **FLOWS.md に該当 flow があるか見る**。あれば ID（A1/B2/D3…）と seam・観測がそのまま設計。
   なければ新規 flow として、近いセクション（A〜G）に `- [ ]` 行を追記してよい。
3. **seam を特定する**（下表）。`src/backend_sync.rs` の `apply_status_update` が
   「どの `BackendStatusUpdate` がどの resource をどう変えるか」の **唯一の正解**。推測せずここを読む。
4. **`tests/e2e_replay.rs` に `#[test]` を足す**。既存 4 本（A1/A2/A6/B1）と追加分（A3/A4/A7/A8/B2/C/D/E/F1/F2/G1）が
   そのままお手本。`Harness::new()` → seam を send → 観測を assert。
5. **観測アクセサが無ければ `Harness` に足す**（`tests/e2e/support/mod.rs`）。既存の `portfolio()` と同形:
   `self.app.world().resource::<T>().clone()`。`BackendStatus` は `Clone` 非実装なのでフラグを直接返す
   （`backend_connected()` 等の前例あり）。
6. **走らせる**: `cargo test --test e2e_replay`。**初回コンパイルは Bevy 全体をリンクするため ~11 分**かかる
   （バックグラウンド実行推奨）。2 回目以降は速い。green を確認。
7. **FLOWS.md を更新**: 実装した flow を `- [x]` にし、末尾「実装状況」に 1 行追記。
8. 完了したら `CLAUDE.md` 規約に従い `simplify` と `post-impl-skill-update` を発動。

## seam → resource 早見表

| 検証したい挙動 | 送る seam | 観測する resource |
|---|---|---|
| run のライフサイクル | `RunStarted` / `RunComplete{startup_id,run_id,summary_json}` / `RunFailed{startup_id,error}` | `LastRunResult.state`(`RunState`) / `.parsed_summary` |
| replay clock（pause/resume/step） | `h.push_state(ts)`（`BackendChannel`、backend→ECS clock） | `TradingSession.timestamp_ms` |
| 起動進捗・相関 ID | `ReplayStartup{startup_id,stage}`（要 `h.begin_startup(id)` 先行） | `ReplayStartupProgress.phase` / `.visible` / `.start_engine_accepted` |
| ポートフォリオ | `PortfolioLoaded{...}` | `PortfolioState`(`loaded`/`equity`/`positions`/`orders`) |
| 銘柄ユニバース | `InstrumentsListStarted/Listed/Failed{source,...}` | `Tickers`(`status`/`list`/`source`) |
| 上場銘柄 fetch | `AvailableInstrumentsLoaded/FetchFailed{end_date,...}` | `AvailableInstruments`(`by_end_date`/`in_flight`/`last_error`) |
| venue ライフサイクル | `VenueChanged{state,venue_id,instruments_loaded}` | `VenueStatusRes`(`state`/`instruments_loaded`) |
| 実行モード | `ExecutionModeChanged{mode}` | `ExecutionModeRes.mode` |
| ライブ価格 | `LastPricesUpdated{prices}` | `LastPrices.map` |
| 接続状態 | `Connected(bool)` / `Running(bool)` / `Error(e)` | `BackendStatus`(`connected`/`running`/`last_error`) |

## ハーネス API（`tests/e2e/support/mod.rs`）

- 構築: `let mut h = Harness::new();`（`backend_enabled: true` で明示構築済み。env 非依存）
- 注入: `h.send_status(update)` / `h.send_event(event)` / `h.push_state(ts)` — いずれも送信後 `tick()` まで実行
- フレーム送り: `h.tick()`（= `app.update()` を 1 回。同期実行・即 return）
- 起動窓を開く: `h.begin_startup(startup_id)` — `ReplayStartup`/`RunComplete` の相関ロジックは
  `visible==true` かつ id 一致でないと no-op になるため、起動進捗系テストの前に必須
- 観測: `run_state()` / `last_run()` / `portfolio()` / `timestamp_ms()` / `venue()` / `exec_mode()` /
  `tickers()` / `available()` / `last_prices()` / `startup_progress()` / `backend_connected()` / `backend_running()` /
  `live_orders()` / `order_feedback()` / `secret_prompt()`

## 落とし穴（事前に知らないと必ずハマる）

- **event seam は一部だけ resource を変える（Phase 9 マージ以降）**。`backend_event_drain_system` は
  `OrderEvent` → `LiveOrders.apply_event`、`AccountEvent` → `apply_account_event`（`PortfolioState`）、
  `SecretRequired` → `SecretPrompt.active` を反映する（= F3/F4/F5 は実装済み・観測可能）。**ただし
  `VenueLogoutDetected` だけは今も `info!` のみで resource を変えない**ので D5 は assert 不可。
  D5 を実装するには `VenueStatusRes` を Disconnected 相当へクリアする本番拡張（`src/backend_sync.rs`）が
  必要 = スコープ拡大。着手前にユーザーへ確認すること。**重要: この欄も含め要約はドリフトしうるので、
  着手時は必ず `src/backend_sync.rs` の `backend_event_drain_system` / `apply_status_update` を実際に
  読んで「どの seam がどの resource を変えるか」を現物確認する**こと。
- **注文 RPC（status seam, Phase 9）**: `OrderSeeded`/`OrderStatusUpdated`/`OrderModified`/`OrderRejected`
  は `apply_status_update` が `LiveOrders`（`upsert_full`/`apply_event`/`apply_modify`）と `OrderFeedback`
  を更新する（FLOWS.md の H セクション=H1〜H5）。`ExecutionModeChanged` は実モード変更時に
  `PortfolioState` を default リセット（Live/Replay 口座データ混線防止）する点が回帰の肝。観測には
  ハーネスの `live_orders()` / `order_feedback()` / `secret_prompt()` アクセサを使う。
- **`TransportCommand` 側（UI → gRPC）はハーネスでは駆動しない**。`SetSpeed`/`StepForward`/`SelectedSymbol`
  のような「UI が backend に投げるコマンド」は seam の手前。v1 は「backend が押し返す ack/clock を UI が
  忠実にミラーする」ことの検証に留める（A2/A3 がこの形）。backend ack の variant が無い挙動
  （例: speed ack）は**保留**にし、FLOWS.md にその理由を明記する。完全な単一プロセスループ
  （コマンド注入→mock gRPC→resource 観測）は transport task（`main.rs setup_backend_connection`）の
  lib 抽出 = 別タスク「Phase A-full」。
- **反対側 seam（`TransportCommand`→gRPC→`BackendStatusUpdate`）は `tests/backend_integration.rs` が
  mock tonic サーバで既にカバー済み**。両者で end-to-end を構成する。重複して書かない。
- **`BackendTradingState` は `Default` を持たない**。clock 以外で必要なら `h.push_state` と同様に
  `serde_json::from_value(json!({"price":0.0,"history":[],"timestamp":0.0, ...}))` で最小構築する
  （必須は `price`/`history`/`timestamp` のみ、他は `#[serde(default)]`）。
- **文字列フィールドは wire フォーマットのまま**。`PortfolioOrder.side`/`.status` は `String`
  （`"BUY"`/`"FILLED"`）。enum 化されていないので文字列リテラルで正しい。
- **既存 warning は触らない**（`main.rs:33 UnsubscribeRequest` 等、本作業と無関係）。新規 warning は増やさない。
- **コメントは「なぜ」だけ**。何をしているかの説明やタスク言及は書かない（プロジェクト規約）。

## テストの型（コピーして埋める）

```rust
/// <FlowID> <name>: <1 行で不変条件>。
#[test]
fn <flow_id>_<snake_name>() {
    let mut h = Harness::new();
    // 1. 前提を整える（必要なら h.begin_startup(id) など）
    // 2. seam を注入
    h.send_status(BackendStatusUpdate::RunStarted);
    assert_eq!(h.run_state(), RunState::Running);
    // 3. 続きの seam を注入し、最終状態を assert
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: None,
        run_id: "run-x".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert_eq!(h.run_state(), RunState::Completed);
}
```

import は `tests/e2e_replay.rs` 冒頭の `use backcast::trading::{...}` / `use backcast::replay::{...}` に
必要な型（`BackendStatusUpdate` の variant が使う enum 等）を足す。

## 完了基準

- `cargo test --test e2e_replay` が全本数 green。
- 観測が「ユーザーが語った挙動」と対応している（resource 遷移がその挙動の十分条件になっている）。
- A8（stale startup_id の相関）/ D7（Live universe が Replay fallback を上書き・prune しない不変条件）の
  ような**回帰の肝**を新規テストで壊していない。
- FLOWS.md のチェックボックスと「実装状況」を更新済み。
