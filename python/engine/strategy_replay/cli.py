"""engine.strategy_replay.cli — CLI entry point for strategy replay.

Usage:
    python -m engine.strategy_replay run --strategy <path.py> --catalog <catalog_dir>
    python -m engine.strategy_replay run --help
"""
from __future__ import annotations

import argparse
import json
import logging
import sys
from pathlib import Path

log = logging.getLogger(__name__)


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m engine.strategy_replay",
        description="Replay a strategy against historical bar data and write a run-buffer.",
    )
    sub = parser.add_subparsers(dest="command", metavar="COMMAND")
    sub.required = True

    run_p = sub.add_parser(
        "run",
        help="Run a strategy replay and produce fills/equity/meta output.",
    )
    run_p.add_argument(
        "--strategy",
        required=True,
        metavar="PATH",
        help="Path to the strategy .py file (must contain SCENARIO and a Strategy subclass).",
    )
    run_p.add_argument(
        "--catalog",
        default=None,
        metavar="DIR",
        help="Path to the Nautilus catalog directory (required unless --bars-json is given).",
    )
    run_p.add_argument(
        "--bars-json",
        default=None,
        metavar="FILE",
        help=(
            "Path to a JSON file mapping instrument_id → list of bar dicts "
            "(for testing without a real catalog)."
        ),
    )
    run_p.add_argument(
        "--run-buffer-dir",
        default=None,
        metavar="DIR",
        help="Override the run-buffer output directory (default: %%APPDATA%%\\flowsurface\\run-buffer).",
    )
    run_p.add_argument(
        "--strategy-param",
        action="append",
        metavar="KEY=VALUE",
        dest="strategy_params",
        default=[],
        help="Override a strategy parameter (e.g. --strategy-param window=10). Repeatable.",
    )
    run_p.add_argument(
        "--granularity",
        default=None,
        metavar="GRANULARITY",
        help=(
            "Override SCENARIO granularity ('Daily' or 'Minute'). "
            "When omitted, the strategy file's SCENARIO value is used."
        ),
    )
    run_p.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Enable DEBUG logging.",
    )
    run_p.set_defaults(func=_cmd_run)
    return parser


def _parse_strategy_params(pairs: list[str]) -> dict[str, str]:
    result: dict[str, str] = {}
    for pair in pairs:
        if "=" not in pair:
            raise argparse.ArgumentTypeError(
                f"--strategy-param must be KEY=VALUE, got: {pair!r}"
            )
        k, v = pair.split("=", 1)
        result[k.strip()] = v.strip()
    return result


def _cmd_run(args: argparse.Namespace) -> int:
    if args.verbose:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")

    strategy_path = Path(args.strategy)
    if not strategy_path.exists():
        log.error("strategy file not found: %s", strategy_path)
        return 1

    # ── Load strategy ─────────────────────────────────────────────────────────
    from engine.strategy_runtime.strategy_loader import load, StrategyLoadError
    try:
        module, scenario, strategy_cls = load(strategy_path)
    except (FileNotFoundError, ValueError, StrategyLoadError) as exc:
        log.error("failed to load strategy: %s", exc)
        return 1

    log.info("loaded strategy: %s  scenario: %s", strategy_cls.__name__,
             scenario.get("instrument") or scenario.get("instruments"))

    # ── Granularity override ──────────────────────────────────────────────────
    if getattr(args, "granularity", None):
        from engine.strategy_runtime.catalog_data_loader import normalize_granularity
        try:
            scenario = dict(scenario, granularity=normalize_granularity(args.granularity))
        except ValueError as exc:
            log.error("invalid --granularity: %s", exc)
            return 1

    # ── Load bars ─────────────────────────────────────────────────────────────
    if args.bars_json:
        bars_by_instrument = _load_bars_from_json(args.bars_json)
        if bars_by_instrument is None:
            return 1
    elif args.catalog:
        bars_by_instrument = _load_bars_from_catalog(args.catalog, scenario)
        if bars_by_instrument is None:
            return 1
    else:
        log.error("either --catalog or --bars-json is required")
        return 1

    # ── Strategy kwargs ───────────────────────────────────────────────────────
    try:
        extra_params = _parse_strategy_params(args.strategy_params)
    except argparse.ArgumentTypeError as exc:
        log.error("%s", exc)
        return 1

    strategy_init_kwargs: dict = {}
    if extra_params:
        strategy_init_kwargs.update(extra_params)

    _inject_blacksheep_kwargs(
        strategy_cls=strategy_cls,
        strategy_path=strategy_path,
        catalog_path=args.catalog,
        kwargs=strategy_init_kwargs,
    )

    # ── RunBuffer ─────────────────────────────────────────────────────────────
    from engine.strategy_runtime.run_buffer import RunBuffer, make_run_id, get_run_buffer_base_dir

    instruments = scenario.get("instruments") or [scenario.get("instrument", "unknown")]
    first_instrument = instruments[0] if instruments else "unknown"

    run_id = make_run_id(str(strategy_path), first_instrument)
    base_dir = Path(args.run_buffer_dir) if args.run_buffer_dir else get_run_buffer_base_dir()

    rb = RunBuffer(
        run_id=run_id,
        strategy_file=str(strategy_path),
        scenario=scenario,
        base_dir=base_dir,
    )

    # ── Run ───────────────────────────────────────────────────────────────────
    from engine.strategy_runtime.engine_runner import run as engine_run
    from engine.strategy_runtime.summary import compute_summary, write_summary_json

    try:
        engine_run(
            strategy_cls=strategy_cls,
            scenario=scenario,
            bars_by_instrument=bars_by_instrument,
            run_buffer=rb,
            strategy_init_kwargs=strategy_init_kwargs or None,
        )
        rb.finish()
    except Exception as exc:
        log.error("replay failed: %s", exc, exc_info=args.verbose)
        rb.abort()
        return 1

    # ── Summary ───────────────────────────────────────────────────────────────
    summary = compute_summary(rb.run_dir)
    write_summary_json(rb.run_dir, summary)

    print(json.dumps({"run_id": run_id, "run_dir": str(rb.run_dir), **summary}, indent=2))
    return 0


