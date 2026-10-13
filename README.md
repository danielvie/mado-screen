# Mado Screen PoC 3

Small native Win32 overlay in Rust. Startup shows an empty, click-through canvas on the primary display. Choose a fixed orange 5-pixel pen, straight line or outline rectangle. There is no toolbar, undo, fill, snapping, persistence or GUI framework.

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

- `Ctrl+Alt+D` toggles click-through and drawing. Drag the logical primary mouse button to draw. Switching mode preserves pen ink and completed annotations, but cancels an unfinished line/rectangle preview.
- In drawing mode only, unmodified `P` selects pen, `L` selects straight line and `R` selects outline rectangle. Lines and rectangles preview during dragging and commit on release. Rectangles work in every drag direction; zero-size gestures may draw a dot.
- Changing tools cancels an unfinished shape and ends the drag. Selecting the current tool does not interrupt it. The selected tool survives clear and mode changes.
- `Ctrl+Alt+C` ends the current drag and clears all strokes without changing mode. Holding the button after clear does not resume drawing until a fresh press.
- `Ctrl+Alt+Q` quits and releases drag capture, its timer, the window, bitmap, device context and shortcuts.

P/L/R are temporary global shortcuts registered only in drawing mode; they select tools instead of typing into the focused application. Other keys still reach that application, without moving keyboard focus. Click-through and exit release P/L/R for normal typing. If one is reserved by another application, drawing-mode entry reports the conflict and stays click-through with existing ink intact. Partial registrations are rolled back; permanent toggle/clear/quit remain available.

Fully transparent layered-window pixels pass mouse input through, so drawing mode uses black background alpha 1/255. This near-transparent background can slightly darken the desktop. Toggle back to click-through to interact with applications beneath the overlay.

Button-up applies its final point and commits the gesture. Cancellation or capture loss removes an unfinished shape but preserves pen ink. A move without the primary button commits the last preview. The outside-release watchdog also commits that preview when capture is still owned; lost ownership cancels it. A 32ms watchdog runs only during an active drag, checking capture ownership and physical primary-button state, including swapped buttons. This covers capture-loss messages delivered synchronously to the window callback and outside-display releases that a nonactivating background window may not receive. Unavailable input state conservatively ends the stroke; stale timer messages do nothing. The interval is not a guaranteed release latency because a busy message loop can delay timers. Outside-release behavior still needs a physical desktop check.

## Missing features and next steps

The user reports PoC 3 worked. Missing capabilities and proposed improvements are recorded in docs/feature-roadmap.md. Multi-monitor work is deferred because hardware is unavailable. The recommended next increment is undo/redo, pending approval; no application behavior changes are included in that proposal.

## Scope and validation

Only the primary display is covered. Restart after display-layout changes. Multiple monitors, mixed-DPI layouts, elevated applications, other topmost windows and screen-sharing compatibility are not validated. Secure desktop and lock screen are excluded; exclusive fullscreen coverage is not guaranteed.

Rendering occurs at startup, mode changes, clear and changed stroke points. Hover and duplicate points do not redraw. The message loop blocks while idle, with no permanent polling timer. Each changed event fills the full-screen CPU bitmap, replays all paths in O(total stroke points), then uploads the whole bitmap. Long drawings and high resolutions can cause lag. Measure on the user's display before choosing another renderer; unit tests and CPU timings do not establish hardware input latency.

Unit tests do not launch the overlay. The optional desktop smoke check needs 64-bit Windows PowerShell and an unlocked, visually stable desktop with no existing PoC instance. It launches a copied executable, compares empty-startup desktop samples, measures idle CPU, queues clear/quit actions, and checks shortcut-conflict cleanup. Add `-PostedStroke` when invoking `scripts/smoke.ps1` to briefly enter drawing mode and queue app-local pen/line/rectangle gestures, tool/mode cancellation, and a recoverable bare-key registration conflict. This also checks P/L/R registration scope and preservation of committed annotations before clearing. Drawing-mode waits are bounded and failure cleanup terminates the copied process. No real mouse or keyboard input is injected and the cursor is not moved. Logs and results are saved under `%TEMP%`.

PoC 3 passed build, all 38 tests, formatting/Clippy and 49 posted-message desktop checks, with zero skipped on a 3440x1440 display. Initial idle process CPU time was unchanged over 30 seconds. Results are recorded in `tasks/todo.md`. Workers ran focused tests; combined checks ran after all deliveries, and again after one smoke-harness repair, using `docs/workflow-strategy.md`.

Empty-canvas smoke comparisons briefly hide and restore only the copied test window without activation to sample a fresh background. Moving samples are explicitly marked skipped/untested, never passed. `scripts/smoke.ps1 -SelfCheck` tests the sample classifier without launching the app.

Queued messages prove application routing and registration scope, not physical hardware, actual character typing, global shortcut delivery, focus preservation or outside-release behavior. Hardware latency and focus checks need manual acceptance. Manual acceptance remains: draw over Notepad, switch and type/click normally, resume drawing; release outside the primary display; clear/toggle/quit while dragging; observe lag at the user's resolution. Module responsibilities and remaining work are in `tasks/plan.md` and `tasks/todo.md`.
