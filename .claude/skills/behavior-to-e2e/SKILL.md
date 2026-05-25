---
name: behavior-to-e2e
description: >-
  The-Trader-Was-Replaced（Bevy + Python gRPC）で、ユーザーが「こう動いてほしい / こうなったら困る /
  この挙動を保証して」と挙動を言葉で説明したとき、それを **リリース前の E2E / release-gate 項目** に変換するスキル。
  `tests/e2e/FLOWS.md` に flow を追加し、実装可能なものは `tests/e2e_replay.rs` や UI/integration/render harness に
  自動テストを足す。backend→ECS seam（`BackendStatusUpdate` / `BackendEvent` / replay clock）だけでなく、
  Bevy UI 入力、layout/file I/O、CLI/backend integration、実ウィンドウ smoke も対象にする。
  必ず起動する場面: 「この挙動をテストにして」「〜したら〜になることを保証したい」「〜が壊れてないか自動で確認したい」
  「リプレイが完走することをテスト」「Run が失敗したら error が出るのを担保」「ポートフォリオが反映されるか」
  「venue ログインの状態遷移をテスト」「銘柄リストの取得失敗をテスト」「回帰テストを追加」「E2E を1本足して」
  「FLOWS.md の flow を実装」「backend からこのイベントが来たら UI がこうなる、をテスト」
  「メニュー/エディタ/チャート/モーダル/レイアウトの操作をリリース前に保証したい」と言われたとき。
  **着手した issue / 計画書の本文が「behavior-to-e2e」を名指ししている、または新しい flow ID（例 M12 /
  N1）の追加＋ `tests/e2e/FLOWS.md` 追記＋ wiki `[FlowID]` 引用をひとまとめに指示しているときは、ユーザーが
  他スキル（`bevy-engine` / `pair-relay` / `plan` 等）を明示していても本スキルを併発する**
  （`feat(ui):` で M/N 群の release-gate flow を新設するタスクが典型。テスト＋FLOWS.md＋wiki が
  「実装の付録」ではなく本スキルの本体なので、実装スキルに気を取られて取りこぼさない）。
  **`docs/wiki` 等のドキュメント/wiki レビューで「実装済みなのに『未実装』『開発中』『将来』扱い」の機能が
  見つかった**ときも本スキルを開く（「wiki を修正」「ドキュメントを実装に追従」と言われたら併発）: その機能は
  E2E 未カバーのことが多い（実装が先行し doc も flow も置き去り）。wiki を現行化しつつ、対応 flow が
  `tests/e2e/flows/` に無ければ追加し、wiki 本文に `[FlowID]` を引く。
  **「未実装扱い」だけでなく、UI 機構のリファクタで wiki が *旧機構* を記述したまま食い違っている**
  ケースも同様に本スキルの対象（例: 「サイドバーの Startup パネル」→ floating window 化、`Node.display`→`Visibility`、
  Bevy-UI → sprite 化、`×` ボタンの有無変更）。「サイドバーパネルを floating window に作り替える」「UI を
  別ホストに移植する」「wiki が実装と食い違う / 旧 UI を記述している」作業に着手したら、コードと同時に
  `docs/wiki/*.md` の該当機構記述（"サイドバー" / "パネル" 等）を grep して現行化する。例: Phase 10 Live Auto
  （LiveStrategyEvent / SafetyRailViolation / StrategyLogMessage / LiveStrategyTelemetry /
  LiveStrategyPromoteResult）は wiki が「未実装」と書く一方で flow 皆無 → N 群（`kind:state`）を新設。
  **「この挙動をテストするテストはあるか」「既にテストされているか」「テストでカバーされているか確認したい」
  「FLOWS.md に該当 flow があるか」「カバレッジを確認」のような“テスト作成”でなく“カバレッジ照会”の問いでも必ず起動する**
  （既存 flow / e2e_replay / Python テストを棚卸しし、足りない分だけ flow 化する。穴が headless 不可なら fake せず
  doc gap として FLOWS.md に明記する）。
  **既存 flow が root/枠だけを assert していて子・中身・深い不変条件を取りこぼしている『カバレッジギャップ』を
  埋める**ときも本スキルを開く（issue の [MEDIUM]/follow-up で「m12 は root の `Visibility` しか見ていない」
  「子（editor/gutter/scrollbar）まで隠れることを検証していない」「m11 のように Parent 連鎖で
  `InheritedVisibility` を辿る assert を足す」と指摘されたら、その flow を実 entity spawn 版に強化し
  FLOWS.md の該当行も追従させる。m8→m11、#33→m12 が実例。新規 flow ID の採番ではなく既存 flow の強化なので
  「新しい flow を足す」トリガーから漏れやすいが本スキルの本体）。
  さらに **`e2e_replay` が全本落ちる / ハーネスが panic する / `could not access system parameter` が出た /
  マージ後に E2E が壊れた / `BackendEvent`・`BackendStatusUpdate` などの列挙子にフィールドが増えて
  `e2e_replay` が `missing field` でコンパイルできない（テストドリフト）**ときの**ハーネス修復**も本スキルの対象
  （必要 resource の insert 漏れ・列挙子フィールド追従漏れが定番原因）。
  **Phase ブランチ（`sasa/N-...`）や E2E ブランチ（`docs/e2e`）を main にマージする / マージの健全性を
  確認する**ときも、壊れてから直すのではなく着手時に本スキルを開く: まずマージが Rust
  （`src/**`/`.proto`/`tests/*.rs`）を触るかを確認し、触るなら各 system の `Res`/`ResMut` 引数を grep して
  新規 resource が `tests/e2e/support/mod.rs` の `Harness` に insert 漏れしていないか先に確認する
  （例: Phase 10 で `LiveRuns` / `PromoteFeedback` 追加 → 入れ忘れて全本 panic）。
  既存方式で不可能な場合も「対象外」にせず、代替方式（`kind:ui` / `kind:render` / `kind:integration` /
  `kind:manual-gate`）を FLOWS.md に明記する。
  **バグ修正（issue 実装）で CLAUDE.md の「先に RED テスト → コード修正 → GREEN」フローが求められる場合も必ず本スキルを
  発動する**: Python 側のバグでも Rust 契約テスト（`BackendStatusUpdate` seam での contract flow）が Acceptance Criteria
  に含まれていれば D9 のような flow + FLOWS.md 追記 + wiki [FlowID] 引用が必要。`pair-relay` の Navigator が内部で
  カバーしても、本スキルを明示的に invoke しないと flow/wiki 追加が「実装の付録」として埋もれやすい（実例: #39 Slice 1
  で D9 flow + venues.md + modes.md 更新が必要だったが behavior-to-e2e を invoke せず、pair-relay Navigator 任せになった）。
  **さらに重要: 初期プロンプトが「レビューして修正して」「Medium をつぶして」のような /pair-relay・code-review ループで、
  flow/wiki の必要性が *レビュー途中で判明する*（codex/Navigator が新しい不変条件・取りこぼしを指摘 → 新 flow ID 追加や
  FLOWS.md/wiki の現行化が要る）パターンでも、その時点で本スキルを invoke する**。初期プロンプトに flow/wiki 意図が無く
  ても、レビュー駆動で挙動を変えた / 新 flow を足すと決まった瞬間が発動点（実例: #39 Slice 2 のレビューで AccountEvent
  ゲートを新設し D22/D23 + modes.md を更新したが、また behavior-to-e2e を invoke せず Navigator 任せになった＝Slice 1 と
  同じ取りこぼしの再発）。レビュー中に設計が変わったら、先に書いた FLOWS.md の Mechanism 列 / wiki が *旧機構を記述したまま*
  食い違う事故が起きやすい（Slice 2 では D23 の Mechanism 列が撤去済みの `_publish_account_snapshot` gate を指したまま残り
  最終レビューで Medium 指摘になった）ので、設計変更のたびに FLOWS.md/wiki の該当記述も同時に追従させる。
  **さらに、headless 不可で `#[ignore]`/doc-stub のまま諦めていた flow を「実テスト化」する**ときも本スキルを開く
  （「i8/i14 を headless テスト可能にする」「`#[test] #[ignore]` を外したい」「rfd / ファイルダイアログ /
  `AsyncComputeTaskPool` / async task を seam でバイパスしてテスト」「ダイアログ要求と書き込みを分離してテスト可能に」
  「Save As の書き込み結果を assert したい」と言われたとき）: production に最小の test seam を足して
  （None 分岐を event 委譲に変える・`inject_resolved` 等の注入口を Resource に足す・private system を `pub` 化する）
  `AsyncComputeTaskPool`/rfd を一切踏まずに書き込み側だけを駆動し、FLOWS.md の当該 flow を `#[ignore]`→✅ /
  代替方式テーブルを更新する。production 経路は無変更に保つ（注入口は本番では誰も呼ばない）。`tdd`（RED→GREEN の
  vertical slice）と `pair-relay`（Navigator/Driver 分業）を併用すると seam 設計と回帰防止が安定する。
  Rust の一般的なユニットテスト作法は `rust-testing` を併用する。
  **Python gRPC backend の挙動を保証したい**ときも本スキルを開く（「EC stream イベントで account が更新されることをテスト」
  「account_sync の dedup をテスト」「server_grpc の挙動をテスト」「Python の backend 挙動を E2E で保証したい」
  「Slice N の Python 側テストを書く」）: Rust ECS seam だけでなく `kind:integration`（Python pytest）の flow として
  FLOWS.md に追加し `python/tests/` に自動テストを足す。EC stream → force_resync トリガー、mode 遷移 →
  account_sync 存続、dedup 保証など「Python サービス内の状態機械」は pytest でカバーできる（Rust seam は不要）。
  **この場合も FLOWS.md への flow 追加・wiki の [FlowID] 引用は必須**（Rust E2E に限らない）。
  **「verify first」パターン（issue に「まず混入するか確認してから修正」「RED が立つか先に検証」「本当に再現するか確かめてから直す」と書かれているとき）でも本スキルを発動する**: verify-first はテストを先に書いて問題を実証するアプローチであり、RED テスト + FLOWS.md 追記 + wiki [FlowID] が必要。issue の Acceptance Criteria に「verify first」が含まれていれば、実装の説明が詳細でもスキルを invoke する（#39 Slice 2 の「verify first: live 接続状態で Replay に切替えたとき混入するか確認（RED が立つか）」が典型）。
