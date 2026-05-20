---
name: nautilus-trader
description: |
  Authoritative development helper for the **nautilus_trader** framework — the core engine of
  this project (The-Trader-Was-Replaced). Use this skill whenever the user is working with
  nautilus_trader APIs, even if they don't name it explicitly: Actors, Strategies, the
  message bus, the data engine, clocks/timers, bar/quote/trade data types, instruments,
  `BacktestEngine` / `BacktestNode` / `BacktestEngineConfig`, `TradingNode` / `LiveExecEngine`,
  `NautilusKernel`, ParquetDataCatalog, indicators, custom data, adapters, or anything in
  `python/engine/nautilus_*.py`. Also trigger on related vocabulary: "msgbus", "ts_event",
  "ts_init", "InstrumentId", "ClientId", "Venue", "BarSpec", "OrderFactory", "ExecAlgorithm",
  "PositionEvent", "OrderEvent", "cache" in a trading sense, "Cython .pyx", "PyO3".
  The full upstream source tree is mirrored at `.claude/skills/nautilus_trader/src/` — use it
  as ground truth instead of guessing API shapes from memory. The current branch
  (`sasa/Phase-6---Nautilus-Replay-Integration`) is actively wiring nautilus data types into
  the project's replay pipeline, so this skill is in heavy use.
---

# nautilus_trader development helper

nautilus_trader is a Rust-native, event-driven trading engine with a Python control plane.
Same execution semantics across **backtest**, **sandbox**, and **live** — strategies move
between contexts without code changes. This skill exists because the API surface is large,
Cython-heavy, and easy to misremember, and because this project is mid-integration on Phase 6.

## First rule: read the source, don't guess

The full upstream codebase is checked into `.claude/skills/nautilus_trader/src/`. **Treat it
as ground truth.** Before claiming an API exists or has a certain signature, grep the source.
Common misses without doing this:

- Confusing `nautilus_trader.common.actor.Actor` with `nautilus_trader.trading.strategy.Strategy`
  (Strategy ⊂ Actor; Strategy adds order/position management).
- Forgetting that most domain types are Cython (`.pyx` / `.pxd`) so signatures live in `.pxd`
  files and editor go-to-definition can mislead.
- Using stale event names (e.g. `OrderFilled` is in `nautilus_trader.model.events`, not
  `nautilus_trader.execution.events`).

Always check the actual file. The relevant subtrees:

| Concern                                    | Path under `.claude/skills/nautilus_trader/src/` |
|--------------------------------------------|--------------------------------------------------|
| Strategies, Trader, Controller             | `nautilus_trader/trading/`                       |
| Actor, Component, Clock, MessageBus interfaces | `nautilus_trader/common/`                    |
| Domain model (bars, ticks, orders, events) | `nautilus_trader/model/`                         |
| Data engine, aggregation, custom data      | `nautilus_trader/data/`                          |
| Execution engine, order pipeline           | `nautilus_trader/execution/`                     |
| Cache (state store)                        | `nautilus_trader/cache/`                         |
| Backtest engine + node                     | `nautilus_trader/backtest/`                      |
| Live node, async loop, reconciliation      | `nautilus_trader/live/`                          |
| NautilusKernel (shared system bootstrap)   | `nautilus_trader/system/kernel.py`               |
| Persistence / ParquetDataCatalog           | `nautilus_trader/persistence/`                   |
| Adapters (Binance, IB, Databento, …)       | `nautilus_trader/adapters/`                      |
| Conceptual docs (Markdown)                 | `docs/concepts/`                                 |
| API reference (Markdown)                   | `docs/api_reference/`                            |
| Runnable examples                          | `examples/backtest/`, `examples/live/`           |

When you need a working pattern (custom data, msgbus pub/sub, clock timer, bar aggregation,
indicator, etc.), the `examples/backtest/example_01..11_*` folders are the fastest reference.
Read one before designing from scratch.

## Project context: where nautilus_trader lives in this repo

This project doesn't *embed* a full `BacktestEngine` yet. Instead it has its own
deterministic replay pipeline (`python/engine/`) and is incrementally adopting nautilus types
as the canonical data representation:

- `python/engine/nautilus_adapter.py` — converts nautilus `Bar` / `TradeTick` → project
  `KlineUpdate` / `TradeUpdate` / `ReplayTimeUpdated`.
- `python/engine/nautilus_runner.py` — iterates an `Iterable[Bar|TradeTick]` through the
  adapter into a `ReplayEventSink`. Critical invariant: **always emit
  `ReplayTimeUpdated` before the data event** for each tick, so the reducer sees time advance
  before state mutates.
