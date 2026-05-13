# Phase 7: Replay UI Integration — Implementation Plan

[Tranceparent Headless Replay](./Tranceparent%20Headless%20Replay.md) Phase 6 で構築した headless replay engine（Replay State Machine + Snapshot Reducer + 制御 API）を、Bevy UI から可視化・制御できる状態にする。`e-station` (Iced ベース) の UI を、本プロジェクトの **Infinite Canvas + Floating Windows** アーキテクチャ（[Infinite Canvas with Bevy Engine](./Infinite%20Canvas%20with%20Bevy%20Engine.md) / [Floating Window on Canvas](./Floating%20Window%20on%20Canvas.md)）に合わせて移植する。

## Goals

1. リプレイの進行（時刻・状態・速度）を UI に常時同期する。
2. メニュー → ファイル選択（`*.py` 戦略ファイル）→ **戦略コードエディタ（monaco 相当）で編集** → リプレイ開始 → エンジン稼働、までの一連の UX を実装する。戦略ファイルは `SCENARIO` dict に instrument / start / end / granularity / initial_cash を内包する想定（`python/tests/data/test_strategy_daily.py` 等を参照）。
3. ローソク足 / Ladder / Buying Power / Positions / Orders の 5 パネル＋ **StrategyEditorWindow** を Bevy の floating window として動作させる。
4. Sidebar（銘柄一覧・設定）と Footer（時刻・トランスポート・FPS）を screen-space UI として常時表示する。Footer の Transport には **⏪ Step-back** を MVP として含める。
5. UI 側のロジックを **Subscription Agnostic** に保ち、backend が Unary polling のままでも streaming に切り替えても動くようにする。

## Non-Goals

- 実取引（Live Venue）との接続は Phase 8 / 9 で扱う。本フェーズはあくまで replay モード。
- バックエンドのインジケータ計算は本フェーズでは導入しない（UI 表示用の簡易 MA のみ Bevy 側で算出）。
- 高度なテーマエディタ / レイアウト保存は Optional とし、骨格のみ提供。

---

## 1. Screen Design / 画面設計

詳細な解説図は [assets/phase7-screen-layout.drawio.svg](../assets/phase7-screen-layout.drawio.svg) を参照。

![Phase 7 Screen Layout](../assets/phase7-screen-layout.drawio.svg)

3 枚構成:

| 図 | 内容 |
|---|---|
| Figure 1 | Dashboard 全体レイアウト — MenuBar / Sidebar / Infinite Canvas + Floating Windows / Footer |
| Figure 2 | Replay Start Modal — File → Open 後に表示されるポップアップ |
| Figure 3 | Bevy ECS コンポーネント階層 — screen-space (bevy_ui) と world-space (Sprite/Transform) の区分、Resources / Systems / Events / gRPC マッピング |

### 1.1 Space の分割（重要な設計判断）

`e-station` は全画面が Iced の widget tree だが、本プロジェクトは「canvas をズーム/パンできる無限空間」を中心に据える。そのため UI を 2 層に分ける:

- **Screen-space (bevy_ui Node)** — ズームの影響を受けない固定 UI:
  - MenuBar（上端）
  - Sidebar（左端）
  - Footer（下端、リプレイトランスポート含む）
  - ModalLayer（ReplayStartModal などのオーバーレイ）
- **World-space (Sprite + Transform)** — `PanCam` で動かせる無限キャンバスに浮かぶウィンドウ:
  - KlineChartWindow / LadderWindow / BuyingPowerPanel / PositionsPanel / OrdersPanel

世界座標側は既存の `WindowRoot` 機構（[src/ui/window.rs](../../src/ui/window.rs)）を拡張して再利用する。

### 1.2 Visual Style Reference（/frontend-design 連携）

実装に着手する直前に `/frontend-design` で **HTML/CSS ピクセル単位ビジュアルリファレンス**を 1 枚生成し、`assets/phase7-visual-reference.html` に保存する。drawio はワイヤフレーム、frontend-design はガラスモーフィズム・タイポグラフィ・色味・ホバー状態などの **見た目の基準**として扱う。Bevy 実装はこのリファレンスを目視で参照しながら近似する。

カラートークン（drawio と統一）:

