"""Tachibana 通信本文の codec/parser ヘルパ。

仕様根拠:
- tachibana skill R7: 本番経路は Shift-JIS errors="strict" 必須。
  errors="ignore" は銘柄名・エラーメッセージのサイレント破損を招くため禁止。
  ログ用途に限り errors="replace" を許容する。
- tachibana skill R8: 空配列フィールドが空文字 "" で返るため [] に正規化する。
- event_protocol.md: EVENT フレームは ^A (\\x01) 項目区切り / ^B (\\x02) key-value 区切り /
  ^C (\\x03) 値区切り。本フェーズ (Phase 8 前半) はミニマル実装として
  ^A / ^B のみを解釈し、^C を含む複数値は join した文字列として保持する。
  WebSocket クライアント実装時 (Phase 8 後半) にリッチ化する。
"""

from __future__ import annotations

from typing import Any, Literal


def decode_response_body(
    data: bytes,
    *,
    errors: Literal["strict", "replace"] = "strict",
) -> str:
    """Tachibana REST/EVENT 応答本文 (Shift-JIS) を decode する。

    本番経路は errors="strict" を維持すること (tachibana skill R7)。
    errors="replace" はログ出力など破損を許容する経路のみで使う。
    errors="ignore" は API として受け付けない。
    """
    if errors not in ("strict", "replace"):
        raise ValueError(
            f"errors must be 'strict' or 'replace', got {errors!r} "
            "('ignore' is forbidden by tachibana skill R7)"
        )
    return data.decode("shift_jis", errors=errors)


def deserialize_tachibana_list(value: Any) -> list:
    """Tachibana JSON における list フィールドを正規化する (R8)。

    Tachibana は空配列を "" で返してくる仕様のため、利用側で常に list を
    想定できるよう "" / None を [] に揃える。
    """
    if value == "" or value is None:
        return []
    if isinstance(value, list):
        return value
    raise ValueError("expected list-like value")


def parse_event_frame(data: str) -> list[tuple[str, str]]:
    """EVENT フレームを (key, value) の list に分解する。

    本フェーズはミニマル実装:
    - ^A (\\x01) で項目分割、空項目はスキップ
    - 各項目内の最初の ^B (\\x02) で key/value 分割
    - ^C (\\x03) を含む複数値は value 文字列にそのまま保持
      (Phase 8 後半の WebSocket クライアント実装時に list[str] へ拡張予定)
    - ^B を含まない項目は無視する (キープアライブ等の単独トークン想定)
    """
    pairs: list[tuple[str, str]] = []
    for item in data.split("\x01"):
        if not item:
            continue
        if "\x02" not in item:
            continue
        key, value = item.split("\x02", 1)
        pairs.append((key, value))
    return pairs
