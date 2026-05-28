---
name: e2e-testing
description: The-Trader-Was-Replaced（Bevy + gRPC backend）の E2E 手動検証パターン。`backcast.exe`（Rust/Bevy GUI）と `python -m engine`（gRPC backend, port 19876）を起動し、AI とユーザーで役割分担しながら検証する。「E2E」「手動検証」「動作確認」「アプリの起動はどうやる」「起動方法」「どうやって起動」「VSCode から起動」「F5 で起動」「デバッグ起動」「launch.json」「tasks.json」「起動引数」「起動の env が足りない」「BACKEND_ENABLED」「BEVY_ASSET_ROOT」「backcast を起動」「backend を起動」「Run ボタン」「Strategy を動かしたい」「gRPC: OK にならない」「リプレイを実機で確認」「レイアウトをロード」「パネルが復活するか確認」「Save/Load のテスト」「Ctrl+O で開く」「Ctrl+S で保存」「× ボタンを確認」「パネルが閉じるか確認」「再起動して確認」「自動 Load を確認」「オートオープン」「Undo/Redo を確認」「Ctrl+Z が動くか確認」「キーボードショートカットを実機で確認」「undo が効いているか確認」「redo を試したい」「cache sidecar を確認」「scenario キーが残っているか確認」「autosave 後に JSON を確認」「sidecar が壊れないか確認」「Load... で JSON がコピーされるか」「メニューバーを確認」「ドロップダウンが開くか」「Alt+F が動くか」「Alt+E が動くか」「メニューショートカットを確認」「File メニュー」「Edit メニュー」「マルチ spawn を確認」「複数エディタを確認」「再起動して復元されるか」「region が復元されるか」「エディタ窓が戻るか」「spawn 後に Run が動くか」「ペア保存を確認」「.py と .json が同時に保存されるか」「保存後のタイムスタンプを確認」「メニューの項目が正しいか確認」「manual-gate」「live venue を確認」「TACHIBANA に接続して確認」「板データが表示されるか確認」「picker に何件出るか確認」と言われたら必ず起動する。本スキルは **GUI を立ち上げる手動目視検証** 専用。⚠️ **screencapture 制限**: macOS では terminal に Screen Recording 権限が無いと `screencapture` コマンドが Bevy Metal ウィンドウをキャプチャできない（デスクトップ背景のみ写る）。AI 単独では目視確認不可 — 必ずユーザーにスクリーンショット貼付を依頼すること。⚠️ **市場時間**: TACHIBANA live の depth data（板）確認は TSE 立会時間（9:00-11:30 / 12:30-15:30 JST）内でないと「No depth data」が表示されるが、これは正常挙動（Bug ではない）。`cargo test --test e2e_replay` のような **ヘッドレス自動 E2E（backend→ECS seam を resource で assert する Rust テスト）/ `tests/e2e/FLOWS.md` の flow 追加** は本スキルではなく `rust-testing` を使う。Playwright / HTTP API（旧 :9876）/ WebSocket IPC は **使わない**（過去の e-station アーキテクチャの残骸）。Phase 7.6 以降: レイアウト JSON に `strategy_path` フィールドが追加済み。Ctrl+O ロード時に Strategy Editor のファイルも自動復元される（`PendingStrategyLoad` 経由）。Phase 7.7 以降: 起動時オートオープン（7B）・サイドカー自動 Load（7C）・デバウンス自動保存（7D）・タイトルバー × ボタン（7Z）が追加。Phase 7.1: Undo/Redo キーバインド（Ctrl+Z/Y/Shift+Z）が追加。Phase 7.3: scenario sidecar 移行済み。cache に `<hash>__<strategy>.json` が同梱コピーされる（T1E）。layout autosave 後も `scenario` キーが保持される（T1D）。Phase 7.1 追加分: メニューバーをドロップダウン式（File/Edit）に変更、Alt+F/Alt+E でドロップダウン展開。"Open Strategy..." ボタン削除。Edit メニューに Undo(Ctrl+Z)/Redo(Ctrl+Y) 追加。Alt+F/E 時は `Events<KeyboardInput>.clear()` で bevy_cosmic_edit への文字書き込みを防止済み。Phase 7.x: File メニューが Save Layout / Save Layout As / Load Layout の3項目に整理。Ctrl+S で .json と .py を同時保存（ペア保存）。保存成功確認は `backcast_err.txt` で `layout saved to` と `strategy .py saved to` が連続して出ることで確認。
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

