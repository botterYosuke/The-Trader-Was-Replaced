---
name: bevy-engine
description: |
  The-Trader-Was-Replaced プロジェクトで Bevy（ゲーム/UI フレームワーク）を使う際の必読スキル。
  特に `src/camera.rs` と `src/ui/**` の floating-window 系（buying_power / positions / orders /
  run_result / strategy_editor / chart / sidebar / footer / menu_bar）、bevy_egui との併用、
  bevy_pancam / bevy_cosmic_edit / bevy_vector_shapes 連携、Pointer 観測子（observe）による
  ドラッグ＆Z オーダー、Sprite + Transform による world-space ウィンドウ、Text2d 三分割
  （Text2d/TextFont/TextColor）を扱う。

  ALWAYS use this skill when:
  ① ユーザが `src/ui/*.rs` や `src/camera.rs` を編集しようとしている
  ② "Bevy", "bevy_egui", "PanCam", "Camera2d", "Sprite", "Text2d", "Plugin", "ECS",
    "Resource", "Component", "Event", "System", "Query", "Commands", "World",
    "observer", "Trigger", "Pointer<Drag>", "Pointer<Down>", "floating window",
    "パネル", "サイドバー", "フッター", "メニューバー" と言われたとき
  ③ Bevy のバージョン差（0.15 と 0.19/0.16/0.17/0.18 の API 差）でハマっているとき
  ④ "Bundle is deprecated", "set_parent", "Parent", "ChildOf", "get_single", "single",
    "Trigger::entity", "Trigger::target", "required components" など破壊的変更語彙が出たとき
  ⑤ 新しいパネル（floating window）を増やす／既存パネルを書き換える作業
  ⑥ bevy_egui::Window と Bevy world-space ウィンドウ（Sprite ベース）の選択で迷ったとき
  ⑦ `pair-relay` / `pair-nav` で `src/ui/**` を触る作業に入るとき（**Orchestrator 自身が
    このスキルを invoke して内容を把握してから** Navigator を spawn すること。
    読まずに進めると Bevy 0.15 固有の罠
    — `add_systems` タプル 20 上限、`app.add_observer()` vs `app.observe()`、
    `IntoObserverSystem` の import path、`CosmicEditor` 経由の `with_buffer_mut`、
    Text2d Anchor の左寄せ — で必ずハマる）
  ⑧ `cosmic_edit`, `CosmicEditBuffer`, `CosmicEditor`, `TextEdit2d`, `FocusedWidget`,
    `CosmicBackgroundColor`, `CursorColor`, `CosmicTextChanged`, `set_text`, `with_buffer_mut`
    という語彙が出てきたとき、または「focused で文字が小さい」「unfocused で文字が大きい」
    「フォントサイズが 2 種類」「DPI スケールが反映されない」「set_initial_scale」
    「Added<CosmicEditBuffer>」と言われたとき
  ⑨ フォント / グリフ / 文字化け関連: "font", "TextFont", "Handle<Font>", "AssetServer",
    "豆腐", "mojibake", "□", "▶", "■", "glyph", "BEVY_ASSET_ROOT", "Path not found",
    "assets/fonts/", "FiraMono", "NotoSansSymbols" が出たとき。
    Bevy 0.15 同梱の FiraMono-subset は **Basic Latin のみ**で Geometric Shapes
    (U+25A0–U+25FF: ▶ ■ ▷ □ 等) を**含まない**ため、これらを Text に使うと豆腐になる。
    また direct exe 起動時 (target/debug/backcast.exe) は AssetServer が exe parent を
    見るので `BEVY_ASSET_ROOT` で repo root を指定しないと assets/ が読めない。
  ⑩ アイドル CPU / 描画レート / 起動の重さ関連: "CPU が高い", "アイドルなのに重い",
    "起動しただけで CPU/メモリ", "ファンが回る", "バッテリが減る", "FPS", "frame_time",
    "vsync", "WinitSettings", "UpdateMode", "desktop_app", "reactive", "Continuous",
    "RequestRedraw", "EventLoopProxy", "WakeUp" と言われたとき。
    Bevy 0.15 の DefaultPlugins は `WinitSettings::game()` 相当 (focused=Continuous) なので
    **アイドル時もモニタリフレッシュ上限まで描き続ける**。trading dashboard では 90Hz モニタで
    1 コア 60% 食ってた (2026-05-18 計測)。`WinitSettings::reactive(200ms)` 化で 4.7% まで落ちる。
    ただし `WinitSettings::desktop_app()` (5s/60s) は **mpsc backend push が最大 5 秒遅延** する
    ので trading UI では使わない。詳細: `references/winit-update-mode.md`。

  本プロジェクトは **Bevy 0.15** をピン留めしている（`Cargo.toml` の `bevy = "0.15"`）。
  一方 `.claude/skills/bevy-engine/src/` にミラーされている upstream は **0.19.0-dev** で、
  ECS API が一部破壊的に変わっている。両者の差は `references/0.15-vs-0.19.md` に集約してある。
  ground truth として src/ を引くときは必ずバージョン差を意識すること。
