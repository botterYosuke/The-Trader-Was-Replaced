# Python Data Engine

"The Trader Was Replaced" の Python エンジン (nautilus_trader ベース)。
GUI 実行時は Rust プロセスに **PyO3 で in-proc 埋め込み**される（旧来の gRPC サーバは #64 / #68 で撤去済み）。
このディレクトリは Python 側の依存セットアップ・テスト・ヘッドレスリプレイ用。

## Requirements

- Python 3.10+
- `uv` (recommended)

## Installation

```bash
uv sync
```

## GUI 起動

GUI アプリ（in-proc）の起動はルート [README.md](../README.md#起動方法) を参照。
このディレクトリから直接エンジンプロセスを起動する手順は無い（gRPC サーバ廃止のため）。

## Strategy Replay（ヘッドレス）

GUI を使わず戦略をリプレイする CLI。詳細は **[docs/strategy-replay.md](../docs/strategy-replay.md)**。

```powershell
# 最速起動
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

## Testing

```bash
uv run pytest
```
