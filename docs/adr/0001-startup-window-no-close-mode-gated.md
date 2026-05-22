# Startup window has no close button and is gated to Replay mode

The Startup window (replay-run parameters) is built like every other floating
window — `spawn_floating_window`, shared title bar, world-space fields as in the
Strategy Editor. It deviates in two deliberate ways: it has **no × close
button**, and its visibility is **owned by `ExecutionMode`** (shown only in
Replay), rather than being user-spawned from a sidebar button and user-closable.

Recorded because both are departures a future engineer would otherwise "fix":
adding a × or a sidebar toggle would let the user dismiss the only way to
configure a replay run, and showing it outside Replay has no meaning. The ×
button is suppressed via a `closeable` flag on `spawn_floating_window` rather
than a forked spawn path.

Startup is a `PanelKind` so its position/z persist through the existing
`WindowLayout` machinery, but it is special-cased *out* of the conventions every
other `PanelKind` follows: no sidebar button, not user-spawned, not user-closable.
Consequently `WindowLayout.visible` is **not** authoritative for Startup —
`ExecutionMode` owns its visibility, so a restored `visible` value must not
override the Replay gate.
