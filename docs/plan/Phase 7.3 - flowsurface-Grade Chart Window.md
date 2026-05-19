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

⚠️ chart entity の Sprite は **`WindowRoot` の子**にする (root に直接付けない)。root には既に `Pointer<Down>` の z-bump observer ([floating_window.rs:54-63](src/ui/floating_window.rs)) と `Pointer<Drag>` の window 移動 observer ([floating_window.rs:104-117](src/ui/floating_window.rs)) が乗っているため、chart Sprite を root に重ねると pan のドラッグ毎に window 全体が移動 / z 跳ね上がりが起きる。子として nest しても Bevy 0.15 picking の event は子→親へ bubble するので、**chart 側 observer の中で明示的に `trigger.propagate(false)` を呼んで bubble を止める** (Bevy 0.15 API: `trigger` は `Trigger<Pointer<...>>` 型で `propagate(bool)` メソッドを持つ)。Phase C / D 着手時に bevy-engine スキル発動で API シグネチャを確認。

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

**新規: `InstrumentTradingData` + `InstrumentTradingDataMap`** — per-instrument OHLC/history (last_price は除く、下記参照):

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
}

#[derive(Resource, Default)]
pub struct InstrumentTradingDataMap {
    pub map: HashMap<String, InstrumentTradingData>,
}
```

⚠️ **既存 `LastPrices { map: HashMap<String, f64> }` ([trading.rs:579-580](src/trading.rs)) と同じ命名**にする (`pub map: HashMap<...>` フィールド名を揃える)。これは「proto 側が `map<string, ...>` を持ち、Rust 側で Resource 化する」既存パターン (`BackendTradingState.last_prices` at [trading.rs:63](src/trading.rs)) の踏襲。新規パターンを発明しない。

⚠️ **last_price は `InstrumentTradingData` に持たせない**。既存 `LastPrices.map: HashMap<String, f64>` ([trading.rs:579-580](src/trading.rs)) を **single source of truth として残す**。理由: `LastPrices` は既に backend_update_system 経由で per-instrument に埋まっており、sidebar/footer など 5+ reader が `marker.instrument_id` でこの map から lookup する完成形になっている ([sidebar.rs:376](src/ui/sidebar.rs))。`InstrumentTradingData.last_price` を新設すると同一情報が 2 Resource に散らばり、どちらが authoritative かが曖昧になる。chart 側は `latest close = ohlc_points.last().close` で代用可、別 panel が last_price 単独で欲しければ `Res<LastPrices>` を引く。

**blast radius (調査済、A0 で消化):**

- **Mutator 3 箇所**: `backend_update_system` ([trading.rs:279](src/trading.rs) ← 唯一の本物 gRPC 入口、ここだけが per-instrument 化する重要箇所), `price_simulation_system` (synthetic, 削除候補), Buy/Sell button (toy debug、Phase E で削除されるので A0 では当面残す)
- **Reader — render-only, 移行容易 (5 箇所)**: `chart.rs:85` (本プラン本丸 → `InstrumentTradingDataMap`), `systems.rs::update_price_display` (Text2d 1 個、tick 価格なので `LastPrices` の `SelectedSymbol` keyed lookup へ), `sidebar.rs::update_instrument_price_text_system` ([sidebar.rs:376](src/ui/sidebar.rs) で既に `LastPrices.map.get(&marker.instrument_id)` keying 済、フォールバックの `trading.close` 経路だけ `InstrumentTradingDataMap` 経由に差し替え), `footer.rs:256,458,561` (session-global なので `TradingSession` 経由), `menu_bar.rs:726,810` (同 session-global)
- **Reader — session-global (3 箇所)**: `replay_startup_window.rs:201,294,321,348,376` (`replay_state` / `timestamp_ms` のみ → `TradingSession`)

「mutator は実質 1 箇所 (`backend_update_system`)」というのが最大の発見。proto を `repeated PerInstrumentState` または `map<string, PerInstrumentState>` 化すれば、ここ 1 つの loop で `InstrumentTradingDataMap` を埋められる。

### `InstrumentTradingDataMap` の lifecycle

⚠️ **`InstrumentRegistry` 退会時に map entry が残る問題**: `instrument_chart_sync_system` ([window.rs:83–107](src/ui/window.rs)) は registry 変更時に chart `WindowRoot` を despawn するが、**map からの削除は誰もやらない**。Phase A0 で `instrument_chart_sync_system` に「desired から消えた id を `map.remove()` する」3 行を追加。あるいは「entry が増え続けても銘柄ユニバース有限なので無視」も選択肢だが、replay/live 切替で銘柄が完全に入れ替わるシナリオを想定すると掃除した方が安全。

⚠️ **close button 経路の map cleanup 漏れ (Critical)**: chart panel の close (×) observer ([floating_window.rs:239–246](src/ui/floating_window.rs)) は `registry.remove(&ci.instrument_id)` と `commands.entity(root_entity).despawn_recursive()` を **observer 内で直接実行**している。次フレームの `instrument_chart_sync_system` 視点では既に `chart_q` から該当 root が消えているため、「`desired` から消えた id を sync 側で `map.remove()`」する設計だと **close 経路では map が残ったまま leak する**。対策は 2 択:
- **(推奨) close observer 自身に `map.remove(&ci.instrument_id)` を追加**: observer 引数に `mut map: ResMut<InstrumentTradingDataMap>` を増やし、`registry.remove` の直後に `map.map.remove(&ci.instrument_id);` を呼ぶ。registry.remove と同一観測子で「entity 削除直前に map も外す」と single source of truth が保たれる
- (代替) close observer は `registry.remove()` のみに留め、despawn を sync system に一元化する。ただし「即時 close フィードバックが 1 フレーム遅延する」ため UX 退行となるので採用しない
Phase A0 の touched-files に **`src/ui/floating_window.rs:239–246` の close observer 拡張**を明示追加する (`instrument_chart_sync_system` の 3 行追加だけでは不十分)。

### Cache の Bevy 翻訳 (Critical)

flowsurface の `Caches { main, x_labels, y_labels, crosshair }` ([chart.rs:625–646](.claude/skills/flowsurface/src/src/chart.rs)) は iced の `canvas::Cache` (retained GPU geometry、`.clear()` まで再描画されない) を 4 層持ち、`clear_crosshair` は `crosshair + y_labels + x_labels` をクリアして main は据え置きにする (= crosshair 移動だけで main を再描画しない最適化)。

Bevy `ShapePainter` は immediate-mode で毎フレーム redraw を強制するので、retained-mode の cache 概念は**そのままは持ち込めない**。本プランでは **「Cache を per-layer system に翻訳」** する:

| flowsurface Cache | 本プラン Bevy system | 駆動方針 |
|---|---|---|
| `Caches::main` | `chart_main_render_system` (Phase A 再構築) | **毎フレーム draw** (ShapePainter は immediate-mode、1 フレームでも描画スキップすると candle が消える)。`ChartViewState` は read-only |
| `Caches::y_labels` | `price_axis_labels_system` (Phase B) | `Changed<ChartViewState>` で発火、Text2d 子 entity (= retained、明示 despawn まで残る) を despawn+respawn |
| `Caches::x_labels` | `time_axis_labels_system` (Phase B) | 同上、`Changed<ChartViewState>` のみ |
| `Caches::crosshair` | `chart_crosshair_render_system` (Phase D) | **毎フレーム draw** (ShapePainter は immediate-mode)。`CrosshairState.cursor_world == None` のときだけ早期 continue、描画自体は毎フレーム発行する |

⚠️ **重要 (immediate-mode 罠)**: `bevy_vector_shapes::ShapePainter` の出力は GPU に retain されない。`Changed<CrosshairState>` や `Changed<InstrumentTradingDataMap>` でクエリ filter すると、変化が無いフレームでは ShapePainter 命令が 0 件 = **次フレームで cross line / candle / volume bar が画面から消える**。「Changed フィルタ」で early-out して良いのは Text2d 子 entity の despawn+respawn など retained-mode の出力のみ。ShapePainter 描画 system はすべて **filter 無しの全 entity ループ** にする。

**重要な不変条件**: chart の「重い計算」(autoscale 範囲、表示候補 OHLC スライス、tick step 計算) は `chart_viewstate_update_system` (新規、Phase A) に集約し、**`InstrumentTradingDataMap` または `ChartViewState` の変化を検出したときだけ走る** (具体的なクエリ条件は Phase A 節で確定)。`chart_main_render_system` は ViewState を immutable に読む純 draw に徹し、毎フレーム走っても OHLC ≤1000 点なら無視できる。これで flowsurface の「Cache 階層化による責務分割」を**スケジューラ層で再現**する。

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
1a. chart_data_tick_system           (map.is_changed() で last_seen_ohlc_len 更新 + RequestAutoscale)
1b. chart_interaction_tick_system    (Changed<ChartViewState> read-only で RequestAutoscale)
1c. chart_autoscale_apply_system     (Event 駆動、base_price_y / cell_height を確定)
2a. chart_interaction_drag_handler   ⚠️ **observer (Pointer<Drag>)、Update schedule 外で event-driven 発火、`configure_sets` 対象外**)  [Phase C]
2b. chart_scroll_zoom_system         (regular system, `EventReader<MouseWheel>` + HoverMap、cell_width/height 更新)  [Phase C]
2c. chart_crosshair_input_handler    ⚠️ **observer (Pointer<Move>/<Out>)、`configure_sets` 対象外**  [Phase D]

[render-ish — 描画 system 群、いずれも .after(ChartSet::Autoscale)]
3.  chart_main_render_system         (毎フレーム、ShapePainter 描画、filter 無し)  [Phase A 再構築]
4.  price_axis_labels_system         (Changed<ChartViewState> で Text2d 子 entity を despawn+respawn)  [Phase B]
5.  time_axis_labels_system          (Changed<ChartViewState>)  [Phase B]
6a. chart_crosshair_derive_system    (Changed<CrosshairState> で hovered_price / hovered_time_ms を Render 段で計算)  [Phase D]
6b. chart_crosshair_render_system    (毎フレーム、ShapePainter で別 z。cursor_world==None で continue)  [Phase D]
7.  chart_volume_render_system       (毎フレーム、ShapePainter 描画、filter 無し)  [Phase E]
```

