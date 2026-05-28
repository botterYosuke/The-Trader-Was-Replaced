---
status: accepted
---

# Merge the Live Run panel into a single mode-scoped Run Result

We replace the two separate run-status surfaces — the Replay-only **Run Result**
floating window (backed by `LastRunResult`) and the live-only **LIVE RUNS** Bevy
UI-Node panel (backed by `LiveRuns`, `live_run_panel.rs`) — with **one** display-only
Run Result floating window backed by a unified **`CurrentRun`** resource (renamed from
`LastRunResult`, with a superset `RunState`: `Idle | Running | Paused | Completed |
Stopped | Failed{error}`). The dashboard is always in exactly one `ExecutionMode`, so
exactly one Run is current; Run Result is mode-scoped (a Replay Run's outcome in Replay,
the Live Run in Auto). `LiveRuns` and `live_run_panel.rs` are removed.

Both seams feed `CurrentRun`: the replay seam (`RunComplete` / `RunFailed`) and the
live seam (`LiveStrategyEvent` / `LiveStrategyTelemetry`, plus the
`RegisterLiveStrategy` / `StartLiveStrategy` reject). Live launch failures route into
`Failed{error}` exactly like replay's `RunFailed`, instead of being only `error!`-logged
in the transport task (the issue #42 "▶ does nothing" symptom).

## Why this is recorded

A future reader will find `LiveRuns` + `live_run_panel.rs` deleted and live run status
flowing through a resource named after the replay path, and would otherwise wonder where
the live panel went and why live errors reuse a "replay" surface. It is deliberate: the
two surfaces were unified because the dashboard only ever runs one Run at a time, and a
single always-visible (per #41) surface makes launch failures un-missable.

## Deliberate proto change

`RegisterLiveStrategyRes` and `StartLiveStrategyRes` gain an `error_message` field
(mirroring `StartEngineResponse`). The backend currently normalizes every load failure to
the opaque `error_code = STRATEGY_LOAD_FAILED` and **swallows the underlying exception**
(`strategy_registry.register` does `raise StrategyRegistryError(...) from exc` but only
the code is returned). Surfacing `error_message` lets Run Result show the real cause
(e.g. `SyntaxError` at line N in the cached strategy) rather than just the symptom code.

## Boundaries (what stays where)

- **Run control** (start / pause / resume / stop) stays on the footer ▶ (ADR-0004 / #40);
  Run Result carries no controls.
- **Visibility**: Run Result is always-visible (#41); this change does not add auto-show.
- **Strategy Log**: the `StrategyLogs` stream stays its own resource (orthogonal to
  run *status*); it is shown in Run Result for both modes (replay gains
  `StrategyLogMessage` streaming so the log fills in batch on completion).

## Considered options

- **Keep two panels / two resources (status quo).** Rejected: live launch failures are
  invisible (only `error!`-logged), and two surfaces duplicate the "what is my run doing"
  question across modes.
- **Single mode-scoped Run Result + unified `CurrentRun` (chosen).** One surface, one
  resource, errors un-missable, maximal reuse of the replay `RunFailed → Failed{error}`
  path. Cost: rename churn (`LastRunResult` → `CurrentRun`), a proto field addition, and
  removing the live panel's wiring.
