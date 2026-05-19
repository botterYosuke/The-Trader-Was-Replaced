# Chart Window を flowsurface-grade に格上げする (flowsurface 参照)

> 想定 final path: `docs/plan/Phase 7.3 - flowsurface-Grade Chart Window.md`
> Phase 7.2 (Monaco-Grade Strategy Editor) と兄弟プラン。同じテンプレートを踏襲する。

## Context

`src/ui/chart.rs` (422 行) は現状、`bevy_vector_shapes::ShapePainter` の immediate-mode 描画で **背景矩形 + ライン + 最大 50 本のローソク足** を毎フレーム塗っているだけの簡易チャート。**軸ラベル無し / pan・zoom 無し / crosshair 無し / volume 無し / 1 銘柄分の `TradingData` (single Resource) を全パネルが共有**という状態で、複数銘柄を `InstrumentRegistry` に積んでも全 chart パネルが同じデータを描画してしまう。

ユーザは「flowsurface を参考に flowsurface-grade に格上げしたい」。スコープは**ユーザ確認済**で:

- **Volume サブペイン**: backend (engine.proto / nautilus 集約 / `src/trading.rs::OhlcPoint`) まで含めて完全実装
- **Multi-symbol データ**: `TradingData` (single) を **`TradingSession`** (session-global: replay state / timestamp) **+ `InstrumentTradingDataMap`** (per-instrument: ohlc/history/last) の 2-resource に分割。各 chart パネルは自分の `ChartInstrument.instrument_id` で lookup。
- **Phase 構成**: **A0 (backend + data refactor preflight) + A〜E (UI phases)** の 6 段階

flowsurface (`.claude/skills/flowsurface/src/src/chart*`) から学ぶ中心は「**ViewState + Caches を分け、`translation`/`scaling`/`cell_width`/`cell_height` で座標変換を一元化し、crosshair-only の状態変化は main geometry を再生成しない**」設計パターン。iced の `canvas::Cache` (retained-mode) は Bevy `ShapePainter` (immediate-mode) に直訳できないため、本プランでは **「Cache を per-layer Bevy system に翻訳し、それぞれが自分の入力 (`Changed<ChartViewState>` / `Changed<CrosshairState>` / `Changed<AxisLabels>`) で early-out する」**形に置き換える (= 同じ責務分割をスケジューラ層で実現)。

## アーキテクチャ概要

### 既存 ECS 構造 (重要)

現行 `spawn_chart_panel` ([src/ui/window.rs:15–80](src/ui/window.rs)) は **2 entity 構成**:

- **root entity** (`WindowRoot` マーカー、`spawn_floating_window` で作る)
  - `PanelKind::Chart` / `ChartInstrument { instrument_id }` / `LayoutExcluded`
  - `Sprite` (window 枠、Pointer<Down>/<Drag> observer 登録済 — `floating_window.rs:54-150`)
- **child chart entity**
  - `(Transform, ChartViewState)` のみ — **`Sprite` も `Mesh` も付いていない**
  - → 本プラン Phase C で transparent `Sprite { custom_size, color: Color::NONE }` を**付与必須**。これが無いと `Pointer<Drag>` / `Pointer<Move>` 系イベントが Bevy 0.15 picking backend (mesh/sprite ピッキング) に拾われない (chart 背景は ShapePainter で描いているが ShapePainter の出力は picking 対象外)

⚠️ chart entity の Sprite は **`WindowRoot` の子**にする (root に直接付けない)。root には既に `Pointer<Down>` の z-bump observer ([floating_window.rs:54-63](src/ui/floating_window.rs)) が乗っているので、chart Sprite を root に重ねると pan のドラッグ毎に window z 順が跳ね上がる。子として nest すれば propagation が止まる (`Pointer<Drag>::propagate(false)` または observer 内で `trigger.entity()` 判定で分岐)。

### `TradingData` の 2-Resource 分割 (Critical)

現行 `Res<TradingData>` ([src/trading.rs:73–87](src/trading.rs)) は単一 instrument のフィールドを持つ global Resource。これを以下のように分割する。

**残す: `TradingSession` (Resource, global)** — replay clock / session state など、instrument 非依存のもの:

```rust
#[derive(Resource, Default)]
pub struct TradingSession {
    pub timestamp_ms: i64,
    pub replay_state: Option<String>,
    pub timer: Timer,
}
```

**新規: `InstrumentTradingData` + `InstrumentTradingDataMap`** — per-instrument OHLC/history/last:

```rust
#[derive(Debug, Clone, Default)]
pub struct InstrumentTradingData {
    pub history_points: Vec<HistoryPoint>,
    pub ohlc_points: Vec<OhlcPoint>,
    pub open: Option<f32>,
    pub high: Option<f32>,
    pub low: Option<f32>,
    pub close: Option<f32>,
    pub open_time_ms: Option<i64>,
    pub last_price: Option<f32>,
}

#[derive(Resource, Default)]
pub struct InstrumentTradingDataMap {
    pub map: HashMap<String, InstrumentTradingData>,
}
```

⚠️ **既存 `LastPrices { map: HashMap<String, f64> }` ([trading.rs:511-514](src/trading.rs)) と同じ命名**にする (`pub map: HashMap<...>` フィールド名を揃える)。これは「proto 側が `map<string, ...>` を持ち、Rust 側で Resource 化する」既存パターン (`BackendTradingState.last_prices` at [trading.rs:63](src/trading.rs)) の踏襲。新規パターンを発明しない。

**blast radius (調査済、A0 で消化):**

- **Mutator 3 箇所**: `backend_update_system` ([trading.rs:279](src/trading.rs) ← 唯一の本物 gRPC 入口、ここだけが per-instrument 化する重要箇所), `price_simulation_system` (synthetic, 削除候補), Buy/Sell button (toy debug)
- **Reader — render-only, 移行容易 (5 箇所)**: `chart.rs:85` (本プラン本丸), `systems.rs::update_price_display` (Text2d 1 個、SelectedSymbol keyed lookup へ), `sidebar.rs::update_ticker_price_text_system:632` (既に `marker.instrument_id` で keying 済、フォールバックの `trading.close` 経路だけ差し替え), `footer.rs:256,458,561` (session-global なので `TradingSession` 経由), `menu_bar.rs:726,810` (同 session-global)
- **Reader — session-global (3 箇所)**: `replay_startup_window.rs:201,294,321,348,376` (`replay_state` / `timestamp_ms` のみ → `TradingSession`)

「mutator は実質 1 箇所 (`backend_update_system`)」というのが最大の発見。proto を `repeated PerInstrumentState` または `map<string, PerInstrumentState>` 化すれば、ここ 1 つの loop で `InstrumentTradingDataMap` を埋められる。

### `InstrumentTradingDataMap` の lifecycle

⚠️ **`InstrumentRegistry` 退会時に map entry が残る問題**: `instrument_chart_sync_system` ([window.rs:83–107](src/ui/window.rs)) は registry 変更時に chart `WindowRoot` を despawn するが、**map からの削除は誰もやらない**。Phase A0 で `instrument_chart_sync_system` に「desired から消えた id を `map.remove()` する」3 行を追加。あるいは「entry が増え続けても銘柄ユニバース有限なので無視」も選択肢だが、replay/live 切替で銘柄が完全に入れ替わるシナリオを想定すると掃除した方が安全。

