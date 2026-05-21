"""InstrumentsStore — Phase 9 Step 9 の Live universe メタデータ parquet 永続化。

責務（§3.6 / §0.4）:
- venue 別の銘柄メタデータ（code/name/market/tick_size/lot_size）を
  `cache_dir/the-trader-was-replaced/instruments/<venue>.parquet` に保存・読み出す。
- 書き込みは **atomic**（tmp へ書いて `os.replace`。`server_grpc._write_artifact_atomic`
  と同じ流儀）。読み手（ListInstruments / 日次更新）が中途半端な parquet を見ない。

設計判断:
- ログイン時 1 回 + 営業日 5:00 JST の日次更新（`instruments_scheduler.py`）が writer。
  ListInstruments(live) は store-first で読み、無ければ adapter から fetch する。
- parquet ライブラリは server_grpc が既に使う pyarrow を流用（追加依存なし）。
- `INSTRUMENTS_CACHE_DIR` env override はテスト用（tachibana_file_store の override と同型）。
"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Optional

from engine.live.adapter import InstrumentRaw

_SUBDIR = ("the-trader-was-replaced", "instruments")


def _cache_base() -> Path:
    override = os.environ.get("INSTRUMENTS_CACHE_DIR")
    if override:
        return Path(override)
    base = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~/.cache")
    return Path(base).joinpath(*_SUBDIR)


def instruments_path(venue: str) -> Path:
    """venue の銘柄メタ parquet パス。ファイル名は venue を小文字化して大小ブレを吸収。"""
    return _cache_base() / f"{venue.lower()}.parquet"


_SCHEMA = None  # built once on first write (pyarrow import stays lazy — see write/read)


def _schema():
    global _SCHEMA
    if _SCHEMA is None:
        import pyarrow as pa

        _SCHEMA = pa.schema(
            [
                ("code", pa.string()),
                ("name", pa.string()),
                ("market", pa.string()),
                ("tick_size", pa.float64()),
                ("lot_size", pa.int64()),
            ]
        )
    return _SCHEMA


def write_instruments(venue: str, raws: list[InstrumentRaw]) -> Path:
    """銘柄メタを atomic に parquet へ書く。空リストも空 parquet として書ける。"""
    import pyarrow as pa
    import pyarrow.parquet as pq

    path = instruments_path(venue)
    path.parent.mkdir(parents=True, exist_ok=True)
    table = pa.table(
        {
            "code": [r.code for r in raws],
            "name": [r.name for r in raws],
            "market": [r.market for r in raws],
            "tick_size": [float(r.tick_size) for r in raws],
            "lot_size": [int(r.lot_size) for r in raws],
        },
        schema=_schema(),
    )
    tmp = path.with_suffix(path.suffix + ".tmp")
    pq.write_table(table, tmp)
    os.replace(tmp, path)
    return path


def read_instruments(venue: str) -> Optional[list[InstrumentRaw]]:
    """parquet → list[InstrumentRaw]。ファイルが無ければ None（store-first の miss）。"""
    import pyarrow.parquet as pq

    path = instruments_path(venue)
    try:
        cols = pq.read_table(path).to_pydict()
    except FileNotFoundError:
        return None  # store miss → caller falls back to live fetch

    n = len(cols["code"])
    return [
        InstrumentRaw(
            code=cols["code"][i],
            name=cols["name"][i],
            market=cols["market"][i],
            tick_size=cols["tick_size"][i],
            lot_size=cols["lot_size"][i],
        )
        for i in range(n)
    ]
