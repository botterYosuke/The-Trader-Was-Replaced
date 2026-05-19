# ExecutionMode 連動の UI 可視性ルール — 修正計画書

## 目的

`ExecutionMode` 切替に応じて Footer の transport / speed ボタン群と Sidebar の Startup
パネルの可視性 (`Node.display`) を切り替え、加えて入力系 system に `ExecutionMode` gate を
入れて二重防御する。バックエンド呼び出しが意味を持たない（または禁止される）コントロールを
物理的に隠した上で、万一隠れたボタンに Pressed が届いても backend に command が送られない
ことを保証する。

UX (可視性) と安全策 (入力 gate) を **別レイヤとして同時に実装する** のが本計画の方針。

## スコープ判断 (前回レビュー反映)

- **LiveAuto でも PauseResume は隠す**: 現行 `footer_pause_resume_system` (footer.rs:556)
  は `data.replay_state` を見て `TransportCommand::Pause/Resume` または
  `StrategyRunRequested` event (Replay run flow) を発火する。これは Replay 専用ハンドラ
  であり、LiveAuto で押されると Replay 用 command が backend に送られて目的（「意味を持た
  ないコントロールを隠す」）と矛盾する。Live 用 start/stop/pause セマンティクスは未定義
  なので、今回はボタン自体を隠す方針で確定する。
- **Live 用 start/stop UI は別計画**: LiveAuto / LiveManual 用の strategy 起動/停止 UI が
  必要になった場合は、`TransportCommand::LiveStart` 等の新 wire と新 entity を別途設計
  する。本計画では一切触れない。

## 仕様

### 可視性マトリクス

| 要素 (marker) | Replay | LiveManual | LiveAuto |
|---|:---:|:---:|:---:|
| `TransportButton::JumpToStart` (`\|<`) | ✓ | ✗ | ✗ |
| `TransportButton::StepBack` (`<`) | ✓ | ✗ | ✗ |
| `TransportButton::PauseResume` (`▶/\|\|`) | ✓ | ✗ | ✗ |
| `TransportButton::StepForward` (`>`) | ✓ | ✗ | ✗ |
| `TransportButton::ForceStop` (`■`) | ✓ | ✗ | ✗ |
| `SpeedButton(1..50)` (`1x..50x`) | ✓ | ✗ | ✗ |
| `ScenarioStartupPanelRoot` (sidebar の "Startup") | ✓ | ✗ | ✗ |

実質「Replay のみ全部表示、それ以外は全部非表示」。実装も同じ単純さに収まる。

### 補足
- 非表示は **`Node.display = Display::None`** で行う（レイアウト上もスペースを取らない）。
  `Visibility` ではなく `Node.display` を使う理由: bevy_ui の Node ツリーで flex spacer
  (`flex_grow: 1.0`) を含むレイアウトのため、レイアウトから外す必要がある。`Visibility`
  は描画のみ抑止で Node のサイズは残る。
- 既存の **enabled/disabled の alpha 表現**（`PauseResumeLabel` の `BUTTON_DISABLED_ALPHA`,
  `footer.rs:428-444`）はそのまま残す。Replay 中は従来通り。
- 既存の **ExecutionMode トグル selected ハイライト** (`footer.rs:382-394`) はそのまま。

### 入力 gate (二重防御)

`Display::None` を翌フレーム以降に適用しても、**Interaction は PreUpdate の
`ui_focus_system` で確定する** ため、可視性 system (Update) を input system より前に
動かしても「同一フレーム内」の race は順序制約では塞げない。実効的な防御は input
system 側の早期 return のみ。順序制約は構造的な意図表明にとどまる。

ガード対象となる隙間:
- テストや programmatic interaction で `Interaction::Pressed` を直書きされたケース
  （対象 3 system は `Changed<Interaction>` で filter しているため、前フレーム由来の据え置き
  Pressed は元々通らない。実害があるのは「同一フレームで Pressed への変化を直接書き込む」
  経路のみ）
