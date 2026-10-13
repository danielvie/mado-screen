# Mado Screen PoC 2

Small native Win32 overlay in Rust. Startup shows an empty, click-through canvas on the primary display. Drawing uses one fixed orange 5-pixel freehand pen. There is no toolbar, undo, shape tool, persistence or GUI framework.

## Requirements and commands

Windows 10 version 1703 or newer, or Windows 11, for Per-Monitor V2 DPI awareness. Install current stable Rust with the Windows MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK. Task is optional.

| Task command | Rust command | Purpose |
| --- | --- | --- |
| `task run` | `cargo run` | Launch the overlay with console output |
| `task build` | `cargo build` | Build the executable |
| `task test` | `cargo test` | Run stroke, pixel, input, control, style, resource and drop-order checks |
| `task smoke` | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe` after building | Check empty startup, clear/quit dispatch, shortcut rollback and 30-second idle CPU |
| `task check` | `cargo fmt --check` then `cargo clippy --all-targets -- -D warnings` | Check formatting and warnings |
| `task clean` | `cargo clean` | Remove build artifacts |

`task` lists available tasks. The debug executable is `target\debug\mado-screen-poc.exe`.

## Controls

The overlay starts click-through and does not activate itself. Shortcuts are global and registered before the window is shown. A shortcut conflict aborts startup and releases any shortcuts already registered.

- `Ctrl+Alt+D` toggles click-through and drawing. In drawing mode, drag the logical primary mouse button to draw, or click for a dot. Switching mode ends a partial stroke and preserves its visible points.
- `Ctrl+Alt+C` ends the current drag and clears all strokes without changing mode. Holding the button after clear does not resume drawing until a fresh press.
- `Ctrl+Alt+Q` quits and releases drag capture, its timer, the window, bitmap, device context and shortcuts.

Fully transparent layered-window pixels pass mouse input through, so drawing mode uses black background alpha 1/255. This near-transparent background can slightly darken the desktop. Toggle back to click-through to interact with applications beneath the overlay.

Button-up appends its final point and ends the stroke. Cancellation or capture loss ends it without discarding visible points. A move without the primary button also ends the drag. A 32ms watchdog runs only during an active drag, checking capture ownership and physical primary-button state, including swapped buttons. This covers capture-loss messages delivered synchronously to the window callback and outside-display releases that a nonactivating background window may not receive. Unavailable input state conservatively ends the stroke; stale timer messages do nothing. The interval is not a guaranteed release latency because a busy message loop can delay timers. Outside-release behavior still needs a physical desktop check.

## Scope and validation

Only the primary display is covered. Restart after display-layout changes. Multiple monitors, mixed-DPI layouts, elevated applications, other topmost windows and screen-sharing compatibility are not validated. Secure desktop and lock screen are excluded; exclusive fullscreen coverage is not guaranteed.

Rendering occurs at startup, mode changes, clear and changed stroke points. Hover and duplicate points do not redraw. The message loop blocks while idle, with no permanent polling timer. Each changed event fills the full-screen CPU bitmap, replays all paths in O(total stroke points), then uploads the whole bitmap. Long drawings and high resolutions can cause lag. Measure on the user's display before choosing another renderer; unit tests and CPU timings do not establish hardware input latency.

Unit tests do not launch the overlay. The optional desktop smoke check needs 64-bit Windows PowerShell and an unlocked, visually stable desktop with no existing PoC instance. It launches a copied executable, compares empty-startup desktop samples, measures idle CPU, queues clear/quit actions, and checks shortcut-conflict cleanup. Add `-PostedStroke` when invoking `scripts/smoke.ps1` to briefly enter drawing mode and queue app-local down/move/up messages, verify orange pixels, return to click-through, and clear. Drawing-mode waits are bounded and failure cleanup terminates the copied process. No real mouse or keyboard input is injected and the cursor is not moved. Logs and results are saved under `%TEMP%`.

Build, formatting, Clippy and 26 tests passed. The desktop smoke check with `-PostedStroke` passed 22 checks on a 3440x1440 display, including stroke rendering, persistence after mode change and clearing. Initial process CPU time was unchanged over a 30-second idle sample.

Queued messages prove application routing, not physical hardware, global shortcut routing, focus preservation or outside-release behavior. PoC 2 hardware latency and focus checks have not passed acceptance here. Manual acceptance remains: draw over Notepad, switch and type/click normally, resume drawing; release outside the primary display; clear/toggle/quit while dragging; observe lag at the user's resolution. Module responsibilities and remaining work are in `tasks/plan.md` and `tasks/todo.md`.
