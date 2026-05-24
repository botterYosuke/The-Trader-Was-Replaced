# E2E Flow Conventions — The-Trader-Was-Replaced

> リリース前の最後の砦として、ユーザーが取りうる行動を原則すべて列挙し、自動テストの対象にする。
> 実装済み flow は `tests/e2e/flows/<id>.rs`（1 ファイル 1 テスト）に索引化されている。未実装の予定 flow も
> 同じディレクトリに `//!` だけのプレースホルダーとして置き、「何の挙動か / seam / 観測」を先に固定する。
> flow 一覧はこの markdown ではなく `.rs` 群が正本。
> 既存の Bevy resource/event/system 直接駆動ハーネスで観測できるものはこの方式で assert する。
> 直接駆動だけでは忠実に検証できない操作（描画依存、OS ダイアログ、キーボード入力、実 backend/環境依存）は
> 「対象外」にせず、代替方式（UI harness / smoke / integration / manual release gate）を明記する。

## このファイルの使い方（編集ルール）

- **実装済み flow の索引は `tests/e2e/flows/<id>.rs`（1 ファイル 1 テスト）に移行した**。実装する flow を1本足す
  = `tests/e2e/flows/<id>.rs` に `#[test]` を追加し、`tests/e2e_replay.rs`（runner）に `#[path]` + `mod` を1行足す。
  `//!` だけのファイルは予定 flow で、runner にはまだ登録しない。
- このファイルは **flow 一覧そのものではなく**、(1) 駆動の縫い目と観測の凡例、(2) 直接駆動では headless
  検証できない操作の代替方式と release gate、(3) ハーネス計画 を定義するメタ文書。
- まだ `.rs` 化していない操作（UI / integration / render / manual-gate 系）は、末尾の「直接駆動では
  不可能な場合の代替方式」テーブルに方式と release gate を記録する。予定 flow は `tests/e2e/flows/<id>.rs` の
  `//!` コメントと wiki の `[FlowID]` 引用を先に作ってよい。

### wiki ↔ E2E 同期ルール（必須）

- E2E flow 群（`tests/e2e/flows/*.rs` ＋ 下の代替方式テーブル）は `docs/wiki/`（実アプリの操作説明書）と
  **対**になっている。wiki に書かれたユーザー可視の挙動は、原則対応する flow を持つ。
- **`docs/wiki/` の操作挙動を変更・追加したら、必ず E2E flow を見直す**:
  新しい挙動 → 実装可能なら `tests/e2e/flows/<id>.rs` を追加、headless 観測不能なら下のテーブルに行を追加。
  挙動の削除・変更 → 対応する `.rs` flow / テーブル行を削除・修正。
- backend→ECS seam を通らない挙動（クライアント側 gating / 純 UI / 描画依存 / backend 内部ガード）も
  除外しない。直接駆動ハーネスで不可能な場合は、末尾の「**直接駆動では不可能な場合の代替方式**」に
  方式と release gate を記録する。

### 凡例

- **seam** … 入力する縫い目（`TransportCommand` 列挙子 / `BackendEvent` / resource 更新）
- **観測** … assert 対象の resource とその状態遷移
- **be** … 想定バックエンド: `mock`（決定論的・CI 向き） / `real`（python -m engine・忠実度確認） / `none`
- **kind** … `state`（resource/event/system 直接駆動） / `ui`（Bevy UI harness: Interaction/Keyboard/MouseWheel/Pointer を注入） /
  `render`（実ウィンドウまたは画像/ログ smoke） / `integration`（backend/CLI/環境依存） / `manual-gate`（自動化不能時のリリース手順）
- **優先** … ★★★ 高 / ★★ 中 / ★ 低

### 駆動の縫い目（参照）

