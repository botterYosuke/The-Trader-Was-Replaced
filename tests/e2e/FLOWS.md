# E2E Flow Conventions — The-Trader-Was-Replaced

> リリース前の最後の砦として、ユーザーが取りうる行動を原則すべて列挙し、自動テストの対象にする。
> 実装済み flow は `tests/e2e/flows/<id>.rs`（1 ファイル 1 テスト）に索引化されている。各ファイル先頭の
> `//!` が「何の挙動か / seam / 観測」を記述するため、flow 一覧はこの markdown ではなく `.rs` 群が正本。
> 既存の Bevy resource/event/system 直接駆動ハーネスで観測できるものはこの方式で assert する。
> 直接駆動だけでは忠実に検証できない操作（描画依存、OS ダイアログ、キーボード入力、実 backend/環境依存）は
> 「対象外」にせず、代替方式（UI harness / smoke / integration / manual release gate）を明記する。

## このファイルの使い方（編集ルール）

- **実装済み flow の索引は `tests/e2e/flows/<id>.rs`（1 ファイル 1 テスト）に移行した**。flow を1本足す
  = `tests/e2e/flows/<id>.rs` を追加し、`tests/e2e_replay.rs`（runner）に `#[path]` + `mod` を1行足す。
- このファイルは **flow 一覧そのものではなく**、(1) 駆動の縫い目と観測の凡例、(2) 直接駆動では headless
  検証できない操作の代替方式と release gate、(3) ハーネス計画 を定義するメタ文書。
- まだ `.rs` 化していない操作（UI / integration / render / manual-gate 系）は、末尾の「直接駆動では
  不可能な場合の代替方式」テーブルに方式と release gate を記録する。

### wiki ↔ E2E 同期ルール（必須）

- E2E flow 群（`tests/e2e/flows/*.rs` ＋ 下の代替方式テーブル）は `docs/wiki/`（実アプリの操作説明書）と
  **対**になっている。wiki に書かれたユーザー可視の挙動は、原則対応する flow を持つ。
- **`docs/wiki/` の操作挙動を変更・追加したら、必ず E2E flow を見直す**:
  新しい挙動 → 実装可能なら `tests/e2e/flows/<id>.rs` を追加、headless 観測不能なら下のテーブルに行を追加。
  挙動の削除・変更 → 対応する `.rs` flow / テーブル行を削除・修正。
- backend→ECS seam を通らない挙動（クライアント側 gating / 純 UI / 描画依存 / backend 内部ガード）も
  除外しない。直接駆動ハーネスで不可能な場合は、末尾の「**直接駆動では不可能な場合の代替方式**」に
  方式と release gate を記録する。

### 凡例

- **seam** … 入力する縫い目（`TransportCommand` 列挙子 / `BackendEvent` / resource 更新）
- **観測** … assert 対象の resource とその状態遷移
- **be** … 想定バックエンド: `mock`（決定論的・CI 向き） / `real`（python -m engine・忠実度確認） / `none`
- **kind** … `state`（resource/event/system 直接駆動） / `ui`（Bevy UI harness: Interaction/Keyboard/MouseWheel/Pointer を注入） /
  `render`（実ウィンドウまたは画像/ログ smoke） / `integration`（backend/CLI/環境依存） / `manual-gate`（自動化不能時のリリース手順）
- **優先** … ★★★ 高 / ★★ 中 / ★ 低

### 駆動の縫い目（参照）

- 入力: `TransportCommandSender`（`mpsc<TransportCommand>`）にコマンドを直接送る → UI ボタン描画をバイパス
- 入力(イベント): backend → ECS の `BackendStatusUpdate` / `BackendEvent` をモックから注入
- 入力(UI): Bevy の `Interaction` / `ButtonInput<KeyCode>` / `MouseWheel` / pointer event / focused entity を直接注入
- 出力(観測): `LastRunResult.state`(RunState) / `PortfolioState` / `BackendStatus` /
  `VenueStatusRes` / `ExecutionModeRes` / `Tickers` / `TickersStatus` / `AvailableInstruments` /
  `LastPrices` / `SelectedSymbol` / `ReplaySpeed` / `TradingSession` / `ReplayStartupProgress`
- 出力(UI/描画): window/panel entity 構造、`Visibility`/`Display`/`Text`/`Style`、layout JSON、strategy cache、
  render/screenshot smoke、構造化ログ

---

## 直接駆動では不可能な場合の代替方式

この節は「テストしないもの」ではない。既存の resource/event/system 直接駆動ハーネスだけではユーザー操作の忠実度が足りない場合に、採用する代替方式を定義する。

release gate 列の ID（`I*` / `J*` / `K*` / `L*` 群、保留中の `A5` / `C5` / `D8` など）は **まだ `.rs` 化していない planned flow** の識別子。実装時に `tests/e2e/flows/<id>.rs`（または `kind:render` / `kind:integration` の専用ハーネス）として起こす。