| トークン | Hex | 用途 |
|---|---|---|
| `bg.canvas` | `#05080f` | 無限キャンバス背景 |
| `bg.window` | `#141a2e` → `#0f1628` gradient | フローティングウィンドウ |
| `border.cyan` | `#00CFFF` | アクセント・選択状態 |
| `accent.green` | `#00FF7F` | BUY / +P&L / FILLED |
| `accent.red` | `#FF3366` | SELL / -P&L |
| `text.primary` | `#e0e8f0` | 通常文字 |
| `text.muted` | `#9fb0c8` | 補助文字 |
| `text.subtle` | `#5a7090` | 補注 |

---

## 2. e-station からの移植マトリクス

| e-station ソース | Phase 7 移植先 | 移植方針 |
|---|---|---|
| `src/menu.rs`, `src/native_menu.rs`, `src/widget_menu_bar.rs` | `src/ui/menu_bar.rs` (新規) | bevy_ui Node の Flexbox で再構築。File → "Open Replay Data..." だけは Phase 7 で必須、他は枠のみ |
| `src/modal/replay_form.rs` (720 行) | (採用しない) | 起動パラメータは戦略ファイル内 `SCENARIO` dict から読み取るため、別モーダルでの入力は不要。File→Open 直後に `StrategyEditorWindow` が開く |
| — (新規) | `src/ui/floating/strategy_editor.rs` (新規) | monaco-editor 相当のコードエディタを載せた floating window。Python シンタックスハイライト + 行番号 + 折りたたみ + `[Load & Start]` ボタン |
| `src/screen/dashboard/sidebar.rs` | `src/ui/sidebar.rs` (新規) | bevy_ui 左固定パネル。Tickers list と Settings の二段 |
| `src/screen/dashboard/panel/buying_power.rs` | `src/ui/floating/buying_power.rs` | world-space floating window |
| `src/screen/dashboard/panel/positions.rs` | `src/ui/floating/positions.rs` | 同上。Text2d でテーブルを描画 |
| `src/screen/dashboard/panel/orders.rs` | `src/ui/floating/orders.rs` | 同上 |
| `src/screen/dashboard/panel/ladder.rs` (1382 行) | `src/ui/floating/ladder.rs` | 同上。MVP は bid/ask × 10 行 + LAST、クリック発注は Phase 9 |
| `src/chart/kline.rs` (2052 行) | 既存 `src/ui/chart.rs` を拡張 | ろうそく足モードを追加（現在は line chart のみ）|
| `src/handlers/replay.rs`, `src/handlers/engine.rs` | `src/trading.rs` 内に gRPC クライアント拡張 | 制御 RPC 群を Tonic で叩く Bevy system に |
| `src/widget/multi_split.rs`, `src/widget/decorate.rs` | (採用しない) | infinite canvas が代替する |

---

## 3. Tasks

### 3.1 Backend 側補強 (Phase 6 の延長)

- **`GetPortfolio` RPC の追加** — `TradingState` から `BuyingPower / Position[] / Order[]` を別 DTO で返す。`GetState` を肥大化させないため分離する。Phase 6.5 の `strategy_runtime` で発行された注文・約定をここに集約する。
- **`SubscribeReplayEvents` (Optional)** — server-streaming で `ReplayTime / Trades / KlineUpdate / OrderEvent / PositionEvent` を push。実装するかは Phase 6 末の判断に従う。実装しない場合は UI は 60 Hz polling のみで動かす。
- **`Step / Pause / Resume / SetSpeed / Stop` の冪等化確認** — UI からの連打耐性。

### 3.2 Bevy UI 共通基盤

- `UiPlugin` を screen-space / world-space / modal の 3 層に分割。
- `Resources`:
  - `ReplayTimeRes { timestamp_ms: i64 }`
  - `ReplayStateRes { phase: ReplayPhase }`（enum: IDLE/LOADED/RUNNING/PAUSED/STOPPING）
  - `ReplaySpeedRes { multiplier: f32 }`
  - `PortfolioStateRes { buying_power, positions, orders }`
  - `SelectedSymbol(Option<TickerId>)`
- `Events`:
  - `OpenReplayRequested` / `ReplayLoaded` / `ReplayStarted` / `ReplayStopped`
- `Systems`:
  - `poll_engine_state_system` — 60 Hz で `GetState` / `GetPortfolio` を叩き Resource を更新
  - `transport_button_system` — フッターの ⏮⏯⏭/Speed を RPC に変換
  - `replay_modal_lifecycle_system` — Open Replay → Modal 表示 → Load → Start
