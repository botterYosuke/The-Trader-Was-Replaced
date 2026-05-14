# ECS Basics（Bevy 0.15 汎用）

プロジェクト固有の流儀は SKILL.md と他リファレンスに分けてある。ここは Bevy ECS 全般。

## Entity / Component / System

- **Entity**: ただの ID（`u64` 相当）。データそのものは持たない。
- **Component**: `#[derive(Component)]` を付けた構造体/enum。Entity に貼る「データ」。
- **System**: `fn(...)` 形式の普通の関数。引数の型で「何が欲しいか」を宣言する。
- **World**: 全 Entity と Component を保持するストア。直接触ることは稀。
- **App**: World + Schedule(s) + Plugins のまとめ役。

## 引数で取れる型（System Param）

| 型 | 意味 |
|---|---|
| `Commands` | spawn/despawn/insert/remove を予約する（バッチ反映） |
| `Query<Q, F>` | 条件 F に合う entity の Q を反復 |
| `Res<T>` / `ResMut<T>` | Resource の不変/可変参照 |
| `Local<T>` | この system だけが持つローカル状態 |
| `EventReader<E>` / `EventWriter<E>` | Event の受信/送信 |
| `Time` | フレーム経過時刻（`Res<Time>` で取る） |
| `Gizmos` | デバッグ描画 |
| `AssetServer` | アセット読み込み |
| `ParamSet<(...)>` | 競合する Query を一つの system に同居させる |

## Query フィルタ

```rust
// Q: 欲しい component
// F: フィルタ
Query<&Transform>                                  // Transform を持つ全 entity
Query<&mut Transform, With<Player>>                // Player を持つ entity の mut Transform
Query<&Transform, Without<Frozen>>                 // Frozen を持たない
Query<(&Transform, &Health)>                       // 複数 component の同時取得
Query<(Entity, &Transform)>                        // entity ID も欲しいとき
Query<&Transform, Or<(With<A>, With<B>)>>          // OR 条件
Query<&Transform, Changed<Transform>>              // 前フレームから変化した entity のみ
Query<&Transform, Added<Transform>>                // 今フレーム追加された entity のみ
```

`Changed` / `Added` は change detection。これを使うとフレーム毎に全件処理しなくて済む。

## 並列実行と排他

- `Query` の引数被りは自動でスケジューラが解決する（同じ component を `&mut` する
  system は並列で走らない、`&` 同士は並列 OK）
- どうしても排他が必要なら `.before(other)` / `.after(other)` / `.chain()`
- `(s1, s2, s3).chain()` は順次、ただのタプル `(s1, s2)` は並列

## Schedule（実行タイミング）

```
First           — 最初
PreUpdate       — 入力処理など
Update          — メインロジック（通常はここ）
PostUpdate      — Transform 伝播、visibility 計算など
Last            — 最後
FixedUpdate     — 固定ステップ（物理など）
Startup         — 起動時 1 回
PreStartup / PostStartup
```

`add_systems(Update, my_sys)` のように schedule label を指定して登録。

## Run condition

```rust
.add_systems(Update, my_sys.run_if(resource_exists::<Foo>))
.add_systems(Update, my_sys.run_if(in_state(GameState::Playing)))
.add_systems(Update, my_sys.run_if(|q: Query<&X>| !q.is_empty()))
```

## Commands は遅延適用

`Commands::spawn` / `despawn` / `insert` は **即時には反映されない**。schedule の
flush 点（system の境目）でまとめて適用される。同じ system 内で spawn → Query
で取得はできない。次の system まで待つ必要がある。

## State

```rust
#[derive(States, Default, Hash, Eq, PartialEq, Clone, Debug)]
enum GameState { #[default] Menu, Playing, Paused }

app.init_state::<GameState>()
   .add_systems(OnEnter(GameState::Playing), setup_game)
   .add_systems(OnExit(GameState::Playing), cleanup_game)
   .add_systems(Update, gameplay.run_if(in_state(GameState::Playing)));

// 遷移: ResMut<NextState<GameState>>.set(GameState::Paused);
```

## Asset

```rust
fn load(asset_server: Res<AssetServer>, mut commands: Commands) {
    let texture: Handle<Image> = asset_server.load("sprites/foo.png");
    commands.spawn((Sprite::from_image(texture), Transform::default()));
}
```

`Handle<T>` は cheap clone な参照カウント。読み込みは非同期で、`AssetEvent<T>` で
完了を観測できる。

## 詳しく知りたいとき

ミラーソース `.claude/skills/bevy-engine/src/examples/ecs/` を当たる:

- `ecs_guide.rs` — 一番網羅的なチュートリアル
- `hierarchy.rs` — 親子関係（**ただし 0.19 では ChildOf**）
- `change_detection.rs`
- `iter_combinations.rs` — 同種 entity の組み合わせ反復
- `system_param.rs` — 自前 SystemParam の作り方
- `system_sets.rs` — system のグルーピング
- `run_conditions.rs`
- `fixed_timestep.rs`
- `observers.rs` — 0.15 で安定化された observer
- `event.rs`
- `state.rs`

**バージョン差注意**: ミラーは 0.19-dev なので、`references/0.15-vs-0.19.md` で
差分を確認しながら読む。
