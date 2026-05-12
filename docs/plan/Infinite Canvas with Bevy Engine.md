# Implementation Plan - Infinite Canvas with Bevy Engine

Migrate from Iced to Bevy Engine to create a high-performance, Miro-like infinite canvas with draggable windows, smooth zoom/pan, and a premium aesthetic.

## User Review Required

> [!IMPORTANT]
> **Architecture Shift**: We are moving from the Elm-style architecture (Iced) to **ECS (Entity Component System)**.
> **World Space UI**: Windows will be "World Entities". This means they exist within the zoomable/pannable world, allowing them to scale naturally like objects on a real map.

## Proposed Changes

### [Dependencies]
#### [MODIFY] `Cargo.toml`
- Remove `iced`.
- Add `bevy` (with 2d and webgpu features).
- Add `bevy_pancam` (for effortless 2D pan/zoom).
- Add `bevy_mod_picking` (for high-level click/drag events on entities).
- Add `bevy_vector_shapes` or `bevy_prototype_lyon` (for drawing the price chart line).

### [Architecture - ECS Components]
- **`WindowRoot`**: Marker component for the floating window entity.
- **`Draggable`**: Component to mark entities that can be moved.
- **`PriceChart`**: Data component holding the price history for rendering.
- **`PriceText`**: Marker for the entity displaying the current price.

### [Core Systems]

#### [NEW] `camera_setup`
- Spawn a `Camera2dBundle`.
- Attach `PanCam` component to enable mouse-drag panning and scroll-wheel zooming out of the box.

#### [NEW] `ui_setup` (Startup System)
- Spawn the "Floating Window" as a hierarchy of entities:
    - **Parent (Window Frame)**: Sprite/Mesh with glassmorphism shader/material.
    - **Child (Title Bar)**: Interactive area for dragging.
    - **Child (Content Area)**: Contains the Price Chart and Buy/Sell buttons.

#### [NEW] `interaction_system`
- Use `bevy_mod_picking` events to:
    - Update the `Transform` of the `WindowRoot` when the Title Bar is dragged.
    - Handle "BUY/SELL" button clicks to update the global `Price` resource.

#### [NEW] `chart_rendering_system`
- Dynamically update the mesh/shape of the price chart line based on the `PriceChart` data.

### [Visuals & Polish]
- **Background Grid**: Implement a procedural grid shader that scales with zoom levels.
- **Smoothness**: Leverage Bevy's 60+ FPS rendering for "fluid" movement.

## Verification Plan

### Manual Verification
- **Infinite Canvas**: Confirm you can pan infinitely in any direction.
- **Zoom**: Confirm zooming in/out centers on the mouse cursor (provided by `bevy_pancam`).
- **Draggable Window**: Confirm the window can be moved and its children (buttons/chart) stay attached.
- **UI Interaction**: Confirm "BUY" and "SELL" buttons trigger price changes.
- **Scale**: Verify the window becomes smaller/larger during zoom, maintaining its relative position in the world.
