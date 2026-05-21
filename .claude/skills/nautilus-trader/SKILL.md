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
  - **There is NO Python log sink — you cannot tap `self.log.*` into Python.** `Logger.{info,
    warning,error}` route straight into the Rust subsystem (`nautilus_pyo3.logger_log`), `init_logging`
    only writes stdout/file (no callback param), the strategy's `_log` is `cdef readonly`, and `Logger`
    is a non-monkeypatchable extension type. So if you need to forward strategy log lines to an external
    consumer (a UI, a gRPC stream), do **not** try to intercept `self.log.*` — instead publish on a
    **msgbus topic** (`self.msgbus.publish("strategy.log.{id}", record)`) via a small helper that also
    mirrors to `self.log.<level>`, and subscribe to that topic on the kernel side (`kernel.msgbus.subscribe`).
    This is the same proven seam as bridging `events.order.{strategy_id}`. (Phase 10 §570 / `python/engine/live/strategy_log.py`.)
- **Identifiers are value types.** `InstrumentId`, `ClientId`, `Venue`, `StrategyId`,
  `ClientOrderId`, `PositionId` — construct from strings with the factory (`InstrumentId.from_str("AAPL.NASDAQ")`),
  don't pass raw strings to APIs expecting them.
- **Bar specifications.** `BarType.from_str("AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL")` — the
  fifth segment (`EXTERNAL` / `INTERNAL`) decides whether nautilus aggregates the bars from
  ticks or trusts an external feed. Mismatch is a common silent bug.

### Live execution stack gotchas (custom `LiveExecutionClient` / live `NautilusKernel`)

Hit while building Phase 10 Step 4 (`python/engine/live/nautilus_exec_client.py` +
`engine_controller.py`, nautilus 1.226.0). These bite anyone wrapping a bespoke venue
adapter as a live client:

- **Minimal live stack = `NautilusKernel` directly, no `TradingNode` needed.**
  `NautilusKernel(name=..., config=TradingNodeConfig(trader_id, logging, exec_engine=LiveExecEngineConfig(), risk_engine=LiveRiskEngineConfig(...), data_engine=LiveDataEngineConfig()), loop=loop)`
  gives you `kernel.{trader,exec_engine,risk_engine,data_engine,cache,portfolio,msgbus,clock}`.
  Then `cache.add_instrument(...)` → `exec_engine.register_client(client)` → `trader.add_strategy(...)`
  → `kernel.start()` / `await kernel.stop_async()`. `TradingNode` only adds client-factory
  wiring + signal handling on top.
- **`OrderDenied` is only a valid transition from `INITIALIZED`.** A custom pre-trade rail in
  `LiveExecutionClient._submit_order` must call `generate_order_denied(...)` **before**
  `generate_order_submitted(...)`. Deny after submit → `SUBMITTED → DENIED` is rejected and the
  order sticks at SUBMITTED. (Native `LiveRiskEngineConfig` rails — `max_notional_per_order`
  dict, `max_order_submit_rate="N/HH:MM:SS"` — deny before the client is reached, so they're fine.)
- **A live client must set its `account_id` before `generate_account_state`.** In `_connect`:
  `if self.account_id is None: self._set_account_id(AccountId(f"{venue}-001"))`, then
  `generate_account_state(balances=[AccountBalance(total, locked, free)], margins=[], reported=True, ts_event=...)`.
  Without an account, RiskEngine free-balance checks deny orders.
- **Live forbids `LoggingConfig(bypass_logging=True)`** (`InvalidConfiguration`). Use
  `LoggingConfig(log_level="ERROR", log_level_file="OFF")` to avoid littering `*.log` in cwd.
  The live kernel initializes Nautilus's process-global logger once, which then defeats a
  backtest's `bypass_logging=True` in the **same process** (combined test runs leak backtest
  dispose logs; production unaffected since replay/live are separate processes).