---

# Bevy Engine — The-Trader-Was-Replaced

このスキルは **Bevy 0.15** を前提にした、本プロジェクトの UI レイヤー（`src/ui/**` と
`src/camera.rs`）の流儀を伝えるためのものです。汎用 Bevy の知識はミラーされた upstream
ソース（`.claude/skills/bevy-engine/src/`）と `references/` 配下にまとめてあります。

## まず読む順序

1. このファイル — プロジェクトの 5 つの規約
2. 編集対象に応じて `references/` の該当ファイル
3. それでも不明なら `.claude/skills/bevy-engine/src/examples/<category>/*.rs` を引く
   （**ただしバージョンは 0.19-dev なので注意**）

## このプロジェクトの Bevy スタック

```
[main.rs]
  DefaultPlugins        — Bevy 本体（window / input / rendering / time / ...）
  PanCamPlugin          — bevy_pancam 0.16: 右/中クリックドラッグでパン、スクロールでズーム
  UiPlugin              — src/ui/mod.rs: パネル系すべての本体
    ├ Shape2dPlugin     — bevy_vector_shapes 0.9: チャートの線・矩形描画
    ├ CosmicEditPlugin  — bevy_cosmic_edit 0.26: Strategy Editor のテキスト編集
    │  ※ bevy_egui は 2026-05-15 Phase 7 Sub-step 1.8d で完全撤去済み
    └ CosmicEditPlugin  — bevy_cosmic_edit 0.26: ストラテジエディタのテキスト編集
  GridPlugin            — src/grid.rs: 背景グリッド
```

**重要な意思決定**: UI は **2 流派** が共存している。
- **bevy_egui (immediate-mode)**: メニューバー、ストラテジエディタ。フレーム毎に再構築。
  状態はリソースに置く。フォーカスや入力ハンドリングが楽。
- **Bevy world-space sprite window (retained-mode)**: floating panels (buying_power /
  positions / orders / run_result)。Sprite + Transform + observe(Pointer<Drag>) で自前実装。
  カメラのパン/ズームに追随する。Z オーダーを `WindowManager::max_z` で管理。

どちらを使うかは [references/egui-vs-worldspace.md](references/egui-vs-worldspace.md) を読んで判断する。
**迷ったら既存パネルと同じ流派を踏襲する**（混在の見た目を増やさない）。

## 5 つの規約 — これだけは守る

### 規約 1: floating panel を増やすときは `spawn_floating_window` を使う

新しい world-space パネルは `src/ui/floating_window.rs` の `spawn_floating_window(commands, FloatingWindowSpec { title, size, position, accent })` を経由する。これが返す `(root, content_area)` の `content_area` に子として中身を貼る。

**why**: タイトルバー drag、Z 順管理（`observe(Pointer<Down>)` で `max_z` を上げる）、rim light、inner glow、title text の生成が一箇所に集約されており、見た目の一貫性とドラッグ挙動の一貫性がここでしか維持できない。各パネルで個別に Sprite を組むと PanCam のスケールに対する drag delta 補正（`drag.event().delta * scale`）を必ず忘れる。

**手順**:
1. `src/ui/<panel_name>.rs` を作る（`buying_power.rs` を参考に）
2. `PanelKind::<Name>` を `src/ui/components.rs` に追加（`label()` も）
3. `spawn_<panel_name>_panel(commands)` 関数で `spawn_floating_window` を呼ぶ
4. `commands.entity(root).insert(PanelKind::<Name>)` を必ずやる（dispatcher の重複防止用）
5. `src/ui/floating_window.rs` の `panel_spawn_dispatcher_system` の `match` に arm 追加
6. `src/ui/mod.rs` で update system を `add_systems(Update, ...)` に追加

### 規約 2: 値表示は marker component で識別、テキストは差分書き込み

更新したい Text2d ノードには列挙型 marker component を貼り、update system は
`Query<(&Marker, &mut Text2d, &mut TextColor)>` で回す。値が変わらなければ書き込まない
（`if text.0 != value_str { text.0 = value_str; }`）。

