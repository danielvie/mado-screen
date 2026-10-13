# PoC 5 implementation and validation

## Scope and execution

User approved multiple-monitor drawing and confirmed hardware is available. Sharing/recording is deferred. Startup-only monitor discovery, one virtual-desktop overlay/bitmap and shared tools/history are specified in tasks/poc5-spec.md. Restart after changing monitor layout or display scaling.

Main starts at a77dc9a. Existing uncommitted README.md/Taskfile.yml install changes are preserved in the primary checkout. Workers use isolated worktrees; one persistent integration worktree at integration/poc5 retains its Cargo cache. No combined build/test/check runs after individual worker integration.

## Deliveries

- Specification checkpoint: 56655e7.
- Composition: original 13f4765 -> 5ccd6fa, main startup reporting, primary-monitor indicator placement, cross-monitor shared-style/history regression and documentation. Tracking record ba9080f -> 3bdf8e6. One focused shared-history test passed after overlay API integration; rustfmt and diff whitespace checks passed.
- Overlay bounds d8c0600 -> d8c0600. Five focused overlay tests passed, including three bound tests for negative/above/positive origins, invalid dimensions and overflow; scoped rustfmt/Clippy passed.
- Surface placement 4471dae -> 9888f07. Four focused tests and scoped rustfmt/Clippy passed. Hidden native tests preserve positive/negative screen positions during uploads and verify window/class cleanup; raster test covers negative/outside canvas points. Restoring the old destination made the placement regression fail as expected.
- Desktop smoke df7a85e -> 324aee8 adds DPI-aware physical monitor enumeration, canvas/screen translation, exact virtual bounds, per-monitor and adjacent-seam posted history/erase checks. Parse and pure self-check passed before delivery.

Read-only design review found no concrete composition blocker. Per-monitor-v2 awareness, canvas-local coordinates and screen-relative owned popups are consistent; physical seam capture, mixed-DPI alignment and picker focus still need manual observation.

## Gates

First combined gate at 324aee8: task build passed, task test 68 passed, task check formatting/Clippy passed, smoke self-check passed. Desktop smoke confirmed exact virtual bounds and visible owned status, then failed at the first CopyFromScreen call with an invalid-handle error. Copied child cleanup ran; no full desktop pass is claimed. A focused sampling repair is assigned without weakening assertions. Read-only DPI-aware metrics report virtual origin (-1920, 0), extent 5760x1200, primary 1920x1080. Early System.Windows.Forms enumeration was DPI-virtualized and is not used as physical geometry evidence; final smoke will enumerate native monitor bounds.

## Manual acceptance

These remain untested until the user observes them. Automated posted messages cannot establish physical interaction.

- [ ] Draw pen and all shapes on each monitor; no positional offset or clipping at screen edges.
- [ ] Draw across adjacent boundaries in both directions, including screens with different scaling; one undo removes the whole annotation.
- [ ] Erase across boundaries, undo/redo and clear across all screens.
- [ ] Open the picker on each monitor; inspect placement, disk/hex controls and scaling.
- [ ] Toggle click-through and type/click/scroll in underlying applications on each screen; confirm focus and shortcuts restore normally.
- [ ] Clear/toggle while dragging, release near/outside display edges and resume only with a new press.
- [ ] Observe pointer responsiveness and memory growth during normal multi-monitor use.
- [ ] Repeat with an above/below screen arrangement where available, restarting after layout changes.

Sharing/recording, live reconfiguration, oversized layouts and secure desktop remain outside this increment.