### Cache の Bevy 翻訳 (Critical)

flowsurface の `Caches { main, x_labels, y_labels, crosshair }` ([chart.rs:625–646](.claude/skills/flowsurface/src/src/chart.rs)) は iced の `canvas::Cache` (retained GPU geometry、`.clear()` まで再描画されない) を 4 層持ち、`clear_crosshair` は `crosshair + y_labels + x_labels` をクリアして main は据え置きにする (= crosshair 移動だけで main を再描画しない最適化)。

Bevy `ShapePainter` は immediate-mode で毎フレーム redraw を強制するので、retained-mode の cache 概念は**そのままは持ち込めない**。本プランでは **「Cache を per-layer system に翻訳」** する:

| flowsurface Cache | 本プラン Bevy system | 入力 (early-out 条件) |
|---|---|---|
| `Caches::main` | `chart_main_render_system` (Phase A 再構築) | `&ChartViewState` を毎フレーム読むが、ShapePainter は安価。`Changed<InstrumentTradingDataMap>` を見て autoscale 計算だけスキップ |
| `Caches::y_labels` | `price_axis_labels_system` (Phase B) | `Changed<ChartViewState>` で発火、Text2d 子 entity を despawn+respawn |
| `Caches::x_labels` | `time_axis_labels_system` (Phase B) | 同上、`Changed<ChartViewState>` のみ |
| `Caches::crosshair` | `chart_crosshair_render_system` (Phase D) | `Changed<CrosshairState>` のみ、main entity に触らない |

**重要な不変条件**: chart の「重い計算」(autoscale 範囲、表示候補 OHLC スライス、tick step 計算) は `chart_viewstate_update_system` (新規、Phase A) に集約し、**`Changed<InstrumentTradingDataMap>` または `Changed<ChartViewState>` (mouse interaction) のときだけ走る**。`chart_main_render_system` は ViewState を immutable に読む純 draw に徹し、毎フレーム走っても OHLC ≤1000 点なら無視できる。これで flowsurface の「Cache 階層化による責務分割」を**スケジューラ層で再現**する。

bevy_vector_shapes 以外の retained-mode 候補 (bevy_prototype_lyon, mesh 直作り) は**採用しない**。codebase に retained-mode shape の既存 user は無く ([grep "ShapePainter" 外の shape API は src/ 内に 0 件]), `floating_window.rs` の `Sprite` ベース UI と混ぜると invalidation プロトコルが二重化する。

### 責務分割と flowsurface 参照対応

| 新規モジュール (`src/ui/`) | 責務 | flowsurface 参考 (`.claude/skills/flowsurface/src/src/`) |
|---|---|---|
| `chart_viewstate.rs` (**新規, Phase A**) | `ChartViewState` 構造体定義 (translation/scaling/cell_width/cell_height/basis/latest_x/base_price_y/decimals/bounds)、autoscale 計算、`chart_viewstate_update_system` | `chart.rs::ViewState` (line 648–663) + `chart/kline.rs` の `update_chart` autoscale 経路 |
| `chart_render.rs` (**新規, Phase A**) | main chart の毎フレーム draw (背景 + candles + ライン)。**ViewState を read-only に取り、`chart_viewstate_update_system` の結果をそのまま座標変換に流すだけ** | `chart/kline.rs::draw` (line 1700 付近以降の `canvas::Program::draw` impl) |
| `chart_axes.rs` (**新規, Phase B**) | `calc_optimal_ticks` (Y) / `calc_time_step` (X) を Bevy 用に翻訳、`price_axis_labels_system` / `time_axis_labels_system`、`PriceLabel` / `TimeLabel` Component (child Text2d) | `chart/scale/linear.rs::calc_optimal_ticks` (line 7–24) + `chart/scale/timeseries.rs::calc_time_step` (line 74–113) |
| `chart_interaction.rs` (**新規, Phase C**) | `Pointer<Drag>` で `translation` 更新、`MouseWheel` で `cell_width`/`cell_height` ズーム、autoscale toggle | `chart.rs::canvas_interaction` (line 86–250) + `chart.rs::update` (line 280–492) |
| `chart_crosshair.rs` (**新規, Phase D**) | `CrosshairState` Component、`Pointer<Move>`/`Pointer<Out>` observer、`chart_crosshair_render_system` (cross lines + 価格/時刻 readout badge) | `chart.rs::Caches::clear_crosshair` (line 641–645) + `chart/kline.rs::draw_crosshair` 周辺 |
| `chart_volume.rs` (**新規, Phase E**) | volume サブペイン (chart 領域の下 20%)、`volume_render_system` (bull/bear 色のバー)、crosshair で volume readout | `chart/indicator/kline.rs::KlineIndicatorImpl` の volume plot |

### システム実行順序

flowsurface の `update`/`view`/`draw` フローを Bevy のスケジューラに翻訳:

```
[Update schedule, after backend_update_system]
1. chart_viewstate_update_system
   - Changed<InstrumentTradingDataMap> or Changed<ChartViewState> で発火
   - autoscale 計算、latest_x / base_price_y / decimals 更新
   - Phase A で導入
2a. chart_interaction_drag_system    (Pointer<Drag> observer 経由、translation 更新)  [Phase C]
2b. chart_interaction_wheel_system   (MouseWheel event reader、cell_width/height 更新)  [Phase C]
2c. chart_crosshair_input_system     (Pointer<Move>/<Out> observer、CrosshairState 更新)  [Phase D]

[render-ish — 描画 system 群、いずれも .after(chart_viewstate_update_system)]
3.  chart_main_render_system         (毎フレーム、ShapePainter 描画)  [Phase A 再構築]
4.  price_axis_labels_system         (Changed<ChartViewState> で Text2d despawn+respawn)  [Phase B]
5.  time_axis_labels_system          (Changed<ChartViewState>)  [Phase B]
6.  chart_crosshair_render_system    (Changed<CrosshairState>, ShapePainter で別 z)  [Phase D]
7.  chart_volume_render_system       (Changed<InstrumentTradingDataMap> or Changed<ChartViewState>)  [Phase E]
```

⚠️ **既存 `chart_render_system` ([src/ui/mod.rs:151](src/ui/mod.rs)) は Phase A で削除し、3 と 7 に分割する**。Phase A 着手前に `mod.rs` の `add_systems` タプルが何個入っているか実カウントし、20-tuple 上限を超えるなら `SystemSet` または別 `app.add_systems(Update, (...))` 呼び出しに分割する (Phase 7.2 と同じ罠)。

### Changed フィルタ駆動 (per-entity)

各 chart entity ごとに `ChartViewState` を個別管理するので、`Changed<ChartViewState>` フィルタは entity 単位で正しく分岐する。`InstrumentTradingDataMap` は単一 Resource (`Res<...>`) なので map 全体が変更時に発火するが、各 chart entity は **自分の `instrument_id` を持つ key の `last_modified`** を見たい。Resource 全体の `Res::is_changed()` で粗く発火 → entity ごとに `ChartViewState.last_seen_ohlc_len` のような local cache を比較して個別 early-out するパターンで十分 (per-entry timestamp は overengineering、銘柄数 = 数十 想定で素朴で OK)。

