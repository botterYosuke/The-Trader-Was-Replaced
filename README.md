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

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/strategy-replay.md](docs/strategy-replay.md) | 戦略リプレイの起動手順・CLI オプション |
| [docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](docs/plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md) | Strategy Runtime 実装仕様 |
| [python/README.md](python/README.md) | Python バックエンドのセットアップ・テスト |