---

# behavior-to-e2e — 挙動の言葉を E2E テストに変える

ユーザーが日本語で語った「こうあってほしい挙動」を、`tests/e2e/FLOWS.md` の flow と、対応する自動テストまたは
release-gate 項目に落とす。既存の `tests/e2e_replay.rs` は backend→ECS seam を検証する state harness だが、
リリース前の最後の砦としてはそれだけでは足りない。ユーザーが取りうる操作は原則すべてカタログ化し、
可能な限り自動テストにする。採用中の方式で忠実に検証できない場合も除外せず、代替方式を明記する。

## なぜこの形なのか（先に理解する）

本アプリには複数の検証面がある。backend（Python gRPC、replay/live runner）が push する状態は
`BackendStatusUpdate` / `BackendEvent` / replay clock を注入すれば deterministic に検証できる。一方で、
メニュー、モード切替 gating、Strategy Editor、Startup パネル、銘柄ピッカー、チャート操作、注文フォーム、
モーダル、レイアウト保存は、ユーザー入力・Bevy UI entity・ファイル I/O・描画 state が本体であり、
backend→ECS seam だけでは十分条件にならない。

したがって flow には必ず `kind` を割り当てる:

| kind | 使う場面 | 主な観測 |
|---|---|---|
| `state` | backend→ECS seam で十分に検証できる挙動 | resource 状態 |
| `ui` | Bevy UI 操作、キーボード、focus、modal、gating | `Interaction` / `ButtonInput<KeyCode>` / text input 注入、entity `Text` / `Display` / command channel |
| `render` | headless resource では画面崩れを検出しづらい挙動 | 実ウィンドウ smoke、スクリーンショット、構造化 UI dump |
| `integration` | CLI、backend、file I/O、env guard、実プロセス | temp fixture、出力ファイル、gRPC 応答、exit status |
| `manual-gate` | 実口座など自動化が危険または不可能なもの | 手順、期待結果、実施記録。必ず自動 smoke と組み合わせる |