- 既存の `WindowManager`/z-order/drag システムは流用。

### 3.3 Screen-space UI

- **MenuBar** ([src/ui/menu_bar.rs])
  - Flexbox Row, height 36px, 黒紺背景
  - File ドロップダウンに「Open Replay Data...」「Save Layout (stub)」「Exit」
  - File→Open で `rfd::AsyncFileDialog` を起動して `OpenReplayRequested` を発火
- **Sidebar** ([src/ui/sidebar.rs])
  - 幅 200px, 左固定
  - 上半分: Tickers リスト（`PortfolioStateRes` と `ReplayTime` 由来の最新価格を表示、クリックで `SelectedSymbol` 更新）
  - 下半分: Settings（Theme dropdown / Backend address field / Save Layout button — 各 stub OK）
- **Footer** ([src/ui/footer.rs])
  - 高さ 60px, 下固定
  - `ReplayTimeLabel`（monospace 16px）
  - `ReplayStateBadge`（色付きピル: RUNNING=green / PAUSED=yellow / IDLE=gray / STOPPING=red）
  - `TransportControls`（⏮ Step-1 / ⏯ Pause-Resume トグル / ⏭ Step+1）
  - `SpeedSelector`（dropdown: 0.5x / 1x / 2x / 5x / 10x / 50x）
  - `ProgressBar`（cyan）+ パーセント
  - `FpsCounter` + `GrpcStatus`（OK / RECONNECTING / ERROR）
- **ReplayStartModal** ([src/ui/modal/replay_start.rs])
  - `OpenReplayRequested` で表示、`Escape`/`Cancel` で閉じる
  - フィールド: Data Path, Symbols (chip selector), Start/End Date, Speed slider, Initial Buying Power
  - `[Load]` → `LoadReplayData` RPC (IDLE→LOADED)、成功で `[Start Engine]` ボタンを enable
  - `[Start Engine]` → `StartEngine` RPC (LOADED→RUNNING) → モーダル close

### 3.4 World-space Floating Windows

各 floating window は既存の `spawn_trader_window` パターン（`WindowRoot`/`TitleBar`/`Draggable`/`bring-to-front`）を踏襲して `src/ui/floating/{name}.rs` に分離する。共通化のため `spawn_floating_window(commands, title, size, content_builder)` ヘルパーを切り出す。

- **KlineChartWindow** — 既存 `chart.rs` をろうそく足対応に拡張。`PortfolioStateRes` ではなく `TradingState.history` を入力とする。BUY/SELL ボタンはプレースホルダ（Phase 9 で実発注に接続）。
- **LadderWindow** — bid/ask × 10 行 + LAST 行。MVP は depth が空でも LAST 行と数量入力＋BUY/SELL の擬似発注ボタンを描画。
- **BuyingPowerPanel** — 3 行（現金 / 評価額 / 建余力）。`PortfolioStateRes.buying_power` を購読。
- **PositionsPanel** — Sym/Qty/Avg/P&L のテーブル。各行 `Text2d` で描画。
- **OrdersPanel** — Time/Sym/Side/Qty/Price/Status のテーブル。Status の色だけ状態に応じて変える。

### 3.5 Replay Time Sync

- 60 Hz の `poll_engine_state_system` が `GetState` を呼び、結果から `ReplayTimeRes` / `ReplayStateRes` / `TradingState` を更新。
- Footer / KlineChart は Resource の `Changed<>` で再描画。
- streaming 採用時は `tonic` の async stream を `bevy_tokio_tasks` 越しに polling channel に流し込み、同じ Resource を埋めるだけで切り替え可能にする。

### 3.6 Step-back (Optional, 後半)

- バックエンドに `StepBackward(n)` を追加し、Phase 6 の snapshot を ring buffer（既定 512 件）で保持。
- `PortfolioStateRes` も snapshot に含めて巻き戻す（positions/orders の整合性のため）。
- Footer に ⏪ ボタンを追加。

### 3.7 Visual Reference (`/frontend-design`)

実装着手前に `/frontend-design` で `assets/phase7-visual-reference.html` を生成。Bevy 実装中はこの HTML をブラウザで開いて目視リファレンスとする。

---

## 4. File Layout（追加・変更）

