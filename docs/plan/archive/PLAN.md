# Startup Cache Restore: Strategy Editor Fallback

## Summary
Fix startup cache restore so cached `app_state.py` fragments produce Strategy Editor window(s) even when `app_state.json` has `windows: []`, `windows` missing, or no Strategy Editor entry. The bug is that fragments are loaded into `PendingStrategyFragments`, but spawn requests are currently emitted only from `layout.windows`.

## Key Changes
- Update `apply_cache_restore_system` in `src/ui/layout_persistence.rs`.
- After loading/splitting `app_state.py`, detect whether restored `windows` contains any non-legacy `PanelKind::StrategyEditor`.
- If no Strategy Editor window is present, emit fallback `PanelSpawnRequested` for each parsed fragment.
  - Use `StrategyEditorSpawnSpec { region_key: Some(key), source: None, layout_source: PanelSpawnSource::LayoutLoad }`.
  - Let `panel_spawn_dispatcher_system` drain `PendingStrategyFragments.by_region_key` via the existing `source: None` path.
  - Insert the same `(PanelKind::StrategyEditor, Some(region_key))` key into `pending.spawn_requested` before sending, matching the normal layout restore dedupe behavior.
- Add a short code comment tying the fallback to `split_py_into_fragments`: unmarked Python currently becomes `region_001`, but the fallback must iterate actual splitter output rather than hard-code that key.
- Inspect layout save paths that write cache `app_state.json` (`debounced_autosave_system`, `save_layout_on_window_close`, explicit save paths) and document whether `windows: []` is expected when no restorable floating windows exist. Only change save behavior if inspection shows Strategy Editor windows are being accidentally excluded while present.

## Test Plan
- Add focused tests around the cache-restore fallback behavior:
  - `windows: Some(vec![])` + cached fragment emits one Strategy Editor spawn request with `source: None`.
  - `windows: None` + cached fragment emits fallback spawn request(s).
  - `windows` with only non-editor panels still emits Strategy Editor fallback.
  - `windows` containing Strategy Editor does not emit fallback and relies on dispatcher drain.
  - A dispatcher-level regression check verifies `source: None` drains `PendingStrategyFragments`, preventing stale fragment pollution.
- Run:
  - `cargo test -p backcast --lib ui::layout_persistence`
  - `cargo test -p backcast --lib ui::floating_window`
  - `cargo check`

## Assumptions
- A cached `app_state.py` means startup should expose at least one editable Strategy Editor.
- `windows: []` remains valid layout data for “no saved floating windows,” but cache restore treats cached strategy source as stronger evidence that an editor should be opened.
- Chart panels remain scenario/instrument-driven and are not restored through this fallback.
