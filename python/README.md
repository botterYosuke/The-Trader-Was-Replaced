# Python Data Engine Backend

This is the Python backend for "The Trader Was Replaced". It provides market data via gRPC.

## Requirements

- Python 3.10+
- `uv` (recommended)

## Installation

```bash
uv sync
```

## Running the Engine

### Static Mode (Default)

```bash
uv run python -m engine --token your-secret-token
```


## Strategy Replay

→ **[docs/strategy-replay.md](../docs/strategy-replay.md)**

```powershell
# 最速起動
.\scripts\run_replay.ps1 -Strategy examples\test_strategy_daily.py
```

## Testing

```bash
uv run pytest
```
