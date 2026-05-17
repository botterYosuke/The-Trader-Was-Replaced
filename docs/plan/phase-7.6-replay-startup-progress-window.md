# Phase 7.6: Replay Startup Window, Status Correlation, Scenario Parameters, Failure Propagation, and Timeout — Implementation Plan

## Summary

Footer の Run ボタン押下後、Python backend が実際に replay を開始して GUI 上の価格・チャート・Run Result に変化が出るまでに数秒以上の無反応時間がある。ユーザーから見ると「クリックが効いていない」「固まった」ように見えるため、Run command が backend channel に送信できた直後から replay 開始または失敗まで、画面中央に小さな startup progress window を表示する。

本計画で実現する変更は次の通り。

- **Replay startup progress window**: `Starting replay` window を追加し、`Starting replay command...` / `Resetting previous replay...` / `Loading replay data...` / `Starting Python strategy...` / `Waiting for first replay tick...` を段階表示する。実進捗率は backend から得られないため、indeterminate progress bar を使う。
- **startup 専用 UI state**: `ReplayStartupProgress` resource、`ReplayStartupPhase` enum、window marker components を追加し、visible / phase / detail / error / timeout / baseline timestamp / first-tick gate を管理する。
- **Run status correlation**: `TransportCommand::RunStrategy` と `BackendStatusUpdate` に UI transport 内の `startup_id` を通し、前回 Run、旧 replay、古い tokio task の status update が新しい window に混ざらないようにする。
- **backend startup phase events**: `BackendStartupStage` と `BackendStatusUpdate::ReplayStartup { startup_id, stage }` を追加し、`ForceStopReplay` / `LoadReplayData` / `StartEngine` / `WaitingForFirstTick` の段階を UI に伝える。
- **failure propagation**: `LoadReplayData` / `StartEngine` 失敗だけでなく、`ForceStopReplay` 失敗や unknown granularity も silent return せず matching `RunFailed` に変換し、progress window 上に表示する。失敗時は自動で閉じず、Close button で閉じる。
- **send failure handling**: backend channel 未接続や `TransportCommand::RunStrategy` 送信失敗時は progress window を表示しない。window は送信成功後にだけ visible にする。
- **auto-hide hardening**: matching startup の `WaitingForFirstTick` 後に `TradingData.replay_state == "RUNNING"` または timestamp 変化を観測した場合、または matching `RunComplete` を受信した場合だけ閉じる。前回 `Completed` や旧 replay の `RUNNING` では閉じない。
- **startup timeout**: 60 秒の soft timeout を追加し、backend 処理は止めずに UI 上で timeout error を表示する。時計は `Time<Real>` を使い、test で手動更新できるようにする。
- **Run Result Panel immediate running state**: Run command 送信成功時に `LastRunResult.state = Running` へ更新し、既存 Run Result Panel も即時に running state を示す。既存 `BackendStatusUpdate::RunStarted` は互換のため残すが、startup window の判定には使わない。
- **Scenario Startup Parameters Panel (Phase 7.6b)**: Run 前に `scenario.start` / `scenario.end` / `scenario.granularity` / `scenario.initial_cash` を表示・変更できる常時可視 panel を追加する。変更成功時は `ScenarioMetadata` と cache sidecar JSON の該当 4 field を更新し、元の `<strategy>.json` は変更しない。
- **scenario parameter validation and Run blocking**: `start` / `end` は `chrono::NaiveDate` で parse し、`start <= end` を保証する。`granularity` は `Daily` / `Minute`、`initial_cash` は正の整数に限定する。invalid input がある間は Run command を送信せず、progress window も表示しない。
- **implementation surface**: `src/ui/replay_startup_window.rs` と `src/ui/scenario_startup_panel.rs` を追加し、`src/ui/components.rs` / `src/ui/mod.rs` / `src/ui/menu_bar.rs` / `src/main.rs` / `src/trading.rs` を更新する。
- **test coverage**: startup 表示開始、send failure、phase update、`startup_id` mismatch 無視、failure 表示、old `RUNNING` / old `Completed` を拾わないこと、timeout、fast complete、scenario parameter sync / validation / cache writeback / Run blocking を Unit / ECS tests で確認する。

progress window の表示対象は replay 本体の進捗ではなく、**startup phase**、つまり `ForceStop -> LoadReplayData -> StartEngine request accepted / first RUNNING observation` までに限定する。Scenario Startup Parameters Panel は progress window とは別 UI として扱い、Run 前に startup 条件を確認・編集するための 7.6b として同じ計画書内に独立させる。

## Review Status (2026-05-18)

Medium 以上のレビュー指摘は本版で解消済み。

- **Resolved / Medium**: `auto_hide_replay_startup_window_system` の system ordering が未指定だった。同一 frame で `status_update_system` が `start_engine_accepted = true` を書いた後に auto-hide が実行されないとフレーム遅延が起きる。`auto_hide_replay_startup_window_system.after(status_update_system)` を ordering section に追加した(§Systems Ordering 参照)。
- **Resolved / Medium**: `RunComplete` による auto-hide の担当システムが §3 と §5 で矛盾していた。§3 では `status_update_system` が処理すると読め、§5 では polling system の条件として書かれていた。`status_update_system` が matching `RunComplete` を受信したとき直接 `progress.visible = false` と cleanup を実行する設計に一本化した(§3, §5 更新)。
- **Resolved / Medium**: `update_scenario_startup_param_ui_system` の ordering が未指定だった。`scenario_startup_param_input_system` と `auto_hide_replay_startup_window_system` の後に配置し、最新の errors と visible 状態を反映するよう明記した(§Systems Ordering 参照)。
- **Resolved / Medium**: Test 4d の「fake backend」セットアップが未記述だった。`mpsc::channel` で gRPC 呼び出しを持たない ECS テストとして構成する方法を明記した(§Test Plan 4d 参照)。
- **Resolved / Medium**: Scenario Startup Parameters Panel の配置が「Sidebar 下部または Footer 上」と未決だった。**Sidebar 下部**（instrument list の下、Sidebar Panel の内部フッター）に固定した(§UX Specification 参照)。
- **Resolved / Medium**: 既存 strategy の cache sidecar に `scenario.granularity` が設定されていない場合、7.6b 導入後は `errors.granularity` が設定されて Run が block される移行リスクが Risks に未記載だった。Risks に追加した。

## Review Status (2026-05-17)

Medium 以上のレビュー指摘は本版で解消済み。