**`ChartSet` enum + `.configure_sets` で順序を明示** (Bevy 0.15 ambient parallelism 回避):

```rust
#[derive(SystemSet, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ChartSet {
    DataTick,      // 1a, 1b (regular system)
    Autoscale,     // 1c (regular system, event apply)
    Interaction,   // 2b のみ (chart_scroll_zoom_system, regular system)
    Render,        // 3, 4, 5, 6, 7 (regular systems)
}

app.configure_sets(Update, (
    ChartSet::DataTick.after(crate::trading::backend_update_system),
    ChartSet::Autoscale.after(ChartSet::DataTick),
    ChartSet::Interaction.after(ChartSet::Autoscale),
    ChartSet::Render.after(ChartSet::Autoscale).after(ChartSet::Interaction),
));
```

これで `chart_main_render_system` が「同一フレーム内で `base_price_y` 確定済み」を保証する。`Interaction` を `Autoscale` の after に置くのは、scroll zoom 後の cursor 中心補正で `interval_to_x` / `price_to_y` が最新の `base_price_y` を必要とするため。

⚠️ **observer 系 handler (2a `chart_interaction_drag_handler`, 2c `chart_crosshair_input_handler`) は `ChartSet` に入れない**。Bevy 0.15 の observer は `Update` schedule の外で event-driven に発火し、`configure_sets` で順序制約をかける手段が無い。幸い両者は `base_price_y` を読まない (drag は translation のスクリーン差分、crosshair は autoscale 値に依存しない `y_to_price`/`x_to_time_ms` の純関数読み) ので順序非依存で安全。**もし将来 observer ロジックが autoscale 結果に依存するようになったら、観測した値を Component に書いて次フレーム regular system で消費する設計に切り替える** (observer 内で `Res<ChartViewState>` の最新値を要求しない)。

⚠️ **既存 `chart_render_system` ([src/ui/mod.rs:151](src/ui/mod.rs)) は Phase A で削除し、3 と 7 に分割する**。Phase A 着手前に `mod.rs` の `add_systems` タプルが何個入っているか実カウントし、20-tuple 上限 (現状 mod.rs の Update タプルは 18-19 個、Phase A〜E で 6+ system 追加するため境界付近に到達)を超えるなら `SystemSet` または別 `app.add_systems(Update, (...))` 呼び出しに分割する (Phase 7.2 と同じ罠)。

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
  - `backend_update_system` を `per_instrument` map を loop して `InstrumentTradingDataMap.map` を埋める形に書き換える。同 system 内で `TradingSession` の `timestamp_ms` / `replay_state` / `timer` も更新する
  - `instrument_chart_sync_system` ([window.rs:107 付近](src/ui/window.rs)) で「`desired` から消えた id は `map.remove(id)` する」3 行を追加
- **既存 `TradingData` reader / mutator の移行 — A0 同一 PR で全箇所を完了する**:
  - `chart.rs:85` (chart_render_system) → `Res<InstrumentTradingDataMap>` + 自 chart entity の `ChartInstrument` で lookup。**Phase A0 では既存 `chart_render_system` のロジックは保持したまま data source だけ差し替える** (chart.rs 全体のリファクタは Phase A 本体の仕事)
    - ⚠️ **`ChartInstrument` の chart entity 側 duplicate も A0 で行う**。現状 `ChartInstrument` は root entity のみ ([window.rs:27](src/ui/window.rs)) で chart entity は `(Transform, ChartViewState)` のみ。`chart_render_system` の Query を `(&mut ChartViewState, &GlobalTransform, &ChartInstrument)` に変えて per-instrument lookup を成立させるためには A0 時点で chart entity 側にも `ChartInstrument { instrument_id }` を insert する必要がある。代替案 (Parent → root の `ChartInstrument` を join) は Bevy 0.15 で二段引きクエリになり、Phase A で結局 duplicate する設計に戻すので、最初から A0 で duplicate する方が手戻りが少ない。Phase A 節の「`ChartInstrument` を chart entity 側にもコピー」記述は「A0 で既に済んでいる前提」に読み替える
  - `systems.rs::update_price_display` → `Res<LastPrices>` + `Res<SelectedSymbol>` lookup
  - `sidebar.rs::update_instrument_price_text_system` ([sidebar.rs:368](src/ui/sidebar.rs)) → **既に `LastPrices.map` keyed lookup のみで `Res<TradingData>` を参照していない。A0 で触る必要なし** (touched-files から外す)
  - `footer.rs:256,458,561` → `Res<TradingSession>` (session-global フィールドのみ参照)
  - `menu_bar.rs:726,810` → `Res<TradingSession>`
  - `replay_startup_window.rs:201,294,321,348,376` → `Res<TradingSession>`
  - **`src/main.rs:133` の `app.insert_resource(TradingData::default())` を `InstrumentTradingDataMap::default() + TradingSession::default()` の 2 insert に置換**
  - **`src/main.rs:151` の `price_simulation_system` 登録を削除** (system 本体も A0 で削除、`main.rs:9` の `use trading::{..., price_simulation_system, ...}` import も同一 PR で外す)
  - **`src/ui/button.rs:23` の `TradeButton::Buy/Sell` click observer が `ResMut<TradingData>` を取って toy 売買を反映している。これは Phase E で削除予定だが A0 でも compile blocker になるため、A0 で同 observer 内の `ResMut<TradingData>` 依存を `ResMut<InstrumentTradingDataMap>` + `ChartInstrument` lookup に置換するか、**Buy/Sell button 自体を A0 で削除して Phase E の前倒し**にする (Phase 9 で売買 UI を独立ウィンドウとして新設するので、A0 時点で削除しても回帰なし)。後者を**推奨**
  - **`TradeButton` enum を A0 で削除する場合は連鎖削除が 3 箇所**:
    1. `src/ui/components.rs:24` の `TradeButton` enum 定義
    2. `src/ui/button.rs:23` の click observer
    3. `src/ui/systems.rs:42` の `button_system` (Pressed/Hovered/None 色更新、`TradeButton::Buy/Sell` を直接参照) と `src/ui/mod.rs:90` の import / `mod.rs:156` の `add_systems` 登録
    4. `src/ui/window.rs:60-79` の Buy/Sell ボタン spawn (Phase E の touched-files に既述、A0 前倒し時は A0 で消す)
    `button_system` 削除を漏らすと `TradeButton` enum を消した時点で compile failure する (Phase E の touched-files にも明記されていないので A0 / Phase E どちらで削除するにせよ抜けやすい)
- 全 reader / mutator 移行完了後、以下の grep がすべて **0 行**になっていることを A0 PR landing 条件とする (旧版の `Res<TradingData>` 1 種類だけでは `ResMut<TradingData>` や直接型参照を漏らす):

```
rg -n '\bTradingData\b|ResMut<\s*TradingData\s*>|Res<\s*TradingData\s*>' src/
```

⚠️ **必ず単語境界 `\b` を入れる**。`\b` 無しの素朴な `'TradingData|...'` だと **`InstrumentTradingData` / `InstrumentTradingDataMap` / `BackendTradingState` 等の substring にもヒット**し、新設した型が landing 条件を満たさなくなる。`Res<\s*...\s*>` のように内側に `\s*` を入れるのは `Res< TradingData >` のような spacing variant を取りこぼさないため。

`TradingData` 構造体を A0 同一 PR で削除する (上記 grep が 0 行になった時点)。**legacy alias は残さない**

**Phase A0 確定スコープ (重要)**: 「new resource 追加」+「`backend_update_system` 書き換え」+「全 reader / mutator 移行 (button.rs / main.rs / price_simulation_system 削除を含む)」+「`TradingData` 削除」+「proto/nautilus volume 集計」を**全て同一 PR**で landing する。**「legacy 経路を残してフェーズ間で並走する」設計は採用しない** (per-instrument 経路と single resource 経路が二重に存在すると `price_simulation_system` ↔ `backend_update_system` の write 順依存が二系統で発生し、後続フェーズの bug 表面化が遅延するため)。

⚠️ **`price_simulation_system` (synthetic) は A0 で削除する**。理由: A0 完了後は `InstrumentTradingDataMap` が single source of truth になり、synthetic と backend-driven の 2 経路が同 map に書くと last-write-wins で chart が振動する。本プロジェクトは backend (`python -m engine`) 起動前提なので synthetic 経路は dead code 化して問題無い。E2E baseline 確認は backend 起動状態で行う。

**新規アセット: なし**

**Verification:**
- `cargo check` + `cargo test --lib` (`rg 'TradingData|ResMut<TradingData>|Res<TradingData>' src/` の結果が 0 行を含む)
- `python -m pytest python/tests/` で proto/aggregation の volume を確認
- `python -m engine` 起動 + `cargo run --bin backcast` → 単一 instrument の OHLC 描画が回帰していないこと、複数 instrument 投入で `InstrumentTradingDataMap` の各 entry が独立に更新されていること

