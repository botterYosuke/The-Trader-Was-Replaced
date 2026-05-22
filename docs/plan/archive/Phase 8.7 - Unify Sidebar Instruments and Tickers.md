# Phase 8.7 — Sidebar Instruments / Tickers 統合（v3 完成形）

Phase 8 §3.5 で並走させた sidebar **Instruments**（`InstrumentRegistry` 駆動 + Chart 1:1）と **Tickers**（venue universe ビュー）を、単一の Instruments セクションに統合する。v3 は v2 のコードレビュー（致命点 6 件 + Replay engine の単一銘柄前提）を受け、**Q1「fixed scenario の `instruments_ref` re-resolve」をやる方針で確定** し、関連する backend / replay engine / adapter lifecycle / unsubscribe wiring を Phase 8.7 のスコープに正式に取り込む。

---

## §00 仕様サマリ（人の言葉で）

この章は Decision Log や §1 以降の実装節を読む前の **読み物**。Phase 8.7 が何を約束するかを宣言する。詳細・gate 条件・テスト名は §1 以降の各節に書く。

### 1 つのリスト、3 つの責務

Sidebar 左側の "Instruments" は、ユーザーから見れば 1 つのリスト。中身を決める責務は 3 つあって、それぞれ別の部品が担当する:

- **「今、画面に Chart を出している銘柄は何か」** — Instruments 行に並ぶものの正体。`InstrumentRegistry` がその真実を持ち、`instrument_chart_sync_system` が registry の中身を Chart panel に 1:1 で反映する。registry に id があれば Chart が浮かび、無くなれば Chart は消える。
- **「ユーザーが今、新しい銘柄を追加できるか」** — `[+ Add]` を押した時に出る dropdown の中身。これは **モードで切り替わる**。Replay モードなら scenario の終了日時点で過去データが存在する銘柄（Backtest universe）。Live モード（Manual / Auto）なら venue が今配信している銘柄（Live universe）。両者は性質も時間軸も違う別物なので、混ぜずに mode で完全に分岐させる。
- **「今のモードで、Chart として表示してよい銘柄か」** — モードや venue 接続状態が変わった瞬間に、現在の universe から外れている銘柄は自動で registry から消す（= Chart も消える）。これが auto-prune。ユーザーが何もしなくても、universe と画面が常に整合する。

この 3 つは別の責務なので、別の gate で動く。`editable` フラグは「ユーザーがこの scenario を編集して sidecar に保存していいか」のフラグで、**manual add/remove と sidecar writeback の両方** を gate する（編集できない scenario は画面上でも一時追加・削除しない、という分担）。一方 auto-prune は `editable` を見ない — universe から外れた銘柄は editable に関係なく Chart を消す。永続化と表示可能性は別軸だが、ユーザーの意思に基づく編集だけは「ファイルとして編集可能か」と一致させる。

### モードごとの挙動

**Replay モード**:
- `[+ Add]` の dropdown には backend が catalog から拾った「scenario.end 時点で過去データが存在する銘柄」が並ぶ
- Live → Replay に切り替えた瞬間、Chart が出ている銘柄のうち catalog universe に存在しないものは自動で消える
- registry を編集すると（editable=true なら）scenario sidecar に writeback される

**Live モード（Manual / Auto）**:
- `[+ Add]` の dropdown には venue が今配信している銘柄が並ぶ
- venue にログインして接続が `Connected` になった瞬間、自動で live universe を取りに行く
- Replay → Live に切り替えた瞬間、または venue 接続状態が変わって universe が更新された時、Chart が出ている銘柄のうち live universe に存在しないものは自動で消える
- どんなに registry が変わっても、Live 中は scenario sidecar に書き戻さない（Replay 専用に保存する設定ファイルを Live の prune で汚さない）

**両モード共通**:
- Instruments 行に表示される銘柄は、Chart として spawn されているものに常に一致する
- 行の右側には最新価格が出る。Replay でも Live でも `TradingState.last_prices` という同じ schema を使うので、UI 側に mode 分岐は無い

### 自動 prune の安全弁

「universe に無い銘柄は消す」だけ書くと、起動直後の中途半端な状態で全銘柄が消し飛ぶリスクがある。なので prune は **universe が確かに分かっている時しか動かない**:

- venue から取得中、取得失敗、まだ取得していない、のいずれかなら prune は走らない
- backend が「Live universe を返す」と約束していない source（例えば Replay catalog の fallback データ）を Live モードの根拠に使わない
- universe 取得に失敗した時は、直前まで見えていた銘柄リストを画面上に残したまま、prune だけ止める

つまり「Tickers に無い銘柄を消す」の正確な意味は、**「venue が今この瞬間に配信していると確実に分かっている銘柄リストに無いものを消す」**。確証が無ければ何もしない。

### 固定 scenario との関係

`instruments_ref` を持つ固定 scenario（editable=false）を開いた状態で Live に切り替えると、auto-prune は走る（Chart は live universe に合わせて消える）。ただし sidecar ファイルは絶対に書き換えないし、`[+ Add]` / × によるユーザー編集も受け付けない（picker は disabled、× は no-op）。Replay に戻った時点で、固定 scenario の `instruments_ref` を再解決して registry を元の universe に復元する。固定 scenario はファイルとして不変、ユーザーも触れない、画面表示だけが mode に追従する、という分担。

編集可能な scenario（editable=true）の場合は、Live で prune されて減った registry はそのまま保持。Replay に戻った時点で **その減った状態を sidecar に writeback する**（Live 中は writeback gate で書かないため、再入の最初の tick で 1 回 flush される）。自動復元はしない。ユーザーが Live で「もう要らない」と意思表示したものと解釈する。

### 一行で

**「Instruments 行は Chart の鏡。universe（Replay catalog または Live venue）から外れたものは消える。consent された scenario なら sidecar にも反映するが、固定 scenario と Live mode はファイルを触らない。」**

---

## Decision Log（v3 確定）

### Phase 8.7 で「やる」決定

| # | 論点 | 決定 |
|---|---|---|
| **D1** | `ListInstruments` の Live universe 取得 | **backend に `source` dispatch を実装**（`"local"` = catalog、`"live"` = adapter `fetch_instruments`） |
| **D2** | `editable` と auto-prune の gate 分離 | `editable` は **manual edit + sidecar writeback の gate のみ**。auto-prune は別 gate（`editable` を見ない） |
| **D3** | Replay モードの行 price 表示 | backend `TradingState.last_prices` を **Replay でも id 別 close で埋める**。Rust 側は `is_replay` 分岐を捨て `LastPrices.map.get(id)` 一本化 |
| **D6** | `Tickers` schema 拡張 | `Tickers { list, source, status }` — `source: TickersSource { LiveVenue, LocalVenueSnapshot, ReplayCatalogFallback, Unknown }`, `status: TickersStatus { NotFetched, InFlight, Loaded, Failed(String) }`。**Failed 時は list を維持**して stale 表示しつつ prune は走らせない |
| **D6b** | Live prune の source 制限 | Live モードでの auto-prune は `source ∈ {LiveVenue, LocalVenueSnapshot}` の時だけ実行。`ReplayCatalogFallback` を根拠に Live Chart を消さない |
| **D6c** | universe fetch のライフサイクル | `InstrumentsListed` を **3 イベント分割**: `InstrumentsListStarted { source }` / `InstrumentsListed { source, instruments }` / `InstrumentsListFailed { source, error }` |
| **Q1** | `editable=false` + Live → Replay 戻り | **fixed scenario / `instruments_ref` を re-resolve して registry を replace**（writeback は引き続き禁止） |
| **Q2** | `editable=true` + Live → Replay 戻り | registry はそのまま（減ったまま）。sidecar reload しない。Replay 入った後 registry 変更があれば writeback |
| **Q3** | Live universe fetch trigger | startup 1 回（Replay catalog）に加え、`VenueState::Connected` 遷移時に `ListInstruments(source="live")` を発火 |
| **Q4** | `[+ Add]` の picker source | `ExecutionMode` で分岐: Replay→`AvailableInstruments`、Live*→`Tickers.list`。`Tickers.status` を picker placeholder に反映 |

### v2 レビューを受けて新規追加した決定