- **Resolved / Medium**: backend channel 送信失敗時に progress window が表示されたまま残る可能性を排除。window は `TransportCommand::RunStrategy` の送信成功後にだけ visible にする。
- **Resolved / Medium**: 前回 Run / 旧 replay の `RUNNING` や `timestamp_ms` を拾って即 auto-hide する可能性を排除。UI 生成の `startup_id` を transport と status update に通し、matching startup の `WaitingForFirstTick` または `RunComplete` だけで閉じる。
- **Resolved / Medium**: `ForceStopReplay` / unknown granularity の失敗が startup window に出ず沈黙する可能性を排除。startup task の早期 return は必ず matching `RunFailed` を送る。
- **Resolved / Medium**: replay startup window と scenario parameter editor の責務が混ざっていたため、同じ計画書内で **Phase 7.6a startup feedback** と **Phase 7.6b Scenario Startup Parameters Panel** に分離した。
- **Resolved / Medium**: timeout 設計で `Time<Real>` を使う方針と test plan の `Time<Virtual>` が矛盾していたため、`Time<Real>` に統一。
- **Resolved / Medium**: Scenario Startup Panel の入力 UI ライブラリ未指定だった点を解消。既存 strategy_editor と同じ `bevy_cosmic_edit` を採用し、DPI トラップ([cosmic-edit-buffer-metrics-dpi-trap](memory))を踏まないよう CosmicEditBuffer メトリクスは `1.0` 倍で持ち CosmicEditor 側でのみ scale するパターンを再利用する(§Phase 7.6b / Input UI library)。
- **Resolved / Medium**: `GranularityChoice` ↔ string canonical form を `"Daily"` / `"Minute"` に固定し、`ScenarioMetadata.granularity` / cache sidecar JSON / `StrategyRunConfig.granularity` の三者を同一表記で揃えた(§Phase 7.6b / Granularity canonical form)。
- **Resolved / Medium**: 新 `write_startup_params_to_cache_sidecar` と既存 `writeback_scenario_instruments_system` が同一 `cache_sidecar` を read-modify-write する race を排除。startup params 側は独立の `writeback_pending` flag で管理し、`writeback_scenario_instruments_system` の後に最新 JSON を読み直してから scenario 配下 4 field だけを書き換える(§Phase 7.6b / Coexistence with scenario.instruments writeback)。
- **Resolved / Medium**: `TransportCommand::RunStrategy` 送信成功後に初めて window を表示する方針と `Preparing strategy cache...` 初期 phase が矛盾していたため、UI-only 初期 phase を `CommandAccepted` / `Starting replay command...` に変更した。
- **Resolved / Medium**: `ScenarioStartupParams.dirty` が「編集中」と「cache writeback 待ち」を兼ねていたため、`dirty` は UI 編集中の保護だけに限定し、commit 成功時は `writeback_pending = true` を立てる設計に分離した。
- **Resolved / Medium**: `progress.visible` 中の panel disabled enforcement を明文化。視覚的 disabled に加え、input system が `progress.visible == true` の間は commit 自体を skip する。
- **Resolved / Medium**: `scenario_startup_param_input_system` と `parse_scenario_system` の ordering を `.after(parse_scenario_system).before(handle_strategy_run_system)` に統一し、reload と commit の同一 frame 衝突を排除した。
- **Resolved / Medium**: cache JSON writeback の round-trip(`commit -> rewrite -> parse_scenario_system -> ScenarioStartupParams 復元`) を Test Plan #10b に追加した。
- **Resolved / Medium**: 7.6a と 7.6b の出荷単位を明文化。**同一 PR で出す**ことを前提とし、Acceptance Criteria を 7.6a / 7.6b に分節した(§Acceptance Criteria)。

## Phase 0: 現状 RUN 動作確認（実装着手前）

本計画は「Run 押下から replay 開始までの空白」を埋めるものなので、**そもそも現在の Run が最後まで成功している**ことを実装前に確認する。ここで失敗するなら progress window を足しても症状は隠れるだけで本質的には壊れたままになる。

### 手順

1. **backend 起動**
   - 別ターミナルで Python backend (gRPC engine) を起動する。
   - `localhost` の engine port が listen 状態であることを確認 (`netstat -ano | findstr <port>`)。

2. **GUI 起動**
   - `cargo run` で GUI を起動する。
   - 起動ログに `gRPC` / `engine` 接続エラーが出ていないこと。

3. **Strategy / Scenario 準備**
   - Strategy Editor で既存の動作確認済み strategy (`.py`) を open。
   - Sidebar の Scenario / Instruments が空でないことを確認する。

4. **Run 実行**
   - Footer の Run ボタンを押下する。
   - 期待される現象:
     - cache `.py` が更新される (timestamp 更新で確認)。
     - backend ログに `ForceStopReplay -> LoadReplayData -> StartEngine` が順に出る。
     - 数秒以内に `TradingData.replay_state == "RUNNING"` になる。
     - Chart の時刻 (`timestamp_ms`) が前進し、価格が更新される。
     - Run Result Panel が `Running` 表示になる。

5. **停止確認**
   - Footer の Force Stop / Pause で停止できる。
   - 再 Run しても同じフローで動く。

### 判定

- 上記すべて満たす → Phase 7.6 実装に進む。
- いずれか満たさない場合 → progress window 実装より先に Run 経路自体の修復を行う。修復タスクは別 issue として切り出し、本計画はブロックする。

### 確認結果を記録する場所

実装 PR の description 冒頭に、上記 1〜5 のチェックリストと、Run から `RUNNING` 観測までの実測秒数 (= progress window が出ている想定時間) を記載する。これは acceptance criteria の妥当性根拠になる。

## Current Flow

Run 経路は現在次のようになっている。

1. `src/ui/footer.rs`
   - `footer_pause_resume_system` が PauseResume ボタン押下を受ける。
   - `StrategyBuffer` を cache `.py` に flush する。
   - `StrategyRunRequested { cache_path }` を送る。

2. `src/ui/menu_bar.rs`
   - `handle_strategy_run_system` が `StrategyRunRequested` を受ける。
   - `ScenarioMetadata` を検証する。
   - cache sidecar を flush する。
   - `TransportCommand::RunStrategy { strategy_file, config }` を backend channel に送る。

3. `src/main.rs`
   - `TransportCommand::RunStrategy` を受ける。
   - tokio task 内で `ForceStopReplay -> LoadReplayData -> StartEngine` を順に実行する。
   - 現状 `BackendStatusUpdate::RunStarted` は `ForceStopReplay` 完了直後（`LoadReplayData` の前）に送られ、`LastRunResult.state = Running` になる。
   - したがって `RunStarted` 自体は「startup の途中」で発火しているだけで、replay が実際に走り出したサインではない。startup window 側はこれを再利用せず、新規 phase event を導入する。

このため、Run button の入力から「startup のどこまで進んでいるか」を UI に返す専用状態がない。`LastRunResult::Running` は replay 実行中と startup 待ちを区別できず、Run Result Panel だけではユーザーに十分な状況説明にならない。

## Goals

- Run 押下直後、同一フレームまたは次フレームで進捗ウィンドウを表示する。
- startup の段階を短い文言で表示する。
- 実際の進捗率がない処理では indeterminate progress bar として動かす。
- `LoadReplayData` / `StartEngine` 失敗時は progress window 上で失敗を表示し、ユーザーが閉じられるようにする。
- replay が始まったと判断できたら progress window を自動で閉じる。
- 既存の Run Result Panel は維持し、startup feedback 専用の軽い UI として追加する。
- stale backend status update が新しい startup window に混ざらないよう、Run ごとの `startup_id` で照合する。

## Non-Goals

- replay 全体の完了率表示は扱わない。
- Python backend に本物のロード件数・バー件数 progress API を追加しない。
- `StartEngine` 内部で長時間同期実行される設計自体の変更は行わない。
- Footer の既存 progress / state badge の全面再設計は行わない。
- scenario `instruments` の編集は本 Phase では扱わない。これは既に Sidebar / InstrumentRegistry の責務である。

## UX Specification

### 表示タイミング

Run ボタンを押して、cache flush と scenario validation が通り、`TransportCommand::RunStrategy` を backend channel へ送信できた時点で表示する。

ただし、cache flush 失敗・scenario 不備・backend channel 未接続など、backend に Run command を送れない場合は progress window を出さない。代わりに既存ログに加えて、将来の toast/error UI で扱う。

### ウィンドウ

- 位置: screen center、footer より上。
- サイズ: 約 `360 x 120 px`。
- 背景: 半透明の濃色 panel。
- 内容:
  - title: `Starting replay`
  - stage label:
    - `Starting replay command...`
    - `Resetting previous replay...`
    - `Loading replay data...`
    - `Starting Python strategy...`
    - `Waiting for first replay tick...`
  - indeterminate progress bar
  - optional small detail: strategy filename or first instrument
  - failure 時のみ `Close` button