## 実装フェーズ (A0 + 5 段階、各 1 PR 想定)

依存関係: **A0 → A → B,C,D は並行可能、E は B/C/D 完了後**。Volume backend (A0 に含む) と Volume UI (E) は分割する (proto 変更を先に landing させてから UI を組む方が衝突しにくい)。

### Phase A0: backend volume + per-instrument data refactor (preflight)

**目的**: chart UI 側のリファクタを「凍結したデータ契約」に対して進められる状態を作る。Phase A 以降は純粋な UI work。

**変更:**

- `python/proto/engine.proto`:
  - `OhlcPoint` メッセージに `optional float volume = N;` を追加 (proto3 explicit-presence 必須 — bare `float` だと 0.0 が "no data" と区別できず、Phase E の volume サブペインが偽の zero-volume bar を描いてしまう)
  - `BackendTradingState` に `map<string, PerInstrumentState> per_instrument = N;` を追加。`PerInstrumentState` は既存の flat フィールド (`history_points` / `ohlc_points` / `open` / `high` / `low` / `close` / `open_time_ms`) を内包
  - 既存 flat フィールドは互換性のため当面残す (proto3 は field removal が安全でない)
- `python/engine/`:
  - nautilus aggregation で bar の volume を集計 (BarType ごと、tick volume または size sum)
  - `BackendTradingState` を埋める箇所で `per_instrument: map[symbol] -> PerInstrumentState` を構築。flat フィールドは "current selected" 銘柄を後方互換で書き続ける
  - tests: `python/tests/` で `per_instrument` が複数 instrument を運ぶこと、volume が None ではなく具体値で出ること
- `src/trading.rs`:
  - `OhlcPoint.volume: Option<f32>` を追加 (`#[serde(default)]`)
  - `InstrumentTradingData` struct (上記定義)、`InstrumentTradingDataMap` Resource、`TradingSession` Resource を追加
  - 既存 `TradingData` は **legacy alias として 1 PR 据え置き** (削除は別 PR)。`Res<TradingData>` を使っている全 8 reader を順次 `Res<InstrumentTradingDataMap>` または `Res<TradingSession>` に書き換える (詳細は触るファイル一覧)
  - `backend_update_system` を `per_instrument` map を loop して `InstrumentTradingDataMap.map` を埋める形に書き換える
  - `instrument_chart_sync_system` ([window.rs:107 付近](src/ui/window.rs)) で「`desired` から消えた id は `map.remove(id)` する」3 行を追加

**新規アセット: なし**

**Verification:**
- `cargo check` + `cargo test --lib`
- `python -m pytest python/tests/` で proto/aggregation の volume を確認
- `backcast` 起動 → 既存単一 instrument の OHLC 描画が回帰していないこと (Phase A 着手前の baseline 確認)

⚠️ **Phase A0 はあくまで data plumbing のみ**。UI コードは触らない (chart.rs はそのまま、`Res<TradingData>` 経由のレガシ参照だけ動く状態をキープ)。

### Phase A: ChartViewState 再設計 + Sprite hit-target + system 分割

**新規 `src/ui/chart_viewstate.rs`:**

```rust
#[derive(Component)]
pub struct ChartViewState {
    pub bounds: Vec2,                  // 描画矩形のサイズ (旧 width/height)
    pub translation: Vec2,             // pan オフセット (世界座標)
    pub scaling: f32,                  // ズーム倍率 (1.0 = 既定)
    pub cell_width: f32,               // 1 candle の幅 (px、既定 6.0)
    pub cell_height: f32,              // 1 tick の高さ (px、価格 1 単位あたり)
    pub basis: Basis,                  // 集約基準。enum Basis { Time(timeframe_ms: u64) }
    pub latest_x: i64,                 // 最新 candle の open_time_ms (X 軸の右端基準)
    pub base_price_y: f32,             // Y 軸の基準価格 (translation.y=0 が指す価格)
    pub decimals: usize,               // 価格表示桁
    pub tick_size: f32,                // 最小価格単位 (axis label 刻み計算用)
    pub auto_scale: bool,
    pub last_seen_ohlc_len: usize,     // per-instrument early-out 用 local cache
}

#[derive(Debug, Clone, Copy)]
pub enum Basis {
    Time(u64),  // timeframe in ms (例: 60_000 = 1min)
}
```

座標変換ヘルパは `ViewState::y_to_price` / `price_to_y` / `x_to_interval` / `interval_to_x` を flowsurface ([chart.rs ViewState impl](.claude/skills/flowsurface/src/src/chart.rs)) からそのまま翻訳。`#[cfg(test)]` で「round-trip: `price_to_y(y_to_price(y))` ≈ y」のテストを必ず置く (Phase C で pan/zoom が乗ったときに崩れないため)。

**`chart_viewstate_update_system` (Phase A の中核):**

```rust
fn chart_viewstate_update_system(
    map: Res<InstrumentTradingDataMap>,
    mut chart_q: Query<(&ChartInstrument, &mut ChartViewState), With<WindowRoot>>,
    // 注意: ChartInstrument は WindowRoot 側、ChartViewState は子 entity 側にある可能性あり
    //       → 実装時に「root に統合する」か「join する」か Phase A 着手時に決定 (現状は別 entity)
) {
    if !map.is_changed() { return; }
    for (chart_instrument, mut state) in &mut chart_q {
        let Some(data) = map.map.get(&chart_instrument.instrument_id) else { continue };
        if data.ohlc_points.len() == state.last_seen_ohlc_len && !state.is_added() { continue; }
        state.last_seen_ohlc_len = data.ohlc_points.len();

        // autoscale 再計算: visible candle range から min/max を取り、10% padding
        if state.auto_scale {
            // ... (現行 chart.rs:113-162 の autoscale ロジックを移植)
            // state.base_price_y / state.cell_height を更新 (min/max を直接持つのを廃止)
        }
        if let Some(last) = data.ohlc_points.last() {
            state.latest_x = last.open_time_ms;
        }
    }
}
```

⚠️ **既存 `ChartViewState` は `min_price` / `max_price` を直接持っていたが、Phase A 以降は `base_price_y` + `cell_height` で表現する**。pan/zoom 時に min/max を直接動かす旧設計だと「ズーム中心が画面中央に固定される」flowsurface パターンが組めない (`cell_height *= factor; translation.y -= (new_cursor_y - cursor_y)` で cursor 中心ズームを実現するため。`chart.rs:429-466` 参照)。

**chart entity の Sprite hit-target 追加 (Critical):**

```rust
// spawn_chart_panel ([window.rs:48-58]) の chart entity spawn を以下に書き換える
let chart = commands.spawn((
    Transform::from_xyz(0.0, 10.0, 0.1),
    Sprite {
        custom_size: Some(Vec2::new(360.0, 180.0)),
        color: Color::NONE,           // 透明 (描画は ShapePainter が担当)
        ..default()
    },
    ChartViewState {
        bounds: Vec2::new(360.0, 180.0),
        cell_width: 6.0,
        cell_height: 1.0,  // 後で autoscale が上書き
        ..default()
    },
    ChartInstrument { instrument_id: instrument_id.to_string() },
)).id();
```

