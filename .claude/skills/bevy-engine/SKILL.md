---
name: bevy-engine
description: |
  The-Trader-Was-Replaced プロジェクトで Bevy（ゲーム/UI フレームワーク）を使う際の必読スキル。
  特に `src/camera.rs` と `src/ui/**` の floating-window 系（buying_power / positions / orders /
  run_result / strategy_editor / chart / sidebar / footer / menu_bar / order_panel / secret_modal）、
  操作系 UI（Bevy UI Node + Interaction/Button、bevy_egui は撤去済み）、
  bevy_pancam / bevy_cosmic_edit / bevy_vector_shapes 連携、Pointer 観測子（observe）による
  ドラッグ＆Z オーダー、Sprite + Transform による world-space ウィンドウ、Text2d 三分割
  （Text2d/TextFont/TextColor）を扱う。

  ALWAYS use this skill when:
  ① ユーザが `src/ui/*.rs` や `src/camera.rs` を編集しようとしている（新規実装だけでなく、
    「issue #N をレビューして修正して」「codex / Navigator のレビューで src/ui の bug を直す」
    などレビュー駆動の修正で src/ui を触るときも含む。**レビュー task でも src/ui を編集するなら
    本スキルを先に invoke する**）
  ② "Bevy", "bevy_egui", "PanCam", "Camera2d", "Sprite", "Text2d", "Plugin", "ECS",
    "Resource", "Component", "Event", "System", "Query", "Commands", "World",
    "observer", "Trigger", "Pointer<Drag>", "Pointer<Down>", "floating window",
    "パネル", "サイドバー", "フッター", "メニューバー",
    "リサイズ", "resize", "CursorIcon", "cursor icon", "カーソルアイコン",
    "ハンドル", "drag handle", "drag-resize" と言われたとき
  ③ Bevy のバージョン差（0.15 と 0.19/0.16/0.17/0.18 の API 差）でハマっているとき
  ④ "Bundle is deprecated", "set_parent", "Parent", "ChildOf", "get_single", "single",
    "Trigger::entity", "Trigger::target", "required components" など破壊的変更語彙が出たとき
  ⑤ 新しいパネル（floating window）を増やす／既存パネルを書き換える作業
  ⑥ 操作系 UI（Bevy UI Node + Interaction/Button）と 表示専用パネル（world-space Sprite）の
    どちらで作るか迷ったとき、または「発注フォーム」「モーダル」「ボタン」「ラジオ」「入力欄」
    「order_panel」「secret_modal」「UI Node」「Interaction」「Button」を扱うとき
    （bevy_egui は撤去済みなので egui は選択肢にない）
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
  ⑪ システム実行順序・スケジューリング・deferred Commands 関連: "add_systems", ".after",
    ".before", ".chain", "ApplyDeferred", "sync point", "システム順序", "スケジュール",
    "schedule cycle", "サイクル", "Commands が反映されない", "marker が反映されない",
    "deferred", "Visibility 競合", "query 競合", "毎フレーム diff-write", "is_changed" と
    言われたとき、または **複数 system が同じ Component（特に `Visibility` / marker）を
    読み書きする順序が絡む bug を直すとき**。Bevy の `Commands`（insert/remove/spawn）は
    sync point まで反映されないため、同フレームの後続 system は未反映の状態を見る。可視性 /
    マーカーの save/restore やパネル spawn のタイミングはこの遅延と system 順序に依存し、
    順序を `.before/.after` で固定しないと「保存側が一時状態を焼き込む」「marker が陳腐化する」
    「新規 spawn が 1 フレーム可視で出る」race になる（issue #31 で実際に踏んだ。読者・
    save 系を mode system の前に、spawn dispatcher を後続 apply/可視性 system の前に置いて解消）。

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

**重要な意思決定**: UI は **2 流派** が共存している。**bevy_egui は 2026-05-15 Phase 7
Sub-step 1.8d で完全撤去済み**（`Cargo.toml` に egui 依存なし／`src/**` に egui import ゼロ。
2026-05-20 Phase 9 確認）。下の「bevy_egui クイックリファレンス」節は**歴史的記録**で、新規実装に
使ってはならない。現存の 2 流派は:
- **world-space Sprite/Text2d (retained-mode)** — **表示専用**の floating panels（buying_power /
  positions / orders / run_result / chart）。`spawn_floating_window` + Sprite + Transform +
  observe(Pointer<Drag>) で自前実装。カメラのパン/ズームに追随。Z オーダーは `WindowManager::max_z`。