### 閉じるタイミング

次のいずれかで自動的に閉じる。

- matching startup の `WaitingForFirstTick` を受信した後、`TradingData.replay_state == Some("RUNNING")`
- matching startup の `WaitingForFirstTick` を受信した後、`TradingData.timestamp_ms` が Run 押下時の baseline と異なる値に変化した(`!=` 比較、§Auto-hide 参照)
- matching startup の `BackendStatusUpdate::RunComplete` が来た
  - 小さい replay で `RUNNING` が UI polling に見える前に完了するケースを吸収する。

失敗時は閉じず、エラー文を表示する。ユーザーが `Close` を押したら消す。

## Data Model

`src/ui/components.rs` に startup progress 用 resource と component を追加する。

```rust
#[derive(Resource, Default, Debug, Clone)]
pub struct ReplayStartupProgress {
    pub visible: bool,
    pub phase: ReplayStartupPhase,
    pub detail: Option<String>,
    pub error: Option<String>,
    /// `Time<Real>::elapsed()` 基準の起動時刻。Bevy の `Time<Real>` resource を使うことで
    /// timeout テストで clock を手動更新できるようにする。
    /// `std::time::Instant` を使わないこと(テスト不能になる)。
    pub started_at_elapsed: Option<std::time::Duration>,
    /// Run 押下時点で観測されていた `TradingData.timestamp_ms`。
    /// `Option<i64>` で None を「未設定」、Some(0) を「ゼロを実値として観測」と区別する。
    pub baseline_timestamp_ms: Option<i64>,
    /// UI が Run ごとに採番する startup id。BackendStatusUpdate と照合し、
    /// 古い tokio task や前回 replay の status が新しい window を閉じないようにする。
    pub startup_id: u64,
    pub next_startup_id: u64,
    /// matching startup の StartEngine request が成功し、UI polling の first tick を
    /// 待つ段階に入ったかどうか。旧 replay の RUNNING/timestamp を拾わないための gate。
    pub start_engine_accepted: bool,
}

/// startup window の進行段階。
/// `Failed` は意図的に含めない —— 失敗は `ReplayStartupProgress.error.is_some()` で表す。
/// phase enum と failure flag を二重ルートにしない。
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStartupPhase {
    #[default]
    Idle,
    CommandAccepted,
    ResettingReplay,
    LoadingData,
    StartingStrategy,
    WaitingForFirstTick,
}
```

`Idle` は `visible == false` 時のみ取り得る。`visible == true` の間は必ず他の variant のいずれかである。`CommandAccepted` は UI 側だけの初期 phase で、`TransportCommand::RunStrategy` 送信成功後から backend の最初の `ReplayStartup` status を受けるまでの短い待機状態を表す。failure 中も最後の phase を保持し、`error.is_some()` で失敗状態を判定する。

UI marker components:

```rust
#[derive(Component)]
pub struct ReplayStartupWindow;

#[derive(Component)]
pub struct ReplayStartupStageLabel;

#[derive(Component)]
pub struct ReplayStartupBarFill;

#[derive(Component)]
pub struct ReplayStartupCloseButton;
```

## Phase 7.6b: Scenario Startup Parameters Panel

Run 前に scenario の主要 startup parameter を確認・変更できる compact form を、progress window とは別の**常時可視 panel**として追加する。progress window は「Run command 送信後から startup 完了まで」の feedback であり、scenario parameter panel は「Run 前に startup 条件を整える」ための UI なので、責務を分離する。

対象 field:

- `scenario.start`
- `scenario.end`
- `scenario.granularity`
- `scenario.initial_cash`

`scenario.instruments` は既に Sidebar / InstrumentRegistry の責務があるため、本 Phase では扱わない。

### UX

Scenario Startup Parameters Panel は **Sidebar 下部**（instrument list の下、Sidebar Panel の内部フッター相当）に小さな常設フォームとして置く。Run 前は編集可能、progress window が visible (= Run startup 中) の間は disabled 表示にする。

- `Start`: date text input (`YYYY-MM-DD`)
- `End`: date text input (`YYYY-MM-DD`)
- `Granularity`: segmented control or dropdown (`Daily` / `Minute`)
- `Initial cash`: numeric input
- validation error: field 近くに短い error text を表示

Commit timing:

- date / cash input: Enter または focus loss
- granularity: 選択変更時

編集が成功したら `ScenarioMetadata` resource を同時に更新し、次の `handle_strategy_run_system` が作る `StrategyRunConfig` に同じ値が使われるようにする。

### Cache JSON Writeback

変更先は **cache sidecar JSON のみ** とする。

- 書き換え対象: `ScenarioWritebackPaths.cache_sidecar`
- 元の `<strategy>.json` は触らない。
- `cache_sidecar == None` の場合:
  - panel は disabled で `Scenario cache is unavailable` を表示する。
  - Run 自体は block しない。cache sidecar が無い状態でも既存 flow で Run できていたため、本 Phase で挙動を変えない。
  - progress window は通常通り表示できる。

### Coexistence with `scenario.instruments` writeback

既存 `writeback_scenario_instruments_system` も同じ `ScenarioWritebackPaths.cache_sidecar` を read-modify-write する。同一 frame で両方が発火すると、後勝ちで片方の field 更新が失われる(両 system が serde_json::Value で読み→書き戻すため)。これを防ぐために以下を守る。

1. **revision fence の共有はしない**: instruments 側の `ScenarioInstrumentsWritebackState` を流用せず、scenario startup params 用に独立の pending flag を `ScenarioStartupParams.writeback_pending` で持つ。理由は責務分離 ── instruments の registry edit と startup param の field commit は独立に起きうるため。`dirty` は UI 入力中の上書き保護だけに使い、writeback 判定には使わない。
2. **system ordering で chain**: `write_startup_params_to_cache_sidecar` を `writeback_scenario_instruments_system` の `.after(...)` に置く。これで同一 frame に両方が pending でも、instruments writeback の `flush_sidecars_now` が先に終わり、その上から startup params が atomic に上書きする。
3. **読み直し前提**: startup param writeback は **必ず `read_json_with_bom_strip` で最新ファイルを読み直してから** scenario 配下 4 field のみ置換する(キャッシュ済み Value は使わない)。これで instruments writeback が書いた `scenario.instruments` を保持できる。
4. **テストで明示**: Test #10c で「同一 frame に registry edit と startup param commit が両方走った場合、両方の更新が cache JSON に残る」を assert する。

JSON の形は既存 sidecar に合わせ、`scenario` object 配下の 4 field だけを書き換える。

```json
{
  "scenario": {
    "start": "2024-04-15",
    "end": "2024-04-15",
    "granularity": "Minute",
    "initial_cash": 10000000
  }
}
```

既存 JSON の未知 field、layout 情報、`scenario.instruments` / `scenario.instruments_ref` は保持する。実装では `serde_json::Value` を使い、`scenario.start` / `scenario.end` / `scenario.granularity` / `scenario.initial_cash` だけを置換する。

### Validation

UI commit 時に最低限の validation を行う。

- `start` / `end`
  - 空文字不可。
  - `chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")` で parse 成功すること。
  - parse 後の `NaiveDate` で `start <= end`。
- `granularity`
  - `Daily` または `Minute`。
- `initial_cash`
  - 正の整数。

validation 失敗時は cache JSON を書き換えず、`ScenarioMetadata` も更新しない。失敗した field に対応する `ScenarioStartupParams.errors.<field>` を `Some(message)` にし、`errors.any() == true` の間は `handle_strategy_run_system` が Run command を送信しない。Run が block された場合、progress window も表示しない。

