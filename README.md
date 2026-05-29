# The Trader Was Replaced

Nautilus Trader ベースの戦略リプレイ・評価エンジン。

## 起動方法

### GUI モード（In-proc）— **推奨**

`BACKEND_TRANSPORT=inproc` を設定すると Python エンジンを Rust プロセスに**直接埋め込み**、
gRPC バックエンドプロセスが不要になる。コマンド 1 本で完結する。

```powershell
.\scripts\run_inproc.ps1
# artifacts を別ドライブに置く場合:
.\scripts\run_inproc.ps1 -ArtifactsPath S:\artifacts
```

スクリプトは `__pycache__` 削除・環境変数設定・GUI 起動を一括実行する。

#### ビルド前提（初回のみ）

```powershell
uv venv                              # .venv 作成
$env:PYO3_PYTHON = "$PWD\.venv\Scripts\python.exe"
cargo build
```

> `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` は `.cargo/config.toml` で自動設定済み。

#### 起動後のログ（正常）

```
[inproc] Python worker thread starting
[inproc] DataEngine initialized
[inproc] RustEventSink registered on DataEngine
[inproc] InprocLiveServer initialized (live_venue_id=None)
```

> **Windows WinError 6714**: `__pycache__` が存在すると Python の `FileFinder` がディレクトリを
> 再スキャンし、TxF フィルタードライバ (Windows Defender 等) に引っかかる。
> `run_inproc.ps1` が起動前に `__pycache__` を自動削除する。
> `sys.dont_write_bytecode=True` が Rust 側で自動設定されるため削除後は再作成されない。

---

### ヘッドレスリプレイ（GUI なし）

Python のみで戦略バックテストを実行する。Bevy GUI は不要。

→ **[docs/strategy-replay.md](docs/strategy-replay.md)**

```powershell
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

---

### GUI モード（gRPC）— レガシー

Python バックエンドを別プロセスで起動する旧来の方式。デバッグ時など backend ログを
独立して確認したいケースに使う。通常は In-proc モードを使うこと。

→ **[python/README.md](python/README.md)**

#### 1. Python backend を起動

```powershell
# .env を PowerShell 環境変数にロード
Get-Content .env | Where-Object { $_ -match '^\s*[^#=]+=.' } | ForEach-Object {
    $k, $v = $_ -split '=', 2
    [System.Environment]::SetEnvironmentVariable($k.Trim(), $v.Trim().Trim('"'), 'Process')
}

# backend 起動（catalog path は ARTIFACTS_PATH から自動構築）
$env:PYTHONPATH = "$PWD\python\engine\proto"
Start-Process -FilePath "uv" `
  -ArgumentList "run","python","-m","engine",
                "--token",$env:BACKEND_TOKEN,
                "--jquants-catalog-path","$env:ARTIFACTS_PATH\jquants-catalog" `
  -RedirectStandardOutput "$env:TEMP\backend_log.txt" `
  -RedirectStandardError  "$env:TEMP\backend_err.txt" `
  -WindowStyle Hidden
```

> `ARTIFACTS_PATH` が `.env` に無い場合のデフォルト: `.\artifacts`。
> j-quants catalog のビルド方法は `scripts/build_catalog_batch.py` を参照。

#### 2. Rust GUI を起動（backend 起動後）

```powershell
$env:BEVY_ASSET_ROOT = $PWD.Path
$env:ARTIFACTS_PATH  = $env:ARTIFACTS_PATH   # .env から継承
$env:BACKEND_ENABLED = "true"
$env:BACKEND_TOKEN   = $env:BACKEND_TOKEN
$env:RUST_LOG        = "info"
Start-Process -FilePath ".\target\debug\backcast.exe" -WorkingDirectory $PWD.Path `
  -RedirectStandardOutput "$env:TEMP\backcast_log.txt" `
  -RedirectStandardError  "$env:TEMP\backcast_err.txt" -PassThru
```

---

## In-proc ビルド詳細

| 項目 | 内容 |
|---|---|
| pyo3 バージョン | 0.22（Python 3.13 まで正式サポート） |
| 動作確認済み Python | 3.13 / 3.14（ABI3 前方互換モード） |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | `.cargo/config.toml` で自動設定済み |
| `PYO3_PYTHON`（ビルド時） | `.venv\Scripts\python.exe` を指定 |
| 注意 | pyo3 を 0.23+ にアップグレードすれば ABI3 フラグ不要（issue #64 フォロータスク②） |

---

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/strategy-replay.md](docs/strategy-replay.md) | 戦略リプレイの起動手順・CLI オプション |
| [docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](docs/plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md) | Strategy Runtime 実装仕様 |
| [python/README.md](python/README.md) | Python バックエンドのセットアップ・テスト |