⚠️ **Phase A0 のスコープ**: data 経路 + reader 全移行 + `TradingData` 削除 + `price_simulation_system` 削除。**UI レイアウト変更や Sprite hit-target 追加は含まない** (それは Phase A 本体)。chart.rs の draw ロジック自体は Phase A0 時点では既存のまま (data source のフィールド経路だけ差し替え)。

### Phase A: ChartViewState 再設計 + Sprite hit-target + system 分割

**新規 `src/ui/chart_viewstate.rs`:**

```rust
#[derive(Component)]
// ⚠️ Default は手書きする (`#[derive(Default)]` だと bool field が `false` で
//    spawn 時に autoscale off になり chart が flat `cell_height = 1.0` で描画される)
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
    pub last_seen_ohlc_signature: u64, // per-instrument early-out 用 signature (詳細は data_tick_system 節)
}

#[derive(Debug, Clone, Copy)]
pub enum Basis {
    Time(u64),  // timeframe in ms (例: 60_000 = 1min)
}

impl Default for ChartViewState {
    fn default() -> Self {
        Self {
            bounds: CHART_DRAW_SIZE,
            translation: Vec2::ZERO,
            scaling: 1.0,
            cell_width: 6.0,
            cell_height: 1.0,                  // autoscale が初回 frame で上書き
            basis: Basis::Time(60_000),
            latest_x: 0,
            base_price_y: 0.0,
            decimals: 2,
            tick_size: 0.01,
            auto_scale: true,                  // ⚠️ 必須: false だと chart が flat 表示で spawn する
            last_seen_ohlc_signature: u64::MAX,// sentinel: 初回 data tick で必ず差分
        }
    }
}
```

**座標変換ヘルパ (impl ChartViewState、Phase A で全て定義):**

| ヘルパ | 用途 | フェーズ |
|---|---|---|
| `price_to_y(price: f32) -> f32` | 価格 → chart local y 座標 | A (main draw / price label) |
| `y_to_price(y: f32) -> f32` | chart local y → 価格 (`price_to_y` の逆関数) | A (crosshair / round-trip テスト) |
| `interval_to_x(open_time_ms: i64) -> f32` | candle 時刻 → chart local x 座標 | A (main draw / x label) |
| `x_to_time_ms(x: f32) -> i64` | chart local x → ms 時刻 (`interval_to_x` の逆関数) | D (crosshair readout) |
| `visible_price_range() -> (f32, f32)` | `(low, high)` 表示価格域。`bounds.y` と `translation` / `scaling` / `base_price_y` / `cell_height` から逆算 | B (price label 列挙) |
| `visible_time_range() -> (i64, i64)` | `(earliest_ms, latest_ms)` 表示時刻域 | B (time label 列挙) |
| `visible_candle_slice<'a>(ohlc: &'a [OhlcPoint]) -> &'a [OhlcPoint]` | 表示中の candle スライス。Phase A autoscale と Phase E volume 集計で共用 | A |
| `main_area_height() -> f32` / `volume_area_height() -> f32` / `main_area_y_bottom() -> f32` | main area / volume area の y 境界。**Phase A で先取り**して定義 — Phase D crosshair の main-area gate と Phase E volume render が依存。詳細式は Phase E 節「レイアウト分割」を参照 | A |

flowsurface ([chart.rs ViewState impl](.claude/skills/flowsurface/src/src/chart.rs)) の `y_to_price` / `x_to_interval` をそのまま翻訳。`#[cfg(test)]` で「round-trip: `y_to_price(price_to_y(p))` ≈ p」「`x_to_time_ms(interval_to_x(t))` == t」のテストを必ず置く (Phase C で pan/zoom が乗ったときに崩れないため)。

**`chart_viewstate_update_system` (Phase A の中核):**

⚠️ **クエリの entity 分布を Phase A 着手前に決定**: 現状 `ChartInstrument` は root entity 側にのみ存在し、`ChartViewState` は子の chart entity 側のみに存在する。Phase A では **chart entity 側にも `ChartInstrument` を duplicate** し、クエリは子 entity のみを対象にする (`With<WindowRoot>` を付けない)。Root 側の `ChartInstrument` は registry sync (`instrument_chart_sync_system`) 用にそのまま残す。これにより `viewstate_update_system` は `ParentQuery` 経由の join が不要になる。

**設計方針: self-`Changed` ループを避けるため、autoscale 再計算は `Event` でトリガする** (data 経路と interaction 経路を分離し、両者とも純 writer にする)。

```rust
#[derive(Event)]
pub struct RequestAutoscale { pub chart: Entity }

fn chart_data_tick_system(
    map: Res<InstrumentTradingDataMap>,
    mut chart_q: Query<(Entity, &ChartInstrument, &mut ChartViewState)>,
    mut req: EventWriter<RequestAutoscale>,
) {
    if !map.is_changed() { return; }
    for (e, chart_instrument, mut state) in &mut chart_q {
        let Some(data) = map.map.get(&chart_instrument.instrument_id) else { continue };
        // ⚠️ **`len()` 単独は intra-bar 更新を取りこぼす** — 同一バー内で high/low/close が動いた場合
        //    `ohlc_points.len()` は変わらないので「変化なし」と判定されてしまい、新高値・新安値が
        //    autoscale に反映されず chart 外に飛び出す。len + 末尾 bar の (open_time, high, low,
        //    close, volume) を bit-mix した signature で差分検出する。
        let signature = compute_ohlc_signature(&data.ohlc_points);
        if signature == state.last_seen_ohlc_signature { continue; }
        state.last_seen_ohlc_signature = signature;
        if let Some(last) = data.ohlc_points.last() {
            state.latest_x = last.open_time_ms;
        }
        if state.auto_scale { req.send(RequestAutoscale { chart: e }); }
    }
}

/// `(len, last.open_time_ms, last.high.to_bits(), last.low.to_bits(),
///  last.close.to_bits(), last.volume.unwrap_or(0.0).to_bits())` を u64 へ畳む。
/// 偽陰性 (異なる状態が同じ signature) は 2^-64 で実害無し、偽陽性 (同状態で signature が変わる) は
/// 下記の bit-mix が決定的なので発生しない。autoscale が 1 frame 余計に走るのは許容。
///
/// ⚠️ **`bevy::utils::AHasher::default()` / `std::collections::hash_map::DefaultHasher::new()`
///    は使わない**。両者とも `RandomState` 経由で **per-instance random keys** を生成するため、
///    同じ入力でも 2 回呼ぶと別の u64 が返り、`signature == last_seen_ohlc_signature` が
///    永久に成立しない (= per-instrument early-out が無効化される)。ahash の固定キー版
///    (`AHasher::new_with_keys(0, 0)`) を使うか、下記のように `u64::wrapping_mul` + `xor` で
///    自前 mix する。本プランは依存追加無しで済む後者を採用。
fn compute_ohlc_signature(ohlc: &[OhlcPoint]) -> u64 {
    // FNV-1a 風の決定的 mix (key 不要、`#[no_std]`/再現性 OK)
    const FNV_PRIME: u64 = 0x100000001b3;
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    let mut h: u64 = FNV_OFFSET;
    let mut mix = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(FNV_PRIME);
    };
    mix(ohlc.len() as u64);
    if let Some(last) = ohlc.last() {
        mix(last.open_time_ms as u64);
        mix(last.high.to_bits() as u64);
        mix(last.low.to_bits() as u64);
        mix(last.close.to_bits() as u64);
        mix(last.volume.unwrap_or(0.0).to_bits() as u64);
    }
    h
}

fn chart_interaction_tick_system(
    // pan/zoom 直後 + spawn フレームで発火。`Added<T>` は `Changed<T>` を含む (Bevy invariant) ので
    // `Changed` 単独で初回も拾える。
    interaction_q: Query<Entity, Changed<ChartViewState>>,
    auto_q: Query<&ChartViewState>,
    mut req: EventWriter<RequestAutoscale>,
) {
    for e in &interaction_q {
        if let Ok(state) = auto_q.get(e) {
            if state.auto_scale { req.send(RequestAutoscale { chart: e }); }
        }
    }
}

