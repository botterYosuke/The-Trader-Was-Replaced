# Plan: Fix Nautilus ParquetDataCatalog UNC-URI portability bug

## Context

Phase 7.6 E2E surfaced a portability bug: a catalog physically stored on NAS
`\\sasaco-ds218\StockData\artifacts\jquants-catalog`, mounted on the
developer machine as `S:\`, cannot be read via `S:/artifacts/jquants-catalog`.
The error is:

```
No suitable object store found for file://sasaco-ds218/StockData/artifacts/
jquants-catalog/data/bar/1301.TSE-1-MINUTE-LAST-EXTERNAL/
```

DataFusion is being handed a `file://<host>/<share>/...` URI even though the
caller passed an `S:/...` drive path.

### Scope (refined per review)

- **In scope**: replay loads via a Windows mapped drive (`S:`, `Z:`, ...) must
  work end-to-end, both for the user-supplied `catalog_path` route and the
  auto-built `ensure_jquants_catalog` route.
- **Out of scope**: making bare UNC paths (`\\host\share\...`) work. Nautilus
  / DataFusion don't currently handle a host component in `file://` URIs, and
  fixing that lives upstream in `nautilus_trader` / DataFusion's
  `LocalFileSystem`. Instead, **we fail fast with a clear error** that tells
  the user to map the share to a drive letter. This is a deliberate
  narrowing of the original "S: or UNC, both should read" wording -- honest
  UNC support would require an upstream PR and is not justified for Phase 7.6.

## Root cause (confirmed)

Read-path failure chain:

1. Rust `TradingSettings::from_env()` builds `S:/artifacts/jquants-catalog`
   from `.env` (`ARTIFACTS_PATH=S:/artifacts`).
   ([src/trading.rs:126-136](src/trading.rs#L126-L136))
2. Rust sends that string in `LoadReplayDataRequest.catalog_path`.
   ([src/main.rs:427-437](src/main.rs#L427-L437))
3. Python `server_grpc.LoadReplayData` -> `core.load_replay_data` ->
   `NautilusBarsReplayProvider` -> `load_bars` ->
   `nautilus_catalog_loader._resolve_catalog_path`.
4. **`_resolve_catalog_path` calls `Path(catalog_path).resolve()`.** On
   Windows, `Path.resolve()` walks reparse points / network drive mappings via
   `GetFinalPathNameByHandle`, so `Path("S:/artifacts/jquants-catalog").resolve()`
   returns `\\sasaco-ds218\StockData\artifacts\jquants-catalog`.
   ([python/engine/nautilus_catalog_loader.py:20-25](python/engine/nautilus_catalog_loader.py#L20-L25))
5. `ParquetDataCatalog.__init__` calls `make_path_posix(str(path))`
   ([parquet.py:164](.claude/skills/nautilus_trader/src/nautilus_trader/persistence/catalog/parquet.py#L164)),
   producing `//sasaco-ds218/StockData/artifacts/jquants-catalog` in
   `self.path`.
6. `self.fs.glob()` returns file paths beginning with `//sasaco-ds218/...`;
   `_build_file_uri()` returns them unchanged for `fs_protocol == "file"`
   ([parquet.py:1905-1938](.claude/skills/nautilus_trader/src/nautilus_trader/persistence/catalog/parquet.py#L1905-L1938)).
7. DataFusion's `add_file` reads `//sasaco-ds218/...` as
   `file://sasaco-ds218/...` (host = `sasaco-ds218`) and finds no
   ObjectStore registered for that host -- `_register_object_store_with_session`
   is skipped when `fs_protocol == "file"`
   ([parquet.py:1800-1801](.claude/skills/nautilus_trader/src/nautilus_trader/persistence/catalog/parquet.py#L1800-L1801)).

Parquet files themselves do **not** embed absolute paths -- they're derived at
read time from `self.path`. So the fix lives entirely in our caller code.

### Second `.resolve()` site (auto-build route, missed in earlier draft)

[python/engine/jquants_to_catalog.py:98](python/engine/jquants_to_catalog.py#L98)
also calls `catalog_path.resolve()` inside `_write_bars_to_catalog`, then
returns that string as `JQuantsCatalogResult.catalog_path`
([same:102-106](python/engine/jquants_to_catalog.py#L102-L106)).
`DataEngine.load_replay_data` consumes `result.catalog_path` and passes it
into `NautilusBarsReplayProvider`
([python/engine/core.py:175](python/engine/core.py#L175)), which is exactly
the failure surface we're fixing. **This route must be patched in lockstep**,
otherwise the auto-build path will produce a UNC string and trip the new
guard from the read side.

## Fix

### 1. `_resolve_catalog_path` -- read side

In [python/engine/nautilus_catalog_loader.py](python/engine/nautilus_catalog_loader.py),
replace `Path(catalog_path).resolve()` with `Path(catalog_path).absolute()`,
and reject UNC input up front:

```python
import os
from pathlib import Path

def _resolve_catalog_path(catalog_path: str | Path) -> str:
    raw = os.fspath(catalog_path)
    # UNC paths become file://host/... in DataFusion, which has no ObjectStore
    # for the host component -> "No suitable object store found". Map the share
    # to a drive letter and pass that instead.
    if raw.startswith("\\\\") or raw.startswith("//"):
        raise ValueError(
            f"UNC catalog paths are not supported (got {raw!r}). "
            "Map the share to a drive letter (e.g. S:) and pass that instead."
        )
    # NOTE: Path.resolve() on Windows walks reparse points and rewrites
    # mapped drives (S:\) back to their UNC form (\\host\share\...). Use
    # absolute() -- it only prepends CWD when the path is relative.
    p = Path(raw).absolute()
    if not p.exists():
        raise FileNotFoundError(f"Catalog path does not exist: {p}")
    return str(p)
```

### 2. `_write_bars_to_catalog` -- auto-build (write) side

In [python/engine/jquants_to_catalog.py:98](python/engine/jquants_to_catalog.py#L98),
apply the **same** two changes as the read side: reject UNC input first, then
swap `.resolve()` for `.absolute()`. The UNC guard here is **required**, not
optional -- without it, the auto-build route can silently write a 30+ MB
catalog into an unreadable UNC location and only fail later when the read
path trips its own UNC guard.

```python
import os

def _write_bars_to_catalog(rows, to_ticks, bar_type_str, catalog_path, price_precision):
    raw = os.fspath(catalog_path)
    if raw.startswith("\\\\") or raw.startswith("//"):
        raise ValueError(
            f"UNC catalog paths are not supported (got {raw!r}). "
            "Map the share to a drive letter (e.g. S:) and pass that instead."
        )
    ...
    catalog_dir = catalog_path.absolute()    # was: catalog_path.resolve()
    catalog_dir.mkdir(parents=True, exist_ok=True)
    ParquetDataCatalog(str(catalog_dir)).write_data(bars)

    return JQuantsCatalogResult(
        catalog_path=str(catalog_dir),
        bar_type=bar_type_str,
        rows_written=len(bars),
    )
```

Factoring a shared `_reject_unc_and_absolutize(path) -> Path` helper between
`nautilus_catalog_loader` and `jquants_to_catalog` is fine if the second use
site is added cleanly; otherwise duplicating the four lines is acceptable for
two call sites.

Parquet metadata doesn't embed the catalog path, so no further write-side
change is needed.

### 3. Tests

**Update** existing
[python/tests/test_nautilus_catalog_loader.py:94](python/tests/test_nautilus_catalog_loader.py#L94):

```python
assert catalog.path == str(patched_catalog)   # was: str(patched_catalog.resolve())
```

The old assertion silently encoded the broken behavior -- it currently passes
only because `tmp_path` happens not to be a reparse point. After the fix,
`_resolve_catalog_path` returns `Path.absolute()`, which on a non-reparse
`tmp_path` equals plain `str(patched_catalog)`.

**Add** `python/tests/engine/test_nautilus_catalog_loader_paths.py` with:

- `test_resolve_does_not_call_pathlib_resolve` -- monkeypatch
  `pathlib.Path.resolve` to raise; assert `_resolve_catalog_path(tmp_path)`
  still succeeds. This is the actual regression we care about (it pins the
  fix in place even on hardware that has no network mounts).
- `test_resolve_rejects_unc_forward_slashes` --
  `_resolve_catalog_path("//sasaco-ds218/share/cat")` -> `ValueError`.
- `test_resolve_rejects_unc_backslashes` --
  `_resolve_catalog_path(r"\\sasaco-ds218\share\cat")` -> `ValueError`.
- `test_resolve_accepts_relative_and_existing(tmp_path, monkeypatch)` --
  `chdir(tmp_path)`; create `./catalog`; assert returned string is absolute
  and points into `tmp_path`.

**Add** write-side regressions in the same file (covering the
`jquants_to_catalog -> core.load_replay_data` route the reviewer flagged).
The critical one is the `Path.resolve` monkeypatch -- on a vanilla local
`tmp_path`, `.resolve()` and `.absolute()` return the same string, so a test
that only inspects the returned path would still pass if someone reintroduced
`.resolve()`. Patching `Path.resolve` to raise around the call is what
actually pins the regression:

- `test_write_bars_does_not_call_pathlib_resolve(tmp_path, monkeypatch)` --
  monkeypatch `pathlib.Path.resolve` to raise (e.g. `RuntimeError`); call
  `_write_bars_to_catalog` with one fake bar and a `tmp_path` catalog dir;
  assert it succeeds and `result.catalog_path` is an absolute string under
  `tmp_path` that does not start with `\\` or `//`. This is the regression
  that would have caught the gap in the earlier draft.
- `test_write_bars_rejects_unc_forward_slashes` --
  `_write_bars_to_catalog(..., catalog_path=Path("//host/share/cat"))` ->
  `ValueError`, **before** any disk write happens. Assert no files were
  created under a sentinel tmp directory.
- `test_write_bars_rejects_unc_backslashes` -- same for
  `Path(r"\\host\share\cat")`.

(`_write_bars_to_catalog` is the narrowest entry point; testing it directly
avoids needing a real `JQuantsLoader` or J-Quants fixtures.)

## Critical files

- [python/engine/nautilus_catalog_loader.py](python/engine/nautilus_catalog_loader.py) -- read-side fix (~10 LoC).
- [python/engine/jquants_to_catalog.py](python/engine/jquants_to_catalog.py) -- write-side / auto-build fix (1 line + optional UNC guard).
- [python/tests/test_nautilus_catalog_loader.py](python/tests/test_nautilus_catalog_loader.py) -- update line 94 assertion.
- [python/tests/engine/test_nautilus_catalog_loader_paths.py](python/tests/engine/test_nautilus_catalog_loader_paths.py) -- new regression file.
- [.claude/skills/nautilus_trader/src/nautilus_trader/persistence/catalog/parquet.py](.claude/skills/nautilus_trader/src/nautilus_trader/persistence/catalog/parquet.py) -- read-only reference (explains why the fix has to live in our caller).

Not touched: `src/trading.rs`, `src/main.rs`, `python/engine/server_grpc.py`,
`python/engine/core.py` -- they forward the string verbatim and are correct.

## Verification

1. **Unit tests** --
   `uv run pytest python/tests/test_nautilus_catalog_loader.py python/tests/engine/test_nautilus_catalog_loader_paths.py -q`.
2. **Full suite sanity** -- `uv run pytest -q` to confirm no other test
   depended on `_resolve_catalog_path` calling `Path.resolve()`.
3. **Direct repro (mapped drive)** with the offending NAS catalog on `S:`:
   ```powershell
   uv run python -c "from engine.nautilus_catalog_loader import load_bars; print(len(load_bars('S:/artifacts/jquants-catalog', instrument_ids=['1301.TSE-1-MINUTE-LAST-EXTERNAL'])))"
   ```
   Expect a positive bar count, not `No suitable object store found`.
4. **Direct repro (UNC, negative case)**:
   ```powershell
   uv run python -c "from engine.nautilus_catalog_loader import load_bars; load_bars(r'\\sasaco-ds218\StockData\artifacts\jquants-catalog')"
   ```
   Expect the new `ValueError` with the "map to a drive letter" hint, **not**
   the cryptic DataFusion error.
5. **End-to-end (Phase 7.6 replay)** via the e2e-testing skill with
   `ARTIFACTS_PATH=S:/artifacts`. The Replay Startup Window should advance
   past `LoadingData` and reach `WaitingForFirstTick`.
6. **Mapped-drive evidence (one-shot, for PR description)**:
   ```powershell
   uv run python -c "from pathlib import Path; print(repr(str(Path('S:/artifacts/jquants-catalog').absolute()))); print(repr(str(Path('S:/artifacts/jquants-catalog').resolve())))"
   ```
   Documents that `.resolve()` returns UNC while `.absolute()` keeps `S:` in
   this environment -- prevents a future refactor from reintroducing
   `.resolve()`.

## Follow-up memory note

After merge, write `memory/catalog-absolute-uri-trap.md` (linked from
`MEMORY.md`):

> **Trap**: `Path.resolve()` on a Windows mapped network drive (e.g. `S:`)
> walks the reparse point and returns the UNC form (`\\host\share\...`).
> Feeding that UNC string into `ParquetDataCatalog` produces
> `//host/share/...`, which DataFusion treats as `file://host/...` and
> cannot resolve (no ObjectStore registered for the host component). Use
> `Path.absolute()` (never follows reparse points) whenever the path is
> destined for Nautilus / DataFusion, and guard against bare UNC input with
> an explicit `ValueError`. Both the read site
> (`nautilus_catalog_loader._resolve_catalog_path`) and the write site
> (`jquants_to_catalog._write_bars_to_catalog`, whose return value flows
> back into the read path via `DataEngine.load_replay_data`) must use
> `.absolute()`.

The memory write is part of the post-implementation step, not this plan.