- **Bevy UI Node + `Interaction`/`Button` (flexbox UI layer)** — **操作系**のすべて（sidebar /
  menu_bar / footer / instrument_picker、Phase 9 の order_panel / secret_modal）。`Node` +
  `BackgroundColor` + `Button` + `Interaction` + `Text`/`TextFont`/`TextColor`、`Changed<Interaction>`
  駆動の system でクリック処理。表示/非表示は `Node.display = Flex/None`（`Visibility` ではなく
  Display）。`GlobalZIndex` で前面化。**テキスト入力は keyboard event の `Events<KeyboardInput>::drain()`**
  （`instrument_picker.rs::picker_searchbox_input_system` / `secret_modal.rs` が手本。cosmic_edit は
  ストラテジエディタ専用で、UI-node の小入力欄には使わない）。

**判断ルール**: 値を出すだけ → world-space Sprite panel（既存 buying_power 等を踏襲）。
ボタン・ラジオ・入力欄・モーダルなど**操作が要る** → **Bevy UI Node**（instrument_picker /
menu_bar が手本）。**「egui を使うか」で迷う必要はない（もう無い）**。
**迷ったら既存の同種パネルと同じ流派を踏襲する**（混在の見た目を増やさない）。

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

**逆向き（パネル/モーダルを撤去するとき）**: 上の 6 点を **すべて** 剥がす。`src/ui/<name>.rs`
を消すだけだと `mod.rs` 側に死んだ配線が残ってコンパイルが割れる。`mod.rs` で消す箇所は
①`pub mod <name>;` ②`use crate::ui::<name>::{...}` ブロック ③`init_resource::<...>()`
④Startup の `add_systems` 内 spawn 関数 ⑤Update の `add_systems` ブロック（その module の
system 群）⑥（floating window なら）`panel_spawn_dispatcher_system` の match arm と
`PanelKind`／sidebar ボタン。あわせて、撤去した module が公開していた **Resource を引数に取る
公開関数のシグネチャ**（例 `apply_status_update` の `&mut XxxFeedback`）と、その **全呼び出し側**
（本番 `main.rs`／`backend_sync.rs` ＋ `#[cfg(test)]`／`tests/e2e/support` のテスト呼び出し）、
`main.rs` の `insert_resource`、e2e harness の accessor、`tests/e2e/flows` の対応 flow と
`tests/e2e_replay.rs` 登録・`FLOWS.md` の記述まで一気に剥がす。残骸検出は
`grep -rn "<RemovedSymbol>" src/ tests/ docs/` を最後に回して 0 件を確認する
（コメント内の旧シンボル参照＝z-order・doc コメントも拾って直す）。

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

⚠️ **world-space sprite ボタンのクリックをシステムに橋渡ししたい（event-driven sprite button パターン）**:
`Changed<Interaction>` / `With<Button>` クエリは UI Node 専用で、Sprite には効かない。Sprite ボタンでは
代わりに `Pointer<Click>` observer → `EventWriter<MyEvent>` → `EventReader<MyEvent>` の 3 段構成を使う。

```rust
// 1. イベント型: Copy が必須（observer クロージャが move キャプチャするため）
#[derive(Event, Debug, Clone, Copy)]
pub struct MyButtonPressed(pub MyButton);

// 2. Sprite に observer を attach（spawn 時に `.observe(...)` をチェーン）
let btn = MyButton::Submit; // Copy enum value をキャプチャ
commands.entity(content_area).with_children(|p| {
    p.spawn((
        Sprite { color, custom_size: Some(Vec2::new(w, h)), ..default() },
        Transform::from_xyz(x, y, z),
        btn, // marker component
    ))
    .observe(move |_: Trigger<Pointer<Click>>, mut ev: EventWriter<MyButtonPressed>| {
        ev.send(MyButtonPressed(btn)); // btn は move でキャプチャ済み
    });
});

// 3. system は EventReader を使う（Changed<Interaction> ではない）
pub fn my_button_system(mut events: EventReader<MyButtonPressed>, ...) {
    for MyButtonPressed(button) in events.read() { ... }
}
```

