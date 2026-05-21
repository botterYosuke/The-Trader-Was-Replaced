"""InstrumentsStore spec (Phase 9 Step 9 — Live universe メタデータ parquet).

責務:
- `instruments_path(venue)` は `INSTRUMENTS_CACHE_DIR` env override（無ければ
  LOCALAPPDATA/~/.cache）配下の `the-trader-was-replaced/instruments/<venue>.parquet`。
- `write_instruments(venue, raws)` は **atomic**（tmp へ書いて os.replace）に parquet 化。
- `read_instruments(venue)` は parquet → `list[InstrumentRaw]`、無ければ None。
- write→read のラウンドトリップで全フィールドが保たれる（空リストも可）。
"""
from __future__ import annotations

from engine.live.adapter import InstrumentRaw
from engine.live import instruments_store


def _raws() -> list[InstrumentRaw]:
    return [
        InstrumentRaw(code="7203", name="トヨタ自動車", market="TSE", tick_size=0.5, lot_size=100),
        InstrumentRaw(code="9984", name="ソフトバンクG", market="TSE", tick_size=1.0, lot_size=100),
    ]


def test_instruments_path_uses_env_override(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    path = instruments_store.instruments_path("TACHIBANA")
    assert path == tmp_path / "tachibana.parquet"


def test_write_then_read_roundtrip(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    instruments_store.write_instruments("TACHIBANA", _raws())
    got = instruments_store.read_instruments("TACHIBANA")
    assert got == _raws()


def test_write_is_atomic_no_tmp_left(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    path = instruments_store.write_instruments("KABU", _raws())
    assert path.exists()
    # tmp ファイルが残らない（os.replace でリネーム済み）
    leftovers = list(tmp_path.glob("*.tmp"))
    assert leftovers == []


def test_read_missing_returns_none(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    assert instruments_store.read_instruments("TACHIBANA") is None


def test_write_empty_list_roundtrips_empty(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    instruments_store.write_instruments("TACHIBANA", [])
    assert instruments_store.read_instruments("TACHIBANA") == []


def test_write_overwrites_previous(monkeypatch, tmp_path) -> None:
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    instruments_store.write_instruments("TACHIBANA", _raws())
    smaller = [InstrumentRaw(code="1301", name="極洋", market="TSE", tick_size=1.0, lot_size=100)]
    instruments_store.write_instruments("TACHIBANA", smaller)
    assert instruments_store.read_instruments("TACHIBANA") == smaller
