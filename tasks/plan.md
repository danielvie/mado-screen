# Windows annotation proof of concept

## Current execution strategy

Future batches follow [the workflow strategy](../docs/workflow-strategy.md). Use up to four worktree workers, one persistent serial integrator, validation after all deliveries, worker repair loops, and promotion to main only after required checks pass. Earlier per-delivery verification and promotion descriptions below are historical, not the current workflow policy.

## PoC 5: multiple-monitor drawing

User confirmed multi-monitor test hardware, deferred sharing/recording and approved implementation under docs/workflow-strategy.md. Scope and module contracts are in tasks/poc5-spec.md. One virtual-desktop overlay/bitmap preserves shared gestures, styles and history across screens. Monitor discovery is startup-only; restart after layout/scaling changes. Three independent workers own overlay.rs, surface.rs and scripts/smoke.ps1. The supervisor composes main/docs in a separate worktree and performs serial integration and final gates. Original/rebased commits and results are recorded in tasks/poc5-progress.md. Historical no-hardware statements below describe earlier PoCs.

## Disk picker follow-up

User accepted PoC 4, approved the Excalidraw disk design and requested implementation. The RGB form is replaced by a smooth hue/saturation disk, brightness, candidate swatch/hex, six presets and Apply/Cancel in a dark DPI-scaled pane. Existing main/annotation/control behavior is unchanged. Validated code d670313: build, 59 tests, formatting/Clippy, 79 desktop regressions with zero skipped, and hidden native/offscreen visual review. Specification, delivery records and manual limits are in tasks/disk-picker.md.

## PoC 4 history

PoC 4 implemented. Final code 7f934a7 passed build, 55 tests, formatting/Clippy and 79 desktop smoke checks with zero skipped, plus indicator visual review. User authorized autonomous implementation of undo/redo, color/thickness, status indicator, arrow/ellipse tools, whole-annotation erasing and a C color picker at the mouse. Contracts, scope and staged build order are in tasks/poc4-spec.md. Completed stage validation and remote checkpoints are in tasks/poc4-progress.md. Multi-monitor work remains deferred; no hardware is available. Each usable stage is validated, promoted and pushed to origin/main without force. Historical display/capture checks remain open.

## PoC 3 history: line and rectangle tools

Implemented under the deferred-validation strategy. Validated code cbc79f1 passed build, 38 tests, formatting/Clippy and 49 posted-message desktop checks with zero skipped. Physical typing/mouse/focus/outside-release/latency evidence remains manual. Full results and delivery commits are in tasks/todo.md.

Approved scope: keep the freehand pen and add interactive straight line and outline rectangle. Bare P/L/R select tools only in drawing mode. Startup remains empty and click-through; default tool is Pen. Color and width remain fixed orange/5px. No toolbar, undo, fills, snapping, new dependencies or renderer rewrite.

### Shared contracts

- `strokes.rs` owns geometry and gesture state. Add `Tool::{Pen, Line, Rectangle}`, deriving Debug/Copy/Clone/Default/PartialEq/Eq with Pen default. Preserve `begin`, `append -> bool`, `finish`, `clear`, `is_active`, `paths`. Add `tool() -> Tool`, `select(tool: Tool) -> bool`, `cancel() -> bool`.
- Existing path representation stays `Vec<Vec<(i32,i32)>>`; no renderer/surface API change is needed. Pen appends points. Line replaces its active path with anchor/end. Rectangle replaces it with a closed five-point axis-aligned path, preserving anchor/opposite corner in every drag direction. A zero-size gesture may render a dot.
- `finish` commits any active gesture. `cancel` ends it, preserving pen ink but removing an active line/rectangle preview; return whether visible pixels changed. `begin` cancels an old unfinished shape before starting another. `select` only cancels when the tool actually changes; selecting the current tool is a no-op. `clear` discards all paths/preview but retains the selected tool.
- `controls.rs` owns global registrations and mapping. Preserve permanent IDs 1/2/3 for Ctrl+Alt+D/C/Q. Add actions `Pen`, `Line`, `Rectangle` at IDs 4/5/6 respectively, and `Hotkeys::drawing_tools() -> Result<Self>` to register bare P/L/R with MOD_NOREPEAT only. Drop unregisters each guard's own IDs, including partial rollback. No dependency on strokes.rs.
- Main registers tool keys before entering drawing mode and releases them on returning to click-through or exiting. A tool-key registration conflict leaves the app click-through with ink intact and reports the error; it must not discard work. Ignore stale tool-selection messages in click-through. Changing tool/mode cancels unfinished shape previews, ends drag capture and preserves completed annotations/freehand ink.
- Pointer-up and detected physical release commit the last preview. Capture loss/cancellation cancels an unfinished shape. Main may add a small `DragCapture::has_capture() -> bool` helper in input.rs to distinguish lost ownership from ordinary release, retaining the active-only watchdog and resource ordering.

### Work and verification

Two independent workers own strokes.rs and controls.rs, run focused tests, and commit in isolated worktrees. The supervisor is the single persistent integrator/composer in integration/poc3, reusing its worktree and Cargo cache. Rebase finished worker branches onto that branch, resolve conflicts serially and fast-forward the integration branch. No per-delivery build/test/lint runs and no promotion of partial work to main.

The supervisor composes main/input handling, updates README and smoke checks, then runs combined build, tests, formatting/Clippy and applicable app-local smoke. Failed validation creates focused worker fix tasks; integrate and revalidate. Main advances only to the validated HEAD. Record original/rebased delivery commits. At most four workers; no worker is rebased while active.