- **Live-loop self-deadlock.** Code on the live asyncio-loop thread (e.g. an account-sync
  callback) must NOT call anything that does `run_coroutine_threadsafe(coro, loop).result()`
  targeting that *same* loop (e.g. tearing down via `kernel.stop_async()`). Offload to a
  `threading.Thread`.
  - **Offloading to a thread is necessary but NOT sufficient** (Phase 10 Step 4 review). If the
    offloaded worker holds a lock *across* its blocking `.result()`, and a live-loop callback
    also acquires that lock, the deadlock just moves: live loop blocks on the lock → can't run
    the coro → worker waits until `.result(timeout=…)` fires (leaving a runaway kernel
    un-torn-down). **Invariant: no live-loop callback may acquire a lock that is held across a
    blocking round-trip to that loop.** Fix: give the live-loop-reachable state its own
    lightweight lock, never held across a round-trip; keep the heavy lifecycle lock (held during
    start/stop/teardown) off the live-loop path entirely.
- **Cancel a strategy's orders by `strategy.id`, not by an instrument attribute.** To cancel a
  run's in-flight orders, query the cache (`cache.orders_open(strategy_id=s.id)` +
  `cache.orders_inflight(strategy_id=s.id)`) and `strategy.cancel_order(o)` each — don't rely on
  `strategy.instrument_id` (the base `Strategy` has no such attribute; kwargs/config strategies
  store it privately, so an attribute-based cancel silently no-ops). `cancel_all_orders` requires
  an `InstrumentId` and also sweeps emulated/exec-algo orders if you use those.

### Live data client + INTERNAL bar aggregation (feeding `on_bar` from venue ticks)

Hit while building Phase 10 Step 8 (`python/engine/live/nautilus_data_client.py` +
`bar_supply.py` + `engine_controller.py`). To make a strategy's `on_bar` fire in live mode you
must supply ticks to the engine — there is **no** automatic feed:

- **A strategy subscribing an `INTERNAL` `BarType` needs a registered `MarketDataClient` for that
  venue, or `on_bar` never fires — silently.** When the strategy calls
  `self.subscribe_bars(<...-INTERNAL>)`, `DataEngine._handle_subscribe_bars` →
  `_start_bar_aggregator` creates a `TimeBarAggregator(handler=self.process)` **and** issues a
  `SubscribeTradeTicks` (LAST price type) / `SubscribeQuoteTicks` to the venue's client
  (`data/engine.pyx:_subscribe_bar_aggregator`). If no data client is registered for that venue,
  the subscribe command is dropped with only a log line and the aggregator is never wired. So you
  must `kernel.data_engine.register_client(your_client)` **before** `kernel.start()` (the strategy's
  `on_start` runs during start and subscribes there).
- **A minimal custom `LiveMarketDataClient` is almost all no-ops.** Implement `_connect`/`_disconnect`
  (no-op if a sibling session already owns the venue connection), and the `_subscribe_trade_ticks` /
  `_unsubscribe_trade_ticks` (and quote variants) **coroutines as no-ops** — the base `subscribe_*`
  sync wrappers already record the subscription set, and INTERNAL aggregation is driven engine-side,
  not by the client. The client only needs a way to push ticks in.
- **Push ticks via `self._handle_data(tick)`.** `MarketDataClient._handle_data` is just
  `self._msgbus.send("DataEngine.process", data)` (`data/client.pyx`) — it does **not** depend on
  connection state or on the trade-tick subscription having been processed. As long as the kernel is
  started and the strategy subscribed the bar (so the aggregator is on the trades topic), feeding a
  `TradeTick` flows: `_handle_data` → engine routes to `data.trades.{id}` → `aggregator.handle_trade_tick`
  → builds → `on_bar`. Call it on the live-loop thread.
- **Time bars close on the `LiveClock` timer, not on tick `ts_event`.** Feeding ticks only updates the
  in-progress builder; the bar is emitted when the aggregator's clock timer fires. So a deterministic
  full-path unit test of a 1-MINUTE bar can't observe a close without waiting wall-clock — use a
  `1-SECOND-...-INTERNAL` bar + a ~2s real settle, and assert on the bar with `volume>0` (the
  `DataEngineConfig.time_bars_build_with_no_updates=True` default also emits empty carry-forward bars).
  Pure OHLCV-aggregation correctness is better pinned with a standalone `TimeBarAggregator` + `TestClock`
  (advance + `event.handle()`), independent of the engine.

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