⚠️ `ChartInstrument` は今 `WindowRoot` 側にも付いているが (window.rs:27)、Phase A で **chart entity 側にもコピー**する (`viewstate_update_system` のクエリが root を経由しないで済むようになる、Phase 7.2 の `StrategyEditorId.region_key` ジョインと同じ思想)。Root 側の `ChartInstrument` は registry 同期用にそのまま残す。

**`chart_main_render_system` (新規、旧 `chart_render_system` の draw 部分のみ):**

ViewState を read-only に取り、`base_price_y + cell_height` で y 座標を計算、`translation` + `scaling` を全 ShapePainter 呼び出しに適用。**autoscale 計算は完全に消す** (それは `chart_viewstate_update_system` の仕事)。

```rust
fn y_of(price: f32, state: &ChartViewState) -> f32 {
    (price - state.base_price_y) * state.cell_height * state.scaling + state.translation.y
}
fn x_of(open_time_ms: i64, state: &ChartViewState) -> f32 {
    let dt = (open_time_ms - state.latest_x) as f32;
    let timeframe_ms = match state.basis { Basis::Time(ms) => ms as f32 };
    (dt / timeframe_ms) * state.cell_width * state.scaling + state.translation.x
}
```

**修正:**
- `src/ui/chart.rs` を削除し `chart_viewstate.rs` / `chart_render.rs` に分割
- `src/ui/window.rs::spawn_chart_panel` で Sprite 付き chart entity spawn
- `src/ui/mod.rs` で新規 system 登録 (旧 `chart_render_system` を削除、`chart_viewstate_update_system` + `chart_main_render_system` に置換)

**Verification (Phase A 単独):**
- 既存と見た目同じ candle が描かれる (regression なし)
- 複数 instrument を registry に積むと、各 chart パネルが自分の OHLC を描く (`InstrumentTradingDataMap` lookup が効いている)
- `cargo test --lib` で `ChartViewState::y_to_price` の round-trip テスト pass

### Phase B: 価格軸 (Y) + 時間軸 (X)

**新規 `src/ui/chart_axes.rs`:**

**Y 軸 (price labels):**

flowsurface `scale/linear.rs::calc_optimal_ticks` ([line 7-24](.claude/skills/flowsurface/src/src/chart/scale/linear.rs)) を翻訳:

```rust
pub fn calc_optimal_price_ticks(highest: f32, lowest: f32, labels_can_fit: i32) -> (f32, f32) {
    let range = (highest - lowest).abs().max(f32::EPSILON);
    let labels = labels_can_fit.max(1) as f32;
    let base = 10.0_f32.powf(range.log10().floor());
    let step = match range / base {
        r if r <= labels * 0.1 => 0.1 * base,
        r if r <= labels * 0.2 => 0.2 * base,
        r if r <= labels * 0.5 => 0.5 * base,
        r if r <= labels       => base,
        r if r <= labels * 2.0 => 2.0 * base,
        _                      => (range / labels).min(5.0 * base),
    };
    let rounded_highest = (highest / step).ceil() * step;
    (step, rounded_highest)
}
```

これは純関数 (`#[cfg(test)]` で 5 ケース固定: 範囲 0.01〜10000、labels_can_fit 3/10/50)。

**`price_axis_labels_system` (Phase B):**

`Changed<ChartViewState>` で発火。chart entity の右側 50px gutter に Text2d 子を despawn+respawn:

```rust
fn price_axis_labels_system(
    mut commands: Commands,
    chart_q: Query<(Entity, &ChartViewState), (With<ChartInstrument>, Changed<ChartViewState>)>,
    existing_labels: Query<(Entity, &PriceLabel)>,
    // ...
) {
    for (chart_entity, state) in &chart_q {
        // 既存ラベルを despawn
        for (label_e, label) in &existing_labels {
            if label.target_chart == chart_entity { commands.entity(label_e).despawn(); }
        }
        // 表示価格域: bounds と translation/scaling/base_price_y から逆算
        let (visible_low, visible_high) = state.visible_price_range();
        let labels_can_fit = (state.bounds.y / (TEXT_SIZE * 3.0)) as i32;
        let (step, rounded_max) = calc_optimal_price_ticks(visible_high, visible_low, labels_can_fit);

        let mut value = rounded_max;
        while value > visible_high { value -= step; }
        while value >= visible_low {
            let y = state.price_to_y(value);  // ViewState::price_to_y ヘルパ
            let label_text = format!("{:.*}", state.decimals, value);
            commands.spawn((
                Text2d::new(label_text),
                TextFont { font_size: TEXT_SIZE, ..default() },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
                Anchor::CenterLeft,
                Transform::from_xyz(state.bounds.x / 2.0 + 4.0, y, 0.3),
                PriceLabel { target_chart: chart_entity },
            ));
            value -= step;
        }
    }
}
```

⚠️ **despawn+respawn は per-`Changed<ChartViewState>` フレームのみ**。pan/zoom が連続する Phase C 完了後はラベル生成が毎フレーム走ることがあるが、最大 ~20 個の Text2d 出し入れなので無視できる (Phase 7.2 の gutter buffer は cosmic_text 文字列再生成、本プランは Text2d 子 entity 出し入れ、コスト同等)。

**X 軸 (time labels):**

flowsurface `scale/timeseries.rs::calc_time_step` ([line 74-113](.claude/skills/flowsurface/src/src/chart/scale/timeseries.rs)) の M1/M3/M5/HOURLY/MS step テーブルを翻訳:

```rust
const M1_TIME_STEPS_MS: [u64; 9] = [
    720*60_000, 180*60_000, 60*60_000, 30*60_000, 15*60_000,
    10*60_000, 5*60_000, 2*60_000, 60_000,
];
// 他の timeframe (M3/M5/HOURLY) 同様

pub fn calc_optimal_time_step(
    earliest_ms: i64, latest_ms: i64, labels_can_fit: i32, timeframe_ms: u64,
) -> (u64, u64) { /* ... */ }
```

`time_axis_labels_system` は同様に `Changed<ChartViewState>` で発火し、chart bottom 24px gutter に Text2d 子を spawn。**`chrono::DateTime` で表示文字列を作る** (`chrono` は既に [trading.rs:2 で](src/trading.rs) 使用済、依存追加不要)。

**レイアウト調整:**

現状 `chart` entity サイズは `360 x 180`、`window.rs:53-55` で hardcode。Phase B で:

```rust
const CHART_PANEL_SIZE: Vec2 = Vec2::new(360.0, 230.0);     // window 全体
const PRICE_GUTTER_WIDTH: f32 = 50.0;                       // Y 軸ラベル領域 (右)
const TIME_GUTTER_HEIGHT: f32 = 24.0;                       // X 軸ラベル領域 (下)
const CHART_DRAW_SIZE: Vec2 = Vec2::new(310.0, 180.0);      // 実描画領域 (= panel - gutters)
```