**登録の注意点**:
- `app.add_event::<MyButtonPressed>()` をプラグインの `build` と **すべてのテスト App builder** に入れる
  （漏れると `order_submit_button_system` 等が "could not access system parameter" で panic）
- テスト側で "クリック" をシミュレートするには `app.world_mut().send_event(MyButtonPressed(btn))` を使う
  （`spawn((Button, Interaction::Pressed, btn))` は UI Node 専用で Sprite テストには使えない）
- E2E harness に `pub fn press_my_button(&mut self, btn: MyButton)` ヘルパーを追加するのが定番
  （`place_order_via_ui` + `press_order_button` in `tests/e2e/support/mod.rs` が実例）

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
    for mut t in &mut q { t.translation.x += time.delta_secs(); } // 0.15 は delta_secs（delta_seconds は ≤0.14 旧名）
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
| `time.delta_secs()` | `time.delta_secs()` | **改名は 0.15 で完了済**。`delta_seconds()` は ≤0.14 の旧名で 0.15 では存在せず E0599（`elapsed_seconds`→`elapsed_secs` も同様）。0.15/0.19 とも `delta_secs()` が正 |
| `OrthographicProjection` 直接 | 同じ（経由が違う） | 0.19 は `Projection` enum 経由 |
| `app.observe(system)` | `app.add_observer(system)` | **App グローバル observer の登録。`app.observe()` は 0.15 に存在しない。エンティティローカルは `.observe(...)` のまま** |

詳細は [references/0.15-vs-0.19.md](references/0.15-vs-0.19.md) を参照。

**判断ルール**: 「ミラーで見た書き方が `cargo check` で落ちる」「`E0XXX` でフィールド
が無い」と言われたら、ほぼ確実に 0.15/0.19 差。0.15 の書き方を `references/0.15-vs-0.19.md`
で確認するか、既存の `src/ui/*.rs` を grep して同じパターンを探す。

## bevy_egui のクイックリファレンス（⚠️ 歴史的記録 — 現在は使用不可）

> **bevy_egui は 2026-05-15 に完全撤去済み**（`Cargo.toml` に依存なし）。以下は撤去前の参考。
> **新規実装で egui を使ってはならない**。操作系 UI は「Bevy UI Node + Interaction」流派
> （上の「重要な意思決定」節、instrument_picker.rs / menu_bar.rs / order_panel.rs が手本）を使う。

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

- **新規 module / 大きな編集が「いつの間にか revert されている」（ファイルが消える / 編集が巻き戻る）**:
  この workspace は外部の linter/watcher が走っており、**`cargo build --lib` がコンパイル不可な間は未コミットの編集（新規ファイル含む）を巻き戻すことがある**（Phase 9 Step 4 で `trading.rs`/`ui/mod.rs`/新規 `modify_modal.rs`/`order_context_menu.rs` が連鎖的に消えた）。複数ファイルにまたがる UI 追加（新 module + `mod.rs` の `pub mod` + use + add_systems + 新 Resource の `init_resource`）は **lib が緑になる単位でまとめて入れ、各バッチ直後に `cargo build --lib` を叩いて緑を確認**すること（緑なら revert されない）。`bins`/`tests` の都合（例: proto 未追加で `engine::Xxx` が無い）でツリー全体が割れていても、**`src/lib.rs` 側だけは自己完結で緑にできる**ので、UI ロジックは lib に閉じた形（proto 依存は `main.rs` の dispatch arm に隔離）で先に確定させると巻き戻されない。System note「file was modified ... intentional」が出たら**自分の編集が消えた可能性を疑い、grep で生存確認**する。
- **計画書 brief が「proto は regen 済み・`engine::Xxx` 利用可能」と言うのにコンパイルで `not found in engine`**:
  proto の追加が別担当 (Python) 所有で**まだ tree に入っていない**可能性。`python/proto/engine.proto` を grep して当該 message/rpc の有無を確認し、`target/debug/build/backcast-*/out/engine.rs` の生成物（複数 hash があり最新とは限らない）と突き合わせる。Rust 担当が proto を触れない制約なら、**proto 依存部分（`engine::Xxx` を使う dispatch arm / mock servicer stub）は briefed shape どおり書いて lib を緑に保ち、blocker を STOP+REPORT**（推測で proto を書き換えない）。
