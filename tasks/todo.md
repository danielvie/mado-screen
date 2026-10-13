# Windows prototype tasks

## PoC 2: freehand drawing

User reported PoC 1 works and approved this increment. No PoC 2 behavior has been validated yet.

- [ ] P2-state: `strokes.rs` stores separate paths and active drag, rejects hover/duplicate points, preserves ink on finish, clears safely. Dependencies: PoC 1. Verify pure state tests. Owns only strokes.rs.
- [ ] P2-render: replace automatic shapes with dots and connected freehand paths in drawing.rs/surface.rs. Dependencies: fixed state/path contract, not state implementation. Verify empty startup, dots, sparse interpolation, separate paths, clipped points, mode alpha and GDI cleanup. Owns only drawing.rs/surface.rs.
- [ ] P2-input: input.rs decodes mouse/timer/cancel messages and owns active-only native capture watchdog; overlay.rs drops obsolete logging. Dependencies: PoC 1. Verify signed coordinates, HWND/timer filtering, primary-button semantics and guard cleanup without visible desktop interaction. Owns only input.rs/overlay.rs.
- [ ] P2-compose: main owns stroke/mode/drag guard, draws on changed points, keeps ink in click-through, clears/ends/releases safely; README and smoke match new empty startup. Dependencies: P2-state, P2-render, P2-input integrated. Owns main.rs/README.md/scripts/smoke.ps1. Verify build/test/check and safe app-local smoke, no injected system input.
- [ ] P2-integrator: one completion-order queue, isolated integration worktrees, checks after cherry-pick, fast-forward main; at most four workers, no concurrent integration.
- [ ] P2-supervisor: review final sources/commits, run final checks, record app-local smoke versus actual hardware/manual evidence, commit verification in a worktree.

### PoC 2 acceptance

- [ ] Empty startup; logical primary-button drag produces a fixed orange 5-pixel stroke, including a click dot and sparse-event interpolation.
- [ ] Distinct strokes do not join; hover does not draw; mode switches end current stroke and preserve ink.
- [ ] Clear while dragging releases capture, empties canvas and does not resume on a held button; toggle and quit during drag also release safely.
- [ ] Button release outside the primary display, capture loss and stale timer events do not leave drawing stuck.
- [ ] Draw over Notepad, toggle to click-through, type/click normally, then resume drawing without unwanted focus changes.
- [ ] Idle still blocks with no timer after drag; actual pointer responsiveness at the tested resolution is observed and not inferred from unit tests.

### PoC 2 results

Pending. PoC 1 historical results and deferred multi-display/presentation tasks remain below.

## PoC 1 history

PoC 1 implemented and integrated: native overlay, automatic shapes, controls, and nine passing tests. Build, formatting and Clippy pass. Desktop rendering/cleanup smoke checks pass; real input and focus checks remain manual. First-level PoC includes tasks 1 through 3 on the primary display; tasks 4 and 5 come later.

## Parallel implementation queue

- [x] Baseline: rasterization, native prototype, Cargo project, Taskfile; build/test/check pass.
- [x] W1: extract `surface.rs` with DIB/DC ownership and bitmap upload. Implemented and integrated. Dependencies: baseline.
- [x] W2: extract `overlay.rs` with hidden window creation, input styles, callback, and destruction. Implemented and integrated. Dependencies: baseline.
- [x] W3: extract `controls.rs` with shortcut guard and action decoding. Implemented and integrated. Dependencies: baseline.
- [x] W4: compose modules in `main.rs`, add concise README. Implemented and integrated. Dependencies: W1, W2, W3 integrated. Verified with `task build`, `task test` (9 passed), and `task check`; actual input/focus acceptance remains manual.
- [x] Integrator: process completed worker commits serially in worktrees; verify and fast-forward main after each integration. All four commits integrated; no blocked tasks.
- [x] Supervisor: final build/test/check, review integrated diff, perform available desktop checks, record limitations, commit results. Verification changes prepared in a separate worktree and integrated serially.

Workers own separate files. At most four workers; integration is strictly serial. The next dependency-ready task starts after its dependencies are integrated.

## 1. Automatic line and rectangle overlay

Create the Cargo executable, Taskfile commands, and primary-monitor layered window. Render one fixed line and one outline rectangle automatically at startup. Start click-through; there are no drawing tools. Add a small runnable check for rasterized shape pixels and premultiplied-alpha math.

Acceptance:
- [x] Line and rectangle appear above ordinary windows without user drawing interaction. Actual desktop pixel samples passed.
- [x] Desktop remains visible around the shapes and inside the rectangle. Transparent pixel tests passed; baseline desktop observation confirmed the unfilled interior.
- [ ] Clicks and scrolling reach the underlying application through both shapes and blank areas.

Verification: `task test`; `task build`; inspect the actual overlay over Notepad and a browser, including clicks directly on the shapes.
Dependencies: none. Scope: small, roughly 3 files including Cargo.toml and Taskfile.yml.

## 2. Global controls without focus theft

Register fixed input-mode toggle, clear, and quit hotkeys. Check registration failures. Keep the overlay from activating on mouse interaction. In capture mode, test nonzero background alpha so blank areas receive mouse events and log coordinates only. No input creates shapes.

Acceptance:
- [ ] Controls work while Notepad or the browser has foreground focus; capture mode logs blank-area clicks.
- [ ] Clicking the overlay does not steal keyboard focus; click-through resumes without clicking to refocus. Record the capture-mode tint.
- [ ] Quit works in either mode; hotkey conflicts fail safely rather than trapping input.