**why**: Bevy の change detection は `DerefMut` の発火で判定する。`mut text` を取った
だけでは変わらないが、`text.0 = ...` を毎フレーム書くと（同じ値でも）change として
扱われ、下流の system（描画 extract など）が無駄に走る。差分書き込みの徹底でフレーム
コストを抑える。`buying_power.rs:126-131` が手本。

### 規約 3: Color は `srgb` / `srgba` を使う、`rgb` は使わない

Bevy 0.15 で `Color::rgb` 系は廃止された。代わりに `Color::srgb(r, g, b)` または
`Color::srgba(r, g, b, a)`。値域は `0.0..=1.0`。定数 `Color::WHITE` などは引き続き使える。

**why**: 0.15 で「色空間を明示する」方針に変わり、sRGB と linear が型レベルで区別される
ようになった。`rgb` は曖昧なので削除。

### 規約 4: Component spawn は **タプル** で、`Bundle` は新規作成しない

```rust
commands.spawn((
    Sprite { color, custom_size: Some(size), ..default() },
    Transform::from_xyz(x, y, z),
    MyMarker,
));
```

`SpriteBundle` / `Text2dBundle` 等の旧 Bundle 構造体は 0.15 から非推奨化が進んでいる
（`Camera2dBundle` → `Camera2d` 単体、`Text2dBundle` → `Text2d` / `TextFont` / `TextColor`
の 3 分割）。新規コードでは **必ずタプル spawn** にする。

**⚠️ spawn タプルの `Bundle` 実装は 15 要素まで**（`add_systems` の 20 上限とは別物）。
既存の大きい spawn（例: `strategy_editor.rs` の editor entity は 13 要素）に component を
足して 16 要素以上になると `the trait Bundle is not implemented for (...)` でコンパイル不可。
**解決**: 追加分をサブタプルにネストすると 1 要素として数えられる（13 + `(A, B, C)` = 14）。
挙動は同一。`commands.spawn((Existing, ..., (NewA, NewB, NewC), More))` の形にする。

### 規約 5: Drag ハンドラでは PanCam のズーム scale を必ず掛ける

タイトルバー等で `Trigger<Pointer<Drag>>` を扱うとき、`drag.event().delta` は
**画面ピクセル** だが、world-space オブジェクトを動かすには **world 単位** に
直す必要がある。PanCam がズームしている分だけ scale を掛ける:

```rust
let scale = camera_query.get_single().map(|p| p.scale).unwrap_or(1.0);
transform.translation.x += drag.event().delta.x * scale;
transform.translation.y -= drag.event().delta.y * scale; // y は反転
```

**why**: PanCam でズームアウトしているとカーソル移動 1px がワールド N 単位に相当する。
scale を掛けないとズーム時にウィンドウが指から「逃げる」または「先回りする」。
`floating_window.rs:96-105` の挙動を踏襲。

⚠️ **`Pointer<Drag>` は全マウスボタンで発火する**（`bevy_picking-0.15.1/src/events.rs` の
`pointer_events` が `for button in PointerButton::iter()`）。**左ドラッグだけに反応させたい
world-space ハンドラ（chart pan 等）は冒頭で `if drag.event().button != PointerButton::Primary
{ return; }` を必ず入れる**。これを忘れると、PanCam の grab_buttons（右/中）でキャンバスを
パンするつもりの右/中ドラッグが同じ entity の `Pointer<Drag>` observer も発火させ、
**カメラと panel が同時に動く二重挙動**＋（chart なら）`auto_scale=false` の副作用が黙って起きる
（`pancam_suppression_over_editor_system` が右/中ドラッグ中は PanCam を強制 enable する設計と衝突する）。
`chart_interaction.rs::install_chart_drag_observer` が実例。`PointerButton` は `bevy::prelude` 経由で引ける。

⚠️ **`Pointer<Click>` は drag 完了 (pointer up) 後にも発火する**（`bevy_picking-0.15.1` の
`pointer_events`：down→up が同 entity なら drag の有無に関わらず Click を送る）。**ドラッグ可能な
entity に double-click 検出（ダブルクリックで reset 等）を載せると、pan ドラッグ 2 連発が
double-click と誤検出される**。対策: drag observer 側で「この press はドラッグ」フラグ
（`Resource` の `HashSet<Entity>` か Component）を立て、Click observer はそのフラグが立った
click を「genuine click ではない」として double-click 列から除外する（フラグを消して early-return、
直前の last_click も捨てる）。フラグの掃除は `RemovedComponents<Marker>` 駆動の cleanup system で
despawn 時に entity key を除く（entity key leak 防止）。`chart_interaction.rs::ChartClickState` +
`install_chart_autoscale_reset_observer` + `chart_click_state_cleanup_system` が実例。flowsurface は
この罠を避けるため double-click を chart 本体ではなく**軸 gutter**（drag が起きない領域）に置いている。