- **"could not access system parameter Res<'_, T>" / "Events<'_, E>" で test が panic**:
  既存 system のシグネチャに `Res<T>` / `ResMut<T>` / **`EventWriter<E>` / `EventReader<E>`** を追加した直後、その system を `app.update()` で踏む既存テスト全てが panic する典型。本番側 (`main.rs` / `UiPlugin::build`) は `init_resource` / `add_event` 済みでも、test 側の `App` builder は手動登録していないので追従漏れする。同 system を踏むテスト全部に `app.init_resource::<T>();`（Res 系）または `app.add_event::<E>();`（Event 系。`EventWriter`/`EventReader` は `Events<E>` リソースを要求するので無いと `could not access system parameter ResMut<Events<E>>` で panic）を追加すること。**最初の panic ログには 1 つの param 名しか出ないが、それを足した後にも別の param で同じ panic が出る**（Phase 7.6 では `ReplayStartupProgress` → `TradingData` → `LastRunResult` → `Time<Real>` → `ScenarioStartupParams` と 5 段階で連続発覚）。signature 拡張時は本番側で App build に並んでいる全 param を確認してから 1 ターンでまとめて登録するのが安全。
  ⚠️ **検証ターゲットの網羅**: この panic は `tests/e2e/` の integration テストだけでなく、`src/ui/*.rs` 内の `#[cfg(test)] mod tests`（lib unit テスト）にも刺さる。`#[cfg(test)]` App は本番 plugin を経由せず手動で system を `add_systems` するので、こちらの追従漏れは `cargo test --test e2e_replay` では**一切観測されない**。public system のシグネチャを変えたら **`cargo test --lib <module>` と `cargo test --test e2e_replay` の両方**を回すこと（Issue #21 で `handle_save_layout_system` に `EventWriter<LayoutSaveAsRequested>` を足した際、e2e は 98 passed のまま lib unit テスト 7 件が panic していたのを review で初めて発見）。
- **"the trait `Bundle` is not implemented for `(Sprite, Transform, ...)`"**:
  ほぼ全部 `Component` で derive されているはずなので、構成要素のどれかが Component
  でない（または import 漏れ）。`use bevy::prelude::*;` を確認。
- **`error[B0001]` "accesses component(s) ... in a way that conflicts with a previous system parameter"
  （実行時 panic／システム実行時に init で落ちる）**: 同一 system に同じ component（典型は `&mut Node`）の
  query を **2 つ以上** 持ち、フィルタが**証明可能には**互いに排他でないと出る。Bevy の衝突チェックは
  「現実にはどの entity も両方に該当しない」では通らず、**`With<X>` vs `Without<X>` の対**で静的に分離
  できることを要求する。例: `pause_q: Query<&mut Node, With<PauseResumeButton>>` と
  `speed_q: Query<&mut Node, (With<SpeedButton>, Without<TransportButton>)>` は、現実の entity が
  重ならなくても **PauseResumeButton で分離されていない**ので衝突する（実際の footer PauseResume entity が
  `PauseResumeButton` + `TransportButton` の二重マーカーを持つ罠と同根）。**対策**: 各 query 対に必ず
  「片方 `With<M>`／もう片方 `Without<M>`」を入れて全 pair を分離する（例: speed_q に `Without<PauseResumeButton>`
  を足す）。分離フィルタは**実 entity の集合を変えない**もの（その marker を実際には持たない）を選ぶこと。
  どうしても重なる更新が要るなら `ParamSet<(Query<..>, Query<..>)>` でまとめる。⚠️ この panic は system が
  **実際に実行される schedule でしか出ない**: e2e harness が当該 system を登録していないと e2e は緑のまま
  `#[cfg(test)]` の単体 App（その system を `add_systems` する）だけが落ちるので、複数 `&mut` query を持つ
  system を足したら **その system を回す側のテスト（lib unit / harness 両方）を必ず走らせる**。
