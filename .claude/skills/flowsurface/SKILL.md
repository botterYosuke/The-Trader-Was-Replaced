---
name: flowsurface
description: The-Trader-Was-Replaced の Bevy フロントエンド (src/ui/**) で、**チャート / オーダーブック / DOM ヒートマップ / footprint / time & sales / ladder / ティッカーテーブル / マルチペイン / レイアウト永続化 / テーマエディタ / トースト通知** 系の UI を新規/改修するときの必読スキル。flowsurface (iced 製、production-grade な crypto チャート GUI) の完全ソースミラー (.claude/skills/flowsurface/src/) を「金融データ可視化の先行事例」として参照し、iced (Elm 風 retained-mode) / canvas::Program / Subscription / Task / palette を Bevy ECS + bevy_egui + bevy_vector_shapes (ShapePainter) + bevy_cosmic_edit + bevy_tasks に翻訳して提案する。flowsurface が同種機能を持つ UI 全般で発動する — チャート種別 (candlestick, heatmap, footprint, comparison, indicator overlay, volume profile, price/time axis scaling, zoom/pan), DOM / 板系 (L2 ladder, time & sales, depth diff highlight, snapshot+diff merge), tickers (table, search/filter, sort, fav, pane→ticker 束縛), マルチペイン (split, multi-split, drag-resize, multi window, pane linking), 永続化 (layout JSON / config / version migrate), テーマ (color picker, theme editor, palette, semantic bid/ask 色), 通知 (toast, audio cue on trade, network reconnect 表示), アグリゲーション (time/tick bucket, 価格 binning)。トリガー語: "flowsurface", "iced", "canvas::Program", "Subscription", "Task::perform", "TradingView", "Bookmap", "DOM heatmap", "DOM ヒートマップ", "板ヒートマップ", "footprint", "フットプリント", "candlestick", "ローソク足", "kline", "time & sales", "T&S", "Time and Sales", "ladder", "ラダー", "板", "L2 orderbook", "L2 板", "depth chart", "デプスチャート", "volume profile", "ボリュームプロファイル", "indicator overlay", "インジケーター", "price scale", "time scale", "tick aggregation", "ティック集約", "price binning", "価格グルーピング", "tickers table", "ティッカーテーブル", "pane linking", "ペイン連動", "multi-split", "split pane", "ペイン分割", "theme editor", "テーマエディタ", "color picker", "カラーピッカー", "toast", "トースト", "audio cue", "WebSocket reconnect", "rate limiter", "exchange adapter", "src/ui/chart.rs", "src/ui/sidebar.rs", "ShapePainter", "bevy_vector_shapes"。src/ui/chart.rs や DOM/板/tickers 系を Edit/Write しようとする前に必ず読むこと (iced の retained-mode と Bevy の immediate-mode の差で設計を間違える)。Bevy ECS / WGPU / bevy_egui の一般論は bevy-engine スキル、テキストエディタ系は zed スキル、Rust テスト戦略は rust-testing スキルへ。
---

# flowsurface — Bevy 取引 UI を flowsurface 参照で設計するスキル

## 何のためのスキルか

The-Trader-Was-Replaced の Bevy フロントエンドで、**チャート / オーダーブック / DOM / ティッカーリスト / マルチペイン** 系の UI を新規/改修するときに、毎回ゼロから設計するのではなく、**flowsurface (iced 製、production-grade な crypto チャート GUI) の「動いている実装」を先行事例として参照しながら**、それを Bevy ECS + bevy_egui + bevy_vector_shapes に翻訳して提案するためのスキル。

flowsurface は Rust + iced 0.14 製の、ローソク足 / DOM ヒートマップ (歴史的板) / footprint / time & sales / DOM ladder / 比較ライン などのチャート + 多取引所 WebSocket 接続 + マルチウィンドウ + 永続レイアウト + テーマエディタ + トーストを備えた "金融データ可視化のリアル先行事例"。我々の `src/ui/chart.rs` / `sidebar.rs` / `orders.rs` / `positions.rs` / `buying_power.rs` / `floating_window.rs` / `layout_persistence.rs` などで欲しがる機能のほとんどは flowsurface のどこかに既に「責務分割された形」で存在する。

iced と Bevy はパラダイムが違う (iced = Elm 風 retained-mode + `Application::update/view/subscription` + `canvas::Cache`、Bevy = ECS + 即時描画 `ShapePainter` + `bevy_egui` immediate-mode) ので写経はできないが、「どう責務分割するか」「どこに edge case が潜むか」「どの計算を pure data layer に切るか」を読むには flowsurface を 5 分眺めるのが最速。`data/` crate (純粋データ構造) は iced 非依存なので**ほぼそのまま参考にできる**のも大きい。

## いつ発動するか / いつ発動しないか

**発動する:**
- `src/ui/{chart,sidebar,orders,positions,buying_power,footer,menu_bar,floating_window,layout_persistence,run_result_panel,instrument_picker}.rs` を Edit/Write しようとしている
- flowsurface が同種機能を持つ UI を新規/改修する:
  - **チャート系** — candlestick (kline), DOM heatmap, footprint cluster, comparison line, indicator overlay, volume profile, price-axis / time-axis scaling, zoom/pan, crosshair
  - **DOM / 板系** — L2 ladder, time & sales (trade list), depth chart, snapshot+diff merge, 板の差分 highlight
  - **ティッカーリスト系** — tickers table, search/filter, sort by metric, fav/pin, pane→ticker 束縛, ミニ ticker picker
  - **マルチペイン / レイアウト系** — split pane, multi-split (column-drag), drag-resize, multi window, pane linking, layout JSON 永続化, layout manager (保存/ロード)
  - **接続管理系** — exchange adapter trait, WebSocket stream multiplex, REST fetcher with rate limit, reconnect, network status 表示
  - **テーマ / 色** — color picker, theme editor, palette, dark/light, semantic color (bid/ask, bull/bear)
  - **通知 / 設定 / 音** — toast, settings modal, audio cue on trade (閾値ベース)
  - **アグリゲーション** — 時間ベース (1s/1m/15m/1h), tick ベース, 価格 binning (Decimal で off-by-one 回避)
- 「flowsurface だとどう書いてる?」「TradingView/Bookmap っぽい ...」「DOM heatmap みたいな ...」「ladder UI 欲しい ...」という要望
- 新しいパネル種別 (footprint / depth ladder / time & sales / heatmap 等) を `src/ui` に増やす

**発動しない:**
- テキストエディタ系 (シンタックスハイライト / gutter / picker / command palette / strategy_editor) → **`zed`** スキル
- Bevy ECS / Camera2d / Sprite / bevy_egui / observer / required components の一般論 → **`bevy-engine`** スキル
- バックエンド (Python/gRPC、戦略実行、nautilus_trader、scenario runner、cache) → **`nautilus_trader`** スキル
- 立花証券 / kabuステーション API → **`tachibana`** / **`kabusapi`** スキル
- Rust テスト戦略 → **`rust-testing`** / **`tdd-workflow`** スキル
- E2E 手動検証 → **`e2e-testing`** スキル

## 前提知識 (これは先に把握すること)

### 我々のスタック
- **Bevy 0.15** ECS — system / Resource / Component / Query / EventReader / Observer
- **bevy_egui** — modal / overlay / 設定 UI / picker / combobox
- **bevy_vector_shapes** `ShapePainter` — ローソク / ライン / 矩形の**即時描画** (`src/ui/chart.rs` が既に採用)
- **bevy_cosmic_edit** — テキスト入力 (search box, settings)
- **bevy_tasks** `AsyncComputeTaskPool` / `IoTaskPool` — 非同期データ取得
- **共通既存ピース** — `src/ui/chart_{viewstate,render,axes}.rs` (Phase 7.3 で `chart.rs` から分割、ShapePainter candlestick + ViewState + axis labels), `sidebar.rs` (virtual scroll + tickers), `floating_window.rs`, `layout_persistence.rs`, `components.rs` (色トークン + Component 型), `instrument_picker.rs`
- バックエンドは **gRPC + nautilus_trader / 立花 / kabuステーション**。flowsurface のように **UI が直接 WebSocket → 取引所** ではない (UI は gRPC ストリーム受信のみ)

並用前提: `bevy-engine` スキルで `add_systems` タプル 20 上限・observer・required components・Anchor の罠を先に押さえること。

### flowsurface のスタック (我々には**ない**もの)
- **iced 0.14** — Elm 風 retained-mode GUI フレームワーク (`Application` / `update` / `view` / `subscription`)
- **iced::widget::canvas** — 低レベル 2D 描画 + `Cache` (frame をキャッシュ、dirty 時のみ再描画)
- **iced::Subscription** — async stream (WebSocket / Time tick) を `Message` に変換するランタイム機構
- **iced::Task** — async command (REST fetch 等) を `Message` に変換
- **palette クレート** — 色空間変換 (HSL/OKLab)、theme editor 用
- **enum-map** — exchange / pane kind 毎の静的 map
- **`exchange` クレート** (`exchange/src/adapter/`) — Binance/Bybit/Hyperliquid/OKX/MEXC を統一 trait に
- **`data` クレート** — config / layout / chart state / aggregation / panel スキーマを iced 非依存に分離 (**ここは我々が直接学べる**)

**翻訳ルールの原則:**
- iced `Application::update(self, msg) -> Task<Msg>` → Bevy **system + EventReader/EventWriter**
- iced `view() -> Element` → Bevy **spawn ツリー** (`Node` 階層) または `bevy_egui::Window` 内クロージャ
- iced `Subscription` → Bevy **async task + Event 配信** (`IoTaskPool` + `crossbeam_channel` + ポーリング system)
- iced `canvas::Program::draw(theme, bounds)` → `Update` system で **`ShapePainter`** に即時描画
- iced `canvas::Cache` → **キャッシュ不要**。重い**計算**だけ `Changed<DataDirty>` で gate (描画自体は毎フレーム)
- iced `Message` 列挙 → Bevy **複数 Event 型** (`app.add_event::<X>()` 毎)
- `exchange` adapter trait → 我々は **gRPC stub + nautilus_trader 側 venue adapter** に置き換え (UI 側は知らない)
- palette / theme JSON → `components.rs` の `pub const` または `Resource<Theme>`

これらの翻訳を**勝手にやらない**。flowsurface の責務分割 (どの関数が何を計算しているか / どこで panic guard を入れているか / pure data と view の境界) は真似て、API は bevy_egui + ShapePainter に置き換える。

## UI ドメイン → flowsurface ファイル対応表

新しい UI を作るときは、まず該当ドメインのファイルを **1〜3 個 Read** して責務分割と edge case を 5 分眺める。`F:` プレフィックスは `.claude/skills/flowsurface/src/` を指す。

### チャート本体 (`src/ui/chart.rs` 拡張)

| 機能 | flowsurface 参考 | 我々の翻訳先 |
|------|------------------|--------------|
| ローソク足 (kline) | `F:src/chart/kline.rs`, `F:data/src/chart/kline.rs` | `ShapePainter` で `draw_candle` (既存) を拡張、`ChartViewState` Component |
| DOM heatmap | `F:src/chart/heatmap.rs`, `F:src/widget/chart/heatmap/`, `F:data/src/chart/heatmap.rs` | 価格ビン × 時間軸の 2D グリッド、`ShapePainter::rect` or `Sprite` 行列、`Changed<DepthSnapshot>` で再計算 |
| footprint (cluster) | `F:src/widget/chart/heatmap.rs` の cluster 部 | bid/ask volume を ローソク幅で帯描画、price-bin マージン共有 |
| 比較ライン (multi-symbol) | `F:src/chart/comparison.rs`, `F:src/widget/chart/comparison.rs`, `F:data/src/chart/comparison.rs` | 複数 series を `% normalize` (close 基準) で `ShapePainter::line` |
| 指標オーバーレイ | `F:src/chart/indicator/`, `F:src/chart/indicator/plot.rs`, `F:src/chart/indicator/kline/`, `F:data/src/chart/indicator.rs` | `IndicatorKind` enum + 計算結果 Component、Update system で描画 |
| 軸 / スケール (linear/log) | `F:src/chart/scale/linear.rs` | `PriceScale` Resource、`auto_scale: bool` を持たせる (既存) |
| 軸 / スケール (時間) | `F:src/chart/scale/timeseries.rs` | `TimeScale` Resource、`time_window_ms` (既存) を流用 |
| ズーム/パン | `F:src/chart/kline.rs` の `on_event` 系 | `MouseWheel` + `Pointer<Drag>` observer、`time_window_ms` と `min/max_price` 更新 |
| データアグリゲーション (時間) | `F:data/src/aggr/time.rs` | `BarSpec` で bucket、純粋関数化して unit test、`is_closed: bool` 付きで未確定足を区別 |
| データアグリゲーション (tick) | `F:data/src/aggr/ticks.rs` | tick 数で bucket、`bevy_tasks` で重い処理逃がす |
| 価格グルーピング (heatmap/ladder) | `F:data/src/chart/heatmap.rs`, `F:data/src/util.rs` | `price_step: Decimal` で `(price / step).floor() * step` |

### DOM / Time & Sales / Ladder (新規 `src/ui/{ladder,time_and_sales}.rs` 想定)

| 機能 | flowsurface 参考 | 我々の翻訳先 |
|------|------------------|--------------|
| Ladder UI (L2 板) | `F:data/src/panel/ladder.rs` | `bevy_egui` table or `Text2d` 行 spawn、bid/ask 色は `components.rs` |
| Time & Sales (trade list) | `F:data/src/panel/timeandsales.rs` | 既存 `sidebar.rs` の virtual scroll パターン流用、最新を上に追加 |
| L2 depth merge / snapshot+diff | `F:exchange/src/depth.rs` | バックエンドは nautilus 側で完結、UI は最終 `OrderBookL2` Event を受け取るだけ |
| 板の差分 highlight (新規/減少) | `F:src/widget/chart/heatmap/` の更新部 | 直近 N ms 内の変化に `Color` を一時的にミックス、`Timer` Component で減衰 despawn |

### Tickers / Search / Picker (`src/ui/sidebar.rs`, `instrument_picker.rs`)

| 機能 | flowsurface 参考 | 我々の翻訳先 |
|------|------------------|--------------|
| ティッカーテーブル全体 | `F:src/screen/dashboard/tickers_table.rs` (1817 行、巨大だが構造は table + filter + sort + fav) | 既存 `sidebar.rs` Tickers list を拡張 |
| サイドバー (左ペイン) | `F:src/screen/dashboard/sidebar.rs` | `src/ui/sidebar.rs` 既存 |
| ミニ ticker リスト (pane 内) | `F:src/modal/pane/mini_tickers_list.rs` | `floating_window` 内 picker、`bevy_egui::Window` modal |
| 検索 / フィルタロジック | `F:data/src/tickers_table.rs` | `SidebarTickersSearchState` 既存、subsequence マッチで十分 |
| Stream 設定 (pane→ticker 束縛) | `F:src/modal/pane/stream.rs` | `SelectedSymbol` Resource + 各 panel の `LinkedSymbol` Component |

### マルチペイン / レイアウト / 永続化 (`src/ui/floating_window.rs`, `layout_persistence.rs`)

| 機能 | flowsurface 参考 | 我々の翻訳先 |
|------|------------------|--------------|
| Dashboard 全体構造 | `F:src/screen/dashboard.rs`, `F:src/screen/dashboard/pane.rs` | `src/ui/window.rs` + `floating_window.rs` |
| Split / Multi-split | `F:src/widget/multi_split.rs`, `F:src/widget/column_drag.rs` | drag-resize は既存 floating_window の `Pointer<Drag>` observer 拡張 |
| Pane (1 panel = 1 pane) | `F:src/screen/dashboard/pane.rs` | `floating_window.rs` の `FloatingWindowKind` enum |
| Pane 設定モーダル | `F:src/modal/pane/settings.rs`, `F:src/modal/pane/indicators.rs` | `bevy_egui::Window` modal + `FocusedWidget` 一時占有 |
| レイアウト永続化スキーマ | `F:data/src/layout/dashboard.rs`, `F:data/src/layout/pane.rs` | `layout_persistence.rs` 既存、version 上げて migrate |
| `layout.rs` トップ | `F:src/layout.rs` (404 行) | 全体構造の参考 |
| レイアウトマネージャ (複数保存/ロード) | `F:src/modal/layout_manager.rs` | 既存 menu_bar の Save/Load + sidecar (`e2e-testing` 参照) |
| マルチウィンドウ (OS window) | `F:src/window.rs` | 当面非対応、必要時に `bevy::window` で別 `Window` Entity |

### テーマ / 色 / 設定 / 通知

| 機能 | flowsurface 参考 | 我々の翻訳先 |
|------|------------------|--------------|
| Theme editor | `F:src/modal/theme_editor.rs` | `bevy_egui` modal、`components.rs` の色を `Resource<Theme>` に持ち替えてから |
| Color picker | `F:src/widget/color_picker.rs` | `bevy_egui` の `color_edit_button_*` を流用、palette クレートは入れずに HSV/HSL を自前 |
| Audio (trade cue) | `F:src/audio.rs`, `F:data/src/audio.rs`, `F:src/modal/audio.rs` | `bevy_audio` で実装、volume/threshold は modal、`last_played: Instant` で debounce |
| Network manager (再接続) | `F:src/modal/network_manager.rs`, `F:exchange/src/adapter/connect.rs`, `F:exchange/src/adapter/limiter.rs` | gRPC stream の reconnect は backend 側、UI は status 表示のみ (`footer.rs`) |
| Toast (通知) | `F:src/widget/toast.rs` | 新規 `src/ui/toast.rs` を作成、`bevy_egui::Area` 右下 + `Timer` Component で寿命管理 |
| ロガー | `F:src/logger.rs`, `F:data/src/log.rs` | `log` crate + `tracing-subscriber`、UI 表示は当面不要 |
| Config 永続化 | `F:data/src/config.rs` | `layout_persistence.rs` と同居 |

### Exchange / Stream (UI ではないが pattern として参考)

| 機能 | flowsurface 参考 | 我々の翻訳先 |
|------|------------------|--------------|
| Exchange adapter trait | `F:exchange/src/adapter/`, `F:exchange/src/adapter/client.rs` | 我々は **gRPC 経由で nautilus_trader 側 venue adapter** に丸投げ、UI 側は知らない |
| Rate limiter | `F:exchange/src/adapter/limiter.rs` | backend 側 (tachibana / kabusapi スキル参照) |
| Stream multiplex (hub) | `F:exchange/src/adapter/hub.rs` | backend 側、UI は `EventReader<MarketDataEvent>` |
| Connector layer | `F:src/connector/stream.rs`, `F:src/connector/fetcher.rs` | gRPC `StreamMarketData` を `IoTaskPool` で受信、Event 配信 |

## flowsurface を読むときの読み方 (時間を浪費しない)

1. **対応表のファイルだけ開く**。flowsurface は workspace 3 crate + ~40 ファイルだが、1 タスクで読むのは 1〜3 ファイルに絞る。
2. **`pub fn` シグネチャ と `enum Message` を眺める** → 責務分割と input/output を掴む。iced の `view()` ボディの widget DSL は読み飛ばし可。
3. **`update(self, msg) -> Task` を読む** — ここに state machine が出る。我々の Bevy system に翻訳する核。
4. **`canvas::Program::draw` メソッドを読む** — チャート描画ロジックの肝。座標変換 (`price → y`, `time → x`) は **そのまま我々の `ShapePainter` で使える**。
5. **`subscription()` を読む** — WebSocket / Time tick の subscribe パターン。我々では `IoTaskPool` + `crossbeam_channel` + ポーリング system で Event に変換。
6. **iced 固有 API (`Element`, `Task`, `Subscription`, `Theme`, `iced::widget::*`, `pane_grid`) は読み飛ばす** — 翻訳パターン表で置き換える。
7. **`data/src/` を先に読む** — 純粋データ構造 (chart state / aggr / layout schema / panel) は iced 非依存で我々のロジックにそのまま流用しやすい。テストもここに集中している。
8. **圧倒されたら**: `src/main.rs` → `src/screen/dashboard.rs` → 該当 widget の順で `pub` exports を辿る。

## 翻訳パターン早見表

| flowsurface (iced) | 我々 (Bevy + bevy_egui + ShapePainter) |
|--------------------|----------------------------------------|
| `Application::update(self, msg) -> Task<Message>` | `fn system(mut commands, mut events: EventReader<Msg>, ...)` |
| `Application::view(&self) -> Element` | spawn ツリー (`Node` 階層) または `egui::Window::show` 内クロージャ |
| `Application::subscription(&self) -> Subscription<Message>` | `IoTaskPool::get().spawn(async {...})` + `crossbeam_channel`、ポーリング system で `EventWriter::send` |
| `Task::perform(future, \|r\| Message::Done(r))` | `bevy_tasks::AsyncComputeTaskPool` で spawn → `Task<T>` を Component 保持 → `block_on(future::poll_once(&mut task))` で取り出し → Event 送信 |
| `Subscription::run(stream)` | `IoTaskPool` で WS connect、受信 loop → `Sender<MarketTick>` に push、UI 側ポーリング system が `EventWriter<MarketTick>` に流す |
| `canvas::Cache::draw(bounds, \|frame\| ...)` | **キャッシュ不要**。ShapePainter は即時描画。重い**計算**だけ `Changed<DataDirty>` で gate |
| `canvas::Program::draw(&self, theme, bounds)` | `Update` system で `ShapePainter::set_translation / .rect / .line / .circle` |
| iced `Theme::palette()` | `components.rs` 定数 or `Resource<Theme>` |
| iced `pick_list` / `combo_box` | `bevy_egui::ComboBox::from_label(...).show_ui(...)` |
| iced `text_input` | bevy_cosmic_edit `TextEdit2d` + `FocusedWidget` |
| iced `scrollable` | 既存 `SidebarTickersScrollOffset` 仮想スクロールパターン |
| iced `pane_grid::State` | `floating_window.rs` + `layout_persistence.rs` |
| iced `Element::map(F)` (子 widget の Msg → 親 Msg) | Event 型を分けるか、`EventReader<ChildMsg>` を親 system が受ける |
| `iced::Subscription::batch([...])` | 複数 system を 1 set にまとめる (`SystemSet`) |
| iced `executor::Default` | Bevy `Update` schedule、async は `bevy_tasks` プール |
| iced `widget::container::Style` | `Sprite` + `Node` の `BackgroundColor` |
| enum-map `EnumMap<Exchange, T>` | `HashMap<VenueId, T>` Resource または `[T; N]` |
| flowsurface `data/src/chart/*.rs` (純粋データ) | **そのまま参考にできる** — Bevy 依存なし、関数とテストを真似る |

## 必ず守る Caveat (Bevy + ShapePainter + gRPC backend 側の都合)

flowsurface の iced パターンを写しただけでは出てこない、Bevy / ShapePainter / 我々のバックエンド構成側の罠。flowsurface コードを読んだ後に必ず照合すること。

### 描画 / ShapePainter 系
1. **ShapePainter は即時描画、iced canvas はキャッシュあり**
   flowsurface は `Cache::draw` で「同じ frame → 再描画スキップ」する。我々は毎フレーム描く前提なので、**重い再計算** (price scale / aggregation / heatmap bin) は `Resource` にメモ化し、`Changed<DataDirty>` でだけ再計算。描画自体は毎フレームで OK。

2. **Z-order と pickability**
   ローソク / 出来高プロファイル / 軸ラベル / カーソル十字線が重なる場合、`Transform.translation.z` で back-to-front を明示。Pointer 観測子を使うときは `Pickable::IGNORE` を背景レイヤに付ける。modal は最前面 z + 背後 `Pickable::IGNORE`。

3. **ShapePainter の `translate` は累積する**
   `painter.set_translation(origin)` で**絶対**に戻すこと。複数 chart entity がある場合、前 entity の translate が残ると別 chart にゴーストが出る。

4. **アンチエイリアスと線幅**
   1px ラインは `painter.thickness(1.0)` で WGPU 上だとぼけることがある。重要な軸線は `painter.thickness(1.5)` or `2.0` のうえで色を抑える。

### データ / アグリゲーション系
5. **時間 bucket 境界の off-by-one**
   flowsurface は `(ts / interval) * interval` の floor 派。最新足の "未確定" 扱いを忘れると最後のローソクが現在値で確定してしまう。`is_closed: bool` を bar に持たせる。

6. **L2 orderbook の snapshot+diff**
   バックエンドが snapshot を投げ直すケース (gap 検出後の再同期) で、UI 側の Decimal price→Vec index マップが古いと panic。**snapshot Event を別 type で受け取り、ladder Component を一旦 clear** してから rebuild。

7. **価格 binning の精度**
   `f64` で `(price / step).floor()` すると 0.1 刻みの誤差で 1 bin ズレる。`Decimal` (rust_decimal) か `i64` (price * scale) で扱う。flowsurface も `data/src/util.rs` でこの罠に対処している。

### 接続 / Stream 系
8. **WebSocket reconnect と Bevy task のライフサイクル**
   flowsurface の `Subscription::run` は iced ランタイムが drop で自動 abort。Bevy では `bevy_tasks::Task` を `Component` に持って自分で drop しないと残る。Window 閉じや mode 切替で確実に `Commands::entity(_).remove::<Task<_>>`。

9. **gRPC stream は backend 都合で切れる**
   我々の構成では UI と取引所の間に gRPC + nautilus_trader が挟まる。flowsurface のように UI が直接 reconnect 制御するのではなく、`TransportCommand` 経由で backend に再接続要求 → backend が ready Event を返す、という間接ルートで実装。flowsurface `network_manager.rs` の UI 表示だけ参考。

### モーダル / Theme / Persistence 系
10. **bevy_egui modal の z-order と focus**
    `egui::Order::Foreground` + `Area` で前面化。modal open 中は `FocusedWidget` を modal の text_input に向け、close 時に元 entity に戻す (戻し忘れると入力が完全に死ぬ)。flowsurface の overlay と同じ責務。

11. **Theme editor で `Resource<Theme>` を hot-swap するときの再描画**
    flowsurface は iced の retained tree が自動で再描画。我々は `Changed<Theme>` で gate するか、`ShapePainter` 系は毎フレームなので問題ない。**`Text2d` の `TextColor` は変更検出に頼らず明示的に書き換える system が必要**。

12. **layout_persistence の version**
    pane の種類 (heatmap / footprint / ladder) を増やすたびに enum variants が増える。version を上げて旧 JSON を migrate or 捨てる。互換性を雑にやると起動時 panic。flowsurface `data/src/layout/` の serde schema を参考。

13. **Audio cue は閾値 + debounce**
    `bevy_audio` で再生するが、毎 trade 鳴らすと負荷大。flowsurface `data/src/audio.rs` の閾値ロジック (size > threshold で鳴らす) を真似て、Component の `last_played: Instant` で debounce。

## 推奨ワークフロー (UI 機能 1 単位 ≒ 1 turn)

1. **ドメインを上の対応表から特定** → 該当 flowsurface ファイルを 1〜3 個 Read (`pub fn` と `enum Message` と `canvas::Program::draw` と `data/src/` 純粋関数)。5 分で十分。
2. **我々の既存隣接コードを Read** (`src/ui/chart.rs`, `sidebar.rs` 等) — 再利用ポイント・既存 `ShapePainter` 慣例・色トークンを特定。
3. **設計を 3-5 行で要約してからコードを書く** — Component / Resource / system / Event の追加分、既存のどれを再利用するか、`data/` 相当の pure data layer を切り出すか。
4. **Caveat 一覧と照合** — 特に 1, 3, 5, 6, 7, 8, 10 はチャート + stream + modal 系で毎回踏みうる。
5. **実装 → `cargo check` → `cargo test --lib` → 目視 E2E** (`e2e-testing` スキル併用)。
6. **長丁場 (複数ファイル / 複数 phase) になりそうなら `pair-relay` スキルへ移行**、本スキルの該当節を Navigator に引き継ぐ。
7. **完了したら `post-impl-skill-update` スキル発動** — 本スキルの description / 対応表 / Caveat を実情にアップデート。

## このスキルの対象になっている src/ui/* (現状スナップショット)

- **chart は Phase 7.3 で `chart.rs` を 3 ファイルに分割済 (旧 `src/ui/chart.rs` は削除)**:
  - `chart_viewstate.rs` — `ChartViewState` Component (translation/scaling/cell_width/cell_height/base_price_y、座標ヘルパ `price_to_y`/`y_to_price`/`interval_to_x`/`x_to_time_ms`、`visible_*_range`)、autoscale 3 system (`chart_data_tick`/`interaction_tick`/`autoscale_apply`、`RequestAutoscale` event 駆動で self-Changed ループ回避)、`ChartSet` enum、レイアウト定数。flowsurface `chart.rs::ViewState` / `scale/linear.rs` autoscale 経路の翻訳
  - `chart_render.rs` — 毎フレーム純 draw (`chart_main_render_system`、ShapePainter で背景+candle+close ライン、`Changed` で gate しない)。flowsurface `kline.rs::draw`
  - `chart_axes.rs` — **価格軸/時間軸ラベル (Phase B)**。`calc_optimal_price_ticks` / `calc_optimal_time_step` (flowsurface `scale/linear.rs` / `scale/timeseries.rs` の翻訳済純関数、再利用可)、`price/time_axis_labels_system` (`Changed<ChartViewState>` 駆動で gutter 子 Text2d を despawn+respawn)、`PriceGutter`/`TimeGutter`/`*GutterRef`/`*Label` Component。⚠️ **価格ラベルは `main_area_price_range()` で main area (上 80%) のみに引く** (Phase E: フル `visible_price_range()` だと volume sub-pane の y 行に無関係な価格目盛りが出る。crosshair の `hovered_price` が `main_area_y_bottom()` でガードしているのと対称)
  - `chart_interaction.rs` — **pan + zoom (Phase C)**。`install_chart_drag_observer` (chart Sprite に `Pointer<Drag>` observer を貼り `translation` を camera scale 補正付きで動かす、`propagate(false)` で title bar drag と分離)、`chart_scroll_zoom_system` (`EventReader<MouseWheel>` + `HoverMap` で hover chart を引く — Bevy 0.15 に `Pointer<Scroll>` 無し、`apply_cursor_zoom` で flowsurface `Message::Scaled` の cursor 中心ズーム補正を写経)。pan/zoom 開始で `auto_scale=false`。⚠️ ホイールはカメラ(PanCam)ズームとも二重発火するので `src/camera.rs::pancam_suppression_over_editor_system` を chart にも拡張済 (Ctrl=キャンバス全体ズーム / 無修飾=chart ズーム)。⚠️ **drag observer は左ボタン限定にする** (`drag.event().button != PointerButton::Primary` で early-return)。`Pointer<Drag>` は全ボタンで発火するため、右/中ドラッグ (PanCam grab) で chart も同時パン+`auto_scale=false` が黙って起きる二重挙動になる (bevy-engine 規約 5 参照)。⚠️ **zoom の `cell_height` クランプは絶対値ではなく px/price-unit (`= cell_height/tick_size`) で持つ**。`price_to_y` が `(price-base)/tick_size*cell_height` なので autoscale 出力 `cell_height = main_area_height*tick_size/range` は tick_size(現状 0.01 固定) 比例で桁が変わり、絶対 `[0.1,1000]` で挟むと高価格銘柄 (¥500 超で `cell_height<0.1`) の最初のホイールが clamp 床に貼り付き縦倍率が不連続ジャンプする (`MIN/MAX_PX_PER_PRICE_UNIT * tick_size` でクランプ)。⚠️ **double-click で view reset (Phase E)**: `install_chart_autoscale_reset_observer` (`Pointer<Click>` で double-click 検出 → `ChartViewState::reset_view()` で translation/scaling/cell_width 既定化 + `auto_scale=true` 再有効化)。pan/zoom で一度 `auto_scale=false` にすると戻す手段が無い問題の解消。Bevy 0.15 は drag 後も `Click` を発火するので `ChartClickState.dragged` フラグで drag 由来 click を double-click 列から除外 (bevy-engine 規約 5 の Click-after-drag 罠参照)。`chart_click_state_cleanup_system` が `RemovedComponents<ChartViewState>` で despawn 時の entity key を掃除
  - `chart_crosshair.rs` — **crosshair 十字線 + price/time readout badge (Phase D)**。`CrosshairState` Component (`cursor_world`/`hovered_price`/`hovered_time_ms`)、`install_chart_crosshair_observer` (chart Sprite に `Pointer<Move>`/`<Out>` observer、Move は `cursor_world` だけ書き派生量は触らない＝flowsurface `clear_crosshair`/Cache 分離の翻訳)、`chart_crosshair_derive_system` (`Or<(Changed<CrosshairState>, Changed<ChartViewState>)>` 駆動で autoscale 確定後の `y_to_price`/`x_to_time_ms` から readout 確定、DerefMut ガードで収束)、`chart_crosshair_render_system` (毎フレーム純 draw、`Changed` gate しない＝immediate-mode 罠、`cursor_world.is_none()` で per-entity continue)、`crosshair_badge_system` (`Changed<CrosshairState>` 駆動で gutter 子に背景 Sprite+Text2d を despawn_recursive+respawn、gutter 生存ガードで set_parent panic 回避)。⚠️ **observer は `ChartViewState` を読まない** (Caveat #28: ChartSet::Autoscale 順序非依存)。⚠️ **hovered_price は main_area 内 (`cursor.y >= main_area_y_bottom()`) のみ計算** (volume area で偽価格を出さない)。⚠️ z オーダー: axis label +0.3 < cross line +0.5 < badge +0.6 (Caveat #16)
  - `chart_volume.rs` — **volume サブペイン (Phase E)**。`volume_render_system` (毎フレーム純 draw、`Changed` gate しない＝immediate-mode 罠、Phase A 予約の volume area `-bounds.y/2`〜`volume_area_height()` に bar 描画、幅は candle と同じ `body_half_width()*2`、`None` volume は skip)、純関数 `max_visible_volume`/`volume_bar_height`/`format_volume`(K/M/B 略記)。candle 色は `chart_render.rs::BULLISH/BEARISH_CANDLE_COLOR` を `with_alpha(0.6)` で再利用 (single source)。crosshair の volume readout は `chart_crosshair.rs` に additive 拡張済 (`CrosshairState.hovered_volume`、volume area のみ最近傍 candle の volume を `nearest_candle_volume` 二分探索で引き、price badge と排他で price gutter の cursor.y 行に badge)
  - `chart_ladder_pane.rs` — **Live モード複合ウィンドウ: Ladder ペイン (Phase F)**。flowsurface `data/src/panel/ladder.rs` の翻訳。`LadderPane{chart_root, last_depth_signature}` / `LadderRow{kind}` (index は spawn 時に y 計算へ渡すだけで保存しない — dead field 除去済) / `LadderRowKind{Ask,Last,Bid}` Component。`chart_ladder_mode_sync_system` (`exec_mode.is_changed() || Added<WindowRoot>` で gate → `is_live_mode` なら Ladder spawn + 枠を `LIVE_COMBINED_PANEL_SIZE` にリサイズ + chart/price child を `CHART_CHILD_LOCAL_X_LIVE` へ左シフト、Replay なら despawn_recursive + 枠 `CHART_PANEL_SIZE` + chart x を `CHART_CHILD_LOCAL_X_REPLAY` へ復帰)、`ladder_render_system` (per-instrument depth → 21 行 = ask10+LAST+bid10 を despawn+respawn、`map.is_changed() || last_prices.is_changed() || Added` の粗 gate + per-pane `depth_signature` early-out で他銘柄 OHLC tick での無駄 rebuild を回避、depth None は `"No depth data"` placeholder)。⚠️ **gate に `last_prices.is_changed()` が必須** — LAST 価格は `LastPrices` 経由 (depth の `InstrumentTradingDataMap` とは別チャンネル更新、main.rs の `BackendStatusUpdate`)。`map.is_changed()` 単独 gate だと LAST だけ動いたフレームを取りこぼし LAST 行が stale になる (`depth_signature` は last を畳むので gate さえ通れば rebuild は正しい)。⚠️ **placeholder/全 Text2d は ASCII 限定** — 既定フォント (FiraMono-subset) は CJK グリフを持たず日本語は豆腐になる (codebase 全体で UI 文字列は ASCII、buying_power の `"—"` 等が前例。`bevy-default-font-no-geometric-shapes` memory 参照)。bid/ask 色は `chart_render.rs::BULLISH/BEARISH_CANDLE_COLOR` を `with_alpha` で再利用 (single source)。⚠️ **chart は WindowRoot の孫** (root→content_area→chart) なので mode_sync は root の*孫*まで辿って chart/price を見つける (plan 擬似コードの「root 直下を走査」は drift)。⚠️ **`CHART_CHILD_LOCAL_X_LIVE` は Replay の -25 ベースから再導出して -85** (枠が左へ `LADDER_WIDTH/2` 広がる分を追従。`-LADDER_WIDTH/2`=-60 のままだと price gutter が ladder と重なる)。⚠️ **Ladder は content_area の子** (root 直下ではない) にして title bar 下に正しく収める。**drag bubble 対策の observer/`propagate(false)` は不要** — window 移動の `Pointer<Drag>` observer は `floating_window.rs` の **title_bar** (content_area の兄弟) に乗っており root には無い。ladder の drag は ladder→content_area→root と bubble するが title_bar は経路に入らないので window は動かない (plan の Caveat #35 / Verification #8 は「root に drag observer がある」前提の stale。chart Sprite の `propagate(false)` も実は window 移動防止には効いておらず防御的。新しい content_area 子パネルは drag observer を足さなくてよい)。⚠️ **`Ref<LadderPane>::is_added()`** で新規 pane を判定 (`Has<Added<T>>` は compile error)。⚠️ Live の chart 足も Ladder 板も per-instrument (`ChartInstrument.instrument_id` で `InstrumentTradingDataMap` lookup、single-global 退行を作らない)。⚠️ **枠リサイズは root `Sprite.custom_size` のみ** — title bar / inner glow / rim sprite は spawn 時サイズのまま (Caveat #34、枠だけ広がり title bar は 360 幅のまま = 既知の見た目限界、polish 余地)
- `sidebar.rs` — virtual scroll + tickers リスト + 検索。flowsurface `tickers_table.rs` の filter/sort/fav パターンが参考になる
- `orders.rs` / `positions.rs` / `buying_power.rs` — trading panel。flowsurface には**ない** (flowsurface は注文機能を持たない) ので、表 / レイアウト部分のみ参考、注文関連ロジックは `nautilus_trader` / `tachibana` / `kabusapi` スキルへ
- `floating_window.rs` / `layout_persistence.rs` — pane / persistence。flowsurface `pane.rs` / `data/src/layout/` / `src/layout.rs` を参考
- `menu_bar.rs` / `footer.rs` — title bar / status bar
- `instrument_picker.rs` — picker。flowsurface `modal/pane/mini_tickers_list.rs` 参考
- `run_result_panel.rs` — flowsurface 直接対応なし。表ベースなので一般的な egui table パターン
- `replay_startup_window.rs` / `scenario_startup_panel.rs` — 起動 modal、flowsurface には**直接対応なし** (zed `recent_projects` 寄り → `zed` スキル)

新機能を作るときはまずこれら**隣接ファイルを Read してから flowsurface を読む**。重複実装を避けるため。

## 他スキルとの境界 (いつ切り替えるか)

- テキストエディタ系 (シンタックスハイライト / picker / command palette / strategy_editor / モーダル UI の type-safe action) → **`zed`** スキル
- Bevy ECS / Camera2d / Sprite / bevy_egui / `add_systems` 20 上限・observer・required components の一般論 → **`bevy-engine`** スキル併用必須
- バックエンド (Python/gRPC、nautilus_trader、戦略実行) → **`nautilus_trader`** スキル
- 立花証券 / kabuステーション API → **`tachibana`** / **`kabusapi`** スキル
- Rust テスト一般 → **`rust-testing`** / **`tdd-workflow`** スキル
- Python テスト一般 (pytest / pytest-httpx / freezegun) → **`tdd-workflow`** スキル
- E2E 手動検証 (`backcast.exe` + `python -m engine`、レイアウトの目視) → **`e2e-testing`** スキル
- 大規模並列実装 (5 タスク以上、依存解決可能) → **`parallel-agent-dev`** スキル
- 長丁場の段階実装 (TDD・複数ファイル・複数レイヤー) → **`pair-relay`** スキル (Navigator spawn 前に本スキル invoke 必須)
- 学習目的・ユーザがドライバー → **`pair-nav`** スキル
- 実装完了/コミット/フェーズ終了 → **`post-impl-skill-update`** スキル (CLAUDE.md 必須ルール)
- 変更コードのレビュー (再利用・品質・効率) → **`simplify`** スキル
