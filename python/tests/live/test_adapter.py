"""Tests for LiveVenueAdapter Protocol skeleton (Phase 8 §1.3)."""

from __future__ import annotations

from typing import AsyncIterator

import pytest

from engine.live.adapter import (
    Channel,
    InstrumentId,
    InstrumentRaw,
    LiveEvent,
    LiveVenueAdapter,
    VenueCredentials,
)


class _DummyAdapter:
    """Protocol を満たす最小ダミー実装（isinstance チェック用）。"""

    venue_id = "DUMMY"

    async def login(self, creds: VenueCredentials) -> None:
        return None

    async def logout(self) -> None:
        return None

    async def fetch_instruments(self) -> list[InstrumentRaw]:
        return []

    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None:
        return None

    async def unsubscribe(self, instrument_id: InstrumentId) -> None:
        return None

    async def _empty(self) -> AsyncIterator[LiveEvent]:
        if False:
            yield  # pragma: no cover

    def events(self) -> AsyncIterator[LiveEvent]:
        return self._empty()


def test_dummy_adapter_satisfies_protocol() -> None:
    """runtime_checkable Protocol として isinstance が通る。"""
    adapter = _DummyAdapter()
    assert isinstance(adapter, LiveVenueAdapter)
    assert adapter.venue_id == "DUMMY"


def test_venue_credentials_rejects_plain_password_fields() -> None:
    """VenueCredentials は credentials_source ベースで、password を持たない。

    Phase 8 §3.2 / §7 ADR: 平文資格情報を gRPC ペイロードに載せない。
    """
    creds = VenueCredentials(credentials_source="prompt", environment_hint="demo")
    assert creds.credentials_source == "prompt"
    assert creds.environment_hint == "demo"
    # password / user_id 等のフィールドは存在しないこと
    assert "password" not in creds.model_fields
    assert "user_id" not in creds.model_fields


def test_venue_credentials_source_allowlist() -> None:
    """credentials_source は prompt / session_cache / env のみ受理。

    §3.2 項目 5: 未知値は INVALID_CREDENTIALS_SOURCE で reject。
    """
    for src in ("prompt", "session_cache", "env"):
        VenueCredentials(credentials_source=src)  # type: ignore[arg-type]

    with pytest.raises(Exception):  # pydantic ValidationError
        VenueCredentials(credentials_source="keyring")  # type: ignore[arg-type]
    with pytest.raises(Exception):
        VenueCredentials(credentials_source="file")  # type: ignore[arg-type]
    with pytest.raises(Exception):
        VenueCredentials(credentials_source="")  # type: ignore[arg-type]


def test_instrument_raw_minimum_fields() -> None:
    raw = InstrumentRaw(
        code="7203", name="トヨタ", market="TSE", tick_size=0.5, lot_size=100
    )
    assert raw.code == "7203"
    assert raw.market == "TSE"
    assert raw.tick_size == 0.5
    assert raw.lot_size == 100


def test_channel_literal_values() -> None:
    """Channel Literal の全許容値を runtime で列挙確認。"""
    from typing import get_args
    assert set(get_args(Channel)) == {"price", "trades", "depth"}


def test_live_event_stub_is_object_alias() -> None:
    """LiveEvent は Phase 8 初期スタブ（object alias）。

    後続 step で Union 型に置き換える契約をテストで明示。
    """
    assert LiveEvent is object