fn chart_autoscale_apply_system(
    mut events: EventReader<RequestAutoscale>,
    map: Res<InstrumentTradingDataMap>,
    mut chart_q: Query<(&ChartInstrument, &mut ChartViewState)>,
) {
    // dedupe: 同フレームに同 chart が複数回 request しても初出の 1 回だけ適用
    // (`HashSet::insert` は新規追加時に true を返すので filter で残るのは各 chart の最初の event。
    //  autoscale は決定的 (同フレームの InstrumentTradingDataMap + ChartViewState から base_price_y/cell_height を一意に算出) なので
    //  「初出 1 回」と「末尾 1 回」は結果同値だが、API の意味と乖離する記述を避けるため "初出" と書く)
    let mut seen = bevy::utils::HashSet::<Entity>::new();
    for ev in events.read().filter(|ev| seen.insert(ev.chart)) {
        let Ok((chart_instrument, mut state)) = chart_q.get_mut(ev.chart) else { continue };
        let Some(data) = map.map.get(&chart_instrument.instrument_id) else { continue };
        // 現行 chart.rs:113-162 の autoscale ロジックを移植 (visible slice → min/max → 10% padding)
        let (new_base_price_y, new_cell_height) = compute_autoscale(&state, data);
        // ⚠️ **load-bearing**: `&mut state` への代入は値が変化したときのみ行う。
        //    同値での代入でも DerefMut を踏み Changed が立ち、次フレームで
        //    chart_interaction_tick_system が再度 event を出し無限ループする。
        if (state.base_price_y - new_base_price_y).abs() > f32::EPSILON {
            state.base_price_y = new_base_price_y;
        }
        if (state.cell_height - new_cell_height).abs() > f32::EPSILON {
            state.cell_height = new_cell_height;
        }
    }
}
```

**なぜこれで self-loop が起きないか:**
- `chart_data_tick_system` は **`map.is_changed()` を gate** にし `ChartViewState` を書く (`Changed<ChartViewState>` を読まない → loop しない)
- `chart_interaction_tick_system` は **`Changed<ChartViewState>` を read-only** で受け event を出すだけ (`mut` で書かない → 次フレームの自分自身を発火させない)
- `chart_autoscale_apply_system` は **event 駆動**、event が無いフレームは何もしない。書き込みは次フレームの interaction_tick で `Changed` を再度立てるが、その時点で `auto_scale` 変化や cursor 中心ズーム以外では新 event を出さないため収束する
- `Added<T>` は `Changed<T>` を含む (Bevy invariant) ので `Changed` 単独 query で spawn フレームの「初回 autoscale」も自然に走る

⚠️ **収束の load-bearing 条件**: `chart_autoscale_apply_system` 内で値が変化したときのみ `&mut state` 経由代入を行う (上記 code skeleton の `if (...).abs() > EPSILON` ガード)。同値代入で DerefMut を踏むと Changed が立ち、interaction_tick が autoscale 確定後の値変化を「ユーザ操作」と区別できず無限 loop する。**「応用上」「optional optimization」ではなく必須実装**。

⚠️ **収束テスト必須**: `cargo test --lib` で「chart spawn → 5 frame schedule 実行 → frame 3 以降 `Changed<ChartViewState>` イベントカウントが 0」を assert する。Phase A landing 条件。

⚠️ **`state.is_added()` は使わない**: `Mut<T>` 経由でも `is_changed()` のみ。`Added<ChartViewState>` を上記のように **クエリ filter** で扱うのが正解。

⚠️ **既存 `ChartViewState` は `min_price` / `max_price` を直接持っていたが、Phase A 以降は `base_price_y` + `cell_height` で表現する**。pan/zoom 時に min/max を直接動かす旧設計だと「ズーム中心が画面中央に固定される」flowsurface パターンが組めない (`cell_height *= factor; translation.y -= (new_cursor_y - cursor_y)` で cursor 中心ズームを実現するため。`chart.rs:429-466` 参照)。

**chart entity の Sprite hit-target 追加 (Critical):**

⚠️ **レイアウト定数は Phase A で一括導入する** — `CHART_DRAW_SIZE` / `PRICE_GUTTER_WIDTH` / `TIME_GUTTER_HEIGHT` / `CHART_PANEL_SIZE` の 4 定数は Phase B の axis label 領域導入を見越して **Phase A で先に宣言**する (Phase A の `Sprite.custom_size` / `ChartViewState.bounds` が `CHART_DRAW_SIZE` を必要とするため、Phase B まで定義を遅らせると Phase A 中で「未定義 ident」になる)。Phase A 時点では `PriceGutter` / `TimeGutter` の子 entity spawn は不要 — 定数だけ確定し、gutter 子 entity は Phase B で追加する:

```rust
// src/ui/chart_viewstate.rs (Phase A で確定):
pub const PRICE_GUTTER_WIDTH: f32 = 50.0;                   // Y 軸ラベル領域 (右、Phase B で使用)
pub const TIME_GUTTER_HEIGHT: f32 = 24.0;                   // X 軸ラベル領域 (下、Phase B で使用)
pub const CHART_DRAW_SIZE: Vec2 = Vec2::new(310.0, 180.0);  // 実描画領域 (Phase A から使用)
pub const CHART_PANEL_SIZE: Vec2 = Vec2::new(
    CHART_DRAW_SIZE.x + PRICE_GUTTER_WIDTH,                 // 360 (Phase A の WindowRoot サイズ計算用)
    CHART_DRAW_SIZE.y + TIME_GUTTER_HEIGHT,                 // 204
);
```

```rust
// spawn_chart_panel ([window.rs:48-58]) の chart entity spawn を以下に書き換える
// Layout 定数 (Phase A 確定):
//   TITLE_BAR_HEIGHT は spawn_floating_window 側の値と一致させる
//   ([floating_window.rs:39](src/ui/floating_window.rs) の `const TITLE_BAR_HEIGHT: f32 = 40.0;` を Read で確認済)
//   CHART_Y_OFFSET = title bar を避けて draw area を panel 中心に置くオフセット
const TITLE_BAR_HEIGHT: f32 = 40.0;   // ← floating_window.rs:39 と一致させる必須値
// ⚠️ 符号と量は `spawn_floating_window` の panel origin / title bar 配置に依存する。
//    WindowRoot の origin が panel 中心 + title bar が上端 (Bevy Y は上が正) 前提で
//    chart child を下にずらすため `-TITLE_BAR_HEIGHT/2.0`。
//    Phase A 着手時に `src/ui/floating_window.rs` の title bar Y / panel size を Read で
//    cross-check してから const 確定 (旧版 literal `10.0` は逆符号の可能性があった)。
const CHART_Y_OFFSET: f32 = -(TITLE_BAR_HEIGHT) / 2.0;
let chart = commands.spawn((
    Transform::from_xyz(0.0, CHART_Y_OFFSET, 0.1),
    Sprite {
        custom_size: Some(CHART_DRAW_SIZE),  // (310, 180) — gutter 領域は別 child entity
        // ⚠️ alpha=0 (`Color::NONE`) は `bevy_sprite_picking_backend` の picking mode 次第で
        //    hit-target から除外される (Bevy 0.15.x で `SpritePickingMode::AlphaThreshold(...)` が
        //    enabled な場合は alpha < threshold の sprite は ignore される)。
        //    既存 codebase の picking 対象 Sprite は alpha 0.05〜0.85 で、alpha=0 picking の前例は無い。
        //    安全側に倒すため alpha 0.001 を使う (視覚的には完全透明と区別不能、ShapePainter の描画と重ならない)。
        //    Phase A 着手時に `bevy::picking::backend::sprite::SpritePickingSettings::default()` の値を
        //    `cargo doc --open` で確認し、`BoundingBox` (alpha 無視) が default なら `Color::NONE` でも動く。
        //    その場合は本コメントを削除して `Color::NONE` に戻す。
        color: Color::srgba(0.0, 0.0, 0.0, 0.001),
        ..default()
    },
    ChartViewState {
        bounds: CHART_DRAW_SIZE,
        cell_width: 6.0,
        cell_height: 1.0,  // 後で autoscale が上書き
        ..default()
    },
    ChartInstrument { instrument_id: instrument_id.to_string() },
)).id();
```

⚠️ `ChartInstrument` は今 `WindowRoot` 側にも付いているが (window.rs:27)、**A0 で chart entity 側にもコピーする** (Phase A0 の chart_render_system 移行が per-instrument lookup を必要とするため。詳細は Phase A0 節の chart.rs:85 移行項目を参照)。Phase A の spawn skeleton (上記) は A0 でこの duplicate が済んでいる前提で書いている。`viewstate_update_system` のクエリが root を経由しないで済むようになる (Phase 7.2 の `StrategyEditorId.region_key` ジョインと同じ思想)。Root 側の `ChartInstrument` は registry 同期用にそのまま残す。

**`chart_main_render_system` (新規、旧 `chart_render_system` の draw 部分のみ):**

ViewState を read-only に取り、`ChartViewState::price_to_y` / `interval_to_x` で座標変換 (これらは上記レイアウト helper の通り main_area の境界を考慮済み)。**autoscale 計算は完全に消す** (それは `chart_viewstate_update_system` の仕事)。

```rust
// (impl ChartViewState 内、Phase A から完成形:)
pub fn interval_to_x(&self, open_time_ms: i64) -> f32 {
    let dt = (open_time_ms - self.latest_x) as f32;
    let timeframe_ms = match self.basis { Basis::Time(ms) => ms as f32 };
    (dt / timeframe_ms) * self.cell_width * self.scaling + self.translation.x
}
// price_to_y は M2 修正で main_area_y_bottom + cell_height スケール (上記レイアウト分割を参照)
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
    // PriceGutter は chart entity の子として spawn_chart_panel で予約済 (下の「レイアウト調整」節)。
    // Changed<ChartViewState> は chart entity に立つので、それを起点に対応する gutter を引く。
    chart_q: Query<(Entity, &ChartViewState, &PriceGutterRef), (With<ChartInstrument>, Changed<ChartViewState>)>,
    existing_labels: Query<(Entity, &PriceLabel)>,
) {
    for (chart_entity, state, gutter_ref) in &chart_q {
        // 既存ラベルを despawn (ラベル単位の出し入れ。
        //  ⚠️ Bevy 0.15 では `commands.entity(gutter).despawn()` は子孫を despawn しないので
        //  「親 despawn で一掃」は使えない。`despawn_recursive` を呼ぶか、本実装のように個別 despawn する)
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
            // gutter local 座標: x=0 は gutter 左端、y は price_to_y を gutter ローカルに変換
            let y_local = state.price_to_y(value);   // chart-local y。gutter は chart 子なので
                                                    // 同一 parent transform 系で OK (chart 中心原点)
            let label_text = format!("{:.*}", state.decimals, value);
            // ⚠️ commands.spawn(...) の戻り値に .set_parent(gutter_ref.0) を必ず付ける。
            //    親子化しないと window 移動 / chart transform 変化にラベルが追従しない。
            commands.spawn((
                Text2d::new(label_text),
                TextFont { font_size: TEXT_SIZE, ..default() },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
                Anchor::CenterLeft,
                Transform::from_xyz(4.0, y_local, 0.3),  // gutter local
                PriceLabel { target_chart: chart_entity },
            )).set_parent(gutter_ref.0);
        }
    }
}
```

⚠️ **必ず gutter 子として spawn する**。`commands.spawn(...)` のままだと world root に裸の Text2d が残り、window を移動した瞬間にラベルだけ画面に置き去りになる (Phase C の pan/zoom 完了時の最大の見落とし箇所)。`PriceGutterRef(pub Entity)` Component を chart entity に持たせ、spawn_chart_panel で `commands.spawn(PriceGutter { ... }).set_parent(chart_entity).id()` の戻り値を埋め込んでおく。`TimeGutterRef` も同様。Time 軸 label system も `.set_parent(time_gutter_ref.0)` を必ず付ける。

⚠️ **`PriceGutter` / `TimeGutter` / `PriceGutterRef` / `TimeGutterRef` の Component 宣言は `chart_axes.rs` (Phase B 新規ファイル) に置く**。Phase A spawn は gutter 子 entity を作らない (Phase B が gutter spawn を追加する) ので、`PriceGutterRef` の埋め込みも Phase B 着手時に `spawn_chart_panel` への追記で行う。Phase A 単独 landing 時点では chart entity に gutter ref 系 Component は付与されない (axis label が未実装なので参照側も無く整合)。「依存関係: A0 → A → B,C,D は並行可能」の **B,C,D は A 完了後**を前提とし、本宣言は Phase B PR で同時 landing する:

```rust
// src/ui/chart_axes.rs (Phase B 新規) — gutter Component 宣言:
#[derive(Component)] pub struct PriceGutter;
#[derive(Component)] pub struct TimeGutter;
#[derive(Component)] pub struct PriceGutterRef(pub Entity);
#[derive(Component)] pub struct TimeGutterRef(pub Entity);
#[derive(Component)] pub struct PriceLabel { pub target_chart: Entity }
#[derive(Component)] pub struct TimeLabel  { pub target_chart: Entity }
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

