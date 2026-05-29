# The-Trader-Was-Replaced

Bevy デスクトップ UI と Python gRPC バックエンドからなる、リプレイ／ライブの取引戦略実行アプリ。この用語集は、オンキャンバスの **UI ウィンドウ** と **銘柄ユニバース** 周辺の語彙を一意に保つためのもの。

## Language

### UI Windowing

The trader dashboard renders an infinite canvas of windows over a Bevy app. これらはオンキャンバスのウィンドウと Startup パラメータフォームの語彙。

**Infinite canvas**:
The product's core workspace model: users arrange trading surfaces as spatial
windows on a pannable/zoomable canvas. This is product identity, not a
replaceable implementation detail; future 3D extension is directional but not
part of the current definition.
_Avoid_: multi-pane layout, dashboard layout, tabbed workspace.

**Floating window**:
A world-space *sprite* window built by `spawn_floating_window` — draggable by its
title bar, z-ordered among other floating windows. Chart, Strategy Editor,
Buying Power, Run Result, Positions, Orders, and Settings are all floating windows,
and the Startup window is built the same way. Most windows' contents are world-space
sprites/`Text2d`; the Strategy Editor's code buffer is the one exception — see
**Strategy Editor (projected overlay)**.
_Avoid_: panel, dialog.

**Strategy Editor (projected overlay)**:
The Strategy Editor's editable code buffer is a Bevy UI `Node` (bevscode's
`CodeEditor`) **projected** over the floating window's world rect each frame:
the editor's `left/top/width/height` and `TextFont.font_size` are recomputed
from the window's world transform and the camera's zoom so the editor follows
pan / zoom / drag and stays crisp at any zoom. The floating window shell
(title bar, × button, resize handles, rim light) remains a world-space sprite
exactly like every other floating window. Two domain consequences flow from
this: (a) the editor content always renders **in front of** other floating
windows (UI is drawn after world sprites), and (b) the editor's world-space
root sprite is pinned to a high baseline `z ≈ 200` so its own affordances
(title bar / × / resize) also stay above the world z-stack. See
[ADR 0006](docs/adr/0006-strategy-editor-projected-ui-overlay.md).
_Avoid_: "embedded editor", "world-space editor" (the buffer is no longer
world-space after issue #50).

**Startup window**:
The form for configuring a replay run — Start date, End date, Granularity, and
Initial cash. A floating window with two deliberate departures from the others:
it has **no close button**, and it is **shown only in Replay mode** (its
visibility is owned by `ExecutionMode`, not by the user or a sidebar button).
_Avoid_: Startup panel, scenario panel, run config dialog.

**Title bar**:
The sprite drag region every floating window shares via `spawn_floating_window`;
also the host for the × close button on windows that have one.
_Avoid_: header.

**Close button (×)**:
The per-window dismiss control on the title bar. Present on every floating
window *except* the Startup window. Suppressing it is a per-window choice.

**Replay mode**:
The `ExecutionMode` in which the dashboard runs a backtest over a date range, as
opposed to LiveManual / LiveAuto. The Startup window exists only here.
_Avoid_: backtest mode, sim mode.

### 銘柄ユニバース

**Universe**:
現在の実行モードで「選べる候補」銘柄の集合。Replay では scenario.end 時点の Listed Symbols、Live では venue の Tickers がその実体。`[+ Add]` ピッカーはこの集合から選ばせる。
_Avoid_: available instruments / listed symbols（単独の同義語としては使わない。下の各レイヤー語を使う）

**Strategy Universe**:
ストラテジーが取引対象として設定している銘柄リスト（`UNIVERSE_JSON_PATH`）。Universe（モードの候補集合）とは別物で、ストラテジー側の設定。
_Avoid_: universe（無修飾。必ず "Strategy" を付ける）

**Listed Symbols**:
ある日付時点で上場している（取引可能な）銘柄。Replay の Universe の実体で、J-Quants 由来の Catalog から導出される。Live モードには存在しない概念。

**Listed-Symbols Artifact**:
ある 1 つの end_date に対する Replay Universe をディスクに永続化した JSON キャッシュ。source of truth は Catalog であり、これはその派生キャッシュ。
_Avoid_: instrument list / symbols file

**Available Instruments**:
取得済みの Replay Universe を end_date でキーして保持するフロント側の resource（`AvailableInstruments`）。Listed-Symbols Artifact をフロントのメモリ上に映したもの。

**Catalog**:
Replay の market data と Listed Symbols の source of truth（Nautilus parquet）。Artifact が無いときはここから走査して Universe を生成する。

#### 境界となる隣接語

**Instrument Registry**:
ユーザーが実際に選択（追加）した銘柄の集合（`InstrumentRegistry`）。Universe が「選べる候補」、Registry が「選んだ結果」という関係。ピッカーはこの 2 つの境界に立つ。
_Avoid_: selected universe / instrument list

**Tickers**:
Live モードで venue から取得した銘柄一覧（`Tickers`）。Live における Universe の実体（Replay の Available Instruments に相当する役割）。

### Strategy Execution

A strategy is launched into a **Run**. The dashboard is always in exactly one
`ExecutionMode`, so exactly one Run is current at a time; the **Run Result** window
describes that one Run.

**Run**:
A single execution of a strategy. Exactly one is *current* at any moment (scoped to
the active `ExecutionMode`). Comes in two kinds — **Replay Run** and **Live Run** —
that share an identity (`run_id`) but differ in lifecycle.
_Avoid_: session, job, backtest (as a synonym for the Run itself).

**Replay Run**:
A Run over a historical date range in Replay mode. **Terminal**: it reaches
*Completed* or *Failed* and stops on its own.
_Avoid_: backtest run, sim run.

**Live Run**:
A Run against a connected venue in Auto mode. **Long-lived**: it stays *RUNNING* /
*PAUSED* until stopped. Capped to one active Run. Run control (start / pause /
resume / stop) lives on the **footer ▶**, not on the Run Result window.
_Avoid_: live session, live strategy (the strategy is what's *run*; the Run is the execution).

**Run Result**:
The single **display-only** floating window that describes the current Run — its
state, identity, P&L / stats, and the Strategy Log. It is **mode-scoped**: a Replay
Run's outcome in Replay mode, the Live Run in Auto mode. It carries **no controls**
(run control is the footer ▶'s job) and is **always visible** (per #41). This is the
*one* run-status surface; there is no separate live panel.
_Avoid_: Live Runs panel, Live Run Panel (abolished — folded into Run Result), "run outcome panel".

**Strategy Log**:
The stream of log lines a strategy emits during its Run (the strategy's own
`self.log.*` output). Shown inside the Run Result window.
_Avoid_: console, output, stdout.

### Flagged ambiguities

- **"Run Result" の意味変更**: 旧来 Run Result は **Replay の結果専用**（terminal な Completed/Failed のみ）だった。今後は **現在の Run 全般**（Replay Run の結果 *および* Live Run の進行・操作・ログ）を担う単一サーフェス。旧「Live Runs / Live Run Panel」は廃止し Run Result に統合する。
- **"universe" の overload**: コードでは `instruments_universe_prune.rs` / `auto_fetch_live_universe` がモードの候補集合（U1）の意味で、`UNIVERSE_JSON_PATH` / `strategy_runtime/universe.py` がストラテジー設定（U2）の意味で使っている。**U1 = Universe、U2 = Strategy Universe** に分離して解決。
- **同一役割の二面性**: Replay の **Available Instruments** と Live の **Tickers** は「Universe を保持する resource」という同じ役割を 2 モードで担う。共通の上位語は **Universe**。

## Example dialogues

### UI Windowing

> **Dev:** Should the Startup window get a close button like the other windows?
> **Expert:** No — it's the one floating window without one. Replay mode owns
> when it shows; the user drags it but can't dismiss it. Closing it would strand
> the only way to configure a replay run.
> **Dev:** But it's built the same way as Buying Power?
> **Expert:** Yes — same `spawn_floating_window`, same title bar. The fields are
> hosted in world space exactly like the Strategy Editor's editable text.

### 銘柄ユニバース

> **Dev**: 「ユニバースが取れない」ってバグ報告、どっちのユニバース？
> **Domain**: Replay 中にピッカーが空。だから **Universe**（モードの候補）の方。Strategy Universe は無関係。
> **Dev**: scenario.end は入ってる？ なら **Available Instruments** にその end_date のキーが無いか、まだ in_flight のはず。
> **Domain**: 入ってる。バックエンドのログだと **Listed-Symbols Artifact** が miss して、**Catalog** を走査して生成し直してた。
> **Dev**: じゃあ Universe は正しく組み上がる。ユーザーがその後 **Instrument Registry** に追加した銘柄が、Universe から外れて prune された、が真因かも。
> **Domain**: なるほど。candidate（Universe）と selected（Registry）を分けて考えればいいのか。
