"""Tachibana 通信本文の codec/parser テスト (tachibana skill R7/R8、event_protocol.md)。"""

from __future__ import annotations

import pytest

from engine.exchanges.tachibana_codec import (
    decode_response_body,
    deserialize_tachibana_list,
    parse_event_frame,
)


# --- decode_response_body ---------------------------------------------------

def test_decode_response_body_shift_jis_roundtrip() -> None:
    raw = "トヨタ自動車".encode("shift_jis")
    assert decode_response_body(raw) == "トヨタ自動車"


def test_decode_response_body_strict_raises_on_invalid_bytes() -> None:
    with pytest.raises(UnicodeDecodeError):
        decode_response_body(b"\x81")


def test_decode_response_body_replace_substitutes_invalid_bytes() -> None:
    decoded = decode_response_body(b"\x81", errors="replace")
    assert decoded != ""
    assert "\x81" not in decoded


# --- deserialize_tachibana_list (R8) ----------------------------------------

def test_deserialize_tachibana_list_empty_string_becomes_empty_list() -> None:
    assert deserialize_tachibana_list("") == []


def test_deserialize_tachibana_list_none_becomes_empty_list() -> None:
    assert deserialize_tachibana_list(None) == []


def test_deserialize_tachibana_list_passthrough_list() -> None:
    value = [{"x": 1}, {"y": 2}]
    assert deserialize_tachibana_list(value) == value


def test_deserialize_tachibana_list_rejects_dict() -> None:
    with pytest.raises(ValueError, match="expected list-like value"):
        deserialize_tachibana_list({"a": 1})


# --- parse_event_frame ------------------------------------------------------

def test_parse_event_frame_single_pair() -> None:
    frame = "\x01p_1_DPP\x02100"
    assert parse_event_frame(frame) == [("p_1_DPP", "100")]


def test_parse_event_frame_multiple_pairs() -> None:
    frame = "\x01p_1_DPP\x02100\x01p_2_DPP\x02200"
    assert parse_event_frame(frame) == [
        ("p_1_DPP", "100"),
        ("p_2_DPP", "200"),
    ]


def test_parse_event_frame_joins_multi_value_with_c() -> None:
    frame = "\x01p_1_LST\x02A\x03B\x03C"
    assert parse_event_frame(frame) == [("p_1_LST", "A\x03B\x03C")]


def test_parse_event_frame_ignores_empty_items() -> None:
    frame = "\x01p_1_DPP\x02100\x01"
    assert parse_event_frame(frame) == [("p_1_DPP", "100")]