- 将来 LiveAuto/LiveManual で「同じ entity を別目的で再利用」する誘惑への抑止
- 将来 `Changed<Interaction>` filter を緩める／外す変更が入ったときの保険

対象 system:
- `transport_button_system` (footer.rs:449)
- `footer_pause_resume_system` (footer.rs:556)
- `speed_button_system` (footer.rs:500)

いずれも `exec_mode: Res<ExecutionModeRes>` を追加するが、**関数頭の early return は使わない**。
理由: 関数頭で return すると `Interaction::Hovered` / `Interaction::None` 時の
`BackgroundColor` 復元（`transport_button_system` の `bg.0 = BTN_HOVER/BTN_NORMAL`
arm: footer.rs:493-494、`speed_button_system`: footer.rs:515-523、`footer_pause_resume_system`:
footer.rs:616-617）まで止まる。Live 中に hover/press 色のまま `Display::None` になり、
Replay に戻したとき stale な BackgroundColor が再表示される。

正しい gate 位置は **`Interaction::Pressed` arm 内、副作用（`sender.tx.send` / `run_events.send`
/ `speed.current = *mult`）の手前** で `if !matches!(exec_mode.mode, ExecutionMode::Replay) { ... }`
を入れて command/event/state 更新だけスキップする。`bg.0 = BTN_PRESSED` 自体は通してよい
（次フレームの Hovered/None で正規化される）。

具体的なパッチ位置:
- `transport_button_system` (footer.rs:463): `Interaction::Pressed =>` の `bg.0 = BTN_PRESSED;`
  の **直後** に `if !matches!(exec_mode.mode, ExecutionMode::Replay) { continue; }`。
- `speed_button_system` (footer.rs:510): 同様に `bg.0 = BTN_SPEED_SELECTED;` を **送信前** に
  動かすか、Replay 以外なら `speed.current` 更新と `sender.tx.send` だけスキップ。
  推奨は `Pressed` arm 内に `if !matches!(exec_mode.mode, ExecutionMode::Replay) { continue; }`
  を入れて両方止め、`bg.0` の Pressed/Hover/None 更新は arm 構造で従来通り処理されるよう
  早期 continue より前に置く。
- `footer_pause_resume_system` (footer.rs:571): `bg.0 = BTN_PRESSED;` の直後に
  `if !matches!(exec_mode.mode, ExecutionMode::Replay) { continue; }`。これで Pause/Resume/
  Run flow 全部止まり、Hovered/None の bg 更新は通る。

## 修正対象ファイル

### 1. `src/ui/footer.rs`

#### 追加: `apply_execution_mode_visibility_system`

```rust
/// ExecutionMode に応じて transport / speed ボタンの `Node.display` を切り替える。
///
/// 可視性ルール:
/// - Replay: 全 transport + speed を表示
/// - LiveManual / LiveAuto: 全 transport + speed を非表示
///
/// 起動初回は `ExecutionModeRes::is_changed()` が true なので初期 mode に応じた
/// レイアウトが 1 フレーム目で確定する。
#[allow(clippy::type_complexity)]
pub fn apply_execution_mode_visibility_system(
    exec_mode: Res<ExecutionModeRes>,
    mut transport_q: Query<&mut Node, (With<TransportButton>, Without<SpeedButton>)>,
    mut speed_q: Query<&mut Node, (With<SpeedButton>, Without<TransportButton>)>,
) {
    if !exec_mode.is_changed() {
        return;
    }
    let target = if matches!(exec_mode.mode, ExecutionMode::Replay) {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut transport_q {
        if node.display != target {
            node.display = target;
        }
    }
    for mut node in &mut speed_q {
        if node.display != target {
            node.display = target;
        }
    }
}
```

#### 確認事項
- PauseResume button は spawn 時に `TransportButton::PauseResume` と `PauseResumeButton`
  の 2 つを同時に付与しているので、`With<TransportButton>` の Query に正しくヒットする
  (`footer.rs:145-157`)。