> ⚠️ **本セクションは PowerShell/Windows 表記だが、開発機が macOS (darwin) のことがある**。その場合は下表に読み替える。コマンド本体・env 名・port は同じ。`backcast.exe` → `backcast`（拡張子なし）。
>
> | やること | Windows (PowerShell) | macOS / Linux (bash/zsh) |
> |---|---|---|
> | port 19876 の PID | `(Get-NetTCPConnection -LocalPort 19876).OwningProcess` | `lsof -ti tcp:19876` |
> | プロセス kill | `Stop-Process -Id $p -Force` / `Get-Process backcast \| Stop-Process` | `kill -9 $(lsof -ti tcp:19876)` / `pkill -x backcast` |
> | backend 起動 (bg + log) | `Start-Process uv -ArgumentList ... -RedirectStandardOutput ...` | `PYTHONPATH=$PWD/python/engine/proto uv run python -m engine --token testtoken ... > /tmp/backend_log.txt 2>&1 &` |
> | GUI 起動 (bg + log) | `Start-Process .\target\debug\backcast.exe -Redirect*` | `BEVY_ASSET_ROOT=$PWD BACKEND_ENABLED=true BACKEND_TOKEN=testtoken RUST_LOG=info ./target/debug/backcast > /tmp/backcast_log.txt 2> /tmp/backcast_err.txt` (Claude Code なら `run_in_background:true` で起動し、終了時に通知が来る) |
> | log 確認 | `Get-Content $env:TEMP\backcast_err.txt -Tail 80` | `tail -80 /tmp/backcast_err.txt` |
> | run-buffer 出力 | `$env:APPDATA\flowsurface\run-buffer\` | `~/Library/Application Support/flowsurface/run-buffer/` (cache は `~/Library/Caches/the-trader-was-replaced/`) |
>
> ⚠️ macOS でも **bevy/tracing ログは stderr** (`backcast_err.txt`)。env は **コマンド前置き** (`BEVY_ASSET_ROOT=$PWD ... ./target/debug/backcast`) で child に渡す（`export` 不要）。Find/Replace・gutter 等の **backend 不要な UI 検証では backend を起動しなくてよい**（フッターは `grpc: DISABLED` で正常）。

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

### 2.5 VSCode `.vscode/launch.json` から起動（F5）

手動で 2.2/2.3 を打たず、VSCode の Run & Debug（F5）で起動したいとき。`.vscode/launch.json` + `tasks.json` は既存（CodeLLDB 拡張 `vadimcn.vscode-lldb` 必須）。**env は launch.json の `env` で明示渡し**する（`.env` は `envFile` で別途読まれるが、`BACKEND_ENABLED` / `BEVY_ASSET_ROOT` は `.env` に無いので `env` に書く）。

2 つの起動戦略があり、どちらも `env` に最低限 `BACKEND_ENABLED=true` / `BACKEND_TOKEN=testtoken` / `BEVY_ASSET_ROOT=${workspaceFolder}` / `ARTIFACTS_PATH=${workspaceFolder}/artifacts` が要る:

- **autospawn（推奨・単一構成）**: `BACKEND_AUTOSPAWN=true` + `PYTHON_BIN=${workspaceFolder}/.venv/bin/python` を足すと、Rust の `backend_supervisor` が `python -m engine --token <t> --port <p>`（`PYTHONPATH=<cwd>/python`, `BACKEND_SUPERVISED=1`）を自前 spawn し、終了時に道連れで kill する。別タスク不要・port kill レース無し。`preLaunchTask` は `cargo build` だけでよい。
- **external（別プロセス）**: backend を `tasks.json` の `start backend (replay)`（`uv run python -m engine ...`）で別ターミナルに起動し、`postDebugTask` で `stop backend` する。backend ログを独立して追いたいとき。

> ⚠️ launch.json の `args` に `--mode replay` / `--engine-cmd` を書かない。**ソースに該当パーサが無く黙って無視される e-station 時代の遺物**（§8）。`args: []` でよい。
>
> ⚠️ `BEVY_ASSET_ROOT` を忘れるとフッターの ▶/■ シンボルフォント・`grid.wgsl` が読めず UI が崩れる（2.3 と同じ罠が F5 でも起きる）。
>
> 検証コマンド（headless smoke）: autospawn の env を前置きして `./target/debug/backcast` を起動 → 1〜2 秒で `lsof -ti tcp:19876` が listen、`backcast_err.txt` に `[backend] Starting gRPC server on port 19876` + `idle gRPC shutdown disabled (BACKEND_SUPERVISED=1)` が出れば供給経路 OK。

### 2.6 Zed `.zed/debug.json` から起動（F5）

⚠️ **開発機が Zed のことがある（`base_keymap: VSCode` でも Zed は VSCode ではない）**。「F5 で起動したら `grpc: DISABLED`」と言われたら、まず **エディタが Zed か VSCode か** を確認する。Zed は VSCode と F5 の挙動が違う:

- Zed は `.vscode/launch.json` を **読む** が、自前の `DebugScenario` に変換して **ピッカーの候補に並べるだけ**。VSCode のように「F5 = launch.json 先頭構成を起動」ではない。**F5 一発だと Zed が Cargo から自動生成した "Debug backcast" シナリオ（env 無し）を掴みがち** → `BACKEND_ENABLED` が渡らず `grpc: DISABLED` になる。これが Zed での典型症状。
- 変換は `type: "lldb"`→`CodeLLDB`、`env`/`program`/`args`/`cwd` は引き継ぐが **`preLaunchTask` は未対応**（Zed ソース `crates/task/src/vscode_debug_format.rs` 冒頭に `// TODO support preLaunchTask linkage`）。`${workspaceFolder}`→`$ZED_WORKTREE_ROOT` に置換される。
- **確実な解は `.zed/debug.json` を置く**（Zed ネイティブで一級市民・既存）。autospawn 構成を移植し、`preLaunchTask` の代わりに `build` フィールドで `cargo build` させる:

```json
[
  {
    "label": "backcast: autospawn backend (CodeLLDB)",
    "adapter": "CodeLLDB",
    "request": "launch",
    "program": "$ZED_WORKTREE_ROOT/target/debug/backcast",
    "args": [],
    "cwd": "$ZED_WORKTREE_ROOT",
    "env": {
      "PATH": "$ZED_WORKTREE_ROOT/.venv/bin:$PATH",
      "BACKEND_ENABLED": "true",
      "BACKEND_AUTOSPAWN": "true",
      "PYTHON_BIN": "$ZED_WORKTREE_ROOT/.venv/bin/python",
      "BACKEND_TOKEN": "testtoken",
      "ARTIFACTS_PATH": "$ZED_WORKTREE_ROOT/artifacts",
      "BEVY_ASSET_ROOT": "$ZED_WORKTREE_ROOT",
      "RUST_LOG": "info"
    },
    "build": { "command": "cargo", "args": ["build"] }
  }
]
```

> F5 → ピッカーで **"backcast: autospawn backend (CodeLLDB)"** を選択。一度選べば以降の F5 は同じシナリオを再実行。`grpc: DISABLED` のままなら Cargo 自動生成シナリオを掴んでいるので選び直す。CodeLLDB アダプタ拡張は Zed が自動取得する。
>
> ⚠️ **autospawn は起動時 19876 を probe し、応答があれば attach（新規 spawn しない）**。デバッグ停止が SIGKILL 系だと前回の python child が孤児化（PPID=1）して 19876 を握り続け、次回 F5 が**孤児に attach**する。孤児が `--live-venue` 無しだと live venue ログインが `LIVE_ADAPTER_NOT_CONFIGURED` で死ぬ（§7 参照）。Zed の `build` は `cargo build` しか走らせず `preLaunchTask` 非対応なので port kill が挟まらない。**恒久対策**: `.zed/debug.json` の `build.command` を kill+build ラッパーにする — `"build": { "command": "bash", "args": ["-c", "lsof -ti tcp:19876 | xargs -r kill -9; cargo build"] }`。