## ワークフロー

1. **挙動を 1 文の不変条件に言い換える**。「Run したら完了する」→「`RunStarted` の後 `RunComplete` を受けると
   `RunState` が `Running`→`Completed` になり `parsed_summary` が埋まる」。UI 操作なら
   「クリック/入力後、どの entity/resource/file/command が変わればユーザー行動が保証されたと言えるか」まで落とす。
   曖昧なら何を観測すれば「動いた」と言えるかをユーザーに確認する。
2. **FLOWS.md に該当 flow があるか見る**。あれば ID（A1/B2/D3…）と seam・観測がそのまま設計。
   なければ新規 flow として、近いセクション（A〜L）に `- [ ]` 行を追記してよい。
   ⚠️ **「この挙動は既にテストされているか」を判定するときは `tests/e2e/` だけ見て『未カバー』と結論しない**。
   spawn/dedup/視認性のような **UI system は `src/ui/**` の `#[cfg(test)] mod tests` に unit テストが
   ある**ことが多い（dispatcher の spawn/重複は `tests/e2e/flows/m1,m5` ではなく `floating_window.rs` の
   `order_dispatcher_tests` にある等、system 本体の隣に置かれる）。`grep -rn '<system名>\|<Marker>' src/ tests/`
   で **src と tests の両方**を当たってから gap を申告する（#25 の確認で、dispatcher Order arm を
   m1/m5 だけ見て「欠落」と誤判定し、実際は floating_window.rs unit テストで既出だった）。
   ⚠️ **新規 ID を採番する前に衝突を必ず確認する**: FLOWS.md の「保留中の `A5`/`C5`/`D8` など」リストと
   `docs/wiki/**` の `[<ID>]` 参照を両方 grep する。**保留中（planned）の ID が wiki では既に別挙動に
   bind 済み**ということがある（例: #32 Slice 2 で C5 を採ろうとしたら、FLOWS.md は C5 を planned 扱いの一方
   wiki venues.md が `[C5]`=`SelectedSymbol` 更新の planned flow に既に割当済みだった → C6 に採番し直した）。
   `grep -rn '\[C5\]' docs/wiki tests/e2e/FLOWS.md` で空でない ID は避ける。
