# The-Trader-Was-Replaced

Bevy デスクトップ UI と Python gRPC バックエンドからなる、リプレイ／ライブの取引戦略実行アプリ。この用語集は、オンキャンバスの **UI ウィンドウ** と **銘柄ユニバース** 周辺の語彙を一意に保つためのもの。

## Language

### UI Windowing

The trader dashboard renders an infinite canvas of windows over a Bevy app. これらはオンキャンバスのウィンドウと Startup パラメータフォームの語彙。

**Floating window**:
A draggable, z-ordered window over the canvas. Two hosting flavours coexist
(ADR 0003):
- **world-space *sprite* window** built by `spawn_floating_window` — Chart,
  Buying Power, Run Result, Positions, Orders. Drag/zoom follow the PanCam camera.
- **screen-space *Bevy UI Node* window** built by `spawn_screen_window` — the
  **Strategy Editor** and the **Startup** window, because their editable text uses
  `bevy_ui_text_input` (`TextInputNode`), which is screen-space only. Position is a
  `Node` `left`/`top` in logical pixels; the title bar drag moves those.
Editable text is hosted in screen space (the Strategy Editor and Startup fields
use `TextInputNode`); display-only panels stay world-space sprites.
_Avoid_: panel, dialog.

**Startup window**:
The form for configuring a replay run — Start date, End date, Granularity, and
Initial cash. A screen-space `spawn_screen_window` floating window (its date/cash
fields are `TextInputNode`s) with two deliberate departures from the others:
it has **no close button**, and it is **shown only in Replay mode** (its
visibility is owned by `ExecutionMode`, not by the user or a sidebar button).
_Avoid_: Startup panel, scenario panel, run config dialog.

**Title bar**:
The drag region every floating window shares — a sprite for world-space windows
(`spawn_floating_window`) or a `Node` for screen-space windows (`spawn_screen_window`);
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

### Flagged ambiguities

- **"universe" の overload**: コードでは `instruments_universe_prune.rs` / `auto_fetch_live_universe` がモードの候補集合（U1）の意味で、`UNIVERSE_JSON_PATH` / `strategy_runtime/universe.py` がストラテジー設定（U2）の意味で使っている。**U1 = Universe、U2 = Strategy Universe** に分離して解決。
- **同一役割の二面性**: Replay の **Available Instruments** と Live の **Tickers** は「Universe を保持する resource」という同じ役割を 2 モードで担う。共通の上位語は **Universe**。

## Example dialogues

### UI Windowing

> **Dev:** Should the Startup window get a close button like the other windows?
> **Expert:** No — it's the one floating window without one. Replay mode owns
> when it shows; the user drags it but can't dismiss it. Closing it would strand
> the only way to configure a replay run.
> **Dev:** But it's built the same way as Buying Power?
> **Expert:** Almost — it's a *screen-space* window (`spawn_screen_window`) rather
> than a world-space sprite, because its date/cash fields are `TextInputNode`s,
> hosted in screen space exactly like the Strategy Editor's editable text (ADR 0003).

### 銘柄ユニバース

> **Dev**: 「ユニバースが取れない」ってバグ報告、どっちのユニバース？
> **Domain**: Replay 中にピッカーが空。だから **Universe**（モードの候補）の方。Strategy Universe は無関係。
> **Dev**: scenario.end は入ってる？ なら **Available Instruments** にその end_date のキーが無いか、まだ in_flight のはず。
> **Domain**: 入ってる。バックエンドのログだと **Listed-Symbols Artifact** が miss して、**Catalog** を走査して生成し直してた。
> **Dev**: じゃあ Universe は正しく組み上がる。ユーザーがその後 **Instrument Registry** に追加した銘柄が、Universe から外れて prune された、が真因かも。
> **Domain**: なるほど。candidate（Universe）と selected（Registry）を分けて考えればいいのか。