- Speed ボタンと TransportButton は別 entity（`spawn_speed_btn` と `spawn_transport_btn`
  が独立 spawn）なので `Without` で disjoint を明示しておけば borrow checker も問題なし。

#### 修正: 入力 system に `ExecutionMode` gate を追加

`transport_button_system`, `footer_pause_resume_system`, `speed_button_system` の 3 つに
以下の変更を入れる:

```rust
pub fn transport_button_system(
    mut query: Query<...>,
    data: Res<TradingData>,
    sender: Res<TransportCommandSender>,
    exec_mode: Res<ExecutionModeRes>,   // ← 追加
) {
    for (interaction, mut bg, action) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                // ← gate はここ。Hovered/None 側の bg 更新は塞がない
                if !matches!(exec_mode.mode, ExecutionMode::Replay) {
                    continue;
                }
                let replay = data.replay_state.as_deref().unwrap_or("IDLE");
                match action { /* 既存ロジックそのまま */ }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,   // 通す
            Interaction::None => bg.0 = BTN_NORMAL,     // 通す
        }
    }
}
```

`footer_pause_resume_system` と `speed_button_system` も同じパターン（`Pressed` arm 内
かつ `bg.0 = BTN_PRESSED;` の直後に `if !matches!(...) { continue; }`）。

理由: `Display::None` で `Interaction` 経路は塞がるが、テストや将来の programmatic
interaction、`Changed<Interaction>` filter を将来緩めた場合への安全策。**Pressed arm 内
gate は backend へ送る command / event 経路だけを塞ぎ、Hovered/None の bg 復元は通す**
ので、Live → Replay 復帰時の stale 色問題が起きない。

### 2. `src/ui/scenario_startup_panel.rs`

#### 追加: `apply_startup_panel_visibility_system`

```rust
/// Replay 以外のモードでは "Startup" パネル全体を非表示にする。
///
/// パネル本体は `spawn_scenario_startup_panel` で常に spawn 済み。
/// `Node.display` で表示/非表示を切り替えるだけ。
pub fn apply_startup_panel_visibility_system(
    exec_mode: Res<crate::trading::ExecutionModeRes>,
    mut panel_q: Query<&mut Node, With<ScenarioStartupPanelRoot>>,
) {
    if !exec_mode.is_changed() {
        return;
    }
    let target = if matches!(exec_mode.mode, crate::trading::ExecutionMode::Replay) {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut panel_q {
        if node.display != target {
            node.display = target;
        }
    }
}
```

### 3. `src/ui/mod.rs`

`UiPlugin::build` の Update スケジュールに 2 system を追加する。現在の `add_systems(Update, ...)`
ブロック (mod.rs:152-176) は 18 system 入っており、ここに 2 つ足すと丁度 20 上限 — マージン
ゼロは将来の追加で詰むので、**末尾に独立した `add_systems(Update, (..., ...))` 呼び出しを
追加する**（既存ブロック内の chain ordering と無関係に並べられる）。

`Display::None` 切替の発生は backend からの polling diff 経由なので、その反映フレームに
input system が走ったとしても、防御は input 側の早期 return が担う（§入力 gate 参照）。
したがって `.before(input)` 制約はあっても無くても挙動は同じ。可読性のため付けるなら以下、
不要と判断するなら制約なしのフラットな並列で OK:

```rust
// オプション A: 意図表明として .before を付ける（race-prevention ではなく構造化）
.add_systems(
    Update,
    (
        crate::ui::footer::apply_execution_mode_visibility_system
            .before(crate::ui::footer::transport_button_system)
            .before(crate::ui::footer::footer_pause_resume_system)
            .before(crate::ui::footer::speed_button_system),
        crate::ui::scenario_startup_panel::apply_startup_panel_visibility_system,
    ),
)

// オプション B: フラット（最小構成）
.add_systems(
    Update,
    (
        crate::ui::footer::apply_execution_mode_visibility_system,
        crate::ui::scenario_startup_panel::apply_startup_panel_visibility_system,
    ),
)
```

