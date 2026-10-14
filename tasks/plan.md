# Windows annotation proof of concept

## Menu reference and compact brush-tip follow-up

Implemented corrected reference order, smaller radii, toolbar hover/release feedback, no inner compact-box border, and the updated compact brush-tip circle in feature/floating-menu after b414fd5 was integrated to main. Eight new release-profile tests and a scoped 15-check release-app trial pass, with actual 125% native/desktop previews inspected. Contract, test boundaries and animation diagnosis: tasks/panel-menu-followup.md. Physical pointer acceptance remains manual. Respect disabled Windows animation preferences unless the user approves an override. Preserve the user's main-checkout design/install edits.

## Proposed floating menu with mouse controls

Status: implemented in the feature/floating-menu worktree; 14 new-feature tests and release build passed. Physical acceptance remains open. User requests only new-feature tests, not the full or legacy suites, and favors real manual acceptance. Use only the new graphite scheme. Do not promote, push or replace the installed app. Progress: tasks/panel-menu-progress.md. Older acceptance items and user-owned changes remain untouched.

Visual proposal: [panel layout and transitions](../docs/design/mado-screen-panel-layout-and-transitions.excalidraw). Implementation tasks: [floating menu checklist](todo.md#proposed-floating-menu-with-mouse-controls).

### Goal and layout

Users can select existing tools, stroke width, color and canvas input mode without remembering shortcuts. Small shortcut badges teach the existing keys.

- Two presentation states only: a 44 x 44 logical-pixel badge and a 440 x 304 logical-pixel full menu. Hover reveals the full menu directly; there is no status-row-only state.
- The badge retains the selected tool icon beside a separate far-right drag grip. The right edge stays fixed as the menu expands left. Near the bottom, open upward with the badge fixed.
- Full menu contains an input-mode toggle in the header, selected-stroke preview, odd-width slider, two rows of eight populated color slots and a clickable tool strip.
- Use graphite #191C20, warm-white #F1EEE8, neutral border #444B52 and UI accent #7CC7B4. Drawing colors remain literal, independent of the UI theme.
- Each tool icon includes a small readable shortcut letter. Tooltips include its full name, shortcut and variant behavior; selection is also marked by an outline, not color alone.
- Native Win32 and existing antialiased layered-panel rendering remain. No new dependency, UI framework or canvas-renderer rewrite.

### Tool and input behavior

| Button | Shortcut hint | Mouse action |
| --- | --- | --- |
| Pen | P | Select permanent Pen explicitly; repeated clicks are a no-op. |
| Temporary pen | P | Select temporary Pen explicitly. Tooltip says P selects Pen, then P again toggles temporary ink. There is no dedicated temporary key. |
| Line | L | Select straight line. |
| Arrow | A | Select Arrow, keeping the saved direction. Clicking an already-selected Arrow flips direction, matching repeated A. Show head-first/tail-first in status and tooltip. |
| Rectangle | R | Select outline rectangle. |
| Ellipse | O | Select outline ellipse. |
| Eraser | E | Select whole-annotation eraser. |
| Canvas input toggle | Ctrl+Alt+D | Switch Drawing/Click-through using the same transition as the global shortcut. |

Bare P and A keep their current select/repeat-to-toggle semantics. Do not map both Pen buttons to Action::Pen, because that would make exact mouse selection accidentally toggle.

The input toggle is a separate two-state control, not another drawing tool. It remains usable in click-through and does not hide the menu, clear ink or alter the selected tool. Entering drawing must acquire drawing shortcuts before enabling canvas input. A conflict preserves annotations, remains click-through and reports the actual key. Leaving drawing releases drawing shortcuts and capture.

Mouse tool, palette and width selection work in both input modes, preparing the next annotation without automatically entering drawing. The modeless picker disables those controls while editing; the input toggle remains usable and closes the picker through the existing mode-change path. The panel never takes keyboard focus for these actions.

Preserve gesture contracts: actual tool/variant/style changes settle partial pen ink, cancel unfinished shape previews, end temporary drags and release canvas capture. They do not change completed annotations or create tool/style history entries. A repeated exact selection is a no-op. Undo/redo remains permanent-history-only. Mode toggling must not restart temporary lifetimes.

### Fixed palette

Fill all slots at startup and keep their positions unchanged. This replaces the five-slot recent/replacement behavior only in the new full panel.

| Row | Left to right |
| --- | --- |
| Warm and purple hues | Yellow #FFDF32, amber #FFB51B, orange #F97316, red #E31635, violet #673B92, purple #9B4FC0, pink #D50D81, magenta #B9085E |
| Cool hues and neutrals | Cyan #35C7EF, azure #0085DC, blue #075CBF, lime #83C400, emerald #00B85A, white #F1F1EF, gray #BDBDBB, black #161616 |

Default remains orange and 5 px. Selecting a different slot applies its exact color without closing or reordering the menu. Preserve the existing current-color/full-picker behavior by making the selected stroke preview clickable; a selected matching palette swatch can also request the picker, as today. Picker Apply must not replace fixed slots. Highlight a slot only on an exact RGB match; a custom color may have no matching slot.

Existing numeric 1..5 shortcut colors remain unchanged. Some reference-derived slots differ from those legacy RGB values, so do not put misleading numeric badges on the new palette or silently change the legacy shortcuts.

### Implementation ownership and routing

- src/ui.rs owns panel geometry, icon/badge painting, hit targets, fixed palette, slider, animation, panel dragging and tooltip/accessibility metadata. Keep current UiEvent::Color, Closed and OpenPicker decoding compatible.
- Extend the pointer-free queued UiEvent protocol with validated selection, width and input-toggle requests. Use explicit integer values and reject unknown tool IDs or payloads. Do not send State pointers through queued messages.
- src/main.rs remains the owner of Strokes, Trail, drawing state, canvas capture and hotkey guards. Both keyboard and menu input use shared gesture-settlement and mode-transition helpers; UI callbacks do not change the canvas themselves.
- Use an exact mouse selection request, including permanent/temporary Pen, separate from existing repeated-key Action semantics. Existing Strokes getters and toggles support this without a new renderer or a new temporary Tool variant.
- src/controls.rs retains existing hotkey IDs/modifiers. Add only the smallest mapping/metadata required to verify the displayed shortcut hints.
- Keep main-driven committed UI updates. Highlight the actual tool/input state after successful handling, not an optimistic clicked state.
- Use current interpolated dimensions for hit-testing. Hidden/unrevealed controls cannot receive clicks. Confirm press and release on the same target; capture loss/cancellation cannot activate a stale target.
- Drag only the right grip, after a 4 logical-pixel threshold. Hold the current size during dragging. Clamp the complete expanded footprint to the destination work area on drop and keep position for this run only.

### Motion

Use a 150 ms ease-out for width and height together. Content fades and clips into the growing bounds; the fixed badge stays opaque. Reverse from current progress on pointer re-entry without resetting. Keep the menu open while the pointer or an active slider/grip gesture is inside it.

Respect Windows disabled-animation preferences. Use a dedicated timer only during a transition, separate from trail expiry and canvas drag watchdog timers. Stop it when settled. No idle animation loop or polling. Verify Win32 capture restrictions for a nonactivating panel; do not assume capture alone guarantees outside-window release.

### Build order

1. Mouse tool selection and accurate shortcut badges, wired through main.
2. Reachable input-mode toggle sharing the existing global toggle path.
3. Fixed two-row palette and working width slider.
4. Smooth two-state reveal and fixed-right dragging.
5. Combined hidden/native checks, offscreen visual review and user acceptance.

Each slice must remain runnable and leave its smallest focused regression check. Work sharing ui.rs or main.rs is sequential. The user approved implementation in an isolated worktree. If implementation uses workers later, follow docs/workflow-strategy.md with one serial integrator.

### Validation

Approved commands for this implementation loop:

- cargo test panel_menu_ -- --test-threads=1
- cargo build --release
- git diff --check

New tests use the panel_menu_ prefix, including tool/variant selection, width, fixed palette, input toggle rollback, typed events, motion and placement. Older recent-color tests may need compilation changes because the five-slot history UI is removed; do not execute legacy tests in this loop. The user explicitly overrides the normal full-suite/smoke gate: run only new-feature tests and a release build, then favor manual acceptance. Format changed source files only; no full-project lint/test pass.

Offscreen renders at 100%, 150% and 200% check all badges, long arrow labels, selected outlines, black-slot visibility, both input states and animation endpoints. Include hidden/native event decoding, invalid payloads, click cancellation, resource cleanup and no UI-owned focus changes. Elapsed-time injection verifies reversal, disabled motion and timer termination deterministically.

Real mouse clicks, outside-window slider/grip release, normal typing after toggle, assistive-technology discoverability and mixed-DPI placement remain manual checks. Do not claim these from posted events or hidden windows. Do not inject system input, move the cursor, launch/close the user's app, overwrite the installed executable, push remotely or modify unrelated README.md/Taskfile.yml edits.

### Approved scope

Implement the depicted full menu, exact Pen/Temporary mouse choices, repeat-click Arrow direction, fixed colors, input toggle, motion and grip dragging. Retain legacy numeric shortcut RGB values. Only new-feature tests run; manual observations remain unverified until the user tries the release binary.

## Current execution strategy

Future batches follow [the workflow strategy](../docs/workflow-strategy.md). Use up to four worktree workers, one persistent serial integrator, validation after all deliveries, worker repair loops, and promotion to main only after required checks pass. Earlier per-delivery verification and promotion descriptions below are historical, not the current workflow policy.

## Panel polish follow-up

Requested changes: smooth compact panel corners/icons at the existing size; show recents immediately on hover; click the selected color to open the full picker; shrink expanded width to fit content. Scope/results: tasks/panel-polish.md. Worker owns ui.rs, parent owns main OpenPicker routing and docs in sibling worktrees. Focused hidden/native/pixel checks and release build/lints only. No full suite, desktop smoke or installed-app replacement.

## Compact panel and picker layout

Approved scope: tasks/layout-spec.md. One UI worker owns src/ui.rs in worker/layout-ui; supervisor owns documentation, stale smoke expectations and serial integration in integration/layout. Reuse the existing Color event and committed update_label state to avoid changing canvas drawing. Deliver icon-only compact, leftward hover expansion, bounded session recents and reference-led picker presentation. Validate focused native/UI regressions, release build and formatting/Clippy only. Results and manual limits: tasks/layout-progress.md. Do not run desktop smoke or disturb an active overlay.

## PoC 7 follow-up: Bézier trial and performance

The user reports release mode much better, approves a freehand Bézier trial and asks for performance measurements. Keep shape tools, arrow size, widths and one-second lifetime unchanged. Use streaming midpoint quadratics with a live tail, measure release rendering before/after, and retain only measured optimization wins. Worker owns bezier/strokes/trail; supervisor owns renderer and offscreen perf check in a persistent integration worktree. Scope, measurements and focused validation are in tasks/bezier-performance.md. No full suite or desktop smoke.

## PoC 7: rounded ink and fading

Approved scope and contracts: tasks/poc7-spec.md. Renderer and shape geometry use separate sibling worker worktrees; the supervisor owns Trail/frame timing/Surface composition, documentation and serial integration. Deliver rounded rendering, one-second fading, larger open arrowheads and smoother ellipses without another backend or dependency. The user requests new-feature and affected regression tests only, plus a build and formatting/Clippy; no full suite or desktop smoke for this increment. Results and physical limits: tasks/poc7-progress.md.

## PoC 6: temporary pen and arrow variants

User approved solid temporary ink lasting about two seconds per part, excluded from undo/redo; repeated P toggles Pen/Temporary pen and repeated A toggles head-first/tail-first arrows. Indicator displays the variant. Requirements and module contracts are in tasks/poc6-spec.md; results are in tasks/poc6-progress.md. Model and Trail workers own separate files in sibling worktrees; the supervisor composes timer/input/render/UI and integrates committed deliveries serially. Visual fading remains the next PoC, sharing deferred.

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