- 入力: `TransportCommandSender`（`mpsc<TransportCommand>`）にコマンドを直接送る → UI ボタン描画をバイパス
- 入力(イベント): backend → ECS の `BackendStatusUpdate` / `BackendEvent` をモックから注入
- 入力(UI): Bevy の `Interaction` / `ButtonInput<KeyCode>` / `MouseWheel` / pointer event / focused entity を直接注入
- 出力(観測): `LastRunResult.state`(RunState) / `PortfolioState` / `BackendStatus` /
  `VenueStatusRes` / `ExecutionModeRes` / `Tickers` / `TickersStatus` / `AvailableInstruments` /
  `LastPrices` / `SelectedSymbol` / `ReplaySpeed` / `TradingSession` / `ReplayStartupProgress`
- 出力(UI/描画): window/panel entity 構造、`Visibility`/`Display`/`Text`/`Style`、layout JSON、strategy cache、
  render/screenshot smoke、構造化ログ

---

## 直接駆動では不可能な場合の代替方式

この節は「テストしないもの」ではない。既存の resource/event/system 直接駆動ハーネスだけではユーザー操作の忠実度が足りない場合に、採用する代替方式を定義する。

release gate 列の ID（`I*` / `J*` / `K*` / `L*` 群、保留中の `A5` / `C5` / `D8` など）は **まだ `.rs` 化していない planned flow** の識別子。実装時に `tests/e2e/flows/<id>.rs`（または `kind:render` / `kind:integration` の専用ハーネス）として起こす。

実装済みの代替方式 flow（I/J/K/L/M 群は `tests/e2e/flows/<id>.rs` に実装済み・`cargo test --test e2e_replay` で green）:

- **I 群（メニュー / file / layout）✅** — i1 メニュー開閉, i2 Alt+F/E/V, i4 モード切替 gating（Live 未接続→未送信 / Replay は strategy 未ロード・IDLE でも常に送信=ホームモード, #30）, i5 file-open→Editor/Chart spawn, i6 File→New, i7 Save→sidecar, i9 Ctrl+S/Shift+S/O dispatch, i10 open→Auto, i11 Edit→Undo/Redo, i12 起動時 cache 復元, i13 scenario-only JSON, i8 Save As→新 .json/.py ペア生成+original_path/ScenarioReadTarget 切替（rfd を `PendingFileDialog::inject_resolved` seam でバイパスし `poll_save_as_dialog_system` を駆動）, i14 original_path=None の Save→Save As フォールバック（`LayoutSaveAsRequested` 発火 assert）。`i3`（Escape/outside-close）は本番未実装のため doc stub。`i15`（cache 復元直後に Replay 突入しても `app_state.py` が scenario `.json` で上書きされない、永続 `.py` 破壊ガード）✅ — restore.rs を `ScenarioFileWatchState` reset 方式に是正（registry 再 resolve を scenario parse 経路へ移し、`restore_fixed_registry_on_replay_entry_system` が `ScenarioReadTarget`(=`.json`) を `.py` 期待の `StrategyFileLoadRequested` に流さない）＋`sync_to_cache` の `.py` ガード（`editable=false`/instruments_ref ロック時の JSON 上書きを防ぐ）の2点で green 化（#23）。`i16`（cache 復元直後の Replay 突入で `StrategyBuffer.original_path` / `PendingStrategyFragments.by_region_key` が scenario `.json` で汚染されない、セッション内 in-memory 汚染ガード、kind:integration）✅。`i17`（`strategy_path` が存在しないファイル＝例: macOS 絶対パスを Windows で開いたとき、前セッションの stale `PendingStrategyFragments` がクリアされる。I5/I12 は実在する絶対パスしかテストせず未検知だった。`layout_persistence.rs` の `else` ブランチで `by_region_key.clear()` を追加して修正）✅。`i18`（`strategy_path` が相対パス `"examples/test_strategy_minute.py"` のとき CWD 基準で解決され `StrategyBuffer.original_path` に設定される。`test_strategy_minute.json` の macOS 絶対パスを相対パスに修正した動作の回帰ガード）✅。
- **J 群（editor / find / startup / scenario / picker）✅** — j1 autosave, j2 Tab indent, j3 Enter autoindent, j4 bracket autoclose, j5 find open/nav, j6 replace current/all, j7 startup 検証 block, j8 valid run command, j9 instruments_ref fail-closed, j10 readonly registry, j11 picker search/add/close, j12 placeholder（空設定/取得中/失敗/未接続/No matches=検索一致なし、空 Universe→"No instruments for this date/venue"=ADR-0002 before_oldest 経路と区別）, j13 sidebar select/remove, j14 schema 正規化, j15 file-watch reparse, j16 field commit→cache writeback。
- **K 群（chart / order / modal）✅** — k2 wheel zoom gate, k3 drag/double-click reset（observer 駆動）, k4 Ctrl+wheel 抑制, k5 ladder live mode entity-tree, k6 reconcile diff, k7 order submit→confirm→PlaceOrder, k8 secret submit/retry, k9 context modify/cancel, k10 form controls/validation, k11 confirm Escape 優先, k12 modify modal, k13 relogin Escape 優先, k14 reconcile Escape 優先, k15 secret zeroize/empty-submit, k16 context menu open/close。`k1`（candle/crosshair 描画）は `kind:render`（ShapePainter+Text2d=GPU 必要）→ doc stub。
- **L 群（CLI / guard / backend）✅** — l3 prod guard（env unset→backend disabled, `#[serial]`）, l5 supervisor→mock gRPC Ready, l7 attach venue 照合（live venue 要求で起動 → port 19876 を握る既存 backend が非 live=`configured_venue` 不一致なら attach を拒否し `StartupFailed("BACKEND_VENUE_MISMATCH")` を publish、footer に明示エラーを出してサイレント `Venue: DISCONNECTED` にしない。孤児・非 live backend に attach してログインがサイレント死する issue #24 の loud 化。外部 backend は kill しない既存設計と整合）。l2 strategy_replay CLI は `STRATEGY_REPLAY_CLI_INTEGRATION=1` gate、l6 catalog build は `DEV_J_QUANTS_CACHE` 依存で `#[ignore]`。`l1`（run_replay.ps1）は Windows 専用 PowerShell → doc stub。`l4`（実ウィンドウ全体 smoke）は `kind:render` → doc stub。
- **M 群（window / sidebar）✅** — m1 sidebar Panels ボタン spawn, m2 drag→位置更新+autosave dirty, m3 close→despawn, m4 focus→z 前面, m5 duplicate policy（StrategyEditor のみ multi）, m7 Startup ウィンドウは × を持たない（closeable:false 回帰ガード）, m8 Startup ウィンドウ可視性は ExecutionMode が所有（Replay のみ可視）, m9 Startup 位置は WindowLayout で復元・`visible` は非権威, m10 resize handle Drag→幅拡大+最小サイズクランプ+DragEnd→autosave dirty（`resizable:true` 回帰ガード）, m11 Startup を Manual/Auto で隠すと枠だけでなく中身（Start/End/Granularity/Initial cash）も隠れる＝`content_area` が `Visibility` を持ち可視性伝播が中身まで届く（root→content 経路の全 entity が `InheritedVisibility` を持つことを assert する回帰ガード。m8 は root の `Visibility` しか見ず中身残存バグを取りこぼした）, m12 Strategy Editor（フローティング窓＋サイドバーボタン）は `ExecutionMode::LiveManual` のときだけ隠れる（Replay / LiveAuto は表示）。save/restore で layout の `visible` 権威を壊さず、Manual 中に spawn された窓も追従して隠す（kind:ui）。実 Strategy Editor を spawn し editor/gutter/scrollbar→root の `InheritedVisibility` 連鎖（content_area 経由）を構造 assert する＝枠だけでなく中身も隠れる（m11 同型・root の `Visibility` しか見ずに中身残存バグを取りこぼす経路を防ぐ回帰ガード）。`m6`（settings sidebar）は固定文字列で BackendStatus 未接続 → doc stub。
- **N 群（Live Auto strategy execution、Phase 10）✅** — n1 LiveStrategyEvent→`LiveRuns` lifecycle（upsert・started_ts_ms 固定・空 strategy_id は既知を消さない）, n2 LiveStrategyTelemetry→`LiveRuns` カウンタ（lifecycle 前に届いても row 作成 / lifecycle が telemetry を消さない）, n3 SafetyRailViolation→`SafetyToast`（最新が古いトーストを置換）, n4 StrategyLogMessage→`StrategyLogs`（oldest-first リング・`recent(n)`・`CAP`=100 で頭切り）, n5 LiveStrategyPromoteResult→`PromoteFeedback`（成功=run id / 拒否=error_code）。全 `kind:state`、既存 `Harness` の `live_runs()` / `safety_toast()` / `strategy_logs()` / `promote_feedback()` で観測。