- `python/engine/reducer.py` — the in-process state reducer. Discards stale-timestamp events.
- `python/engine/core.py` — `DataEngine` (project-level, not the nautilus `DataEngine`). Owns
  a `ReducerState`, primes from a `BaseReplayProvider`, and exposes `apply_replay_event`.

When extending Phase 6 work:

- **Don't shadow nautilus names.** This project has its own `DataEngine` in
  `python/engine/core.py`. If you need the nautilus one, import it as
  `from nautilus_trader.data.engine import DataEngine as NautilusDataEngine` and say so.
- **Preserve the `ts_event` ordering invariant.** Anything that produces replay events for
  the project reducer must emit `ReplayTimeUpdated(ts_event_ns)` before the corresponding
  `KlineUpdate` / `TradeUpdate`. This is enforced by `NautilusReplayRunner`; new entry
  points must replicate it.
- **Nanoseconds vs milliseconds.** Nautilus uses `uint64` nanoseconds end-to-end (`ts_event`,
  `ts_init`). This project's reducer is milliseconds (`timestamp_ms`). The adapter is the
  one place this conversion happens — keep it there.

## Common tasks: where to look first

When the user asks for one of these, open the listed reference *before* writing code.

| Task                                                 | Open this first                                             |
|------------------------------------------------------|-------------------------------------------------------------|
| Build a new strategy                                 | `docs/concepts/strategies.md` + any `examples/backtest/fx_ema_cross_*.py` |
| Build an Actor (no orders, just data/signals)        | `docs/concepts/actors.md` + `examples/backtest/example_10_messaging_with_actor_data/` |
| Publish/subscribe over the message bus               | `docs/concepts/message_bus.md` + `examples/backtest/example_09_messaging_with_msgbus/` |
| Use a Clock / Timer                                  | `examples/backtest/example_02_use_clock_timer/`             |
| Aggregate bars from ticks                            | `examples/backtest/example_03_bar_aggregation/`             |
| Load data from a custom CSV                          | `examples/backtest/example_01_load_bars_from_custom_csv/`   |
| Use the Cache                                        | `examples/backtest/example_06_using_cache/`                 |
| Use Portfolio                                        | `examples/backtest/example_05_using_portfolio/`             |
| Indicators                                           | `examples/backtest/example_07_using_indicators/` + `nautilus_trader/indicators/` |
| Custom data type                                     | `docs/concepts/custom_data.md`                              |
| Stand up a BacktestEngine directly                   | `docs/getting_started/backtest_low_level.py`                |
| Use BacktestNode + ParquetDataCatalog                | `docs/getting_started/backtest_high_level.py`               |
| Wire a live venue                                    | `examples/live/<venue>/` and `docs/integrations/`           |
| Add a new adapter                                    | `docs/concepts/adapters.md` + an existing small adapter (e.g. `adapters/sandbox`) |

For deeper background on a single subsystem, the `docs/concepts/*.md` files (architecture,
data, events, execution, orders, positions, portfolio, cache, message_bus, logging, dst) are
short and high-signal. Prefer them over the API reference for *understanding*; use the API
reference (`docs/api_reference/`) only after you know what you're looking for.

## API gotchas worth internalizing

These bite people repeatedly. Worth holding in working memory rather than rediscovering:

- **There is NO `StrategyEngine`.** Nautilus has `DataEngine`, `ExecutionEngine`, and
  `RiskEngine` — but strategies are managed by the **`Trader`** (`nautilus_trader/trading/trader.py`,
  used by both `BacktestEngine` and live), and the live system host is **`TradingNode`**
  (`nautilus_trader/live/node.py`, wraps `NautilusKernel` + `Trader` + the Live*Engines).
  You add a strategy with `engine.add_strategy(...)` / `trader.add_strategy(...)`, not by
  "enabling a StrategyEngine". Plan/design docs in this repo keep inventing a `StrategyEngine`
  — flag it on sight and replace with `Trader` / `TradingNode`.
- **A strategy's `self.clock` / `self.cache` / `self.msgbus` are injected at `register()`,
  NOT via `StrategyConfig`.** See `common/actor.pyx` (`self.cache = None # Initialized when
  registered`, set in `register()`). `StrategyConfig` carries instrument/venue/params only.
  This is *why* the same strategy runs unchanged across backtest/live — the engine supplies
  the clock/data/exec, the strategy never branches on mode. Don't describe portability as
  "inject clock/data_engine via config" — that's not how it works.
