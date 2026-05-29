# run_inproc.ps1 — backcast を in-proc モード（BACKEND_TRANSPORT=inproc）で起動する。
#
# gRPC バックエンドプロセスは不要。Python エンジンが Rust プロセスに直接埋め込まれる。
#
# 使い方:
#   cd <repo-root>
#   .\scripts\run_inproc.ps1
#
#   # artifacts を別ドライブに置く場合:
#   .\scripts\run_inproc.ps1 -ArtifactsPath S:\artifacts
#
# 前提:
#   1. uv venv が作成済み (.venv/)
#   2. cargo build が完了済み (target/debug/backcast.exe)
#
# Windows 固有の注意 (WinError 6714):
#   Python の FileFinder は __pycache__ ディレクトリの存在でディレクトリキャッシュを
#   無効化し再スキャンを行う。このスキャンが Windows の TxF フィルタードライバ
#   (Windows Defender / VSS) に引っかかり WinError 6714 が発生する。
#   回避策: 起動前に python/ 以下の __pycache__ を削除する（このスクリプトが自動実行）。
#   dont_write_bytecode=True が Rust コード側で設定されるため、削除後は __pycache__
#   が再作成されず、以降の再起動でも問題は起きない。

param(
    [string]$ArtifactsPath = "$PSScriptRoot\..\artifacts"
)

$RepoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $RepoRoot

# Python DLL のベースパスを取得
$PyBase = & .\.venv\Scripts\python.exe -c "import sys; print(sys.base_prefix)" 2>$null
if (-not $PyBase) {
    Write-Error "ERROR: .venv が見つかりません。先に 'uv venv' を実行してください。"
    exit 1
}

# __pycache__ を削除（WinError 6714 回避）
Write-Host "[inproc] Clearing python/__pycache__ (WinError 6714 workaround)..."
Get-ChildItem ".\python" -Recurse -Directory -Filter "__pycache__" -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# 環境変数設定
$env:PATH            = "$PyBase;$env:PATH"
$env:PYTHONPATH      = "$RepoRoot\.venv\Lib\site-packages"
$env:RUST_LOG        = "info"
$env:BACKEND_ENABLED = "true"
$env:BACKEND_TRANSPORT  = "inproc"
$env:PYTHON_ENGINE_PATH = "python"
$env:ARTIFACTS_PATH     = $ArtifactsPath
$env:BEVY_ASSET_ROOT    = $RepoRoot

Write-Host "[inproc] Starting backcast (BACKEND_TRANSPORT=inproc)..."
Write-Host "[inproc] ARTIFACTS_PATH = $env:ARTIFACTS_PATH"
Write-Host "[inproc] Logs -> $env:TEMP\backcast_err.txt"

Start-Process -FilePath ".\target\debug\backcast.exe" `
  -WorkingDirectory $RepoRoot `
  -RedirectStandardOutput "$env:TEMP\backcast_log.txt" `
  -RedirectStandardError  "$env:TEMP\backcast_err.txt" `
  -PassThru | ForEach-Object { Write-Host "[inproc] PID: $($_.Id)" }