残る doc stub（headless 不可 / 未実装の release gate）: `i3`(menu Escape 未実装), `k1`/`l4`(kind:render), `l1`(Windows PowerShell), `m6`(未接続)。Live Run Panel / Safety Rails modal / Promote ボタン の **UI 操作**（`[Pause]`/`[Resume]`/`[Stop]` クリック→`TransportCommand`、± ステッパー、Promote pre-flight gating）は `kind:ui` で未実装（N 群は backend→ECS の state seam のみ）。

| 対象 | 直接駆動だけで不足する理由 | 代替方式 | release gate |
|---|---|---|---|
| メニュー開閉 / Alt+F/E/V | backend seam を通らず、keyboard focus と UI entity 表示が本体 | `kind:ui`。`ButtonInput<KeyCode>` と `Interaction` を注入し `OpenMenu` / entity 表示を assert | I1 ✅ / I2 ✅ / I3 stub（Escape 未実装） |
| モード切替 gating | Live 遷移は precondition NG なら未送信が仕様。Replay は常に送信（ホームモード, #30） | `kind:ui`。送信 channel を監視し Live=未送信 / Replay=送信を assert。E1 で Replay→LiveManual→Replay 往復も検証 | I4 ✅ / E1 ✅ |
| OS ファイルダイアログ | CI で OS native dialog を安定操作しにくい | dialog 自体はバイパスし、選択済み path を `PendingFileDialog::inject_resolved` seam で注入。別途 smoke で起動確認 | I5 ✅ / I6 ✅ / I7 ✅ / I8 ✅ / I14 ✅ / L4 stub |
| レイアウト永続化 | ファイル I/O と debounce が主対象 | temp dir fixture で `Save/Load` を integration 実行し JSON と復元 entity を assert | I7 ✅ / I12 ✅ / M2 ✅（drag autosave） |
| cosmic_edit 入力 | text editor plugin の focus/keyboard 処理が主対象 | `kind:ui`。focused entity と keyboard/text input を注入。必要なら最小実ウィンドウ smoke を追加 | J1-J4 ✅ |
| find / replace | match span 計算と置換が主対象 | `kind:ui`。FindReplaceState と FindActionRequested を駆動 | J5/J6 ✅ |
| Startup パネル入力検証 | Run command を送らない UI gating が仕様 | `kind:ui`。field editor state、error label、transport channel 未送信/送信を assert | J7/J8/J16 ✅ |
| `instruments_ref` fail-closed | file-watch / parser / writeback の連携 | temp sidecar/ref file を使う integration。破損・空・正常の fixture を固定 | J9/J10/J14/J15 ✅ |
| 銘柄ピッカー | searchbox、debounce、候補表示、readonly が純 UI | `kind:ui`。time advance と text/entity assert。取得 seam は C1-C4/C6 と組み合わせる | J11/J12/J13 ✅ |
| 銘柄ユニバース取得の pending（cold-store warming） | backend が `LIVE_UNIVERSE_PENDING` を返したら赤エラーでなく Loading spinner（`PendingLiveUniverse`）にする（#32 Slice 2） | `kind:state`。`InstrumentsListFailed{LiveVenue,"LIVE_UNIVERSE_PENDING"}` を注入し `Tickers.status==PendingLiveUniverse`（PENDING 以外は `Failed`）を assert | C6 ✅ |
| Chart 操作 | wheel/drag/double click と render state が主対象 | `kind:ui` で `ChartViewState` / camera を assert。描画崩れは `kind:render` smoke | K2/K3/K4/K5 ✅ / K1 stub(render) / L4 stub |
| 注文フォーム / modal / context menu | 2 段階 confirm、focus、Escape 優先順位が主対象 | `kind:ui`。command channel、modal visibility、feedback resource を assert | K7-K16 ✅ |
| Prod guard / 実 venue | CI で実口座・外部環境に依存 | env isolated backend integration で guard を確認。実接続はリリース時 manual-gate に残す | L3 ✅ / L5 ✅(mock gRPC) / 実接続は manual |
| attach 経路 venue 照合 | attach は subprocess を起こさず handshake のみで、孤児・非 live backend への誤 attach は実 venue ログインがサイレント死する（footer `grpc: OK` のまま `Venue: DISCONNECTED`）ため、起動構成と backend の `configured_venue` 一致を保証する必要がある | mock gRPC + `run_supervisor(autospawn:false)` で `live_venue` を設定し GetState の `configured_venue` を照合。不一致→`StartupFailed("BACKEND_VENUE_MISMATCH")`、一致→Ready を `tests/backend_integration.rs::attach_live_venue_mismatch_reaches_venue_mismatch` / `attach_live_venue_match_reaches_ready` で assert。手動 GUI 検証（任意）: 非 live backend を 19876 に立てて GUI 起動 → footer が BACKEND_VENUE_MISMATCH 由来エラーになることを目視 | L7 ✅(mock gRPC) |
| CLI replay / catalog | 外部データ・uv 依存 | `std::process::Command` で driver、env-gate / `#[ignore]` | L2 `#[ignore]`(env) / L6 `#[ignore]`(data) / L1 stub(Windows) |
| window 操作 | drag/close/focus/duplicate が observer + ECS | `kind:ui`。observer trigger と Transform/WindowManager/AutoSaveState を assert | M1-M5 ✅ / M6 stub(未接続) / M10 ✅（resize handle） |
| Startup ウィンドウ（× 無し / mode 所有 / 位置永続化） | sprite floating window だが close 不可・可視性は ExecutionMode 所有・layout の `visible` 非権威（ADR-0001） | `kind:ui`。本番 spawn の `CloseButton` 不在、`apply_startup_panel_visibility_system` の `Visibility`、root→content の可視性伝播が `content_area` の `Visibility` で繋がること（中身も隠れる）、`apply_layout_system` 復元時の pos/z と visible 無視を assert | M7 ✅ / M8 ✅ / M9 ✅ / M11 ✅ |
| Strategy Editor（Manual で非表示） | 戦略エディタ floating window とサイドバー「Strategy Editor」ボタンは `LiveManual` でのみ隠れる（Replay / LiveAuto は表示）。Manual hide は ExecutionMode による**一時上書き**で、Startup と違い layout の `visible` は**権威のまま**（save/restore で永続 layout に焼き込まない）。root を Hidden にしても中身が伝播するには root→content の全 entity が `InheritedVisibility` を持つ必要があり、欠けると枠だけ消えて中身が残る（m11 同型の構造バグ） | `kind:ui`。`LiveManual` で editor window 群とサイドバーボタンの `Visibility` が Hidden、Replay / LiveAuto で表示、Manual 中の追加 spawn も隠れること、save/restore が layout `visible` 権威を保つこと（Manual 中の autosave/Save が `visible:false` を焼き込まない・Manual 中の layout load でマーカーが陳腐化しない）を assert | M12 ✅ |
| 画面全体の見た目 | headless resource assert では重なり・欠落を検出しづらい | `BACKCAST_E2E=1` 固定 fixture 起動、スクリーンショットまたは構造化 UI dump の smoke | L4 stub(必須・kind:render) |
| Live 突入時の口座 refill（Manual connect→Live 切替で BP/Positions が空のまま残らない） | reset 後の再 fetch は backend dedup（同一スナップショットを再送しない）を貫通する必要があり、`tests/e2e/flows/` の Harness seam（backend→ECS 注入）では「観測した実モード遷移を起点に front が `ForceAccountSnapshot` を送り返す」observer 経路を観測できない（コマンド送出の起点が遷移検知 system にある）。Replay/未設定→Live(Manual/Auto) の実遷移でのみ送り、同一モード no-op では送らない gating が肝（#29 Slice 2'） | `kind:state`（lib test）。observer `request_force_account_snapshot_on_live_entry` を `src/backend_sync.rs` の lib test で駆動: `ExecutionModeChanged{LiveManual}` 注入で `TransportCommand::ForceAccountSnapshot` 送出を assert（`live_mode_entry_requests_force_account_snapshot`）、reset→`AccountEvent` 再 push で `PortfolioState` 再充填の seam を `live_entry_resets_then_account_event_refills_portfolio` で characterize。いずれも `cargo test --lib` | H8 ✅(kind:state, lib) |
| Orders 接続/再起動 seed（GetOrders スナップショットで OrdersPanel に完全な注文行が復元される） | proto `OrderEvent` が symbol/side/qty/price を運ぶようになり、`GetOrders` 応答から完全な `LiveOrder` 行を組んで `OrdersSeeded` で送る。seed は merge-safe（`LiveOrders::seed_working`）: 未知 id は完全行を挿入、既知 id は記録済みの monotonic fill と既知静的属性を壊さず空欄のみ gap-fill。背景 sync なので feedback は触らない（#29 Slice 3a・基盤。立花 working-orders 取得（CLMOrderList）と接続時トリガは Slice 3b に分離） | `kind:state`。`OrdersSeeded{orders}` を注入し `live_orders()` で完全行・gap-fill・feedback 不変を assert（`h9_orders_seeded_full_rows`）。merge 不変条件（monotonic fill / 非空が勝つ）は `src/trading.rs` の `seed_working_*` lib test で固定 | H9 ✅(kind:state) |
| Orders 接続時 seed — venue working-orders 取得（立花接続時に CLMOrderList で venue 側の既存注文を seed する） | `GetOrders` ハンドラが facade 注文に加えて `adapter.fetch_working_orders()`（CLMOrderList）をマージ。Rust 側は venue CONNECTED 遷移を observer で検知し `GetOrders` を発火（`request_get_orders_on_venue_connected`）。facade に存在しない venue_order_id の注文は追加、既知は facade 側が勝つ（#29 Slice 3b） | `kind:state`（lib test + Python pytest）。Rust: `venue_connected_requests_get_orders` lib test で Disconnected→Connected 遷移時の `GetOrders` 送出・二重送出しない・離脱時は送らないを assert（`cargo test --lib`）。Python: `test_parse_order_list_response_*` でコーデック、`test_fetch_working_orders_*` で adapter、`test_get_orders_merges_venue_working_orders` でマージ（`cd python && uv run python -m pytest`）。実機 E2E: 立花 demo 接続時に既存注文が Orders パネルに seed される目視確認が必要 | H10 ✅(kind:state, lib+pytest; 実機 E2E 未確認) |
| Orders 取得タイムアウト notice（venue working-orders 取得が失敗/タイムアウトしたら footer に明示 notice を出し、サイレントに「注文なし」扱いさせない） | `GetOrders` 応答の `error_code` が非空なら transport task が `get_orders_notice` の `OrderNotice` を送り、`apply_status_update` が `order_feedback.message` に焼く（#29 Slice 3b Medium-4） | `kind:state`。`OrderNotice{message}` を注入し、取得失敗前は `order_feedback().message` が空・注入後に notice 文字列が feedback line に出ることを assert（`h11_venue_orders_timeout_notice`） | H11 ✅(kind:state) |
| Orders 接続時 seed は reconcile しない（venue CONNECTED 直後の GetOrders は完全行 seed のみで OrdersReconciled を撃たず、venue-only working order が unknown 誤判定されない） | connect-seed（`GetOrders`）と auto-restart（`GetOrdersAndReconcile`）を共通 `seed_orders_from_backend(.., reconcile)` に括り、`reconcile == false`（接続時）は seed だけ・`reconcile_ids_for_seed` が `None` を返し `OrdersReconciled` を撃たない。restart 時のみ id-diff reconcile を送る。背景 sync なので feedback は触らない（#29 Slice 3a codex Med-A） | `kind:state`（lib test + bin test）。lib: `reconcile_ids_for_seed(seeded,false)==None` / `(seeded,true)==Some`（空 client_id 除外）を `reconcile_ids_for_seed_returns_none_when_not_reconciling` / `reconcile_ids_for_seed_returns_nonempty_client_ids_when_reconciling` / `reconcile_ids_for_seed_excludes_empty_client_ids` で assert（`cargo test --lib`）。bin: reconnect 後の TransportCommand flush で `GetOrdersAndReconcile` のみ survive し `GetOrders` は捨てられることを `reconnect_flush_preserves_only_get_orders_and_reconcile` で assert（`cargo test --bin backcast`） | H12 ✅(kind:state, lib+bin) |

