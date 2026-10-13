# Disk color picker proposal

Status: implemented after user approval. Validated code d670313 passed 59 tests, formatting/Clippy and 79 desktop checks with zero skipped. Source and detailed limits are tracked in tasks/disk-picker.md. The Excalidraw drawing remains the design reference; disk-color-picker-implemented.png is an actual offscreen native-pane capture.

Editable design: disk-color-picker.excalidraw. Preview: disk-color-picker.png. Drop the Excalidraw file into https://excalidraw.com to inspect or edit it. The disk is grouped native polygon geometry, not an embedded reference image; ungroup it to edit individual sectors. All text uses Excalifont. The PNG previews the layout with Segoe UI rather than the Excalidraw font.

## Design

Adapt the supplied color-disk reference into an actual compact control, without copying its pipette artwork. A dark panel contains one circular hue/saturation disk, a brightness slider, candidate-color swatch and hex input, familiar presets, and Apply/Cancel. No title-bar clutter or separate RGB form by default. The intended disk has a smooth gradient; editable tiles in the drawing approximate it.

Approximate popup size: 300 by 460 logical pixels. It opens near the mouse on drawing-only C and clamps to the nearest work area. Scale controls with DPI rather than clipping them. Placement examples are schematic, not pixel-perfect scaled copies of the full panel.

## Interactions

- Angle changes hue; distance from the center changes saturation. The center is neutral, the rim vivid. A white/black double-ring marker stays visible over every color.
- Brightness controls value from black to full brightness. Center plus maximum brightness selects white; the minimum brightness selects black.
- Disk dragging, slider movement, presets and valid hex edits update the candidate swatch and hex value, not existing annotations or the committed brush color.
- Apply or Enter confirms and closes. Escape, close or an outside click cancels and preserves the previous color. Invalid hex remains in the pane with an inline error; it never commits.
- Tab navigates labeled disk, brightness, hex, presets and buttons. Arrow keys adjust hue/saturation or brightness on the focused control. Native accessibility names expose the values; hex entry remains an alternative to visual selection.
- Preserve PoC4 key suspension while the picker is open, safe reacquisition/conflict fallback, permanent toggle/clear/quit, and no unintended canvas strokes. Existing annotations retain their styles.

Screen eyedropper/sampling, opacity and thickness controls are not part of this picker change. The reference pipette is treated as an illustration, not an additional requested feature.

The user approved this layout and confirmation behavior. Native hidden tests and offscreen capture verify candidate/event handling and rendering. Physical focus/drag, live Tab behavior, mixed-monitor DPI and assistive-technology acceptance remain manual; the design drawing and hidden tests do not establish them.
