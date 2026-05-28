---
status: accepted
---

# Strategy Editor renders as a projected UI overlay, not a world-space sprite

Issue #50 replaces the Strategy Editor backend from `bevy_cosmic_edit` (world-space
sprite, custom render) with **bevscode** (Bevy UI `Node`, GPU-instanced text,
tree-sitter, rope undo). bevscode's `CodeEditor` is fundamentally a Bevy UI Node
— screen-space flexbox, drawn after the world layer — so it does not naturally
follow a world `Transform`. We chose the **Projected Node** approach: a
per-frame system reads the floating window root's world transform and the world
camera's orthographic projection, computes the screen rect, and overwrites the
editor `Node`'s `left/top/width/height` and `TextFont.font_size = base * zoom`.
Pan / zoom / drag all follow because the projection re-runs every frame; text
stays crisp because bevscode re-rasterizes at the scaled `font_size`.

To keep the floating window's affordances (title bar drag, × button, resize
handles) visible above other windows, the Strategy Editor's world-space root
sprite is pinned to a high baseline `z ≈ 200` (other windows start at z=10 and
grow by 2 per click-to-front). Bevy UI is drawn after world sprites, so the
editor content layer naturally renders in front of all other floating windows
regardless.

## Why this is recorded

A future reader will see one floating window whose contents are a Bevy UI Node
while every other window's contents are world-space sprites (`buying_power`,
`chart`, `positions`, `orders`, `run_result`, `startup`), and would otherwise
wonder why this one is asymmetric. The asymmetry is deliberate: bevscode is the
only Bevy-native code editor with the feature set we need (multi-cursor,
tree-sitter, rope undo, find/replace) and it is Node-based. RTT was rejected
because the world→editor input coordinate remapping would be a custom-pipeline
maintenance burden. Forking `bevy_instanced_text` to support world-space
transforms was rejected for the same reason. The Projected Node approach gets
the real editor with no fork, in exchange for one asymmetry and one z-order
inversion.

## Considered alternatives

- **RTT (render-to-texture)** — route bevscode through a dedicated camera with
  `RenderTarget::Image`, display as the window's content sprite. Rendering and
  pan/zoom-follow are clean (`plugin_tests.rs:1085` proves bevscode supports
  RTT). Rejected because input coordinate remapping (world-sprite hit →
  Node-local cursor) is the actual hard problem and adds a permanent
  coord-translation seam.
- **Brain + custom world-space renderer** — use bevscode's editing engine
  (`TextBuffer<RopeBuffer>`, tree-sitter via `bevy_tree_sitter`, display_map for
  wrap/fold) but reimplement the rendering layer in world-space `Text2d`.
  Rejected: re-implements a non-trivial fraction of `text_view` / `editor_ui` /
  `line_numbers` / `lsp_ui` and loses GPU-instanced text performance.
- **Fork `bevy_instanced_text`** to add a `GlobalTransform`-driven mode.
  Rejected: would mean maintaining a fork of a custom render pipeline (the same
  cost that motivated leaving `bevy_cosmic_edit` in the first place).

## Consequences

**Accepted trade-offs**

- Strategy Editor cannot be visually occluded by other floating windows. UI is
  drawn after world; the editor's UI Node renders on top of every world sprite.
  Mitigated only in that the editor is normally the focus when in use.
- Z-stack drift: roughly 95 click-to-front activations on other windows raise
  `WindowManager.max_z` to ≈ 190, approaching the editor's z=200 baseline.
  Acceptable in the near term; a future tweak can re-bump the editor's z
  whenever `max_z` crosses a threshold.
- The per-frame projection system must run after `floating_window_layout_system`
  and `Transform` propagation so it reads up-to-date world state.

**Reference**

- Three.js `CSS3DRenderer` — same pattern (rich screen-space content positioned
  per frame from a world anchor with full transform projection).
- bevscode RTT test: `.claude/skills/bevy-engine/bevscode/crates/bevscode/src/display_map/plugin_tests.rs:1085-1133`
  — referenced for "renderer works with arbitrary `Camera2d`", though we do not
  use RTT.