`ChartViewState.bounds` には `CHART_DRAW_SIZE` を入れる (axis label 領域を含まない)。`spawn_chart_panel` で Y gutter 用と X gutter 用の空 child entity (Anchor 付き) を準備し、axis label system はそれらの子としてラベルを spawn する。

### Phase C: Pan + Zoom (Drag + Wheel)

**新規 `src/ui/chart_interaction.rs`:**

**Pan (translation):**

```rust
fn install_chart_drag_observer(
    mut commands: Commands,
    new_charts: Query<Entity, (Added<ChartViewState>, With<Sprite>)>,
) {
    for entity in &new_charts {
        commands.entity(entity).observe(
            |drag: Trigger<Pointer<Drag>>,
             mut chart_q: Query<&mut ChartViewState>| {
                // ⚠️ Bevy 0.15: trigger.entity()  (0.16+ で target() にリネーム)
                let Ok(mut state) = chart_q.get_mut(drag.entity()) else { return };
                state.translation.x += drag.delta.x;
                state.translation.y -= drag.delta.y;  // Bevy Y は上が正、Pointer delta は下が正
                state.auto_scale = false;             // pan 開始で autoscale off
            },
        );
    }
}
```

⚠️ **Bevy 0.15 では `trigger.entity()`**。`trigger.target()` は 0.16+ rename ([floating_window.rs:55-63](src/ui/floating_window.rs) の既存 observer と揃える)。

⚠️ **chart Sprite の Pointer<Drag> は WindowRoot の Pointer<Down> と競合する可能性**。WindowRoot は `Pointer<Down>` で z bump、`Pointer<Drag>` で window 移動 ([floating_window.rs:104-117](src/ui/floating_window.rs)) を観測している。chart Sprite を root の child にすると Bevy picking の propagation は子から親へ bubble するが、observer 内で `Pointer<Drag>::propagate(false)` を呼ぶか、root 側の drag observer に「子から来た drag は無視」のガードを入れる。**Phase C 着手時に bevy-engine スキルで propagation 規則を再確認**。

**Zoom (cell_width / cell_height):**

```rust
const ZOOM_SENSITIVITY: f32 = 30.0;
const MIN_CELL_WIDTH: f32 = 1.0;
const MAX_CELL_WIDTH: f32 = 50.0;
const MIN_CELL_HEIGHT: f32 = 0.1;
const MAX_CELL_HEIGHT: f32 = 1000.0;

fn chart_wheel_zoom_system(
    mut wheel: EventReader<MouseWheel>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut chart_q: Query<(&GlobalTransform, &Sprite, &mut ChartViewState)>,
) {
    for event in wheel.read() {
        // cursor world pos を計算 (camera.viewport_to_world_2d)
        // chart sprite bounds 内にあるか判定
        // cursor 中心ズーム: cell_width *= (1.0 + delta / ZOOM_SENSITIVITY)
        // translation.x -= (new_cursor_x - cursor_x);  ← flowsurface chart.rs:421 翻訳
    }
}
```

⚠️ **cursor 中心ズームの translation 補正は重要**。これが無いと「ズーム時に画面中央が動かない」flowsurface の挙動が出ない。flowsurface `chart.rs::Message::Scaled` ([line 400–428](.claude/skills/flowsurface/src/src/chart.rs)) の cursor delta 計算を逐行で写経する。

**Autoscale toggle:**

chart panel 右下に小ボタン (`spawn_button` 既存ヘルパ流用) を追加、click で `state.auto_scale = !state.auto_scale`。Phase E (polish) に回すか Phase C に含めるかは PR 粒度次第。

### Phase D: Crosshair

**新規 `src/ui/chart_crosshair.rs`:**

```rust
#[derive(Component, Default)]
pub struct CrosshairState {
    pub cursor_world: Option<Vec2>,    // chart 座標系の cursor 位置 (None = hover 外)
    pub hovered_price: Option<f32>,
    pub hovered_time_ms: Option<i64>,
}
```

chart entity spawn 時に `CrosshairState::default()` を一緒に挿入。

**Pointer<Move>/<Out> observer:**

```rust
fn install_chart_crosshair_observer(/* Added<CrosshairState>... */) {
    commands.entity(entity).observe(
        |trigger: Trigger<Pointer<Move>>,
         mut chart_q: Query<(&GlobalTransform, &Sprite, &ChartViewState, &mut CrosshairState)>| {
            let Ok((gt, sprite, state, mut crosshair)) = chart_q.get_mut(trigger.entity()) else { return };
            // trigger.event().hit.position は world space
            let local = trigger.event().hit.position.unwrap_or(Vec3::ZERO) - gt.translation();
            crosshair.cursor_world = Some(local.xy());
            crosshair.hovered_price = Some(state.y_to_price(local.y));
            crosshair.hovered_time_ms = Some(state.x_to_time_ms(local.x));
        }
    );
    commands.entity(entity).observe(
        |trigger: Trigger<Pointer<Out>>, mut chart_q: Query<&mut CrosshairState>| {
            if let Ok(mut crosshair) = chart_q.get_mut(trigger.entity()) {
                crosshair.cursor_world = None;
                crosshair.hovered_price = None;
                crosshair.hovered_time_ms = None;
            }
        }
    );
}
```

**`chart_crosshair_render_system`:**

```rust
fn chart_crosshair_render_system(
    mut painter: ShapePainter,
    chart_q: Query<(&GlobalTransform, &ChartViewState, &CrosshairState), Changed<CrosshairState>>,
    // ⚠️ Changed フィルタを付けても system は毎フレーム呼ばれる (空ループ)
) {
    for (gt, state, crosshair) in &chart_q {
        let Some(cursor) = crosshair.cursor_world else { continue };
        painter.set_translation(gt.translation() + Vec3::new(0.0, 0.0, 0.5));  // z + 0.5
        painter.color = Color::srgba(0.8, 0.8, 0.8, 0.5);
        painter.thickness = 1.0;
        // 縦線
        painter.line(Vec3::new(cursor.x, -state.bounds.y / 2.0, 0.5),
                     Vec3::new(cursor.x,  state.bounds.y / 2.0, 0.5));
        // 横線
        painter.line(Vec3::new(-state.bounds.x / 2.0, cursor.y, 0.5),
                     Vec3::new( state.bounds.x / 2.0, cursor.y, 0.5));
    }
}
```

**Readout badge (price/time):**

crosshair の cursor 位置に近い Y gutter / X gutter に「現在 hover 中の価格 / 時刻」を強調表示 (背景塗り + 太字)。Text2d 子 entity (`CrosshairBadge { target_chart }`) を毎フレーム despawn+respawn する system を追加 (`Changed<CrosshairState>` 駆動)。**axes label system と CrosshairBadge は同じ gutter に重なる可能性**があるので、CrosshairBadge は z + 0.6、axes label は z + 0.3 で重ね順を固定。

⚠️ **crosshair が動いた瞬間 main chart は再描画されない**ことを目視確認 (flowsurface の Cache 分離と等価動作)。`chart_main_render_system` は毎フレーム走る純 draw だが、`ChartViewState` 自体が変わらない限り出力は同一フレーム間で恒等。crosshair-only の変化で main draw が「重く」なる事象は起きない (毎フレーム走るのは固定コスト)。