field を跨ぐ `start > end` のような validation は `errors.cross_field` に入れ、UI 上は `start` field の直下に表示する。

### Resource / State

`src/ui/components.rs` に `ScenarioStartupParams` と `GranularityChoice` を追加する。

```rust
#[derive(Resource, Default, Debug, Clone)]
pub struct ScenarioStartupParams {
    pub start: String,
    pub end: String,
    pub granularity: GranularityChoice,
    /// 文字列で保持する理由は input UX 上、編集中の "" や "100" のような
    /// 不完全状態を許容するため。commit 時に parse する。
    pub initial_cash: String,
    /// UI 側で編集中フラグ。`sync_startup_params_from_scenario_system` は
    /// `dirty == false` のときだけ ScenarioMetadata から上書きする。
    /// cache writeback 待ちとは分ける。commit 成功時は dirty を false に戻し、
    /// writeback_pending を true にする。
    pub dirty: bool,
    /// validated commit があり、cache sidecar への反映がまだ終わっていないことを表す。
    /// `write_startup_params_to_cache_sidecar` が成功したら false に戻す。
    pub writeback_pending: bool,
    /// field-level error。各 field を独立に検証して格納する。
    /// UX 仕様「field 近くに短い error text を表示」を保つため、
    /// 単一 `Option<String>` ではなく per-field map にする。
    /// `start` と `initial_cash` を同時に invalid にしたケースでも両方表示できる。
    /// Run block 条件は `errors.values().any(|e| e.is_some())`。
    pub errors: ScenarioStartupParamsErrors,
}

#[derive(Default, Debug, Clone)]
pub struct ScenarioStartupParamsErrors {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub initial_cash: Option<String>,
    /// `start <= end` のような **field 横断** validation 失敗のとき設定する。
    /// どの field の近くに表示するかは UI 側で `start` field の下に固定する。
    pub cross_field: Option<String>,
}

impl ScenarioStartupParamsErrors {
    pub fn any(&self) -> bool {
        self.start.is_some()
            || self.end.is_some()
            || self.granularity.is_some()
            || self.initial_cash.is_some()
            || self.cross_field.is_some()
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GranularityChoice {
    #[default]
    Daily,
    Minute,
}
```

`granularity` を `String` ではなく enum にする理由: UI dropdown は 2 値固定で、stringly-typed にすると validation を全 commit path に重複させる必要が出る。`ScenarioMetadata.granularity: Option<String>` との変換は `as_str()` / `parse` helper に閉じる。

### Granularity canonical form

`GranularityChoice` を文字列化するときの canonical 表記を以下に固定する。既存 cache sidecar(`tests` 配下の fixture / `src/ui/components.rs` の test data) と `src/main.rs` の `granularity_i32` 判定の両方が `"Daily"` / `"Minute"` を前提にしているため、それに揃える。

| `GranularityChoice` | `ScenarioMetadata.granularity` | cache JSON `scenario.granularity` | `StrategyRunConfig.granularity` |
|---------------------|--------------------------------|-----------------------------------|--------------------------------|
| `Daily`             | `Some("Daily")`                | `"Daily"`                         | `"Daily"`                      |
| `Minute`            | `Some("Minute")`               | `"Minute"`                        | `"Minute"`                     |

変換 helper は `scenario_startup_panel.rs` 内に閉じる:

```rust
impl GranularityChoice {
    pub fn as_canonical_str(self) -> &'static str { ... }
    pub fn parse_canonical(s: &str) -> Option<Self> { ... }
}
```

`parse_canonical` は完全一致のみ(`"daily"` などは弾く)。`ScenarioMetadata.granularity` が `None` または canonical 外の場合、`GranularityChoice::default() == Daily` にフォールバックし、`ScenarioStartupParams.errors.granularity = Some("unknown granularity ...")` を設定する。これにより不明値で silent に Daily 扱いされる事故を防ぐ。

### Input UI library

入力 widget は **既存 `strategy_editor.rs` と同じ `bevy_cosmic_edit`** を使う。理由:

- `bevy_egui` は本プロジェクトに導入されておらず、本 Phase で増やしたくない([bevy-engine](skill) の選定方針)。
- Bevy native の `Text2d` だけでは text input(キャレット、IME、選択範囲)を扱えない。
- strategy_editor で `CosmicEditBuffer` / `CosmicEditor` を既に使っており、フォーカス管理(`FocusedWidget`) や DPI トラップ([cosmic-edit-buffer-metrics-dpi-trap](memory)) の対策パターンを流用できる。

注意:

- `CosmicEditBuffer` の Metrics は **`1.0` 倍** で構築し、DPI scale は `CosmicEditor` 側でのみ適用する。`Buffer` 側で `DPI * font_size` の Metrics を作ると、`CosmicEditor` 内部 buffer と二重 scale になり文字が極端に大きく/小さくなる。
- `start` / `end` / `initial_cash` の 3 field それぞれに独立の `CosmicEditor` entity を spawn し、`FocusedWidget` で 1 つだけ active にする。
- `granularity` は cosmic_edit を使わず、Bevy native button 2 つの segmented control にする(`Daily` / `Minute` トグル)。

### Sync Direction

1. `ScenarioMetadata` loaded / changed
   - `sync_startup_params_from_scenario_system` は `params.dirty == false` のときだけ ScenarioMetadata を読んで params を上書きする。
   - `dirty == true` のときは UI 側の入力途中なので触らない。
2. UI field commit
   - validate する。
   - 失敗時は cache writeback と ScenarioMetadata 更新を skip し、`params.errors.<field> = Some(...)`(または cross-field 失敗なら `params.errors.cross_field`)。
   - 成功時は対応する `params.errors.<field>` を `None` にクリア、`params.dirty = false` に戻し、`params.writeback_pending = true` を立てる。
   - `ScenarioMetadata` を更新する。
   - cache sidecar JSON の書き換えは同 frame の `write_startup_params_to_cache_sidecar` に任せる。
3. Run
   - `handle_strategy_run_system` は更新済み `ScenarioMetadata` から `StrategyRunConfig` を作る。
   - `ScenarioStartupParams.errors.any() == true` の間は Run を block し、progress window も表示しない。

### Systems

新規 file `src/ui/scenario_startup_panel.rs` を追加する。

- `spawn_scenario_startup_panel`
  - 常時可視 panel を spawn する。
- `sync_startup_params_from_scenario_system`
  - `ScenarioMetadata` から `ScenarioStartupParams` を初期化・同期する。
- `scenario_startup_param_input_system`
  - input commit を検知し、validation する。
- `write_startup_params_to_cache_sidecar`
  - `ScenarioStartupParams.writeback_pending == true` のときだけ動く。
  - `ScenarioWritebackPaths.cache_sidecar` の JSON を `serde_json::Value` で読み直し、scenario 配下 4 field だけを書き換える。
  - 書き込み成功時に `writeback_pending = false` へ戻す。失敗時は `writeback_pending = true` のまま残し、error log を出す。
- `update_scenario_startup_param_ui_system`
  - form 表示、disabled 状態、error 表示を更新する。

Ordering:

```rust
sync_startup_params_from_scenario_system
    .after(parse_scenario_system)
    .before(scenario_startup_param_input_system)

scenario_startup_param_input_system
    .after(parse_scenario_system)
    .before(handle_strategy_run_system)
    .before(write_startup_params_to_cache_sidecar)

write_startup_params_to_cache_sidecar
    .after(writeback_scenario_instruments_system)
    .before(handle_strategy_run_system)

// auto_hide は status_update_system が start_engine_accepted を書いた後に実行する。
// 同一 frame で WaitingForFirstTick が届き RUNNING も観測された場合に
// auto-hide が確実に発火するよう、status_update_system の後に配置する。
auto_hide_replay_startup_window_system
    .after(status_update_system)

// update_scenario_startup_param_ui_system は validation errors と
// progress.visible の最新状態を反映するため、以下の後に配置する。
update_scenario_startup_param_ui_system
    .after(scenario_startup_param_input_system)
    .after(auto_hide_replay_startup_window_system)
```