本計画ではオプション B を採用する（理由: 順序制約は実害ゼロだが、コメントなしに `.before`
が並ぶと将来読者に「ここで race を防いでいる」と誤読される。意図は §入力 gate のテキストで
明示済み）。

`apply_startup_panel_visibility_system` は sidebar 入力 system と直接 race しない。
将来必要になれば後付けで `.before(panel_button_system)` を追加できる。

## テスト計画 (`#[cfg(test)] mod tests`)

### `src/ui/footer.rs`

#### 可視性テスト

| テスト名 | 検証内容 |
|---|---|
| `transport_buttons_visible_in_replay` | `ExecutionMode::Replay` で 5 つの transport ボタン全てが `Display::Flex`。`Node::default().display` は Flex のため初期値のままでも assert は通るが、可視性 system が **Flex を維持する** 振る舞いを期待値として固定する |
| `transport_buttons_flip_back_to_flex_on_replay_return` | LiveAuto で `Display::None` に書き込まれた後、Replay へ戻すと visibility system が `Display::Flex` に **書き戻す** ことを厳密に検証（system が Flex を書く側であることの正例） |
| `transport_buttons_hidden_in_manual` | `ExecutionMode::LiveManual` で 5 つ全てが `Display::None` |
| `transport_buttons_hidden_in_auto` | `ExecutionMode::LiveAuto` で 5 つ全てが `Display::None`（PauseResume 含む） |
| `speed_buttons_visible_only_in_replay` | Replay → `Display::Flex`、Manual/Auto → `Display::None`（5 ボタン全部） |
| `mode_switch_toggles_display` | Replay→Manual→Auto→Replay と遷移させて各段階で期待状態 |
| `system_skips_when_mode_unchanged` | mode 変更なしの 2 回目の update で Node が touch されない（`is_changed()` で再 trigger しない） |

#### 入力 gate negative テスト (新規)

| テスト名 | 検証内容 |
|---|---|
| `transport_command_not_sent_in_manual` | `LiveManual` で transport ボタンに `Interaction::Pressed` を直書きしても `TransportCommandSender` に何も流れない（receiver 側を `try_recv` で empty 確認） |
| `transport_command_not_sent_in_auto` | `LiveAuto` でも同様 |
| `pause_resume_does_not_emit_run_event_in_manual` | `LiveManual` で `PauseResumeButton` に Pressed を入れても `StrategyRunRequested` event が 0 件 |
| `pause_resume_does_not_emit_run_event_in_auto` | `LiveAuto` でも同様 |
| `speed_command_not_sent_in_live` | `LiveManual` / `LiveAuto` で `SpeedButton` Pressed → `TransportCommand::SetSpeed` が送られない、かつ `ReplaySpeed.current` が変わらない |
| `transport_command_sent_in_replay_smoke` | Replay モードで `JumpToStart` Pressed → `TransportCommand::ForceStop` が 1 件流れる（gate が誤って Replay も塞いでいないことの positive sanity check） |

#### テストのセットアップ

**可視性テスト用 App** — `apply_execution_mode_visibility_system` 単体で十分。

```rust
fn make_visibility_app() -> App {
    let mut app = App::new();
    app.init_resource::<ExecutionModeRes>(); // default = Replay
    app.add_systems(Update, apply_execution_mode_visibility_system);
    app
}

fn spawn_transport(app: &mut App, kind: TransportButton) -> Entity {
    // visibility system は With<TransportButton> でしか filter しないので Node のみで OK
    app.world_mut().spawn((Node::default(), kind)).id()
}

#[test]
fn transport_buttons_hidden_in_auto() {
    let mut app = make_visibility_app();
    let jump = spawn_transport(&mut app, TransportButton::JumpToStart);
    let pause = spawn_transport(&mut app, TransportButton::PauseResume);
    let stop = spawn_transport(&mut app, TransportButton::ForceStop);

    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    app.update();

    for e in [jump, pause, stop] {
        assert_eq!(
            app.world().entity(e).get::<Node>().unwrap().display,
            Display::None,
        );
    }
}
```

