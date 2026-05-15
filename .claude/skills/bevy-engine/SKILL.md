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
  ⑦ `pair-relay` / `pair-nav` で `src/ui/**` を触る作業に入るとき（Orchestrator または
    Navigator サブエージェントがこのスキルを読まずに進めると、Bevy 0.15 固有の罠
    — `add_systems` タプル 20 上限、`IntoObserverSystem` の import path、
    `CosmicEditor` 経由の `with_buffer_mut`、Text2d Anchor の左寄せ — で必ずハマる）
  ⑧ `cosmic_edit`, `CosmicEditBuffer`, `CosmicEditor`, `TextEdit2d`, `FocusedWidget`,
    `CosmicBackgroundColor`, `CursorColor`, `CosmicTextChanged`, `set_text`, `with_buffer_mut`
    という語彙が出てきたとき

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

- **"the trait `Bundle` is not implemented for `(Sprite, Transform, ...)`"**:
  ほぼ全部 `Component` で derive されているはずなので、構成要素のどれかが Component
  でない（または import 漏れ）。`use bevy::prelude::*;` を確認。
- **"no method named `single` found"**: 0.15 は `get_single()`。
- **"no field `entity` on `Trigger`"**: 0.19 流。0.15 は `trigger.entity()` (method)。
- **"unresolved import `bevy::ChildOf`"**: 0.19 流。0.15 は `Parent`。
- **ウィンドウがドラッグでカーソルから逃げる**: 規約 5 の scale 倍を忘れている。
- **テキストが更新されない**: marker component を貼り忘れ、または親子関係の貼り
  忘れ（`add_child` か `set_parent` のどちらかが必要）。
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
  が実装例。
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