## ECS ミニリファレンス（0.15 ピン）

```rust
// Component
#[derive(Component)] struct Foo;
#[derive(Component, Clone, Copy)] enum Bar { A, B }

// Resource
#[derive(Resource, Default)] struct Config { pub max: f32 }
// 登録: app.init_resource::<Config>() か insert_resource(Config { ... })
// 参照: fn sys(c: Res<Config>, mut cm: ResMut<Config>) { ... }

// Event
#[derive(Event, Debug, Clone)] struct MyEvent { pub n: i32 }
// 登録: app.add_event::<MyEvent>()
// 送信: mut w: EventWriter<MyEvent> ... w.send(MyEvent { n: 1 });
// 受信: mut r: EventReader<MyEvent> ... for ev in r.read() { ... }

// System / Query
fn move_things(time: Res<Time>, mut q: Query<&mut Transform, With<Player>>) {
    for mut t in &mut q { t.translation.x += time.delta_seconds(); }
}

// Plugin
pub struct MyPlugin;
impl Plugin for MyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Config>()
           .add_event::<MyEvent>()
           .add_systems(Startup, setup)
           .add_systems(Update, (sys_a, sys_b));
    }
}

// Hierarchy (0.15)
let parent = commands.spawn(Transform::default()).id();
let child = commands.spawn(Transform::default()).id();
commands.entity(parent).add_child(child);
// or: commands.entity(child).set_parent(parent);
// 取得: fn (parent_q: Query<&Parent>) { parent_q.get(child)?.get() }

// Observer (entity-local)
.observe(|trigger: Trigger<Pointer<Down>>, mut q: Query<&mut Transform>| {
    if let Ok(mut t) = q.get_mut(trigger.entity()) { /* ... */ }
})
```

## 0.15 vs 0.19 の差で踏むトラップ

ミラー `.claude/skills/bevy-engine/src/` は **0.19.0-dev**。ground truth として引く
ときは下記が **このプロジェクトでは使えない** ことを意識する:

| 0.19-dev (ミラー) | 0.15 (プロジェクト) | 備考 |
|---|---|---|
| `ChildOf` component | `Parent` component | hierarchy の参照 |
| `query.single()` | `query.get_single()` | `Result` を返す |
| `Trigger::target()` | `Trigger::entity()` | observer の対象 entity |
| Required components が前提 | tuple spawn でフル指定 | `Sprite` だけで足りる場合あり |
| `time.delta_secs()` | `time.delta_seconds()` | 関数名変更 |
| `OrthographicProjection` 直接 | 同じ（経由が違う） | 0.19 は `Projection` enum 経由 |
| `app.observe(system)` | `app.add_observer(system)` | **App グローバル observer の登録。`app.observe()` は 0.15 に存在しない。エンティティローカルは `.observe(...)` のまま** |

詳細は [references/0.15-vs-0.19.md](references/0.15-vs-0.19.md) を参照。

**判断ルール**: 「ミラーで見た書き方が `cargo check` で落ちる」「`E0XXX` でフィールド
が無い」と言われたら、ほぼ確実に 0.15/0.19 差。0.15 の書き方を `references/0.15-vs-0.19.md`
で確認するか、既存の `src/ui/*.rs` を grep して同じパターンを探す。

## bevy_egui のクイックリファレンス

```rust
use bevy_egui::{EguiContexts, egui};

fn my_window_system(mut contexts: EguiContexts, mut state: ResMut<MyState>) {
    egui::Window::new("Title")
        .default_width(800.0)
        .show(contexts.ctx_mut(), |ui| {
            ui.label(&state.message);
            if ui.button("Click").clicked() { state.count += 1; }
            ui.add(egui::TextEdit::multiline(&mut state.buffer)
                .desired_width(f32::INFINITY));
        });
}
```

**注意**: `ResMut<T>` を毎フレーム触ると Bevy の change detection が常に発火する。
egui の中で `state.buffer` のような大きい String を編集するときは、
`let mut local = state.buffer.clone();` → edit → 変化したときだけ書き戻す
パターンにする（`strategy_editor.rs:73-80` 参照）。