## ハーネス計画（参考・別途実装）

- **済**: 各 flow を手書きの `tests/e2e/flows/<id>.rs` として実装し、`tests/e2e_replay.rs` が
  `MinimalPlugins` の headless App に `BackendStatusUpdate` / `BackendEvent` / replay clock を注入して
  resource を assert する（`tests/e2e/support/mod.rs` の `Harness`）。CI 向き。
- **済（A–H の UI ジャーニー化）**: `Harness` は backend→ECS の注入だけでなく、本番 UI 入力 system も
  載せ（footer の Run/Pause/Resume/Step/Stop・モードトグル・速度、サイドバー行クリック/× 削除、Venue メニュー
  Connect/Disconnect、注文フォーム→確認モーダル、SecretModal、universe auto-fetch）、`TransportCommandSender`
  の受信側を保持する。A–H 各 flow は **まず実 UI 操作を本番 system で駆動**（`click(marker)` で `Interaction::Pressed`
  を注入 / `run_via_ui` / `place_order_via_ui` / `type_secret`）して発射された `TransportCommand` を `drain_commands`
  で assert し、**その後 backend 応答を seam から注入**して resource 遷移を assert する。これで「ユーザー操作 →
  コマンド」の前半が本番経路で保証される。G1–G3 は起動時の transport 自動接続が縫い目でクリック UI が無いため
  backend-seam のみ（前置 UI 無し）。