ポイント:

- Run と同じ frame で param を commit した場合、Run config が古い値を読まないように、input system は `handle_strategy_run_system` より前に置く。
- `parse_scenario_system` が同 frame で sidecar を reload した場合、sync system は `dirty == false` のときだけ params を上書きする。input commit 中(`dirty == true`)に reload が走っても params は守られる。
- cache writeback は `writeback_scenario_instruments_system` の後に置き、§Coexistence with `scenario.instruments` writeback の race を回避する。
- `auto_hide_replay_startup_window_system` を `status_update_system` の後に置くことで、`start_engine_accepted = true` が書かれた同一 frame に auto-hide が発火できる（ordering なしでは 1 frame 遅延する）。
- `update_scenario_startup_param_ui_system` は mutation 系の後に置き、同一 frame の commit 結果を UI に即反映する。

### Disable enforcement while progress window is visible

`ReplayStartupProgress.visible == true` の間、scenario startup panel は以下のように扱う。

1. **視覚的 disabled**: stage label / fill / input field を grey 化し、`CosmicEditor` の readonly flag を立てる(commit に進めない見た目)。
2. **入力 commit を skip**: `scenario_startup_param_input_system` の冒頭で `if progress.visible { return; }` を行い、Enter / focus loss / dropdown 選択イベントが来ても commit せず破棄する。`dirty` フラグも触らない。
3. **writeback も skip**: `write_startup_params_to_cache_sidecar` も同じ guard を持ち、Run startup 中に古い `writeback_pending` flag が残っていても sidecar を上書きしない。
4. **解除タイミング**: progress window が `visible == false` に戻った直後の frame から、編集と commit を再開する。`writeback_pending` は通常 Run startup 開始前に flush 済みだが、残っていた場合でも解除後に最新 JSON を読み直してから 4 field だけを書き戻す。

## Event / Status Design

`src/main.rs` の `BackendStatusUpdate` に startup phase を追加する。**UI 専用型 (`ReplayStartupPhase`) を `main.rs` の enum に直接埋めると依存方向が逆転する**ため、backend → UI 境界では transport-neutral な独立 enum を使い、UI 側の `status_update_system` で `ReplayStartupPhase` に変換する。

```rust
// src/main.rs
enum BackendStartupStage {
    ResettingReplay,
    LoadingData,
    StartingStrategy,
    WaitingForFirstTick,
}

enum BackendStatusUpdate {
    ...
    ReplayStartup {
        startup_id: u64,
        stage: BackendStartupStage,
    },
    RunComplete {
        startup_id: Option<u64>,
        run_id: String,
        summary_json: String,
    },
    RunFailed {
        startup_id: Option<u64>,
        error: String,
    },
}
```

`CommandAccepted` は UI 側 (`handle_strategy_run_system`) でのみ設定する phase で、backend は知らない。`status_update_system` で `BackendStartupStage -> ReplayStartupPhase` 変換を行い、**かつ `ReplayStartupProgress.visible == true && progress.startup_id == startup_id` のときだけ反映する**。visible == false 時や `startup_id` 不一致時に backend から phase が来ても無視する(古い run の名残、前回 tokio task の遅延 status、UI 起動前に backend が走っていたケースを潰す)。

`TransportCommand::RunStrategy` には `startup_id: u64` を追加する。これは backend/proto へ送らず、Rust UI transport 内だけで status update の相関に使う。

`src/main.rs` の tokio task は次のタイミングで送信する。

1. `ResettingReplay`
   - `force_stop_replay` の直前。
   - `force_stop_replay` が gRPC error または `success == false` を返した場合は、`RunFailed { startup_id: Some(startup_id), ... }` を送り、以降へ進まない。
2. `LoadingData`
   - `load_replay_data` の直前。
3. `StartingStrategy`
   - `start_engine` の直前。
4. `WaitingForFirstTick`
   - `StartEngineResponse.success == true` だが、まだ UI polling で tick を観測していない段階。

`config.granularity` が `"Daily"` / `"Minute"` 以外の場合も、silent return せず `RunFailed { startup_id: Some(startup_id), ... }` を送る。UI 側 validation で通常は到達しないが、defense-in-depth として startup window に失敗を表示する。

失敗時は `BackendStatusUpdate::RunFailed { startup_id: Option<u64>, error }` を使い、`status_update_system` で `ReplayStartupProgress.error = Some(...)` を設定する(phase はそのままにする)。**ただし `startup_id == Some(progress.startup_id)` かつ `progress.visible == true` のときだけ反映する**。`startup_id == None` または不一致の `RunFailed` は startup window と無関係(replay 実行中の失敗や古い task など)なので、既存通り `LastRunResult` のみを更新し window は触らない。

`RunComplete` も `startup_id: Option<u64>` を持たせる。auto-hide は matching `RunComplete` だけで発火させ、`LastRunResult.state == Completed` 単独では閉じない。

## System Plan

### 1. Resource 初期化

`src/ui/mod.rs`:

- `.init_resource::<ReplayStartupProgress>()` を追加。
- `.init_resource::<ScenarioStartupParams>()` を追加。
- `spawn_replay_startup_window` を Startup に追加。
- `spawn_scenario_startup_panel` を Startup に追加。
- `update_replay_startup_window_system`
- `animate_replay_startup_bar_system`
- `replay_startup_close_button_system`
- `auto_hide_replay_startup_window_system`
- `sync_startup_params_from_scenario_system`
- `scenario_startup_param_input_system`
- `write_startup_params_to_cache_sidecar`
- `update_scenario_startup_param_ui_system`

を Update に追加する。

### 2. Run 押下直後の表示

`src/ui/footer.rs` または `src/ui/menu_bar.rs` のどちらかに責務を置く。

採用案: `handle_strategy_run_system`。

理由:

- cache flush と scenario validation が成功し、実際に `TransportCommand::RunStrategy` を送る直前が最も正確。
- Footer 側で表示すると、その後の scenario validation failure でも一瞬表示される可能性がある。

実装順序:

1. scenario validation と inline sidecar flush を完了する。
2. `ScenarioStartupParams.errors.any() == false` を確認する。error がある場合は Run を block し、progress window を表示しない。
3. `startup_id = progress.next_startup_id; progress.next_startup_id += 1;` を採番する。
4. `TransportCommand::RunStrategy { strategy_file, config, startup_id }` を送信する。
5. `sender.tx.send(...)` が成功した場合だけ、progress window と `LastRunResult` を更新する。
6. `sender` が無い、または `send` が失敗した場合は window を表示しない。既存通り error log を出し、必要なら後続の toast/error UI で扱う。

```rust
progress.visible = true;
progress.phase = ReplayStartupPhase::CommandAccepted;
progress.detail = Some(event.cache_path.file_name()...);
progress.error = None;
progress.started_at_elapsed = Some(real_time.elapsed());
progress.baseline_timestamp_ms = Some(trading_data.timestamp_ms);
progress.startup_id = startup_id;
progress.start_engine_accepted = false;
// 前回 Run の終了状態が auto-hide を即発火させないようリセットする。
last_run.state = RunState::Running;
```

`handle_strategy_run_system` に `ResMut<ReplayStartupProgress>`, `Res<ScenarioStartupParams>`, `Res<TradingData>`, `Res<Time<Real>>`, `ResMut<LastRunResult>` を追加する。

