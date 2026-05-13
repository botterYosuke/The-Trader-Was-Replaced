# The Trader Was Replaced

Nautilus Trader ベースの戦略リプレイ・評価エンジン。

## 起動方法

### 戦略リプレイ

→ **[docs/strategy-replay.md](docs/strategy-replay.md)**

```powershell
.\scripts\run_replay.ps1 -Strategy python\tests\data\test_strategy_daily.py
```

### Python バックエンド（gRPC）

→ **[python/README.md](python/README.md)**

```bash
cd python && uv run python -m engine --token your-secret-token
```

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/strategy-replay.md](docs/strategy-replay.md) | 戦略リプレイの起動手順・CLI オプション |
| [docs/plan/Phase 6.5 - Blacksheep Strategy Runtime.md](docs/plan/Phase%206.5%20-%20Blacksheep%20Strategy%20Runtime.md) | Strategy Runtime 実装仕様 |
| [python/README.md](python/README.md) | Python バックエンドのセットアップ・テスト |
