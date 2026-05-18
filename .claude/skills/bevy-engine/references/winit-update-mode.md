# WinitSettings と Idle CPU — Bevy 0.15

## 問題: デフォルトは「ゲーム想定」で常時最大 FPS で描く

Bevy 0.15 の `DefaultPlugins` は何も指定しないと **`WinitSettings::game()`** 相当で動く:

- `focused_mode: UpdateMode::Continuous` — フォーカス時は **vsync 上限まで描き続ける**
- `unfocused_mode: UpdateMode::reactive_low_power(1/60s)`

trading dashboard のような「待機が長いデスクトップ UI」だと、これだけで **アイドル中も常時 50–60% / 1 コア** を食う。
本プロジェクトでは 90Hz モニタで `frame_time ≈ 11ms` × 90fps = フレーム予算の 99% を消費していた（2026-05-18 計測）。

## 対策: reactive mode に切り替える

`src/main.rs` の `App::new()` に **1 行** insert_resource するだけで桁で削れる:

```rust
.insert_resource(bevy::winit::WinitSettings {
    focused_mode: bevy::winit::UpdateMode::reactive(std::time::Duration::from_millis(200)),
    unfocused_mode: bevy::winit::UpdateMode::reactive_low_power(std::time::Duration::from_secs(2)),
})
```

実測 (release, 90Hz モニタ, idle):

| 設定 | CPU / 1 core | 体感 |
|---|---:|---|
| デフォルト (Continuous) | 59% | 重い |
| `WinitSettings::desktop_app()` (5s/60s) | 6.2% | UI 反映が最大 5 秒遅れる → trading 用途では NG |
| **reactive(200ms) / reactive_low_power(2s)** | **4.7%** | UI 反映 200ms 遅延（許容） |

## desktop_app() を使わない理由

`WinitSettings::desktop_app()` は焦点 5 秒 / 非焦点 60 秒。`mpsc::try_recv()` で backend を drain する設計 (`backend_update_system`, `status_update_system`) と組み合わせると、**最大 5 秒 backend 反映が遅延** する。trading UI には粗すぎる。

## チェック: 自分のシステムが reactive モードで動くか

`UpdateMode::Reactive { wait, .. }` は以下で wake する:

1. winit input event (mouse / key / window event)
2. wait 時間経過
3. `RequestRedraw` event の発火
4. `EventLoopProxy::send_event(WakeUp)` (外部スレッドから)

backend からの mpsc push は **どれにも該当しない** ので、wait 時間経過まで反映されない。対策は次のどれか:

- **A.** wait を短く (今回採用、200ms)
- **B.** `EventLoopProxy<bevy::winit::WakeUp>` を `Res` で取り、tokio task から `send_event` する (確実だが侵襲的)
- **C.** 動きが必要な期間 (RunState::Running 中など) だけ `WinitSettings` を一時的に Continuous に差し替える

## アニメーションが止まる罠

`Time<Real>` を読んで「毎フレーム更新」前提のアニメーション (例: `animate_replay_startup_bar_system`,
loading spinner) は、reactive モードだと wait 時間ごとにしか進まない。

- wait=200ms なら 5fps 相当でカクつくが「進む」
- wait=5s だと「ほぼ停止」して見える

長いアニメ中は **`commands.queue(...)` で `WinitSettings` を一時 Continuous に上書き** するか、wait
を短くする。

## 計測手順

```rust
// 一時挿入: アイドル CPU/FPS を実測する
.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
.add_plugins(bevy::diagnostic::LogDiagnosticsPlugin::default())
```

```powershell
# プロセス CPU の伸びを wall-clock で割って 1 コア比を出す
Get-Process -Id <pid> | Select-Object CPU
# 60 秒後の CPU 差分 / 60 = % of 1 core
```

`LogDiagnosticsPlugin` 自体が「毎秒ログ出力 = 毎秒 update 強制」になるので、最終確認時は必ず外して計測する。

## 関連

- `WinitSettings::game()` / `desktop_app()` / `mobile()` のプリセット定義: `bevy_winit/src/winit_config.rs:31` (0.15.1)
- `UpdateMode::reactive(wait)` / `reactive_low_power(wait)` ヘルパー: 同 `:102` `:117`
- 本プロジェクトでの採用例: `src/main.rs:50-` (2026-05-18 追加)