| # | 論点 | 決定 | 背景 |
|---|---|---|---|
| **D7** | `instruments_ref` resolver の復活 | **`instruments_ref` を Phase 8.7 で正式 schema に戻し、resolver を実装** する。`scenario.py validate` は `instruments_ref` を unknown key として reject しているのを許可 key に戻し、`scenario_parser.rs` の has-flag 検出を「resolve して `instruments` に展開する」処理に格上げ。新規 RPC または event を切らず、`StrategyFileLoadRequested` 経路をそのまま流用する | Q1 を「やる」と決めた以上、現状の `has_instruments_ref` boolean フラグ + Python 側 reject では restore できないため |
| **D8** | `GetState` ハンドラ側の `last_prices` 上書きを mode-aware に | server_grpc.py `GetState` の `last_prices` 上書きを、**Live/Replay で分岐**: Live なら `_live_price_cache.snapshot()`、Replay なら `engine.get_replay_last_prices()`（新規 method）を使う。`engine.get_current_state()` の `last_prices` は今後使わず、必ず server 側で組み立てる方針を維持 | 現状 server が常に `_live_price_cache.snapshot() or {}` で上書きしており、D3 を core.py だけで直しても表面に出てこない |
| **D9** | Replay engine の multi-instrument 化 | core.py の `_replay_provider` を **`dict[instrument_id, NautilusBarsReplayProvider]`** に拡張。`_advance_one_locked` は **`min_ts` と等しい ts を持つ全 provider を 1 tick でまとめて drain**（D24 で確定。同 ts に複数銘柄がある場合に「同時」表示を厳密に成立させるため）し、id 付き `KlineUpdate` をグループ全件発行。`ReplayTimeUpdated` は同 ts グループにつき 1 回。`KlineUpdate` / `TradeUpdate` に `instrument_id: str` field 追加。`ReducerState` に `per_id_close: dict[str, float]` を追加し、`apply_event` で更新。`load_replay_data` は `instrument_ids` の全要素から provider を作る | D3 を真に成立させるには Replay 経路で「同 tick 内に複数銘柄」が流せる必要がある。現状 `instrument_ids[0]` しか使わず `KlineUpdate` に id 無しで、id 別 close の出所が無い |
| **D10** | Live adapter の lifecycle 公開 | `GrpcDataEngineServer` 自身は adapter を field で持たず、**`LiveRunner` に `adapter` プロパティと `fetch_instruments_blocking(timeout)` method を公開**。`is_logged_in` 状態も `LiveRunner.is_logged_in() -> bool` 経由で参照。`VenueLogin` の `NOT_IMPLEMENTED` は別 Phase（Phase 8.8）扱いとし、Phase 8.7 では **`MockVenueAdapter` の `is_logged_in` を `_start_live_components` 成功で True とみなす** 暫定実装で進める | 現状 server に `_live_adapter` field が無く、`_live_adapter_factory` / `_live_runner` / `_live_bridge` だけ。さらに `VenueLogin` 自体が `NOT_IMPLEMENTED` を返すので、adapter login lifecycle と `fetch_instruments` の接続点が無い |
| **D11** | Replay 入場時の `AvailableInstruments` 自動 fetch | `ExecutionMode::Replay` への**遷移時** および `ScenarioMetadata.end` 変更時に、`FetchAvailableInstruments { end_date }` を自動 dispatch する system `auto_fetch_available_on_replay_entry_system` を新設 | 現状 `FetchAvailableInstruments` は `[+ Add]` 押下時のみ発火。auto-prune の Replay 経路は `available.by_end_date.get(&end)` に依存しているため、ユーザーが picker を開かない限り prune が永遠に skip される |
| **D12** | Live mode の Unsubscribe 配線 | `TransportCommand::UnsubscribeMarketData { instrument_id }` variant を追加。**auto-prune / Chart close / manual remove で registry から消えた id を diff 検出し、Live mode の時だけ** backend へ `UnsubscribeMarketData` を送る system `unsubscribe_removed_instruments_system` を新設 | backend には RPC があるが Rust 側 `TransportCommand` に variant 無し。これが無いと kabu の 50 銘柄上限・stale `LastPrices` リークが発生 |
| **D13** | 計画書 §5.1 コードの compile 修正 | `parse_scenario_end(&scenario)?` は `Option` を返す関数内専用なので `let Some(end) = parse_scenario_end(&scenario) else { return; };` に直す。`TickersSource` の import を `instruments_universe_prune.rs` 冒頭に追加 | v2 レビュー指摘事項 #5 |
| **D14** | Mock/Dev adapter の login 自動化（Phase 8.7 暫定） | **配置先は `_start_live_components_async` ではなく D21 の `VenueLogin` RPC ハンドラ内**（§2.2.1）。`_start_live_components()` で runner/bridge/cache を bootstrap した直後に、ハンドラが `if not getattr(adapter, "is_logged_in", False): await adapter.login(VenueCredentials(credentials_source=<request 由来>))` を呼ぶ。**`credentials_source` は pydantic v2 必須フィールド** (`Literal["prompt", "session_cache", "env"]`、default 無し) なので引数なし構築は `ValidationError`。Phase 8.7 では mock adapter のみ実 login を実行（real adapter (tachibana / kabu) は `is_logged_in == True` を返す stub のままで no-op、本実装は Phase 8.8）。`LiveRunner.is_logged_in()` は `getattr(self._adapter, "is_logged_in", True) and self.bus is not None`（**field 名 `self.bus` に修正**） | v3 レビュー: 現行 `_start_live_components_async` は `adapter.login()` を呼ばないため `MockVenueAdapter.is_logged_in == False` のままで `fetch_instruments` が `_require_login` で例外。さらに plan 擬似コードの `self._bus` は実 field 名 `self.bus` と不一致。v3.1 レビュー: `VenueCredentials` の `credentials_source` フィールドは `Literal["prompt", "session_cache", "env"]` で default 無しのため `VenueCredentials()` は構築不能。**v4 レビュー: D14 を `_start_live_components_async` に置くと D21 の循環依存に巻き戻る。配置を `VenueLogin` ハンドラに修正** |
| **D15** | 既存 venue-transition fetch を新 system へ統合 | [src/main.rs:782-789](../../src/main.rs#L782) の `if matches!(vs, VenueState::Connected \| VenueState::Subscribed) { fire_list_instruments(&client, &token, None, &status_tx); }` ブロックを**削除**し、Live universe 取得は `auto_fetch_live_universe_on_connect_system`（§4.6.1）に一本化する。`fire_list_instruments` 自体は startup 経路（main.rs:372 `ReplayCatalogFallback`）のため残す。これにより同 trigger で 2 本 fetch が走る race（`source=None` 由来の local 結果が後着で `LiveVenue` 結果を上書きして D6b prune 判定が崩れる）を回避 | v3 レビュー: 現行 `fire_list_instruments(..., None, ...)` は `source=None`（backend 側 `"local"` 扱い）で発火し、新 `auto_fetch_live_universe_on_connect_system` の `LiveVenue` fetch と重複する |
| **D16** | `StartEngine` 後の Replay bar injection を全 instrument 化 | [server_grpc.py:443-450](../../python/engine/server_grpc.py#L443) の `for bars in bars_by_instrument.values(): ... break` の `break` を**外し**、`bar_to_kline_update(bar, instrument_id=iid)` の形で id 付き `KlineUpdate` を全銘柄分流す。`bar_to_kline_update` 側に `instrument_id: str = ""` キーワード引数を追加（D9 の `KlineUpdate.instrument_id` field 追加と対）。これにより D9 + D8 が成立し、`GetState.last_prices` が全 instrument 分埋まる | v3 レビュー: 現行は `break` で primary 1 銘柄しか inject せず、しかも `bar_to_kline_update` が `instrument_id` を取らないため D9 の `per_id_close` が primary すら埋まらない |
| **D17** | `LoadReplayData.instrument_ids` の契約統一 | **「instrument id ("1301.TSE" 形式) を渡し、backend が BarType に変換する」** に統一する。`server_grpc.LoadReplayData` ハンドラ内で `bar_type = instrument_id_to_bar_type(iid, granularity)`（新規 helper）に変換してから `NautilusBarsReplayProvider(bar_type=bar_type, ...)` を作る。helper は `f"{iid}-1-MINUTE-LAST-EXTERNAL"` 等 granularity から組み立て。[engine.proto:124](../../python/proto/engine.proto#L124) のコメント「instrument_ids[0] is used as the BarType identifier」を「instrument_ids are full instrument IDs (e.g. "1301.TSE"); backend converts to BarType using the requested granularity」に書き換え。既存 `test_nautilus_catalog_route` の「BarType 文字列を直接渡す」ケースは `NautilusBarsReplayProvider` 単体テストとして残し、E2E 経路は instrument id 渡しに更新 | v3 レビュー: D9 擬似コードが同じ `iid` を「`bar_type=iid`（BarType 文字列）」「`_replay_primary_id`（instrument id）」「`per_id_close[iid]`（registry キー）」の 3 役で使い回しており、Rust `InstrumentRegistry.ids` ("1301.TSE" 形式) と整合しない |
| **D18** | Live mode 入場経路の確保（`venue_sm` mock 遷移） | **配置先は `_start_live_components_async` ではなく D21 の `VenueLogin` RPC ハンドラ内**（§2.2.1）。ハンドラが adapter.login (D14) 成功後に `venue_sm.transition_to("AUTHENTICATING")` → `venue_sm.transition_to("CONNECTED")` を順に呼んで `venue_sm.current == "CONNECTED"` まで進める。`GetState.venue_state` 経由で Rust `VenueStatusRes` に伝播することを確認。**`ModeManager.set_execution_mode("LiveManual")` の precondition `venue_state ∈ {CONNECTED, SUBSCRIBED}` が満たされるまで踏ませる**のが目的。real adapter (tachibana/kabu) の `venue_sm` 管理は Phase 8.8 の `VenueLogin` 本実装が担う | v3.1 レビュー: D14 で `adapter.is_logged_in == True` にしても `mode_manager.py:18-23` は `venue_sm.current` を見るため `LiveManual/LiveAuto` への遷移が `EXECUTION_MODE_PRECONDITION` で失敗し、UI から Live モードに入れず `auto_fetch_live_universe_on_connect_system` も発火しない。`venue_sm.transition_to(...)` を呼ぶコード経路が現状コードベースに 1 つも存在しないことを `grep` で確認済み |
| **D19** | `Tickers` の VenueState 鮮度 invalidation | Rust 側に `invalidate_tickers_on_venue_disconnect_system` を新設。`VenueStatusRes.state` が `Connected/Subscribed` 以外（`Disconnected` / `Reconnecting` / `Error` / `Authenticating`）に**遷移した瞬間**に `Tickers { list: list 維持, source: TickersSource::Unknown, status: TickersStatus::NotFetched }` にリセットする。**list 自体は UI 表示用に維持**（picker placeholder の "Venue not connected" 経路に乗せるため）。**`prune_instruments_outside_universe_system` の Live gate に `VenueState::Connected/Subscribed` AND を追加**（§5.1 改修）。これにより disconnect / venue 切替後の古い `LiveVenue` リストを根拠に Chart が消える race を遮断 | v3.1 レビュー: 現行計画 §5.1 の Live prune gate は `tickers.status == Loaded && tickers.source ∈ {LiveVenue, LocalVenueSnapshot}` のみで、venue が落ちても Tickers は Loaded のまま残るため stale universe で prune が走る |
| **D21** | Live 入場経路の構造修正（最小 `VenueLogin` RPC） | `SetExecutionMode` 内の `_start_live_components()` 呼び出しを **削除し**、代わりに **最小 `VenueLogin` RPC ハンドラ** を新設する。ハンドラ責務は (1) `_live_adapter_factory` から adapter を生成、(2) `_start_live_components_async(adapter)` を回す、(3) `await adapter.login(...)`（D14）、(4) `venue_sm.transition_to("AUTHENTICATING") → "CONNECTED"`（D18）。**v5.2 追加**: `_teardown_live_components` で `venue_sm` を `DISCONNECTED` にリセットする（Replay 戻り時に CONNECTED が残ると次の `VenueLogin` が冪等 early return で login を skip し、unlogged adapter が立ち上がる経路が成立するため）。`SetExecutionMode("LiveManual")` は **既に CONNECTED な前提** で precondition を通過し、**`_live_runner is None` の場合は冪等再起動ではなく `VENUE_LOGIN_REQUIRED` で reject する**（v5.2 Claim 2: 冪等再起動だと unlogged adapter が立ち上がる race を防げない）。UI side は startup または既存 `Venue Login` ボタンから `TransportCommand::VenueLogin` を送信して venue_sm 遷移をトリガし、`GetState.venue_state` 経由で `VenueStatusRes::Connected` に届いてから初めて `SetExecutionMode` を許可する（footer.rs 既存 gate がそのまま機能） | **v4 致命指摘**: D14/D18 を `_start_live_components_async` に置くと、その関数の呼び出し元は `SetExecutionMode` 1 箇所のみ ([server_grpc.py:853](../../python/engine/server_grpc.py#L853))、しかも呼び出しは `mode_manager.set_execution_mode()` precondition 通過**後**。precondition は `venue_sm.current ∈ {CONNECTED, SUBSCRIBED}` を要求する ([mode_manager.py:18](../../python/engine/mode_manager.py#L18)) ため、D14/D18 へ到達できない循環依存になる。UI 側も `VenueState::Disconnected/Error` で `SetExecutionMode` を握り潰す ([footer.rs:652](../../src/ui/footer.rs#L652))。最小 `VenueLogin` で venue_sm 遷移を `SetExecutionMode` の外に出すことで循環を断つ。`VenueLogin` 本実装（credentials 入力 UI / reconnect / lifecycle 管理 / real adapter 対応）は引き続き Phase 8.8 送り（mock/dev adapter は credentials="env" 固定で十分） |
| **D22** | Picker add → Live mode 自動 Subscribe | **新規 system `subscribe_added_instruments_system`** を追加し、registry に追加された id を diff 検出して Live mode のときだけ `SubscribeMarketData` を発火する。Phase 8.7 で Tickers セクション撤去後、`[+ Add]` picker → 行 click という 2 段 UX を 1 段に縮約するため。mode 切替直後 frame は `prev_ids` 更新のみで skip し、Replay → Live 切替時の大量再 subscribe を防ぐ。`unsubscribe_removed_instruments_system` (D12) と対称 |
| **D23** | Live universe fetch trigger に ExecutionMode 遷移を追加 | `auto_fetch_live_universe_on_connect_system` の trigger を「VenueState live 新規遷移 **OR** ExecutionMode Live 新規遷移」の OR 条件に拡張。Tickers.status が `Loaded/InFlight` なら skip。Replay 中に Venue Connect 済み → 後で Live 切替えるケースで fetch が永遠に発火しない bug を防ぐ |
| **D24** | Replay multi-instrument の同 ts drain | `_advance_one_locked` で `min_ts` と等しい ts を持つ全 provider を 1 tick でまとめて pop / KlineUpdate 発行に変更。DoD #6「複数 Chart の price が同時に sidebar に表示」を厳密に成立させる。`ReplayTimeUpdated` は同 ts のグループにつき 1 回だけ発行 |
| **D25** | Python / Rust resolver の責務分離明記 | `instruments_ref` resolve は backend (`scenario.py`) と UI (`scenario_parser.rs`) の両方に実装するが、**同じファイル形式仕様を共有** する不変条件を §2.5 冒頭に明記。Phase 8.7 では bare path + 最小 JSON pointer のみ。将来拡張時は共通仕様 doc に切り出して 2 重実装の drift を防ぐ |
| **D26** | **MOCK venue を Phase 8.7 の通常導線に追加** | `build_live_adapter_factory` に `"MOCK" → MockVenueAdapter()` 分岐を追加し、`server_grpc.GrpcDataEngineServer._KNOWN_VENUES` を `{"TACHIBANA", "KABU", "MOCK"}` に拡張。menu_bar に `MenuItem::VenueConnectMock`（"Venue → Connect → Mock"）を追加し `send_venue_login(&sender, "mock", "demo")` を発火。`MockVenueAdapter.login` は credentials_source を問わず `is_logged_in=True` に倒すので `"prompt"` のままで通る。`_KNOWN_VENUES` に `MOCK` を含めないと §2.2.1 ハンドラ冒頭で `UNKNOWN_VENUE` reject になる点に注意。**Phase 8.7 DoD #5/#11 は MOCK venue で満たす**（real venue (tachibana/kabu) は §2.2.1 ハンドラが no-op login のまま `fetch_instruments` で session-less 例外を返すため、universe 取得まで通電するのは MOCK のみ。real venue 経路は Phase 8.8 の `VenueLogin` 本実装で `credentials_source="env"` を実 login に流して通電させる） | **v5.1 致命指摘**: 現行 `live_adapter_factory.py` は `TACHIBANA` / `KABU` のみ ([live_adapter_factory.py:20-30](../../python/engine/live/live_adapter_factory.py#L20))、`menu_bar.rs` の Venue→Connect も `tachibana` / `kabu` のみ ([menu_bar.rs:425-435](../../src/ui/menu_bar.rs#L425))、`_KNOWN_VENUES` も `{"TACHIBANA","KABU"}` ([server_grpc.py:158](../../python/engine/server_grpc.py#L158))。MOCK adapter を通常導線で立ち上げる経路が **どこにも存在しない**。一方 tachibana adapter は `is_logged_in` 属性自体を持たないため、§2.2.1 の `getattr(adapter, "is_logged_in", True)` は default `True` を返して `login()` を skip → `fetch_instruments` が `_session is None` で `RuntimeError` を投げる ([tachibana.py:115-118](../../python/engine/exchanges/tachibana.py#L115))。よって Phase 8.7 を画面で完走させる唯一の手段は MOCK factory + menu_bar entry の追加 |
| **D27** | **Live Kline 経由 last price ingest** | `LastPriceCache._run` で `KlineUpdate` を **`pass` から `self._last_kline[evt.instrument_id] = evt.close` に変更**。`snapshot()` の優先順位を `quote_mid > last_trade > last_kline` に拡張。`remove(id)` も新規 dict から pop。**理由**: `LiveRunner._run` は `TradesUpdate` を **aggregator に渡して closed bar (= `KlineUpdate`) のみ bus.publish** する ([live_runner.py:96-102](../../python/engine/live/live_runner.py#L96)) ため、`LastPriceCache` には raw `TradesUpdate` が **届かない**。Depth が来ない adapter (trade-only / kline-only venue や mock の depth 欠落シナリオ) では `quote_mid` / `_last_trade` が永遠に空のまま、Live の sidebar price 列が完全に死ぬ。D3「Live でも `TradingState.last_prices` で UI mode 分岐ゼロ」の invariant を Live 側でも実成立させるための最小修正。`KlineUpdate` を ignore する既存テスト ([test_last_price_cache.py](../../python/tests/test_last_price_cache.py) の該当 assert) は「`KlineUpdate` の `close` を `_last_kline` に取り込み、`quote_mid` / `last_trade` が無い銘柄で snapshot 経路に出る」を pin するテストに書き換え | **v5.1 致命指摘**: 上記 |
| **D28** | Rust resolver も fail-closed 化 | `scenario_parser.rs` の `instruments_ref` resolver 失敗時、当初案の「error log + 空 vec」を **撤回** し、Python 側と同じく `ScenarioLoadedFromFile` を送出せず早期 return する。registry は維持される。**さらに既存 fixture が参照する `examples/refs/universe_topix_core.json` を新規 commit する**（現状 repo に存在せず、fail-closed 化と同時に既存テストが壊れるため）。fixture 中身は `["1301.TSE", "7203.TSE"]` 程度の list[str] で十分 | **v5.2 Claim 1**: Python は fail-closed (`ScenarioValidationError` raise) なのに Rust だけ silent fallback だと、sidecar `instruments_ref` 1 箇所の破損で UI 側 registry が空になり Chart が一斉に消える事故が起きる。さらに既存 fixture (`e2e_instruments_ref_locked.json` / `e2e_instruments_ref_mixed_locked.json`) が `refs/universe_topix_core.json` を参照するが target file が repo に無い |
| **D29** | Rust `ExecutionMode::default()` を `Replay` に揃える | [src/trading.rs:453-461](../../src/trading.rs#L453) の `#[default]` を `LiveManual` → `Replay` に移す。`ExecutionModeRes::default()` も連動して `mode: Replay` になる。テスト `assert_eq!(ExecutionMode::default(), ExecutionMode::LiveManual)` ([trading.rs:946](../../src/trading.rs#L946)) と `ExecutionModeRes::default().mode == LiveManual` ([trading.rs:975](../../src/trading.rs#L975)) を `Replay` に書き換え | **v5.2 Claim 3**: backend `ModeManager.current_mode = "Replay"` ([mode_manager.py:12](../../python/engine/mode_manager.py#L12)) と食い違っており、初回 `GetState` 受領前の数 frame で UI が Live モードとして振る舞う。Phase 8.7 では picker source / writeback gate / subscribe / prune が `ExecutionModeRes` に強依存するため影響が大きい |
| **D22-ext** | Replay → Live 切替時の survivor bulk subscribe | `subscribe_added_instruments_system` (D22) を改修し、`mode_changed && Live` の frame で survivor (`prev_ids ∩ current`) を上限 50 銘柄まで 1 回 bulk subscribe する。kabu 50 銘柄上限と整合。`§4.3.1` 擬似コードに反映済み | **v5.2 Claim 4**: 当初案 D22 は mode 切替 frame で diff を完全 skip していたため、Replay → Live で生き残った既存 Chart が「画面では見えるが price 来ない・order 不可」の半接続 UX を生む。Phase 8.7 内で取り込む |
| **D20** | `LastPriceCache.remove(id)` と Unsubscribe 連動 | `python/engine/live/last_price_cache.py` に `def remove(self, instrument_id: str) -> None: self._quote_mid.pop(instrument_id, None); self._last_trade.pop(instrument_id, None)` を追加。`server_grpc.py` の `UnsubscribeMarketData` RPC ハンドラ（[server_grpc.py:816 付近](../../python/engine/server_grpc.py#L816) 周辺、新規 or 既存ハンドラに追記）内で `runner.unsubscribe(id)` 成功後に `self._live_price_cache.remove(id)` を呼ぶ。**さらに `MockVenueAdapter.unsubscribe`/`adapter.unsubscribe` 成功と `LastPriceCache.remove` の同期を fail-safe にするため、`GetState` の Live last_prices 構築側でも `runner.subscribed_ids()`（新規 getter）で snapshot を filter する二段ガード**を入れる | v3.1 レビュー: 現行 `LastPriceCache.snapshot()` は内部 `_quote_mid | _last_trade` の全 id を返却し、unsubscribe / id 削除手段が存在しない。D12 で Rust → backend の `UnsubscribeMarketData` 配線まで通しても backend 側で stale 価格が残り続け、行が再 add された瞬間に古い価格が出る |

### Phase 8.7 で「やらない」と確定したもの

| # | 論点 | 決定 |
|---|---|---|
| **OUT-1** | `VenueLogin` の **本実装**（credentials prompt UI / session_cache 復元 / reconnect / disconnect / real adapter lifecycle） | Phase 8.8 送り。Phase 8.7 では D21 の最小 RPC（mock/dev adapter 限定、`credentials_source` は request 透過、login → venue_sm CONNECTED 遷移 + live components 起動のみ）で Live 入場経路だけ通電させる。menu_bar 現行 UI は `"prompt"` を送るが mock adapter は環境変数 fallback で受理する |
| **OUT-2** | `LocalVenueSnapshot` 経路の実装 | enum 値は schema に確保するが発火経路は別 Phase。`tickers_source_to_wire` では `"local"` として backend に送る |
| **OUT-3** | proto schema 追加 | `source` enum 化等は将来。既存 `Instrument` / `ListInstrumentsRequest.source` / opaque JSON `TradingState` で全部済ませる |
| **OUT-4** | Chart panel 内部描画 / 注文 hooks / Phase 9 / Phase 10 | 別 Phase |

責務原則（変更なし）:
- `editable` = **永続化ポリシー**（ユーザーが scenario を編集して保存していいか）
- auto-prune = **表示可能性**（今 Chart 出していい銘柄か）
- price = `TradingState.last_prices` 統一スキーマ（mode 不問）
- universe 真実 = `TickersSource` の `LiveVenue` / `LocalVenueSnapshot` のみ。`ReplayCatalogFallback` は Live universe を語れない

---

## §0 Scope

### in（v3 で確定）

**Python backend**:
- `ListInstruments` の `source` dispatch（D1）
- Replay engine の multi-instrument 化（D9 + D24）: `KlineUpdate.instrument_id`、`ReducerState.per_id_close`、`_replay_provider: dict`、`_advance_one_locked` の **同 ts group drain**（`min_ts` と等しい全 provider を 1 tick で pop、`ReplayTimeUpdated` は同グループに 1 回）
- `engine.get_replay_last_prices() -> dict[str, float]` の新設（D8）
- `GetState` ハンドラの `last_prices` 上書きを mode-aware 化（D8）
- `LiveRunner.adapter` / `LiveRunner.is_logged_in()` / `LiveRunner.fetch_instruments_blocking(timeout)` の公開（D10）
- **最小 `VenueLogin` RPC ハンドラ実装**（D21）: 現行 `NOT_IMPLEMENTED` を置換。adapter 生成 → `_start_live_components_async(adapter)` → `adapter.login(VenueCredentials(credentials_source=<request 由来>, environment_hint=<request 由来>))`（D14、§2.2.1 参照）→ `venue_sm.transition_to("AUTHENTICATING") → "CONNECTED"`（D18）を順に実行。冪等（既に CONNECTED なら no-op）。**`credentials_source` は request 透過**（"prompt"/"session_cache"/"env" のいずれも受ける）。mock adapter は `"prompt"` でも環境変数 fallback で login 成功する
- `SetExecutionMode` ハンドラから `_start_live_components()` 呼び出しを **削除**（D21）。live components が未起動なら冪等再起動だけ行う安全ガードは残す（VenueLogin が失敗した稀な race のため）
- `LastPriceCache.remove(instrument_id)` 追加 + `UnsubscribeMarketData` RPC ハンドラ内で呼び出し（D20）
- **`LastPriceCache` の `KlineUpdate.close` ingest（D27）**: `_run` の `KlineUpdate` 分岐を `pass` から `self._last_kline[evt.instrument_id] = evt.close` に変更。`snapshot()` 優先順位を `quote_mid > last_trade > last_kline` に拡張。`remove(id)` は `_last_kline` も pop
- **MOCK venue factory 拡張（D26）**: `build_live_adapter_factory` に `"MOCK" → MockVenueAdapter()` 分岐追加。`GrpcDataEngineServer._KNOWN_VENUES` を `{"TACHIBANA","KABU","MOCK"}` に拡張
- `LiveRunner.subscribed_ids() -> set[str]` getter 追加（D20 二段ガード）
- `StartEngine` 後の Replay bar injection ループから `break` を外し、`bar_to_kline_update(bar, instrument_id=iid)` に変換（D16）
- `instrument_id_to_bar_type(iid, granularity)` helper + `LoadReplayData` ハンドラでの変換 + proto コメント書き換え（D17）
- `scenario.py validate` の `instruments_ref` 許可 + resolver（D7）

**Proto**: 変更なし（D1 既存 field の利用範囲拡大のみ）

**Rust UI / trading**:
- `Tickers { list, source, status }` schema 拡張（D6）
- `BackendStatusUpdate` の `InstrumentsListed` 3 分割（D6c）
- `TransportCommand::ListInstruments.source: TickersSource` 型化 + wire 変換（D6）
- `TransportCommand::UnsubscribeMarketData { instrument_id }` 追加（D12）
- `TransportCommand::VenueLogin` 既存 variant の活用（D21）。**menu_bar の `Venue → Connect → <Venue> <Env>` 既存ボタン** ([menu_bar.rs:425-435](../../src/ui/menu_bar.rs#L425)) から発火。Phase 8.7 では footer に新ボタンを追加しない、auto-startup system も作らない（mock 環境でも明示的な Venue Connect 操作を必須化）
- **menu_bar に `MenuItem::VenueConnectMock` 追加（D26）**: "Venue → Connect → Mock" メニュー項目を追加し `send_venue_login(&sender, "mock", "demo")` を呼ぶ。Phase 8.7 DoD #5/#11 はこの MOCK 経路で満たす。real venue (tachibana/kabu) は §2.2.1 ハンドラの no-op login のままで `_list_instruments_live` が `RuntimeError`/`LIVE_UNIVERSE_UNSUPPORTED` を返すため、universe 通電は Phase 8.8 まで mock 専用
- Sidebar Tickers セクション撤去 + Instruments 行に price 列・行 click 移植
- `[+ Add]` dropdown のモード分岐（Q4）
- `auto_fetch_live_universe_on_connect_system`（Q3）+ 既存 [src/main.rs:782-789](../../src/main.rs#L782) の `fire_list_instruments(..., None, ...)` ブロック削除（D15）
- `invalidate_tickers_on_venue_disconnect_system`（D19）— Connected/Subscribed 以外への遷移で `Tickers.status = NotFetched, source = Unknown` にリセット
- `auto_fetch_available_on_replay_entry_system`（D11）
- `prune_instruments_outside_universe_system`（D2/D6b）
- `unsubscribe_removed_instruments_system`（D12）
- `restore_fixed_registry_on_replay_entry_system`（Q1）
- writeback gate に `Replay` AND 追加
- `update_ticker_price_text_system` の `is_replay` 分岐撤去（D3 確立後）
- `scenario_parser.rs` を `instruments_ref` resolver 実装に格上げ（D7）
- **`ExecutionMode::default()` を `Replay` に変更（D29 / v5.2 Claim 3）**: [src/trading.rs:453-461](../../src/trading.rs#L453) の `#[default]` attribute を `LiveManual` から `Replay` に移す。backend `ModeManager.current_mode` の初期値が `"Replay"` であるため ([python/engine/mode_manager.py:12](../../python/engine/mode_manager.py#L12))、Rust 側 default を揃えないと初回 `GetState` を受け取る前の数 frame で UI が Live モードとして振る舞う（picker source が `Tickers`、subscribe/prune の Live 分岐が走る、footer gate を Replay 専用 UI が通らない等）。テスト `ExecutionMode::default() == ExecutionMode::Replay` と `ExecutionModeRes::default().mode == ExecutionMode::Replay` に更新

### out
- `VenueLogin` 本実装（credentials prompt UI / session cache / reconnect / real adapter lifecycle）（OUT-1）。最小実装は D21 で in
- `LocalVenueSnapshot` 発火経路（OUT-2）
- proto schema 追加（OUT-3）
- Chart panel 内部描画／注文 hooks／Phase 9／Phase 10（OUT-4）

---

## §1 責務マトリクス（完成形）

| 機能 | gate | 実装 system |
|---|---|---|
| manual add（`[+ Add]` row click） | `registry.editable == true` | `handle_picker_row_click` ([src/ui/instrument_picker.rs:415](../../src/ui/instrument_picker.rs#L415)) |
| manual remove（× / Chart close） | `registry.editable == true` | `instrument_remove_button_system` ([src/ui/sidebar.rs:362](../../src/ui/sidebar.rs#L362)) + chart close observer |
| auto-prune（universe 不一致） | **`editable` 無関係**。Live: `Tickers.status == Loaded && source ∈ {LiveVenue, LocalVenueSnapshot} && VenueState ∈ {Connected, Subscribed}`（D19）。Replay: `AvailableInstruments.by_end_date.contains_key(end)` | `prune_instruments_outside_universe_system`（新規） |
| Venue 切断時の Tickers 鮮度リセット | `VenueState` が `Connected/Subscribed` 以外に**遷移した瞬間** | `invalidate_tickers_on_venue_disconnect_system`（新規、D19） |
| sidecar writeback | `editable == true && ExecutionMode == Replay`（gate は **writeback_scenario_instruments_system 側のみ**。`mark_registry_dirty_system` は mode 不問で revision を bump し続け、Live で蓄積した分を Replay 再入の最初の tick で 1 回 flush する。詳細 §5.4） | `mark_registry_dirty_system`（gate なし）+ `writeback_scenario_instruments_system`（Replay + editable AND gate） |
| Replay 再入時 restore | `editable == false` のときだけ fixed scenario から再同期 | `restore_fixed_registry_on_replay_entry_system`（新規、Q1） |
| Replay 入場時 universe fetch | mode 遷移時 / `scenario.end` 変更時 | `auto_fetch_available_on_replay_entry_system`（新規、D11） |
| Live mode 入場時 universe fetch | `VenueState::Connected/Subscribed` への新規遷移時 | `auto_fetch_live_universe_on_connect_system`（新規、Q3） |
| Live mode unsubscribe | registry diff で消えた id があり、`ExecutionMode == Live*`、mode 切替直後 frame は skip | `unsubscribe_removed_instruments_system`（新規、D12） |
| Live mode subscribe (registry add 追従) | registry diff で増えた id があり、`ExecutionMode == Live*`、mode 切替直後 frame は skip | `subscribe_added_instruments_system`（新規、D22） |

---

## §2 Backend (Python) 変更

### 2.1 `ListInstruments(source=...)` の dispatch（D1 + D10）

[python/engine/server_grpc.py:624-672](../../python/engine/server_grpc.py#L624) を以下に置換:

```python
def ListInstruments(self, request, context):
    if request.token != self.token:
        context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

    source = (request.source or "local").lower()
    if source not in {"local", "live"}:
        return engine_pb2.ListInstrumentsResponse(
            success=False, error_message=f"unknown source: {source}"
        )

    if source == "live":
        return self._list_instruments_live(context)
    return self._list_instruments_local(context)

def _list_instruments_live(self, context):
    runner = self._live_runner
    if runner is None or not runner.is_logged_in():
        return engine_pb2.ListInstrumentsResponse(
            success=False, error_message="LIVE_VENUE_NOT_LOGGED_IN"
        )
    try:
        raws = runner.fetch_instruments_blocking(timeout=self._live_timeout_s)
    except Exception as exc:
        return engine_pb2.ListInstrumentsResponse(
            success=False, error_message=f"fetch_instruments failed: {exc}"
        )
    # v4 致命修正: KABU adapter は MVP で fetch_instruments が `return []` ([kabusapi.py:94]).
    # success=True + empty list を返すと Rust 側 Tickers が status=Loaded で
    # 空 universe を受け取り、§5.1 の Live prune が全 Chart を消す。
    # 「empty list ≠ 配信ゼロ」「empty list == adapter 未実装」を区別できない以上、
    # 空 list は Failed 扱いに倒す（list 維持、prune skip）。MVP 未実装 adapter から
    # 「banner 空 OK」が必要になったら adapter 側で sentinel raise に書き換える。
    if not raws:
        return engine_pb2.ListInstrumentsResponse(
            success=False, error_message="LIVE_UNIVERSE_UNSUPPORTED"
        )
    instruments = [
        engine_pb2.Instrument(
            id=f"{r.code}.{r.market}",
            name=r.name,
            market=r.market,
        )
        for r in raws
    ]
    return engine_pb2.ListInstrumentsResponse(
        success=True,
        instrument_ids=[i.id for i in instruments],
        instruments=instruments,
    )

def _list_instruments_local(self, context):
    # 既存ロジックそのまま（catalog Parquet scan）— L628-672 をこの method に移すだけ
    ...
```

### 2.2 `LiveRunner` への adapter 公開（D10）

`LiveRunner`（既存）に以下を追加:

```python
class LiveRunner:
    @property
    def adapter(self):
        return self._adapter

    def is_logged_in(self) -> bool:
        # Phase 8.7 暫定: adapter.is_logged_in（mock は D14 で start 時に True 化済み）
        # かつ bus が生きていれば logged_in とみなす。本実装は Phase 8.8。
        # NOTE: field 名は `self.bus`（public）。`self._bus` ではない。
        return getattr(self._adapter, "is_logged_in", True) and self.bus is not None

    def fetch_instruments_blocking(self, timeout: float):
        # asyncio loop 外（gRPC thread）から adapter.fetch_instruments() を回す
        loop = self._loop  # _ensure_live_loop で作った loop の参照を runner にも保持
        fut = asyncio.run_coroutine_threadsafe(self._adapter.fetch_instruments(), loop)
        return fut.result(timeout=timeout)
```

`GrpcDataEngineServer._start_live_components_async` で `runner._loop = self._live_loop` を 1 行差し込む（または runner 側 constructor に loop を取らせる）。**D21**: `_start_live_components_async` 自体は **adapter.login も venue_sm 遷移もしない** 純粋な component bootstrap に保ち、`VenueLogin` RPC ハンドラ（§2.2.1）と `SetExecutionMode` の両方から呼べる冪等関数にする（既存挙動どおり）。

### 2.2.1 最小 `VenueLogin` RPC ハンドラ（D21 + D14 + D18）

現行 `VenueLogin` は `NOT_IMPLEMENTED` を返すスタブ ([server_grpc.py 周辺](../../python/engine/server_grpc.py))。Phase 8.7 では **最小実装** に置換する。`SetExecutionMode` 側からは `_start_live_components()` 呼び出しを **削除** し（後述）、Live 入場経路を `VenueLogin → SetExecutionMode` の 2 段に分解する:

```python
def VenueLogin(self, request, context):
    if request.token != self.token:
        context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")

    # v4 致命修正: request.venue_id を必ず正規化 + validate する。
    # UI 側 (menu_bar.rs:426-435) は "tachibana"/"kabu" lowercase で送るが、
    # backend の _KNOWN_VENUES は {"TACHIBANA", "KABU"} uppercase ([server_grpc.py:158]).
    # 正規化抜きだと既存挙動 (UNKNOWN_VENUE) と整合せず、複数 venue 環境で誤起動する。
    venue_id = (request.venue_id or "").upper()
    # D26: _KNOWN_VENUES に "MOCK" を追加。menu_bar の Venue → Connect → Mock から
    # venue_id="mock" で到達する。MockVenueAdapter は credentials_source を問わず
    # is_logged_in=True に倒すので "prompt" のままで通る。
    if venue_id not in self._KNOWN_VENUES:
        return engine_pb2.VenueLoginResponse(
            success=False, error_code="UNKNOWN_VENUE",
            venue_state=self.venue_sm.current if self.venue_sm else "DISCONNECTED",
            instruments_loaded=0,
        )
    cred_source = request.credentials_source or "prompt"
    if cred_source not in self._KNOWN_CRED_SOURCES:
        context.abort(
            grpc.StatusCode.INVALID_ARGUMENT, "INVALID_CREDENTIALS_SOURCE"
        )
    # configured factory が想定する venue と request の venue が一致するか確認。
    # Phase 8.7 は backend 起動時 --live-venue で factory 固定の現行仕様を維持し、
    # mismatch は VENUE_MISMATCH で reject（複数 factory 切替は Phase 8.8）。
    if self._live_adapter_factory is None:
        return engine_pb2.VenueLoginResponse(
            success=False, error_code="LIVE_ADAPTER_NOT_CONFIGURED",
            venue_state=self.venue_sm.current if self.venue_sm else "DISCONNECTED",
            instruments_loaded=0,
        )
    configured_venue = getattr(self, "_live_venue_id", venue_id).upper()
    if configured_venue != venue_id:
        return engine_pb2.VenueLoginResponse(
            success=False, error_code="VENUE_MISMATCH",
            venue_state=self.venue_sm.current if self.venue_sm else "DISCONNECTED",
            instruments_loaded=0,
        )

    # 冪等: 既に CONNECTED/SUBSCRIBED なら no-op success
    if self.venue_sm is not None and self.venue_sm.current in ("CONNECTED", "SUBSCRIBED"):
        return engine_pb2.VenueLoginResponse(
            success=True, error_code="",
            venue_state=self.venue_sm.current, instruments_loaded=0,
        )
    try:
        # (1) live components bootstrap（runner / bridge / cache）— 既に起動済みなら no-op
        self._start_live_components()
        runner = self._live_runner
        adapter = runner.adapter  # D10
        # (2) adapter login（D14）: request.credentials_source / environment_hint を透過。
        # Phase 8.7 では mock adapter のみ実 login (real adapter は is_logged_in=True を
        # 返す stub のまま Phase 8.8 まで no-op)。pydantic v2 VenueCredentials は
        # credentials_source 必須、environment_hint optional.
        if not getattr(adapter, "is_logged_in", True):
            from engine.live.adapter import VenueCredentials
            loop = self._ensure_live_loop()
            creds = VenueCredentials(
                credentials_source=cred_source,
                environment_hint=(request.environment_hint or None),
            )
            fut = asyncio.run_coroutine_threadsafe(adapter.login(creds), loop)
            fut.result(timeout=self._live_timeout_s)
        # (3) venue_sm 遷移（D18）: SetExecutionMode の precondition を満たす
        if self.venue_sm is not None and self.venue_sm.current == "DISCONNECTED":
            self.venue_sm.transition_to("AUTHENTICATING")
            self.venue_sm.transition_to("CONNECTED")
    except Exception as exc:
        logging.exception("VenueLogin failed: %s", exc)
        return engine_pb2.VenueLoginResponse(
            success=False, error_code="VENUE_LOGIN_FAILED",
            venue_state=self.venue_sm.current if self.venue_sm else "DISCONNECTED",
            instruments_loaded=0,
        )
    return engine_pb2.VenueLoginResponse(
        success=True, error_code="",
        venue_state=self.venue_sm.current, instruments_loaded=0,
    )
```

**D26 補足 (factory / KNOWN_VENUES 拡張)**:
- `python/engine/live/live_adapter_factory.py::build_live_adapter_factory` に MOCK 分岐を追加:
  ```python
  if venue == "MOCK":
      from engine.live.mock_adapter import MockVenueAdapter
      return lambda: MockVenueAdapter()
  ```
- `GrpcDataEngineServer._KNOWN_VENUES = {"TACHIBANA", "KABU", "MOCK"}` に拡張（[server_grpc.py:158](../../python/engine/server_grpc.py#L158)）
- backend 起動は `python -m engine --live-venue MOCK` で MOCK factory を選択。real venue (tachibana/kabu) で起動した backend に対し menu_bar から `venue_id="mock"` を送ると **`VENUE_MISMATCH`** で reject される（既存の 1 backend = 1 factory 制約は維持）。Phase 8.7 で複数 factory 同時起動はしない
- menu_bar に新項目 `MenuItem::VenueConnectMock`（"Venue → Connect → Mock"）を追加し `send_venue_login(&sender, "mock", "demo")` を呼ぶ。`environment_hint="demo"` は MockVenueAdapter が無視するため任意

**venue_id / credentials_source の不変条件 (v4 追加)**:
- UI → backend の `venue_id` は **lowercase or uppercase どちらも受け付ける**。backend 側で `.upper()` 正規化必須
- `credentials_source` は request 由来。UI 側 `menu_bar.rs` の現行 hard-coded `"prompt"` を残しつつ、mock adapter が `"prompt"` でも動くことを `MockVenueAdapter.login` 側で担保（環境変数 fallback で credentials が無くても成功する既存挙動）
- `environment_hint` は `VenueCredentials.environment_hint` に透過するだけ。Phase 8.7 では adapter 内部で参照されない (mock のみ login 実行のため) が、Phase 8.8 で real adapter が "demo"/"prod"/"verify" 切替に使う準備として配線しておく
- `_live_venue_id` field は `GrpcDataEngineServer.__init__` で `--live-venue` から渡される値を `.upper()` して保持する（追加実装）。configured venue と request venue が一致しない場合は **VENUE_MISMATCH で reject**（現状 1 backend インスタンス = 1 venue factory の制約を明示）

そして `SetExecutionMode` から `_start_live_components()` の事前呼び出しは削除する（[server_grpc.py:851-866](../../python/engine/server_grpc.py#L851)）:

```python
# 削除前:
#   if applied in ("LiveManual", "LiveAuto"):
#       try:
#           self._start_live_components()
#       except ...
# 削除後: precondition (venue_sm CONNECTED) は VenueLogin で既に満たされている前提。
# v5.2 修正 (Claim 2): live components が teardown 後に再起動が必要なケースで
# "unlogged adapter を起動してしまう" race を防ぐため、冪等再起動は撤廃し
# VENUE_LOGIN_REQUIRED で reject する。venue_sm が DISCONNECTED にリセットされる
# (下記 _teardown_live_components 修正) ので、UI 側 footer gate も自然と閉じる。
if applied in ("LiveManual", "LiveAuto") and self._live_runner is None:
    return engine_pb2.SetExecutionModeResponse(
        success=False,
        error_code="VENUE_LOGIN_REQUIRED",
        execution_mode="",
    )
```

**さらに `_teardown_live_components` で `venue_sm` を DISCONNECTED にリセット**（v5.2 Claim 2 対応）。現状 [server_grpc.py:867](../../python/engine/server_grpc.py#L867) の Replay 戻りで `_teardown_live_components()` は呼ばれるが `venue_sm.current` は CONNECTED のまま残るため、Live → Replay → Live 再入場時に (a) `VenueLogin` ハンドラ冒頭の冪等 early return ([§2.2.1 line 318-323 相当](../../python/engine/server_grpc.py)) を踏んで adapter login が **skip され**、(b) `SetExecutionMode` 側の冪等再起動が unlogged adapter を立ち上げる、という経路が成立する:

```python
def _teardown_live_components(self) -> None:
    # 既存: runner.stop() / bridge.close() / cache 解放
    ...
    # v5.2 追加: venue_sm を DISCONNECTED に戻し、次の Live 入場で
    # VenueLogin ハンドラが必ず adapter.login を再実行するようにする。
    # adapter 自体も factory から作り直すため _live_adapter は破棄
    # (LiveRunner.adapter プロパティ経由でアクセスする現行モデルなら自動)。
    if self.venue_sm is not None and self.venue_sm.current != "DISCONNECTED":
        self.venue_sm.transition_to("DISCONNECTED")
```

これにより不変条件「`venue_sm.current == CONNECTED` ⇒ `_live_runner is not None` かつ `adapter.is_logged_in == True`」が teardown を跨いでも成立する。UI 側 footer gate ([footer.rs:652](../../src/ui/footer.rs#L652)) は `VenueState::Connected/Subscribed` 以外で `SetExecutionMode` 送信を握り潰すため、Live → Replay → Live の 2 回目入場時にも自然と `Venue → Connect` 手順を踏ませる動線になる。

**呼び出しフロー（UI → backend、Live 入場）**:
1. UI: ユーザーが **menu_bar の `Venue → Connect → <Venue> <Env>` を選択** ([menu_bar.rs:425-435](../../src/ui/menu_bar.rs#L425)) → 既存 `send_venue_login(&sender, venue_id, environment_hint)` 経由で `TransportCommand::VenueLogin { venue_id, credentials_source, environment_hint }` を送信（**Phase 8.7 では既存 menu_bar 経路を正規ルートに採用**。footer に新ボタンは追加しない）
2. Backend: `VenueLogin` ハンドラが venue_id 正規化 → live components 起動 → adapter.login → `venue_sm CONNECTED`
3. UI: 次回 `GetState` polling で `VenueStatusRes::Connected` を受け取り、`footer.rs:652` の `SetExecutionMode` UI gate を通過させる
4. UI: ユーザーが Live toggle 押下 → `TransportCommand::SetExecutionMode { mode: LiveManual }` 送信
5. Backend: `mode_manager.set_execution_mode("LiveManual")` 既に CONNECTED なので precondition pass

**Mock adapter 自動 login の扱い (v4 修正)**:
- 当初案の `auto_venue_login_on_startup_system` は **Phase 8.7 では実装しない**。「mock adapter かどうか」を UI 側で判定する経路が無く (backend 起動 flag が UI に未伝播)、誤発火で real adapter 環境を壊すリスクが高い
- mock adapter で開発する場合も明示的に menu_bar → `Venue → Connect` を 1 回押す運用とする。DoD #11 の手動 QA 手順に「Venue Connect 操作」を必須ステップとして書く
- 自動化は Phase 8.8 で「backend が `GetState.live_adapter_kind = "mock"` を返す → UI が startup hook で `VenueLogin` 送信」の経路に分離して実装する（チケット化）

**D18 補足**:
- `venue_sm` は `server` 自身の field ([server_grpc.py:149](../../python/engine/server_grpc.py#L149))
- 遷移後の `venue_sm.current` は次回 `GetState` の `venue_state` field 経由で Rust `VenueStatusRes` に伝播（既存経路）。`auto_fetch_live_universe_on_connect_system` も同 GetState tick で発火条件を満たす
- 冪等ガード: 既に `CONNECTED/SUBSCRIBED` ならハンドラ冒頭で early return するため `transition_to` 不正遷移例外は起きない

**Rust side（D21 transport variant）**:
- `TransportCommand::VenueLogin { venue_id, credentials_source, environment_hint }` は **既存 enum variant** ([trading.rs](../../src/trading.rs))。Phase 8.7 では variant 追加なし、送信経路だけ menu_bar 既存ボタン ([menu_bar.rs:425-435](../../src/ui/menu_bar.rs#L425)) を使う
- response の `success=true` を受けたら UI は何もしない（venue_sm 状態は GetState polling 経由で `VenueStatusRes` に伝播）。エラー時のみ menu_bar 側で error log 出力
- `auto_venue_login_on_startup_system` は **Phase 8.7 スコープから除外**（上記参照、Phase 8.8 へ送り）

**注意**:
- `id` 形式: `f"{code}.{market}"`（Rust `InstrumentId` 既存規約と一致）。tachibana / kabu adapter の `InstrumentRaw.market` を要確認。不揃いなら adapter 側で投影
- `engine.proto` の `VenueLoginRequest` / `VenueLoginResponse` メッセージは既存（OUT-3 と矛盾しない）。フィールド追加が必要な場合は確認のうえ最小限に
- Phase 8.8 で `VenueLogin` 本実装が入ったら、D14 の `credentials_source="env"` 固定と D18 の手動 transition を「real credentials prompt + adapter 側 venue_sm 管理」に置き換える

### 2.3 Replay engine の multi-instrument 化（D9）

#### 2.3.1 `reducer.py` 改修

```python
@dataclass(frozen=True)
class KlineUpdate:
    timestamp_ms: int
    close: float
    open: float = 0.0
    high: float = 0.0
    low: float = 0.0
    open_time_ms: int = 0
    instrument_id: str = ""  # NEW（既存呼び出しは "" のまま動く）

@dataclass(frozen=True)
class TradeUpdate:
    timestamp_ms: int
    price: float
    instrument_id: str = ""  # NEW

@dataclass
class ReducerState:
    timestamp_ms: int
    price: float
    open: float = 0.0
    high: float = 0.0
    low: float = 0.0
    open_time_ms: int = 0
    history: list = field(default_factory=list)
    history_points: list = field(default_factory=list)
    ohlc_points: list = field(default_factory=list)
    max_history_len: int = 1000
    per_id_close: dict[str, float] = field(default_factory=dict)  # NEW
```

`apply_event` の `KlineUpdate` / `TradeUpdate` 分岐で `if event.instrument_id: state.per_id_close[event.instrument_id] = price` を 1 行追加。`history` / `ohlc_points` の累積は **primary（= 後方互換で `instrument_id == ""` または `instrument_ids[0]`）の時のみ** にして、UI Chart 1 系列の既存表示を壊さない。

#### 2.3.2 `core.py` 改修

```python
def load_replay_data(self, instrument_ids, start_date, end_date, granularity, catalog_path=None):
    ...
    # D17: instrument_ids は instrument id ("1301.TSE" 形式)。
    # NautilusBarsReplayProvider が要求するのは BarType 文字列なので変換する。
    providers: dict[str, NautilusBarsReplayProvider] = {}
    for i, iid in enumerate(instrument_ids):
        bar_type = instrument_id_to_bar_type(iid, granularity)  # D17 helper
        try:
            providers[iid] = NautilusBarsReplayProvider(
                catalog_path=effective_catalog_path,
                bar_type=bar_type,
                start=start_date or None,
                end=end_date or None,
            )
        except (ValueError, FileNotFoundError) as e:
            return False, f"{iid}: {e}"
    self._replay_providers = providers
    self._replay_primary_id = instrument_ids[0]
    self._prime_providers_locked()
    ...

def _advance_one_locked(self):
    if self._replay_providers:
        # 全 provider から peek し、最古 ts の **全 provider** を 1 tick で進める。
        # v5 修正 (D24): 同 ts (日足の同一 close / 同分足の同時刻) に複数銘柄が
        # ある場合、1 つだけ pop すると残りは次 tick 持ち越しで「同時」表示が
        # 厳密に壊れる。DoD #6「複数 Chart の price が同時に sidebar に表示」を
        # 満たすため、min_ts と等しい ts を持つ provider をまとめて drain する。
        pending = []
        for iid, p in self._replay_providers.items():
            tick = p.peek_next_tick()  # NEW: get_next_tick を peek/pop に分ける
            if tick is not None:
                pending.append((tick[0], iid, p))
        if not pending:
            self._is_exhausted = True
            return
        pending.sort(key=lambda x: x[0])
        min_ts = pending[0][0]
        ts_ms = int(min_ts * 1000)
        # ReplayTimeUpdated は 1 回だけ（同 ts のグループに対して）
        self._apply_event_locked(ReplayTimeUpdated(timestamp_ms=ts_ms))
        # 同 ts の全 provider を pop して KlineUpdate を発行
        for ts, iid, p in pending:
            if ts != min_ts:
                break  # sort 済みなので min_ts より大きい ts はここで打切り
            popped = p.pop_next_tick()  # returns (ts, o, h, l, c)
            _, o, h, l, c = popped
            self._apply_event_locked(KlineUpdate(
                timestamp_ms=ts_ms, close=c, open=o, high=h, low=l,
                open_time_ms=ts_ms, instrument_id=iid,
            ))
        self._is_exhausted = all(p.is_exhausted() for p in self._replay_providers.values())
    else:
        # legacy random walk path（既存）
        ...

def get_replay_last_prices(self) -> dict[str, float]:
    with self._lock:
        return dict(self._rs.per_id_close)

def reset_replay_state(self):
    # 既存 reset 経路（load 解除/再ロード時）の最後に追加:
    self._rs.per_id_close.clear()
```

`NautilusBarsReplayProvider` に `peek_next_tick` / `pop_next_tick` を追加（既存 `get_next_tick` は `pop` の alias として残す）。

**後方互換**:
- 既存単一銘柄テスト（`_replay_provider` 直設定）も並走サポート: `_replay_provider is not None` 時は legacy 経路、`_replay_providers` dict ありなら新経路
- `_rs.price` / `_rs.history` は primary id の更新時のみ動く（UI Chart 既存挙動の維持）

### 2.4 `GetState` の `last_prices` を mode-aware に（D8）

[python/engine/server_grpc.py:271-279](../../python/engine/server_grpc.py#L271):

```python
mode = self.mode_manager.current_mode if self.mode_manager else "Replay"
if mode in ("LiveManual", "LiveAuto"):
    last_prices = (
        self._live_price_cache.snapshot()
        if self._live_price_cache is not None
        else {}
    )
else:  # Replay
    last_prices = self.engine.get_replay_last_prices()

state = self.engine.get_current_state()
state = state.model_copy(
    update={"live_last_error": live_last_error, "last_prices": last_prices}
)
```

### 2.5 `scenario.py` の `instruments_ref` 復活 + resolver（D7）

**責務分離 (v5 明記、D25)**: 同じ `instruments_ref` 解決ロジックが Python と Rust の両方に存在することになるが、それぞれ呼び出し経路と目的が異なる:

| 実装場所 | 呼び出し経路 | 用途 |
|---|---|---|
| `python/engine/strategy_runtime/scenario.py::resolve_instruments_ref` | `load_scenario` 内 (backend) | strategy_runtime での scenario validate / E2E backend テスト |
| `src/ui/scenario_parser.rs::resolve_instruments_ref` | `parse_scenario_system` 内 (UI) | sidebar restore / Q1 fixed scenario 復元時の sidecar 読込 |

**不変条件**:
- 両者は **同じファイル形式仕様** (bare path / `<path>#<json-pointer>` の併用、target root が list[str]) を共有する
- 仕様変更時は両方を同時更新する。Phase 8.7 では bare path + 最小 JSON pointer (`/key`, `/key/0`) のみサポート
- pointer 形式の RFC 6901 拡張等が必要になった場合は、**共通仕様 doc** (`docs/spec/instruments_ref_format.md` を新設) に切り出し、両 implementation がそれを参照する形に格上げする (Phase 8.8 候補)
- backend / UI を別 Phase で改修する際は、**この対称性を破らない** ことを reviewer ガイドラインに明記

#### 2.5.1 validate を「`instruments` OR `instruments_ref`」に二段化

**v4 致命修正**: v3 schema は `instruments` を required にしていた ([scenario.py:194-197](../../python/engine/strategy_runtime/scenario.py#L194))。`instruments_ref` を optional key に足すだけだと「`instruments_ref` only」シナリオが `_check_keys` で `missing required keys: ['instruments']` を投げる。

正しい設計は **「validate を resolve の後 (= `instruments` に展開済みの dict に対して) 呼ぶ」** か **「validate 側を `instruments` OR `instruments_ref` の択一に書き換える」** のどちらか。Phase 8.7 では後者を採用する（順序依存を作らない方が安全）:

```python
# scenario.py
_V3_REQUIRED_BASE: frozenset[str] = frozenset({
    "schema_version", "start", "end", "granularity", "initial_cash",
})
_V3_OPTIONAL: frozenset[str] = frozenset({"strategy_init_kwargs"})

def validate(d: dict) -> None:
    ...
    elif sv == 3:
        # instruments と instruments_ref のいずれか一方は必須（両方可、両方ある場合は ref 優先）
        has_inline = "instruments" in d
        has_ref = "instruments_ref" in d
        if not (has_inline or has_ref):
            raise ScenarioValidationError(
                "SCENARIO v3 requires either 'instruments' or 'instruments_ref'"
            )
        allowed_extra = _V3_OPTIONAL | frozenset(
            (["instruments"] if has_inline else [])
            + (["instruments_ref"] if has_ref else [])
        )
        _check_keys(d, _V3_REQUIRED_BASE, allowed_extra)
        _check_types(d, {k: v for k, v in _V3_TYPES.items() if k not in ("instruments",)})
        if has_inline:
            _check_str_list(d, "instruments")
        if has_ref and not isinstance(d["instruments_ref"], str):
            raise ScenarioValidationError(
                "SCENARIO['instruments_ref'] must be str"
            )
```

`test_validate_v3_rejects_instruments_ref` を **`test_validate_v3_accepts_instruments_ref` に書き換え**。さらに以下を追加:
- `test_validate_v3_accepts_instruments_ref_only`（ref のみで `instruments` 不在でも pass）
- `test_validate_v3_accepts_both_instruments_and_ref`（併用可）
- `test_validate_v3_rejects_when_neither_instruments_nor_ref`（両方無いと reject）
- `test_validate_v3_rejects_non_string_instruments_ref`

#### 2.5.2 resolver

`scenario.py` に `resolve_instruments_ref(scenario: dict, sidecar_path: Path) -> list[str]` を新設。

**v4 修正: bare path 形式を正規対応** — 既存 fixture ([e2e_instruments_ref_locked.json](../../examples/e2e_instruments_ref_locked.json), [e2e_instruments_ref_mixed_locked.json](../../examples/e2e_instruments_ref_mixed_locked.json)) は `"refs/universe_topix_core.json"` のような **JSON pointer 無しの bare path**。Phase 8.7 では既存形式を壊さず、JSON pointer は optional 拡張として後置:

- 値の形式: `"<relative-path>.json"` または `"<relative-path>.json#/<json-pointer>"`
- pointer が無い場合は **target JSON の root が list[str]** であることを要求
- pointer がある場合は RFC 6901 の最小実装（`/key`, `/key/0` 程度。複雑ケースは Phase 8.8）
- `sidecar_path.parent / relative_path` を読み、結果を `scenario["instruments"]` に展開（**両方存在する場合は `instruments_ref` 優先で上書き**）
- 失敗時 (target 不存在 / 形式不正 / 空 list) は `ScenarioValidationError` を raise。silent fallback はしない（fixed scenario の不変条件を守るため）

呼び出し順序:
1. `load_scenario` で JSON parse
2. `normalize_scenario` で legacy key 正規化
3. **`resolve_instruments_ref` で `instruments` を埋める**（ref があれば）
4. `validate` で最終チェック（このとき `instruments` は inline / resolved どちらでも埋まっている）

これにより `validate` 側の択一ロジックは「ref を resolve できなかった呼び出し経路（純粋 dict validation 用 API）」のためのバックストップになる。

#### 2.5.2.1 既存 fixture 互換性確認

- `e2e_instruments_ref_locked.json`: bare path、root が list[str]。**ただし target file `examples/refs/universe_topix_core.json` が repo に存在しない**（v5.2 レビュー Claim 1）。Phase 8.7 で resolver を fail-closed 化すると、現状の fixture は load 時に `ScenarioValidationError` を確実に raise する。**修正アクション**: `examples/refs/` ディレクトリを新設し `universe_topix_core.json`（中身: `["1301.TSE", "7203.TSE"]` 程度の list[str]）を commit する。または fixture 側を inline `instruments` に倒して `instruments_ref` 経路は新規 fixture でのみ検証する
- `e2e_instruments_ref_mixed_locked.json`: 併用、ref 優先で `instruments` を上書きする挙動を `test_load_scenario_ref_overrides_inline_instruments` で pin。**こちらも同じ target file に依存するため、上の `refs/universe_topix_core.json` を commit すれば両方が動く**

新規 fixture 移行は **不要**（target file の追加だけで済む）。pointer 形式は新規シナリオでのみ使う想定。

#### 2.5.3 Rust `scenario_parser.rs` を resolver 実装に格上げ（D7）

[src/ui/scenario_parser.rs:110-157](../../src/ui/scenario_parser.rs#L110):

```rust
// 既存:
// let has_instruments_ref = ... boolean だけ
// → 削除し、以下に置換:

let instruments_ref: Option<String> = serde_json::from_str::<serde_json::Value>(&text)
    .ok()
    .and_then(|v| {
        v.get("scenario")
            .and_then(|s| s.get("instruments_ref"))
            .and_then(|r| r.as_str().map(|s| s.to_string()))
    });

let instruments: Vec<String> = if let Some(ref_spec) = instruments_ref.as_deref() {
    // <path>#<json-pointer> を分解、sibling file を読み、ポインタで配列を取り出す
    resolve_instruments_ref(ref_spec, &json_path)?
} else if let Some(list) = sf.instruments {
    list
} else if let Some(sol) = sf.instrument {
    match sol {
        StringOrList::One(s) => vec![s],
        StringOrList::Many(v) => v,
    }
} else {
    vec![]
};

// ScenarioLoadedFromFile の has_instruments_ref → ref_path: Option<String> に rename
loaded_events.send(ScenarioLoadedFromFile {
    source_path: json_path,
    instruments,
    end: sf.end,
    ref_path: instruments_ref,  // None = inline、Some = resolved
});
```

`resolve_instruments_ref` 関数を `scenario_parser.rs` に追加。**失敗時は Python 側と同じく fail-closed**（D28）: error log を出した上で `ScenarioLoadedFromFile` を **送出せず** 早期 return する。Rust 側 registry は維持され、UI Chart が破壊的に消える事故を防ぐ（v5.2 レビュー Claim 1 対応。当初案の「error log + 空 vec で進める」だと sidecar の `instruments_ref` 破損だけで registry が空になり Chart が一斉に消える）。inline `instruments` のみ存在する既存 scenario は影響なし（`instruments_ref` 自体が None なので resolver 経路に入らない）。

### 2.6 `StartEngine` 後の Replay bar injection を全 instrument 化（D16）

[python/engine/server_grpc.py:443-450](../../python/engine/server_grpc.py#L443) を以下に置換:

```python
# Expose ALL bars (all instruments) to GetState so the chart can draw
# multiple candles. bars[0] of each instrument was already primed by
# _prime_providers_locked; inject bars[1:] for every instrument.
from .nautilus_adapter import bar_to_kline_update
for iid, bars in bars_by_instrument.items():
    if not bars:
        continue
    for bar in bars[1:]:
        self.engine.apply_replay_event(
            bar_to_kline_update(bar, instrument_id=iid)  # D9 + D16
        )
```

`bar_to_kline_update` の署名を `bar_to_kline_update(bar, instrument_id: str = "") -> KlineUpdate` に拡張し、`KlineUpdate(..., instrument_id=instrument_id)` を返すよう改修。

**不変条件**:
- `break` を外したことで `bars_by_instrument` が 1 銘柄しか持たない既存 single-instrument scenario でも挙動は変わらない（ループ 1 周で終わる）
- D9 で `_rs.history` / `_rs.ohlc_points` の累積は primary id のみのため、UI Chart 1 系列の既存挙動は維持される

### 2.7 `LoadReplayData.instrument_ids` の契約統一（D17）

`server_grpc.LoadReplayData` ハンドラ:

```python
def LoadReplayData(self, request, context):
    ...
    granularity = self._granularity_name(request.granularity) or "Minute"
    # D17: instrument_ids は instrument id ("1301.TSE")。BarType 文字列ではない。
    # load_replay_data 内部で instrument_id_to_bar_type で変換する。
    success, message = self.engine.load_replay_data(
        instrument_ids=list(request.instrument_ids),
        start_date=request.start_date,
        end_date=request.end_date,
        granularity=granularity,
        catalog_path=request.catalog_path or None,
    )
    ...
```

`engine/core.py` に helper:

```python
def instrument_id_to_bar_type(instrument_id: str, granularity: str) -> str:
    # granularity: "Minute" | "Daily" | "Tick" 等
    gran_map = {"Minute": "1-MINUTE-LAST-EXTERNAL", "Daily": "1-DAY-LAST-EXTERNAL"}
    spec = gran_map.get(granularity, "1-MINUTE-LAST-EXTERNAL")
    return f"{instrument_id}-{spec}"
```

[python/proto/engine.proto:123-125](../../python/proto/engine.proto#L123) コメント書き換え:

```proto
// instrument_ids are full instrument IDs (e.g. "1301.TSE", "AAPL.NASDAQ").
// Backend converts each to a BarType identifier using the requested granularity
// before querying the Nautilus ParquetDataCatalog.
optional string catalog_path = 11;
```

**既存テストの扱い**:
- `test_nautilus_catalog_route` 等で `NautilusBarsReplayProvider(bar_type="AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL", ...)` を直接 instantiate しているものは **provider 単体テストとして残す**（provider の契約は BarType 文字列のまま）
- `LoadReplayData` RPC を通る E2E / integration テストは `instrument_ids=["1301.TSE"]` の形式に更新
- 変換関数 `instrument_id_to_bar_type` 単体テストを `test_instrument_id_to_bar_type` で追加（granularity 別マッピング）

### 2.8 `UnsubscribeMarketData` RPC ハンドラ + `LastPriceCache.remove`（D20）

#### 2.8.1 `LastPriceCache.remove`

[python/engine/live/last_price_cache.py](../../python/engine/live/last_price_cache.py) に追加:

```python
def remove(self, instrument_id: str) -> None:
    """Unsubscribe 済み id を quote_mid / last_trade / last_kline 全部から削除。

    re-add 時の stale 価格漏れを防ぐ (D20 + D27)。スレッドは _run task
    のみが write するため lock 不要 (server_grpc gRPC thread から
    呼ぶ場合も dict pop の atomicity に依存)。
    """
    self._quote_mid.pop(instrument_id, None)
    self._last_trade.pop(instrument_id, None)
    self._last_kline.pop(instrument_id, None)  # D27
```

**§2.8.1b: `KlineUpdate.close` ingest（D27）** — 同ファイルの `__init__` に `self._last_kline: dict[str, float] = {}` を追加し、`_run` の `KlineUpdate` 分岐を `pass` から `self._last_kline[evt.instrument_id] = evt.close` に変更。`snapshot()` の優先順位を `quote_mid > last_trade > last_kline` に拡張:

```python
def snapshot(self) -> dict[str, float]:
    ids = set(self._quote_mid) | set(self._last_trade) | set(self._last_kline)
    out: dict[str, float] = {}
    for iid in ids:
        if iid in self._quote_mid:
            out[iid] = self._quote_mid[iid]
        elif iid in self._last_trade:
            out[iid] = self._last_trade[iid]
        else:
            out[iid] = self._last_kline[iid]
    return out
```

**理由**: `LiveRunner._run` ([live_runner.py:96-102](../../python/engine/live/live_runner.py#L96)) は `TradesUpdate` を **aggregator に渡して closed bar (`KlineUpdate`) のみ bus.publish** する。生 `TradesUpdate` は bus に出ない。つまり `_last_trade` は **venue が直接 trades を publish する経路を持たない限り永遠に空**。Depth が来ない adapter (trade-only / kline-only venue) では `quote_mid` も空。`KlineUpdate.close` を ignore する旧実装では Live sidebar price 列が黙って死ぬため、ingest 対象に追加する。`KlineUpdate` を pass で無視するテストは「`KlineUpdate.close` を `_last_kline` に取り込み、quote_mid/trade が無い銘柄では snapshot に kline close が出る」を pin するテストに置換する。

#### 2.8.2 `UnsubscribeMarketData` RPC ハンドラ

`server_grpc.py` に新規 RPC ハンドラ (proto `engine.proto` で既存 / 未存在のいずれでも、Phase 8.7 のスコープとして配線する):

```python
def UnsubscribeMarketData(self, request, context):
    if request.token != self.token:
        context.abort(grpc.StatusCode.UNAUTHENTICATED, "Invalid token")
    iid = request.instrument_id
    if not iid:
        context.abort(grpc.StatusCode.INVALID_ARGUMENT, "EMPTY_INSTRUMENT_ID")
    runner = self._live_runner
    if runner is None:
        return engine_pb2.SubscribeResponse(
            success=False, error_code="LIVE_RUNNER_NOT_STARTED"
        )
    try:
        # D20: adapter.unsubscribe を blocking で完了させてから cache 削除
        fut = asyncio.run_coroutine_threadsafe(
            runner.unsubscribe(iid), self._live_loop
        )
        fut.result(timeout=self._live_timeout_s)
    except Exception as exc:
        return engine_pb2.SubscribeResponse(
            success=False, error_code=f"UNSUBSCRIBE_FAILED: {exc}"
        )
    # D20: cache から id を消す。例外時は cache に残しておいても fail-safe filter
    # (2.8.3) で除外されるので、ここでは best-effort。
    if self._live_price_cache is not None:
        self._live_price_cache.remove(iid)
    return engine_pb2.SubscribeResponse(success=True, error_code="")
```

`LiveRunner` に既存 `unsubscribe(instrument_id)` method が無ければ追加（adapter `unsubscribe` 委譲 + 内部 `_subscribed` set からも除去）。

#### 2.8.3 `GetState` Live last_prices の二段ガード（D20 fail-safe）

[python/engine/server_grpc.py](../../python/engine/server_grpc.py) §2.4 の Live 分岐を以下に差し替え:

```python
if mode in ("LiveManual", "LiveAuto"):
    raw = (
        self._live_price_cache.snapshot()
        if self._live_price_cache is not None
        else {}
    )
    # D20 二段ガード: runner.subscribed_ids() で snapshot を filter する。
    # UnsubscribeMarketData ハンドラの cache.remove が何らかの理由で漏れても、
    # subscribed set に居ない id は last_prices に乗せない。
    runner = self._live_runner
    if runner is not None:
        try:
            subscribed = runner.subscribed_ids()
            last_prices = {k: v for k, v in raw.items() if k in subscribed}
        except Exception:
            last_prices = raw  # subscribed_ids 自体が壊れたら fall back
    else:
        last_prices = raw
```

`LiveRunner.subscribed_ids() -> set[str]` は内部 `_subscribed: dict[InstrumentId, set[Channel]]` の keys を copy で返す。

### 2.9 Live universe fetch trigger（既存通り）

backend → Rust への自動通知は **不要**。Rust 側 `VenueState::Connected` 遷移検知 system が `ListInstruments(source="live")` を dispatch する（§4.6.2）。**注意**: 現行 [src/main.rs:782-789](../../src/main.rs#L782) の venue-transition fetch（`source=None`）は **D15 で削除** し、新 system に一本化する。

---

## §3 Wire / Resource Schema 変更

### 3.1 proto

**変更なし**。`Instrument` / `ListInstrumentsRequest.source` は既存。`TradingState.last_prices` は opaque JSON。

### 3.2 Rust `Tickers` resource 拡張

[src/trading.rs:493-505](../../src/trading.rs#L493):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickersSource {
    #[default]
    Unknown,
    LiveVenue,             // venue adapter fetch_instruments 由来
    LocalVenueSnapshot,    // 将来 Phase 用、Phase 8.7 では発火経路なし
    ReplayCatalogFallback, // Replay Parquet catalog 由来。Live prune の根拠に使ってはいけない
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TickersStatus {
    #[default]
    NotFetched,
    InFlight,
    Loaded,
    Failed(String),  // list は直前値維持で stale 表示
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Tickers {
    pub list: Vec<Ticker>,
    pub source: TickersSource,
    pub status: TickersStatus,
}
```

### 3.3 `BackendStatusUpdate` の 3 イベント分割（D6c）

[src/trading.rs:574-582](../../src/trading.rs#L574):

```rust
InstrumentsListStarted { source: TickersSource },
InstrumentsListed { source: TickersSource, instruments: Vec<Ticker> },
InstrumentsListFailed { source: TickersSource, error: String },
```

reducer:

```rust
BackendStatusUpdate::InstrumentsListStarted { source } => {
    tickers.source = source;
    tickers.status = TickersStatus::InFlight;
    // list は維持
}
BackendStatusUpdate::InstrumentsListed { source, instruments } => {
    tickers.source = source;
    tickers.status = TickersStatus::Loaded;
    tickers.list = instruments;
}
BackendStatusUpdate::InstrumentsListFailed { source, error } => {
    tickers.source = source;
    tickers.status = TickersStatus::Failed(error);
    // list は維持（stale 表示）
}
```

[src/main.rs:60-99](../../src/main.rs#L60) `auto_list_instruments_after_startup` 改修:
- 引数 `source: TickersSource` を受ける
- 発火**直前**に `InstrumentsListStarted { source }` を push
- 成功時 `InstrumentsListed`、`!success` / transport error 時 `InstrumentsListFailed`
- transport task に **timeout** を設定（OUT-1 / Phase 8.8 連動）。timeout 超過時は必ず `Failed` を push して `InFlight` を放置しない

### 3.4 `TransportCommand` 拡張

[src/trading.rs:173-220](../../src/trading.rs#L173):

```rust
ListInstruments {
    source: TickersSource,  // String → 型化
},
SubscribeMarketData { instrument_id: String },          // 既存
UnsubscribeMarketData { instrument_id: String },        // NEW（D12）
```

transport task 側 `tickers_source_to_wire(source) -> Option<String>`:
- `Unknown` → `None`
- `ReplayCatalogFallback` → `Some("local")`
- `LocalVenueSnapshot` → `Some("local")`（OUT-2 で将来差し替え）
- `LiveVenue` → `Some("live")`

`UnsubscribeMarketData` は backend 既存 RPC へ直結。

---

## §4 Rust UI 改修

### 4.1 Sidebar — Tickers セクション撤去

[src/ui/sidebar.rs:62-152](../../src/ui/sidebar.rs#L62):

**削除する spawn**: L75-117（`Tickers` セクション全部）
**削除する system**:
- `update_tickers_list_system` (L411)
- `tickers_search_focus_system` (L531)
- `tickers_search_input_system` (L555)
- `update_search_text_system`
- `tickers_scroll_system` (L656)
- `ticker_row_click_system` (L696) — §4.3 に Instruments 行用として移植

**削除 component / resource**: `SidebarTickersList`, `SidebarTickersSearchBox`, `SidebarTickersSearchText`, `SidebarTickersScrollOffset`, `SidebarTickersSearchState`, `SidebarTickerRow`

**保持（rename）**: `SidebarTickerPriceText { instrument_id }` → `SidebarInstrumentPriceText { instrument_id }`

### 4.2 Instruments 行に price 列を追加

[src/ui/sidebar.rs:243-300](../../src/ui/sidebar.rs#L243) の row spawn に price 列を挿入。レイアウト: `[label (flex_grow=1)] [price (70px)] [× button]`。

### 4.3 行 click を Instruments 行に移植

行 root に Button を後付けせず、label 領域専用の透明 Button child を追加:

```rust
row.spawn((
    Button,
    Node { flex_grow: 1.0, overflow: Overflow::clip_x(), ..default() },
    BackgroundColor(Color::NONE),
    SidebarInstrumentRowClick { instrument_id: id.clone() },
))
.with_children(|l| {
    l.spawn((
        Text::new(id.clone()),
        TextFont { font_size: 11.0, ..default() },
        TextColor(ROW_TEXT),
    ));
});
```

`instrument_row_click_system` は `ticker_row_click_system` のコピー。Live モード時は `TransportCommand::SubscribeMarketData` を発火。

#### 4.3.1 Picker add → Live mode 自動 Subscribe（D22）

**重要**: 行 click から Subscribe を発火するだけでは、`[+ Add]` picker で新規追加した銘柄は購読されない（追加後ユーザーが行を改めて click しない限り backend に subscribe が届かない）。Phase 8.7 では Tickers セクション撤去で「追加後にもう 1 度 Tickers 行を click して購読」という旧 UX が消えるため、**registry に追加された id を diff 検出して Live mode のときだけ Subscribe を発火する system** を新設する。

新規 system `subscribe_added_instruments_system`（`unsubscribe_removed_instruments_system` と対）:

```rust
pub fn subscribe_added_instruments_system(
    registry: Res<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    sender: Option<Res<TransportCommandSender>>,
    mut prev_ids: Local<HashSet<String>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur_mode = exec_mode.mode;
    let mode_changed = prev_mode.replace(cur_mode) != Some(cur_mode);
    let current: HashSet<String> = registry.ids.iter().cloned().collect();
    let added: Vec<String> = current.difference(&prev_ids).cloned().collect();
    let prev_for_bulk = prev_ids.clone();  // mode 切替時の bulk 用 snapshot
    *prev_ids = current.clone();
    if !matches!(cur_mode, ExecutionMode::LiveManual | ExecutionMode::LiveAuto) { return; }
    let Some(tx) = sender.as_ref() else { return; };
    // v5.2 Claim 4 対応 (D22-ext): mode 切替直後 frame は通常の diff を捨てるが、
    // Replay → Live 直後に survivor (auto-prune 後も残った既存 registry id) を
    // 1 回だけ bulk subscribe して "Chart は見えるが price 来ない" 半接続 UX を防ぐ。
    // backend SubscribeMarketData は冪等なので二重発火しても害なし。
    // 上限 (BULK_SUBSCRIBE_CAP=50) を設けて kabu の 50 銘柄上限と整合させる。
    if mode_changed {
        const BULK_SUBSCRIBE_CAP: usize = 50;
        let survivors: Vec<String> = prev_for_bulk
            .intersection(&current)
            .take(BULK_SUBSCRIBE_CAP)
            .cloned()
            .collect();
        for id in survivors {
            let _ = tx.tx.send(TransportCommand::SubscribeMarketData { instrument_id: id });
        }
        return;
    }
    if added.is_empty() { return; }
    for id in added {
        let _ = tx.tx.send(TransportCommand::SubscribeMarketData { instrument_id: id });
    }
}
```

**不変条件**:
- picker add / restore による registry 追加で発火
- Replay mode では発火しない（backend Replay は subscription concept 無し）
- Replay → Live 切替直後の 1 frame で survivor ids を上限 50 銘柄まで bulk subscribe（**v5.2 Claim 4 / D22-ext を Phase 8.7 で取り込み**）。当初案の「click し直す運用」は survivor Chart が「表示はされているが price 来ない・order 不可」の半接続状態を作るため、ユーザー視点で危険と判断し Phase 8.7 内で解消する。`§5.3` 順序で auto-prune → unsubscribe が先に走った後の survivor のみが対象なので、universe 外の id を誤って subscribe することは無い
- 行 click 経路 (§4.3) と重複発火するが、backend `SubscribeMarketData` ハンドラは冪等（既存実装）
- 上限 50 銘柄を超えた survivor は subscribe されない（kabu の 50 銘柄上限と整合）。超過分は警告 log を残し、ユーザーが手動で click すれば従来通り subscribe される

### 4.4 price 列の mode 分岐撤去（D3 確立後）

[src/ui/sidebar.rs:628-652](../../src/ui/sidebar.rs#L628) を `update_instrument_price_text_system` に rename:

```rust
pub fn update_instrument_price_text_system(
    last_prices: Res<LastPrices>,
    mut q: Query<(&SidebarInstrumentPriceText, &mut Text)>,
) {
    if !last_prices.is_changed() { return; }
    for (marker, mut text) in &mut q {
        let s = format_price(last_prices.map.get(&marker.instrument_id).copied());
        if text.0 != s { text.0 = s; }
    }
}
```

`ExecutionModeRes` / `TradingData` / `SelectedSymbol` 依存を削除（D3 + D8 + D9 backend 改修により両モード対称になる）。

### 4.5 `[+ Add]` dropdown のモード分岐（Q4）

[src/ui/instrument_picker.rs:435-534](../../src/ui/instrument_picker.rs#L435) を以下に書き換え:

```rust
let ids: Vec<String> = match exec_mode.mode {
    ExecutionMode::Replay => {
        let Some(end) = picker.end_date else {
            spawn_placeholder(container, "Set scenario.end first"); return;
        };
        if let Some((d, msg)) = &available.last_error {
            if *d == end { spawn_placeholder(container, &format!("Error: {msg}")); return; }
        }
        if available.in_flight.contains(&end) {
            spawn_placeholder(container, "Loading..."); return;
        }
        match available.by_end_date.get(&end) {
            None => { spawn_placeholder(container, "Loading..."); return; }
            Some(v) => v.clone(),
        }
    }
    ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
        match &tickers.status {
            TickersStatus::NotFetched => {
                spawn_placeholder(container, "Venue not connected"); return;
            }
            TickersStatus::InFlight => {
                spawn_placeholder(container, "Loading..."); return;
            }
            TickersStatus::Failed(msg) => {
                spawn_placeholder(container, &format!("Error: {msg}")); return;
            }
            TickersStatus::Loaded => {
                tickers.list.iter().map(|t| t.id.clone()).collect()
            }
        }
    }
};
```

### 4.6 `[+ Add]` の fetch trigger を mode 分岐

[src/ui/instrument_picker.rs:160](../../src/ui/instrument_picker.rs#L160) `add_instrument_button_system`:

```rust
match exec_mode.mode {
    ExecutionMode::Replay => {
        picker.end_date = parse_scenario_end(&scenario_meta);
        // 既存 AvailableInstruments fetch — ただし D11 system が事前に投げているので
        // ここでは not-in-flight / not-cached のみ補完 dispatch
        dispatch_fetch_available_instruments(...);
    }
    ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
        picker.end_date = None;
        if matches!(tickers.status, TickersStatus::NotFetched | TickersStatus::Failed(_)) {
            if let Some(tx) = sender.as_ref() {
                let _ = tx.tx.send(TransportCommand::ListInstruments {
                    source: TickersSource::LiveVenue,
                });
            }
        }
    }
}
```

#### 4.6.1 `VenueState::Connected` 遷移 or `ExecutionMode → Live` 遷移で live universe を auto-fetch（Q3 + D23）

**v5 修正 (D23)**: 旧設計は `VenueState` 遷移だけを trigger にしていたため、Replay 中に Venue Connect 済み → 後で Live に切替えるケースで fetch が発火しない（VenueState はもう変化済み、ExecutionMode 切替を見ていない）。**ExecutionMode が Live\* に新規遷移したとき** も「VenueState がすでに Connected/Subscribed なら fetch を投げる」second trigger を追加する。Tickers.status が `Loaded` ならそのまま使えるので skip し、`NotFetched/Failed` のときだけ fetch する。

新規 system `auto_fetch_live_universe_on_connect_system`:

```rust
pub fn auto_fetch_live_universe_on_connect_system(
    venue: Res<VenueStatusRes>,
    exec_mode: Res<ExecutionModeRes>,
    tickers: Res<Tickers>,
    sender: Option<Res<TransportCommandSender>>,
    mut prev_state: Local<Option<VenueState>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur_venue = venue.state;
    let was_venue = prev_state.replace(cur_venue);
    let cur_mode = exec_mode.mode;
    let was_mode = prev_mode.replace(cur_mode);

    let became_connected = matches!(cur_venue, VenueState::Connected | VenueState::Subscribed)
        && !matches!(was_venue, Some(VenueState::Connected) | Some(VenueState::Subscribed));
    let became_live = matches!(cur_mode, ExecutionMode::LiveManual | ExecutionMode::LiveAuto)
        && !matches!(was_mode, Some(ExecutionMode::LiveManual) | Some(ExecutionMode::LiveAuto));

    // Trigger 1: VenueState が live になった瞬間（Live mode 前提）
    let trigger_by_venue = became_connected
        && matches!(cur_mode, ExecutionMode::LiveManual | ExecutionMode::LiveAuto);
    // Trigger 2: ExecutionMode が Live になった瞬間（VenueState すでに live 前提）
    let trigger_by_mode = became_live
        && matches!(cur_venue, VenueState::Connected | VenueState::Subscribed);

    if !(trigger_by_venue || trigger_by_mode) { return; }
    // Tickers がすでに Loaded ならスキップ（重複 fetch 防止）。Failed/InFlight/NotFetched
    // のときだけ再 fetch する。InFlight でも投げてしまうと race するので skip。
    if matches!(tickers.status, TickersStatus::Loaded | TickersStatus::InFlight) { return; }
    if let Some(tx) = sender.as_ref() {
        let _ = tx.tx.send(TransportCommand::ListInstruments {
            source: TickersSource::LiveVenue,
        });
    }
}
```

#### 4.6.2 Replay 入場 / `scenario.end` 変更で AvailableInstruments を auto-fetch（D11）

新規 system `auto_fetch_available_on_replay_entry_system`:

```rust
pub fn auto_fetch_available_on_replay_entry_system(
    exec_mode: Res<ExecutionModeRes>,
    scenario: Res<ScenarioMetadata>,
    sender: Option<Res<TransportCommandSender>>,
    mut available: ResMut<AvailableInstruments>,
    backend_status: Option<Res<BackendStatus>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
    mut prev_end: Local<Option<String>>,
) {
    let cur_mode = exec_mode.mode;
    let cur_end = scenario.end.clone();
    let mode_entered_replay = prev_mode.replace(cur_mode) != Some(ExecutionMode::Replay)
        && cur_mode == ExecutionMode::Replay;
    let end_changed = prev_end.as_ref() != Some(&cur_end);
    if end_changed { *prev_end = cur_end.clone(); }
    if !mode_entered_replay && !end_changed { return; }
    if cur_mode != ExecutionMode::Replay { return; }
    let Some(end) = parse_scenario_end(&scenario) else { return; };
    if available.by_end_date.contains_key(&end) || available.in_flight.contains(&end) { return; }
    if backend_status.as_ref().map(|s| !s.connected).unwrap_or(true) {
        available.last_error = Some((end, "backend not connected".to_string()));
        return;
    }
    let Some(tx) = sender.as_ref() else { return; };
    available.in_flight.insert(end);
    available.last_error = None;
    let _ = tx.tx.send(TransportCommand::FetchAvailableInstruments { end_date: end });
}
```

#### 4.6.3 Startup 時の fetch を Replay 経路に固定 + 既存 venue-transition fetch 削除（D15）

[src/main.rs:60-99](../../src/main.rs#L60) `auto_list_instruments_after_startup` は **`source = TickersSource::ReplayCatalogFallback`** の初期 1 回のみ。Live universe は `auto_fetch_live_universe_on_connect_system` 経由。D6b により ReplayCatalogFallback では Live prune が走らない不変条件が成立。

**D15: [src/main.rs:782-789](../../src/main.rs#L782) の削除**:

```rust
// 削除する block:
if matches!(vs, VenueState::Connected | VenueState::Subscribed) {
    fire_list_instruments(&client, &token, None, &status_tx);
}
```

これを残したまま `auto_fetch_live_universe_on_connect_system` を足すと、同 trigger で 2 本 fetch（`source=None` = local、`source=Some("live")` = LiveVenue）が走り、後着の結果が `Tickers` を上書きして D6b の `source ∈ {LiveVenue, LocalVenueSnapshot}` 判定が race で崩れる。`fire_list_instruments` 関数自体は startup 経路（[main.rs:372](../../src/main.rs#L372)）で使うので**関数定義は残す**。

---

## §5 auto-prune system + Live unsubscribe

### 5.1 新ファイル `src/ui/instruments_universe_prune.rs`（D2 / D6b / D13）

```rust
use std::collections::HashSet;
use bevy::prelude::*;
use crate::trading::{
    AvailableInstruments, ExecutionMode, ExecutionModeRes,
    Tickers, TickersSource, TickersStatus,   // ← TickersSource 追加（D13）
    VenueState, VenueStatusRes,              // ← D19 鮮度 gate
};
use crate::ui::components::{InstrumentRegistry, ScenarioMetadata};
use crate::ui::instrument_picker::parse_scenario_end;

pub fn prune_instruments_outside_universe_system(
    mut registry: ResMut<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    tickers: Res<Tickers>,
    available: Res<AvailableInstruments>,
    scenario: Res<ScenarioMetadata>,
    venue: Res<VenueStatusRes>,   // D19
) {
    let trigger = exec_mode.is_changed()
        || tickers.is_changed()
        || available.is_changed()
        || scenario.is_changed()
        || venue.is_changed();   // D19
    if !trigger { return; }
    if registry.ids.is_empty() { return; }

    let allowed: Option<HashSet<String>> = match exec_mode.mode {
        ExecutionMode::Replay => {
            // D13: `?` は使えない（system は () を返す）
            let Some(end) = parse_scenario_end(&scenario) else { return; };
            available.by_end_date.get(&end)
                .map(|v| v.iter().cloned().collect())
        }
        ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
            let status_ok = matches!(tickers.status, TickersStatus::Loaded);
            let source_ok = matches!(
                tickers.source,
                TickersSource::LiveVenue | TickersSource::LocalVenueSnapshot,
            );
            // D19: venue が今この瞬間に配信中であることを必須にする。
            // 直前まで Loaded だった universe でも、disconnect 後は prune の根拠に使わない。
            let venue_ok = matches!(
                venue.state,
                VenueState::Connected | VenueState::Subscribed,
            );
            if status_ok && source_ok && venue_ok {
                Some(tickers.list.iter().map(|t| t.id.clone()).collect())
            } else {
                None
            }
        }
    };
    let Some(allowed) = allowed else { return; };

    let before = registry.ids.clone();
    registry.ids.retain(|id| allowed.contains(id));
    if registry.ids != before {
        info!(
            "auto-prune: {} → {} (mode={:?})",
            before.len(), registry.ids.len(), exec_mode.mode
        );
    }
}
```

### 5.2 Unsubscribe diff system（D12）

新規 system `unsubscribe_removed_instruments_system`:

```rust
pub fn unsubscribe_removed_instruments_system(
    registry: Res<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    sender: Option<Res<TransportCommandSender>>,
    mut prev_ids: Local<HashSet<String>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur_mode = exec_mode.mode;
    let mode_changed = prev_mode.replace(cur_mode) != Some(cur_mode);
    let current: HashSet<String> = registry.ids.iter().cloned().collect();
    let removed: Vec<String> = prev_ids.difference(&current).cloned().collect();
    *prev_ids = current;
    // v5 修正 (リスク表対応): mode 切替直後 frame は diff を skip し prev_ids だけ
    // 更新する。auto-prune が mode 切替と同 tick で大量削除した分が「unsubscribe
    // 大量送信」に化けるのを防ぐ。Live → Replay の場合は下の Live gate でも
    // 防げるが、Live → Live 内の sub-mode 切替（LiveManual ↔ LiveAuto）で false
    // negative にならないよう、mode_changed guard を独立に置く。
    if mode_changed { return; }
    if removed.is_empty() { return; }
    if !matches!(cur_mode, ExecutionMode::LiveManual | ExecutionMode::LiveAuto) { return; }
    let Some(tx) = sender.as_ref() else { return; };
    for id in removed {
        let _ = tx.tx.send(TransportCommand::UnsubscribeMarketData { instrument_id: id });
    }
}
```

**Replay mode では送らない**（backend Replay は subscription concept 無し）。mode 切り替え直後 frame は `mode_changed` ガードで skip し `prev_ids` だけ更新する。Live → Replay の遷移時には Live gate と mode_changed guard の二重で blocked、Replay → Live の遷移時には registry が prune 済み + mode_changed guard でも blocked される。

### 5.2.1 VenueState 遷移で Tickers を invalidate（D19）

新規 system `invalidate_tickers_on_venue_disconnect_system`:

```rust
pub fn invalidate_tickers_on_venue_disconnect_system(
    venue: Res<VenueStatusRes>,
    mut tickers: ResMut<Tickers>,
    mut prev_state: Local<Option<VenueState>>,
) {
    let cur = venue.state;
    let was = prev_state.replace(cur);
    let was_live = matches!(was, Some(VenueState::Connected) | Some(VenueState::Subscribed));
    let is_live = matches!(cur, VenueState::Connected | VenueState::Subscribed);
    if !(was_live && !is_live) { return; }
    // 配信が落ちた瞬間。list は UI 表示用に維持 (picker placeholder 経路は status で出す)
    // し、source/status だけ「今は universe を語れない」状態に戻す。
    tickers.source = TickersSource::Unknown;
    tickers.status = TickersStatus::NotFetched;
}
```

**不変条件**:
- `Connected → Subscribed` / `Subscribed → Connected` の往復では発火しない（両方 live 扱い）
- `Disconnected → Authenticating → Connected` の再接続では `Connected` 復帰時に `auto_fetch_live_universe_on_connect_system` が再 fetch を投げて `Tickers.status = InFlight → Loaded` に戻す
- `tickers.list` は維持されるので、picker dropdown は `NotFetched` placeholder（"Venue not connected"）になり、空 list を見せるよりも UX が良い

### 5.3 system 順序（`UiPlugin::build`）

```text
status_update_system (Tickers / AvailableInstruments / VenueState 更新)
  ↓
invalidate_tickers_on_venue_disconnect_system  (D19) ← VenueState live → !live で source/status リセット
  ↓
auto_fetch_live_universe_on_connect_system     (Q3) ← VenueState !live → live で再 fetch
auto_fetch_available_on_replay_entry_system    (D11)
  ↓
prune_instruments_outside_universe_system      (D2/D6b/D19)
  ↓
instrument_chart_sync_system                   (Chart spawn/despawn)
  ↓
unsubscribe_removed_instruments_system         (D12)
subscribe_added_instruments_system             (D22) ← picker add / restore で増えた id を Subscribe
  ↓
restore_fixed_registry_on_replay_entry_system  (Q1)
  ↓
mark_registry_dirty_system    (mode 不問で revision を bump、§5.4 参照)
writeback_scenario_instruments_system (Replay + editable AND gate)
```

`.chain()` で順序固定。同 tick で「Tickers 更新 → prune → Chart despawn → Unsubscribe」が完結。

### 5.4 writeback gate 強化

**重要（v4 修正）**: `mark_registry_dirty_system` は **mode 不問で revision を bump し続ける**。Replay gate は **`writeback_scenario_instruments_system` と `assert_scenario_metadata_in_sync_system` の 2 system にだけ** 追加する。

理由: `mark_registry_dirty_system` に Replay gate を入れると、Live mode で auto-prune が起きた frame で `is_changed()` が consume されたまま revision bump が抑止される。その後 Replay 再入しても `registry.is_changed()` は false なので writeback が永遠に発火せず、§00 narrative が約束する「Live で減った registry を Replay 再入時に sidecar へ flush」が成立しない（Bevy change detection は frame-local）。

revision を常時 bump しておけば、Live 中は `revision != flushed_revision` のまま writeback が gate で skip され、Replay 再入直後の最初の tick で `writeback_scenario_instruments_system` が走り、現 registry を sidecar に flush して `flushed_revision = revision` で追いつく。`mark_registry_dirty_system` 自体は副作用が `ScenarioInstrumentsWritebackState` の revision counter だけなので、Live で +N されても害はない。

[src/ui/components.rs:699-720](../../src/ui/components.rs#L699) `mark_registry_dirty_system`:

```rust
// 変更なし（既存どおり、ExecutionModeRes は注入しない）。
// editable gate も入れない: editable=false でも Live で manual edit は禁止されているので
// registry mutation 自体が起きない。安全弁として、もし editable=false の registry が
// 何らかの経路で mutate された場合は revision を bump し、writeback 側 editable gate で
// 弾く（= sidecar には書かないが状態は追跡する）。
```

[src/ui/components.rs:776](../../src/ui/components.rs#L776) `writeback_scenario_instruments_system` と `assert_scenario_metadata_in_sync_system` の 2 system に `ExecutionModeRes` を注入し、先頭に:

```rust
if !registry.editable { return; }
if !matches!(exec_mode.mode, ExecutionMode::Replay) { return; }
```

を追加。

**Replay 再入時の挙動（Q2 を成立させる経路）**:
1. Live 中: auto-prune で registry mutate → `mark_registry_dirty_system` が revision を +1（writeback は Replay gate で skip）
2. ユーザー Replay 切替 → `ExecutionModeRes.mode = Replay` に変化
3. 次 tick: `writeback_scenario_instruments_system` が `revision != flushed_revision` を見て発火、現 registry（= Live で減った状態）を sidecar に flush

**editable=false の保護**: writeback 側 `if !registry.editable { return; }` が引き続き sidecar への書き込みを完全に阻止する。Q1 の固定 scenario restore はこのあと `restore_fixed_registry_on_replay_entry_system` で再 resolve する経路に乗る（§6.1）。

---

## §6 Replay 再入時の restore（Q1 / Q2）

### 6.1 `editable=false`: fixed scenario re-resolve（Q1 + D7）

新規 system `restore_fixed_registry_on_replay_entry_system`:

```rust
pub fn restore_fixed_registry_on_replay_entry_system(
    exec_mode: Res<ExecutionModeRes>,
    registry: Res<InstrumentRegistry>,
    scenario_path: Res<ScenarioReadTarget>,
    mut prev_mode: Local<Option<ExecutionMode>>,
    mut event_writer: EventWriter<StrategyFileLoadRequested>,
) {
    let cur = exec_mode.mode;
    let was = prev_mode.replace(cur);
    let entered_replay = was != Some(ExecutionMode::Replay)
        && cur == ExecutionMode::Replay;
    if !entered_replay { return; }
    if registry.editable { return; }
    let Some(path) = scenario_path.0.clone() else { return; };
    // v5 修正: StrategyFileLoadRequested は `path` だけでなく `mode: StrategyLoadMode`
    // が必須 ([src/ui/components.rs:351-355])。restore は「ユーザー操作 / レイアウト
    // 復元のどちらでもない」が、sidecar 適用 + 全置換挙動として LayoutRestore が
    // 最も近い (UserOpen は明示的なユーザー Open フローでサイドカー保存挙動が違う)。
    // Step 0 の副作用検証で start/stop が走らないことを確認した上で LayoutRestore を
    // 採用する。問題があれば専用 event `ScenarioReResolveRequested` に差し替え。
    event_writer.send(StrategyFileLoadRequested {
        path,
        mode: StrategyLoadMode::LayoutRestore,
    });
}
```

D7 で `parse_scenario_system` が `instruments_ref` を **resolve して `instruments` を埋める** ようになっているため、既存パイプライン (`StrategyFileLoadRequested → parse_scenario_system → ScenarioLoadedFromFile → sync_registry_from_scenario_loaded_system`) がそのまま fixed universe に registry を replace する。

**副作用検証**:
- `StrategyFileLoadRequested` を再発行すると、Engine start/stop 系の subscriber が反応しないか、Step 8 冒頭で挙動確認。問題あれば `ScenarioReResolveRequested` 専用 event に差し替え

### 6.2 `editable=true`: 何もしない（Q2）

`registry.editable == true` で `Replay` 再入時は registry をそのまま維持（restore system はそもそも `editable=false` 専用）。

writeback は §5.4 の新ゲート設計により、Live 中の auto-prune が `mark_registry_dirty_system` で revision を +N 進めた **未 flush 分** が残っているため、Replay 再入の最初の tick で `writeback_scenario_instruments_system` が `revision != flushed_revision` を検出して 1 回 flush する（= Live で減った状態がそのまま sidecar に書かれる）。ユーザーが Replay 中にさらに manual edit した分も同じ counter で追跡され、続けて flush される。

**v3 までの誤設計（修正済）**: 旧 §5.4 案では `mark_registry_dirty_system` 自体に Replay gate を入れていたため、Live 中の revision bump が抑止され、Replay 再入時に `is_changed()` が立たず writeback が発火しない循環バグになっていた。v4 で `mark_registry_dirty_system` を gate-free に戻し、Replay gate を writeback 側にだけ残すことで成立。

---

## §7 実装順序（TDD）

各 step は **RED → 実装 → GREEN** で進める。step 間に小コミット。

### Step 0 — 検証セッション（実装前 30 分）
- `_replay_provider` 単一 → multi 化（D9）の互換性確認: 既存 `test_data_engine_*` / replay E2E を grep し、`apply_replay_event(KlineUpdate(...))` を直接呼んでいるテストを列挙
- `parse_scenario_system` の `StrategyFileLoadRequested` 再発行で start/stop が走らないか動的確認
- adapter `InstrumentRaw.market` の規約確認（tachibana / kabu それぞれ）
- D14 確認: `MockVenueAdapter.login()` を引数なしの `VenueCredentials()` で呼べるか、`VenueCredentials` の必須フィールドが無いか確認（無ければ dataclass default 追加）
- D17 確認: 既存テストで `NautilusBarsReplayProvider(bar_type=...)` を直接呼んでいる箇所、`LoadReplayData(instrument_ids=...)` RPC を呼んでいる箇所を全列挙し、provider 単体 / RPC E2E に分類
- D16 確認: `bar_to_kline_update` の現行シグネチャと呼び出し元を grep（`break` 削除によるテスト退行範囲を確定）

### Step 1 — backend `ListInstruments(source)` dispatch + 最小 `VenueLogin`（D1 + D10 + D14 + D18 + D21）
- `python/tests/test_grpc_phase8.py`:
  - `test_list_instruments_local_returns_catalog_ids`
  - `test_list_instruments_unknown_source_returns_error`
  - `test_list_instruments_live_requires_logged_in_runner`
  - `test_list_instruments_live_returns_adapter_fetch_instruments`
  - `test_list_instruments_live_propagates_adapter_failure`
  - `test_list_instruments_live_empty_returns_unsupported_failure`（v4 Finding 2 pin: adapter が `[]` を返したら `success=False, error_message="LIVE_UNIVERSE_UNSUPPORTED"` で reject）
  - `test_venue_login_normalizes_lowercase_venue_id`（v4 Finding 3: `venue_id="tachibana"` でも `TACHIBANA` として通る）
  - `test_venue_login_rejects_unknown_venue_id`（v4 Finding 3）
  - `test_venue_login_rejects_venue_mismatch_with_configured_factory`（v4 Finding 3: 起動時 `--live-venue=TACHIBANA` + request `KABU` で `VENUE_MISMATCH`）
  - `test_venue_login_passes_credentials_source_and_environment_hint_to_adapter`（v4 Finding 3: request の cred_source / env_hint が `VenueCredentials` に透過）
  - `test_venue_login_logs_in_mock_adapter`（D14 + D21 pin: `VenueLogin` RPC 完了後 `adapter.is_logged_in == True`）
  - `test_venue_login_is_idempotent_when_already_connected`（D21 冪等性: 2 回連続 `VenueLogin` で error にならない）
  - `test_venue_login_returns_not_configured_when_adapter_factory_missing`（D21 negative）
  - `test_set_execution_mode_no_longer_starts_live_components_when_already_running`（D21 構造変更 pin: `SetExecutionMode` 単独呼び出しは live components を新規起動しない、起動済みであれば mode 切替だけ）
  - `test_live_runner_is_logged_in_reflects_adapter_state`（D14: `self.bus` フィールド名確認も兼ねる）
  - `test_venue_credentials_requires_credentials_source`（D14 補強: 引数なし `VenueCredentials()` が `ValidationError` を投げることを pin。plan 擬似コードの regression 防止）
  - `test_venue_login_transitions_venue_sm_to_connected`（D18 + D21 pin: `VenueLogin` RPC 完了後 `venue_sm.current == "CONNECTED"`）
  - `test_set_execution_mode_live_manual_succeeds_after_venue_login`（D21 統合: `VenueLogin → SetExecutionMode("LiveManual")` の 2 段経路で `EXECUTION_MODE_PRECONDITION` を踏まない）
  - `test_set_execution_mode_live_manual_rejects_without_prior_venue_login`（D21 negative: `VenueLogin` 抜きで `SetExecutionMode("LiveManual")` を呼ぶと `EXECUTION_MODE_PRECONDITION` で reject。これが Live 入場経路を `VenueLogin → SetExecutionMode` に分けたことの逆向き pin）
  - `test_venue_login_is_idempotent_on_venue_sm_transition`（D18 + D21: 2 回目以降 `VenueLogin` で early return）
  - `test_teardown_live_components_resets_venue_sm_to_disconnected`（v5.2 Claim 2 / D21 補強: Replay 戻りで `venue_sm.current == "DISCONNECTED"` になる）
  - `test_live_replay_live_cycle_requires_relogin`（v5.2 Claim 2: Live → Replay → Live で 2 回目の `SetExecutionMode("LiveManual")` が `_live_runner is None` で `VENUE_LOGIN_REQUIRED` を返し、新規 `VenueLogin` 経由でないと Live に戻れないことを pin。MOCK adapter で統合テスト化）
  - `test_set_execution_mode_rejects_with_venue_login_required_when_runner_missing`（v5.2 Claim 2: 冪等再起動撤廃の pin。`venue_sm == CONNECTED` でも `_live_runner is None` なら reject）
  - `test_unsubscribe_market_data_removes_id_from_last_price_cache`（D20 pin: subscribe → tick → unsubscribe → `LastPriceCache.snapshot()` から消える）
  - `test_unsubscribe_market_data_calls_runner_unsubscribe`（D20）
  - `test_get_state_live_last_prices_filtered_by_subscribed_ids`（D20 二段ガード pin: cache に残骸があっても subscribed set に居なければ last_prices に乗らない）
  - `test_last_price_cache_remove_clears_both_quote_and_trade`（D20 単体）
- `LiveRunner` に `adapter` / `is_logged_in()` / `fetch_instruments_blocking(timeout)` / `subscribed_ids()` / `unsubscribe(id)` を実装
- `VenueLogin` RPC ハンドラ実装（§2.2.1）: live components 起動 + adapter.login（D14）+ `venue_sm.transition_to(...)`（D18）。**`_start_live_components_async` 自体は触らず純粋な bootstrap のまま** 保つ（D21）
- `SetExecutionMode` ハンドラから `_start_live_components()` 事前呼び出しを削除（D21 構造修正）。代わりに `live_runner is None` だった場合の冪等再起動ガードのみ残す
- `LastPriceCache.remove(instrument_id)` 追加（D20）
- `UnsubscribeMarketData` RPC ハンドラ実装（D20 §2.8.2）
- `GetState` Live last_prices に subscribed_ids filter を追加（D20 §2.8.3）
- `ListInstruments` 分岐実装

### Step 2 — backend Replay multi-instrument（D9 + D16 + D17）
- `python/tests/replay/test_multi_instrument_replay.py`（新規）:
  - `test_replay_advances_provider_with_oldest_timestamp`
  - `test_kline_update_carries_instrument_id`
  - `test_per_id_close_accumulates_independent_prices`
  - `test_legacy_single_provider_still_works`（regression pin）
  - `test_replay_drains_all_providers_with_equal_min_ts_in_single_tick`（D24 pin: 同 ts の 2 銘柄が 1 tick で両方 per_id_close を更新）
  - `test_replay_time_updated_fires_once_per_ts_group`（D24 pin: 同 ts グループに対し ReplayTimeUpdated は 1 回だけ）
- `python/tests/test_instrument_id_to_bar_type.py`（D17 新規）:
  - `test_minute_granularity_appends_minute_spec`
  - `test_daily_granularity_appends_day_spec`
  - `test_unknown_granularity_falls_back_to_minute`
- `python/tests/test_grpc_phase8.py` 追加:
  - `test_load_replay_data_accepts_instrument_id_and_converts_to_bar_type`（D17 pin）
  - `test_start_engine_injects_bars_for_all_instruments`（D16 pin: `bars_by_instrument` に 2 銘柄ある場合に両方の `per_id_close` が埋まる）
  - `test_bar_to_kline_update_carries_instrument_id`（D16 + D9 schema pin）
- `KlineUpdate.instrument_id` / `ReducerState.per_id_close` / `_replay_providers` dict / `_advance_one_locked` の **同 ts group drain** 実装（D24 確定）
- `bar_to_kline_update(bar, instrument_id="")` シグネチャ拡張（D16）
- `StartEngine` の `for bars in bars_by_instrument.values(): ... break` から `break` 削除（D16）
- `instrument_id_to_bar_type` helper + `load_replay_data` 内変換 + proto コメント書き換え（D17）

### Step 3 — backend `last_prices` mode-aware（D8）
- `python/tests/replay/test_last_prices_replay_mode.py`:
  - `test_replay_get_state_returns_per_id_last_prices`
  - `test_live_get_state_uses_live_price_cache`
  - `test_replay_last_prices_cleared_on_session_reset`
- `engine.get_replay_last_prices()` + `GetState` mode 分岐実装

### Step 4 — backend `instruments_ref` 復活（D7 Python 側）
- `python/tests/strategy_runtime/test_scenario_extract.py`:
  - `test_validate_v3_accepts_instruments_ref`（旧 `_rejects_` を rename + 書き換え）
  - `test_validate_v3_accepts_instruments_ref_only`（v4 Finding 1 pin: `instruments` 不在で ref のみでも validate pass）
  - `test_validate_v3_accepts_both_instruments_and_ref`（v4 Finding 1: 併用可）
  - `test_validate_v3_rejects_when_neither_instruments_nor_ref`（v4 Finding 1）
  - `test_validate_v3_rejects_non_string_instruments_ref`（v4 Finding 1: 型 check）
  - `test_resolve_instruments_ref_loads_bare_path_sibling_json`（v4 Finding 1: 既存 fixture 形式）
  - `test_resolve_instruments_ref_with_json_pointer`（v4 Finding 1: pointer 拡張）
  - `test_load_scenario_expands_instruments_ref_into_instruments`
  - `test_load_scenario_ref_overrides_inline_instruments`（v4 Finding 1: mixed fixture で ref 優先を pin）
  - `test_load_scenario_resolve_runs_before_validate`（v4 Finding 1: 順序保証 — ref only でも `validate` を通過することを pin）
- `scenario.py validate` を `instruments` / `instruments_ref` 択一仕様に書き換え + `resolve_instruments_ref` 実装 + `load_scenario` で **resolve → validate の順序** を保証

### Step 5 — Rust `Tickers` schema 拡張（D6 / D6c）
- `TickersSource` / `TickersStatus` enum + `Tickers` 拡張
- `BackendStatusUpdate` 3 分割
- `TransportCommand::ListInstruments.source: TickersSource`、`UnsubscribeMarketData` 追加
- `auto_list_instruments_after_startup` 改修
- テスト:
  - `tickers_default_status_is_not_fetched_source_unknown`
  - `tickers_list_started_sets_inflight_keeps_list`
  - `tickers_listed_overwrites_list_and_source_and_status_loaded`
  - `tickers_list_failed_keeps_list_sets_status_failed`
  - `tickers_source_to_wire_maps_all_variants`
  - `unsubscribe_market_data_command_serializes_to_backend_rpc`

### Step 6 — Rust `scenario_parser.rs` resolver（D7 Rust 側）
- `ScenarioLoadedFromFile.has_instruments_ref: bool` → `ref_path: Option<String>` に置換
- `resolve_instruments_ref(ref_spec, sidecar_path)` 関数実装
- テスト:
  - `parse_resolves_instruments_ref_to_instruments`
  - `parse_inline_instruments_still_works`
  - `parse_falls_back_on_missing_ref_target`

### Step 7 — auto-prune + unsubscribe（D2 / D6b / D12 / D13）
- `src/ui/instruments_universe_prune.rs` 新ファイル（§5.1）
- `unsubscribe_removed_instruments_system`（§5.2）
- テスト:
  - `prune_removes_chart_only_id_on_switch_to_live`
  - `prune_removes_chart_only_id_on_switch_to_replay`
  - `prune_skips_when_tickers_status_not_loaded`
  - `prune_skips_when_tickers_source_is_replay_catalog_fallback_in_live_mode`（D6b pin）
  - `prune_runs_when_tickers_source_is_local_venue_snapshot_in_live_mode`
  - `prune_skips_when_available_not_fetched_in_replay`
  - `prune_runs_even_when_editable_is_false`（D2 pin）
  - `prune_keeps_list_on_failed_status_does_not_prune_from_stale`
  - `prune_skips_when_venue_state_disconnected_even_if_tickers_loaded`（D19 pin: 直前まで Loaded だった LiveVenue list を根拠に Chart を消さない）
  - `prune_skips_when_venue_state_reconnecting`（D19）
  - `invalidate_tickers_on_subscribed_to_disconnected_resets_source_and_status`（D19 pin）
  - `invalidate_tickers_on_connected_to_subscribed_does_not_fire`（D19 negative）
  - `invalidate_tickers_keeps_list_for_picker_placeholder`（D19 不変条件）
  - `unsubscribe_sent_for_removed_id_in_live`
  - `unsubscribe_not_sent_in_replay`
  - `unsubscribe_not_sent_when_no_id_removed`
  - `unsubscribe_not_sent_on_mode_change_frame`（v5 リスク表対応: Live → Replay 切替と同 tick で auto-prune が起きても Unsubscribe が大量送信されない）
  - `subscribe_sent_for_added_id_in_live`（D22 pin）
  - `subscribe_not_sent_in_replay`（D22）
  - `subscribe_not_sent_on_mode_change_frame`（D22: Replay → Live 切替で既存 registry が一括 subscribe されない）
  - `subscribe_sent_when_picker_adds_id_in_live`（D22 統合: picker → registry add → backend 受信 1 件）

### Step 8 — writeback gate に `Replay` AND（D2 補強、v4 修正）
- `mark_registry_dirty_system` は **ゲート追加せず** mode 不問で revision を bump
- `writeback_scenario_instruments_system` と `assert_scenario_metadata_in_sync_system` の **2 system にだけ** `ExecutionModeRes` を注入し、先頭で `editable=true && mode==Replay` AND gate
- テスト:
  - `mark_registry_dirty_increments_revision_even_in_live_mode`（v4 重要 pin: Live 中 mutate でも revision が +1 されることを担保。Replay 再入時の writeback 発火条件を成立させる）
  - `live_prune_skips_writeback_but_keeps_revision_pending`（Live 中 prune → revision > flushed_revision のまま、sidecar 未変更）
  - `replay_reentry_flushes_live_pruned_state_to_sidecar`（v4 Q2 成立 pin: Live で減った state を Replay 再入後の最初の tick で writeback）
  - `assert_metadata_in_sync_skipped_in_live`
  - `replay_manual_edit_writes_sidecar_as_before`（regression pin）

### Step 9 — Sidebar Tickers 撤去 + Instruments 行に price + click
- §4.1〜4.4 を一括（テスト先行）
- テスト:
  - `sidebar_has_no_tickers_section`
  - `instrument_row_has_price_text_child`
  - `instrument_row_click_sets_selected_symbol`
  - `instrument_row_click_in_live_sends_subscribe_market_data`
  - `remove_button_press_does_not_trigger_row_click`
  - `instrument_row_price_uses_last_prices_map`（D3 unified pin）

### Step 10 — `[+ Add]` モード分岐 + auto-fetch system（Q4 / Q3 / D11）
- §4.5 / §4.6 / §4.6.1 / §4.6.2 / §4.6.3
- テスト:
  - `picker_dropdown_uses_available_in_replay_mode`
  - `picker_dropdown_uses_tickers_in_live_mode`
  - `picker_dropdown_shows_not_fetched_placeholder_in_live`
  - `picker_dropdown_shows_in_flight_placeholder_in_live`
  - `picker_dropdown_shows_failed_placeholder_in_live`
  - `add_button_in_live_does_not_fetch_available_or_require_scenario_end`
  - `add_button_in_live_triggers_list_instruments_live_when_not_fetched`
  - `venue_connected_transition_auto_fetches_live_universe`
  - `venue_connected_transition_does_not_double_fetch`（D15 pin: 旧 `fire_list_instruments(..., None, ...)` を消したことを `mock transport` 受信 command が `ListInstruments { source: LiveVenue }` 1 件のみであることで pin）
  - `mode_entered_live_with_venue_already_connected_fetches_live_universe`（D23 pin: Replay 中に Venue Connect 済み → Live 切替で fetch が発火）
  - `mode_entered_live_skips_fetch_when_tickers_already_loaded`（D23 pin: Loaded なら重複 fetch しない）
  - `replay_entry_auto_fetches_available_instruments`（D11 pin）
  - `scenario_end_change_in_replay_refetches_available_instruments`（D11 pin）
  - `replay_startup_fetches_local_universe`（regression pin）

### Step 11 — Replay 再入 restore（Q1 + D7）
- §6.1
- テスト:
  - `replay_reentry_with_editable_false_restores_fixed_registry`
  - `replay_reentry_with_editable_false_restores_via_instruments_ref`（D7 pin）
  - `replay_reentry_with_editable_true_keeps_pruned_registry`
  - `replay_reentry_does_not_fire_when_already_in_replay`
  - `strategy_file_load_re_request_does_not_start_engine`（Step 0 検証の固定化）

### Step 12 — E2E 統合
- `src/ui/components.rs` E2E ブロックに追加:
  - `e2e_replay_to_live_prunes_unknown_instrument_and_unsubscribes`
  - `e2e_live_to_replay_prunes_unknown_instrument`
  - `e2e_fixed_scenario_with_instruments_ref_replay_to_live_to_replay_restores_universe`（Q1 + D7）
  - `e2e_editable_scenario_replay_to_live_to_replay_keeps_pruned`
  - `e2e_live_universe_fetched_after_venue_connected`
  - `e2e_multi_instrument_replay_populates_per_id_last_prices`（D3 + D9 + D8 全段 pin）

### Step 13 — 結線と手動 QA ✅ Round 3 完了 (2026-05-19)
- `UiPlugin::build` を §5.3 順序で更新 ✅
- `MenuItem::VenueConnectMock` を components.rs / menu_bar.rs に追加（D26） ✅
- Step 12 統合テスト追加（`integration_live_prune_then_unsubscribe_chain` / `integration_writeback_skipped_in_live_mode`） ✅
- `cargo test --workspace`: 297 passed, 0 failed ✅
- `uv run pytest python/tests/ -q`: 825 passed, 3 preexisting failed ✅
- `cargo run` 手動 QA:
  - Replay 起動（multi-instrument scenario）: sidebar 全行の price が独立に更新される
  - `[+ Add]` から catalog 銘柄追加 → Chart spawn、× で削除、scenario sidecar 更新
  - Live 切替: `Venue Login` → `Connected` 遷移で Tickers fetch → `[+ Add]` で live 銘柄追加 → Chart spawn、`SubscribeMarketData` が backend に届く
  - Replay → Live (editable=true): Live universe に無い銘柄の Chart 自動 despawn、`UnsubscribeMarketData` が backend に届く、sidecar 不更新
  - Live → Replay (editable=true): catalog に無い銘柄の Chart 自動 despawn、現 registry を sidecar に writeback
  - Live → Replay (editable=false, `instruments_ref` sidecar): fixed registry に自動復元、sidecar byte-identical

---

## §8 テストカタログ

```
python/tests/test_grpc_phase8.py
  test_list_instruments_local_returns_catalog_ids                — D1 local
  test_list_instruments_unknown_source_returns_error             — D1 validation
  test_list_instruments_live_requires_logged_in_runner           — D1 + D10
  test_list_instruments_live_returns_adapter_fetch_instruments   — D1 happy path
  test_list_instruments_live_propagates_adapter_failure          — D1 failure path
  test_venue_login_logs_in_mock_adapter                          — D14 + D21
  test_venue_login_transitions_venue_sm_to_connected             — D18 + D21
  test_venue_login_is_idempotent_when_already_connected          — D21
  test_venue_login_returns_not_configured_when_adapter_factory_missing — D21
  test_set_execution_mode_live_manual_succeeds_after_venue_login — D21
  test_set_execution_mode_live_manual_rejects_without_prior_venue_login — D21
  test_set_execution_mode_no_longer_starts_live_components_when_already_running — D21
  test_live_runner_is_logged_in_reflects_adapter_state           — D14
  test_load_replay_data_accepts_instrument_id_and_converts_to_bar_type — D17
  test_start_engine_injects_bars_for_all_instruments             — D16
  test_bar_to_kline_update_carries_instrument_id                 — D16 + D9

python/tests/test_instrument_id_to_bar_type.py
  test_minute_granularity_appends_minute_spec                    — D17
  test_daily_granularity_appends_day_spec                        — D17
  test_unknown_granularity_falls_back_to_minute                  — D17

python/tests/replay/test_multi_instrument_replay.py
  test_replay_advances_provider_with_oldest_timestamp            — D9 ordering
  test_kline_update_carries_instrument_id                        — D9 schema
  test_per_id_close_accumulates_independent_prices               — D9 state
  test_legacy_single_provider_still_works                        — D9 regression

python/tests/replay/test_last_prices_replay_mode.py
  test_replay_get_state_returns_per_id_last_prices               — D8 + D9
  test_live_get_state_uses_live_price_cache                      — D8 regression
  test_replay_last_prices_cleared_on_session_reset

python/tests/strategy_runtime/test_scenario_extract.py
  test_validate_v3_accepts_instruments_ref                       — D7（書き換え）
  test_resolve_instruments_ref_loads_sibling_json                — D7
  test_resolve_instruments_ref_with_json_pointer                 — D7
  test_load_scenario_expands_instruments_ref_into_instruments    — D7

src/trading.rs (tests)
  tickers_default_status_is_not_fetched_source_unknown           — D6 default
  tickers_list_started_sets_inflight_keeps_list                  — D6c
  tickers_listed_overwrites_list_and_source_and_status_loaded
  tickers_list_failed_keeps_list_sets_status_failed              — D6 stale
  tickers_source_to_wire_maps_all_variants
  unsubscribe_market_data_command_serializes_to_backend_rpc      — D12

src/ui/scenario_parser.rs (tests)
  parse_resolves_instruments_ref_to_instruments                  — D7 Rust
  parse_inline_instruments_still_works                           — regression
  parse_falls_back_on_missing_ref_target

src/ui/instruments_universe_prune.rs (tests)
  prune_removes_chart_only_id_on_switch_to_live
  prune_removes_chart_only_id_on_switch_to_replay
  prune_skips_when_tickers_status_not_loaded
  prune_skips_when_tickers_source_is_replay_catalog_fallback_in_live_mode  — D6b
  prune_runs_when_tickers_source_is_local_venue_snapshot_in_live_mode
  prune_skips_when_available_not_fetched_in_replay
  prune_runs_even_when_editable_is_false                         — D2
  prune_keeps_list_on_failed_status_does_not_prune_from_stale    — D6
  unsubscribe_sent_for_removed_id_in_live                        — D12
  unsubscribe_not_sent_in_replay                                 — D12
  unsubscribe_not_sent_when_no_id_removed                        — D12

src/ui/components.rs (writeback)
  mark_registry_dirty_increments_revision_even_in_live_mode      — v4 D21/Q2 構造 pin
  live_prune_skips_writeback_but_keeps_revision_pending          — D2 v4 修正
  replay_reentry_flushes_live_pruned_state_to_sidecar            — Q2 v4 成立 pin
  assert_metadata_in_sync_skipped_in_live                        — D2 補強
  replay_manual_edit_writes_sidecar_as_before                    — regression

src/ui/sidebar.rs (tests)
  sidebar_has_no_tickers_section
  instrument_row_has_price_text_child
  instrument_row_click_sets_selected_symbol
  instrument_row_click_in_live_sends_subscribe_market_data
  remove_button_press_does_not_trigger_row_click
  instrument_row_price_uses_last_prices_map                      — D3

src/ui/instrument_picker.rs (tests)
  picker_dropdown_uses_available_in_replay_mode
  picker_dropdown_uses_tickers_in_live_mode
  picker_dropdown_shows_not_fetched_placeholder_in_live
  picker_dropdown_shows_in_flight_placeholder_in_live
  picker_dropdown_shows_failed_placeholder_in_live
  add_button_in_live_does_not_fetch_available_or_require_scenario_end
  add_button_in_live_triggers_list_instruments_live_when_not_fetched
  venue_connected_transition_auto_fetches_live_universe          — Q3
  venue_connected_transition_does_not_double_fetch               — D15
  replay_entry_auto_fetches_available_instruments                — D11
  scenario_end_change_in_replay_refetches_available_instruments  — D11
  replay_startup_fetches_local_universe                          — regression

src/ui/restore.rs (tests, 新規 module)
  replay_reentry_with_editable_false_restores_fixed_registry     — Q1
  replay_reentry_with_editable_false_restores_via_instruments_ref — Q1 + D7
  replay_reentry_with_editable_true_keeps_pruned_registry
  replay_reentry_does_not_fire_when_already_in_replay
  strategy_file_load_re_request_does_not_start_engine            — 副作用 pin

src/ui/components.rs (E2E)
  e2e_replay_to_live_prunes_unknown_instrument_and_unsubscribes
  e2e_live_to_replay_prunes_unknown_instrument
  e2e_fixed_scenario_with_instruments_ref_replay_to_live_to_replay_restores_universe
  e2e_editable_scenario_replay_to_live_to_replay_keeps_pruned
  e2e_live_universe_fetched_after_venue_connected
  e2e_multi_instrument_replay_populates_per_id_last_prices       — D3 + D8 + D9
```

---

## §9 リスクと対策

| リスク | 対策 |
|---|---|
| D9 multi-instrument 化が既存 single-instrument E2E を壊す | Step 0 で既存 `apply_replay_event` 直接呼び出し箇所を全洗い出し。`_replay_provider` 単一 path も legacy として温存。`_rs.price` / `_rs.history` 累積は primary id のみ（UI Chart 1 系列の挙動を保つ） |
| D9 で `KlineUpdate.instrument_id` を default `""` にしたことで existing test が「空 id 経路」を回し続け、新 `per_id_close` を検査しない | Step 2 の `test_kline_update_carries_instrument_id` で「id 付き」「id 無し」両方を pin。混在パターン test も入れる |
| D7 で `instruments_ref` を許可に戻したら他テストの「unknown key reject」regression が壊れる | Step 4 で `test_validate_v3_rejects_instruments_ref` を rename+書き換え。他の unknown key test（例: typo 検出）は別 key で残す |
| D7 resolver の JSON Pointer 実装が複雑化 | RFC 6901 のフルサポートは不要。`#/instruments` 形式のみ対応で十分（既存 sidecar の使い方が固定）。Step 4 テストで形式を pin |
| D10 暫定 `is_logged_in` が `_start_live_components` 成功で True 返却 → Phase 8.8 で本実装に差し替え忘れ | Step 1 テストに `# TODO(phase-8.8): replace with real login state` コメント。Phase 8.8 のチケットに引き継ぐ |
| D11 system が backend 未接続時に無限 dispatch（in_flight に入らないまま `last_error` だけ立てて毎フレーム再試行） | system 内で `prev_end` だけでなく `last_error.is_some() && last_error.0 == end` も skip 条件に追加 |
| D12 unsubscribe diff system が初回 frame で `prev_ids` が空のため何も送らない、その後 mode 切替で誤送信 | Local の `prev_ids` を `is_changed()` ではなく実値 diff で扱う（実装どおり）。`exec_mode.is_changed()` 直後の frame は diff を skip して `prev_ids` だけ更新するガードを追加 |
| auto_fetch_live_universe で `Subscribed` → `Reconnecting` → `Connected` 往復で過剰 fetch | `prev_state` Local 比較で「Connected/Subscribed への**新規**遷移」時のみ発火（実装どおり） |
| Live モードで `editable=true` のまま prune が起き、Replay 戻り時に空 registry を writeback して sidecar 破壊 | §5.4 の `Replay` gate を `writeback_scenario_instruments_system` 側だけに置き、Live 中は writeback skip。`mark_registry_dirty_system` は revision を bump し続けるので、Replay 戻り後の最初の tick で 1 回 flush して追いつく（Q2 v4 仕様）。空 registry になりうるシナリオ自体は editable=true scenario の運用判断（ユーザーが Live で全消ししたなら sidecar も空になる）。空 registry を許さないなら別途 `writeback_scenario_instruments_system` 内に `if registry.is_empty() { return; }` を入れる選択肢を Phase 8.8 で再検討 |
| Sidebar 行 click と remove button の event 競合 | Step 9 の `remove_button_press_does_not_trigger_row_click` で pin。entity が別なので Bevy 上は分離されるが padding/margin の hit area 重なりは目視確認 |
| `InstrumentsListStarted` 発火後 `Listed/Failed` が来ないままタイムアウト | transport task に timeout を設定し、超過時に必ず `Failed` push（D6c の不変条件） |
| Q1 で `StrategyFileLoadRequested` 再発行が start/stop 系副作用を起こす | Step 0 と Step 11 の `strategy_file_load_re_request_does_not_start_engine` で pin。問題あれば `ScenarioReResolveRequested` 専用 event に差し替え |
| D14: `VenueCredentials()` 引数なし構築が tachibana / kabu adapter で fail する | mock adapter のみが `login()` 引数を緩く許容する想定。`getattr(adapter, "is_logged_in", True)` が True を返す real adapter（= 既に別経路で login 済み / 未実装で True default）では D14 ブロックは no-op。real adapter の正規 login は Phase 8.8 `VenueLogin` 経路 |
| D14: そもそも plan 擬似コードの `VenueCredentials()` が pydantic v2 `ValidationError` で死ぬ | v3.1 で `VenueCredentials(credentials_source="env")` に修正済み。Step 1 `test_venue_credentials_requires_credentials_source` で「引数なしは ValidationError」を pin し、plan ドリフト regression を防止 |
| D18: `venue_sm.transition_to` の不正遷移例外（`DISCONNECTED` 以外で再呼び出し時） | `VenueLogin` ハンドラ冒頭で `if venue_sm.current in ("CONNECTED", "SUBSCRIBED"): return success` の冪等 early return + 内部の `if venue_sm.current == "DISCONNECTED":` ガード。Step 1 `test_venue_login_is_idempotent_when_already_connected` / `test_venue_login_is_idempotent_on_venue_sm_transition` で 2 回連続呼び出しが exception を投げないことを pin |
| D21: `SetExecutionMode` 単独経路で Live 入場を試みる古い UI / クライアントが `EXECUTION_MODE_PRECONDITION` で永久に reject される | UI 側 `VenueLogin → SetExecutionMode` 順を必須化（`auto_venue_login_on_startup_system` + footer ボタン）。`test_set_execution_mode_live_manual_rejects_without_prior_venue_login` で逆向き挙動を pin、`test_set_execution_mode_live_manual_succeeds_after_venue_login` で正規経路を pin。manual QA で「`Venue Login` 押下前に Live toggle 押下しても何も起きない」を確認 |
| D21: 最小 `VenueLogin` 実装が `_start_live_components` 例外（adapter factory が None / bus 起動失敗）を握る箇所が増えて握り潰し | ハンドラ内 try/except は logging.exception + `error_code="VENUE_LOGIN_FAILED"` 返却で必ず response に出す。Step 1 negative test `test_venue_login_returns_not_configured_when_adapter_factory_missing` で代表ケースを pin |
| D18: mock の自動 CONNECTED 遷移が tachibana/kabu real adapter にも誤って適用される | gate は `self.venue_sm.current == "DISCONNECTED"` のみで adapter 種別を問わないが、real adapter の login lifecycle は Phase 8.8 で `VenueLogin` RPC が `venue_sm` を本格管理するため、Phase 8.8 着手時に D18 ブロック自体を撤去するチケットを残す（DoD #10 memory に明記） |
| D19: `Connected → Subscribed` 遷移を「live → !live」と誤判定して Tickers をリセット | `was_live` / `is_live` どちらも `Connected \| Subscribed` の or で組むため、状態遷移内では発火しない。`invalidate_tickers_on_connected_to_subscribed_does_not_fire` で pin |
| D19 で Tickers リセット後、再接続したのに universe が空のまま放置 | `Connected` 復帰時に `auto_fetch_live_universe_on_connect_system` が新規遷移として再 fetch する（§5.3 順序で invalidate → auto_fetch の順）。手動 QA で `Disconnect → Reconnect` の往復後 picker に live 銘柄が出ることを確認 |
| D20: `LastPriceCache.remove` の race（`_run` task が同時 write） | `dict.pop(key, None)` は GIL 下で atomic。例外時も `_run` task は次の event で再 write するので、stale が一時的に再出現する程度。subscribed_ids filter (§2.8.3) が二段ガードとして残骸を遮断 |
| D20 `UnsubscribeMarketData` ハンドラ未配線 | proto には [engine.proto:70](../../python/proto/engine.proto#L70) `rpc UnsubscribeMarketData (UnsubscribeRequest) returns (SubscribeResponse)` が既存。Phase 8.7 の D20 では **server_grpc.py 側のハンドラ実装** と `LastPriceCache.remove` 連動だけ追加すれば proto は触らずに済む（OUT-3 と矛盾しない） |
| D15: `fire_list_instruments(..., None, ...)` 削除で venue transition 時の **local universe refresh** も同時に失われる | local universe は `auto_list_instruments_after_startup`（startup 1 回）で取得済み。venue 接続中に local catalog が更新される稀ケースは Phase 8.7 で対応せず、必要なら `[+ Add]` から手動 refresh で対処 |
| D16: `break` 削除で `bars_by_instrument` が空 list を含む銘柄に対し冗長ループ | `if not bars: continue` で skip。実害なし |
| D17: instrument id → BarType 変換で granularity 違いの BarType が catalog に存在せず `load_bars` が空 list を返す | provider constructor で `bars == []` なら `ValueError("no bars for {bar_type}")` を raise（既存挙動）。`load_replay_data` 呼び出し側で `(False, msg)` 経路へ落ちる。E2E 側で `test_load_replay_data_with_missing_bartype_returns_failure` を追加して pin |

---

## §10 DoD

1. `cd python && uv run pytest -m "not slow"` 全 GREEN（既存 + Step 1〜4 の新規）
2. `cargo test` 全 GREEN（既存 + Step 5〜12 の新規）
3. `cargo run` 手動 QA で §7 Step 13 の 6 シナリオすべて意図通り動作
4. Sidebar に "Tickers" セクションと検索ボックスが **無い**
5. Live モードで menu_bar `Venue → Connect → Mock` 成功後、自動で live universe が `[+ Add]` に並ぶ（D26: backend は `--live-venue MOCK` 起動。MockVenueAdapter が `["7203.TSE", "9984.TSE"]` を返す）。tachibana/kabu の real venue は Phase 8.7 では `_list_instruments_live` が session-less 例外 / `LIVE_UNIVERSE_UNSUPPORTED` を返す前提（Tickers status=Failed、list 維持、prune skip。Live 通電は Phase 8.8 で `VenueLogin` 本実装が `credentials_source="env"` を実 login に流す形で完成）
6. Replay モードで複数 Chart の price が **同時に** sidebar に表示される（D3 + D8 + D9 完了確認）
7. `instruments_ref` fixed sidecar を開いた状態で Live → Replay 往復しても sidecar ファイルが byte-identical（Q1 + D7）
8. Live モードで Chart を閉じると backend に `UnsubscribeMarketData` が届く（D12）、かつ backend `LastPriceCache` から該当 id が消える（D20）。逆方向: Live で `[+ Add]` から銘柄追加すると backend に `SubscribeMarketData` が 1 度だけ届く（D22、行 click なしで購読開始）
9. Replay 入場時、ユーザーが `[+ Add]` を開かなくても auto-prune が走る（D11）
10. Venue 切断（`Connected → Disconnected`）後も Chart が消えない、再接続後に live universe が再取得されて picker dropdown に並ぶ（D19）
11. UI 上で menu_bar `Venue → Connect → Mock` → `Connected` 遷移後に `LiveManual/LiveAuto` への切替が `EXECUTION_MODE_PRECONDITION` 例外なく成立する（D14 + D18 + D21 + D26）。auto-startup login は Phase 8.7 では実装せず、手動 QA も明示的な Venue Connect 操作を含む。**MockVenueAdapter は inject_tick / emit_depth_snapshot で `DepthUpdate` を流す経路がある**ため sidebar price 列も埋まる（D27 の `_last_kline` fallback も併用、kline pass-through が venue から来た場合に動く）
12. memory に caveat を追加: 「auto-prune は `editable` を見ない」「Live prune 許可条件は `Tickers.status==Loaded && source ∈ Live* && VenueState ∈ {Connected, Subscribed}` の 3 条件 AND（D19）」「Replay last_prices は server `GetState` 側で mode 分岐して埋める」「Replay engine は `_replay_providers` dict で最古 ts pick」「`instruments_ref` は Phase 8.7 で resolver 復活」「mock adapter は `VenueLogin` RPC ハンドラ内で adapter.login を実行（D14 + D21、配置先は `_start_live_components_async` ではなくハンドラ内、Phase 8.8 で本実装に差し替え）」「`credentials_source` は **request 透過** であってハードコードではない（v5 修正）。menu_bar 現行 UI が送る `"prompt"` を mock adapter は環境変数 fallback で受理する」「**`VenueCredentials()` 引数なしは pydantic v2 `ValidationError`。必ず `credentials_source="env"/"prompt"/"session_cache"` を渡す**」「`venue_sm.transition_to("AUTHENTICATING")→"CONNECTED"` は **`VenueLogin` RPC ハンドラ内**（§2.2.1）でのみ呼ぶ。`_start_live_components_async` には絶対に置かない（D21 で潰した循環依存が再発するため）。D14 + D18 + D21 配置統一」「VenueState が live → !live に遷移した瞬間 `Tickers.source=Unknown, status=NotFetched` にリセットして stale universe で prune が走るのを防ぐ（D19）」「`LastPriceCache` には `remove(id)` がある。`UnsubscribeMarketData` ハンドラから必ず呼ぶ。さらに `GetState` Live last_prices は `runner.subscribed_ids()` で二段 filter する（D20）」「**`LastPriceCache` は `KlineUpdate.close` を `_last_kline` に取り込む（D27）**。`LiveRunner` は raw `TradesUpdate` を bus に publish せず aggregated `KlineUpdate` だけ流すため、Depth が来ない adapter では `_last_kline` fallback が無いと Live price 列が黙って空になる」「**Phase 8.7 で Live 通電するのは MOCK venue のみ（D26）**。`build_live_adapter_factory` に MOCK 分岐、`_KNOWN_VENUES` に "MOCK"、menu_bar に `VenueConnectMock` 項目を追加。real venue (tachibana/kabu) の `_list_instruments_live` 通電は Phase 8.8 で `credentials_source="env"` 実 login を入れるまで未通電」「venue transition 時の `fire_list_instruments(..., None, ...)` は D15 で削除済み — Live universe fetch は `auto_fetch_live_universe_on_connect_system` 一本」「`StartEngine` の post-run injection は全 instrument loop（D16、`break` 削除済み）」「`LoadReplayData.instrument_ids` は instrument id 形式、backend が `instrument_id_to_bar_type` で BarType 変換（D17）」

---

## 付録 A: 既存挙動の保護リスト（regression pin）

- `test_e2e_open_to_chart_spawn`
- `test_e2e_add_via_picker_creates_chart_and_writes_cache_sidecar`
- `test_e2e_remove_via_registry_writes_back_scenario_instruments`
- `instrument_chart_sync_system_*`（spawn / despawn / idempotent / partial diff）
- `replay_startup_fetches_local_universe`
- `mark_registry_dirty_system` 既存テスト群（editable=false で no-op）
- `picker_*` 既存テスト群（debounce / locked / 100ms gate）
- replay single-instrument E2E（D9 legacy path 維持）

---

## 付録 B: フォールバック（Decision Log を覆したい場合）

| Decision | フォールバック | 影響 |
|---|---|---|
| D1 後回し | `ReplayCatalogFallback` のまま Live 流用 | D6b により Live 切替時 auto-prune が動かない |
| D7（Q1 yes）撤回 | `editable=false` 時の restore を諦め、Live → Replay で「instruments が減ったまま」を許容 | UX 後退、ただし Phase 8.7 scope は小さくなる |
| D8 やらず core.py だけ直す | `last_prices` が server で常に上書きされ Replay には届かない | D3 が表面に出ない（症状は v2 と同じ） |
| D9 やらず Replay multi-instrument 諦め | `per_id_close` は `instrument_ids[0]` のみ更新 | sidebar price 列は primary 銘柄しか動かない、D3 が事実上 1 銘柄のみ |
| D10 暫定 `is_logged_in=True` 撤回 | Phase 8.8 の VenueLogin 本実装を Phase 8.7 内に取り込む | scope が +1 Phase 分膨らむ |
| D11 やらず picker 押下 fetch のまま | Replay 入場直後の auto-prune が機能しない | Live → Replay で「catalog に無い銘柄の Chart」が残る |
| D12 やらず unsubscribe 無視 | kabu 50 銘柄上限に貼り付く / stale LastPrices | Phase 8 trader experience の信頼性が崩れる |
| D14 やらず mock adapter login スキップ | `_list_instruments_live` が常に `LIVE_VENUE_NOT_LOGGED_IN` を返す | Live `[+ Add]` dropdown が永遠に Failed のまま。Phase 8.7 の Live 経路が画面上で完全に死ぬ |
| D15 やらず 旧 venue-transition fetch 残置 | 同 trigger で `source=None` と `source=LiveVenue` の 2 本が同時飛び、後着が `Tickers` を上書き | D6b の `source ∈ {LiveVenue, LocalVenueSnapshot}` 判定が race で false negative になり Live prune が動かない時間帯が発生 |
| D16 やらず `break` 残置 | Run 完了後の `apply_replay_event` が primary 1 銘柄分のみ | D9 + D8 の per_id_close が primary しか埋まらず、sidebar 行 price が全銘柄で動く DoD #6 が満たせない |
| D17 やらず `instrument_ids` 解釈ブレ放置 | Rust `InstrumentRegistry.ids` ("1301.TSE") を BarType として直接 catalog に投げて load_bars が空で fail | Replay multi-instrument 経路が catalog エラーで起動不能になる |
| D18 やらず `venue_sm` 遷移なし | UI から `LiveManual/LiveAuto` への切替が `EXECUTION_MODE_PRECONDITION` で常に失敗 | Phase 8.7 の Live 経路が画面上で完全に死ぬ（VenueLogin が NOT_IMPLEMENTED のため）。Phase 8.8 まで Live モードが未通電状態 |
| D19 やらず VenueState を prune gate に入れない | venue disconnect / venue 切替後も古い `LiveVenue` Tickers list を根拠に Chart が消える | 接続が瞬断するたびに「直前の universe」で Chart が殺される race。再接続後に手動で `[+ Add]` し直す UX 後退 |
| D20 やらず `LastPriceCache.remove` 無し | Chart 削除後も backend cache に価格が残り、行を再 add した瞬間に古い価格が出る。kabu 50 銘柄上限とは独立のメモリ leak | sidebar price 列で stale 表示。subscribe/unsubscribe を繰り返す usage で dict が単調増加 |


## 付録 C: v4 で修正した致命:
- v3 の D14/D18 配置は `_start_live_components_async` 内 → 同関数の呼び出し元は `SetExecutionMode` のみ → `mode_manager.set_execution_mode` precondition が `venue_sm.CONNECTED` を要求 → 循環依存で Live 入場不能。**D21（最小 `VenueLogin` RPC ハンドラ）を新設** し、Live 入場を `VenueLogin → SetExecutionMode` の 2 段経路に分解。**v4 review: D14/D18 文面が `_start_live_components_async` 配置のまま残っていたので D21 と整合する位置に書き換え**
- v3 §5.4 で `mark_registry_dirty_system` に Replay gate を入れると、Live 中の revision bump が抑止され、Bevy change detection の frame-local 特性により Replay 再入時に `is_changed()` が立たず writeback が永久に発火しない（Q2 不成立）。**`mark_registry_dirty_system` は mode 不問・gate なしに修正**、Replay gate を `writeback_scenario_instruments_system` 側のみに置く。**v4 review: §5.3 順序表に残っていた `(Replay only)` 表記を §5.4 と整合する形に訂正**
- §00 narrative と D2 / §1 matrix の `editable` 責務記述の不整合を解消し、「`editable` は manual edit + sidecar writeback の両方を gate する」を明示

## 付録 D: v4 追加 review で修正した致命:
- **§2.5.1 validate vs resolve 順序**: `_V3_OPTIONAL` に `instruments_ref` を追加するだけでは v3 schema が `instruments` を required にしている ([scenario.py:194-197](../../python/engine/strategy_runtime/scenario.py#L194)) ため「`instruments_ref` only」シナリオが reject される。validate を `instruments` / `instruments_ref` 択一に書き換え + `load_scenario` で **resolve → validate の順** を保証するように修正。bare path 形式（既存 fixture）と JSON pointer 形式の両対応を明記
- **§2.1 KABU empty universe**: `fetch_instruments() → []` ([kabusapi.py:94](../../python/engine/exchanges/kabusapi.py#L94)) を `success=True + empty list` で Rust に返すと §5.1 Live prune が全 Chart を消す。`_list_instruments_live` 末尾に `if not raws: return Failed("LIVE_UNIVERSE_UNSUPPORTED")` を追加し、list 維持 + prune skip 経路に倒す
- **§2.2.1 VenueLogin venue 契約**: 擬似コードが `request.venue_id` / `credentials_source` / `environment_hint` を全て無視 + `credentials_source="env"` ハードコード。UI ([menu_bar.rs:425-435](../../src/ui/menu_bar.rs#L425)) は lowercase + `"prompt"` を送るので `_KNOWN_VENUES = {"TACHIBANA", "KABU"}` ([server_grpc.py:158](../../python/engine/server_grpc.py#L158)) と完全不整合。`venue_id.upper()` 正規化、configured venue mismatch reject (`VENUE_MISMATCH`)、`credentials_source` / `environment_hint` の `VenueCredentials` 透過を追加
- **§2.2.1 / §0 Footer Venue Login の不在**: 「Footer Venue Login ボタン」も `auto_venue_login_on_startup_system` も実コードに存在しない（[footer.rs](../../src/ui/footer.rs) に該当ボタン無し、grep 0 件）。menu_bar 既存 `Venue → Connect` を Phase 8.7 の正規ルートに採用し、auto-startup login は Phase 8.8 送りに明文化
- **§5.3 順序表 `mark_registry_dirty_system (Replay only)`**: §5.4 と直接矛盾していたので「mode 不問で revision を bump」に訂正

## 付録 E: v5 で修正した致命（外部 review 反映）:
- **D22 / §4.3.1: picker add → Live 自動 Subscribe**: 旧設計は picker add で registry mutate するのみで `SubscribeMarketData` を発火しなかった。Phase 8.7 で Tickers セクション撤去後、ユーザーが新規追加銘柄の購読を開始するには「picker で add → Instruments 行を改めて click」という 2 段操作が必要になり、DoD #8 と矛盾。`subscribe_added_instruments_system` を新設し、registry diff で added id を Live mode のときだけ `SubscribeMarketData` 発火。mode 切替直後 frame は skip
- **D23 / §4.6.1: ExecutionMode 遷移を fetch trigger に追加**: 旧設計の `auto_fetch_live_universe_on_connect_system` は VenueState 遷移のみ trigger だった。Replay 中に Venue Connect 済み → 後で Live 切替えるケースで VenueState 不変 + Live mode gate により fetch が永遠に発火しない bug。trigger を「VenueState live 新規遷移 OR ExecutionMode Live 新規遷移」の OR 条件に拡張、Tickers.status Loaded/InFlight なら skip
- **D24 / §2.3.2: Replay multi-instrument 同 ts drain**: 旧 `_advance_one_locked` は最古 ts の 1 provider のみ pop だった。同 ts (日足同 close / 同分足) で複数銘柄が並んだ場合に「同時」表示にならず DoD #6 厳密違反。`min_ts` と等しい ts を持つ全 provider を 1 tick でまとめて drain、`ReplayTimeUpdated` は同 ts グループに 1 回だけ
- **§6.1 restore に `mode` 必須フィールド明記**: 旧擬似コード `StrategyFileLoadRequested { path }` は compile error（[src/ui/components.rs:351-355](../../src/ui/components.rs#L351) で `mode: StrategyLoadMode` 必須）。`LayoutRestore` を明示採用し、副作用検証で問題あれば `ScenarioReResolveRequested` 専用 event 化
- **§0 / OUT-1 credentials_source ハードコード表記の整理**: v4 までの「`credentials_source="env"` 固定」が §2.2.1 の request 透過設計と矛盾していた。§0 / OUT-1 / DoD memory line を request 透過 (`"prompt"/"session_cache"/"env"` 受理、mock adapter は環境変数 fallback) に統一

## 付録 F: v5.1 で修正した致命（外部 review 反映）:
- **D26 / §2.1 / §2.2.1 / menu_bar / DoD #5,#11: MOCK venue の通常導線追加**: v5 まで `live_adapter_factory` は `TACHIBANA` / `KABU` のみ、`_KNOWN_VENUES` も同様、menu_bar Venue→Connect も real venue のみ。MOCK adapter を立ち上げる経路が存在せず、`MockVenueAdapter.is_logged_in` を `True` 化する D14 配置に到達不能だった。さらに tachibana adapter は `is_logged_in` 属性自体を持たず `getattr(..., True)` default で login skip → `fetch_instruments` が `_session is None` で `RuntimeError`。よって Phase 8.7 DoD #5/#11 は **どの venue でも画面上完走しない**状態だった。`build_live_adapter_factory` に `"MOCK" → MockVenueAdapter()` を追加、`_KNOWN_VENUES` に `"MOCK"`、menu_bar に `VenueConnectMock` 項目、`--live-venue MOCK` で backend 起動する運用を Phase 8.7 の正規ルートに採用。real venue 通電は Phase 8.8 送り
- **D27 / §2.8.1b: LastPriceCache に KlineUpdate.close ingest**: `LiveRunner._run` は `TradesUpdate` を **aggregator に渡して closed bar (= `KlineUpdate`) のみ bus.publish** する設計。`LastPriceCache` は KlineUpdate を `pass` で ignore していたため、Depth が来ない adapter (trade-only / kline-only venue) で Live sidebar price 列が永遠に空になる致命を D3「Live でも `TradingState.last_prices` 統一」の invariant が黙って踏んでいた。`_last_kline` 辞書を追加し snapshot 優先順位を `quote_mid > last_trade > last_kline` に拡張、`remove(id)` も `_last_kline` を pop する形に修正
- **D9 行 / §0 Scope / Step 1 checklist の「最古 ts pick (1 provider)」表記訂正**: D24 で確定した「同 ts group drain (全 provider)」と矛盾する旧表記が 3 箇所に残存していたので統一
- **DoD #12 memory caveat の `_start_live_components_async` 残骸削除**: D21 で潰した循環依存配置を再導入する文言が caveat に残っていたので「`venue_sm.transition_to` は `VenueLogin` RPC ハンドラ内のみ」に修正
- **D25 / §2.5 resolver 責務分離明記**: Python (`scenario.py`) と Rust (`scenario_parser.rs`) の `instruments_ref` resolver 二重実装は責務として正当 (前者 = strategy_runtime validate、後者 = UI sidecar 読込) だが、ファイル形式仕様の drift リスクがある。両者が **同一仕様を共有** する不変条件を §2.5 冒頭に明記、Phase 8.7 では bare path + 最小 JSON pointer のみ、将来拡張時は共通仕様 doc 化
- **§5.2 mode-change frame skip ガード**: リスク表 line 1634 で「`exec_mode.is_changed()` 直後 frame は diff を skip するガード追加」と書かれていたが §5.2 擬似コードに反映されていなかった。`prev_mode` Local を追加して mode 切替直後 frame は `prev_ids` 更新のみで skip、Step 7 のテストカタログにも対応 case を追加