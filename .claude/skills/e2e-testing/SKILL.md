---
name: e2e-testing
description: The-Trader-Was-Replaced（Bevy + gRPC backend）の E2E 手動検証パターン。`backcast.exe`（Rust/Bevy GUI）と `python -m engine`（gRPC backend, port 19876）を起動し、AI とユーザーで役割分担しながら検証する。「E2E」「手動検証」「動作確認」「backcast を起動」「backend を起動」「Run ボタン」「Strategy を動かしたい」「gRPC: OK にならない」「リプレイを実機で確認」「レイアウトをロード」「パネルが復活するか確認」「Save/Load のテスト」「Ctrl+O で開く」「Ctrl+S で保存」「× ボタンを確認」「パネルが閉じるか確認」「再起動して確認」「自動 Load を確認」「オートオープン」「Undo/Redo を確認」「Ctrl+Z が動くか確認」「キーボードショートカットを実機で確認」「undo が効いているか確認」「redo を試したい」「cache sidecar を確認」「scenario キーが残っているか確認」「autosave 後に JSON を確認」「sidecar が壊れないか確認」「Load... で JSON がコピーされるか」「メニューバーを確認」「ドロップダウンが開くか」「Alt+F が動くか」「Alt+E が動くか」「メニューショートカットを確認」「File メニュー」「Edit メニュー」「マルチ spawn を確認」「複数エディタを確認」「再起動して復元されるか」「region が復元されるか」「エディタ窓が戻るか」「spawn 後に Run が動くか」「ペア保存を確認」「.py と .json が同時に保存されるか」「保存後のタイムスタンプを確認」「メニューの項目が正しいか確認」と言われたら必ず起動する。Playwright / HTTP API（旧 :9876）/ WebSocket IPC は **使わない**（過去の e-station アーキテクチャの残骸）。Phase 7.6 以降: レイアウト JSON に `strategy_path` フィールドが追加済み。Ctrl+O ロード時に Strategy Editor のファイルも自動復元される（`PendingStrategyLoad` 経由）。Phase 7.7 以降: 起動時オートオープン（7B）・サイドカー自動 Load（7C）・デバウンス自動保存（7D）・タイトルバー × ボタン（7Z）が追加。Phase 7.1: Undo/Redo キーバインド（Ctrl+Z/Y/Shift+Z）が追加。Phase 7.3: scenario sidecar 移行済み。cache に `<hash>__<strategy>.json` が同梱コピーされる（T1E）。layout autosave 後も `scenario` キーが保持される（T1D）。Phase 7.1 追加分: メニューバーをドロップダウン式（File/Edit）に変更、Alt+F/Alt+E でドロップダウン展開。"Open Strategy..." ボタン削除。Edit メニューに Undo(Ctrl+Z)/Redo(Ctrl+Y) 追加。Alt+F/E 時は `Events<KeyboardInput>.clear()` で bevy_cosmic_edit への文字書き込みを防止済み。Phase 7.x: File メニューが Save Layout / Save Layout As / Load Layout の3項目に整理。Ctrl+S で .json と .py を同時保存（ペア保存）。保存成功確認は `backcast_err.txt` で `layout saved to` と `strategy .py saved to` が連続して出ることで確認。
---

# E2E Testing — The-Trader-Was-Replaced（Bevy GUI + Python gRPC backend）

> **重要**: 本リポジトリの E2E は **GUI を実際に立ち上げて目視確認する** タイプ。
> 自動 UI 操作の仕組み（Playwright / Selenium / Iced helper attach）は無い。
> したがって **AI 単独では完結しない**。ユーザーとの役割分担を最初に握ること。

---

## 0. まず最初に — 役割分担（必読）

手動検証セッションでは AI とユーザーが必ず分担して動く。`docs/plan/Phase 7 - Replay UI Integration.md` §7「Implementation Tips: [AI 作業分担]」が出典。

