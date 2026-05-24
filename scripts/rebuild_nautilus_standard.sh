#!/usr/bin/env bash
#
# Rebuild nautilus_trader in STANDARD-precision (8-byte) mode on this machine.
#
# Why this exists (GH #34)
# ------------------------
# The shared J-Quants catalog (Synology) is written by Windows in standard precision
# (`fixed_size_binary[8]`, i64, FIXED_PRECISION=9). nautilus bakes its precision mode
# into the compiled wheel via the `HIGH_PRECISION` Cargo feature, and there is **no
# prebuilt standard-precision wheel for Intel macOS** on PyPI — so on this box uv
# builds nautilus from the sdist, and that source build defaults to HIGH_PRECISION=true
# (16-byte). A 16-byte build reading the 8-byte catalog makes nautilus abort the whole
# backend process (SIGABRT) inside catalog.query(); the UI only sees "transport error".
#
# This script rebuilds nautilus standard-precision so it matches the shared catalog.
# Run it after any `uv sync` / `uv pip install` that may have pulled a high-precision
# build. The shared catalog must NOT be rewritten (Windows would then break in reverse).
#
# Regression guard: `python/engine/nautilus_catalog_loader.py` preflights the parquet
# width before query() and raises CatalogPrecisionMismatchError (surfaced to the UI) if
# this box ever ends up high-precision again — so a missed rebuild fails loud, not fatal.
#
# Usage:  scripts/rebuild_nautilus_standard.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VENV_PY="$REPO_ROOT/.venv/bin/python"
if [[ ! -x "$VENV_PY" ]]; then
  echo "error: $VENV_PY not found — run 'uv sync' first." >&2
  exit 1
fi

echo "Rebuilding nautilus_trader standard-precision (HIGH_PRECISION=false) from sdist..."
echo "(this recompiles the Rust extension and takes several minutes)"

# `--no-binary` forces a source build; `--reinstall`/`--no-cache` defeat uv's cached
# high-precision wheel; `env -u VIRTUAL_ENV` ignores any stale activated venv.
env -u VIRTUAL_ENV HIGH_PRECISION=false BUILD_MODE=release \
  uv pip install --no-cache --reinstall --no-binary nautilus-trader \
  --python "$VENV_PY" \
  'nautilus-trader==1.226.0'

echo ""
echo "Verifying precision build..."
"$VENV_PY" - <<'PY'
import sys
from nautilus_trader.core import nautilus_pyo3 as p
print(f"HIGH_PRECISION={p.HIGH_PRECISION} PRECISION_BYTES={p.PRECISION_BYTES}")
if p.PRECISION_BYTES != 8:
    print("ERROR: expected PRECISION_BYTES=8 (standard) to match the shared catalog", file=sys.stderr)
    sys.exit(1)
print("OK: nautilus is standard-precision (8-byte) and matches the shared catalog.")
PY