---

## 3. 検証フロー（典型）

例: 「Load... → Run でリプレイが完走するか」

| # | 主体 | 操作 |
|---|---|---|
| 1 | AI | port kill → backend 起動 → ログで `Starting gRPC server on port 19876` 確認 |
| 2 | AI | Rust GUI 起動 |
| 3 | AI → ユーザー | 「フッター `state: IDLE  grpc: OK` を確認してください」 |
| 4 | ユーザー | `Load...` → `examples/test_strategy_daily.py` を選択 |
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

> ⚠️ **macOS で backend を kill するとき `kill -9 $(lsof -ti tcp:19876)` は GUI も道連れにする**。`lsof -ti tcp:19876` はそのポートに socket を持つ全 PID を返すので、**listener（backend）だけでなく、接続中の backcast クライアントの PID も含まれる**。両方 SIGKILL されて GUI が落ちる。backend だけ残して落としたいときは **起動時に控えた backend の PID を直接 kill**（`backend launching (pid $!)` を覚えておく）するか、`pkill -f "python -m engine"` を使う。GUI は `pkill -x backcast`。port ベースで kill してよいのは「全部まとめて落とす後片付け」のときだけ。

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
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

詳細は [docs/strategy-replay.md](../../../docs/strategy-replay.md)。

---

## 7. よくある詰まり