- **Bar aggregation lives in `data/aggregation.pyx`** (`BarBuilder`, `BarAggregator`,
  `TimeBarAggregator` / `TickBarAggregator` with `handle_trade_tick(TradeTick)`). `BarType`'s
  5th segment (`INTERNAL`/`EXTERNAL`) decides whether the engine aggregates from ticks or
  trusts an external bar feed. Note: this project *also* has its own `TickBarAggregator` in
  `python/engine/live/aggregator.py` that emits the project's `KlineUpdate` dataclass for the
  UI — that one is NOT a Nautilus `BarBuilder` wrapper; don't conflate the two.
- **Cython types use class-only construction.** You generally can't subclass `Bar`, `TradeTick`,
  `Quote Tick`, `OrderFilled`, etc. and add attributes. If you need extra state, store it in
  the cache or attach via msgbus topics. (To tag the *origin* of an order, use its
  `StrategyId` or order `tags`, not a bolted-on field.)
- **`ts_event` vs `ts_init`.** `ts_event` is when the event happened in the market;
  `ts_init` is when nautilus constructed the object. Replay/ordering logic must key on
  `ts_event`. Logging may want `ts_init`.
- **Strategy `on_start` runs before any data flows.** Subscriptions belong in `on_start`,
  not `__init__` — the message bus and data engine aren't fully wired in the constructor.
- **`self.clock.set_time_alert` / `set_timer`** are how strategies schedule callbacks; do
  **not** use `time.sleep`, `asyncio.sleep`, or wall-clock APIs inside a strategy. That
  breaks backtest determinism.
- **Order submission is async even in backtest.** `self.submit_order(order)` enqueues; the
  matching engine processes it on the next event. Don't read fill state in the same callback.
- **Logging.** Use `self.log.info(...)` etc. from inside an Actor/Strategy; never `print` or
  `logging` directly — those bypass the structured log and the in-memory log buffer used by
  tests.
- **Identifiers are value types.** `InstrumentId`, `ClientId`, `Venue`, `StrategyId`,
  `ClientOrderId`, `PositionId` — construct from strings with the factory (`InstrumentId.from_str("AAPL.NASDAQ")`),
  don't pass raw strings to APIs expecting them.
- **Bar specifications.** `BarType.from_str("AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL")` — the
  fifth segment (`EXTERNAL` / `INTERNAL`) decides whether nautilus aggregates the bars from
  ticks or trusts an external feed. Mismatch is a common silent bug.

## How to research an API question

When the user asks "how do I do X with nautilus", follow this in order:

1. **Grep the source.** `Grep -r "<symbol or phrase>" .claude/skills/nautilus_trader/src/nautilus_trader/` —
   you'll usually land on either the implementation or a docstring with a working example.
2. **Check `examples/`.** If grep didn't yield a runnable pattern, scan
   `examples/backtest/` / `examples/live/` for the closest analogue and adapt.
3. **Read the matching `docs/concepts/<topic>.md`.** Concept docs are short and explain
   *why* the API is shaped the way it is — important for not fighting the framework.
4. **Only then suggest code.** State which source file you confirmed against
   (`nautilus_trader/.../foo.pyx:LNN`) so the user can verify.

Skipping step 1 produces plausible-but-wrong API calls — Cython signatures and event names
in particular drift between versions, and this skill's mirror reflects exactly the version
this project depends on.

## When working on Phase 6 replay integration

Specific to the current branch (`sasa/Phase-6---Nautilus-Replay-Integration`):

- Tests covering the adapter/runner live in `python/tests/test_nautilus_adapter_engine.py`
  and `python/tests/test_nautilus_runner.py`. Run these first whenever editing
  `python/engine/nautilus_*.py` — they pin the `ReplayTimeUpdated → data-event` invariant.
- The adapter intentionally does **not** depend on a running `NautilusKernel` or any nautilus
  engine — it operates on pure data objects (`Bar`, `TradeTick`). Keep it that way; if a
  conversion needs context, push the context into the call site, not into a kernel reference.
- The eventual goal (later phases) is to feed the project reducer from a real
  `BacktestDataEngine` or a `LiveDataEngine`, replacing the bespoke `JQuants*ReplayProvider`.
  Designs should leave room for that without forcing it now.

## Output expectations

When answering nautilus_trader questions:

- Cite the file you confirmed the API against — `path/to/file.pyx:line` — so the user can
  click through. Don't quote large blocks; a precise pointer is more useful.
- Prefer the smallest working snippet over a full strategy class.
- If two APIs could plausibly satisfy the request (e.g. Actor vs Strategy, msgbus publish
  vs cache write), name the tradeoff in one sentence and let the user choose, rather than
  silently picking.
- If the user is mid-Phase-6 work, frame answers in terms of `python/engine/` integration
  points, not standalone nautilus examples.