現状 `chart` entity サイズは `360 x 180`、`window.rs:53-55` で hardcode。**レイアウト定数 (`CHART_DRAW_SIZE` / `PRICE_GUTTER_WIDTH` / `TIME_GUTTER_HEIGHT` / `CHART_PANEL_SIZE`) は Phase A で既に宣言済**(前節「レイアウト定数は Phase A で一括導入」参照)、Phase B では gutter 子 entity の spawn のみ追加する。

⚠️ **算術整合は const expr で固定する** (旧版は `panel = (360, 230)` と `draw = (310, 180)` の差が gutter と合わなかった)。Phase A 初期 spawn の `Sprite.custom_size` と `ChartViewState.bounds` は既に `CHART_DRAW_SIZE` 由来 (axis label 領域を含まない)。Phase B では `spawn_chart_panel` で Y gutter / X gutter の空 child entity (`PriceGutter` / `TimeGutter` マーカー Component 付き、Transform は chart-local) を**新規に**準備し、その Entity id を chart entity 側に `PriceGutterRef(Entity)` / `TimeGutterRef(Entity)` Component として埋め込む。axis label system は前述の通り `.set_parent(gutter_ref.0)` でラベルを gutter 子として spawn する。

**gutter 子 Transform も const 由来で定義** (literal 数値を直書きしない、`CHART_DRAW_SIZE` 変更で自動追従):

```rust
// PriceGutter (chart の右隣、(50, 180) サイズ)
Transform::from_xyz(CHART_DRAW_SIZE.x / 2.0 + PRICE_GUTTER_WIDTH / 2.0, 0.0, 0.1)
// TimeGutter (chart の下、(310, 24) サイズ)
Transform::from_xyz(0.0, -CHART_DRAW_SIZE.y / 2.0 - TIME_GUTTER_HEIGHT / 2.0, 0.1)
```

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
            |mut drag: Trigger<Pointer<Drag>>,
             mut chart_q: Query<&mut ChartViewState>,
             // ⚠️ camera scale 補正は必須。floating_window.rs:107,114 と同じ理由で、bevy_pancam
             //    のズーム状態でも world-space pan 量が screen-space drag 距離と一致するように
             //    scale を掛ける。これを忘れると camera zoom 時に chart pan が pixel delta のまま
             //    で動き、ズームインで pan が「鈍く」/ ズームアウトで「速すぎる」挙動になる。
             camera_q: Query<&OrthographicProjection, With<Camera2d>>| {
                // ⚠️ Bevy 0.15: trigger.entity()  (0.16+ で target() にリネーム)
                let Ok(mut state) = chart_q.get_mut(drag.entity()) else { return };
                // ⚠️ Bevy 0.15: Trigger は inner event を deref しないので必ず .event() 経由。
                //    floating_window.rs:115 の `drag.event().delta.x` パターンと揃える。
                let delta = drag.event().delta;
                let scale = camera_q.get_single().map(|p| p.scale).unwrap_or(1.0);
                state.translation.x += delta.x * scale;
                state.translation.y -= delta.y * scale;   // Bevy Y は上が正、Pointer delta は下が正
                state.auto_scale = false;                 // pan 開始で autoscale off
                drag.propagate(false);                    // WindowRoot 側の window 移動 observer に bubble させない
            },
        );
    }
}
```

⚠️ **Bevy 0.15 では `trigger.entity()`**。`trigger.target()` は 0.16+ rename ([floating_window.rs:55-63](src/ui/floating_window.rs) の既存 observer と揃える)。

⚠️ **`drag.propagate(false)` は必須**。これを忘れると chart 内でドラッグした瞬間に WindowRoot の `Pointer<Drag>` ([floating_window.rs:104-117](src/ui/floating_window.rs)) にも event が届き、window 全体が移動する二重挙動になる。同じく `Pointer<Down>` observer (z bump) もキャンセルしたい場合は `install_chart_down_observer` を別途追加して `down.propagate(false)` する。**Phase C 着手時に bevy-engine スキルで propagation 規則を再確認**。

**Zoom (cell_width / cell_height):**

⚠️ **Bevy 0.15 には `Pointer<Scroll>` event は存在しない** (`bevy_picking 0.15.1` の `events.rs` に `Scroll` variant 無し、0.16+ で追加)。**`EventReader<MouseWheel>` + picking の `HoverMap` で hovered chart entity を引く** パターンを採用する。`HoverMap` (Bevy 0.15.1 では `bevy::picking::focus::HoverMap`、`bevy_picking-0.15.1/src/focus.rs`。0.16+ で `bevy::picking::hover` にリネーム済) は最前面の hovered entity 集合を提供するため、cursor 位置の sprite を「`camera.viewport_to_world_2d` + sprite bounds 逆引き」せずに entity だけで特定できる。これが現実的に取れる最小実装。

```rust
const ZOOM_SENSITIVITY: f32 = 30.0;
const MIN_CELL_WIDTH: f32 = 1.0;
const MAX_CELL_WIDTH: f32 = 50.0;
const MIN_CELL_HEIGHT: f32 = 0.1;
const MAX_CELL_HEIGHT: f32 = 1000.0;