def _load_bars_from_catalog(catalog_dir: str, scenario: dict):
    """Load bars from a Nautilus catalog directory."""
    from engine.strategy_runtime.catalog_data_loader import load_bars_for_scenario
    try:
        return load_bars_for_scenario(catalog_dir, scenario)
    except Exception as exc:
        log.error("failed to load bars from catalog: %s", exc)
        return None


def _load_bars_from_json(json_path: str):
    """Load bars from a pre-serialised JSON file (for testing / offline use).

    Expected format:
        {
          "1301.TSE": [
            {"ts_event": 0, "ts_init": 0, "open": "1000", "high": "1010",
             "low": "990", "close": "1005", "volume": "1000",
             "granularity": "Daily"},
            ...
          ]
        }

    This is intentionally minimal: each bar dict must have the fields above.
    """
    from decimal import Decimal
    from nautilus_trader.model.data import Bar, BarSpecification, BarType
    from nautilus_trader.model.enums import AggregationSource, BarAggregation, PriceType
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.objects import Price, Quantity

    _AGG_MAP = {"Daily": BarAggregation.DAY, "Minute": BarAggregation.MINUTE}

    try:
        raw = json.loads(Path(json_path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        log.error("failed to read --bars-json: %s", exc)
        return None

    result: dict = {}
    for symbol, bar_list in raw.items():
        iid = InstrumentId.from_str(symbol)
        bars = []
        for d in bar_list:
            granularity = d.get("granularity", "Daily")
            agg = _AGG_MAP.get(granularity, BarAggregation.DAY)
            bar_spec = BarSpecification(1, agg, PriceType.LAST)
            bar_type = BarType(iid, bar_spec, AggregationSource.EXTERNAL)
            precision = 1
            bars.append(Bar(
                bar_type=bar_type,
                open=Price(Decimal(str(d["open"])), precision=precision),
                high=Price(Decimal(str(d["high"])), precision=precision),
                low=Price(Decimal(str(d["low"])), precision=precision),
                close=Price(Decimal(str(d["close"])), precision=precision),
                volume=Quantity(int(d["volume"]), precision=0),
                ts_event=int(d["ts_event"]),
                ts_init=int(d["ts_init"]),
            ))
        result[iid] = bars
    return result


def _inject_blacksheep_kwargs(
    *,
    strategy_cls,
    strategy_path: Path,
    catalog_path: str | None,
    kwargs: dict,
) -> None:
    """Inspect strategy_cls.__init__ and inject warmup_loader / universe_json_path.

    Mutates *kwargs* in-place.  Only injects if the parameter exists in the
    signature AND it is not already set (by --strategy-param or env override).
    """
    import inspect

    try:
        sig = inspect.signature(strategy_cls.__init__)
    except (ValueError, TypeError):
        return

    params = set(sig.parameters)

    # ── warmup_loader ─────────────────────────────────────────────────────────
    if "warmup_loader" in params and "warmup_loader" not in kwargs:
        if catalog_path:
            from engine.strategy_runtime.warmup import make_catalog_warmup_loader
            kwargs["warmup_loader"] = make_catalog_warmup_loader(catalog_path)
            log.debug("injected catalog warmup_loader from %s", catalog_path)
        else:
            # No catalog available: inject a no-op so the strategy won't call
            # the default jquants_warmup_loader (unavailable in this repo).
            kwargs["warmup_loader"] = lambda *_a: []
            log.debug("injected no-op warmup_loader (no --catalog provided)")

    # ── universe_json_path ────────────────────────────────────────────────────
    if "universe_json_path" in params and "universe_json_path" not in kwargs:
        # Honour env override first (strategy code does its own env check, but
        # passing it as a kwarg makes the resolved value visible in logs).
        import os
        env_val = os.environ.get("STRATEGY_PARAM_UNIVERSE_JSON_PATH")
        if env_val:
            kwargs["universe_json_path"] = env_val
            log.debug("universe_json_path from env: %s", env_val)
        else:
            # Resolve from UNIVERSE_JSON_PATH defined in the strategy module.
            # strategy_loader already imported the module; get the attr.
            # We resolve relative to the strategy file's parent.
            try:
                import importlib
                mod_name = f"user_strategy_{strategy_path.stem}"
                # Module is already in sys.modules from strategy_loader.load()
                import sys as _sys
                mod = _sys.modules.get(mod_name)
                raw_path = getattr(mod, "UNIVERSE_JSON_PATH", None) if mod else None
                if raw_path:
                    from engine.strategy_runtime.universe import resolve_universe_json_path
                    resolved = resolve_universe_json_path(strategy_path, raw_path)
                    kwargs["universe_json_path"] = str(resolved)
                    log.debug("universe_json_path resolved: %s", resolved)
            except Exception as exc:
                log.debug("could not resolve universe_json_path: %s", exc)


def run_command(args: argparse.Namespace) -> int:
    """Public entry point for direct invocation (e.g. from tests with monkeypatching)."""
    return _cmd_run(args)


def main(argv: list[str] | None = None) -> None:
    parser = _build_parser()
    args = parser.parse_args(argv)
    sys.exit(args.func(args))