- **Phase A-full（未）**: App 組み立てとトランスポートタスクを `main.rs` から lib へ抽出し、
  `TransportCommand` 注入 → mock gRPC（`backend_integration.rs` の `MyDataEngine` を `tests/e2e/support/`
  へ共有抽出）→ `RunState` 観測 の単一プロセスループを閉じる。**UI→`TransportCommand` の前半は上記の本番 UI
  system 駆動で既に閉じている**。残るのは発射コマンドを実 gRPC に通して `BackendStatusUpdate` として戻す往復のみ。
  - **ここで初めて pin できる未カバー挙動**: `ListAllListedSymbols` が end_date を clamp（未来日→Catalog 最新日, ADR-0002）した
    とき、フロントは `AvailableInstrumentsLoaded` を **要求時の end_date** でキーする（`main.rs` transport task, *resolved* では
    ない）。これが resolved 側に退行すると `in_flight[要求日]` が永久に消えずピッカーが無限 Loading になる。ロジックが
    `main.rs` の tokio タスク内に inline で Harness の seam 注入経路を通らないため、現状は doc gap（fake テストは置かない）。
- **Phase B（未）**: `--e2e` / `BACKCAST_E2E=1` のウィンドウ実行モード（固定ウィンドウ・固定パス・
  構造化ログ）で、`.rs` flow と同じシナリオを実描画で smoke 実行（`kind:render` / `L4`）。

### ディレクトリ構成

```
tests/
├── e2e_replay.rs     ← runner（#[path] で flows/ と support/ を取り込む単一テストバイナリ）
└── e2e/
    ├── FLOWS.md      ← このメタ文書（凡例 / 代替方式 / 計画）
    ├── flows/        ← 各 flow の Rust テスト *.rs（1 ファイル 1 #[test]・先頭 //! が解説）
    ├── fixtures/     ← strategy .py / scenario sidecar JSON など素材（未作成・将来用）
    └── support/      ← 共有 Rust ヘルパ（headless app builder = Harness / mock engine）
```
