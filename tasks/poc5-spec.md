# PoC 5: drawing across multiple monitors

## Objective and scope

The user approved multiple-monitor drawing, confirmed test hardware is available, and deferred sharing/recording. Preserve all existing tools, styles, picker behavior, shortcuts and history. Detect the Windows extended desktop at startup. Draw on any connected monitor without selecting a screen; a stroke crossing adjacent monitors remains one annotation and one undo action. Restart after adding/removing/rearranging monitors or changing display scaling. Mirrored displays expose the same desktop, not independent canvases.

## Design and contracts

Use one topmost nonactivating layered window and one bitmap spanning the virtual desktop. This keeps existing canvas-local annotation coordinates and a single history without routing between windows. The window's screen origin can be negative. Mouse messages remain canvas-local physical pixels; do not add the desktop origin to stored paths. Brush widths remain physical pixels across mixed-DPI monitors. Keep the single indicator on the primary monitor and the picker on the monitor nearest the cursor.

- `overlay.rs`: read SM_XVIRTUALSCREEN/SM_YVIRTUALSCREEN/SM_CXVIRTUALSCREEN/SM_CYVIRTUALSCREEN once. Validate dimensions and coordinate overflow, preserve the actual origin in creation and show. Keep `Overlay::new`, `hwnd`, `size`, `show`, `set_capture`; add `origin() -> (i32, i32)`. Retain the existing class/title to preserve process guards. Reject dimensions beyond the signed client-coordinate range with a clear error rather than decoding wrapped mouse positions.
- `surface.rs`: retain API and render canvas-local paths. Pass no destination position to UpdateLayeredWindow so uploads never move the window to (0,0). Keep the source bitmap origin (0,0). Add a hidden native placement/resource regression and test negative/outside canvas points.
- `main.rs`: compose virtual surface/overlay unchanged; report PoC 5 origin/dimensions and restart limit. Pass primary display dimensions, not virtual dimensions, to the existing DesktopUi constructor so the status stays on a real monitor.
- `scripts/smoke.ps1`: sample screen pixels using virtual-origin translation; retain existing primary-display geometry tests in canvas-local coordinates. Check exact virtual overlay bounds, draw and undo/redo/erase on each enumerated monitor, and check a crossing stroke on an adjacent pair where available. Skip unavailable layouts explicitly. Never inject system input, move the cursor, steal focus, close an existing overlay or manipulate another process's windows.

## Tradeoffs and boundaries

The virtual bounding bitmap includes gaps between monitors. Full fill/replay/upload and snapshot history remain unchanged and may cost more time/memory. Measure responsiveness before considering per-monitor surfaces or GPU rendering. Signed 16-bit client mouse messages limit this first implementation to virtual dimensions of at most 32767 pixels per axis; oversized layouts must fail startup clearly. No new dependency, toolbar, settings, save/export, sharing or live display reconfiguration.

Always preserve input-registration rollback, capture cleanup, shared undo/redo and frozen annotation styles. Ask before changing these contracts or adding dependencies. Never overwrite unrelated changes or weaken failed checks. Physical input, boundary capture, mixed-DPI alignment and picker focus require user observation; posted events cannot establish those behaviors.

## Structure, style and commands

Rust 2024 with windows = 0.62.2. Source and inline tests are in src; desktop smoke is scripts/smoke.ps1; evidence is tasks/poc5-progress.md. Match existing Result-returning native methods and RAII cleanup, for example `pub fn origin(&self) -> (i32, i32)`. Format with rustfmt; warnings must pass Clippy.

- Run: `task run`
- Build: `task build`
- Tests: `task test`
- Formatting/lint: `task check`
- Desktop: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe -PostedStroke`
- Harness self-check: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -SelfCheck`

## Execution and acceptance

Follow docs/workflow-strategy.md with at most four isolated workers, one persistent integration worktree/Cargo cache, serial private-branch rebases and fast-forwards, and no combined checks after individual deliveries. Workers run focused checks and commit handoffs. Main stays at its previous validated revision until the assembled code passes task build/test/check and applicable smoke. Repair failed gates and rerun against changed code before promotion.

1. Pure tests cover negative origins, left/above layouts, dimension/overflow rejection and coordinate mapping. Native hidden tests prove uploads retain nonzero positions.
2. Desktop smoke proves startup spans the current virtual desktop; existing tools/history/conflict/cleanup still work; annotations render on each available monitor; cross-monitor edits share one history. Record absent scenarios as skipped/untested.
3. User manually draws on each monitor and across seams, tests all tools, picker placement/scaling and click-through typing. Repeat with an above/below layout and mixed scaling where available. Record results without inferring physical behavior from automated checks.

## Official sources checked

- Virtual desktop and negative coordinates: https://learn.microsoft.com/en-us/windows/win32/gdi/the-virtual-screen
- Virtual metrics: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics
- UpdateLayeredWindow destination position may be NULL when unchanged: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow
- Signed mouse coordinates: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousemove
