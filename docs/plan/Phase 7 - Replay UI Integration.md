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

詳細な解説図は [assets/phase7-screen-layout.drawio.svg](assets/phase7-screen-layout.drawio.svg) を参照。

![Phase 7 Screen Layout](assets/phase7-screen-layout.drawio.svg)

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

- **`TradingState` に `replay_state` を追加** — 既存の `GetState` JSON に `replay_state: str`（`"IDLE"` / `"LOADED"` / `"RUNNING"` / `"PAUSED"` / `"STOPPING"`）フィールドを追加し、Rust 側 `BackendTradingState` で受け取れるようにする。これが Footer の状態バッジの唯一のソース。
- **`GetPortfolio` RPC の追加** — `TradingState` から `BuyingPower / Position[] / Order[]` を別 DTO で返す。`GetState` を肥大化させないため分離する。Phase 6.5 の `strategy_runtime` で発行された注文・約定をここに集約する。
- **`LoadStrategy` RPC の追加** — UI から戦略ファイルのパスと**編集後のソース文字列**を受け取り、`SCENARIO` dict を解析して replay window を確定 → in-process で戦略を import / instantiate する。`schema_version` をチェックし不一致なら明示的に reject。
- **`StepBackward(n)` RPC を MVP に含める** — Phase 6 の snapshot ring buffer（既定 512 件）と組み合わせて、`PortfolioStateRes`/`positions`/`orders` を含む完全な状態を巻き戻す。Optional ではなく必須。
- **`SubscribeReplayEvents` (Optional)** — server-streaming で `ReplayTime / Trades / KlineUpdate / OrderEvent / PositionEvent` を push。実装するかは Phase 6 末の判断に従う。実装しない場合は UI は 60 Hz polling のみで動かす。
- **`Step / Pause / Resume / SetSpeed / Stop / StepBackward` の冪等化確認** — UI からの連打耐性。

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
  - File ドロップダウンに「Open Strategy...」「Save Layout (stub)」「Exit」
  - File→Open で `rfd::AsyncFileDialog` を起動して `*.py` のみフィルタ、`OpenStrategyRequested(path)` を発火
  - 既定ディレクトリは `python/tests/data/`（`test_strategy_daily.py` / `test_strategy_minute.py` / `pair_trade_minute.py` がある場所）
- **Sidebar** ([src/ui/sidebar.rs])
  - 幅 200px, 左固定
  - 上半分: Tickers リスト（`PortfolioStateRes` と `ReplayTime` 由来の最新価格を表示、クリックで `SelectedSymbol` 更新）
    - **銘柄マスタの導出**: `SCENARIO.end_date` のディレクトリ配下（例: `S:/j-quants/2024/04/15/`）の CSV ファイル名からシンボルを列挙する。スキャンは `LoadStrategy` RPC の完了後、バックエンドが `LOADED` に遷移したタイミングで行う。スキャン結果は `Tickers` Resource として保持。
  - 下半分: Settings（Theme dropdown / Backend address field / Save Layout button — 各 stub OK）
- **Footer** ([src/ui/footer.rs])
  - 高さ 60px, 下固定
  - `ReplayTimeLabel`（monospace 16px）
  - `ReplayStateBadge`（色付きピル: RUNNING=green / PAUSED=yellow / IDLE=gray / STOPPING=red）
  - `TransportControls`（⏪ Step-back / ⏮ Jump-to-start / ⏯ Pause-Resume トグル / ⏭ Step+1）
  - `SpeedSelector`（dropdown: 0.5x / 1x / 2x / 5x / 10x / 50x）
  - `ProgressBar`（cyan）+ パーセント
  - `FpsCounter` + `GrpcStatus`（OK / RECONNECTING / ERROR）
- **（ReplayStartModal は廃止）** — 起動パラメータは戦略ファイルの `SCENARIO` dict に集約されたので、別モーダルは設けない。File→Open 直後に `StrategyEditorWindow`（§3.4）が world-space に出現し、そこの `[Load & Start]` ボタンで `LoadStrategy` → `StartEngine` を一気に走らせる。

### 3.4 World-space Floating Windows

各 floating window は既存の `spawn_trader_window` パターン（`WindowRoot`/`TitleBar`/`Draggable`/`bring-to-front`）を踏襲して `src/ui/floating/{name}.rs` に分離する。共通化のため `spawn_floating_window(commands, title, size, content_builder)` ヘルパーを切り出す。

- **StrategyEditorWindow** ([src/ui/floating/strategy_editor.rs]) — monaco-editor 相当のコードエディタ。File→Open でファイルパスを受け取り、ファイル内容を読み込んで表示・編集。
  - 機能: Python シンタックスハイライト / 行番号 / 行折りたたみ / Find & Replace / Undo-Redo / オートインデント
  - ヘッダ: ファイル名表示、`[Save]` / `[Load & Start]` / `[Reload from disk]`、ダーティマーク（`●` 印）
  - **UI 状態の保存場所（採用方針）**: floating window の位置・サイズ・z-order・可視性、infinite canvas の pan / zoom、選択銘柄などの **UI 状態は戦略 `.py` ファイル自体に埋め込む**（`SCENARIO` / `LIVE_SCENARIO` と同じ「戦略ファイル＝単一ソース」ポリシー）。
    - 埋め込み形式: ファイル末尾に **センチネルブロック**を置く。Rust から安全に書き換えるため、AST 解析や dict literal の rewrite ではなく、行ベースで全置換できる単純なフォーマットにする。
      ```python
      # === UI_LAYOUT_BEGIN (auto-generated; do not edit by hand) ===
      UI_LAYOUT = {
          "schema_version": 1,
          "viewport": {"pan_x": 0.0, "pan_y": 0.0, "zoom": 1.0},
          "windows": {
              "kline":         {"x": 100, "y": 80,  "w": 800, "h": 500, "z": 0, "visible": True},
              "ladder":        {"x": 920, "y": 80,  "w": 320, "h": 500, "z": 1, "visible": True},
              "buying_power":  {"x": 100, "y": 600, "w": 300, "h": 120, "z": 2, "visible": True},
              "positions":     {"x": 420, "y": 600, "w": 500, "h": 200, "z": 3, "visible": True},
              "orders":        {"x": 940, "y": 600, "w": 500, "h": 200, "z": 4, "visible": True},
              "strategy_editor": {"x": 50, "y": 50, "w": 900, "h": 700, "z": 5, "visible": True},
          },
          "selected_symbol": "1301.TSE",
      }
      # === UI_LAYOUT_END ===
      ```
    - 書き換え戦略: `# === UI_LAYOUT_BEGIN ` 行から `# === UI_LAYOUT_END ===` 行までを Rust 側で正規表現マッチして丸ごと差し替える（中身は Rust struct → `serde_json` → Python literal 風 pretty-print）。AST は触らない。
    - ブロックが存在しない戦略ファイル（既存サンプル等）を開いたときは、**ファイル末尾に新規追加**する形で初回書き込み。Python としては未使用の module-level dict なので副作用ゼロ。
    - 読み込み: アプリは Python AST を持たないので、同じ正規表現でブロックを抽出 → ブロック内の `UI_LAYOUT = {...}` を **JSON5 互換パーサ**（`json5` crate 等）で読む。Python の `True`/`False`/`None` は JSON5 が扱えないので、書き出し時に `true`/`false`/`null` に正規化しておく（Python 側はこの dict を実行しないので問題なし）。
      実装簡略化のため、書き出しを Python 風（`True`/`False`/`None`）ではなく **JSON 風**（`true`/`false`/`null`）に統一する案も可。表示上は Python ファイル内に JSON が埋まる形になるが、`UI_LAYOUT = json.loads("""...""")` のラッパで Python からも読めるようにする手もある。**MVP は JSON 風で統一**。
  - **キャッシュフォルダ運用（採用方針）**: 編集中の状態は OS 標準のキャッシュディレクトリにミラーリングして運用する。元ファイルは `[Save]` するまで触らない。UI 状態の自動保存先も**キャッシュ内のコピー**（同じ `UI_LAYOUT` ブロック）。
    - キャッシュ位置: `dirs::cache_dir()` 配下に `the-trader-was-replaced/strategy_buffers/` を作る（Windows: `%LOCALAPPDATA%\the-trader-was-replaced\cache\strategy_buffers\`、Linux: `~/.cache/the-trader-was-replaced/strategy_buffers/`、macOS: `~/Library/Caches/the-trader-was-replaced/strategy_buffers/`）
    - キャッシュファイル名: `{sha256(original_abs_path)[..16]}__{original_filename}` ＋ サイドカー JSON `{...}.meta.json` に `original_path` / `last_modified_ms` / `dirty: bool` を保存
    - File→Open のフロー: ① 元ファイルを開く → ② キャッシュにコピー → ③ エディタはキャッシュファイルを「作業ファイル」として読み書き → ④ 元ファイルには触らない
    - 自動保存: **値の変更イベント駆動**（タイマーは使わない）。`egui::TextEdit` の `response.changed()` が立った時だけキャッシュへ書き出す。書き込みは `bevy_tokio_tasks` 経由の非同期 I/O で UI フレームをブロックしない。タイピング中は変更があるフレームでのみ I/O が走るため、無編集時はゼロコスト。
    - `[Load & Start]`: キャッシュをフラッシュ（同期書き込み）→ **キャッシュのパス**を `LoadStrategy` RPC へ送る（元ファイルは送らない／編集中の内容で実行される）
    - `[Save]`: キャッシュ → 元ファイル へコピー（`dirty: false` に更新）
    - `[Reload from disk]`: 元ファイルで キャッシュを上書き（編集内容は破棄、確認ダイアログあり）
    - 起動時の復元: 同じ元ファイルを再度開いたとき、キャッシュの `last_modified_ms` が元ファイルより新しい & `dirty: true` ならモーダル「未保存の変更があります。復元しますか？ [復元] [破棄]」
    - クリーンアップ: `[Save]` 直後やユーザが明示的に閉じたタブのキャッシュは即削除しない（=次回も復元できる）。`.meta.json` の `dirty: false` で 30 日経過したものを起動時に GC。
  - **実装方針（確定）**: `bevy_egui` + `egui_code_editor` (v0.2.23) を採用。`egui::Window` 内にウィジェットを1行で配置できるターンキー構成。Python syntax は `Syntax` struct にキーワードセットを手動定義（~30 行）。精度向上が必要になったときのみ `egui_extras` の `syntect` feature を差し込む（差し替え不要、レイヤー追加のみ）。Undo/Redo は `TextEdit` 標準 undo + Bevy `Resource` の `Vec<String>` スナップショットスタックで対応。`wry`/`tao` WebView 路線は採用しない。
- **KlineChartWindow** — 既存 `chart.rs` をろうそく足対応に拡張。`PortfolioStateRes` ではなく `TradingState.history` を入力とする。BUY/SELL ボタンはプレースホルダ（Phase 9 で実発注に接続）。
- **LadderWindow** — bid/ask × 10 行 + LAST 行。MVP は depth が空でも LAST 行と数量入力＋BUY/SELL の擬似発注ボタンを描画。
- **BuyingPowerPanel** — 3 行（現金 / 評価額 / 建余力）。`PortfolioStateRes.buying_power` を購読。
- **PositionsPanel** — Sym/Qty/Avg/P&L のテーブル。各行 `Text2d` で描画。
- **OrdersPanel** — Time/Sym/Side/Qty/Price/Status のテーブル。Status の色だけ状態に応じて変える。

### 3.5 Replay Time Sync

- 60 Hz の `poll_engine_state_system` が `GetState` を呼び、結果から `ReplayTimeRes` / `ReplayStateRes` / `TradingState` を更新。
- Footer / KlineChart は Resource の `Changed<>` で再描画。
- streaming 採用時は `tonic` の async stream を `bevy_tokio_tasks` 越しに polling channel に流し込み、同じ Resource を埋めるだけで切り替え可能にする。

### 3.6 Step-back (MVP / 必須)

- バックエンドに `StepBackward(n)` を追加し、Phase 6 の snapshot を ring buffer（既定 512 件）で保持。
- `PortfolioStateRes` も snapshot に含めて巻き戻す（positions/orders の整合性のため）。
- Footer に ⏪ ボタンを追加。連打した場合は ring buffer の最古を超えないようクランプ、超えたら no-op。
- streaming 採用時は巻き戻し直後に `ReplayStateRes` を強制再同期して UI の整合性を保つ。

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
└── floating/
    ├── mod.rs         [NEW]
    ├── strategy_editor.rs [NEW]   # monaco 相当のコードエディタ floating window
    ├── kline.rs       [NEW]   # KlineChartWindow ラッパ（chart.rs を組む）
    ├── ladder.rs      [NEW]
    ├── buying_power.rs [NEW]
    ├── positions.rs   [NEW]
    └── orders.rs      [NEW]

src/trading.rs                   # gRPC: GetPortfolio / Pause/Resume/Step/SetSpeed 追加
python/engine/models.py          # TradingState に replay_state: Optional[str] 追加
python/engine/core.py            # get_current_state() に replay_state を含める
python/engine/server_grpc.py     # GetPortfolio RPC, LoadStrategy RPC 追加
python/engine/portfolio.py       [NEW]  # PortfolioState DTO + GetPortfolio 集約ロジック
docs/plan/assets/
├── phase7-screen-layout.drawio.svg  [DONE]  ← drawio 出力済
└── phase7-visual-reference.html     [TODO]  ← /frontend-design で生成（Step 6 着手時）
```