### Phase E: Volume サブペイン + polish

**新規 `src/ui/chart_volume.rs`:**

**レイアウト分割:**

`ChartViewState.bounds` を「main 80% + volume 20%」に分割。Phase E で導入:

```rust
pub fn main_area_height(&self) -> f32  { self.bounds.y * 0.80 }
pub fn volume_area_height(&self) -> f32 { self.bounds.y * 0.20 }
pub fn volume_area_y_top(&self) -> f32  { -self.bounds.y / 2.0 + self.volume_area_height() }
// main area: y ∈ [volume_area_y_top, bounds.y / 2]
// volume area: y ∈ [-bounds.y / 2, volume_area_y_top]
```

`y_to_price` / `price_to_y` は main area の高さでスケールするように `Phase A の式を修正` (`cell_height * scaling` の代わりに `main_area_height` で正規化)。これは Phase A の `chart_viewstate.rs` を後付け修正することになるので、**Phase E 着手時に Phase A のテストを通したまま** 移行する。

**`volume_render_system`:**

```rust
fn volume_render_system(
    mut painter: ShapePainter,
    map: Res<InstrumentTradingDataMap>,
    chart_q: Query<(&GlobalTransform, &ChartInstrument, &ChartViewState)>,
) {
    for (gt, instrument, state) in &chart_q {
        let Some(data) = map.map.get(&instrument.instrument_id) else { continue };
        // visible candle slice (Phase A の autoscale と同じスライス計算)
        let max_volume = visible_candles.iter()
            .filter_map(|c| c.volume).fold(0.0_f32, f32::max);
        if max_volume <= 0.0 { continue; }
        painter.set_translation(gt.translation());

        for candle in visible_candles {
            let Some(vol) = candle.volume else { continue };
            let x = state.x_of_open_time(candle.open_time_ms);
            let bar_height = (vol / max_volume) * state.volume_area_height();
            let bar_bottom_y = -state.bounds.y / 2.0;
            let color = if candle.close >= candle.open {
                Color::srgba(0.0, 0.78, 0.31, 0.6)
            } else {
                Color::srgba(0.9, 0.2, 0.2, 0.6)
            };
            painter.color = color;
            painter.set_translation(Vec3::new(
                gt.translation().x + x,
                gt.translation().y + bar_bottom_y + bar_height / 2.0,
                gt.translation().z + 0.15,
            ));
            painter.rect(Vec2::new(state.cell_width * 0.8, bar_height));
        }
    }
}
```

⚠️ **`candle.volume: Option<f32>` が `None` の候補は skip**。Phase A0 で proto3 `optional float` にしているので `None` と `Some(0.0)` は区別可能。`None` の銘柄では volume サブペインが空のままになる (Phase A0 backend が埋めてくれていない symbol の場合)。

**Polish 項目:**

- Autoscale toggle button (Phase C で先に入れた場合は Phase E で見た目改善のみ)
- crosshair が volume サブペインに入ったときの volume readout (`hovered_volume: Option<f32>` を `CrosshairState` に追加)
- 旧 chart.rs の single-candle fallback ([chart.rs:225-257](src/ui/chart.rs)) を完全削除 (Phase A 以降に必要なし、`InstrumentTradingDataMap` から ohlc が来なければ何も描かない方が clean)
- 旧 `chart_render_system` 関連の dead test (`chart.rs:267-421` のうち実装ロジックに依存していないものは残す、`HistoryPoint` 直接参照のものは `InstrumentTradingData` 経由に書き換え)
- **BUY/SELL ボタンの spawn を削除** ([window.rs:60-79](src/ui/window.rs))。`spawn_button` 呼び出し 2 つ + `commands.entity(content_area).add_child(buy_button/sell_button)` を消す。`TradeButton::Buy` / `TradeButton::Sell` の Observer ([button.rs](src/ui/button.rs)) も spawn ポイントが消えれば dead code になるので、参照が他に無いことを `grep TradeButton::` で確認した上で `TradeButton` enum ごと削除。Reason: 売買入力は本フェーズではなく [Phase 9 - Live Account and Order API](docs/plan/Phase%209%20-%20Live%20Account%20and%20Order%20API.md) で**独立した「売買入力ウィンドウ」として新設される**ため、chart panel に同居させない (toy debug の遺物)。Phase 9 着手まで売買 UI が一時的に消える状態になることを Phase E の PR 説明に明記

## 触るファイル一覧

**新規 (6 ファイル):**
- `src/ui/chart_viewstate.rs` (Phase A) — `ChartViewState` Component + `chart_viewstate_update_system` + 座標変換ヘルパ
- `src/ui/chart_render.rs` (Phase A) — `chart_main_render_system` (純 draw)
- `src/ui/chart_axes.rs` (Phase B) — `calc_optimal_price_ticks` / `calc_optimal_time_step` + 2 system + `PriceLabel`/`TimeLabel` Component
- `src/ui/chart_interaction.rs` (Phase C) — pan/zoom observer + system
- `src/ui/chart_crosshair.rs` (Phase D) — `CrosshairState` + observer + render system + readout badge
- `src/ui/chart_volume.rs` (Phase E) — volume サブペイン render system

**新規アセット: なし** (flowsurface も追加 asset 無し)

**修正:**
- `python/proto/engine.proto` (Phase A0) — `OhlcPoint.volume = optional float`、`BackendTradingState.per_instrument = map<string, PerInstrumentState>`
- `python/engine/` (Phase A0) — nautilus aggregation 側で volume 集計、`per_instrument` map 構築
- `python/tests/` (Phase A0) — proto round-trip テスト追加
- `src/trading.rs` (Phase A0):
  - `OhlcPoint.volume: Option<f32>` 追加 (serde default)
  - `InstrumentTradingData` + `InstrumentTradingDataMap` Resource 追加
  - `TradingSession` Resource 追加
  - `backend_update_system` を `per_instrument` map loop に書き換え
  - `TradingData` は **同 PR 内では legacy alias として残す** (削除は Phase A0 後の cleanup PR)
- `src/ui/window.rs`:
  - (Phase A) `spawn_chart_panel` で chart entity に Sprite (transparent, custom_size) を付与
  - (Phase A) `instrument_chart_sync_system` に `map.remove()` の 3 行を追加 (lifecycle 整合)
  - (Phase A) `ChartInstrument` を chart entity 側にもコピー
  - (Phase E) `spawn_chart_panel` から BUY/SELL ボタン spawn ([window.rs:60-79](src/ui/window.rs)) を**削除** — 売買 UI は Phase 9 で独立ウィンドウとして新設
- `src/ui/button.rs` (Phase E) — `TradeButton::Buy` / `TradeButton::Sell` enum と関連 Observer を削除 (参照が他に無いことを事前 grep 確認)
- `src/ui/systems.rs` (Phase A0) — `update_price_display` を `InstrumentTradingDataMap` の SelectedSymbol lookup に書き換え
- `src/ui/sidebar.rs` (Phase A0) — `update_ticker_price_text_system` の fallback path を `InstrumentTradingDataMap` 経由に
- `src/ui/footer.rs` / `src/ui/menu_bar.rs` / `src/ui/replay_startup_window.rs` (Phase A0) — `Res<TradingData>` 経由の session-global フィールド読みを `Res<TradingSession>` に
- `src/ui/mod.rs`:
  - 6 モジュール宣言追加
  - 旧 `chart_render_system` を削除
  - 新規 7 system 登録 (Phase A〜E)、20-tuple 上限を超えそうなら `SystemSet` か別 `add_systems` 呼び出しに分割