3. **kind を決める**。backend→ECS の resource 変化だけで十分なら `kind:state`。UI 操作・入力・未送信 gating は
   `kind:ui`。ファイル/CLI/backend/env は `kind:integration`。見た目崩れや overlap は `kind:render`。
   実口座など自動化が危険なら `kind:manual-gate` だが、必ず自動 smoke と組み合わせる。
4. **state flow は seam を特定する**（下表）。`src/backend_sync.rs` の `apply_status_update` と
   `backend_event_drain_system` が「どの `BackendStatusUpdate` / `BackendEvent` がどの resource をどう変えるか」の
   正解。推測せずここを読む。
5. **ui flow はユーザー入力 seam を特定する**。`Interaction::Pressed`、`ButtonInput<KeyCode>`、`MouseWheel`、
   text input、focused entity、time advance、command channel のどれを注入し、どの entity/resource/file を assert するかを決める。
   「command を送らない」ことが仕様なら、transport channel を監視して未送信を assert する。
6. **integration/render/manual-gate flow は代替方式を書く**。OS file dialog は選択済み path event/resource を注入する。
   実ウィンドウ smoke は `BACKCAST_E2E=1` の固定 fixture、スクリーンショットまたは構造化 UI dump で確認する。
   実 venue は env isolated backend integration と手順付き manual gate に分ける。
7. **実装する**。`kind:state` は `tests/e2e_replay.rs` に `#[test]` を足す。`Harness::new()` → seam を send →
   観測を assert。`kind:ui` / `kind:integration` / `kind:render` は既存 harness が無ければ `tests/e2e/support/`
   に薄い helper を足し、flow ごとに最小実装する。
8. **観測アクセサが無ければ Harness に足す**（`tests/e2e/support/mod.rs`）。既存の `portfolio()` と同形:
   `self.app.world().resource::<T>().clone()`。`BackendStatus` は `Clone` 非実装なのでフラグを直接返す
   （`backend_connected()` 等の前例あり）。
9. **走らせる**。state harness は `cargo test --test e2e_replay`。**初回コンパイルは Bevy 全体をリンクするため ~11 分**
   かかる（バックグラウンド実行推奨）。integration/render は flow に書いた release gate command を実行する。green を確認。
10. **FLOWS.md を更新**: 実装した flow を `- [x]` にし、末尾「実装状況」に 1 行追記。
11. 完了したら `CLAUDE.md` 規約に従い `simplify` と `post-impl-skill-update` を発動。

## state seam → resource 早見表

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

## UI / integration flow 早見表

| 検証したい挙動 | 入力 seam | 観測 |
|---|---|---|
| メニュー開閉 | `Alt+F/E/V`、menu button `Interaction::Pressed`、`Escape` | `OpenMenu`、menu entity の `Display` / `Visibility` |
| モード切替 gating | Replay/Manual/Auto segment click | `TransportCommandSender` の受信有無、error/disabled 表示 |
| File Open | OS dialog をバイパスし selected path event/resource を注入 | sidecar/strategy load、Strategy Editor spawn、scenario metadata |
| レイアウト保存/復元 | window move/close/resize、Save/Load、time advance | temp sidecar JSON、復元後 `WindowRoot` / viewport |
| Strategy Editor 入力 | focused editor + key/text input | `StrategyFragment`、cache file、history、find panel state |
| Startup パネル検証 | field edit + Run button | error label、Run command 未送信/送信、sidecar writeback |
| 銘柄ピッカー | `+ Add`、search text、candidate click、remove | candidate rows、placeholder text、scenario instruments、readonly |
| Chart 操作 | wheel、Ctrl+wheel、drag、double click | `ChartViewState`、camera pan/zoom、autoscale |
| 注文 UI / modal | form input、submit、confirm、context menu、Escape | command channel、modal visibility、feedback resource |
| CLI/backend | process command、gRPC request、env guard | exit status、stdout、files、gRPC response |
| render smoke | `BACKCAST_E2E=1` fixed fixture | screenshot or structured UI dump baseline |

## kind:integration レシピ — File Open → パネル spawn を headless で駆動