| やること | 誰 |
|---|---|
| backend (`python -m engine`) の起動 / 停止 / kill | **AI** |
| Rust GUI (`backcast.exe`) の起動 / 停止 / kill | **AI** |
| port 19876 競合チェック・既存プロセス kill | **AI** |
| backend / Rust ログのファイルリダイレクト & `cat` で確認 | **AI** |
| gRPC 疎通確認（`grpc: OK` がフッターに出ているか確認するため、画面はユーザー側） | **AI が指示・ユーザーが目視** |
| **UI のボタン操作**（Load... / Run / Sidebar 切替 など） | **ユーザー** |
| **画面の目視確認**（candle が出たか、フッター状態、Run Result Panel など） | **ユーザー** |
| スクリーンショット撮影・貼付 | **ユーザー** |

**AI の動き方**:

1. 検証開始時に「これから backend と backcast を起動します。起動後に Load... → Run の操作をお願いします」と先に宣言する
2. 自分で実行できる手順（起動・ログ確認）を **黙って勝手にやらない**。ユーザーに見えるように一言告げてから走らせる
3. UI 操作が必要になった時点で **明示的に依頼する**。「Run ボタンを押してください。フッターが `state: RUNNING` → `IDLE` に戻ったら教えてください」のように、**観測してほしいもの** を一緒に伝える
4. ユーザーが「動かない」「変な表示が出た」と言ってきたら、**まずログ** を読む。憶測しない

これを守らないと、AI が backend を起動したまま放置して port を専有したり、ユーザーが UI で何をすべきか分からず止まったりする。

---

## 1. アーキテクチャ（最小モデル）

```
[ Rust GUI: backcast.exe (Bevy 0.15) ]
       │ gRPC client (BACKEND_TOKEN で認証)
       ▼
[ Python backend: `python -m engine` ]  on tcp://127.0.0.1:19876
       │
       ├─ NautilusRunner (リプレイ実行)
       └─ ParquetDataCatalog (artifacts/jquants-catalog)
```

- **GUI のフッター** が真実のソース。`state: <IDLE|LOADED|RUNNING>  grpc: <OK|DISABLED>` が表示される
- gRPC port: **19876**（旧 e-station の WebSocket IPC とは別物。混同しないこと）
- token は `.env` の `BACKEND_TOKEN`（既定 `testtoken`）。両プロセスで一致が必須
- catalog path は `{ARTIFACTS_PATH}/jquants-catalog`（`ARTIFACTS_PATH` env var から自動構築、デフォルト: `{cwd}/artifacts`）

---

## 2. 起動手順（AI が実行）

詳細は [docs/strategy-replay.md](../../../docs/strategy-replay.md) §「Bevy GUI でのリプレイ実行」が一次資料。短縮版を以下に置く。

### 2.1 port 競合チェック → 既存プロセス kill（PowerShell）

```powershell
$p = (Get-NetTCPConnection -LocalPort 19876 -ErrorAction SilentlyContinue).OwningProcess
if ($p) { Stop-Process -Id $p -Force }
```

### 2.2 backend 起動（ログを `$env:TEMP\backend_log.txt` にリダイレクト）

```powershell
$env:RUST_LOG = "info"
# `engine_pb2_grpc.py` が `import engine_pb2` の flat import なので、proto ディレクトリを
# PYTHONPATH に通さないと `ModuleNotFoundError: No module named 'engine_pb2'` で即死する。
$env:PYTHONPATH = "$PWD\python\engine\proto"
Start-Process -FilePath "uv" `
  -ArgumentList "run","python","-m","engine","--token","testtoken","--jquants-catalog-path","artifacts\jquants-catalog" `
  -RedirectStandardOutput "$env:TEMP\backend_log.txt" `
  -RedirectStandardError  "$env:TEMP\backend_err.txt" `
  -WindowStyle Hidden
```

`Starting gRPC server on port 19876` が `$env:TEMP\backend_log.txt` に出れば OK。

> ⚠️ `python -m engine.server_grpc` ではなく **`python -m engine`**。前者には `__main__` が無く即エラー。
>
> ⚠️ **`PYTHONPATH` 必須**。`backend_err.txt` に `ModuleNotFoundError: No module named 'engine_pb2'` が出たら `$env:PYTHONPATH` を忘れている。proto 再生成で根治する余地もあるが、現状の `engine_pb2_grpc.py` は `import engine_pb2` flat import で生成されているため、環境変数で吸収するのが最速。

