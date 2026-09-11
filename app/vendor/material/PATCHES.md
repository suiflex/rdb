# Local patches to the vendored Material library

This directory is `ui-libraries/material/src` from the slint `v1.17.1` tag.
The changes below are ours; re-apply them (or check they are no longer
needed) whenever the library is re-vendored for a newer Slint.

## 1. Tooltips go to a global instead of an in-tree `ToolTip`

Files: `ui/components/tooltip.slint`, `ui/components/state_layer.slint`,
`ui/components/extended_touch_area.slint`, `material.slint`.

Upstream draws a control's tooltip as a `ToolTip` child of its own touch area
(`z: 10000`). `z` only orders siblings, so anything a parent paints later still
covers it, and any `clip: true` ancestor — every `ElevatedCard` clips its
children — cuts it off. In the app the tooltip regularly vanished behind cards,
the grid and the tab strip.

The patch adds an exported `ToolTipState` global (`text`, `x`, `y`, `shown`).
`FocusTouchArea` and `ExtendedTouchArea` keep their exact show condition, but on
`has-hover` / `pressed` changes they write the tooltip and a window-space anchor
there instead of instantiating `ToolTip`. The application's top-level overlay
(`app/src/ui/app-window.slint`) reads `ToolTipState` and paints it above
everything, modals included. `tooltip_offset` is kept (unused) so the public
properties of the components do not change.