- **「`spawn((Marker, Button, Interaction))` は `BackgroundColor` を含まないから `Query<(&Interaction, &mut BackgroundColor)>` にマッチしない」と思い込む（レビューの false positive）**:
  **required components は推移的**。`Button` は `#[require(Node, FocusPolicy, Interaction)]`、`Node` は
  `#[require(ComputedNode, BackgroundColor, BorderColor, BorderRadius, FocusPolicy, ScrollPosition, Transform,
  Visibility, ZIndex)]`（`bevy_ui-0.15.1/src/ui_node.rs` / `widget/button.rs`）。つまり `Button` を spawn する
  だけで `Node` 経由 `BackgroundColor` まで**自動挿入**され、`(&Interaction, &mut BackgroundColor)` クエリに
  マッチする。`footer_pause_resume_system` のシグネチャに `&mut BackgroundColor` を足しても、`Harness::click`
  が spawn する `(marker, Button, Interaction::Pressed)` は依然マッチする（A2 は green のまま）。
  ⚠️ **罠の方向**: 「query に component を足した → spawn 側がそれを明示していないから壊れる」という指摘は、
  その component が既存 component の推移 require で供給されていれば**誤り**。判定するときは
  spawn タプルの**直接の**構成要素だけでなく、各 component の `#[require(...)]` を**推移的に**辿る
  （`~/.cargo/registry/src/*/bevy_ui-0.15.1/src/` を grep）。最終的な真偽は `cargo test` の実測で確定する
  （静的読みで「マッチしない」と断じない）。実例: issue #40 フォローアップのレビューで codex が「A2 が
  `BackgroundColor` 不足でマッチせず fail」と Medium 指摘 → 推移 require の見落としで false positive、
  実測 `e2e_replay` 116 passed で A2 green だった。
- **"no method named `single` found"**: 0.15 は `get_single()`。
- **"no field `entity` on `Trigger`"**: 0.19 流。0.15 は `trigger.entity()` (method)。
- **"unresolved import `bevy::ChildOf`"**: 0.19 流。0.15 は `Parent`。
- **ウィンドウがドラッグでカーソルから逃げる**: 規約 5 の scale 倍を忘れている。
- **マウスカーソルアイコンを変えたいが `window.cursor.icon` フィールドが無いと言われる**: Bevy 0.15 の
  カーソルアイコン変更は **フィールドではなく Component 挿入** で行う。`window.cursor_options.icon` ではなく、
  PrimaryWindow entity に `CursorIcon` Component を insert する方式。import が非 prelude なので明示が必要:
  ```rust
  use bevy::window::{PrimaryWindow, SystemCursorIcon};
  use bevy::winit::cursor::CursorIcon;
  // observer 内 (commands あり):
  if let Ok(win) = window_q.get_single() {
      commands.entity(win).insert(CursorIcon::from(SystemCursorIcon::EwResize));
  }
  // Pointer<Out> で元に戻す:
  commands.entity(win).insert(CursorIcon::from(SystemCursorIcon::Default));
  ```
  `bevy::winit` は `bevy_internal` が `bevy_winit as winit` で re-export しているので
  `bevy::winit::cursor::CursorIcon` でアクセスできる。`floating_window.rs` の resize handle observer が実例。
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
  `InheritedVisibility=true` になり、描画も sprite picking も効く。
  ⚠️ **ただしこの「fallback true」は逆方向＝隠す側で黙って壊れる**: 親 root を `Visibility::Hidden`
  にしても、間に `Visibility` を欠く中間 entity（Transform だけの content_area 等）があると
  `propagate_recursive` が `(&Visibility, &mut InheritedVisibility)` の `get_mut` に失敗して
  **early-return し、そこで伝播の連鎖が切れる**（`bevy_render-0.15.x/.../visibility/mod.rs` の
  `propagate_recursive` 冒頭）。結果、root 枠（Sprite なので Visibility あり）と兄弟の inner_glow/
  rim_light/title_bar は隠れるが、**content_area の子（ラベル/フィールド）は `InheritedVisibility`
  が default の `true` のまま残り続ける**（枠だけ消えて中身が表示されたままになる実バグ。Startup パネルを
  Manual/Auto で隠す経路で顕在化）。**対策**: 隠す可能性のある world-space ツリーでは、root と
  葉の間の全中間 entity に `Visibility::default()` を明示付与する（required components で
  `InheritedVisibility` も付き、連鎖が繋がる）。`spawn_floating_window` の content_area は
  この理由で `(Transform, Visibility::default())` で spawn 済（floating window 全種に効く）。
  ⚠️ **回帰テストの落とし穴**: 「root の `Visibility` が `Hidden` になる」だけを assert するテスト
  （m8）は、中身が残るこのバグを取りこぼす（root はちゃんと Hidden になっているため）。中身まで
  隠れることを保証するには、**content の葉から root まで Parent を辿り、経路上の全 entity が
  `InheritedVisibility` を持つ（＝伝播が中身まで届く構造）ことを assert** する（render プラグイン
  不要で root-cause を固定できる）。`tests/e2e/flows/m11_startup_window_content_hides_with_panel.rs` が実例。
