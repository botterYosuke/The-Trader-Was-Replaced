# The Trader Was Replaced

Nautilus Trader ベースの戦略リプレイ・評価エンジン。

## 起動方法

### 戦略リプレイ

→ **[docs/strategy-replay.md](docs/strategy-replay.md)**

```powershell
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

### Python バックエンド（gRPC）

→ **[python/README.md](python/README.md)**

`.env` の `ARTIFACTS_PATH` と `BACKEND_TOKEN` を読み込んでから起動する。

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

> `ARTIFACTS_PATH` が `.env` に無い場合のデフォルト: `.\artifacts`（リポジトリ内）。
> j-quants catalog のビルド方法は `scripts/build_catalog_batch.py` を参照。

### Rust GUI（backcast.exe）

backend 起動後に実行する。`.env` は GUI に自動ロードされないため、env を明示渡しする。

```powershell
# .env ロード（上記コマンドを先に実行済みの場合はスキップ可）
Get-Content .env | Where-Object { $_ -match '^\s*[^#=]+=.' } | ForEach-Object {
    $k, $v = $_ -split '=', 2
    [System.Environment]::SetEnvironmentVariable($k.Trim(), $v.Trim().Trim('"'), 'Process')
}

$env:BEVY_ASSET_ROOT = $PWD.Path
$env:ARTIFACTS_PATH  = $env:ARTIFACTS_PATH  # .env から継承
$env:RUST_LOG        = "info"
Start-Process -FilePath ".\target\debug\backcast.exe" -WorkingDirectory $PWD.Path `
  -RedirectStandardOutput "$env:TEMP\backcast_log.txt" `
  -RedirectStandardError  "$env:TEMP\backcast_err.txt" -PassThru
```

### In-proc モード（PyO3 直接呼び出し）— **推奨起動方法**

`BACKEND_TRANSPORT=inproc` を設定すると Python エンジンを Rust プロセスに **直接埋め込み**、
gRPC も別途 Python プロセスも不要になる。

#### gRPC モードとの違い

| | gRPC モード（旧来） | In-proc モード（推奨） |
|---|---|---|
| Python backend プロセス | **必要**（別途起動） | **不要** |
| `BACKEND_TOKEN` | 必要 | 不要 |
| ネットワーク通信 | TCP 127.0.0.1:19876 | なし（関数呼び出し） |
| 起動コマンド数 | 2（backend + GUI） | 1（GUI のみ） |

#### ビルド前提

| 項目 | 内容 |
|---|---|
| pyo3 バージョン | 0.22（Python 3.13 まで正式サポート） |
| 動作確認済み Python | 3.13 / 3.14（ABI3 前方互換モード） |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | `.cargo/config.toml` で `"1"` に設定済み（手動不要） |

#### Windows セットアップ（初回のみ）

1. uv で venv を作成
2. `PYO3_PYTHON` を `.venv` に向けてビルド

```powershell
uv venv                          # .venv 作成
$env:PYO3_PYTHON = "$PWD\.venv\Scripts\python.exe"
cargo build
```

> **注意 (pyo3 0.22 + Python 3.14)**: ABI3 前方互換フラグは「バージョンチェックを抑止する」
> 暫定措置。pyo3 を 0.23 以上にアップグレードすれば不要になる（issue #64 フォロータスク②）。

#### 起動（推奨: スクリプト使用）

```powershell
.\scripts\run_inproc.ps1
# artifacts を別ドライブに置く場合:
.\scripts\run_inproc.ps1 -ArtifactsPath S:\artifacts
```

スクリプトは `__pycache__` 削除・環境変数設定・GUI 起動を一括実行する。

#### 起動（手動）

```powershell
# 1. __pycache__ を削除（WinError 6714 回避 — 初回と __pycache__ が作られたとき）
Get-ChildItem .\python -Recurse -Directory -Filter "__pycache__" | Remove-Item -Recurse -Force

# 2. Python DLL と venv site-packages を環境変数に設定
$pybase = & .\.venv\Scripts\python.exe -c "import sys; print(sys.base_prefix)"
$env:PATH    = "$pybase;$env:PATH"
$env:PYTHONPATH = "$PWD\.venv\Lib\site-packages"

# 3. In-proc 起動（Python バックエンドを別途起動する必要はない）
$env:BACKEND_ENABLED    = "true"
$env:BACKEND_TRANSPORT  = "inproc"
$env:PYTHON_ENGINE_PATH = "python"
$env:ARTIFACTS_PATH     = "$PWD\artifacts"
$env:BEVY_ASSET_ROOT    = $PWD.Path
$env:RUST_LOG           = "info"
.\target\debug\backcast.exe
```

> **Windows WinError 6714**: Python の `FileFinder` は `__pycache__` 存在時にディレクトリを
> 再スキャンし、Windows TxF フィルタードライバ (Windows Defender / VSS) に引っかかる。
> 起動前に `__pycache__` を削除することで回避。`sys.dont_write_bytecode = True` が
> Rust 側で設定されるため、削除後は `__pycache__` が再作成されず以降の起動も問題なし。

起動後のログで以下が出れば正常:
```
[inproc] Python worker thread starting
[inproc] DataEngine initialized
[inproc] RustEventSink registered on DataEngine
[inproc] InprocLiveServer initialized (live_venue_id=None)
```

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/strategy-replay.md](docs/strategy-replay.md) | 戦略リプレイの起動手順・CLI オプション |
| [docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](docs/plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md) | Strategy Runtime 実装仕様 |
| [python/README.md](python/README.md) | Python バックエンドのセットアップ・テスト |
