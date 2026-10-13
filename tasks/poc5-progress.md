# PoC 5 implementation and validation

## Scope and execution

User approved multiple-monitor drawing and confirmed hardware is available. Sharing/recording is deferred. Startup-only monitor discovery, one virtual-desktop overlay/bitmap and shared tools/history are specified in tasks/poc5-spec.md. Restart after changing monitor layout or display scaling.

Main starts at a77dc9a. Existing uncommitted README.md/Taskfile.yml install changes are preserved in the primary checkout. Workers use isolated worktrees; one persistent integration worktree at integration/poc5 retains its Cargo cache. No combined build/test/check runs after individual worker integration.

## Deliveries

- Specification checkpoint: 56655e7.
- Composition: original 13f4765, main startup reporting, primary-monitor indicator placement, cross-monitor shared-style/history regression and documentation. Focused native tests wait for overlay API integration; rustfmt and diff whitespace checks passed.
- Overlay bounds, surface placement and desktop smoke deliveries are pending integration.

## Gates

Combined build/test/check and safe desktop smoke are pending. No PoC 5 desktop pass is claimed yet. Read-only DPI-aware metrics report virtual origin (-1920, 0), extent 5760x1200, primary 1920x1080. Early System.Windows.Forms enumeration was DPI-virtualized and is not used as physical geometry evidence; final smoke will enumerate native monitor bounds.

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
