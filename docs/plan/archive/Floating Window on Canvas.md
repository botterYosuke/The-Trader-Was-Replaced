# Implementation Plan - Floating Window on Canvas

The goal is to transform the current static dashboard into a dynamic, canvas-based UI where the main dashboard is a draggable "floating window".

## User Review Required

> [!IMPORTANT]
> The entire UI will be rendered within a `canvas`. This means standard widgets like `button` and `container` will be replaced by custom-drawn elements on the canvas for the floating window part.

## Proposed Changes

### [Component Name]

#### [MODIFY] [main.rs](file:///Users/sasac/The-Trader-Was-Replaced/src/main.rs)
- Update `Message` to include `WindowMoved(Point)`.
- Update `State` to include `window_pos: Point`.
- Update `State::view` to only contain a full-screen `canvas`.
- Rewrite `PriceChart` (rename to `DashboardCanvas`) to implement `canvas::Program` with full interactivity:
    - **Interaction State**: Add a state to track dragging.
    - **`update` method**: Handle mouse events for dragging the window and clicking custom buttons.
    - **`draw` method**: 
        - Draw a "Desktop" background.
        - Draw a window frame at `window_pos`.
        - Draw the title bar, price display, and Buy/Sell buttons manually.
        - Draw the price chart within the window's content area.

## Verification Plan

### Automated Tests
- Run the application and manually verify:
    - The window can be dragged by its title bar.
    - The "BUY" and "SELL" buttons still work when clicked.
    - The price chart updates correctly.

### Manual Verification
- Verify the aesthetics (glassmorphism/premium look) match the user's expectations for a "wow" factor.