### 2.3 Rust GUI 起動（`.env` は読まれないので env を **明示的に** 渡す）

ログをファイルに残したいなら **`Start-Process` の `-RedirectStandardOutput` / `-RedirectStandardError` を使う**。これは OS レベルのファイルリダイレクトなので、ツール呼び出しが終わってもプロセスが回り続ける限りログが書かれ続ける。env を明示渡しする必要があるときも `Start-Process` の前に `$env:XXX` をセットすれば child に継承される。

```powershell
$env:RUST_LOG = "info"
$env:BACKEND_ENABLED = "true"
$env:BACKEND_TOKEN = "testtoken"
$env:ARTIFACTS_PATH = $PWD.Path + "\artifacts"
# Bevy AssetServer は exe parent (`target/debug/assets/`) を見るが、本プロジェクトの
# assets は repo root の `assets/` にあるため、BEVY_ASSET_ROOT で明示的に向け直す。
# 未設定だとフッターの ▶/■ シンボルフォント (NotoSansSymbols2) と grid.wgsl が
# `Path not found` で読めず、フッターのボタンが空白になる。
$env:BEVY_ASSET_ROOT = $PWD.Path
$p = Start-Process -FilePath ".\target\debug\backcast.exe" -WorkingDirectory $PWD.Path `
  -RedirectStandardOutput "$env:TEMP\backcast_log.txt" `
  -RedirectStandardError  "$env:TEMP\backcast_err.txt" -PassThru
```

> ⚠️ **bevy / tracing のログは stdout ではなく stderr に出る**。`render_texture` 等に仕込んだ `info!` を読むときは `backcast_err.txt`（`-RedirectStandardError` 側）を見ること。`backcast_log.txt` は空のことが多い。
>
> ⚠️ **`ProcessStartInfo` + `Register-ObjectEvent` で OutputDataReceived を拾う方式は使うな**。PowerShell のイベントサブスクリプションはツール呼び出しのセッションが終わると死ぬため、起動直後の数秒しかログが取れない。必ず `Start-Process` の `-Redirect*` でファイルに直接書かせる。

> ⚠️ **絶対に避けること**:
> - `cargo run` 単体 → `.env` が読まれず `grpc: DISABLED` になる
> - `$env:BACKEND_ENABLED="true"; cargo run` → 同上（child に伝播しない）
> - WSL / Git Bash 経由起動 → Bevy が早期終了する。必ず PowerShell から

### 2.4 ユーザーに目視確認を依頼

> 「フッター（画面右下）に `state: IDLE  grpc: OK` と出ていますか？」

- `grpc: DISABLED` → `BACKEND_ENABLED` が child に渡っていない。2.3 をやり直す
- `state: RUNNING` で始まる → `python/engine/__main__.py` の `auto_start` が `True` になっている。`False` に直して backend を再起動

---

## 3. 検証フロー（典型）

例: 「Load... → Run でリプレイが完走するか」

| # | 主体 | 操作 |
|---|---|---|
| 1 | AI | port kill → backend 起動 → ログで `Starting gRPC server on port 19876` 確認 |
| 2 | AI | Rust GUI 起動 |
| 3 | AI → ユーザー | 「フッター `state: IDLE  grpc: OK` を確認してください」 |
| 4 | ユーザー | `Load...` → `python/tests/data/test_strategy_daily.py` を選択 |
| 5 | ユーザー | Strategy Editor で `Run` をクリック |
| 6 | ユーザー → AI | 「`state: RUNNING` → `IDLE` に戻り、Run Result Panel に `Completed` が出ました」と報告 |
| 7 | AI | `cat $env:TEMP\backend_log.txt` で `StartEngine: run complete run_id=...` を確認 |
| 8 | AI | `ls "$env:APPDATA\flowsurface\run-buffer\<run_id>"` で `meta.json/fills.jsonl/equity.jsonl/summary.json` を確認 |