実装済みの代替方式 flow:

- **I5 ✅** `tests/e2e/flows/i5_file_open_spawns_editor_and_chart.rs`（`kind:integration`）— temp `.json` を `LayoutLoadRequested{UserJsonOpen}` で開き、本番の `apply_layout_system` → `panel_spawn_dispatcher_system` で Strategy Editor、`instrument_chart_sync_system` で Chart の entity spawn を assert（rfd ダイアログはバイパス、cosmic はフォント resource のみ headless 挿入）。残る画面 smoke は `L4`。

| 対象 | 直接駆動だけで不足する理由 | 代替方式 | release gate |
|---|---|---|---|
| メニュー開閉 / Alt+F/E/V | backend seam を通らず、keyboard focus と UI entity 表示が本体 | `kind:ui`。`ButtonInput<KeyCode>` と `Interaction` を注入し `OpenMenu` / entity 表示を assert | I1/I3 必須 |
| モード切替 gating | command を送らないことが仕様で、backend ack では観測できない | `kind:ui`。送信 channel を監視し「未送信」を assert | I4 必須 |
| OS ファイルダイアログ | CI で OS native dialog を安定操作しにくい | dialog 自体はバイパスし、選択済み path event/resource を注入。別途 smoke で起動確認 | I5 ✅ + L4 |
| レイアウト永続化 | ファイル I/O と debounce が主対象 | temp dir fixture で `Save/Load` を integration 実行し JSON と復元 entity を assert | I7/I8 必須 |
| cosmic_edit 入力 | text editor plugin の focus/keyboard 処理が主対象 | `kind:ui`。focused entity と keyboard/text input を注入。必要なら最小実ウィンドウ smoke を追加 | J1-J4 必須 |
| Startup パネル入力検証 | Run command を送らない UI gating が仕様 | `kind:ui`。field editor state、error label、transport channel 未送信/送信を assert | J5/J6 必須 |
| `instruments_ref` fail-closed | file-watch / parser / writeback の連携 | temp sidecar/ref file を使う integration。破損・空・正常の fixture を固定 | J7 必須 |
| 銘柄ピッカー | searchbox、debounce、候補表示、readonly が純 UI | `kind:ui`。time advance と text/entity assert。取得 seam は C1-C4 と組み合わせる | J8/J9 必須 |
| Chart 操作 | wheel/drag/double click と render state が主対象 | `kind:ui` で `ChartViewState` / camera を assert。描画崩れは `kind:render` smoke | K2/K3 + L4 |
| 注文フォーム / modal / context menu | 2 段階 confirm、focus、Escape 優先順位が主対象 | `kind:ui`。command channel、modal visibility、feedback resource を assert | K4/K5/K6 必須 |
| Prod guard / 実 venue | CI で実口座・外部環境に依存 | env isolated backend integration で guard を確認。実接続はリリース時 manual-gate に残す | L3 必須 |
| 画面全体の見た目 | headless resource assert では重なり・欠落を検出しづらい | `BACKCAST_E2E=1` 固定 fixture 起動、スクリーンショットまたは構造化 UI dump の smoke | L4 必須 |

## ハーネス計画（参考・別途実装）

- **済**: 各 flow を手書きの `tests/e2e/flows/<id>.rs` として実装し、`tests/e2e_replay.rs` が
  `MinimalPlugins` の headless App に `BackendStatusUpdate` / `BackendEvent` / replay clock を注入して
  resource を assert する（`tests/e2e/support/mod.rs` の `Harness`）。CI 向き。
- **Phase A-full（未）**: App 組み立てとトランスポートタスクを `main.rs` から lib へ抽出し、
  `TransportCommand` 注入 → mock gRPC（`backend_integration.rs` の `MyDataEngine` を `tests/e2e/support/`
  へ共有抽出）→ `RunState` 観測 の単一プロセスループを閉じる。現状は片側（`BackendStatusUpdate` 注入）のみ。
- **Phase B（未）**: `--e2e` / `BACKCAST_E2E=1` のウィンドウ実行モード（固定ウィンドウ・固定パス・
  構造化ログ）で、`.rs` flow と同じシナリオを実描画で smoke 実行（`kind:render` / `L4`）。

### ディレクトリ構成

```
tests/
├── e2e_replay.rs     ← runner（#[path] で flows/ と support/ を取り込む単一テストバイナリ）
└── e2e/
    ├── FLOWS.md      ← このメタ文書（凡例 / 代替方式 / 計画）
    ├── flows/        ← 各 flow の Rust テスト *.rs（1 ファイル 1 #[test]・先頭 //! が解説）
    ├── fixtures/     ← strategy .py / scenario sidecar JSON など素材（未作成・将来用）
    └── support/      ← 共有 Rust ヘルパ（headless app builder = Harness / mock engine）
```