**`system_skips_when_mode_unchanged` の検出パターン**: 初回 update 後に Node.display を
手動で wrong 値（例: `Display::None` を Replay 中に書き込む）に上書きし、mode 据え置きで
もう一度 `app.update()` を回し、その wrong 値が **そのまま残っている**（visibility system
が `is_changed()` false で early-return した）ことを assert する。

```rust
#[test]
fn system_skips_when_mode_unchanged() {
    let mut app = make_visibility_app();
    let e = spawn_transport(&mut app, TransportButton::JumpToStart);
    app.update(); // 1 回目: is_changed=true で Display::Flex 確定
    // 強制的に wrong 値を書き込む
    app.world_mut().entity_mut(e).get_mut::<Node>().unwrap().display = Display::None;
    app.update(); // 2 回目: ExecutionModeRes 不変 → early-return → 書き戻されない
    assert_eq!(
        app.world().entity(e).get::<Node>().unwrap().display,
        Display::None,
        "system must skip when ExecutionModeRes is unchanged",
    );
}
```

**入力 gate negative テスト用 App** — 該当 system のフル依存を init する必要がある。
レシピは以下:

```rust
use tokio::sync::mpsc;

fn make_input_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let mut app = App::new();
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();
    app.insert_resource(TransportCommandSender { tx })
        .init_resource::<ExecutionModeRes>()         // default Replay
        .init_resource::<TradingData>()
        .init_resource::<ReplaySpeed>()
        .init_resource::<StrategyBuffer>()
        .init_resource::<LastRunResult>()
        .init_resource::<StrategyAutoSaveState>()
        .add_event::<StrategyRunRequested>();
    app.add_systems(Update, (
        transport_button_system,
        footer_pause_resume_system,
        speed_button_system,
    ));
    (app, rx)
}

fn spawn_pressed_transport(app: &mut App, kind: TransportButton) -> Entity {
    // 入力 system は With<Button> + Changed<Interaction> で filter。
    // PauseResume だけは PauseResumeButton マーカー併用。
    app.world_mut()
        .spawn((Node::default(), Button, Interaction::Pressed, kind))
        .id()
}
```

PauseResume 用は `PauseResumeButton` も併せて insert。

**negative テストの RED 担保**: gate を入れずに対象 system を回したとき、テストが他の理由で
「何も送られない」状態にならないよう、各 system が **command を送る／event を emit する手前まで
進む** 入力を作る必要がある。これをサボると gate を外しても green のままで RED-1 が成立せず、
gate の検証ができない。

| テスト | system が gate 無しで実行する分岐 | 必須セットアップ |
|---|---|---|
| `transport_command_not_sent_in_manual` / `_in_auto` | `JumpToStart` arm (footer.rs:478) は `replay_state ∈ {RUNNING,PAUSED,LOADED}` のみ send | `data.replay_state = Some("RUNNING".into())` |
| `pause_resume_does_not_emit_run_event_in_manual` / `_in_auto` | Run flow (footer.rs:586-613) は `replay_state` が `RUNNING`/`PAUSED` 以外 **かつ** `flush_strategy_cache` が `Ok(true)` を返す（= `cache_path` が `Some` で書き込み成功）ときだけ event を emit | `data.replay_state = Some("IDLE".into())` + `buffer.cache_path = Some(tmp_file_path)`（**ファイルパス**。下記注を参照） |
| `speed_command_not_sent_in_live` | `speed_button_system` は state 依存なしで常に send | 追加設定不要 |
| `transport_command_sent_in_replay_smoke` | 同上 (JumpToStart arm) | `data.replay_state = Some("RUNNING".into())` |

PauseResume 系の `cache_path` には **tempdir 配下のファイルパス** を渡す:

```rust
let tmp = tempfile::tempdir().unwrap();
let cache_file = tmp.path().join("strategy.py");
// std::fs::File::create(&cache_file).unwrap();  // 空ファイルでも flush_strategy_cache は overwrite するので不要
app.world_mut().resource_mut::<StrategyBuffer>().cache_path = Some(cache_file);
// tmp は test 関数末尾まで生存させる (drop で dir ごと削除)
```