---

## 4. ログ確認チートシート（AI 用）

```powershell
# backend stdout
Get-Content "$env:TEMP\backend_log.txt" -Tail 80

# backend stderr（例外はこちら）
Get-Content "$env:TEMP\backend_err.txt" -Tail 80

# run-buffer 出力
Get-ChildItem "$env:APPDATA\flowsurface\run-buffer\" | Sort-Object LastWriteTime -Descending | Select-Object -First 3
```

期待ログ（成功時、この順で出る）:

```text
LoadReplayData success=True state=LOADED
StartEngine: strategy loaded cls='BuyAndHoldStrategy' instruments=['1301.TSE']
StartEngine: bars loaded total=57
StartEngine: run complete run_id=<ts>-...-1301_TSE summary={...}
StartEngine success=True state=RUNNING
```

---

## 5. 後片付け（AI が実行）

検証セッション終了時は必ずプロセスを落とす。port を専有したまま放置すると次回起動時に競合する。

```powershell
# backcast を kill
Get-Process backcast -ErrorAction SilentlyContinue | Stop-Process -Force
# backend (port 19876) を kill
$p = (Get-NetTCPConnection -LocalPort 19876 -ErrorAction SilentlyContinue).OwningProcess
if ($p) { Stop-Process -Id $p -Force }
```

---

## 6. 自動テスト（手動検証 **ではない** もの）

GUI を立ち上げない pytest / cargo test は AI が普通に走らせて良い。

```powershell
# Python: backend ユニット + gRPC ルート + Nautilus runner
uv run pytest python/tests/ -v

# Rust: backend 統合
cargo test --test backend_integration
```

ヘッドレスでリプレイだけ走らせたいなら `scripts/run_replay.ps1` ラッパーが最速。GUI は不要。

```powershell
.\scripts\run_replay.ps1 -Strategy python\tests\data\test_strategy_daily.py
```

詳細は [docs/strategy-replay.md](../../../docs/strategy-replay.md)。

---

## 7. よくある詰まり

