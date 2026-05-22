# run_replay.ps1 — One-shot strategy replay launcher
#
# 使い方:
#   .\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
#   .\scripts\run_replay.ps1 -Strategy <path> -StrategyParam window=10,lot_size=200
#   .\scripts\run_replay.ps1 -Strategy <path> -Catalog D:\my\catalog -SkipCatalogBuild
#
# 動作:
#   1. 戦略ファイルから SCENARIO を抽出（AST、副作用なし）
#   2. --Catalog の中に必要な instrument/期間/granularity の Bar が無ければ
#      ensure_jquants_catalog で自動構築（base_dir は $env:DEV_J_QUANTS_CACHE → S:/j-quants）
#   3. uv run python -m engine.strategy_replay run を実行
#   4. stdout JSON から run_id を抽出して表示

param(
    [Parameter(Mandatory=$true)]
    [string]$Strategy,

    [string]$Catalog,

    [string]$RunBufferDir,

    [string[]]$StrategyParam = @(),

    [switch]$SkipCatalogBuild,

    [switch]$VerboseRun
)

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$ErrorActionPreference = "Stop"

$RepoRoot   = Split-Path $PSScriptRoot -Parent
$PyRoot     = Join-Path $RepoRoot "python"

function Get-DotEnvValue([string]$Name) {
    $envFile = Join-Path $RepoRoot ".env"
    if (-not (Test-Path $envFile)) { return $null }
    foreach ($line in (Get-Content $envFile)) {
        if ($line -match "^\s*$Name\s*=\s*['""]?([^'""#]+)['""]?\s*(?:#.*)?$") {
            return $Matches[1].Trim()
        }
    }
    return $null
}

function Resolve-RepoRelativePath([string]$PathValue) {
    if ([System.IO.Path]::IsPathRooted($PathValue)) { return $PathValue }
    return Join-Path $RepoRoot $PathValue
}

if (-not $Catalog) {
    $ArtifactsRoot = $env:ARTIFACTS_PATH
    if (-not $ArtifactsRoot) { $ArtifactsRoot = Get-DotEnvValue "ARTIFACTS_PATH" }
    if (-not $ArtifactsRoot) { $ArtifactsRoot = "artifacts" }
    $ArtifactsRoot = Resolve-RepoRelativePath $ArtifactsRoot
    $Catalog = Join-Path $ArtifactsRoot "jquants-catalog"
}
if (-not (Test-Path $Strategy)) {
    throw "strategy not found: $Strategy"
}
$StrategyAbs = (Resolve-Path $Strategy).Path

Write-Host "=== strategy replay ===" -ForegroundColor Cyan
Write-Host "strategy : $StrategyAbs"
Write-Host "catalog  : $Catalog"

# ── 1. scenario 読み込み ──────────────────────────────────────────────────────
$scenarioJson = & {
    Push-Location $PyRoot
    try {
        uv run python -c @"
import json, sys
from pathlib import Path
from engine.strategy_runtime.scenario import load_scenario
p = Path(r'$StrategyAbs')
d = load_scenario(p)
print(json.dumps(d))
"@
    } finally { Pop-Location }
}
$scenario = $scenarioJson | ConvertFrom-Json

# v1 = instrument(str), v2/v3 = instruments(list)
if ($scenario.PSObject.Properties.Name -contains "instruments") {
    $instruments = @($scenario.instruments)
} else {
    $instruments = @($scenario.instrument)
}
$granularity = $scenario.granularity
$start = $scenario.start
$end   = $scenario.end

Write-Host "scenario : instruments=$($instruments -join ',') granularity=$granularity period=$start..$end"

# ── 2. catalog 自動構築 ───────────────────────────────────────────────────────
if (-not $SkipCatalogBuild) {
    # .env から DEV_J_QUANTS_CACHE を読む（PowerShell は .env を自動ロードしない）
    $baseDir = $env:DEV_J_QUANTS_CACHE
    if (-not $baseDir) { $baseDir = Get-DotEnvValue "DEV_J_QUANTS_CACHE" }
    if (-not $baseDir) {
        throw "DEV_J_QUANTS_CACHE is not set (.env or environment). Cannot auto-build catalog."
    }
    if (-not (Test-Path $baseDir)) {
        Write-Host "[catalog] J-Quants base_dir not found ($baseDir) — skipping auto-build" -ForegroundColor Yellow
    } else {
        Write-Host "[catalog] ensure: base_dir=$baseDir" -ForegroundColor Yellow
        Push-Location $PyRoot
        try {
            foreach ($iid in $instruments) {
                uv run python -c @"
from engine.jquants_to_catalog import ensure_jquants_catalog
r = ensure_jquants_catalog(
    base_dir=r'$baseDir',
    catalog_path=r'$Catalog',
    instrument_id='$iid',
    start_date='$start',
    end_date='$end',
    granularity='$granularity',
)
print(f'[catalog] wrote {r.rows_written} rows for {r.bar_type}')
"@
                if ($LASTEXITCODE -ne 0) { throw "catalog build failed for $iid" }
            }
        } finally { Pop-Location }
    }
}

# ── 3. リプレイ実行 ───────────────────────────────────────────────────────────
$ReplayArgs = @(
    "-u", "-m", "engine.strategy_replay", "run",
    "--strategy", $StrategyAbs,
    "--catalog",  $Catalog
)
if ($RunBufferDir) { $ReplayArgs += @("--run-buffer-dir", $RunBufferDir) }
foreach ($p in $StrategyParam) { $ReplayArgs += @("--strategy-param", $p) }
if ($VerboseRun) { $ReplayArgs += "--verbose" }

Write-Host ""
Write-Host "--- replay start ---" -ForegroundColor Yellow

Push-Location $PyRoot
$prevEAP = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    $output = & uv run python @ReplayArgs 2>&1
    $exitCode = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $prevEAP
    Pop-Location
}

Write-Host ($output -join "`n")

if ($exitCode -ne 0) {
    Write-Host "replay FAILED (exit=$exitCode)" -ForegroundColor Red
    exit $exitCode
}

# ── 4. run_id 抽出 ────────────────────────────────────────────────────────────
try {
    $jsonText = ($output | Where-Object { $_ -notmatch "^\s*(INFO|DEBUG|WARNING|ERROR)\s" }) -join "`n"
    $result = $jsonText | ConvertFrom-Json
    Write-Host ""
    Write-Host "=== replay finished ===" -ForegroundColor Green
    Write-Host "run_id        : $($result.run_id)"
    Write-Host "run_dir       : $($result.run_dir)"
    Write-Host "equity_points : $($result.equity_points)"
    Write-Host "fills_count   : $($result.fills_count)"
    Write-Host "total_pnl     : $($result.total_pnl)"
} catch {
    Write-Host "(run_id JSON parse failed: $_)" -ForegroundColor Yellow
}