Verification: `task build`; type into Notepad, toggle modes repeatedly, simulate a conflicting shortcut, and quit while the mouse button is held.
Dependencies: 1. Scope: small, 1-2 files.

## Checkpoint: safe desktop integration

- [ ] Proofs 1 and 2 pass on Windows hardware.
- [ ] Human reviews input behavior before further implementation.

## 3. Static rendering persistence and idle validation

Validate the startup shapes without adding interaction or new tools. Redraw only on changes, not in an idle loop.

Acceptance:
- [ ] Line and rectangle remain visible while switching modes and moving underlying windows.
- [ ] Clear removes both shapes without leftover pixels or focus theft.
- [x] Record idle CPU over 30 seconds, hardware, resolution, and whether idle redraw occurs; report against the proposed idle target. Process CPU time unchanged; blocking loop has no idle redraw timer.

Verification: `task test`; `task build`; actual desktop checks over Notepad/browser. Restart to restore shapes after clearing. Do not interpret a static render as proof of interactive drawing latency.
Dependencies: 2. Scope: small, 1-2 files plus proof results.

## Checkpoint: first-level PoC

- [ ] Launch and see the automatic line and rectangle; use the underlying application while they remain visible.
- [ ] Static rendering, safe input switching, clear, quit, and idle checks have explicit results.
- [x] Human reviews the primary-display PoC before later drawing or display/capture validation. User reports it works fine and approved PoC 2; this is not a detailed automated input matrix.

## 4. Monitor and DPI correctness

Extend the working overlay to one window per monitor. Place automatic shapes and map logged mouse coordinates correctly with Per-Monitor V2 DPI awareness.

Acceptance:
- [ ] Shape placement and logged pointer coordinates are correct at available 100%, 150%, and 200% scaling configurations.
- [ ] Mixed-DPI and negative-coordinate monitor layouts work where hardware permits.
- [ ] Shapes do not clip incorrectly near display edges; restart correctly after display-layout changes.

Verification: `task test`; `task build`; manual monitor matrix with passed/failed/untested recorded.
Dependencies: 3. Scope: small to medium, 1-3 files.

## 5. Capture and presentation validation

Test the executable without adding presentation integrations.

Acceptance:
- [ ] Whole-display sharing in the chosen application shows annotations to the recipient.
- [ ] Record application-window capture and borderless/exclusive fullscreen results separately.
- [ ] Normal desktop interaction remains recoverable after every tested scenario; unsupported cases are documented.

Verification: `task build`; actual recording or remote recipient check, not just the local preview. Secure desktop is excluded.
Dependencies: 3; include 4 when multiple displays are available. Scope: results in this file, no new application subsystem.

## Final checkpoint

- [x] Run, build, test, and clean commands work. `task run` launched the overlay and exited via queued quit; `task clean` passed in the verification worktree.
- [x] Every proof has a result or an explicit reason it remains untested. See results; input/focus and deferred display/capture checks are not claimed as passed.
- [x] Human decides whether to extend the prototype into an application. Approved the next freehand proof; no full-product scope approved.

## Results

## Automated results

Integrated source reviewed at 7de673c; final verification also sets WS_EX_TOPMOST at creation after a smoke check observed inconsistent startup z-order. Four worker tasks finished in isolated worktrees and were integrated in completion order: controls, overlay, surface, composition. Main was fast-forwarded after each serial integration; no force/reset or parallel integration was used.

- `task build`: passed.
- `task test`: 9 passed, covering pixels, alpha, action decoding, style preservation, noactivation callback, dimensions, GDI ownership and resource drop order.
- `task check`: formatting and Clippy passed.
- Desktop smoke: 17 checks passed using a copied integrated executable. Verified window creation/styles, visible line/rectangle samples, idle CPU, clear/quit action dispatch, window removal, real shortcut-registration conflicts and rollback. `task smoke` leaves logs/results under `%TEMP%` and does not inject mouse/keyboard input.
- Hardware: AMD Ryzen 7 7800X3D, 16 logical processors; Windows kernel 10.0.26200.0; primary display 3440x1440 physical pixels.
- Idle sample: process CPU time delta 0 seconds over 30 seconds, reported utilization 0% normalized over 16 logical processors. One sample only; not a performance guarantee.
- Final smoke artifact with startup topmost style set at creation: C:/Users/daniel/AppData/Local/Temp/mado-poc1-4095bcbe-148f-44fa-906c-6515411ade5e/results.json.

## Remaining manual checks

- Real Ctrl+Alt+D/C/Q delivery with Notepad/browser active, mouse click/scroll routing across blank areas and shapes, focus preservation, capture-mode tint and quit with mouse held. Earlier baseline synthetic-input checks were partial and became unreliable when desktop focus/cursor changed; they do not count as a complete final pass.
- The smoke test queues clear/quit directly; it proves action handling, not global keyboard delivery. It does not exercise capture mode.
- Persistence across actual mode switching and underlying-window movement, and clearing without focus changes still need controlled desktop observation.
- Multiple displays/mixed DPI, screen sharing and fullscreen compatibility are deferred to tasks 4/5. Sharing application is still unspecified.
- User subsequently reported PoC 1 works fine and approved PoC 2. Detailed scenarios above were not individually signed off.