- **テキストが更新されない**: marker component を貼り忘れ、または親子関係の貼り
  忘れ（`add_child` か `set_parent` のどちらかが必要）。
- **world-space sprite パネルで `Text2d` ラベルが親ウィンドウ枠を黙ってはみ出す / フィールドと重なる**:
  world-space sprite には Bevy-UI のような clip/overflow が無いので、`Text2d` は親ウィンドウ sprite の
  枠を**自由に超えて描画される**（背後の別パネルに重なって見えるだけで、警告も clip も出ない）。さらに
  `Text2d` の既定アンカーは中央なので、**長いラベルほど左右対称に伸びて枠を超える**（中心アンカー window は
  左端 = `-size.x/2`。ラベル中心 `LABEL_X` + 文字列半幅がこれを下回るとはみ出す）。フォーム的に直すなら
  `bevy::sprite::Anchor::CenterRight` を `Text2d` に **component として足し**（0.15 で有効。`floating_window.rs`
  のタイトル文字が `Anchor::CenterLeft` を使うのが実証）、`Transform.x` をフィールド左端の少し手前に置く →
  ラベルは右揃えで左へ伸び、**ラベル長に依存せずフィールドとの間隔が一定**になる。あわせてウィンドウ幅を
  最長ラベルが収まるサイズに広げる。`scenario_startup_panel.rs` の Startup window のラベル列が実例。
  ⚠️ ラベルのはみ出しは「DPI/tint バグ」と症状が混ざりやすい（暗い文字が読めない＝tint、文字が枠外＝anchor/幅）。
  原因を分けて、ピクセル位置（zoom 倍率 × world 座標）で「どの要素同士が重なるか」を先に特定してから直す。
- **パネルを Bevy-UI `Node` から world-space sprite に作り替えたら、可視性/表示制御が黙って効かなくなった**:
  そのパネルの可視性 system が `Query<&mut Node, With<Marker>>` で `Node.display = Flex/None` を書く設計だと、
  sprite 化後の root は `Node` を持たない（`Visibility` を持つ）ので **query が 0 件マッチ＝完全な no-op**。
  例外も警告も出ず、モード連動の表示/非表示が黙って壊れる（Phase: Startup を sidebar flexbox → sprite 化した際、
  Live モードで隠れなくなった実バグ）。host を sprite に変えたら、その entity を読む全 system の query 型を
  `&mut Node`→`&mut Visibility`、`Node.display`→`Visibility::{Inherited,Hidden}` に揃える
  （show=`Inherited`、hide=`Hidden`、layout_persistence の restore も同じ規約）。
  ⚠️ **テストが「自前で `Node` entity を spawn して `Node.display` を assert」していると、host 切替後も
  そのテストは通り続ける（実在しない構成＝fiction を検証している）**。本番 root が sprite なら、テストも
  `(Visibility::Inherited, Marker)` を spawn して `Visibility` を assert する形に移植する。`scenario_startup_panel.rs`
  の `apply_startup_panel_visibility_system` とその visibility テスト群が実例。