`Time<Real>` を使う理由: UI 側で playback を pause する将来機能 (`Time<Virtual>` の pause) で startup timeout が止まると、replay 起動失敗の検知漏れになる。startup timeout は wall-clock で測る。

`LastRunResult.state = Running` の事前リセットは**必須**である。これをしないと、Run Result Panel が前回 Run の `Completed` のまま残り、新しい startup が始まったことを既存 UI が示せない。

既存挙動への影響: これまで `Running` への遷移は backend からの `BackendStatusUpdate::RunStarted` 受信(force_stop 後)を待っていたため、UI が `Running` 表示になるまで 100ms〜数 frame の遅延があった。本変更で UI 側が先回りで `Running` を書くため、Run Result Panel の `Running` 表示が即時化する。これは既存 Acceptance「Footer Run / Pause / Resume / ForceStop の挙動を壊さない」に抵触しない(状態遷移の方向は同じで、見えるタイミングが早まるだけ)。

### 3. Backend phase 反映

`src/main.rs`:

- `BackendStartupStage` enum と `BackendStatusUpdate::ReplayStartup { startup_id, stage }` を追加(§Event / Status Design 参照)。
- `status_update_system` で `ReplayStartupProgress` を更新するため、引数に `ResMut<ReplayStartupProgress>` を追加する。
- `BackendStartupStage -> ReplayStartupPhase` の変換は `status_update_system` 内で行う。
- Run task 内の各 RPC 直前に matching `startup_id` 付き phase update を送る。
- `WaitingForFirstTick` 反映時に `progress.start_engine_accepted = true` を設定する。
- `RunFailed` は matching `startup_id` のときだけ `progress.error` を設定する。
- `RunComplete` は matching `startup_id` のとき `status_update_system` が **直接** cleanup を実行する（`auto_hide_replay_startup_window_system` の polling ではなく event-driven で確実に処理する）。cleanup code は §5 auto-hide の「満たしたら」block と同じ（`progress.visible = false`, `phase = Idle`, etc.）。

注意: `BackendStatusUpdate::RunStarted` の意味は既存互換のため残す。ただし UI 表示上は `RunStarted == startup started` と解釈しない。Startup window は専用 resource だけを見る。

### 4. UI 描画

新規ファイル案: `src/ui/replay_startup_window.rs`

責務:

- `spawn_replay_startup_window`
  - hidden 状態で screen-space UI を spawn。
  - `ReplayStartupWindow` root に `Visibility::Hidden`。
- `update_replay_startup_window_system`
  - `ReplayStartupProgress` の変化を label / visibility / color に反映。
- `animate_replay_startup_bar_system`
  - visible かつ failed でない時だけ、時間ベースで bar fill の `left` / `width` を往復させる。
- `replay_startup_close_button_system`
  - failed 時のみ Close button を有効にし、押下で `visible=false`。

indeterminate bar は Bevy UI の親 `Node` に clip 相当がなければ、固定幅 container の中で fill node の `left` を動かす。clip が難しい場合は、bar container 全幅に対して fill width を `30%` にし、透明度と横位置を変えるだけでもよい。

### 5. Auto-hide

auto-hide は 2 つの経路に分かれる。

**経路 A — event-driven（`status_update_system` が担当）**

`status_update_system` が matching `BackendStatusUpdate::RunComplete { startup_id: Some(progress.startup_id), ... }` を受信したとき、直接 cleanup を実行する（§3 参照）。小さい replay で `RUNNING` が UI polling に見える前に完了するケースを確実に吸収するため、polling system ではなく event-driven にする。

**経路 B — polling（`auto_hide_replay_startup_window_system` が担当）**

`auto_hide_replay_startup_window_system`:

条件:

- `progress.visible == true`
- `progress.error.is_none()` (failure 中は手動 Close)
- 次のいずれか:
  - `progress.start_engine_accepted == true` かつ `TradingData.replay_state.as_deref() == Some("RUNNING")`
  - `progress.start_engine_accepted == true` かつ `progress.baseline_timestamp_ms` が `Some(b)` で、かつ `TradingData.timestamp_ms != b`
    （**`>` ではなく `!=`**: scenario 切替で過去日に飛ぶケースもあり、単調増加を仮定しない。baseline は Run 押下時の snapshot で「matching startup が StartEngine accepted まで進んだ後に変化したら startup が完了した」とみなす）

満たしたら:

```rust
progress.visible = false;
progress.phase = ReplayStartupPhase::Idle;
progress.detail = None;
progress.baseline_timestamp_ms = None;
progress.started_at_elapsed = None;
progress.start_engine_accepted = false;
// progress.error は触らない(false パスでは初期化済み、failure パスでは
// 手動 Close で別途クリアする)
```

`LastRunResult.state == RunState::Completed` 単独では auto-hide しない。前回 Run の completed state や startup_id 不明の completion を拾うと、window が即時に消えるため。

### 6. Timeout

startup が長時間無応答になった場合に、表示が永久に動き続けるのを避ける。

初期値:

- 60 秒で soft timeout。

挙動:

- `error = Some("Replay startup is taking longer than expected. Check backend logs or try Force Stop.")`
- Close button を表示。
- phase はそのまま(最後に観測された段階を残す)。

これは backend の処理自体を止めない。あくまで UI feedback の timeout とする。

判定式:

```rust
let elapsed_since_start =
    progress.started_at_elapsed.map(|s| real_time.elapsed().saturating_sub(s));
if progress.visible
    && elapsed_since_start.is_some_and(|d| d >= Duration::from_secs(60))
    && progress.error.is_none()
{
    progress.error = Some(...);
}
```

`Res<Time<Real>>` を使うことで、test では Bevy app の Time resource を手動更新(`time.advance_by(Duration::from_secs(61))`)して確実に発火させられる。`std::time::Instant` ベースだと test が wall-clock 待ちになり禁則事項になる。

## Files to Change

- `src/ui/components.rs`
  - `ReplayStartupProgress`
  - `ReplayStartupPhase`
  - `ScenarioStartupParams`
  - `GranularityChoice`
  - marker components
- `src/ui/replay_startup_window.rs` (new)
  - startup progress window UI systems
- `src/ui/scenario_startup_panel.rs` (new)
  - scenario startup parameter form systems
  - cache sidecar writeback helper
- `src/ui/mod.rs`
  - module import
  - resource init
  - systems registration
- `src/ui/menu_bar.rs`
  - `handle_strategy_run_system` で `startup_id` 採番、`RunStrategy` 送信成功後の progress 表示開始
  - `ScenarioStartupParams.errors.any() == true` の間は Run を block
- `src/main.rs`
  - `BackendStartupStage` enum (transport-neutral)
  - `BackendStatusUpdate::ReplayStartup { startup_id, stage }`
  - run task の phase update 送信
  - `status_update_system` で `BackendStartupStage -> ReplayStartupPhase` 変換 + `visible == true` / `startup_id` ガード + matching `RunFailed` の `progress.error` 設定 + matching `RunComplete` の auto-hide
- `src/trading.rs`
  - `TransportCommand::RunStrategy` に `startup_id: u64` を追加する。
  - `RunState` に `Starting` を追加したくなった場合でも本計画では採用しない。

## Test Plan

### Unit / ECS Tests

1. `handle_strategy_run_system` success path
   - `StrategyRunRequested` を送る。
   - `TransportCommand::RunStrategy { startup_id, .. }` が送られる。
   - `ReplayStartupProgress.visible == true`
   - `phase == CommandAccepted`
   - `ReplayStartupProgress.startup_id == startup_id`
   - `ReplayStartupProgress.start_engine_accepted == false`

