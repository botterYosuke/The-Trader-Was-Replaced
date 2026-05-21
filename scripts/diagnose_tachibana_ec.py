#!/usr/bin/env python
"""Live-DEMO diagnostic for Phase 9 Step 5 — account-level EC subscription URL.

Resolves the open verification questions against the real Tachibana DEMO server
(``demo-kabuka.e-shiten.jp``). Run AFTER 9:00 JST (TSE open) — EC frames only
flow when there is order activity.

POINT 1 (OPEN — needs this live run): which EVENT WebSocket URL shape actually
delivers account-level EC (注文約定通知) frames? The shapes differ across our
code and the e-station reference, and even e-station never confirmed it live:

  * step5    — our Step 5 production shape: p_rid/p_board_no/p_eno + p_evt_cmd=ST,KP,EC,SS,US
               (this is what the adapter's own EC stream uses, captured via the
                on_order_event hook below — no separate socket needed)
  * bare     — e-station prod: connect to url_event_ws with NO query params at all
  * fd_ec    — e-station test comment: full FD-style params + p_evt_cmd=FD,EC + p_issue_code
  * issue_ec — FD-style params + p_evt_cmd=ST,KP,EC + p_issue_code (issue-scoped EC)

We open the alternatives in parallel, place+cancel one tiny resting demo order,
and report which connection(s) received the EC frame for that order number.

POINT 2 (ALREADY FIXED): raw comma vs %2C in p_evt_cmd. e-station's production
postmortem (2026-05-01) already proved %2C breaks the subscription, and
build_event_url now sends raw commas. Pass --include-encoded to additionally open
a %2C-encoded variant so the difference is visible live.

SAFETY
------
* DEMO ONLY. Refuses unless the resolved environment is 'demo'. Never prod.
* Reads creds from env / .env: DEV_TACHIBANA_USER_ID / _PASSWORD / _SECOND_PASSWORD.
  Secrets and the virtual session URLs are never logged (URLs are masked).
* Places an order ONLY with --place-order. It is a BUY *limit* order priced far
  BELOW market so it rests without filling, then is cancelled. You decide when to
  pass --place-order; nothing fires by default.

Usage
-----
  # connection-only (safe any time the server is up — verifies the server accepts
  # each candidate URL via ST p_errno=0 / KP keepalives; no order placed):
  .venv/Scripts/python.exe scripts/diagnose_tachibana_ec.py --seconds 20

  # full EC verification (after 9:00 JST). --price MUST be well below market:
  .venv/Scripts/python.exe scripts/diagnose_tachibana_ec.py \
      --place-order --ticker 7203 --qty 100 --price 1000 --seconds 30
"""
from __future__ import annotations

import argparse
import asyncio
import logging
import sys
import time
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path

_REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(_REPO / "python"))

_JST = timezone(timedelta(hours=9))


def _load_dotenv(path: Path) -> None:
    """Minimal .env loader (the app does not auto-load it). Does not overwrite
    values already present in the environment."""
    import os

    if not path.exists():
        return
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        key, value = key.strip(), value.strip()
        if key and value and key not in os.environ:
            os.environ[key] = value


def _mask(url: str) -> str:
    """Mask the session token in a virtual URL for safe logging."""
    if "://" not in url:
        return "***"
    scheme, _, rest = url.partition("://")
    host, _, _path = rest.partition("/")
    return f"{scheme}://{host}/***"


class _FrameSink:
    """Per-candidate frame collector. Records (frame_type, p_NO, p_NT) tuples."""

    def __init__(self, label: str) -> None:
        self.label = label
        self.counts: dict[str, int] = defaultdict(int)
        self.ec_order_numbers: list[tuple[str, str]] = []  # (p_NO, p_NT)
        self.first_st_errno: str | None = None
        self.connected = False

    async def __call__(self, frame_type: str, fields: dict, recv_ts_ms: int) -> None:
        self.counts[frame_type] += 1
        self.connected = True
        if frame_type == "ST" and self.first_st_errno is None:
            self.first_st_errno = fields.get("p_errno", "?")
        if frame_type == "EC":
            p_no = fields.get("p_NO", "")
            p_nt = fields.get("p_NT", "")
            self.ec_order_numbers.append((p_no, p_nt))
            log.info("  [%s] EC frame: p_NO=%s p_NT=%s p_ZSU=%s",
                     self.label, p_no, p_nt, fields.get("p_ZSU", ""))


