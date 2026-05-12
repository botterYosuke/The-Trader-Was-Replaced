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

### Replay Mode

```bash
uv run python -m engine --token your-secret-token --mode replay --replay-path path/to/data.csv
```

The CSV should be in `timestamp,price` format (no header required, or non-numeric header will be skipped).

## Testing

```bash
uv run pytest
```