- **`PanelKind` は「サイドバーボタン」と「floating window root」の両方に貼られる二重 marker**:
  sidebar.rs は Panels セクションの各ボタンに `PanelKind::Xxx` を marker として付け（`spawn_panel_btn`）、
  dispatcher は同じ `PanelKind::Xxx` を window root にも付ける。よって `Query<(Entity, &PanelKind)>` を
  **フィルタ無し**で回す system は、window だけのつもりでも**サイドバーボタンまで巻き込む**。despawn 系
  （例 `order_window_despawn_system`：mode 離脱で ORDER window を畳む）でこれをやると、ボタン entity ごと
  `despawn_recursive` してボタンが**二度と出なくなる**（`ExecutionMode` の default が `Replay` なら起動 1
  フレーム目で消える）。**window だけを対象にする system は必ず `With<WindowRoot>` を付ける**
  （`spawn_floating_window` が root に `WindowRoot` を付与済み。ボタンは持たない）。逆にボタンだけを
  gate する visibility system は `With<Button>` を付ける（`apply_order_button_visibility_system` が実例）。
  回帰テスト: window（`(WindowRoot, PanelKind::X)`）とボタン（`(Button, PanelKind::X)`）を両方 spawn し、
  system 実行後に **window は despawn / ボタンは生存** を両方 assert する（片方だけだと巻き込みを取りこぼす）。
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
- **macOS だけアプリ全体がフリーズ（ファイルダイアログ等の blocking native API を Update system で呼んでいる）**:
  `add_systems(Update, ...)` の system は **NonSend 引数が無ければ全 Send 扱い**で、Bevy 0.15 の
  multi-threaded executor が**ワーカースレッドで実行し得る**。`rfd::FileDialog::pick_file()/save_file()`
  のような **同期 blocking native API** はワーカーから呼ぶと macOS で NSOpenPanel をメインキューへ
  `dispatch_sync` し、メインスレッドは executor 内でその system 完了を待ってブロック → **デッドロック**
  （ダイアログ未表示・無クラッシュの完全フリーズ。Windows/Linux では出ない。Issue #17）。
  **対策**: `NonSend` でメイン強制実行は描画も止まるので不採用。代わりに **非ブロッキング化**する:
  ```rust
  // 起動 system: ワーカーで future を spawn し Resource に保持
  let task = bevy::tasks::AsyncComputeTaskPool::get()
      .spawn(async move { rfd::AsyncFileDialog::new().save_file().await.map(|h| h.path().to_path_buf()) });
  pending.task = Some(task);
  // poll system: 毎フレーム poll_once（完了したら結果を取り出す）
  use bevy::tasks::futures_lite::future;
  if let Some(result) = future::block_on(future::poll_once(pending.task.as_mut()?)) { pending.task = None; /* use result */ }
  ```
  rfd が内部でメインスレッドへ正しく dispatch し（パネルは winit run loop が駆動）、メインを
  ブロックしないのでデッドロックしない。⚠️ **async 化でダイアログは非モーダルになる**: 同期 API は
  事実上モーダル（開いている間アプリが止まる）だったので、複数ダイアログの同時起動や、ダイアログ
  表示中の state drift を防ぐ guard を自前で持つこと（**単一の `Pending{...}` Resource + `is_active()`
  チェックで全ダイアログを相互排他**にしてモーダル性を復元する／pre-flush 等の「ダイアログ直前→直後」で
  原子的だった処理は poll 完了側（write 直前）に寄せる）。`src/ui/layout_persistence.rs` の
  `PendingFileDialog` / `poll_*_dialog_system` が実装例。
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
- **headless テストで Strategy Editor / cosmic パネルを spawn したい（`CosmicEditPlugin` を足さずに）**:
  spawn 系（`panel_spawn_dispatcher_system` → `spawn_strategy_editor_panel`）が要求するのは
  `ResMut<CosmicFontSystem>` resource だけ。**プラグイン全体を足すと render/asset 依存（`Assets<Image>` /
  primary camera 要求）で MinimalPlugins 下では panic する**ので、`CosmicFontSystem(pub FontSystem)` を
  手構築して `insert_resource` する: `CosmicFontSystem(cosmic_text::FontSystem::new())`。`FontSystem::new()`
  は CPU のみ（fontdb の system font ロード）で headless OK、glyph は出ないが entity spawn には無関係。
  ⚠️ **依存の罠**: `bevy_cosmic_edit` は backcast の normal dep なので統合テスト（`tests/`）から
  `bevy_cosmic_edit::prelude::CosmicFontSystem` を引けるが、`cosmic_text` は transitive なので
  **`cosmic-text` を `[dev-dependencies]` に明示追加**しないと `FontSystem` を名指せない
  （バージョンは Cargo.lock の解決版＝現在 `0.12` に合わせる。型同一性のため）。
  bare `App`（MinimalPlugins すら無し）＋必要 system だけでも spawn は走る（spawn は純 ECS）。
  実例: `tests/e2e/flows/i5_file_open_spawns_editor_and_chart.rs`。

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