async def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--seconds", type=float, default=20.0,
                    help="how long to keep listening after the last action")
    ap.add_argument("--place-order", action="store_true",
                    help="place AND cancel one resting demo limit order (needs market open)")
    ap.add_argument("--ticker", default="7203", help="issue code for the demo order")
    ap.add_argument("--qty", type=float, default=100.0, help="order qty (one unit)")
    ap.add_argument("--price", type=float, default=None,
                    help="LIMIT price — MUST be well BELOW market so the BUY rests unfilled")
    ap.add_argument("--include-encoded", action="store_true",
                    help="also open a %%2C-encoded variant to show point 2 live")
    args = ap.parse_args()

    _load_dotenv(_REPO / ".env")

    from engine.exchanges.tachibana import TachibanaAdapter
    from engine.exchanges.tachibana_url import EventUrl, build_event_url
    from engine.exchanges.tachibana_ws import TachibanaEventWs
    from engine.live.adapter import VenueCredentials

    import os
    second_pw = os.environ.get("DEV_TACHIBANA_SECOND_PASSWORD", "")

    now_jst = datetime.now(_JST)
    log.info("JST now: %s", now_jst.strftime("%Y-%m-%d %H:%M:%S %a"))
    if args.place_order and not (9 <= now_jst.hour < 15):
        log.warning("Market likely CLOSED (TSE 09:00-15:00 JST). EC frames may not flow.")

    captured_via_hook: list[tuple[str, str, str]] = []  # (venue_order_id, status, cid)

    def _on_order_event(ev) -> None:  # production path = 'step5' shape
        captured_via_hook.append((ev.venue_order_id, ev.status, ev.client_order_id))
        log.info("  [step5/hook] OrderEvent: venue_order_id=%s status=%s filled=%s",
                 ev.venue_order_id, ev.status, ev.filled_qty)

    class _Resolver:
        async def resolve(self, venue: str, purpose: str) -> str:
            if not second_pw:
                raise RuntimeError("DEV_TACHIBANA_SECOND_PASSWORD not set in env/.env")
            return second_pw

    adapter = TachibanaAdapter(environment="demo")
    if adapter._env != "demo":
        log.error("REFUSING: environment is not 'demo'.")
        return 2
    adapter.set_execution_hooks(secret_resolver=_Resolver(), on_order_event=_on_order_event)

    log.info("Logging in to Tachibana DEMO (env credentials)...")
    try:
        await adapter.login(VenueCredentials(credentials_source="env"))
    except Exception as exc:
        log.error("Login failed: %s (pre-market / 閉局 or bad creds?)", exc)
        return 3
    sess = adapter._session
    assert sess is not None
    log.info("Login OK. url_event_ws=%s", _mask(str(sess.url_event_ws)))

    # Build the alternative candidate connections (step5 is the adapter's own stream).
    base = str(sess.url_event_ws)
    candidates: dict[str, str] = {
        "bare": base,
        "fd_ec": build_event_url(EventUrl(base), {
            "p_rid": "22", "p_board_no": "1000", "p_gyou_no": "1",
            "p_issue_code": args.ticker, "p_mkt_code": "00", "p_eno": "0",
            "p_evt_cmd": "FD,EC",
        }),
        "issue_ec": build_event_url(EventUrl(base), {
            "p_rid": "22", "p_board_no": "1000", "p_gyou_no": "1",
            "p_issue_code": args.ticker, "p_mkt_code": "00", "p_eno": "0",
            "p_evt_cmd": "ST,KP,EC",
        }),
    }
    if args.include_encoded:
        # Manual %2C variant (build_event_url now refuses '%'): demonstrates point 2.
        candidates["encoded"] = (
            base.rstrip("?&")
            + "?p_rid=22&p_board_no=1000&p_eno=0&p_evt_cmd=ST%2CKP%2CEC%2CSS%2CUS"
        )

    sinks: dict[str, _FrameSink] = {}
    stops: list[asyncio.Event] = []
    tasks: list[asyncio.Task] = []
    for label, url in candidates.items():
        sink = _FrameSink(label)
        sinks[label] = sink
        stop = asyncio.Event()
        stops.append(stop)
        ws = TachibanaEventWs(url, stop, ticker=f"DIAG-{label}")
        tasks.append(asyncio.create_task(ws.run(sink)))
        log.info("opened candidate [%s] %s", label, _mask(url))

    # Give every connection time to handshake + receive the initial ST/KP.
    await asyncio.sleep(6.0)
    for label, sink in sinks.items():
        log.info("after connect: [%s] connected=%s ST.p_errno=%s counts=%s",
                 label, sink.connected, sink.first_st_errno, dict(sink.counts))

    placed_order_number = ""
    if args.place_order:
        if args.price is None:
            log.error("--place-order requires --price (a LIMIT price below market).")
        else:
            log.info("Placing resting BUY LIMIT %s x%s @ %s (demo)...",
                     args.ticker, args.qty, args.price)
            res = await adapter.submit_order(
                venue="TACHIBANA", instrument_id=f"{args.ticker}.TSE", side="BUY",
                qty=args.qty, price=args.price, order_type="LIMIT", time_in_force="DAY",
            )
            log.info("submit_order -> status=%s reject=%s cid=%s",
                     res.status, res.reject_reason, res.client_order_id)
            ref = adapter._orders_ref.get(res.client_order_id)
            placed_order_number = ref.order_number if ref else ""
            log.info("venue order_number=%s eigyou_day=%s",
                     placed_order_number, ref.eigyou_day if ref else "")
            await asyncio.sleep(min(args.seconds, 15.0))
            if res.status == "ACCEPTED":
                log.info("Cancelling the demo order...")
                cres = await adapter.cancel_order(venue="TACHIBANA", order_id=res.client_order_id)
                log.info("cancel_order -> status=%s reject=%s", cres.status, cres.reject_reason)

    log.info("Listening %.0fs for EC frames...", args.seconds)
    await asyncio.sleep(args.seconds)

    # Teardown.
    for stop in stops:
        stop.set()
    for t in tasks:
        t.cancel()
    for t in tasks:
        try:
            await t
        except (asyncio.CancelledError, Exception):
            pass
    await adapter.logout()

    # ---- Summary --------------------------------------------------------
    print("\n" + "=" * 64)
    print("DIAGNOSTIC SUMMARY")
    print("=" * 64)
    print(f"placed order_number: {placed_order_number or '(none)'}")
    print(f"\n[step5/hook] (production EC stream) OrderEvents: {captured_via_hook}")
    for label, sink in sinks.items():
        ec_for_order = [t for t in sink.ec_order_numbers
                        if placed_order_number and t[0] == placed_order_number]
        print(f"\n[{label}] connected={sink.connected} ST.p_errno={sink.first_st_errno} "
              f"counts={dict(sink.counts)}")
        print(f"        all EC (p_NO,p_NT): {sink.ec_order_numbers}")
        if placed_order_number:
            print(f"        EC for placed order: {ec_for_order} "
                  f"{'<-- DELIVERS EC' if ec_for_order else ''}")
    print("\nVerdict: the shape(s) showing EC for the placed order are correct for")
    print("account-level EC. If only [step5/hook] fired, the current code is right.")
    print("=" * 64)
    return 0


if __name__ == "__main__":
    # Windows consoles default to cp932 and cannot encode em dashes / Japanese in
    # our log/help text — force UTF-8 so output never crashes mid-diagnostic.
    for _stream in (sys.stdout, sys.stderr):
        try:
            _stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(message)s",
        datefmt="%H:%M:%S",
    )
    log = logging.getLogger("diag")
    raise SystemExit(asyncio.run(main()))
else:
    log = logging.getLogger("diag")