fn chart_scroll_zoom_system(
    mut wheel: EventReader<bevy::input::mouse::MouseWheel>,
    // ⚠️ Bevy 0.15.1: `HoverMap` は `bevy::picking::focus` モジュール (`bevy_picking-0.15.1/src/focus.rs`)。
    //    0.16+ で `bevy::picking::hover::HoverMap` にリネームされたが、本プロジェクトは 0.15 ピン。
    hover_map: Res<bevy::picking::focus::HoverMap>,
    // PointerId と PointerLocation の対を引く (hover_map で得た id に対応する pointer を選ぶため)
    pointers: Query<(&bevy::picking::pointer::PointerId, &bevy::picking::pointer::PointerLocation)>,
    // ⚠️ Multi-camera 環境では `get_single()` が `Err(MultipleEntities)` で zoom silent-fail する。
    //    本プロジェクトは bevy_pancam の world Camera2d (`src/camera.rs`) を 1 つ持つ前提なので
    //    `With<Camera2d>` で絞り込む。複数 2D camera を導入する未来があれば pointer の
    //    `Location.target` (NormalizedRenderTarget) で正しい camera を選ぶ実装に拡張する。
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut chart_q: Query<(&GlobalTransform, &mut ChartViewState), With<ChartInstrument>>,
) {
    for ev in wheel.read() {
        // unit 正規化: Pixel は line に概算 (Bevy 0.15: MouseScrollUnit::{Line, Pixel})
        let y = match ev.unit {
            bevy::input::mouse::MouseScrollUnit::Line  => ev.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => ev.y / 20.0,
        };
        // 全 pointer の hover_map から、chart_q にマッチする (entity, pointer_id) を採用
        let Some((entity, ptr_id)) = hover_map.iter()
            .flat_map(|(ptr_id, set)| set.keys().map(move |e| (*e, *ptr_id)))
            .find(|(e, _)| chart_q.contains(*e))
        else { continue };
        // cursor を world に投影 (sprite picking backend 前提 — Caveat 22 参照)
        let Ok((cam, cam_t)) = camera_q.get_single() else { continue };
        // ⚠️ hover_map で得た ptr_id に**対応する** PointerLocation を引く。
        //    `find_map(|p| p.location.clone())` で最初の pointer を拾うと、複数 pointer / stale
        //    pointer がある環境で hover 中の chart と別 pointer の座標で zoom 中心が計算される。
        let Some(loc) = pointers.iter()
            .find_map(|(id, p)| (*id == ptr_id).then(|| p.location.clone()).flatten())
        else { continue };
        let Ok(world) = cam.viewport_to_world_2d(cam_t, loc.position) else { continue };
        let Ok((gt, mut state)) = chart_q.get_mut(entity) else { continue };
        let cursor_local = world - gt.translation().xy();
        let cursor_price = state.y_to_price(cursor_local.y);
        let cursor_time  = state.x_to_time_ms(cursor_local.x);
        let factor = 1.0 + y / ZOOM_SENSITIVITY;
        state.cell_width  = (state.cell_width  * factor).clamp(MIN_CELL_WIDTH,  MAX_CELL_WIDTH);
        state.cell_height = (state.cell_height * factor).clamp(MIN_CELL_HEIGHT, MAX_CELL_HEIGHT);
        state.auto_scale = false;
        let new_cursor_y = state.price_to_y(cursor_price);
        let new_cursor_x = state.interval_to_x(cursor_time);
        state.translation.y -= new_cursor_y - cursor_local.y;
        state.translation.x -= new_cursor_x - cursor_local.x;
    }
}
```

⚠️ **cursor 中心ズームの translation 補正は重要**。これが無いと「ズーム時に画面中央が動かない」flowsurface の挙動が出ない。flowsurface `chart.rs::Message::Scaled` ([line 400–428](.claude/skills/flowsurface/src/src/chart.rs)) の cursor delta 計算を逐行で写経する。

⚠️ **`MouseScrollUnit::Pixel` の正規化**: OS / マウス / トラックパッド で `Pixel` 単位の値が `Line` 単位より桁違いに大きく届く。上記の `ev.y / 20.0` は経験則の暫定値 (Phase C で実機 tuning)。

⚠️ **`HoverMap` から chart を引く時の親子問題**: chart Sprite は WindowRoot の子 entity。`HoverMap` は最前面 entity を返すので chart Sprite 自身がヒットする想定だが、もし root sprite の方が前面に来るレイアウトであれば picking layer の調整が必要 (chart Sprite の z を root sprite より大きく、`Pickable::default()` を明示)。

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

⚠️ **observer は `cursor_world` だけを保存し、`hovered_price` / `hovered_time_ms` を計算しない** (Caveat 28 整合)。`y_to_price`/`x_to_time_ms` は `base_price_y` / `cell_height` / `latest_x` / `cell_width` / `translation` / `scaling` に依存し、Bevy 0.15 observer は `ChartSet::Autoscale` の前後どちらで発火するか保証されないため、stale な派生量で readout 値が 1 フレーム古くなる。**派生量は Render 段の regular system で計算**:

```rust
fn install_chart_crosshair_observer(/* Added<CrosshairState>... */) {
    commands.entity(entity).observe(
        |trigger: Trigger<Pointer<Move>>,
         mut chart_q: Query<(&GlobalTransform, &mut CrosshairState)>| {
            let Ok((gt, mut crosshair)) = chart_q.get_mut(trigger.entity()) else { return };
            // ⚠️ `hit.position` の座標系は backend 依存 (HitData の doc 参照)。
            //    本プロジェクトは `bevy_sprite_picking_backend` 単独使用前提で world 座標扱いする。
            //    別 backend を有効化する場合は Caveat 22 を参照して再検証。
            //    observer は ChartSet::Autoscale 順序非依存にするため `ChartViewState` を読まない。
            let local = trigger.event().hit.position.unwrap_or(Vec3::ZERO) - gt.translation();
            crosshair.cursor_world = Some(local.xy());
            // hovered_price / hovered_time_ms は touch しない (Render system が計算)
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

// Render 段 (ChartSet::Render, .after(Autoscale)): autoscale 確定後の派生量で readout を確定
fn chart_crosshair_derive_system(
    // ⚠️ `Changed<CrosshairState>` 単独だと、cursor 静止中に autoscale で base_price_y/cell_height が
    //    動いた frame で hovered_price が stale になる。`Or<(Changed<CrosshairState>, Changed<ChartViewState>)>`
    //    で「cursor 動 or viewstate 動」どちらの起点でも再計算する。
    mut chart_q: Query<
        (&ChartViewState, &mut CrosshairState),
        Or<(Changed<CrosshairState>, Changed<ChartViewState>)>,
    >,
) {
    for (state, mut crosshair) in &mut chart_q {
        match crosshair.cursor_world {
            Some(c) => {
                let new_t = state.x_to_time_ms(c.x);
                // ⚠️ **`hovered_price` は main_area 内のみで計算**: Phase A で volume area
                //    (`y < main_area_y_bottom()`) を 20% 予約済み。volume area の y を
                //    `y_to_price` に渡すと `base_price_y` 以下の負値方向に外挿された偽の価格が
                //    badge に出る。cursor が main_area 外なら hovered_price = None。
                //    Phase E で `hovered_volume` を追加する時に対称な分岐になる。
                let new_p = if c.y >= state.main_area_y_bottom() {
                    Some(state.y_to_price(c.y))
                } else {
                    None
                };
                // DerefMut 抑制ガード (Caveat 29 と同じ理由で同値代入をスキップ)
                if crosshair.hovered_price != new_p { crosshair.hovered_price = new_p; }
                if crosshair.hovered_time_ms != Some(new_t) { crosshair.hovered_time_ms = Some(new_t); }
            }
            None => {}  // Out observer 側で既に None 化済み
        }
    }
}
```

**`chart_crosshair_render_system`:**

```rust
fn chart_crosshair_render_system(
    mut painter: ShapePainter,
    chart_q: Query<(&GlobalTransform, &ChartViewState, &CrosshairState)>,
    // ⚠️ Changed<CrosshairState> フィルタを付けてはいけない (ShapePainter は immediate-mode、
    // 「変化が無いフレーム」で描画を発行しないと cross line が消える)。
    // 描画スキップは crosshair.cursor_world.is_none() による per-entity continue で行う。
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

**レイアウト分割 (Phase A で先取りして設計に組み込む):**

⚠️ **volume サブペインの存在は Phase A の `ChartViewState` 設計時点で予約しておく**。Phase E で `cell_height` の意味を後付け変更すると Phase A の round-trip テストが崩れ、main draw / axes / interaction の全座標式に retroactive 修正が必要になる (incompatible な設計改修)。Phase A 時点で以下を確定:

```rust
// ChartViewState の impl に Phase A から含めるレイアウト helper:
pub fn main_area_height(&self) -> f32   { self.bounds.y * 0.80 }
pub fn volume_area_height(&self) -> f32 { self.bounds.y * 0.20 }
pub fn main_area_y_bottom(&self) -> f32 { -self.bounds.y / 2.0 + self.volume_area_height() }
// main area: y ∈ [main_area_y_bottom, bounds.y / 2]
// volume area: y ∈ [-bounds.y / 2, main_area_y_bottom]
```

**`cell_height` の意味は Phase A で確定**: 「main area 内の 1 price unit あたり px」(volume area の高さは含まない)。`price_to_y` / `y_to_price` は最初から `main_area_height()` を境界とし、`main_area_y_bottom` を 0 オフセットとして実装する:

```rust
pub fn price_to_y(&self, price: f32) -> f32 {
    self.main_area_y_bottom() + (price - self.base_price_y) * self.cell_height * self.scaling
        + self.translation.y
}
```

Phase A〜D の間は volume area が「空の下 20%」として確保されるだけで描画は無し。Phase E で volume bar の draw system だけが追加される。**Phase A round-trip テストはこのレイアウトのまま記述する**。これにより Phase E は純粋に additive な変更で済む。

**`volume_render_system`:**

```rust
fn volume_render_system(
    // ⚠️ ShapePainter は immediate-mode。毎フレーム filter 無しで draw する。
    // Changed<InstrumentTradingDataMap> や Changed<ChartViewState> で gate してはいけない。
    mut painter: ShapePainter,
    map: Res<InstrumentTradingDataMap>,
    chart_q: Query<(&GlobalTransform, &ChartInstrument, &ChartViewState)>,
) {
    for (gt, instrument, state) in &chart_q {
        let Some(data) = map.map.get(&instrument.instrument_id) else { continue };
        // Phase A で導入した helper を再利用 (autoscale 計算と同じスライス)
        let visible_candles = state.visible_candle_slice(&data.ohlc_points);
        let max_volume = visible_candles.iter()
            .filter_map(|c| c.volume).fold(0.0_f32, f32::max);
        if max_volume <= 0.0 { continue; }
        painter.set_translation(gt.translation());

        for candle in visible_candles {
            let Some(vol) = candle.volume else { continue };
            let x = state.interval_to_x(candle.open_time_ms);
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
- crosshair が volume サブペインに入ったときの volume readout — **Phase D の `CrosshairState` / `chart_crosshair_derive_system` を additive に拡張**する:
  1. `CrosshairState` に `pub hovered_volume: Option<f32>` を追加 (Phase D で予約していないので新規 field、Phase D の Out observer / `Default` は touch 不要 — Bevy の `#[derive(Default)]` で `None` になる)
  2. `chart_crosshair_derive_system` の query を `(&ChartViewState, &ChartInstrument, &mut CrosshairState)` に拡張し、`Res<InstrumentTradingDataMap>` を追加引数で受ける
  3. cursor が volume area (`c.y < state.main_area_y_bottom()`) のときのみ `hovered_volume = Some(...)` を計算: cursor の x を `state.x_to_time_ms(c.x)` で時刻化 → `data.ohlc_points` を時刻で二分探索 → 最近傍 candle の `volume` を返す。main area 内なら `None`
  4. DerefMut 抑制ガード (Caveat 29) を `hovered_volume` にも適用
  5. badge 描画 system は「`hovered_price` が `Some` なら price badge、`hovered_volume` が `Some` なら volume badge」を z + 0.6 で別 entity として spawn。両方同時に Some になることは無い (排他的な if-else 分岐) ので衝突しない
- Phase A で `chart_render.rs` に移管された single-candle fallback (旧 [chart.rs:226-258](src/ui/chart.rs) 相当のロジック) を完全削除 (`InstrumentTradingDataMap` から ohlc が来なければ何も描かない方が clean)
- Phase A 移管時に `chart_render.rs` / `chart_viewstate.rs` に分散された旧 chart.rs のテスト群 (旧 chart.rs:261-422 由来) のうち、`HistoryPoint` 直接参照のものを `InstrumentTradingData` 経由に書き換え、dead test を削除
- **BUY/SELL ボタンの spawn を削除** ([window.rs:60-79](src/ui/window.rs))。`spawn_button` 呼び出し 2 つ + `commands.entity(content_area).add_child(buy_button/sell_button)` を消す。`TradeButton::Buy` / `TradeButton::Sell` の Observer ([button.rs](src/ui/button.rs)) も spawn ポイントが消えれば dead code になるので、参照が他に無いことを `grep TradeButton::` で確認した上で `TradeButton` enum ごと削除。Reason: 売買入力は本フェーズではなく [Phase 9 - Live Account and Order API](docs/plan/Phase%209%20-%20Live%20Account%20and%20Order%20API.md) で**独立した「売買入力ウィンドウ」として新設される**ため、chart panel に同居させない (toy debug の遺物)。Phase 9 着手まで売買 UI が一時的に消える状態になることを Phase E の PR 説明に明記

## 触るファイル一覧

**新規 (6 ファイル):**
- `src/ui/chart_viewstate.rs` (Phase A) — `ChartViewState` Component + `chart_viewstate_update_system` + 座標変換ヘルパ
- `src/ui/chart_render.rs` (Phase A) — `chart_main_render_system` (純 draw)
- `src/ui/chart_axes.rs` (Phase B) — `calc_optimal_price_ticks` / `calc_optimal_time_step` + 2 system + `PriceLabel`/`TimeLabel` Component + `PriceGutter`/`TimeGutter` マーカー + `PriceGutterRef(Entity)`/`TimeGutterRef(Entity)` (chart entity に埋める gutter 参照)
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
  - `TradingData` 構造体は A0 同一 PR で**削除** (全 reader 移行完了が landing 条件、legacy 並走は不可)
  - `price_simulation_system` (synthetic) を A0 同一 PR で**削除**
- `src/ui/window.rs`:
  - (Phase A) `spawn_chart_panel` で chart entity に Sprite (alpha≈0.001, custom_size) を付与
  - (Phase A0) `instrument_chart_sync_system` に `map.remove()` の 3 行を追加 (lifecycle 整合、`InstrumentTradingDataMap` 新設と同 PR)
  - (Phase A0) `ChartInstrument` を chart entity 側にも duplicate (chart_render_system per-instrument lookup 用)
- `src/ui/floating_window.rs` (Phase A0) — close (×) observer ([floating_window.rs:239–246](src/ui/floating_window.rs)) に `mut map: ResMut<InstrumentTradingDataMap>` 引数を追加し、`registry.remove(&ci.instrument_id)` の直後に `map.map.remove(&ci.instrument_id);` を呼ぶ。close 経路では sync system が走る前に entity が despawn されるため、close observer 自身で map cleanup しないと entry が leak する
  - (Phase E) `spawn_chart_panel` から BUY/SELL ボタン spawn ([window.rs:60-79](src/ui/window.rs)) を**削除** — 売買 UI は Phase 9 で独立ウィンドウとして新設
- `src/ui/button.rs` (Phase E、A0 前倒し推奨) — `TradeButton::Buy` / `TradeButton::Sell` click observer を削除 (参照が他に無いことを事前 grep 確認)
- `src/ui/components.rs` (TradeButton 削除と同 PR) — `TradeButton` enum を削除
- `src/ui/systems.rs` (TradeButton 削除と同 PR) — `button_system` 全体を削除 (`TradeButton::Buy/Sell` の color/log dispatch のみが本体、TradeButton が消えれば dead code)。`update_price_display` の `Res<TradingData>` 依存もここで `Res<LastPrices> + Res<SelectedSymbol>` に書き換え
- `src/ui/mod.rs` (TradeButton 削除と同 PR) — `use crate::ui::systems::{button_system, ...}` から `button_system` を除去、`add_systems(Update, (..., button_system, ...))` ([mod.rs:156](src/ui/mod.rs)) のタプル要素も除去 (A0 で 20-tuple 上限を超えないための玉突き整理も同タイミングで行う)
- `src/main.rs` (Phase A0) — `use trading::{..., price_simulation_system, ...}` ([main.rs:9](src/main.rs)) から `price_simulation_system` を除去 (system 本体削除に伴う dead import 解消)
- `src/ui/sidebar.rs` — **A0 で変更不要** (`update_instrument_price_text_system` は既に `LastPrices` のみ)
- `src/ui/footer.rs` / `src/ui/menu_bar.rs` / `src/ui/replay_startup_window.rs` (Phase A0) — `Res<TradingData>` 経由の session-global フィールド読みを `Res<TradingSession>` に
- `src/ui/mod.rs` (Phase A 以降):
  - 6 モジュール宣言追加
  - 旧 `chart_render_system` を削除
  - 新規 7 system 登録 (Phase A〜E)、20-tuple 上限 (現状 mod.rs の Update タプルは 18-19 個、Phase A〜E で 6+ system 追加するため境界付近に到達)を超えそうなら `SystemSet` か別 `add_systems` 呼び出しに分割
  - ※ TradeButton 削除に伴う `button_system` の import / `add_systems` 除去は上記「TradeButton 削除と同 PR」の項に含まれる
- `src/ui/components.rs` (Phase B 以降) — axis label / crosshair badge 用の色定数 (`AXIS_LABEL_FG`, `CROSSHAIR_LINE`, `CROSSHAIR_BADGE_BG`, `VOLUME_BULL_BAR`, `VOLUME_BEAR_BAR`)

**削除:**
- `src/ui/chart.rs` (Phase A で `chart_viewstate.rs` / `chart_render.rs` に分割移管後に削除)

**unchanged だが確認のみ:**
- `src/ui/floating_window.rs:54-150` — Pointer observer の propagation 規則 (Phase C で chart drag が干渉しないか目視確認)
- `src/ui/layout_persistence.rs` — `ChartInstrument` 付き root は既に `LayoutExcluded` 経由で layout JSON から除外されている ([window.rs:30](src/ui/window.rs))、現状維持

## 再利用する既存ピース

- `spawn_floating_window` ([src/ui/floating_window.rs](src/ui/floating_window.rs)) — chart panel の枠はそのまま
- `BackendTradingState.last_prices` + `LastPrices.map` ([trading.rs:63, 579-580](src/trading.rs)) — per-instrument map の命名/構造の precedent。**新パターンを発明せず、これに揃える**
- `bevy_vector_shapes::ShapePainter` — Phase A/D/E すべてで draw bus として使う
- `chrono::DateTime` ([trading.rs:2](src/trading.rs)) — X 軸 time label の文字列フォーマットに使用
- `Pointer<Drag>` / `Pointer<Move>` / `Pointer<Out>` observer パターン ([src/ui/floating_window.rs:54-150](src/ui/floating_window.rs)) — `trigger.entity()` (Bevy 0.15) で揃える
- `instrument_chart_sync_system` ([window.rs:83](src/ui/window.rs)) — registry 同期、Phase A0 で 3 行追加のみ
- `Text2d` + `Anchor` — axis label / crosshair badge の retained text として既存 codebase 通り

## Caveat 一覧 (本タスクで踏みうるもの)

1. **chart entity は今 Sprite 無し** — `(Transform, ChartViewState)` のみ。Phase C の `Pointer<Drag>` を機能させるために Phase A で `Sprite { custom_size, color: ~alpha 0.001 }` を必ず付ける。**`Color::NONE` (alpha=0) は sprite picking backend の AlphaThreshold mode で除外される可能性がある** — 既存 codebase の picking 対象 Sprite が alpha 0.05〜0.85 のみで alpha=0 の前例が無いため、安全側に倒すならごく微小な alpha (0.001) で実質透明にする。ShapePainter で描いた背景は picking 対象外
2. **WindowRoot の Pointer<Down>/<Drag> と chart の Pointer<Drag> の競合** — chart Sprite を root の子として nest し、Phase C 着手時に `Pointer<Drag>::propagate(false)` または observer ガードで分離。設計確認は bevy-engine スキル発動で行う
3. **Bevy 0.15 は `trigger.entity()`** — `trigger.target()` は 0.16+ rename。[`floating_window.rs:55-63`](src/ui/floating_window.rs) 既存パターンと揃える
4. **`TradingData` の 2-resource 分割は mutator 1 箇所だけが本質** — `backend_update_system` ([trading.rs:279](src/trading.rs)) のみが per-instrument 化を必要とする。他 reader は単純な lookup 置換。Phase A0 のコアはこの 1 system + proto + nautilus aggregation
5. **proto3 `optional float volume`** — bare `float` だと 0.0 が "no data" と区別不能で Phase E volume サブペインが偽の zero-bar を描く。**explicit-presence で必ず定義**
6. **`OhlcPoint` は 2 箇所に存在** — `python/proto/engine.proto` (生成 Rust `engine::OhlcPoint`) と `src/trading.rs:21-29` (serde 付き手書き)。volume 追加時は両方触る + `backend_update_system` の conversion も
7. **`InstrumentTradingDataMap` の entry cleanup** — `instrument_chart_sync_system` で `map.remove()` を 3 行追加しないと、registry 退会後も entry が残る。replay/live 切替で銘柄入れ替わる用途では掃除する
8. **`ChartInstrument` は WindowRoot と chart entity の両方に持たせる** — root: registry 同期用 (既存), chart entity: A0 chart_render_system の per-instrument lookup と Phase A 以降の `viewstate_update_system` クエリ簡略化のため **A0 で追加コピー** (旧版は Phase A 着手と書いていたが、A0 で chart_render_system の data source 差し替えに必要なので前倒し)
9. **autoscale 計算は `chart_viewstate_update_system` に集約、`chart_main_render_system` は純 draw** — Phase A で旧 `chart_render_system` の autoscale ロジック ([chart.rs:113-162](src/ui/chart.rs)) を移管。draw 系で `mut ChartViewState` を取らない
10. **`ChartViewState` は `min_price`/`max_price` を持たず `base_price_y` + `cell_height` 表現に切替** — flowsurface 流のズーム中心固定 (`cursor_price = state.y_to_price(cursor_y); ... ; new_cursor_y = state.price_to_y(cursor_price); state.translation.y -= (new_cursor_y - cursor_y)`、[chart.rs:453-461](.claude/skills/flowsurface/src/src/chart.rs)) を可能にするため
11. **iced `canvas::Cache` retained-mode は Bevy 翻訳でスケジューラ層に降ろす** — Cache 4 層は 4 system に対応し、各 system が `Changed<...>` で early-out する。bevy_prototype_lyon 等の retained shape は導入しない (codebase に既存 user 無し)
12. **`Pointer<Move>` の `trigger.event().hit.position`** — world space pos が来る。chart 座標 (local) は `gt.translation()` を引いて算出。Y 軸の Bevy 流儀 (上が正) と Pointer delta (下が正) の符号反転に注意 (Phase C drag)
13. **`Changed<ChartViewState>` フィルタの per-entity 性** — 各 chart entity ごとに独立して立つ。複数 instrument の chart が同時表示されていても、片方の pan で他方の axis label が再生成されることはない
14. **`Res::is_changed()` は粒度が粗い** — `InstrumentTradingDataMap` は単一 Resource なので map 全体が変更時に立つ。entity 側の `last_seen_ohlc_signature` local cache (末尾 bar の `(len, open_time, high, low, close, volume)` を u64 へ畳む、`compute_ohlc_signature` 参照) で per-instrument early-out する
15. **`add_systems` タプル 20 上限** — Phase A〜E で 7+ system 追加、既存 `mod.rs` のタプル数次第で `SystemSet` 分割か別 `add_systems` 呼び出しに (Phase 7.2 と同じ罠)
16. **gutter (axis label) と CrosshairBadge の重ね順** — Phase B axis label は z + 0.3、Phase D CrosshairBadge は z + 0.6 (cross line は + 0.5)。逆だと crosshair の値強調が固定ラベルに隠れる
17. **volume None の skip** — Phase E volume system は `candle.volume.is_none()` の bar を描かない。proto3 `optional float` を活かす唯一のポイント
18. **time axis の timezone** — flowsurface は `data::UserTimezone` をユーザ設定として持つ。本プランは初版 UTC 固定 (chrono の `DateTime<Utc>` で format)、ユーザ要望が出てから JST 等を追加
19. **`TradingData` は A0 同一 PR で削除** — legacy alias を残して並走しない (per-instrument 経路と single resource 経路の二重 write を避けるため)。A0 PR の landing 条件に `grep 'Res<TradingData>'` が 0 件を含める
20. **flowsurface `ViewState` の `tick_size` / `decimals`** — `tick_size` は最小価格刻み、`decimals` は表示桁。Phase A では `InstrumentTradingData` または `BackendTradingState` の symbol meta から拾う設計 (まずは hardcode `0.01` / `2` decimals、後で銘柄ごとに正しく拾うのは別タスク)
21. **Phase A 着手前に bevy-engine スキル + flowsurface スキル発動必須** — Bevy 0.15 罠 (`add_systems` 20 上限、observer の import path、Anchor 左寄せ、`trigger.entity()`) と flowsurface の `canvas::Program::draw` / `update` 経路 を navigator が読む
22. **`Pointer<Scroll>` は Bevy 0.15 に存在しない** — `bevy_picking 0.15.1` の `events.rs` に `Scroll` variant 無し (0.16+ で追加)。zoom は `EventReader<MouseWheel>` + `HoverMap` で chart entity を引き、`Camera::viewport_to_world_2d` で cursor を投影する pattern にする。これは Phase C compile blocker
23. **`Trigger<Pointer<...>>` は inner event を Deref しない** — `drag.delta` / `drag.hit` は compile error。必ず `drag.event().delta` / `drag.event().hit.position` 経由 (`floating_window.rs:115` 既存パターンと揃える)。`propagate(false)` のみ `Trigger` 直下メソッド
24. **`HitData.position` の座標系は backend 依存** — `bevy_sprite_picking_backend` 単独使用前提で world 扱いしている。他 backend (ui_picking 等) を有効化すると screen space になり crosshair / drag が壊れる。Cargo.toml で picking features を変更したら Phase C/D の `local = hit.position - gt.translation()` 式を再検証
25. **autoscale self-`Changed` ループは `Event` で分離** — `chart_data_tick_system` (writer / map.is_changed gate) + `chart_interaction_tick_system` (Changed<...> reader) + `chart_autoscale_apply_system` (event consumer) の 3 段。`Mut` で書く system は `Changed<ChartViewState>` を読まないこと、reader 側は `&mut` を取らないことを徹底
26. **`commands.entity(parent).despawn()` は子孫を despawn しない (Bevy 0.15)** — 親 despawn 一掃で子 Text2d ラベルを掃除できない。axis label / crosshair badge は個別 despawn または `despawn_recursive` を明示使用
27. **`ChartSet` enum で system order を固定する** — Bevy 0.15 ambient parallelism で `chart_main_render_system` が `chart_autoscale_apply_system` の前に走ると stale `base_price_y` を描画する。`configure_sets(Update, ...)` で `Render.after(Autoscale)` を宣言
28. **Bevy 0.15 observer は `configure_sets` の対象外** — `Pointer<Drag>` / `Pointer<Move>` 観測子は `Update` schedule の外で event-driven 発火。`ChartSet::Interaction` に含めない。observer ロジックは autoscale 結果 (`base_price_y` 等) を読まない設計で安全側に倒す
29. **autoscale 収束の DerefMut-ガード必須** — `chart_autoscale_apply_system` で `if (state.field - new).abs() > EPSILON { state.field = new; }` を全 mut field に適用しないと、同値代入で `Changed` が立ち interaction_tick が再発火 → 無限 loop。Phase A landing 条件として「spawn 後 5 frame で Changed イベント 0」テストを置く
30. **`ChartViewState` は手書き `Default` impl** — `#[derive(Default)]` だと `auto_scale: false` で spawn し chart が flat 表示になる。`auto_scale: true`、`last_seen_ohlc_signature: u64::MAX` (初回 data tick で必ず差分が立つ sentinel)、`bounds: CHART_DRAW_SIZE` を明示
31. **multi-camera 環境で `camera_q.get_single()` は silent-fail** — bevy_pancam の world Camera2d 1 個前提でも UI camera が追加されると `Err(MultipleEntities)`。`With<Camera2d>` filter を必ず付ける (`src/camera.rs` 既存の Camera2d 1 個前提)。複数 2D camera 導入時は `PointerLocation.target` で選び直し
32. **`Added<T>` は `Changed<T>` を含む** — Bevy invariant。`Or<(Added, Changed)>` は冗長で `Changed` 単独で spawn フレームも拾える
33. **`CHART_Y_OFFSET` の符号は `spawn_floating_window` を Read で検証してから確定** — WindowRoot origin が panel 中心か上端か、title bar Y、panel size に title bar が含まれるかで符号 / 量が変わる。Phase A の最初の差分にこの確認を入れる

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
5. **flat / map 一致テスト**: proto には「どの symbol が flat に乗っているか」を示すフィールドが**無い**ため、Python aggregator 側のテストで「flat に書いた symbol」を test fixture が直接知っている前提で、`flat.close == per_instrument[that_symbol].close` 等を assert する。あるいは弱契約として「`per_instrument` の中にちょうど 1 個 flat と全フィールド一致する entry が存在する」を assert。divergence は selected symbol の chart silent 回帰になるので必須
6. **`InstrumentTradingDataMap` lifecycle test**: `instrument_chart_sync_system` が registry から消えた id について `map.remove()` を呼ぶこと、map.len() が desired set 以下にしか増えないことを `cargo test` で確認
7. **`TradingData` 全消去 grep**: `rg -n '\bTradingData\b|ResMut<\s*TradingData\s*>|Res<\s*TradingData\s*>' src/` の出力が 0 行であることを landing 条件にする。**単語境界 `\b` 必須** (素朴な `TradingData` だと `InstrumentTradingData` / `InstrumentTradingDataMap` / `BackendTradingState` の substring にもヒットして永久に 0 行にならない)。旧 `Res<TradingData>` のみだと `ResMut<TradingData>` や型直接参照 = `button.rs:23` / `main.rs:133,151` を漏らす

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
- Sidebar の ticker 一覧が `LastPrices.map` 経由で last_price を表示する (既存挙動の維持、A0 では fallback path のみ `InstrumentTradingDataMap.close` 経由に差し替え)
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