注意点:
- `tempfile::tempdir()` 単体だと **ディレクトリ** を返す。`std::fs::write(path, ...)` に
  ディレクトリパスを渡すと OS が `Err(IsADirectory)` を返し、`flush_strategy_cache` は
  `Err` を返し、`footer_pause_resume_system` は `Err(e) => continue` (footer.rs:603) に
  落ちて event を emit しないまま終わる。**この状態で RED-1 を回すと gate 無しでも
  test が pass してしまい TDD signal が壊れる**。必ず tempdir 配下の **ファイル名を
  join したパス** を `cache_path` に入れること。
- `TempDir` は drop されると配下ファイルごと削除されるので、`tmp` は assert 行以降まで
  生存させる（途中で `drop(tmp)` しない、return しない）。
- gate ありの GREEN-1 時には early-return で `flush_strategy_cache` 自体が呼ばれないので、
  ファイルが作られなくても問題ない。RED-1（gate 無し）時にだけ実際にファイルが書かれる。

**StrategyRunRequested 検証**: 本プロジェクトの慣用に揃え、`Events<T>::get_cursor()` →
`reader.read(events)` で件数を数える（scenario_parser.rs:422-424 と同パターン）:

```rust
let events = app.world().resource::<Events<StrategyRunRequested>>();
let mut reader = events.get_cursor();
assert_eq!(reader.read(events).count(), 0);
```

`TransportCommandSender` 経路は `rx.try_recv()` が `Err(TryRecvError::Empty)` を返すことを assert。

### `src/ui/scenario_startup_panel.rs`

| テスト名 | 検証内容 |
|---|---|
| `startup_panel_visible_in_replay` | Replay で `Display::Flex` |
| `startup_panel_hidden_in_live_manual` | LiveManual で `Display::None` |
| `startup_panel_hidden_in_live_auto` | LiveAuto で `Display::None` |
| `mode_switch_toggles_panel_display` | Replay → LiveManual → Replay で flex / none / flex |

## 実装順 (TDD)

17 テスト（footer gate 6 + footer visibility 7 + startup panel 4）を一括追加すると、
step 2 時点で `apply_execution_mode_visibility_system` 未定義のためコンパイル不能 →
`cargo test` 全体が走らない。**RED→GREEN を「gate 系」「footer visibility 系」「startup
panel 系」で 3 回に分割する**:

1. **RED-1 (gate)**: `footer.rs::tests` に入力 gate negative 6 テストのみ追加（§テストの
   セットアップ「negative テストの RED 担保」表に従って `replay_state` / `cache_path` を
   明示的に仕込む）。gate 未実装なので receiver にコマンドが流れて／event が emit されて RED。
2. **GREEN-1 (gate)**: `transport_button_system` / `footer_pause_resume_system` /
   `speed_button_system` の 3 つに `exec_mode: Res<ExecutionModeRes>` + 早期 return を
   追加。`cargo test -p backcast footer::tests` で 6 テスト green。
3. **RED-2 (visibility)**: 可視性 7 テストを追加。`apply_execution_mode_visibility_system`
   が未定義のため compile fail (RED)。
4. **GREEN-2 (visibility)**: `footer.rs` に `apply_execution_mode_visibility_system` を実装。
   可視性 7 テスト green。
5. **REGISTER-1 (footer)**: `mod.rs` 末尾に独立 `add_systems(Update, (...))` ブロックを 1 つ
   追加し `apply_execution_mode_visibility_system` のみ登録（オプション B フラット構成、
   この時点ではタプルでなく単独 system でも可）。既存ブロック (mod.rs:152-176, 現 18
   system) を膨らませずタプル 20 上限のマージンを残す。
6. **RED-3 (startup panel)**: `scenario_startup_panel.rs::tests` に 4 テスト追加。
   `apply_startup_panel_visibility_system` 未定義のため compile fail (RED)。
