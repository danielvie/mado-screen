# Windows prototype tasks

## Disk color picker follow-up

User reports PoC 4 works fine and approved the disk design in docs/design/disk-color-picker.excalidraw. Implementation contract and final evidence: tasks/disk-picker.md. Scope is the picker pane only; existing annotation/history/control behavior is preserved.

- [ ] Pure HSV/ARGB/hex math and bounded disk selection.
- [ ] Dark DPI-scaled disk pane, brightness, candidate/hex/presets and Apply/Cancel.
- [ ] Hidden native checks, offscreen visual review, combined build/test/check/desktop smoke.
- [ ] Validated promotion, origin/main push, primary executable rebuild and integration cleanup.

## PoC 4: editing, styles and native UI

Specification: tasks/poc4-spec.md. Stage evidence and remote checkpoints: tasks/poc4-progress.md. User authorized autonomous delivery and a remote push after each completed stage.

- [x] Stage 1: styled annotations, width/color controls, undo/redo and undoable clear. Validated and pushed at eaea201.
- [x] Stage 2: arrow/ellipse tools, whole-annotation erasing, sparse sweeps and grouped eraser undo. Validated and pushed at c7f7ad4.
- [x] Stage 3: visible mode/tool/style indicator, cursor-position RGB picker, temporary shortcut suspension and safe reacquisition. Validated 7f934a7: 55 tests, 79 desktop checks, zero skipped; final push follows recorded gates.
- [x] Final: completed source/evidence recorded; primary executable rebuilt and own integration worktree removed after successful final promotion/push.

Physical popup focus/placement/navigation and actual pointer/key behavior remain manual. Multiple monitors and sharing/recording remain deferred/open. Remaining capabilities: docs/feature-roadmap.md.

## PoC 3: tool selection and shape previews

- [x] P3-state: pen/line/rectangle gesture geometry in strokes.rs. Worker 21fbbe1, 15 focused tests passed; integrated unchanged via serial rebase/fast-forward.
- [x] P3-controls: drawing-only P/L/R registration guard and IDs/actions. Worker 45f1e05, 3 focused tests and scoped Clippy passed; rebased to 19d2ef6 and integrated after state. Full old-main test lint was correctly reported blocked before composition.
- [x] P3-compose: main/input mode, tool selection, commit/cancel routing, recoverable key conflicts; README and smoke coverage. Supervisor in the same persistent integration worktree.
- [x] P3-validation: build/test/check passed with 38 tests; posted desktop smoke passed 49 checks with zero skipped. One worker repair fixed stale background references and batched synthetic gestures; revalidated after serial integration.
- [x] P3-promotion: code HEAD cbc79f1 verified before promotion; main fast-forwarded only after required gates passed. Final record is documentation-only.

### PoC 3 acceptance

- [x] Drawing-only P/L/R actions select Pen/Line/Rectangle; bare keys are owned only while drawing and released in click-through. Posted actions and actual registration scope passed; character typing remains manual.
- [x] Posted gestures preview line/rectangle, release commits, all rectangle directions pass state tests and previews leave no trails in raster/desktop checks. Physical mouse behavior remains manual.
- [x] Changing tool/mode cancels active shape previews but preserves completed annotations and partial pen ink; same-tool selection leaves the gesture active. State/routing tests and posted cancellation checks passed.
- [ ] Clear during drag removes all annotations and prevents held-button resumption; capture loss cancels a preview and release stops safely.
- [x] Tool-key conflicts roll back registration and preserve ink in click-through; permanent quit/toggle/clear remain available. Reserved-R and permanent-quit conflict checks passed.
- [ ] Real mouse and unmodified letter keys, focus, outside release and responsiveness are observed on the user's desktop, not inferred from posted messages.

### PoC 3 results

User reports "poc 3 worked". This records general acceptance; individual untested hardware scenarios above remain open.

Validated code HEAD: cbc79f1. Main remained at fcbe14a throughout assembly and validation. Two independent feature workers were used; the supervisor composed and integrated in one persistent worktree. State arrived first, then controls. Original/rebased deliveries: 21fbbe1 -> 21fbbe1; 45f1e05 -> 19d2ef6. No conflicts and no per-delivery build/test/lint commands.

