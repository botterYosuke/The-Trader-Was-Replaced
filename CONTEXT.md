# The-Trader-Was-Replaced — UI Windowing

The trader dashboard renders an infinite canvas of windows over a Bevy app. This
context covers the vocabulary for the on-canvas windows and the Startup
parameter form.

## Language

**Floating window**:
A world-space *sprite* window built by `spawn_floating_window` — draggable by its
title bar, z-ordered among other floating windows. Chart, Strategy Editor,
Buying Power, Run Result, Positions, and Orders are all floating windows, and
the Startup window is built the same way. Editable text lives in world space too
(the Strategy Editor hosts a `cosmic-edit` buffer this way).
_Avoid_: panel, dialog.

**Startup window**:
The form for configuring a replay run — Start date, End date, Granularity, and
Initial cash. A floating window with two deliberate departures from the others:
it has **no close button**, and it is **shown only in Replay mode** (its
visibility is owned by `ExecutionMode`, not by the user or a sidebar button).
_Avoid_: Startup panel, scenario panel, run config dialog.

**Title bar**:
The sprite drag region every floating window shares via `spawn_floating_window`;
also the host for the × close button on windows that have one.
_Avoid_: header.

**Close button (×)**:
The per-window dismiss control on the title bar. Present on every floating
window *except* the Startup window. Suppressing it is a per-window choice.

**Replay mode**:
The `ExecutionMode` in which the dashboard runs a backtest over a date range, as
opposed to LiveManual / LiveAuto. The Startup window exists only here.
_Avoid_: backtest mode, sim mode.

## Example dialogue

> **Dev:** Should the Startup window get a close button like the other windows?
> **Expert:** No — it's the one floating window without one. Replay mode owns
> when it shows; the user drags it but can't dismiss it. Closing it would strand
> the only way to configure a replay run.
> **Dev:** But it's built the same way as Buying Power?
> **Expert:** Yes — same `spawn_floating_window`, same title bar. The fields are
> hosted in world space exactly like the Strategy Editor's editable text.