| 症状 | 原因 | 対処 |
|---|---|---|
| ExecutionMode トグルで Manual/Auto に切り替わらない・クリック無反応に見える | `execution_mode_toggle_system` (footer.rs) の既存 guard。`VenueState != Connected` のとき `WARN ExecutionMode→Live blocked: venue not connected (state=Disconnected)` を吐いて切替を拒否する | 立花/kabusapi など venue を接続するか、`apply_execution_mode_visibility_system` 系の挙動だけ確認したいなら `cargo test ui::` で代替（venue 接続なしでは Live モードに遷移できない） |
| `grpc: DISABLED` のまま | `BACKEND_ENABLED` が GUI プロセスに渡っていない | `ProcessStartInfo.EnvironmentVariables` で明示渡し（§2.3）。**Zed の F5 なら §2.6**（launch.json が効かず Cargo 自動シナリオを掴んでいる → `.zed/debug.json`） |
| 起動直後から `state: RUNNING` | backend 側 `auto_start=True` | `python/engine/__main__.py` を `auto_start=False` |
| Run ボタン無反応 | `grpc: DISABLED` / dirty buffer（`cached *` 表示） | backend 起動状態確認 + Save Cache を押してから Run |
| blank Strategy Editor を spawn 後に Run が `state: IDLE` のまま変わらない | `panel_spawn_dispatcher_system` が blank spawn 時に `buffer.original_path = None` を書き込み → `parse_scenario_system` が ScenarioMetadata をリセット → instruments 空になって Run blocked。ログに `Run blocked: SCENARIO has no instruments` が出る | `floating_window.rs` の blank spawn ハンドラで `original_path.is_none() && cache_path.is_none()` 条件チェックが入っているか確認。**Phase 7.x-sasa で修正済み**（2026-05-16） |
| Run ボタン連打で backend が INVALID_STATE | 複数 tokio タスクが並列起動し `LoadReplayData is only allowed from IDLE` エラー | **修正済み**（`RunState::Running` 中は Run ボタン disabled に）。ログで `INVALID_STATE` が出たら GUI 側の guard が機能していない可能性 |
| Run Result パネルに何も出ない（run は実際には成功している） | サイドバーで「Run Result」を選択していない / `run_id` または `summary_json` が None で `RunComplete` が送信されない | ① Run Result パネルを選択して確認 ② `backcast_err.txt` で `RunComplete:` ログを検索 |
| candle が出ない | `KlineUpdate` に `open_time_ms` が無い | `python/engine/core.py` を確認 |
| port 19876 が掴めない | 前回の backend がゾンビ | §2.1 で kill |
| live venue で `Connect Tachibana/Kabu` しても**無反応・ダイアログが出ない**（`grpc: OK` なのに） | autospawn supervisor は起動時に 19876 を probe し、**応答があれば attach（新規 spawn しない）**（`backend_supervisor.rs` probe→attach 経路）。前回の Zed/F5 セッションを SIGKILL 系で止めると python child が**孤児化**（PPID=1）して 19876 を握り続ける。それが `--live-venue` **無し**で起動された非 live backend だと、`LIVE_VENUE` env を無視して attach → `_live_adapter_factory=None` → `VenueLogin` が `LIVE_ADAPTER_NOT_CONFIGURED` で即 reject → tkinter ダイアログ生成の前段で終了。`.env`・`LIVE_VENUE` が正しくても起きる | `ps -p $(lsof -ti tcp:19876) -o command` で 19876 の cmdline を確認。**`--live-venue` が無ければ孤児**なので §2.1 で kill → live シナリオを再起動すると `--live-venue TACHIBANA` 付きで spawn される。ログで `VenueLogin rejected: error_code=LIVE_ADAPTER_NOT_CONFIGURED` が出ていれば確定（出ず `menu: Venue→Connect requested` の後に何も無いなら attach 先が無効） |
| `python -m engine.server_grpc` でエラー | `__main__` 不在 | `python -m engine` を使う |
| backcast 再起動後に "No instruments" + 警告「This sidecar uses 'instruments_ref'」が出るが、cache JSON に `instruments_ref` が無い | `InstrumentRegistry::default()` が `editable=false`。cache restore は `ScenarioLoadedFromFile` event を発火しないので registry が default に張り付き、warning が誤発火（Phase 7.5a Issue B） | 手動で `File → Load... → <strategy>.py` 再 Load で state 再確立。**Phase 7.5c で fix 予定** |
| File→Load で cache `app_state.json` の windows / strategy_path / 追加した instruments が消える | `sync_to_cache → copy_sidecar_to_cache` が cache_sidecar を**無条件削除**し、original sidecar の bare 内容で上書き（Phase 7.5a Issue A、`menu_bar.rs:642-669`） | 仕様としては「Load は fresh start」だが、検証中に状態が縮退する。**Phase 7.5c で fix 予定** |
| picker `[+ Add]` で "Loading..." 永久ループ（backend dead 時） | transport task が connect retry loop 中、queued FetchAvailableInstruments を処理しない。preflight 無いと `in_flight` 残る | **修正済み**（Phase 7.5b §5.4 項目 7、`add_instrument_button_system` に `BackendStatus` preflight）。`Error: backend not connected` が picker に出れば OK |
| picker `[+ Add]` で "Loading..." 永久ループ（**`grpc: OK`・backend 健全なのに**） | `FetchAvailableInstruments` が reconnect エッジで `flush_stale_transport_commands` に破棄され backend に届かない（H1）。`in_flight` が stuck → `[+ Add]` 再押下も dedup guard で no-op。`auto ListInstruments`（`main.rs:490` 直接 fire）は届くので非対称になる | **修正済み — GH #53**（`is_reconcile_command` に `FetchAvailableInstruments` 追加で flush 保持。reconnect エッジで stuck in_flight をクリア＆再送）。切り分け: backend ログに `ListAllListedSymbols` が出るか確認 |
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