```
src/ui/
├── mod.rs                       # plugin 構成を screen/world/modal 3 層に
├── components.rs                # 既存 + ReplayPhase, PortfolioStateRes など
├── window.rs                    # 既存。spawn_floating_window ヘルパー切り出し
├── chart.rs                     # 既存。ろうそく足モード追加
├── button.rs                    # 既存
├── menu_bar.rs        [NEW]
├── sidebar.rs         [NEW]
├── footer.rs          [NEW]
├── floating/
│   ├── mod.rs         [NEW]
│   ├── kline.rs       [NEW]   # KlineChartWindow ラッパ（chart.rs を組む）
│   ├── ladder.rs      [NEW]
│   ├── buying_power.rs [NEW]
│   ├── positions.rs   [NEW]
│   └── orders.rs      [NEW]
└── modal/
    ├── mod.rs         [NEW]
    └── replay_start.rs [NEW]

src/trading.rs                   # gRPC: GetPortfolio / Pause/Resume/Step/SetSpeed 追加
python/engine/                   # GetPortfolio RPC, PortfolioState DTO 実装
docs/
├── assets/
│   ├── phase7-visual-reference.html
└── plan/
    ├── phase7-screen-layout.drawio.svg   [NEW]  ← drawio 出力済
    └── phase7-visual-reference.html       [NEW]  ← /frontend-design で生成（着手時）
```

---

## 5. Implementation Order / 実装順

各ステップで `cargo run` できる状態を維持する。

1. **Step 1 — Footer & Time Sync**: Footer + `poll_engine_state_system` + `ReplayTimeRes`/`ReplayStateRes`。RUNNING 中に時刻と状態バッジが動けば合格。
2. **Step 2 — MenuBar & Modal**: File→Open → ファイル選択ダイアログ → `ReplayStartModal` → `Load` で IDLE→LOADED、`Start Engine` で RUNNING。
3. **Step 3 — Transport Controls**: Footer の ⏮⏯⏭ / Speed を RPC に接続。Pause→Step→Resume が動く。
4. **Step 4 — Sidebar**: Tickers リストと Settings の枠。`SelectedSymbol` に応じてチャートのタイトルが切り替わる。
5. **Step 5 — Visual Reference**: `/frontend-design` で `phase7-visual-reference.html` を生成。以降の floating window 実装の見た目基準にする。
6. **Step 6 — Floating Windows (簡単な順)**: BuyingPower → Positions → Orders → Ladder → Kline（既存 chart の拡張）。
7. **Step 7 — Backend: `GetPortfolio` RPC**: Python 側に DTO を追加し、UI と接続。
8. **Step 8 — Polish**: glassmorphism / rim light / hover / focus z-order。
9. **Step 9 (Optional) — Step-back**: ring buffer + ⏪ ボタン。

---

## 6. Success Criteria

- File → Open → モーダル → Load → Start で IDLE→LOADED→RUNNING の遷移がフッターに反映される。
- リプレイ中、Footer の時刻が連続的に進み、Kline / Ladder / Positions / Orders / BuyingPower がすべて同期する。
- ⏯ Pause / ⏭ Step / Speed 変更が即座に反映される。
- 5 つの floating window をドラッグ・ズーム・前面化できる。
- Sidebar から銘柄を切り替えると Kline と Ladder の対象銘柄が変わる（リプレイは同一セッションを維持）。
- gRPC が `Unary polling` でも `Server streaming` でも UI 側の system を切り替えるだけで動作する。

---

## 7. Open Questions（計画確定前に user 確認）

1. **`GetPortfolio` の置き場所** — Phase 6 のスコープに含めるか、Phase 7 で新規追加するか。本計画は Phase 7 で追加前提。
2. **ファイル選択ダイアログ** — `rfd` を採用してよいか。Bevy 公式パッケージではない外部 crate。
3. **メニューバーの実装方式** — `bevy_ui` 内で自前 Flexbox を組むか、ネイティブメニュー（Windows: WinAPI / mac: AppKit）にするか。本計画は `bevy_ui` 内自前前提（クロスプラットフォーム単純化のため）。
4. **Symbol selector** — Tickers list はサーバから返す（J-Quants の銘柄マスタ）か、ローカル固定リストで開始するか。本計画は Phase 7 内では「ローカル固定 + ReplayData から導出」で開始。
5. **Step-back を Phase 7 に入れるか Phase 7.5 に分けるか**。本計画は Optional 扱い。
