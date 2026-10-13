# Mado Screen PoC

Small native Win32 overlay in Rust. Startup automatically displays a fixed orange line on the left and an orange outline rectangle on the right of the primary display. Color and thickness are fixed; positions use display dimensions. There is no interactive drawing or GUI framework.

## Requirements and commands

Windows 10 version 1703 or newer, or Windows 11, for Per-Monitor V2 DPI awareness. Install current stable Rust with the Windows MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK. Task is optional.

| Task command | Rust command | Purpose |
| --- | --- | --- |
| `task run` | `cargo run` | Launch the overlay with console output |
| `task build` | `cargo build` | Build the executable |
| `task test` | `cargo test` | Run pixel, control, style, resource and drop-order checks |
| `task smoke` | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe` after building | Check actual desktop rendering, clear/quit dispatch, shortcut rollback and 30-second idle CPU |
| `task check` | `cargo fmt --check` then `cargo clippy --all-targets -- -D warnings` | Check formatting and warnings |
| `task clean` | `cargo clean` | Remove build artifacts |

`task` lists available tasks. The debug executable is `target\debug\mado-screen-poc.exe`.

## Controls

The overlay starts click-through and does not activate itself. Shortcuts are global and registered before the window is shown. A shortcut conflict aborts startup and releases any shortcuts already registered.

- `Ctrl+Alt+D` toggles click-through and input capture.
- `Ctrl+Alt+C` clears both shapes. Restart to restore them.
- `Ctrl+Alt+Q` quits and releases the window, bitmap, device context and shortcuts.

Capture mode only logs left-button down/up and wheel coordinates. It does not draw. Fully transparent layered-window pixels pass mouse input through, so capture uses black background alpha 1/255. This near-transparent background can slightly darken the desktop; it is an intentional compromise to receive input in blank areas. Clear does not disable capture. Toggle back to click-through or quit to release input.

## Scope and validation

Only the primary display is covered. Restart after display-layout changes. Multiple monitors, mixed-DPI layouts, elevated applications, other topmost windows and screen-sharing compatibility are not validated. Secure desktop and lock screen are excluded; exclusive fullscreen coverage is not guaranteed.

Rendering occurs at startup and on toggle or clear. The message loop blocks while idle, without continuous redraw. The CPU-rendered bitmap uploads the whole window on each change. Static shapes do not establish interactive drawing latency or frame rate.

Unit tests do not launch the overlay. The optional desktop smoke check needs 64-bit Windows PowerShell and an unlocked desktop with no existing PoC instance. It launches a copied executable in click-through mode, verifies visible shape pixels, measures idle CPU, queues clear/quit actions, and checks shortcut-conflict cleanup. It does not inject mouse or keyboard input or switch to capture mode. Logs and results are saved under `%TEMP%`.

The integrated executable passed 9 unit/native-resource tests, formatting, Clippy, and the desktop smoke check on a 3440x1440 primary display. Measured process CPU time was unchanged over the 30-second idle sample. This is one hardware sample, not a general performance guarantee.

Real keyboard shortcuts, click/scroll routing, focus preservation and capture mode still need manual checks over Notepad and a browser. The smoke check queues actions directly and does not prove global keyboard routing. Full verification results and module responsibilities are in `tasks/todo.md` and `tasks/plan.md`.