`tests/e2e/flows/i5_file_open_spawns_editor_and_chart.rs` が手本。state `Harness` は使わず、bare `App` に**必要な system だけ**を載せる（`UiPlugin` / `LayoutPersistencePlugin` 全体を足すと save/shortcut 系が `ButtonInput`/`Time` 等を要求し resource whack-a-mole になる）。要点:

- **seam**: temp `.json`（`windows:[{kind:"StrategyEditor", region_key:"region_001", ...}]`・`strategy_path:null`）を書き、`LayoutLoadRequested{ path, mode: LayoutLoadMode::UserJsonOpen }` を `send_event`。`apply_layout_system` は **`strategy_path` 付きだと `.py` ロード待ちに defer** するが、`strategy_path:null` + `windows` だと同フレームで `PanelSpawnRequested` を直接送る（headless で素直なのはこちら）。
- **載せる system（すべて pub）**: `apply_layout_system` → `panel_spawn_dispatcher_system`（Strategy Editor spawn）→ `instrument_chart_sync_system`（Chart spawn）を `.chain()` で順序固定。`instrument_chart_sync_system` は `registry.is_changed()` で early-return するので `InstrumentRegistry` を **insert してから**初回 `update()` で spawn される。`scenario` JSON → `InstrumentRegistry` の parse は `scenario_parser` 単体テスト持ちなので registry は直接 insert してよい。
- **resource**: apply_layout=`WindowManager`/`PendingLayoutApply`/`PendingStrategyFragments`/`ScenarioReadTarget` + `Camera2d` entity（`get_single_mut`）。dispatcher=`CosmicFontSystem`/`RegionKeyAllocator`/`AppHistory`/`PendingStrategyFragments`/`StrategyBuffer`。chart sync=`InstrumentRegistry`/`InstrumentTradingDataMap`。event=`LayoutLoadRequested`/`PanelSpawnRequested`/`StrategyFileLoadRequested` を `add_event`。
- **観測**: `query_filtered::<(), (With<StrategyEditorId>, With<WindowRoot>)>()` 件数 ≥ 1、`query_filtered::<&ChartInstrument, With<WindowRoot>>()` に対象 `instrument_id`。
- **cosmic / 依存の罠は `bevy-engine` スキル参照**（`CosmicFontSystem(FontSystem::new())` の手構築、`cosmic-text` を dev-dep 追加、外部 tests/ は normal deps は引けるが transitive は引けない）。

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

- **ハーネスは wire した system が要求する resource を全部 insert していないと全テストが即死する**。
  `Harness::with_backend_enabled`（`mod.rs`）は `status_update_system` / `backend_update_system` /
  `backend_event_drain_system` / `replay_startup_timeout_system` が取る `ResMut<T>` を**手で全部
  `insert_resource(T::default())` している**。これらの system に新しい resource param が増えたら
  （例: Phase 9 hardening が `status_update_system` に `ResMut<ReconcilePrompt>` を追加）、
  **ハーネスにも同じ 1 行を足さないと**全 e2e_replay が
  `... could not access system parameter ResMut<'_, T>` で 0.05 秒のうちに 35 本まとめてパニックする
  （bevy_ecs の system-param validation）。パニックは `bevy_ecs/.../function_system.rs` を指すので
  ハーネス漏れだと気づきにくい。**正解セットは `src/main.rs` の `insert_resource(...)` 群**＝ハーネスは
  これをミラーする。**ブランチをマージした直後は特に要注意**（片方が system に resource を足し、
  もう片方がハーネスを持つと、テキスト無衝突でも合体時に必ずこれで壊れる）。
- **フェーズマージの健全性確認は「そもそも Rust を触っているか」から始める**。上記のハーネス／テスト
  ダブル破壊は全て Rust 側 (`src/**`/`.proto`/`tests/*.rs`) が変わって初めて起きる。マージ後
  `git diff --cached --name-only HEAD | grep -E '\.rs$|\.proto$|Cargo'` が**空なら**ハーネスは壊れようが
  なく、検証面は Python+docs だけ（`uv run python -m pytest -m "not slow and not kabu_live"`）。空でなければ
  上のチェックを実施し、最後に `cargo test --test e2e_replay`（全 flow green が正＝現状 87 passed /
  2 ignored）で締める。例: Phase 10 Step 8（`7125ff35`）のマージは Python+docs のみ＝Rust e2e は不変、
  Phase 10 Step 9 は `backend_event_drain_system` に `SafetyToast`/`StrategyLogs` を足したので
  ハーネスに 2 行 insert が要った。
