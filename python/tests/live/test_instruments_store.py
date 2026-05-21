"""InstrumentsStore spec (Phase 9 Step 9 — Live universe メタデータ parquet).

責務:
- `instruments_path(venue)` は `INSTRUMENTS_CACHE_DIR` env override（無ければ
  LOCALAPPDATA/~/.cache）配下の `the-trader-was-replaced/instruments/<venue>.parquet`。
- `write_instruments(venue, raws)` は **atomic**（tmp へ書いて os.replace）に parquet 化。
- `read_instruments(venue)` は parquet → `list[InstrumentRaw]`、無ければ None。
- write→read のラウンドトリップで全フィールドが保たれる（空リストも可）。
"""
from __future__ import annotations

import threading

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


def test_read_corrupt_parquet_returns_none(monkeypatch, tmp_path) -> None:
    # MEDIUM-4: a corrupt/truncated parquet must be a clean store-miss (None),
    # not propagate ArrowInvalid/OSError, so the caller falls back to live fetch.
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    path = instruments_store.instruments_path("TACHIBANA")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"this is not a parquet file at all \x00\x01\x02")
    assert instruments_store.read_instruments("TACHIBANA") is None


def test_concurrent_writes_do_not_corrupt_final_file(monkeypatch, tmp_path) -> None:
    # MEDIUM-5: two writers (login persist + 5:00 refresh) to the same venue path
    # must use unique tmp names so neither clobbers the other's tmp mid-write.
    # The final file must always be one complete, valid, readable instrument list.
    monkeypatch.setenv("INSTRUMENTS_CACHE_DIR", str(tmp_path))
    list_a = [InstrumentRaw(code="1000", name="A", market="TSE", tick_size=1.0, lot_size=100)] * 50
    list_b = [InstrumentRaw(code="2000", name="B", market="TSE", tick_size=0.5, lot_size=100)] * 50
    start = threading.Barrier(2)
    errors: list[BaseException] = []

    def _w(lst):
        try:
            start.wait()
            for _ in range(20):
                instruments_store.write_instruments("TACHIBANA", lst)
        except BaseException as e:  # noqa: BLE001
            errors.append(e)

    threads = [threading.Thread(target=_w, args=(list_a,)), threading.Thread(target=_w, args=(list_b,))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    assert errors == [], f"writer raised: {errors}"
    # No tmp files left behind.
    assert list(tmp_path.glob("*.tmp")) == []
    # Final file is a complete, valid list (one writer's full payload, never a mix
    # or a corrupt truncation).
    got = instruments_store.read_instruments("TACHIBANA")
    assert got is not None
    assert got in (list_a, list_b)
