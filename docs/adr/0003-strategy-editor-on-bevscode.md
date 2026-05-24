---
status: superseded
superseded_by: 0003-strategy-editor-on-bevy-ui-text-input.md
---

> **⚠️ SUPERSEDED (2026-05-24)** by
> [`0003-strategy-editor-on-bevy-ui-text-input.md`](0003-strategy-editor-on-bevy-ui-text-input.md).
> The bevscode / big-bang plan below was rejected in favour of a staged Bevy 0.16→0.17→0.18
> migration with `bevy_ui_text_input` (screen-space) and the accepted syntax-highlight /
> world-space-editor regressions. This file is retained for the decision trail only; the two
> `0003-*` files share a number by accident (collision is intentional-historical, not a live ADR).

# Strategy Editor moves to bevscode; project upgrades to Bevy 0.18 (SUPERSEDED)

We are replacing the in-house `bevy_cosmic_edit`-based Strategy Editor with
[bevscode](https://github.com/PoHsuanLai/bevscode) (a GPU-instanced-text code-editor
plugin set). bevscode targets **Bevy 0.18**, so the whole project is upgraded from
**0.15 → 0.18** first as a prerequisite. We keep the Strategy Editor a **world-space
floating window** (consistent with every other window per `CONTEXT.md`), which means
bevscode's UI-node text rendering must be driven in a world-space context. `cosmic_edit`
is removed entirely. (Porting the 0.15 cosmic fork to 0.18, or migrating to
another 0.18-compatible cosmic release, are the logical alternatives; we choose removal so
that the 0.18 upgrade and the cosmic removal land together in one green build rather than
maintaining a cosmic port.) The Startup window's three text fields (Start / End / InitialCash) move to the existing
keyboard-event-drain input pattern, while the Granularity choice keeps its existing button UX. Syntax highlighting switches from syntect to **tree-sitter-python**; whether bevscode bundles it natively or it needs to be added as a separate dependency (and wired to bevscode's highlight API) is confirmed in the Slice 0 spike. The custom editor features not provided by
bevscode — find/replace, scrollbar, tab-to-spaces/autoindent, bracket-autoclose — are
**dropped** rather than re-wired; the editor falls back to bevscode's built-ins
(multi-cursor, folding, bracket-match, line-number gutter, undo/redo, LSP).

## Considered options

- **Upgrade to 0.18 then adopt bevscode (chosen).** Sustainable: tracks upstream bevscode
  and a current Bevy. Largest blast radius (the engine upgrade dwarfs the editor swap).
- **Backport bevscode to Bevy 0.15.** Rejected: would mean owning a fork of bevscode's
  render layer (instanced text) on an old engine — permanent maintenance, no upstream.
- **Keep cosmic_edit, just add features.** Rejected: doesn't deliver the bevscode/VSCode-style
  editor the change is about.

## Consequences

- **Primary risk:** bevscode renders text via Bevy UI nodes; forcing it into world-space (so the editor still pans/zooms with the infinite canvas) is unproven. The first implementation task is a **world-space render spike on `bevy_instanced_text`**. The spike gate is not draw-only: it must also confirm click-to-caret, typed input, scroll-wheel exclusivity, picking coordinates after PanCam pan+zoom, and floating-window drag/resize/clipping/visibility (M12 Manual hide)/z-order. If any of these can't be made to work under a `Camera2d` without forking bevscode's render pipeline, this decision must be revisited.
- The 0.18 upgrade touches `main.rs`, `camera.rs`, `grid.rs`, all of `src/ui/**`, and the
  `tests/e2e` harness (the `Parent→ChildOf`, `get_single→single`, `Trigger::entity→target`
  deltas in the bevy-engine skill are exactly the 0.15→0.18 breakage).
- `bevy_pancam` and `bevy_vector_shapes` both have 0.18-compatible releases (0.20 / 0.12),
  so no further forks are required there.
- **Big-bang upgrade caveat (dev-environment, not a repo constraint):** an external linter/watcher in this development environment is known to revert uncommitted edits while `cargo build --lib` is non-compiling (documented in the `bevy-engine` skill, troubleshooting section). This is an operational property of the local toolchain, not something the repo enforces. To avoid silent loss of work during the long red window of a big-bang upgrade, run it on a dedicated branch/worktree with the watcher disabled, or land changes in lib-green batches with very frequent commits.
- User-visible regressions accepted: no find/replace, no custom scrollbar, and tab/autoindent/
  bracket-autoclose only to the extent bevscode provides them. The guarding E2E flows
  [J2]/[J3]/[J4]/[J5]/[J6] are deleted or relaxed. [L4] stays an unchanged render smoke
  (it asserts neither scrollbar nor highlight). [I11] keeps its existing app-level
  AppHistory undo (cosmic-independent — only the CosmicTextChanged echo-suppression seam
  is re-pointed to bevscode's text-changed seam). No highlight regression flow exists today;
  add a tree-sitter-python highlight flow or record the gap explicitly.