| 症状 | 原因 | 対処 |
|---|---|---|
| ExecutionMode トグルで Manual/Auto に切り替わらない・クリック無反応に見える | `execution_mode_toggle_system` (footer.rs) の既存 guard。`VenueState != Connected` のとき `WARN ExecutionMode→Live blocked: venue not connected (state=Disconnected)` を吐いて切替を拒否する | 立花/kabusapi など venue を接続するか、`apply_execution_mode_visibility_system` 系の挙動だけ確認したいなら `cargo test ui::` で代替（venue 接続なしでは Live モードに遷移できない） |
| `grpc: DISABLED` のまま | `BACKEND_ENABLED` が GUI プロセスに渡っていない | `ProcessStartInfo.EnvironmentVariables` で明示渡し（§2.3） |
| 起動直後から `state: RUNNING` | backend 側 `auto_start=True` | `python/engine/__main__.py` を `auto_start=False` |
| Run ボタン無反応 | `grpc: DISABLED` / dirty buffer（`cached *` 表示） | backend 起動状態確認 + Save Cache を押してから Run |
| blank Strategy Editor を spawn 後に Run が `state: IDLE` のまま変わらない | `panel_spawn_dispatcher_system` が blank spawn 時に `buffer.original_path = None` を書き込み → `parse_scenario_system` が ScenarioMetadata をリセット → instruments 空になって Run blocked。ログに `Run blocked: SCENARIO has no instruments` が出る | `floating_window.rs` の blank spawn ハンドラで `original_path.is_none() && cache_path.is_none()` 条件チェックが入っているか確認。**Phase 7.x-sasa で修正済み**（2026-05-16） |
| Run ボタン連打で backend が INVALID_STATE | 複数 tokio タスクが並列起動し `LoadReplayData is only allowed from IDLE` エラー | **修正済み**（`RunState::Running` 中は Run ボタン disabled に）。ログで `INVALID_STATE` が出たら GUI 側の guard が機能していない可能性 |
| Run Result パネルに何も出ない（run は実際には成功している） | サイドバーで「Run Result」を選択していない / `run_id` または `summary_json` が None で `RunComplete` が送信されない | ① Run Result パネルを選択して確認 ② `backcast_err.txt` で `RunComplete:` ログを検索 |
| candle が出ない | `KlineUpdate` に `open_time_ms` が無い | `python/engine/core.py` を確認 |
| port 19876 が掴めない | 前回の backend がゾンビ | §2.1 で kill |
| `python -m engine.server_grpc` でエラー | `__main__` 不在 | `python -m engine` を使う |
| backcast 再起動後に "No instruments" + 警告「This sidecar uses 'instruments_ref'」が出るが、cache JSON に `instruments_ref` が無い | `InstrumentRegistry::default()` が `editable=false`。cache restore は `ScenarioLoadedFromFile` event を発火しないので registry が default に張り付き、warning が誤発火（Phase 7.5a Issue B） | 手動で `File → Load... → <strategy>.py` 再 Load で state 再確立。**Phase 7.5c で fix 予定** |
| File→Load で cache `app_state.json` の windows / strategy_path / 追加した instruments が消える | `sync_to_cache → copy_sidecar_to_cache` が cache_sidecar を**無条件削除**し、original sidecar の bare 内容で上書き（Phase 7.5a Issue A、`menu_bar.rs:642-669`） | 仕様としては「Load は fresh start」だが、検証中に状態が縮退する。**Phase 7.5c で fix 予定** |
| picker `[+ Add]` で "Loading..." 永久ループ（backend dead 時） | transport task が connect retry loop 中、queued FetchAvailableInstruments を処理しない。preflight 無いと `in_flight` 残る | **修正済み**（Phase 7.5b §5.4 項目 7、`add_instrument_button_system` に `BackendStatus` preflight）。`Error: backend not connected` が picker に出れば OK |
| picker close → 再 open で list がブランク | `picker_list_rebuild_system` が cache hit 経路で `is_changed()` 反応せず、新 container が空のまま | **修正済み**（Phase 7.5b §5.4 項目 5-a、`Added<InstrumentPickerListContainer>` を trigger に追加） |
| catalog に instrument データ無く `ListAllListedSymbols` が 1 件しか返さない | `artifacts/jquants-catalog/data/bar/` 配下が少銘柄のみ | `scripts/build_catalog_batch.py` を `BASE_DIR=S:\j-quants` / `UNIVERSE_JSONS=C:\Users\sasai\Documents\🐃_blacksheep\data\universe\v05_B_top100_*.json` で実行 → 302 銘柄分 build。**`CATALOG` 定数は環境別 absolute path なので注意** |

---

## 8. やってはいけないこと（過去の遺物）

このリポジトリは過去 e-station と呼ばれていた頃の名残があり、混同すると即誤動作する。**触らないこと**:

- `cargo run -- --mode replay` ← **存在しない**（e-station 時代の起動方法）
- `engine.replay_session.ReplaySession` / `LiveSession` ← **存在しない**（旧 helper class）
- WebSocket IPC `:19876` / `engine-session.json` / `FLOWSURFACE_ENGINE_TOKEN` ← **すべて旧アーキ**。今は gRPC。port 番号だけ偶然同じ
- HTTP API `/api/replay/*` / `/api/order/*` ← Phase 8.3 で全廃
- `scripts/run-replay-debug.sh` / `scripts/replay_dev_load.sh` ← 機能しない残骸
- Playwright / ブラウザ自動操作 ← GUI が Bevy（ネイティブ）なので不可

これらが書かれた古いコメントや過去スキルを真に受けると、Claude は即詰まる。**常に「Bevy + gRPC + backcast.exe」が正** と覚える。

---

## 9. 一次資料

- [docs/strategy-replay.md](../../../docs/strategy-replay.md) — 起動手順の完全版
- [docs/plan/Phase 7 - Replay UI Integration.md](../../../docs/plan/Phase%207%20-%20Replay%20UI%20Integration.md) §7 — 役割分担と検証手順の出典
- [docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](../../../docs/plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md) — runtime 仕様