7. **GREEN-3 (startup panel)**: `apply_startup_panel_visibility_system` を追加。step 5 で
   作った Update ブロックを `(apply_execution_mode_visibility_system,
   apply_startup_panel_visibility_system)` のタプルに拡張して登録。startup panel 4 テスト green。
8. **E2E**: `cargo run` で起動 → Footer の Replay / Manual / Auto セグメントを順にクリック
   し、以下を目視確認:
   - Replay 時: `|< < ▶ > ■  1x 2x 5x 10x 50x` 全部表示、Sidebar 下部に "Startup" パネル
   - Manual 時: transport / speed が全部消える、"Startup" パネル消える
   - Auto 時: transport / speed が全部消える（PauseResume も消える）、"Startup" パネル消える
   - Sidebar が flex layout で詰まっていることを確認（"Startup" 消失後に下のパネルが
     上に詰まる）

## 影響範囲・回帰リスク

- **変更すること**:
  - `transport_button_system` / `footer_pause_resume_system` / `speed_button_system` の
    シグネチャに `exec_mode: Res<ExecutionModeRes>` を追加し、**`Interaction::Pressed` arm 内
    `bg.0 = BTN_PRESSED;` の直後** で Replay 以外なら `continue;` する。
    Hovered/None の bg 復元は通すので、Live → Replay 復帰時の stale 色問題は起きない。
    既存ロジックは Replay モードでは無変更。
  - `mod.rs` に visibility 2 system を追加（フラット配置、`.before` 制約なし — §mod.rs 参照）。
- **影響しないこと**:
  - `ExecutionModeRes` の更新経路（backend diff 経由のみ）に変更なし。
  - Replay モードでの transport / speed / PauseResume の挙動は完全に不変。
  - Footer の `time:` ラベル、`state:` ラベル、`Venue:`, `grpc:` 表示は無変更。
  - `execution_mode_toggle_system` (mode 切替ボタン本体) は変更なし。
  - `update_speed_buttons_system` は不変（hidden SpeedButton の BackgroundColor を更新する
    無駄処理が残るが、`Display::None` 配下なので視覚的影響ゼロ。最適化は別計画）。
- **回帰チェック対象**:
  - footer.rs には現在 `#[cfg(test)] mod tests` が存在しないため、既存テストの移行作業は無し。
    今回新規追加するテスト側で `app.init_resource::<ExecutionModeRes>()`（および入力 system
    依存リソース群）を明示するだけで足りる。
  - `scenario_startup_panel.rs` の既存テストは新 system を組み込まなければ `ExecutionModeRes`
    を必要としない（UiPlugin 経由のフル App を構築するテストが現状無いため）。新 system を
    使う新規テストだけが `ExecutionModeRes` の init を必要とする。
- **Phase 8 §3.5.1 のトグル仕様**: ユーザが Manual / Auto をクリックして遷移する経路は
  変更なし。トグル自体は常に 3 つとも表示される。

## 非対象（今回触らない）

- Footer の `time:` 表示、`state:` バッジ、Venue / gRPC バッジ。
- Instrument Picker (`[+ Add]` dropdown) の mode 依存挙動（既に実装済み）。
- Universe 自動プルーニング (`instruments_universe_prune.rs`、既に実装済み）。
- Menu Bar の `File → Open / New` 副作用（既に実装済み）。
- `LiveManual` vs `LiveAuto` の File → Open 自動遷移先 (`LiveAuto` 固定) 仕様。
- **Live 用 start/stop/pause UI**: LiveAuto/LiveManual で strategy を起動・停止する UI が
  必要なら、別計画で `TransportCommand::LiveStart` 等の wire と新 entity を設計する。
  本計画は「不要なものを隠す」のみで「新しい操作を作る」ことはしない。

## オープン課題

- **トグルセグメント自体の hover / focus 状態**: 3 セグメントは常時表示で変更しない。
- **Replay panel ヘッダ "Startup" の文字サイズ等**: 既存仕様維持。