---

## 5. Implementation Order / 実装順

各ステップで `cargo run` できる状態を維持する。

1. **Step 1 — Footer & Time Sync**:
   - **Backend**: `TradingState` に `replay_state: Optional[str]` を追加 → `get_current_state()` に含める（後方互換: デフォルト `None`）。
   - **Rust**: `BackendTradingState` / `TradingData` に `replay_state` フィールドを追加。`ReplayPhase` enum + `ReplayTimeRes` / `ReplayStateRes` Resource を定義。
   - **Bevy UI**: bevy_ui Node ベースの Footer を実装（`src/ui/footer.rs`）。`ReplayTimeLabel` / `ReplayStateBadge` / Transport ボタン（⏪⏯⏭ / Speed — この Step ではログのみ） / FPS カウンタ / gRPC ステータスを表示。
   - **合格基準**: backend RUNNING 時にフッターの時刻が更新され、状態バッジが色付きで切り替わる。
2. **Step 2 — MenuBar & File→Open**: File→Open → `*.py` ファイル選択 → `OpenStrategyRequested` 発火。この時点ではログ出力だけで OK。
3. **Step 3 — StrategyEditorWindow (MVP)**: `bevy_egui` + `egui_code_editor` を導入し、File→Open で受け取ったパスのファイル内容を表示・編集。`[Load & Start]` で `LoadStrategy` + `StartEngine` を呼ぶ。
4. **Step 4 — Transport Controls (Step-back 含む)**: Footer の ⏪⏯⏭ / Speed を RPC に接続。Pause→Step→Resume→Step-back が動く。
5. **Step 5 — Sidebar**: Tickers リストと Settings の枠。`SelectedSymbol` に応じてチャートのタイトルが切り替わる。
6. **Step 6 — Visual Reference**: `/frontend-design` で `phase7-visual-reference.html` を生成。以降の floating window 実装の見た目基準にする。
7. **Step 7 — Floating Windows (簡単な順)**: BuyingPower → Positions → Orders → Ladder → Kline（既存 chart の拡張）。
8. **Step 8 — Backend: `GetPortfolio` / `LoadStrategy` / `StepBackward` RPC**: Python 側に DTO と RPC を追加し、UI と接続。
9. **Step 9 — Polish**: glassmorphism / rim light / hover / focus z-order。

