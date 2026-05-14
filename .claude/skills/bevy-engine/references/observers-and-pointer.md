# Observer & Pointer events（Bevy 0.15）

Bevy 0.15 で安定化された **entity-local observer**。本プロジェクトでは
`floating_window.rs` の drag / Z-order に多用している。

## 基本形

```rust
use bevy::prelude::*;

commands.spawn((
    Sprite { ..default() },
    Transform::from_xyz(0., 0., 0.),
))
.observe(|trigger: Trigger<Pointer<Down>>,
          mut q: Query<&mut Transform>| {
    let entity = trigger.entity();  // この observer が貼られた entity
    let event  = trigger.event();    // PointerEvent
    if let Ok(mut t) = q.get_mut(entity) {
        // ...
    }
});
```

- `Trigger<E>` で受け取りたいイベント型を指定
- `trigger.entity()` で対象 entity（0.19 では `target()`）
- `trigger.event()` で event 本体
- observer のクロージャは普通の system と同じく `Query` / `Res` / `ResMut` などを取れる

## 主要な Pointer イベント

```rust
Trigger<Pointer<Over>>       // ホバー開始
Trigger<Pointer<Out>>        // ホバー終了
Trigger<Pointer<Down>>       // 押し下げ
Trigger<Pointer<Up>>         // 離す
Trigger<Pointer<Click>>      // クリック
Trigger<Pointer<Move>>       // 移動
Trigger<Pointer<DragStart>>
Trigger<Pointer<Drag>>       // ドラッグ中（delta が取れる）
Trigger<Pointer<DragEnd>>
Trigger<Pointer<DragEnter>>
Trigger<Pointer<DragOver>>
Trigger<Pointer<DragLeave>>
Trigger<Pointer<Drop>>
Trigger<Pointer<Scroll>>
```

`Drag` イベントは `event.delta: Vec2`（前フレームからの移動量、ピクセル）を持つ。

## Drag のテンプレート（このプロジェクトの流儀）

```rust
.observe(
    |drag: Trigger<Pointer<Drag>>,
     mut query: Query<&mut Transform, With<WindowRoot>>,
     parent_query: Query<&Parent>,
     camera_query: Query<&OrthographicProjection, With<Camera2d>>| {
        // 1. 自分の親 entity を取る（drag を受けたのは title bar、動かすのは root）
        let Ok(parent) = parent_query.get(drag.entity()) else { return };
        let Ok(mut transform) = query.get_mut(parent.get()) else { return };

        // 2. PanCam のズーム scale を取得
        let scale = camera_query.get_single().map(|p| p.scale).unwrap_or(1.0);

        // 3. delta を world 単位に直して適用（y は反転）
        transform.translation.x += drag.event().delta.x * scale;
        transform.translation.y -= drag.event().delta.y * scale;
    },
)
```

## Z-order を最前面に持ってくる

```rust
.observe(
    |trigger: Trigger<Pointer<Down>>,
     mut query: Query<&mut Transform, With<WindowRoot>>,
     mut wm: ResMut<WindowManager>| {
        wm.max_z += 2.0;
        if let Ok(mut transform) = query.get_mut(trigger.entity()) {
            transform.translation.z = 10.0 + wm.max_z;
        }
    },
)
```

ベース z=10 に `max_z` を加算する。`+= 2.0` は内部ノードと衝突しないための余裕。

## Picking backend

`Cargo.toml` で `bevy_picking` と `bevy_sprite_picking_backend` を有効化してある。
これにより `Sprite` を持つ entity は自動的に pickable になる（明示的な `Pickable`
component 不要）。透明度を考慮した hit test も標準で動く。

UI 側（`Node` ベースの UI、本プロジェクトでは使っていない）は別 backend が必要。

## トラブル: observe しても呼ばれない

- Sprite ではなく `Transform` 単独 entity に `observe` を貼ってもクリックは取れない
  → クリック対象には Sprite（または Mesh + Material）が必要
- Z 順で背後にいる: 親が `max_z` で前に来ているか確認
- 親で `Pointer<Down>` を握っているとイベントが伝播しない場合あり
  → bubbling は 0.15 でデフォルト有効、`event.propagate(false)` で止められる