詳しくは [references/egui-integration.md](references/egui-integration.md).

## トラブルシュート

- **"could not access system parameter Res<'_, T>" で test が panic**:
  既存 system のシグネチャに `Res<T>` / `ResMut<T>` を追加した直後、その system を `app.update()` で踏む既存テスト全てが panic する典型。本番側 (`main.rs` / `UiPlugin::build`) は `init_resource` 済みでも、test 側の `App` builder は手動 init していないので追従漏れする。同 system を踏むテスト全部に `app.init_resource::<T>();` を追加すること。**最初の panic ログには 1 つの Res 名しか出ないが、その Res を init した後にも別の Res で同じ panic が出る**（Phase 7.6 では `ReplayStartupProgress` → `TradingData` → `LastRunResult` → `Time<Real>` → `ScenarioStartupParams` と 5 段階で追従漏れが連続発覚）。signature 拡張時は本番側で App build に並んでいる全 Res を確認してから 1 ターンでまとめて init するのが安全。
- **"the trait `Bundle` is not implemented for `(Sprite, Transform, ...)`"**:
  ほぼ全部 `Component` で derive されているはずなので、構成要素のどれかが Component
  でない（または import 漏れ）。`use bevy::prelude::*;` を確認。
- **"no method named `single` found"**: 0.15 は `get_single()`。
- **"no field `entity` on `Trigger`"**: 0.19 流。0.15 は `trigger.entity()` (method)。
- **"unresolved import `bevy::ChildOf`"**: 0.19 流。0.15 は `Parent`。
- **ウィンドウがドラッグでカーソルから逃げる**: 規約 5 の scale 倍を忘れている。
- **透明 Sprite を hit-target にしたいが picking が効くか不安（`Color::NONE` / alpha≈0）**: 0.15 は
  **bounds ベース picking で alpha を見ない**（`bevy_sprite-0.15.1/src/picking_backend.rs` 冒頭
  "Picking is done based on sprite bounds, not visible pixels"）。よって `Color::srgba(_,_,_,0.0)` でも
  `custom_size` さえあれば pickable。alpha threshold は 0.16+ の話なので 0.15 で alpha を 0.001 等に
  盛る必要は無い（盛っても害は無いが不要）。`SpritePickingMode` 自体 0.15 に存在しない。
- **`Pointer<Move>`/`<Down>` observer で `trigger.event().hit.position` を読むとき**: `HitData.position`
  は **`Option<Vec3>`**。`unwrap_or(Vec3::ZERO)` で握り潰すと、None のとき「world 原点 - entity 位置」
  という garbage 座標になり crosshair / drag が画面外へ飛ぶ。**`let Some(p) = trigger.event().hit.position
  else { return };` で skip する**（sprite picking backend では実際は常に Some だが、別 backend 併用や
  将来の変更に対する安全側）。world→entity-local は `p - gt.translation()`（chart 等 scale=1 前提）。
  `chart_crosshair.rs::install_chart_crosshair_observer` が実例。
- **`Visibility` を持たない中間 entity（`Transform` だけで spawn した content_area 等）の子 Sprite が
  描画 / picking されないのでは？と不安**: 0.15 の `visibility_propagate_system` は親が Visibility
  コンポーネントを欠く場合 **`true` にフォールバックする**（`bevy_render-0.15.1/.../visibility/mod.rs:404-407`
  "fall back to true if no parent is found or parent lacks components"）。よって `Transform` のみの
  content_area の下に `Sprite`（required components で Visibility 自動付与）を吊るしても
  `InheritedVisibility=true` になり、描画も sprite picking も効く。floating_window の content_area が実例。
- **テキストが更新されない**: marker component を貼り忘れ、または親子関係の貼り
  忘れ（`add_child` か `set_parent` のどちらかが必要）。