---

## 6. Success Criteria

- File → Open（`*.py`） → `StrategyEditorWindow` 表示 → コード編集 → `[Load & Start]` で IDLE→LOADED→RUNNING の遷移がフッターに反映される。
- 上記が `python/tests/data/test_strategy_daily.py` / `test_strategy_minute.py` / `pair_trade_minute.py` の **3 ファイルすべて**で動く（`SCENARIO` の granularity が `Daily` / `Minute` 双方で機能する）。
- リプレイ中、Footer の時刻が連続的に進み、Kline / Ladder / Positions / Orders / BuyingPower がすべて同期する。
- ⏯ Pause / ⏭ Step / **⏪ Step-back** / Speed 変更が即座に反映される。Step-back は positions / orders / buying_power も含めて正しく巻き戻る。
- 6 つの floating window（StrategyEditor + 5 panel）をドラッグ・ズーム・前面化できる。
- Sidebar から銘柄を切り替えると Kline と Ladder の対象銘柄が変わる（リプレイは同一セッションを維持）。
- gRPC が `Unary polling` でも `Server streaming` でも UI 側の system を切り替えるだけで動作する。

---

## 7. Open Questions

### 確定済み

1. ✅ **`GetPortfolio` RPC** → Phase 7 で新設。`GetState` との分離理由: 更新頻度が異なる（約定時のみ変化）ため将来 `SubscribePortfolio` として独立させやすい。in-process gRPC なので 2 本叩くコストは無視できる。
2. ✅ **ファイル選択ダイアログ** → `rfd::AsyncFileDialog` 採用。`*.py` フィルタ、既定ディレクトリ `python/tests/data/`。
3. ✅ **メニューバー** → `bevy_ui` 自前 Flexbox。ネイティブメニューは使わない。
4. ✅ **銘柄マスタ** → **end-date CSV からスキャンして導出**。`SCENARIO.end_date` のディレクトリ配下の CSV ファイル名から symbol を列挙し `Tickers` リストを構築する。サーバから取らない・固定リストも使わない。
5. ✅ **Step-back** → MVP 必須（§3.6）。