1b. backend channel send failure
   - `TransportCommandSender` が無い、または receiver drop 済みの状態で `StrategyRunRequested` を送る。
   - `ReplayStartupProgress.visible == false`
   - `LastRunResult.state` は前回値から変化しない。

2. scenario validation failure
   - `ScenarioMetadata.instruments` 空など。
   - `ReplayStartupProgress.visible == false`

3. backend phase update
   - 事前に `progress.visible = true`。
   - `BackendStatusUpdate::ReplayStartup { startup_id: progress.startup_id, stage: BackendStartupStage::LoadingData }` を流す。
   - `ReplayStartupProgress.phase == ReplayStartupPhase::LoadingData`

3b. backend phase update を visible == false 時は無視
   - `progress.visible = false` のまま `BackendStatusUpdate::ReplayStartup { startup_id, stage: LoadingData }` を流す。
   - `progress.phase == Idle` のまま、`visible == false` のまま。

3c. backend phase update の startup_id mismatch は無視
   - `progress.visible = true`, `progress.startup_id = 10`。
   - `BackendStatusUpdate::ReplayStartup { startup_id: 9, stage: LoadingData }` を流す。
   - `progress.phase` は変化しない。

3d. `WaitingForFirstTick` で first-tick gate が開く
   - matching `BackendStatusUpdate::ReplayStartup { stage: WaitingForFirstTick }` を流す。
   - `progress.phase == WaitingForFirstTick`
   - `progress.start_engine_accepted == true`

4. failure during startup
   - 事前に `progress.visible = true`(`CommandAccepted` 設定済み)。
   - matching `BackendStatusUpdate::RunFailed { startup_id: Some(progress.startup_id), error }` を流す。
   - `progress.error == Some(error)`
   - `visible == true` のまま(Close 待ち)。
   - phase は変えない。

4b. failure outside startup
   - `progress.visible = false` のまま `BackendStatusUpdate::RunFailed { startup_id: None, .. }` を流す。
   - `progress.visible == false` のまま、`progress.error == None` のまま(無関係 run の失敗を吸わない)。
   - `LastRunResult.state == Failed { .. }` は更新される(既存挙動)。

4c. failure startup_id mismatch
   - `progress.visible = true`, `progress.startup_id = 10`。
   - `BackendStatusUpdate::RunFailed { startup_id: Some(9), error }` を流す。
   - `progress.error == None` のまま。

4d. startup task early failure emits matching RunFailed
   - テストセットアップ: `BackendStatusUpdate` を送る `mpsc::channel` の sender を test 側が握り、実際の gRPC call は行わない ECS test として構成する。`TransportCommandSender` を差し替えるのではなく、tokio task を spawn せず代わりに `status_update_tx.send(BackendStatusUpdate::RunFailed { startup_id: Some(startup_id), error: "force_stop failed".into() })` を直接呼ぶ。
   - 事前に `progress.visible = true`、`progress.startup_id = startup_id` を設定。
   - `status_update_system` を回して `BackendStatusUpdate::RunFailed` を処理させる。
   - `progress.error` に失敗文言が設定される。
   - `progress.visible == true` のまま(Close 待ち)。
   - `load_replay_data` / `start_engine` は呼ばれない（tokio task を spawn しないため自明）。

4e. unknown granularity emits matching RunFailed
   - `StrategyRunConfig.granularity = "Tick"` などを渡す。
   - silent return せず matching `RunFailed` が送られる。

5. auto-hide by RUNNING
   - progress visible, `error == None`, `start_engine_accepted == true`
   - `TradingData.replay_state = Some("RUNNING")`
   - system 実行後に hidden。

5b. old RUNNING does not auto-hide before StartEngine accepted
   - progress visible, `error == None`, `start_engine_accepted == false`
   - `TradingData.replay_state = Some("RUNNING")`
   - system 実行後も visible。

6. auto-hide by timestamp
   - `start_engine_accepted == true`, `baseline_timestamp_ms = Some(1000)`, `TradingData.timestamp_ms = 2000`
   - `replay_state == None`(他経路と独立に発火することを確認)
   - system 実行後に hidden。

6b. auto-hide by timestamp 後退
   - `start_engine_accepted == true`, `baseline_timestamp_ms = Some(2_000_000_000)`, `TradingData.timestamp_ms = 1_000_000_000`
   - (scenario 切替で過去日に飛ぶケース)
   - `!=` 判定なので hidden になることを確認。

7. fast complete (matching RunComplete)
   - progress visible, `startup_id = 10`。
   - `BackendStatusUpdate::RunComplete { startup_id: Some(10), .. }` を流す。
   - `LastRunResult.state == Completed`
   - `ReplayStartupProgress.visible == false`

7b. previous or unrelated Completed を誤って拾わない
   - 事前 `LastRunResult.state = Completed`
   - `StrategyRunRequested` を発火 → `handle_strategy_run_system` が `state = Running` にリセット & visible = true
   - `BackendStatusUpdate::RunComplete { startup_id: Some(old_id), .. }` または `startup_id: None` を流す。
   - **hidden にならないこと**を確認。

8. timeout
   - `Time<Real>` を test app 上で `advance_by(Duration::from_secs(61))` する。
   - `progress.error == Some(...)` が設定される(timeout 文言を含む)。
   - phase は最後に観測された値のまま。

9. scenario params sync
   - `ScenarioMetadata { start, end, granularity, initial_cash }` を設定。
   - `ScenarioStartupParams` に同じ値が表示用文字列として反映される。
   - `dirty == true` の間は `ScenarioMetadata` 側の変更で UI 入力中の値が上書きされない。

10. scenario params cache writeback
   - cache sidecar JSON に未知 field と `scenario.instruments` を含める。
   - `start` / `end` / `granularity` / `initial_cash` を UI commit する。
   - 対象 4 field だけが更新され、未知 field、layout 情報、`scenario.instruments` が保持される。
   - 書き込み成功後に `ScenarioStartupParams.writeback_pending == false` へ戻る。
   - 元の `<strategy>.json` は変更されない。

10b. scenario params cache writeback round-trip
   - 上記 #10 直後に `parse_scenario_system` を 1 tick 回す。
   - `ScenarioMetadata.start` / `end` / `granularity` / `initial_cash` が commit 値で再ロードされる。
   - `sync_startup_params_from_scenario_system` を回した後、`ScenarioStartupParams` の 4 field が同じ値で再現される(canonical 表記 `"Daily"` / `"Minute"` で round-trip すること)。

10c. concurrent writeback with `scenario.instruments` does not lose either change
   - registry 編集で `ScenarioInstrumentsWritebackState.revision` を bump し、同時に `start` / `end` を commit する。
   - 同一 frame で `writeback_scenario_instruments_system` → `write_startup_params_to_cache_sidecar` の順に走る。
   - cache JSON が `scenario.instruments` (registry 値) と `scenario.start` / `scenario.end` (commit 値) の両方を保持する。

10d. disabled while progress window visible
   - `progress.visible = true` の状態で `start` / `end` / `granularity` / `initial_cash` の commit イベントを流す。
   - cache JSON が変更されない。
   - `ScenarioMetadata` が変更されない。
   - `ScenarioStartupParams.dirty` / `writeback_pending` / `errors` のいずれも触られない。

11. scenario params validation failure
   - `start > end`、invalid date、`initial_cash <= 0` のいずれかを commit する。
   - cache JSON が変更されない。
   - 対応する `ScenarioStartupParams.errors.<field>` (または `errors.cross_field`) が設定される。
   - `handle_strategy_run_system` が Run を送信せず、`ReplayStartupProgress.visible == false` のまま。