- **`Changed<T>` 駆動の system が収束せず毎フレーム再発火し続ける**: ある system が
  `Changed<T>` を読んで反応し、別の（または同じ）system が `T` を毎フレーム書き戻すと
  無限に再発火する。典型は autoscale 等の派生値を `&mut T` で毎フレーム代入するケース。
  `DerefMut` は**同じ値を代入しても** change tick を立てる（規約 2 の差分書き込みと同根。
  Resource/Component どちらにも効く）ので、`if (old - new).abs() > f32::EPSILON { old = new; }`
  で実変化時のみ書く。self-`Changed` ループを断ちたいときは **writer と reader を別 system に
  分け、reader は read-only にして再計算要求を `Event` で渡す**（writer が `Changed<T>` を
  読まなくなり依存が一方向化する）。検証は収束テスト: 対象を spawn → `app.update()` を 5 回 →
  末尾に `|q: Query<(), Changed<T>>, mut log: ResMut<_>| log.push(q.iter().count())` を
  `.chain()` で挟み frame 3 以降のカウントが 0 であることを assert する
  （`add_event::<RequestX>()` をテスト側 App にも入れる。無いと `EventWriter` 取得で panic）。
  src/ui/chart_viewstate.rs の autoscale 3 段 (data_tick / interaction_tick / autoscale_apply) が実例。
- **`Changed<T>` 駆動 system が `commands.spawn(...).set_parent(stored_entity)` で
  `The entity with ID ...v.. does not exist` panic (`bevy_hierarchy child_builder.rs:173`)**:
  `set_parent`/`add_child` は親が despawn 済だと apply 時に panic する（`world.entity_mut(parent)`）。
  典型は「ある entity の `Changed<T>` を読み、その entity が保持する子 entity (gutter/badge 等) の
  Entity id へ動的に子を spawn+parent する」system で、**同じ frame に別 system がその親 entity を
  `despawn_recursive` する**ケース。query は実行時点で生きている entity を返すが、despawn の
  command が自分の `set_parent` より先に flush されると親が消えて panic する（本プロジェクトでは
  chart panel が prune→`instrument_chart_sync_system` で spawn 直後に despawn される startup churn で発生）。
  **対策 2 段**: ① 親 (gutter) の生存を `Query<(), With<GutterMarker>>` で受け
  `if !q.contains(stored_id) { continue; }` で skip（despawn 済なら描かない）。
  ② 動的 spawn system を **despawn する側の system の `.after(...)` に置く**（sync point で despawn が
  先に flush され、`Changed` query に死んだ親が出なくなる）。①だけだと「同一 flush batch で despawn が
  先順位」のレースが残るので ②と併用が確実。回帰テスト: 親を spawn→即 despawn→ref だけ残した entity で
  system を回し panic しないこと + 子が 0 件を assert。src/ui/chart_axes.rs の axis label system が実例。
- **`CosmicEditBuffer` を spawn したのに文字が全く描画されない (背景すら出ない)**:
  このフォークの `render_texture` は `CosmicWidgetSize::scan()` で `Has<TextEdit2d>` または
  `Has<TextEdit>` を要求する。**`TextEdit2d` を付けないと `logical_size()` が `Err` を返し描画が
  skip される**。read-only ラベル/gutter でも `TextEdit2d` は必須。入力を受けたくないなら
  `TextEdit2d` を外すのではなく **`ReadOnly`** を付ける（`kb_input_text` は readonly で早期 return、
  かつ `change_active_editor_sprite` は `Without<ReadOnly>` フィルタなので focus-on-click 対象外になる）。
- **cosmic_edit で折り返しを切りたい / 行を折り返したい**: このフォークに `Buffer::set_wrap` 経路は
  実質無く、**`CosmicWrap` Component** で制御する。`CosmicWrap::InfiniteLine` = 折返し無し
  (source 行 == layout 行)、`CosmicWrap::Wrap` (default) = 折返しあり。spawn tuple に入れるだけ。
- **cosmic_edit の `InputSet` を `.before/.after` したい**: パスは **`bevy_cosmic_edit::InputSet`**
  (crate root)。`bevy_cosmic_edit::input::InputSet` は `input` module が private なので不可。
  keyboard 系 (`kb_input_text`/`kb_move_cursor`/`kb_clipboard`) は **`Update`** の InputSet で走る
  (`input_mouse` だけ `PreUpdate`) ので、自前の Update system から `.before/.after(InputSet)` が効く。
  Tab/Enter を奪うのは `.before(InputSet)` + `ResMut<ButtonInput<KeyCode>>::reset(key)`。
  カスタム編集後は `CosmicTextChanged` を手動 send しないと下流 (sync/undo/autosave) が空振りする
  (cosmic は内部 `is_edit` のときだけ発火)。全文取得の `BufferExtras::get_text` は `pub(crate)` で
  src/ から呼べない → `b.lines.iter().map(|l| l.text()).collect::<Vec<_>>().join("\n")` で代替。
- **bevy_cosmic_edit の unfocused field (TextEdit / TextEdit2d) が黒い箱で文字が見えない**:
  通常 2 つの独立した罠が同時に起きる。背景を一時的に `CosmicBackgroundColor(rgb(1,0,0))`
  にして切り分けると速い：赤背景が出れば render は走っており文字色 or layout の問題、
  出なければ render 自体が回ってない別問題（logical_size==0 など）。
  - 罠 A: **`DefaultAttrs` が default のまま** → `Attrs::new()` で `color_opt = None`、
    `render_texture` の fallback `rgb(0,0,0)` で黒文字になり背景に溶ける。
    `with_text`/`set_text` に attrs を渡すだけでは効かない。spawn 時に
    `DefaultAttrs(AttrsOwned::new(Attrs::new().color(CosmicColor::rgb(220,220,220))))`
    を明示挿入する（`strategy_editor.rs:163-165` 参照）。required components の
    default が黒なのでオーバーライドが必要。
  - 罠 B: **Node の height が DPI 倍化された line_height より小さい** → `set_initial_scale`
    が DPI 2x で `Metrics(12, 14)` を `(24, 28)` に倍化する一方、`set_buffer_size` は
    `ComputedNode.logical_size()` で buffer height を決める。例えば `Val::Px(18.0)` の
    row だと logical 18 < line_height 28 となり `shape_until_scroll` が `layout_runs=0`
    を返して glyph が生成されない（バッファに text はあるのに表示されない）。
    DPI 2x 想定で `Val::Px(30.0)` 以上にする、または初期 `Metrics(9, 11)` のように
    小さくする。診断は buffer の `layout_runs().count()` / `metrics()` / `size()` を
    `info!` で出す。`scenario_startup_panel.rs:119-127` が修正例。
- **gRPC コールバックから Bevy world を触りたい**: 触れない。
  `tokio::sync::mpsc::UnboundedSender` でメッセージを送り、Bevy 側の system
  （`status_update_system` 参照）で `try_recv` → `ResMut` に反映する。
  `main.rs:80-127` がテンプレート。
- **マウスホイール / ドラッグが「カメラ操作」と「パネル操作」の両方に効く**:
  `bevy_pancam` の `do_camera_zoom` / `do_camera_movement` と、`bevy_cosmic_edit` の
  `input_mouse` 等は **同じ `MouseWheel` / マウスイベントを別 system が独立に読む**。
  `EventReader` は system ごとにカーソルが独立なので「片方が消費して終わり」にはならず、
  両方が反応する。解決は「毎フレーム条件判定して `PanCam.enabled` を書き換える system を
  追加し、`PanCamSystemSet` より `.before()` で前に走らせる」。`enabled = false` で
  zoom/pan 両方止まる。`bevy_cosmic_edit` 側はエディタ entity の `ScrollEnabled`
  component（`Enabled`/`Disabled`、`TextEdit2d` の required ではないので spawn 時に
  明示付与が必要）で on/off する。`src/camera.rs::pancam_suppression_over_editor_system`
  が実装例。**この system は Phase 7.3 C で chart draw 領域 (`With<ChartViewState>` の Sprite) も
  対象に拡張済**: cursor が editor **または** chart 上 かつ Ctrl 非押下なら PanCam を無効化する
  (`pancam.enabled = ctrl || !(over_editor || over_chart) || dragging`)。新しい world-space パネルで
  ホイール/ズームを自前実装するときは、この 1 system に `Query<(&GlobalTransform,&Sprite), With<新Marker>>`
  と `over_xxx` を OR で足す (PanCam.enabled を書く system を 2 つに増やすと last-writer-wins で競合する)。
  パネル側のホイール system は対称に「Ctrl 押下中は `wheel.clear(); return` でカメラズームに譲る」。
  値変化時のみ `pancam.enabled` を書く (無条件代入は spurious な `Changed<PanCam>` を立てる)。
- **ズーム時に編集パネルの表示行がズレる / スクロール位置が動く**: まず疑うのは上記の
  入力二重消費（ホイールがズームとスクロールの両方に効く）。render shadow buffer の
  layout を疑う前に、`info!` で `render_scale` と `buffer.scroll().line` を出して
  「ユーザーがスクロールしていないのに scroll.line がズーム倍率と相関して動くか」を
  確認する。スクショ目視推測で原因を追うとハマる。