Final gates: task build passed, task test 38 passed, task check formatting/Clippy passed. One cold dependency build occurred in the persistent worktree; the repair validation reused it.

First smoke check failed on a stale prelaunch background comparison. Repair worker cbc79f1 changed only the smoke harness, with focused PowerShell parse/self-checks and no app launch. Fresh owned-window hide/restore references now classify moving samples as skipped, not passed; posted gestures/cancellation use fast native batches. After delivery, reran combined gates and desktop smoke.

Desktop smoke: 49 passed, zero skipped, 3440x1440 primary display, 16 logical processors. Initial idle process CPU delta 0 seconds over 30 seconds. Verified committed pen/line/reverse rectangle, rectangle interior, no preview trails, tool/mode cancellation, P/L/R registration scope, recoverable bare-R conflict, ink retention, clear, quit and cleanup. Artifact: C:/Users/daniel/AppData/Local/Temp/mado-poc3-c30e8bec-6f93-4272-a087-a890c449bae9/results.json.

No real keys/mouse events were injected and no cursor/focus was moved. These results do not establish physical letter typing, actual focus behavior, outside-display release or pointer latency. Live held-button scenarios and multi-monitor/sharing/fullscreen remain manual or deferred.

## PoC 2: freehand drawing

PoC 2 implemented. All 26 tests, build, formatting and Clippy pass. Desktop smoke passed 22 checks with a posted stroke. Physical mouse, focus, outside-release and responsiveness acceptance remain manual.

- [x] P2-state: `strokes.rs` stores separate paths and active drag, rejects hover/duplicate points, preserves ink on finish, clears safely. Implemented and integrated; seven pure state tests passed. Dependencies: PoC 1. Owns only strokes.rs.
- [x] P2-render: replace automatic shapes with dots and connected freehand paths in drawing.rs/surface.rs. Implemented and integrated; seven raster tests and native DIB/resource checks passed. Dependencies: fixed state/path contract, not state implementation. Verify empty startup, dots, sparse interpolation, separate paths, clipped points, mode alpha and GDI cleanup. Owns only drawing.rs/surface.rs.
- [x] P2-input: input.rs decodes mouse/timer/cancel messages and owns active-only native capture watchdog; overlay.rs drops obsolete logging. Implemented and integrated; five input decoding/button tests passed, native capture/watchdog behavior remains untested. Dependencies: PoC 1. Verify signed coordinates, HWND/timer filtering, primary-button semantics and guard cleanup without visible desktop interaction. Owns only input.rs/overlay.rs.
- [x] P2-compose: main owns stroke/mode/drag guard, draws on changed points, keeps ink in click-through, clears/ends/releases safely; README and smoke match new empty startup. Worker commit 40fbad9 integrated after the test-only Clippy fix. Dependencies: P2-state, P2-render, P2-input integrated. Owns main.rs/README.md/scripts/smoke.ps1. Verify build/test/check and safe app-local smoke, no injected system input.
- [x] P2-integrator: one completion-order queue, isolated integration worktrees, checks after cherry-pick, fast-forward main; at most four workers, no concurrent integration. Composition initially stopped on an input-test lint; supervisor corrected the dummy pointer in a separate integration worktree, reverified and completed serial integration.
- [x] P2-supervisor: reviewed final sources/commits, ran build/test/check and posted-stroke desktop smoke, recorded hardware limitations, committed verification in an integration worktree.

### PoC 2 acceptance

- [ ] Empty startup; logical primary-button drag produces a fixed orange 5-pixel stroke, including a click dot and sparse-event interpolation.
- [x] Distinct strokes do not join; hover does not draw; mode switches end current stroke and preserve ink. Pure routing/raster tests passed; posted stroke remained visible after returning to click-through.
- [ ] Clear while dragging releases capture, empties canvas and does not resume on a held button; toggle and quit during drag also release safely.
- [ ] Button release outside the primary display, capture loss and stale timer events do not leave drawing stuck.
- [ ] Draw over Notepad, toggle to click-through, type/click normally, then resume drawing without unwanted focus changes.
- [ ] Idle still blocks with no timer after drag; actual pointer responsiveness at the tested resolution is observed and not inferred from unit tests.