- `src/ui/components.rs` (Phase B 以降) — axis label / crosshair badge 用の色定数 (`AXIS_LABEL_FG`, `CROSSHAIR_LINE`, `CROSSHAIR_BADGE_BG`, `VOLUME_BULL_BAR`, `VOLUME_BEAR_BAR`)

**削除:**
- `src/ui/chart.rs` (Phase A で `chart_viewstate.rs` / `chart_render.rs` に分割移管後に削除)

**unchanged だが確認のみ:**
- `src/ui/floating_window.rs:54-150` — Pointer observer の propagation 規則 (Phase C で chart drag が干渉しないか目視確認)
- `src/ui/layout_persistence.rs` — `ChartInstrument` 付き root は既に `LayoutExcluded` 経由で layout JSON から除外されている ([window.rs:30](src/ui/window.rs))、現状維持

## 再利用する既存ピース

- `spawn_floating_window` ([src/ui/floating_window.rs](src/ui/floating_window.rs)) — chart panel の枠はそのまま
- `BackendTradingState.last_prices` + `LastPrices.map` ([trading.rs:63, 511-514](src/trading.rs)) — per-instrument map の命名/構造の precedent。**新パターンを発明せず、これに揃える**
- `bevy_vector_shapes::ShapePainter` — Phase A/D/E すべてで draw bus として使う
- `chrono::DateTime` ([trading.rs:2](src/trading.rs)) — X 軸 time label の文字列フォーマットに使用
- `Pointer<Drag>` / `Pointer<Move>` / `Pointer<Out>` observer パターン ([src/ui/floating_window.rs:54-150](src/ui/floating_window.rs)) — `trigger.entity()` (Bevy 0.15) で揃える
- `instrument_chart_sync_system` ([window.rs:83](src/ui/window.rs)) — registry 同期、Phase A0 で 3 行追加のみ
- `Text2d` + `Anchor` — axis label / crosshair badge の retained text として既存 codebase 通り

## Caveat 一覧 (本タスクで踏みうるもの)

1. **chart entity は今 Sprite 無し** — `(Transform, ChartViewState)` のみ。Phase C の `Pointer<Drag>` を機能させるために Phase A で `Sprite { custom_size, color: Color::NONE }` を必ず付ける。ShapePainter で描いた背景は picking 対象外
2. **WindowRoot の Pointer<Down>/<Drag> と chart の Pointer<Drag> の競合** — chart Sprite を root の子として nest し、Phase C 着手時に `Pointer<Drag>::propagate(false)` または observer ガードで分離。設計確認は bevy-engine スキル発動で行う
3. **Bevy 0.15 は `trigger.entity()`** — `trigger.target()` は 0.16+ rename。[`floating_window.rs:55-63`](src/ui/floating_window.rs) 既存パターンと揃える
4. **`TradingData` の 2-resource 分割は mutator 1 箇所だけが本質** — `backend_update_system` ([trading.rs:279](src/trading.rs)) のみが per-instrument 化を必要とする。他 reader は単純な lookup 置換。Phase A0 のコアはこの 1 system + proto + nautilus aggregation
5. **proto3 `optional float volume`** — bare `float` だと 0.0 が "no data" と区別不能で Phase E volume サブペインが偽の zero-bar を描く。**explicit-presence で必ず定義**
6. **`OhlcPoint` は 2 箇所に存在** — `python/proto/engine.proto` (生成 Rust `engine::OhlcPoint`) と `src/trading.rs:21-29` (serde 付き手書き)。volume 追加時は両方触る + `backend_update_system` の conversion も
7. **`InstrumentTradingDataMap` の entry cleanup** — `instrument_chart_sync_system` で `map.remove()` を 3 行追加しないと、registry 退会後も entry が残る。replay/live 切替で銘柄入れ替わる用途では掃除する
8. **`ChartInstrument` は WindowRoot と chart entity の両方に持たせる** — root: registry 同期用 (既存), chart entity: `viewstate_update_system` のクエリ簡略化用 (Phase A で追加コピー)
9. **autoscale 計算は `chart_viewstate_update_system` に集約、`chart_main_render_system` は純 draw** — Phase A で旧 `chart_render_system` の autoscale ロジック ([chart.rs:113-162](src/ui/chart.rs)) を移管。draw 系で `mut ChartViewState` を取らない
10. **`ChartViewState` は `min_price`/`max_price` を持たず `base_price_y` + `cell_height` 表現に切替** — flowsurface 流のズーム中心固定 (`cursor_price = state.y_to_price(cursor_y); ... ; new_cursor_y = state.price_to_y(cursor_price); state.translation.y -= (new_cursor_y - cursor_y)`、[chart.rs:453-461](.claude/skills/flowsurface/src/src/chart.rs)) を可能にするため
11. **iced `canvas::Cache` retained-mode は Bevy 翻訳でスケジューラ層に降ろす** — Cache 4 層は 4 system に対応し、各 system が `Changed<...>` で early-out する。bevy_prototype_lyon 等の retained shape は導入しない (codebase に既存 user 無し)
12. **`Pointer<Move>` の `trigger.event().hit.position`** — world space pos が来る。chart 座標 (local) は `gt.translation()` を引いて算出。Y 軸の Bevy 流儀 (上が正) と Pointer delta (下が正) の符号反転に注意 (Phase C drag)
13. **`Changed<ChartViewState>` フィルタの per-entity 性** — 各 chart entity ごとに独立して立つ。複数 instrument の chart が同時表示されていても、片方の pan で他方の axis label が再生成されることはない
14. **`Res::is_changed()` は粒度が粗い** — `InstrumentTradingDataMap` は単一 Resource なので map 全体が変更時に立つ。entity 側の `last_seen_ohlc_len` local cache で per-instrument early-out する
15. **`add_systems` タプル 20 上限** — Phase A〜E で 7+ system 追加、既存 `mod.rs` のタプル数次第で `SystemSet` 分割か別 `add_systems` 呼び出しに (Phase 7.2 と同じ罠)
16. **gutter (axis label) と CrosshairBadge の重ね順** — Phase B axis label は z + 0.3、Phase D CrosshairBadge は z + 0.6 (cross line は + 0.5)。逆だと crosshair の値強調が固定ラベルに隠れる
17. **volume None の skip** — Phase E volume system は `candle.volume.is_none()` の bar を描かない。proto3 `optional float` を活かす唯一のポイント
18. **time axis の timezone** — flowsurface は `data::UserTimezone` をユーザ設定として持つ。本プランは初版 UTC 固定 (chrono の `DateTime<Utc>` で format)、ユーザ要望が出てから JST 等を追加
19. **`TradingData` の legacy alias 期間** — Phase A0 完了時点では `TradingData` を消さない。全 reader を `InstrumentTradingDataMap` または `TradingSession` に移行した cleanup PR (Phase A0.1 として A0 直後) で削除
20. **flowsurface `ViewState` の `tick_size` / `decimals`** — `tick_size` は最小価格刻み、`decimals` は表示桁。Phase A では `InstrumentTradingData` または `BackendTradingState` の symbol meta から拾う設計 (まずは hardcode `0.01` / `2` decimals、後で銘柄ごとに正しく拾うのは別タスク)
21. **Phase A 着手前に bevy-engine スキル + flowsurface スキル発動必須** — Bevy 0.15 罠 (`add_systems` 20 上限、observer の import path、Anchor 左寄せ、`trigger.entity()`) と flowsurface の `canvas::Program::draw` / `update` 経路 を navigator が読む