- **ズームインで折り返し位置・行高が変わる（render_scale=1 では正常）**: bevy_cosmic_edit
  の **DPI トラップ**。`set_initial_scale`（buffer.rs）が `CosmicEditBuffer` のメトリクス
  を window scale factor 倍する（Windows 200% なら 14/18 → 28/36）。一方 focused 時に
  実描画される `CosmicEditor` 内部 buffer（`editor.with_buffer(|b| b.metrics())` で取れる）
  は DPI 倍されない。render shadow buffer 用のメトリクス計算で `buffer.metrics()`
  （= `&mut CosmicEditBuffer`）を読むと、focused 側のクローン元（14/18）と基準が
  食い違って二重スケールになる。**clone した buffer 自身のメトリクスを基準にスケールする**
  のが正解。診断は `info!` で `buffer.metrics().font_size` と
  `editor.with_buffer(|b| b.metrics().font_size)` を同時に出すと一目瞭然。
  詳細は memory `cosmic-edit-buffer-metrics-dpi-trap.md`。
- **focused エディタと unfocused エディタでフォントサイズが 2 種類になる（focused が小さい）**:
  `set_initial_scale`（`First` スケジュール、`Added<CosmicEditBuffer>` フィルタ）と
  `add_editor_to_focused`（`PostUpdate`）の**フレーム跨ぎ競合**。
  entity が spawn された「同フレームの PostUpdate」で `add_editor_to_focused` が先に走り、
  DPI スケール前の (14,18) で `CosmicEditor` を生成する。翌フレームの `First` で
  `set_initial_scale` が `CosmicEditBuffer` を (28,36) にスケールするが、
  **`CosmicEditor` 内部 buffer は触らない**ため、両者の metrics がズレる。
  修正は `set_initial_scale` のクエリに `Option<&mut CosmicEditor>` を追加し、存在すれば
  `ed.with_buffer_mut(|buf| buf.set_metrics(&mut font_system.0, m))` で同時スケール。
  診断ログパターン:
  ```
  add_editor_to_focused  entity=112v1  buffer_metrics=(14.0, 18.0)   ← DPI 前
  set_initial_scale      entity=112v1  scale=2.00  after=(28.0,36.0)
  drop_editor_unfocused  entity=112v1  buffer=(28.0,36.0) editor=(14.0,18.0)  ← ズレ
  ```
  詳細は memory `cosmic-edit-buffer-metrics-dpi-trap.md`。
- **Alt+F など修飾キーショートカットが bevy_cosmic_edit に「f」として書き込まれる**:
  `ButtonInput<KeyCode>` で Alt+キーを検出して処理した後、**同フレームで**
  `ResMut<Events<KeyboardInput>>` の `.clear()` を呼ぶことで `bevy_cosmic_edit` が
  そのフレームの keyboard イベントを読めなくなる。`.clear()` は `EventReader` の
  カーソルとは独立した一括削除なので、自分より前に読み終えた system には影響しない。
  **実行順を守ること**: `.clear()` を呼ぶ system を `bevy_cosmic_edit` の input system
  より `.before()` で前に走らせる（例: `.add_systems(Update, change_active_editor_sprite.after(menu_keyboard_system))`）。
  `src/ui/menu_bar.rs::menu_keyboard_system` が実装例。

## ground truth ソースの引き方

`.claude/skills/bevy-engine/src/` には Bevy 0.19-dev の全ソースがある。

- API 名や signature を確認したい: `crates/bevy_<name>/src/lib.rs` を読む
- 例を探したい: `examples/<category>/*.rs`（例: `examples/ecs/hierarchy.rs`）
- ただし **0.15 のコードを書く** ので、見たコードが 0.15 でも動くかは
  `references/0.15-vs-0.19.md` か実プロジェクトの既存パターンで照合する

汎用カテゴリ別の入口:
- ECS: `examples/ecs/` （`ecs_guide.rs` が最も網羅的）
- 2D: `examples/2d/`
- Picking / Pointer: `examples/picking/`
- Window / Input: `examples/window/`, `examples/input/`
- Assets: `examples/asset/`
- Shaders: `examples/shader/`

## 参考リファレンス

- [references/0.15-vs-0.19.md](references/0.15-vs-0.19.md) — バージョン差チートシート
- [references/egui-integration.md](references/egui-integration.md) — bevy_egui の流儀
- [references/egui-vs-worldspace.md](references/egui-vs-worldspace.md) — どちらを使うか
- [references/observers-and-pointer.md](references/observers-and-pointer.md) — observe & Pointer
- [references/ecs-basics.md](references/ecs-basics.md) — 汎用 ECS（プロジェクト外でも使える）
- [references/winit-update-mode.md](references/winit-update-mode.md) — WinitSettings / アイドル CPU 削減 / reactive update mode