- **`git diff --stat main <branch>` の「削除」表示に騙されない（diff 方向の罠）**。これは対称差なので
  「main にあって branch に無い」ファイル（例: e2e_replay.rs / docs/wiki/*）が大量に `---` 削除と出るが、
  マージはそれらを**消さない**（merge-base にも branch にも無ければ touch されない）。マージが実際に何を
  するかは `git show --stat <commit>`（そのコミット自身の diff）で見、マージ後は
  `git diff --cached --diff-filter=D HEAD`（空＝削除なし）で確認する。
- **マージ後に e2e が 1〜数本だけ落ちたら、まず単体で再実行して flake と回帰を切り分ける**。
  `cargo test --test e2e_replay <name>` が単体で green なら、それはマージが**起こした**回帰ではなく、
  プロセスグローバル状態（`std::env::set_var` の env var 等）を複数テストが並列で奪い合う既存の隔離バグを、
  マージのスレッドタイミング変化が**露出させた**だけ。`#[serial]`（`serial_test`、前例 `l3_prod_guard`）で
  該当テストを直列化する。例: Phase 10 Step 9 マージで `BACKCAST_CACHE_DIR` を使う i12/i5/i7/j16/j1 が
  並列衝突し i12 が散発失敗 → 5 本に `#[serial]` 付与で解消。
- **`BackendEvent` / `BackendStatusUpdate` の variant にフィールドが増えると e2e の構築が壊れる**。
  Phase ブランチをマージすると `OrderEvent` / `OrderSeeded` 等にフィールドが足される（例: Phase 10 で
  `OrderEvent.strategy_id` と `BackendStatusUpdate::OrderSeeded.strategy_id`）。テストが
  `tests/e2e/flows/*.rs` に分割された後はドリフトが多数の flow に散るので、`cargo test --test e2e_replay
  --no-run` で `missing field`(E0063) を列挙し、`grep -rn 'OrderSeeded\|BackendEvent::OrderEvent'
  tests/e2e/flows/` で構築箇所を特定して新フィールドを足す（未使用値は `String::new()` 等の中立値で OK＝
  assert 対象でなければ挙動は変わらない）。`OrderSeeded {` / `OrderEvent {` 行でアンカーして該当箇所だけ
  直す（一括置換は誤爆する）。なお e2e ブランチが Phase より前に分岐していると、gutted な
  `tests/e2e_replay.rs`（flow を `#[path]` で束ねるだけのファイル）は `git checkout --theirs` で branch 版を
  採り、登録 flow が main の test 関数の superset であることを確認してから採用する（coverage を落とさない）。
  なお backend 側の gRPC 拡張（新 RPC）は `tests/backend_integration.rs` のモック `impl DataEngine` が
  trait 未実装(E0046)になる別件 — マージ後チェックリストは memory `phase-merge-breaks-test-doubles` 参照。
- **event seam は一部だけ resource を変える（Phase 9 マージ以降）**。`backend_event_drain_system` は
  `OrderEvent` → `LiveOrders.apply_event`、`AccountEvent` → `apply_account_event`（`PortfolioState`）、
  `SecretRequired` → `SecretPrompt.active`、`VenueLogoutDetected` → `ReloginPrompt.active` を反映する
  （= F3/F4/F5/D5 は実装済み・観測可能）。**重要: この欄も含め要約はドリフトしうるので、
  着手時は必ず `src/backend_sync.rs` の `backend_event_drain_system` / `apply_status_update` を実際に
  読んで「どの seam がどの resource を変えるか」を現物確認する**こと。
- **注文 RPC（status seam, Phase 9）**: `OrderSeeded`/`OrderStatusUpdated`/`OrderModified`/`OrderRejected`
  は `apply_status_update` が `LiveOrders`（`upsert_full`/`apply_event`/`apply_modify`）と `OrderFeedback`
  を更新する（FLOWS.md の H セクション=H1〜H5）。`ExecutionModeChanged` は実モード変更時に
  `PortfolioState` を default リセット（Live/Replay 口座データ混線防止）する点が回帰の肝。観測には
  ハーネスの `live_orders()` / `order_feedback()` / `secret_prompt()` アクセサを使う。
- **`TransportCommand` 側（UI → gRPC）は state harness だけでは駆動しない**。`SetSpeed`/`SelectedSymbol`
  のような「UI が backend に投げるコマンド」は backend→ECS seam の手前。これを対象外にせず、
  `kind:ui` で command channel への送信/未送信を assert する。backend ack の variant が無い挙動
  （例: speed ack）は state flow だけで完結させず、UI command 発行テストと transport/integration テストに分ける。
  完全な単一プロセスループ（コマンド注入→mock gRPC→resource 観測）は transport task
  （`main.rs setup_backend_connection`）の lib 抽出 = 別タスク「Phase A-full」。
- **反対側 seam（`TransportCommand`→gRPC→`BackendStatusUpdate`）は `tests/backend_integration.rs` が
  mock tonic サーバで既にカバーしている部分がある**。重複を避けつつ、ユーザー操作として未保証なら
  `kind:ui` または `kind:integration` の flow を追加する。
- **`BackendTradingState` は `Default` を持たない**。clock 以外で必要なら `h.push_state` と同様に
  `serde_json::from_value(json!({"price":0.0,"history":[],"timestamp":0.0, ...}))` で最小構築する
  （必須は `price`/`history`/`timestamp` のみ、他は `#[serde(default)]`）。
- **文字列フィールドは wire フォーマットのまま**。`PortfolioOrder.side`/`.status` は `String`
  （`"BUY"`/`"FILLED"`）。enum 化されていないので文字列リテラルで正しい。
- **bare `App`（UI flow）には input プラグインが無く `ButtonInput<KeyCode>` はフレーム境界で自動 clear されない**。
  `just_pressed` が前フレームから sticky に残り、既に pressed のキーへの再 `press()` は no-op（just_pressed を作らない）。
  各キー操作の前に `keys.reset_all()` してから `press(...)` し直すこと。Escape の連続押下・トグル系（メニュー Alt+F、
  注文/モーダルの Escape 優先テスト）で「2 回目が効かない」のは大抵これ。同様に **`Time::<()>` の delta は最後の
  `advance_by` 値で固定**（`update()` で 0 に戻らない）。cooldown 等を「時間未経過」で検証したいときは
  `advance_by(Duration::ZERO)` で delta=0 にし、cooldown 解除には `advance_by(1s)` する。
- **`tests/e2e/support/mod.rs` の `Harness` は UI 駆動メソッド（`run_via_ui`/`click<M>`/`drain_commands`/`set_replay_state`）
  を持つ拡張版で、A/B/C 群の flow がこれらに依存する**。failing な 1 本を直すために `git checkout HEAD -- mod.rs`
  したり mod.rs / 他 flow を安易に上書きしないこと（未コミットの拡張を消すと 10+ 本が `no method named run_via_ui`
  で全落ちし、working-tree のみの内容は復元不能になりうる）。共有ファイル（mod.rs / runner / 別 flow）を触る前に
  「その working-tree 版に依存する未コミット flow が無いか」を必ず確認する。詳細は memory `e2e-harness-extended-ui-driven`。
- **`click<M>(marker)` の仕組み = `(marker, Button, Interaction::Pressed)` を spawn して `tick()` 1 回**。新規追加した
  `Interaction` は `Changed<Interaction>` に必ずヒットするので、本番ハンドラ（`*_button_system`）が**ちょうど 1 回**発火する
  （実マウス押下と同じ経路）。毎回新 entity なので同じボタンを連続クリックしても再発火する。producer→consumer
  （例 `footer_pause_resume_system`→`handle_strategy_run_system`、remove→`unsubscribe_removed_instruments_system`）は
  harness 側で `.chain()` 済みなので 1 tick で「クリック→`StrategyRunRequested`→`RunStrategy` コマンド」まで通る。
  発射コマンドは `drain_commands()` で受ける。**「実 UI 操作で `TransportCommand` を assert → その後 backend 応答を
  seam から注入」**が A–H の基本パターン。
- **command-level テスト（resource 直 seed ＋ 合成 entity spawn）は実機 wiring を素通りする＝false-green の温床**。
  `Harness` で `set_xxx()` により resource を直接埋め、`click<M>` で `(marker, Button, Interaction)` を**手で spawn**して
  system を回すテストは、「branch logic が完璧入力で正しく動く」ことしか保証しない。**本番 plugin の system 登録漏れ・
  本番 `spawn_xxx` が作る実 entity のマーカー/可視性・`Node.display` gating・pre-flight guard の充足経路**は一切踏まない。
  実例（issue #40 フォローアップ）: footer ▶ の LiveAuto 起動を `N5`（command-level）が「`StartLiveAuto` の送出有無」だけ
  assert して green だったが、実機では ▶ が無反応だった。原因は pre-flight guard が `warn!`+`continue` の **silent block**
  （venue 未接続等）で、N5 はそれを「送らないのが正」として暗黙に許容していた（＝抜け漏れ）。
  **gap を疑ったら、`i5`/`N6`/`N7` の bare-App パターンで本番経路を踏むテストを足す**: `App::new()`＋`MinimalPlugins`＋
  `AssetPlugin`＋`init_asset::<Font>()` に **本番 `spawn_footer`（等の構築 system）を Startup で 1 回回し**、本番の
  visibility/handler system を `add_systems` して、`query_filtered::<Entity, With<RealMarker>>()` で引いた**実 entity**を
  `entity_mut(e).insert(Interaction::Pressed)` で押す。これで「登録・実 entity・可視性・guard」まで丸ごと検証できる
  （resource は `make_app` で本番 `main.rs` と同じ insert セットを揃える＝1 つ漏れると system-param panic）。
- **「クリックしても何も起きない」系のバグは silent guard（`warn!`+`continue` だけで UI に何も出さない）を最優先で疑う**。
  挙動を「保証」するテストは「コマンドが出る/出ない」だけでなく **「ブロック時にユーザーへ理由が surfacing される」**まで
  assert する（例 N7: pre-flight 失敗時に `LastRunResult.state=RunState::Failed{error}` を書き Run Result パネルへ赤字表示）。
  silent block を「送らないのが正」とだけ固定すると、無言の無反応を仕様として温存してしまう。
- **`push_state(ts)` は `TradingSession.replay_state` を `None` に上書きする**（fixture に replay_state が無いため）。
  footer の Pause/Resume は `replay_state` で分岐するので、**`set_replay_state(Some("RUNNING"))` は必ず `push_state` の
  「後」に呼ぶ**。順序を逆にすると clock push が RUNNING を消し、Pause クリックが Run 扱いになって command assert が落ちる
  （A2 で踏んだ）。`unsubscribe_removed_instruments_system` は mode 切替 frame を skip するので、削除クリックの前に
  `set_instruments` + 安定 tick を 1 回挟んで Local の prev 集合を整えてから × ボタンを押す（F2）。
- **共有 runner（`tests/e2e_replay.rs`）の登録は orchestrator が一括で行う**。並行 subagent に書かせると重複登録・
  順序衝突・cargo の target ロック競合が起きる。subagent には「flow ファイルだけ書く / cargo も runner も触らない」と
  明示し、登録・コンパイル・修正は中央で回す。
- **headless 不可 / 未実装の flow は fake せず doc stub（`//!` のみ・`#[test]` 無し）にして runner 未登録のまま残す**。
  `kind:render`（ShapePainter+Text2d=GPU 必要）、Windows 専用 PowerShell、実ウィンドウ smoke、production 未実装機能が該当。
  外部データ依存（catalog / J-Quants）や OS dialog（rfd 直呼び）は `#[test] #[ignore]` + 理由 doc にする。
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

## wiki への引用元記載（必須）

テストを追加・更新したら、対応する `docs/wiki/` のページに `[FlowID]` を書く。

1. flow が説明する挙動が wiki のどのページに書かれているかを確認する。
2. そのページの冒頭に引用元注記がなければ追加する（既存ページの書き方に倣う）:
   ```
   > 文中の `[A1]` などは、その挙動を保証する E2E flow の ID。一覧は [`tests/e2e/FLOWS.md`](../../tests/e2e/FLOWS.md) を参照。
   ```
3. 挙動の記述箇所（手順・表・説明文）に `[FlowID]` をインラインで付ける（例: `Run を開始 [A1]`）。
4. 対応する wiki ページが存在しない場合は記載不要（FLOWS.md の flow 行にその旨をコメントする）。

現時点で引用済みのページ: `replay.md` / `venues.md` / `orders.md` / `modes.md` / `getting-started.md`。
未記載のページ: `backtest.md` / `strategy.md` / `troubleshooting.md` / `windows-and-panels.md` など。

## 完了基準

- 追加・変更した flow の `kind` に対応する release gate が green。
  `kind:state` は `cargo test --test e2e_replay`、`kind:ui` / `kind:integration` / `kind:render` は
  flow に記載した command または harness、`kind:manual-gate` は手順・期待結果・実施条件が明記されている。
- **例外: 既知バグの回帰ガードは「RED で確定」が完了**。ユーザーが「このバグをテストにして」と
  挙動の **崩れ** を語り、fix を別 issue に委ねるとき（`/to-issues` 併発が典型）、テストは
  わざと **RED**（バグを再現して fail）させて登録する。このとき完了基準は ①RED が**正しい理由で
  落ちる**こと（wiring/compile エラーではなく assert で fail。`cargo test --test e2e_replay <id>` の
  panic メッセージで確認）②他 flow を巻き込まない（`N passed; 1 failed`）③FLOWS.md に「RED＝回帰ガード・
  fix は #issue 後に green」と明記し ✅ にしない、の 3 点。fix 実装時に green へ反転し FLOWS.md を ✅ に更新する。
- 観測が「ユーザーが語った挙動」と対応している。resource 遷移、UI entity、command channel、file output、
  screenshot/structured dump のいずれかが、その挙動の十分条件になっている。
- A8（stale startup_id の相関）/ D7（Live universe が Replay fallback を上書き・prune しない不変条件）の
  ような**回帰の肝**を新規テストで壊していない。
- FLOWS.md のチェックボックスと「実装状況」を更新済み。
- 対応する wiki ページに `[FlowID]` の引用元を記載済み。