11b. multiple field errors are tracked independently
   - `start = "not-a-date"` と `initial_cash = "-5"` を同時に commit。
   - `errors.start` と `errors.initial_cash` の両方が `Some(...)` になる。
   - 片方だけ修正して再 commit したとき、その field の error だけ `None` に戻り、もう片方は残る。

12. scenario params cache unavailable
   - `ScenarioWritebackPaths.cache_sidecar = None`。
   - scenario startup panel は disabled / unavailable 表示になる。
   - `ScenarioStartupParams.errors.any() == false` であれば Run 自体は block されない。

### Manual Verification

1. backend を起動し、GUI で strategy を open。
2. Footer の Run を押す。
3. `Starting replay` window が即表示される。
4. `Resetting previous replay... -> Loading replay data... -> Starting Python strategy...` と段階が変わる。
5. replay state が `RUNNING` になるか、チャート時刻が動いたら window が消える。
6. backend を止めた状態で Run。
7. window が failure 表示になり、Close で閉じられる。
8. backend channel が無い状態または送信失敗状態では window が出ず、既存 error log のみになる。
9. 再 Run を連打しても、古い startup の status で新しい window が閉じない。
10. Run していない状態で、Scenario Startup Parameters Panel から `start` / `end` / `granularity` / `initial_cash` を変更する。
11. cache sidecar JSON の該当 4 field だけが更新され、元の `<strategy>.json` は変更されない。
12. invalid date または invalid cash を入力した状態では Run が送信されず、progress window も出ない。
13. Run 中(progress window visible)は Scenario Startup Parameters Panel が disabled になる。

## Risks

- `StartEngine` が同期的に replay 完了まで返らない場合、`WaitingForFirstTick` が見えず `RunComplete` で閉じるだけになる可能性がある。この場合でも「Run 押下後に無反応」という UX 問題は解消できる。
- `TradingData.replay_state == RUNNING` が polling タイミングで観測できない短時間 replay がある。このため `RunComplete` でも閉じる。
- backend task から UI resource を直接触らず、既存の `BackendStatusUpdate` channel 経由に限定する。Bevy world の thread-safety を保つ。
- `startup_id` は Rust UI transport 内だけの相関 ID とし、backend/proto には渡さない。proto 変更を避けつつ stale update を防ぐ。
- scenario parameter edit は cache sidecar のみを書き換えるため、明示 Save しない限り元ファイルには反映されない。この挙動を UI 上で誤解させないため、panel は startup/run configuration の編集として扱い、元ファイル保存の表現はしない。
- `initial_cash` は `ScenarioMetadata` 上では `Option<i64>` だが、UI input は文字列で持つ。parse 失敗時に古い値で Run されないよう、error がある間は Run を block する。
- **既存 strategy の `granularity = None` による移行時の Run block**: 7.6b 導入前に作成された strategy の cache sidecar に `scenario.granularity` が無い場合、`sync_startup_params_from_scenario_system` が `GranularityChoice::Daily` にフォールバックして `errors.granularity = Some(...)` を設定し、Run が block される。ユーザーは Scenario Startup Parameters Panel から granularity を選択・commit することで解除できる。初回利用時に説明なしでフリーズして見えるリスクがあるため、panel の `granularity` エラー文言を「Please select a granularity to enable Run」のように案内的にする。本計画では strict 側（error + block）を採用し、silent に `Daily` を適用して動くようにはしない（7.6a が `RunFailed` に変換する defense-in-depth と一致させるため）。

## Acceptance Criteria

7.6a と 7.6b は **同一 PR で出す**。Acceptance Criteria は責務ごとに分節するが、PR マージ時には両方の条件を満たす必要がある。理由: 7.6b の `ScenarioStartupParams.errors` ガードが 7.6a の Run block 条件に組み込まれており、片方だけ merge すると Run flow が中途半端な状態(Run block 条件が宙に浮く / 7.6a 側で参照する resource が無い)になるため。

### 7.6a — Replay startup progress window

- Run 押下後、backend startup 中に progress window が表示される。
- window は startup の現在段階を表示する。
- progress bar は実進捗率なしでも動いて見える。
- replay 開始または完了時に自動で閉じる。
- `LoadReplayData` / `StartEngine` 失敗時はエラー表示に切り替わり、Close できる。
- backend channel 送信失敗時は progress window を表示しない。
- 前回 Run / 古い tokio task の status update で新しい progress window が閉じない。
- 60 秒の soft timeout が `Time<Real>` ベースで発火し、phase はそのまま error 表示に切り替わる。

### 7.6b — Scenario Startup Parameters Panel

- Scenario Startup Parameters Panel で `start` / `end` / `granularity` / `initial_cash` を表示・変更できる。
- scenario parameter 変更時、cache sidecar JSON の該当 4 field だけが更新される。`scenario.instruments` / 未知 field / layout 情報は保持される。
- scenario parameter 変更では元の `<strategy>.json` は変更されない。
- `GranularityChoice` は `"Daily"` / `"Minute"` の canonical 表記で `ScenarioMetadata` / cache JSON / `StrategyRunConfig` を round-trip する。
- 同一 frame で registry edit と startup param commit が並走しても、cache JSON 上で両方の更新が保持される(`scenario.instruments` writeback との race が無い)。
- invalid な scenario parameter (`ScenarioStartupParams.errors.any() == true`) がある間は Run が送信されず、progress window も表示されない。
- progress window が visible の間、Scenario Startup Parameters Panel は視覚的にも入力 commit 上も disabled になる(commit 自体が skip され cache JSON / ScenarioMetadata は変更されない)。

### 共通

- 既存の Footer Run / Pause / Resume / ForceStop の挙動を壊さない。
- `cargo test` の関連 UI / run-flow tests が通る。

## Implementation Notes

### 配置変更: `ReplayStartupProgress` 関連型は `src/replay/startup_progress.rs` へ (2026-05-18)

§Data Model および §Files to Change では `src/ui/components.rs` に `ReplayStartupProgress` / `ReplayStartupPhase` / 4 marker components を追加する設計だったが、実装時に **新規モジュール `src/replay/startup_progress.rs` + `src/replay/mod.rs`** へ配置することに変更した（Human 承認、Step C）。

- `src/lib.rs` に `pub mod replay;` を 1 行追加。
- 公開 API: `crate::replay::{ReplayStartupProgress, ReplayStartupPhase, ReplayStartupWindow, ReplayStartupStageLabel, ReplayStartupBarFill, ReplayStartupCloseButton}`。
- 型・field・variant・marker 名は §Data Model 原文と完全一致（`ReplayStartupPhase` は `Failed` を含まない、`error.is_some()` で失敗表現）。

**Why**: `src/ui/components.rs` が 2736 行に達しており、replay startup 専用の transient overlay 状態を分離した方が責務が明確になる。`src/ui/components.rs` への集中はモジュール肥大化を加速するため。

**How to apply**: 後続 Step（G の replay_startup_window.rs / I の scenario_startup_panel.rs）からも `use crate::replay::*` で参照する。Step H で追加予定の `ScenarioStartupParams` / `GranularityChoice` の配置先は別途判断（計画書原文どおり `src/ui/components.rs` か、`src/replay/scenario_params.rs` 等の新規 module か）。

§Files to Change の `src/ui/components.rs` 行はこの変更により以下に読み替える:

- `src/ui/components.rs` → `ReplayStartupProgress` / `ReplayStartupPhase` / replay marker components は **配置しない**。`ScenarioStartupParams` / `GranularityChoice` は Step H で別途判断。
- `src/replay/startup_progress.rs` (new) → `ReplayStartupProgress` / `ReplayStartupPhase` / 4 marker components
- `src/replay/mod.rs` (new) → 上記の re-export
- `src/lib.rs` → `pub mod replay;` 追加
