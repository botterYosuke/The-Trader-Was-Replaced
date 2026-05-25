---
status: accepted
---

# Remove the "Promote to Live" UI entry but keep the Live-Auto launch RPCs

The dedicated **"Promote to Live" button + Safety Rails modal** (and the Rust transport
they drove: `TransportCommand::PromoteToLive`, `SafetyLimitsInput`, `PromoteFeedback`, the
`BackendStatusUpdate::LiveStrategyPromoteResult` seam, and the `n5` E2E flow) are removed
in issue #40. The backend launch chain (`RegisterLiveStrategy` → `SetExecutionMode(LiveAuto)`
→ `StartLiveStrategy`, the proto messages, `server_grpc.py` handlers, `_build_safety_rails`,
and the whole Live execution engine — `safety_rails.py` / exec client / engine_controller /
Live Run Panel / N1–N4) is **kept**. `ExecutionMode::LiveAuto` and the footer "Auto" segment
also stay selectable. The result is a transitional state: Auto mode has **no UI launch entry**
until the launch is rewired onto the footer transport (play ▶) button (switch to Auto →
`File ▸ Open` a strategy → ▶), tracked as a follow-up.

## Why this is recorded

A reader will otherwise find `RegisterLiveStrategy` / `StartLiveStrategy` RPCs (and a fully
wired exec engine) with **no caller anywhere in the UI**, and an "Auto" mode that cannot be
started, and reasonably assume it is dead code to delete. It is not: it is preserved on
purpose for the imminent footer-play rewiring.

## Considered options

- **Remove the Promote entry, keep the launch RPCs (chosen).** No churn — the follow-up that
  wires footer-play → Live Auto reuses the existing RPCs and `safety_rails.py`. Cost: a window
  where the RPCs have no caller.
- **Remove Promote *and* the launch RPCs (the literal issue #40 Slice 3).** Rejected: the
  follow-up would have to re-add the proto messages, Python handlers, and tests we just deleted.
- **Also rewire footer-play → Live Auto in this same change.** Rejected for scope: it ungated
  `footer_pause_resume_system` (Replay-only today, with tests asserting the play button is a
  no-op in Auto) and is a feature in its own right, not part of removing the Promote entry.

`safety_rails.py` is *not* Promote-specific — it is the exec client's required pre/post-trade
risk gate — so it is out of scope for removal regardless.

## Follow-up status (resolved)

The transitional gap above is now closed. The footer transport **▶** button launches Live Auto in
`ExecutionMode::LiveAuto` by emitting `TransportCommand::StartLiveAuto`, which the transport task
serializes into `RegisterLiveStrategy` → `StartLiveStrategy` — reusing the preserved RPCs and
`default_live_auto_safety_limits` (AC#8 defaults). The ▶ does **not** re-send `SetExecutionMode`
(mode stays backend-authoritative). Pre-flight gates guard the send: scenario instruments present, venue live, venue identity set
(`venue_id` or `configured_venue`), and the strategy cache flushed. The launch instrument is
scenario-derived to match Replay Run; sidebar selection is only a tiebreaker for multi-instrument
scenarios. Covered by E2E flows **N5** (command-level, `n5_footer_play_starts_live_auto.rs`, kind:ui) and
**N6** (real `spawn_footer` integration, `n6_footer_play_starts_live_auto_via_real_footer.rs`, kind:ui)
plus footer visibility unit tests
(PauseResume ▶ visible in Replay | LiveAuto, hidden in LiveManual). The "Auto mode has no UI launch
entry" window described above no longer applies.
