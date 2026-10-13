# Compact panel and color picker layout

## Approved scope

The user approved the supplied reference layouts with these refinements: the collapsed panel has only the selected tool's icon; it sits at the primary monitor's top-right and expands to the left on hover. Clicking the expanded current-color dot reveals five distinct recent colors for immediate selection. History is session-only. Keep drawing, shortcuts, shapes, ink, undo and temporary lifetime unchanged.

The color picker keeps C-at-pointer opening, nearest-monitor work-area clamping, hue/saturation selection, brightness, native hex input, presets, Cancel and Apply. Apply commits; dismissal does not change the current color. Use a light rounded pane with blue/gray text, orange Apply treatment and circular swatches, following the supplied reference. Keep hue/saturation continuous, including the less-saturated center. Do not turn the reference's illustrative wedges into a fixed palette.

The panel is dark charcoal with blue border/text. Compact state contains no status text or color dot. Expanded state shows mode, selected tool variant, thickness and current-color dot, without the old redundant hex text. Recent colors appear as a second row in that same panel. A selected tool icon must distinguish Pen, Arrow, Line, Rectangle, Ellipse and Eraser. Keep the top-right anchor while resizing.

## Interaction contract

The panel is nonactivating, but cannot remain mouse-transparent because hover and swatch clicks must work. Only its rounded occupied region intercepts the mouse, in drawing and click-through modes. No keyboard shortcuts are captured by the panel. Moving within the expanded panel or recent row must not collapse it. Leaving collapses it and hides recents. The current-color dot toggles the recent row. Quick selection changes future ink and settles the active gesture through the existing UiEvent::Color route, without painting a canvas dot. Do not show the full picker or steal focus for a recent selection.

Recent colors are newest-first, deduplicated, maximum five; include the initial brush color. Record committed color changes from existing keyboard choices, picker Apply and quick selection. Picker preview and Cancel must not enter history. No storage/configuration dependencies. Ignore stale/out-of-bounds clicks.

Retain native keyboard picker controls, meaningful window/control names, visible focus indicators, invalid-hex validation and best-effort focus restoration. Existing keyboard shortcuts remain the accessible alternative to the nonactivating hover panel.

## Ownership and delivery

Use sibling integration/layout and worker/layout-ui worktrees. Worker owns src/ui.rs only and commits delivery. Supervisor owns main integration if needed, docs and affected smoke expectations. Keep main and local README.md, Taskfile.yml install edits and the untracked Excalidraw untouched until serial integration and focused validation pass. No remote push or installed-app replacement.

DesktopUi::update_label remains compatible and receives committed model state; use it to record actual selected colors. Preserve UiEvent::Color/Closed and existing picker routing. Supervisor updates old click-through panel assertions in scripts/smoke.ps1 if necessary; do not run desktop smoke.

## Checks and commands

Build: cargo build --release.
Focused checks: cargo test ui::tests and directly affected main UI/picker input-routing tests. Native UI tests use hidden windows, no real input or foreground activation. Add checks for icon-only compact painting, primary top-right geometry, hover/leave transitions, hit-tested recent colors and five-entry deduplication, cancellation versus Apply, stale clicks, nonactivation, existing focus/capture cleanup, picker controls and clipping/DPI fitting. Export hidden offscreen preview images for visual review, not full desktop automation.
Formatting: cargo fmt --check.
Warnings: cargo clippy --all-targets -- -D warnings.
Review: git diff --check, actual worker changes and preview images.
Do not run the full test suite or desktop smoke, inject real input, move the cursor, launch the overlay or replace the installed executable. Physical hover/click behavior, mixed-DPI appearance and focus require later user acceptance.

## Implementation style and limits

Rust 2024, windows 0.62.2; use existing Win32/GDI ownership helpers and State model. No frontend framework, animation timer, new backend or dependency. Keep repainting event-driven. Native sizing/hover tracking must follow Win32 APIs; record relevant official documentation in the progress record. Prefer direct bounded recent-color storage and shared geometry for hit testing/painting over abstractions.

Success is matching the approved states while retaining drawing and picker semantics, passing focused checks/build/lints, and preserving user-owned changes. Physical acceptance is separate and remains open.
