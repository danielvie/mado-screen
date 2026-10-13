# Windows annotation proof of concept

## Status and goal

PoC 1 implemented through isolated workers and serial integration. Build, 9 tests, formatting, Clippy, and desktop rendering/cleanup smoke checks pass. Actual input and focus checks remain manual. Scope is tasks 1 through 3 on the primary display; tasks 4 and 5 remain deferred. Runtime proof results are tracked in tasks/todo.md.

Build one small Rust executable to prove that the proposed native overlay can render a line and rectangle above Windows applications and switch input ownership safely. Draw both shapes programmatically at startup, not through user interaction.

Assumptions: Windows 10/11, mouse input, ordinary non-elevated applications, primary display first. Target OS confirmation is complete; hardware and preferred sharing application are not yet specified.

## Minimal application

- Start in click-through mode, without stealing foreground focus.
- Automatically render one fixed line and one outline rectangle at startup, positioned within the primary display. Use fixed color and thickness.
- Global shortcuts to toggle input capture versus click-through, clear both shapes, and quit safely. Input capture only logs mouse events; it does not create or modify shapes.
- Shapes remain visible in click-through mode until cleared or the application exits.
- Console output for mode, mouse coordinates, display dimensions, and proof observations.
- In-memory state only. No pen, drag drawing, tool selection, toolbar, tray, settings, undo, additional shapes, export, installer, or remote control.
- Implement one primary-display overlay first. Extend to one overlay per monitor only for later validation.

## Module boundaries and parallel execution

PoC 1 is one application, not several prototypes. Keep modules small and concrete:

- `drawing.rs`: pure rasterization and pixel checks. Existing implementation is the baseline.
- `surface.rs`: owns memory DC and DIB; `Surface::new(width: i32, height: i32) -> Result<Self>` and `present(&mut self, hwnd: HWND, capture: bool, cleared: bool) -> Result<()>`. Depends only on drawing and Windows bindings.
- `overlay.rs`: owns the hidden primary-display window and callback, with topmost style set before the first upload; `Overlay::new() -> Result<Self>`, `hwnd() -> HWND`, `size() -> (i32, i32)`, `show() -> Result<()>`, `set_capture(capture: bool) -> Result<()>`. It does not own rendering or register shortcuts.
- `controls.rs`: shortcut registration guard `Hotkeys::register() -> Result<Self>` plus `Action::{Toggle, Clear, Quit}` and `action(id: usize) -> Option<Action>`. No window creation or rendering.
- `main.rs`: DPI initialization, module composition, blocking message loop, capture/clear state. Registers shortcuts before showing the overlay. Declares resources so the window is destroyed before its surface on shutdown.

Ready worker tasks: extract surface, extract overlay, extract controls. Start these independently in isolated Git worktrees. The final main composition task depends on all three being integrated. Maximum four concurrent workers. Module workers own only their assigned source file; use a temporary, uncommitted compile harness if needed. They commit their completed work.

When a worker finishes, enqueue its commit and immediately start an integrator if none is running. A single integrator processes the queue in order; never integrate concurrently. Integrate in a worktree based on current main, cherry-pick the worker commit, resolve/rebase if necessary, verify, then fast-forward main. Only integrated dependencies unlock downstream work. Failed integrations block their dependent tasks and are reported. Keep the workflow journal for recovery.

## Proposed implementation

Use the Microsoft-maintained Rust `windows` crate and native Win32 APIs. A topmost layered window plus a CPU-rendered premultiplied-alpha DIB and `UpdateLayeredWindow` is the initial rendering experiment. No GUI framework or webview. Dependency pinned to windows 0.62.2.

This minimizes setup, not necessarily rendering cost. `UpdateLayeredWindow` updates the whole window; measure at high resolution before committing to this backend. Only investigate Direct2D or another GPU path if measured responsiveness is inadequate.

Important input trap: fully transparent pixels in a layered window pass clicks through even without `WS_EX_TRANSPARENT`. For the first experiment, test a background alpha of 1/255 in input-capture mode, and use `WS_EX_TRANSPARENT` in interaction mode. The tiny dark tint is an intentional prototype compromise, not perfect transparency. If blank areas still cannot capture input, report the input proof as failed; static shape rendering can be verified independently. Verify the visual effect on actual hardware.

Use `WS_EX_NOACTIVATE`, no-activation positioning, and `RegisterHotKey`; do not add global keyboard or mouse hooks initially. Report shortcut conflicts and do not leave an input-blocking overlay running without working escape controls. Enable Per-Monitor V2 DPI awareness before creating windows.

Create `Taskfile.yml` with default listing and described run, build, test, and clean tasks alongside the initial Cargo project. Leave one small runnable check for line/rectangle rasterization and alpha calculations; manual checks remain essential for Windows behavior.

## Proofs and order

1. Static overlay rendering: line and outline rectangle appear automatically above ordinary windows; the desktop and rectangle interior remain visible; clicks and scrolling pass through the shapes as well as blank areas.
2. Input ownership and global controls: blank areas log mouse input in capture mode; toggle, clear, and quit work while another application is active; overlay does not steal keyboard focus; quitting restores normal interaction.
3. Persistence and idle behavior: shapes survive mode switches and underlying-window movement, clear together without trails, and cause no continuous idle redraw. Record idle CPU and initial render observations.
4. Display coordinates: test 100%, 150%, and 200% scaling where available, mixed-DPI displays, negative monitor coordinates, and shape placement near display edges. Add one overlay per monitor for this proof.
5. Presentation compatibility: verify whole-display capture in the user's chosen sharing app, then separately record application-window capture, borderless fullscreen, and exclusive fullscreen results. Passing desktop capture does not imply passing window capture.

First-level PoC includes proofs 1 through 3 on the primary display. Proofs 4 and 5 are later validation.

Internal checkpoint after proofs 1 and 2: do not extend input functionality if safe switching fails. First-level checkpoint after proof 3: decide whether the static overlay approach is viable. Static rendering does not prove interactive drawing responsiveness; that needs a later experiment. Final checkpoint: list passed, failed, and untested scenarios explicitly.

## Performance checks

Proposed diagnostic target, not an established product requirement: idle CPU below 1% on the test machine, with no repeated redraw after startup. Record hardware, resolution, and initial render observations. A static line and rectangle cannot validate frame rate or input-to-display latency; defer those measurements until interactive drawing exists. Avoid creating a benchmark framework.

## Boundaries and open questions

- UAC secure desktop, lock screen, and guaranteed coverage of exclusive fullscreen are outside this prototype.
- Elevated applications and interactions with other topmost windows are not initially guaranteed.
- Which sharing application should be the primary compatibility test?
- Are multiple monitors and mixed scaling available for validation? Unavailable hardware checks remain untested, not passed.

## Sources

- Rust Windows bindings: https://github.com/microsoft/windows-rs
- Layered-window hit testing: https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows
- Full-window bitmap updates: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow
- Premultiplied alpha: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-blendfunction
- DIB allocation: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createdibsection
- Window styles: https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
- Topmost and no-activation positioning: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
- Global hotkeys: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey
- DPI awareness: https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process

Tasks and proof results are tracked in `tasks/todo.md`.