## Verification (各フェーズ完了時)

### コンパイル & 単体テスト
```bash
cargo check
cargo test --lib
python -m pytest python/tests/   # Phase A0 のみ
```

### E2E 手動検証 (`e2e-testing` スキル併用)

#### Phase A0
1. `python -m engine` 起動、`cargo run --bin backcast` で接続
2. 既存の単一 instrument チャートが回帰せず描かれる
3. 複数 instrument を `InstrumentRegistry` に積み、`InstrumentTradingDataMap.map` が各 id で別データを保持していることを `cargo test` のフィクスチャで確認
4. `BackendTradingState.per_instrument` map が proto round-trip で破損しないことを `python/tests/` で確認

#### Phase A
1. 既存と見た目同一の candle チャートが描かれる (regression test)
2. 複数 instrument を registry に積むと、各 chart パネルが**異なる OHLC** を描く (= per-instrument lookup が効いている)
3. chart entity に `Sprite { custom_size, color: Color::NONE }` が付いている (Bevy inspector で確認)
4. `ChartViewState::y_to_price(state.price_to_y(p))` ≈ `p` の round-trip テスト pass (`cargo test --lib`)

#### Phase B
1. chart 右に価格ラベル、下に時刻ラベルが表示される (5〜15 個目安、chart 高さに応じて自動増減)
2. autoscale で価格域が動いたとき、ラベルも追従する (`Changed<ChartViewState>` 駆動の確認)
3. timeframe が変わったとき、time label の刻みが M1 系列に切り替わる (flowsurface `M1_TIME_STEPS` 翻訳の確認)

#### Phase C
1. chart 領域を**ドラッグで pan** できる、左右に動かせる、上下にも動かせる
2. **マウスホイールでズームイン/アウト** ができる、cursor が中央にある時に zoom しても画面中央が動かない (cursor 中心ズームの確認 — flowsurface chart.rs:421 翻訳)
3. pan を始めた瞬間に autoscale が off になる (右下 toggle button があれば C 段階で C/A 切替可能)
4. **WindowRoot のドラッグ (タイトルバー)と chart のドラッグが混線しない** — title bar をドラッグで window 移動、chart 領域をドラッグで pan
5. **既存機能の非退行**: window title bar drag, close button, price text

#### Phase D
1. chart にマウスを乗せると**十字線 (crosshair)** が cursor 位置に表示される
2. crosshair の現在 hover 行に対応する**価格ラベルが強調表示** (背景塗り / 太字)、時刻ラベルも同様
3. **crosshair が動いている間、main chart は再描画されないこと** (= 軽量 — flowsurface `clear_crosshair` の Bevy 翻訳が効いている)
4. chart 領域から外に出たら crosshair が消える (`Pointer<Out>`)

#### Phase E
1. chart 下 20% に **volume サブペイン**が表示される、bull (緑) / bear (赤) で色分け
2. `OhlcPoint.volume = None` の symbol では volume サブペインが空のまま (`optional float` の効果確認)
3. crosshair を volume サブペイン上に持っていくと、**hover 中の volume 値が badge で表示**される
4. backend 側で nautilus aggregation が正しく volume を集計していること (engine の test で確認)
5. autoscale button (もし Phase C で先行導入していなければここで)、見た目改善

### 既存機能の非退行
- 銘柄追加/削除で chart window が spawn/despawn される (`instrument_chart_sync_system`)
- window 移動 (title bar drag) + close button
- replay/live mode 切替で `TradingSession.replay_state` が更新される (Phase A0 で session-global に分離した経路)
- Sidebar の ticker 一覧が `InstrumentTradingDataMap` 経由でも last_price を表示する (Phase A0 reader 移行の確認)
- Layout JSON Save/Load (chart panel は `LayoutExcluded` で除外されている、Phase A 以降も保たれる)

## 実装方針メモ

- **pair-relay 移行候補**: Phase A0 だけで proto + nautilus + trading.rs + reader 6 箇所書き換えで 500〜800 行、全フェーズ完遂は 1 セッションでは厳しい。**Phase A0 着手前に `pair-relay` スキルへ移行**、本プランを Navigator に引き継ぐのが安全。Navigator は事前に:
  - `bevy-engine` スキル — Bevy 0.15 罠 (observer の import path、`trigger.entity()`、`add_systems` 20 上限、Anchor 左寄せ)
  - `flowsurface` スキル — `canvas::Program::draw`/`update`、`Caches` 階層、`PlotConstants`、`scale/linear.rs`/`scale/timeseries.rs` の tick 計算 を読む
  - `nautilus_trader` スキル — Phase A0 の volume 集計 (BarType ごとの volume aggregation)
  - `tdd-workflow` + `rust-testing` — `calc_optimal_price_ticks` / `calc_optimal_time_step` の純関数テスト
- **Bevy 0.15 罠**: `add_systems` タプル 20 上限、observer の import path (`bevy::ecs::observer::Trigger`)、`trigger.entity()` (NOT `target()`)、Anchor 左寄せ (`bevy::sprite::Anchor::CenterLeft`) は `bevy-engine` スキル発動で都度確認
- **flowsurface 翻訳の精度確認手順**: 着手 1 コミット目で `examples/chart_smoke.rs` を作り、以下を `cargo run --example` で先に確認する:
  1. `calc_optimal_price_ticks(110.0, 100.0, 10)` が flowsurface 同等の (step=1.0, max=110.0) を返すこと
  2. `calc_optimal_time_step` の M1 step テーブルが flowsurface 1:1 で写経されていること
  3. `ChartViewState::price_to_y(y_to_price(y)) ≈ y` (round-trip テスト)
  4. cursor 中心ズーム後の translation 補正で「ズーム中心の価格」が画面上の同じピクセルに残ること (`cursor_price` 保存 → `cell_height *= 1.5` → `new_cursor_y - cursor_y` 補正 → `cursor_price` が同位置)
- **Phase F (将来) への布石**:
  1. heatmap モード — flowsurface `chart/heatmap.rs` (1056 行) を参照、別 PanelKind として実装
  2. 複数 timeframe 切替 (`Basis::Tick(...)` 追加、tick aggregation)
  3. indicator overlay (MA, EMA, RSI) — flowsurface `chart/indicator/` を参照
  4. theme editor (`modal/theme_editor.rs` 翻訳、`components.rs` の色定数を Resource 化)
  5. footprint / cluster mode — backend に per-bar trade breakdown が無いので proto 拡張から