### PoC 2 results

Final composition and supervisor verification:
- `task build`: passed.
- `task test`: 26 passed.
- `task check`: formatting and Clippy passed. The first composition integration was blocked by clippy::manual_dangling_ptr in an input test. A standard dangling pointer replaces the fabricated dummy HWND address; production input code is unchanged.
- Desktop smoke with `-PostedStroke`: 22 checks passed. Empty startup, drawing-mode transition, posted down/move/up rendering, stroke persistence in click-through, clear, quit, registration conflict/rollback and window cleanup verified.
- Test display: 3440x1440, 16 logical processors. Initial idle sample: process CPU time delta 0 seconds over 30 seconds. Not a hardware input-to-display latency measurement.
- Smoke artifact: C:/Users/daniel/AppData/Local/Temp/mado-poc2-1fd89608-1c56-42d9-ad7a-978479bfab91/results.json.
- No real keyboard/mouse input was injected or cursor moved. Native posted messages prove routing/rendering, not physical drag accuracy, focus, outside-release behavior, live clear/toggle/quit while held or responsiveness. These acceptance boxes intentionally remain open.
- Multi-monitor, screen-sharing and fullscreen checks remain deferred.

Historical intermediate integration checks follow; their earlier pending statements do not describe the final build.

P2-state integrated from worker commit `955bfd254a914fd3d7c514b6472ab15a785a8f12`. Source changes are limited to `src/strokes.rs`; no conflicts or main adapter were needed.

- `cargo test --bin module_check`: 15 passed, including seven state regressions and existing drawing/surface/overlay/controls checks. No input module exists yet.
- `cargo check --bin module_check`: passed. The temporary path-import harness was deleted afterward.
- `task build`: passed; `task test`: nine existing application tests passed after harness removal.
- State is not wired into main until P2-compose. No app launch, desktop or hardware acceptance checks performed. Other PoC 2 tasks remain pending.

P2-render integrated from worker commit `d44487564fd4bf9a52262f35e5d8b658619c339e`. Main and the clean integration worktree shared HEAD `cd92888b1c579ad23d7b06f3ff673c3b4120e2f3` before cherry-pick, so no rebase was needed. The cherry-pick had no conflicts and changed only `src/drawing.rs` and `src/surface.rs`.

- `src/main.rs` temporarily passes `&[]` to all three `Surface::present` calls and drops the unused `cleared` variable/assignment. No other main behavior changed. Stroke composition and PoC 1 console text remain for P2-compose.
- `cargo test --bin module_check`: 19 passed across drawing/surface/overlay/controls/strokes, including seven raster regressions and native DIB/GDI lifecycle checks without a window. No input module exists yet.
- `cargo check --bin module_check`: passed. The temporary path-import harness was deleted afterward.
- `task build`: passed; `task test`: 13 application tests passed after harness removal.
- Ownership review found only renderer sources, the required main adapter and this integration record changed. No app launch, desktop or hardware acceptance checks performed. All other PoC 2 checkboxes retain their prior state.

P2-input integrated from worker commit `fd1b1775967fdcc827bb57cba920dc2d485fed55`. Main and the clean integration worktree shared HEAD `15320b531968f03f95d09588619841de85cb0b68` before cherry-pick, so no rebase was needed. The cherry-pick had no conflicts and changed only `src/input.rs` and `src/overlay.rs`; no main adapter was needed.

- `cargo test --bin module_check`: 24 passed across drawing/surface/overlay/controls/input/strokes, including five signed-coordinate, HWND/timer-filtering and primary-button regressions. Existing native DIB/GDI checks passed without creating a window.
- `cargo check --bin module_check`: passed. The temporary path-import harness was deleted afterward.
- `task build`: passed; `task test`: 13 application tests passed after harness removal.
- Ownership review found only input/overlay sources and this integration record changed. Input is not wired into main until P2-compose. Native drag capture, watchdog operation and capture cleanup remain untested; no app launch, desktop interaction or hardware acceptance checks performed. Only the P2-input checkbox changed.

PoC 1 historical results and deferred multi-display/presentation tasks remain below.

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