6. ✅ **コードエディタの実装方式** → `bevy_egui` + `egui_code_editor` (v0.2.23) 確定。Python syntax は `Syntax` struct でキーワードセット手動定義。精度向上時のみ `egui_extras/syntect` を追加。`wry`/`tao` WebView 路線は採用しない。`bevy_ui` と `bevy_egui` の同居・focus 競合・IME は実装時に確認。
7. ✅ **編集バッファとディスクファイルの関係** → **キャッシュフォルダ方式**で確定（§3.4 StrategyEditorWindow 詳細参照）。File→Open で元ファイルを `dirs::cache_dir()/the-trader-was-replaced/strategy_buffers/` にコピーし、エディタはキャッシュ側を読み書き。元ファイルは `[Save]` するまで触らない。`[Load & Start]` はキャッシュをフラッシュしてキャッシュパスを RPC に送る。クラッシュ復元・差分の汚染回避を同時に達成。
8. ✅ **UI 状態（floating window 位置・viewport・選択銘柄）の保存場所** → **戦略 `.py` ファイル内に `UI_LAYOUT` センチネルブロックとして埋め込む**（§3.4 参照）。`SCENARIO` / `LIVE_SCENARIO` と同じ「戦略ファイル＝単一ソース」ポリシーに揃える。書き換えは末尾ブロックの正規表現置換のみ（AST 不触）、書式は JSON 風で統一。