Worker tests cover rectangle drag directions, preview replacement, release versus cancel, same-tool no-op, pen retention, hover/clear/separate annotations and hotkey mapping/modifiers. Combined tests and posted messages cover geometry and routing; actual keyboard/physical mouse/focus acceptance remains explicit manual evidence.

## PoC 2 history

Implemented and verified with build, 26 tests, formatting, Clippy and 22 desktop smoke checks using posted stroke messages. Physical mouse/focus/outside-release/latency validation remains manual; detailed results are in tasks/todo.md.

Approved by the user after reporting PoC 1 works. Replace automatic shapes with an empty startup canvas and one fixed orange 5-pixel pen. Ctrl+Alt+D toggles drawing/click-through; Ctrl+Alt+C clears all strokes; Ctrl+Alt+Q quits. Drag the logical primary mouse button to draw. Switching mode ends the current stroke and preserves its visible points. Clear ends the drag and discards all strokes; holding the button after clear does not resume drawing until a new press. No toolbar, undo, shapes, settings or new dependencies.

### Responsibilities and fixed interfaces

- `strokes.rs`: pure state, independent of Windows and pixels. `#[derive(Default)] pub struct Strokes`; `begin(point: (i32,i32))`, `append(point: (i32,i32)) -> bool`, `finish()`, `clear()`, `is_active() -> bool`, `paths() -> &[Vec<(i32,i32)>]`. Keep separate strokes including the active one, ignore hover/duplicate points, preserve completed paths. All input points come from signed 16-bit Win32 client coordinates.
- `drawing.rs`: replace automatic geometry with rasterization of provided paths. `render(pixels: &mut [u32], width: usize, drawing: bool, paths: &[Vec<(i32,i32)>])`. A single point draws a dot; interpolate adjacent points without joining separate strokes. Preserve premultiplied alpha, 5-pixel fixed color and clipping.
- `surface.rs`: retain DIB/DC ownership; change `present(&mut self, hwnd: HWND, drawing: bool, paths: &[Vec<(i32,i32)>]) -> Result<()>`. Empty paths replace the old cleared flag.
- `input.rs`: decode only overlay HWND messages via `event(&MSG, HWND) -> Option<Event>` with `Event::{Down((i32,i32)), Move { point:(i32,i32), left_down:bool }, Up((i32,i32)), Cancel, CheckRelease}`. Ignore other HWNDs and unrelated timers. Native `DragCapture::begin(HWND) -> Result<Self>` owns capture and a 32ms active-drag-only watchdog; `lost_or_released(&self) -> bool` checks capture ownership and primary-button high bit; Drop stops timer and releases capture only if still owned.
- `overlay.rs`: window styles/lifecycle/focus as before; remove obsolete mouse-coordinate logging now handled by main. No mutable stroke state in WindowProc.
- `main.rs`: owns drawing mode, Strokes and Option<DragCapture>; routes queued events, redraws only on changed points/mode/clear, and ends drag on release, capture loss, clear, toggle or exit. Drop drag guard before window, window before surface. Register quit before showing as before.

### Capture and rendering decisions

A nonactivating background window cannot rely on SetCapture for outside-screen button-up. Use a watchdog only during an active drag, not a permanent idle poll. The timer checks GetCapture and GetAsyncKeyState high bit, accounting for SM_SWAPBUTTON and physical left/right semantics. Conservatively end on unavailable input state. Ignore leftover timer messages after a drag. Capture-loss messages may be synchronous to WindowProc; the watchdog observes ownership independently without sharing mutable application state with the callback.

Retain full-screen CPU fill/path replay and bitmap upload for this proof. Measure actual interaction at the tested resolution before switching renderers. This has O(total stroke points) replay plus full-screen upload per changed event; no GPU rewrite or speculative frame scheduler now. Pointer lag and outside-release behavior need real desktop validation, not just CPU timing or posted test messages.

### Dependency-ready work and verification

Three independent workers: stroke state; drawing/surface path rendering; input/overlay. Composition and README/smoke updates start only after all three are integrated. Maximum four workers, isolated worktrees, worker commits, one serial integration queue. Integration of the renderer includes a temporary main call-site adapter using empty paths so every intermediate main builds. Remove that temporary behavior during final composition.

Verify pure state transitions, sparse connected paths/dots/separate strokes/clipping/clear/alpha, signed mouse decoding and capture ownership cleanup where supported. Then build/test/Clippy and safe app-local smoke checks. Do not inject real system mouse/keyboard input on a shared desktop. Manual acceptance: draw over Notepad, switch and type/click normally, resume drawing; release outside the primary display; clear/toggle/quit while dragging; observe lag on the user's display.

## PoC 1 history

PoC 1 was implemented through isolated workers and serial integration. Build, 9 tests, formatting, Clippy, and desktop rendering/cleanup smoke checks passed. User reports it works fine and approved PoC 2. Earlier detailed input/focus matrix was not fully automated. Display/capture tasks 4 and 5 remain deferred. Historical results are preserved below and in tasks/todo.md.

Build one small Rust executable to prove that the proposed native overlay can render a line and rectangle above Windows applications and switch input ownership safely. Draw both shapes programmatically at startup, not through user interaction.

Assumptions: Windows 10/11, mouse input, ordinary non-elevated applications, primary display first. Target OS confirmation is complete; hardware and preferred sharing application are not yet specified.

## PoC 1 minimal application

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

PoC 2 input behavior:
- Mouse capture restrictions: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture
- Capture release: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-releasecapture
- Capture-change delivery: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-capturechanged
- Active drag watchdog: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-settimer
- Queued timer caveat: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-killtimer
- Physical button state: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getasynckeystate
- Mouse coordinates and primary-button flag: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousemove

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
