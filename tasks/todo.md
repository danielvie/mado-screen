# Windows prototype tasks

PoC 1 baseline implemented: native overlay, automatic shapes, controls, and three passing pixel tests. Build and Clippy pass. Desktop validation has not yet been performed. First-level PoC includes tasks 1 through 3 on the primary display; tasks 4 and 5 come later.

## Parallel implementation queue

- [x] Baseline: rasterization, native prototype, Cargo project, Taskfile; build/test/check pass.
- [ ] W1: extract `surface.rs` with DIB/DC ownership and bitmap upload. Dependencies: baseline.
- [x] W2: extract `overlay.rs` with hidden window creation, input styles, callback, and destruction. Implemented and integrated. Dependencies: baseline.
- [x] W3: extract `controls.rs` with shortcut guard and action decoding. Implemented and integrated. Dependencies: baseline.
- [ ] W4: compose modules in `main.rs`, add concise README. Dependencies: W1, W2, W3 integrated.
- [ ] Integrator: process completed worker commits serially in worktrees; verify and fast-forward main after each integration.
- [ ] Supervisor: final build/test/check, review integrated diff, perform available desktop checks, record limitations, commit results.

Workers own separate files. At most four workers; integration is strictly serial. The next dependency-ready task starts after its dependencies are integrated.

## 1. Automatic line and rectangle overlay

Create the Cargo executable, Taskfile commands, and primary-monitor layered window. Render one fixed line and one outline rectangle automatically at startup. Start click-through; there are no drawing tools. Add a small runnable check for rasterized shape pixels and premultiplied-alpha math.

Acceptance:
- [ ] Line and rectangle appear above ordinary windows without user drawing interaction.
- [ ] Desktop remains visible around the shapes and inside the rectangle.
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
- [ ] Record idle CPU over 30 seconds, hardware, resolution, and whether idle redraw occurs; report against the proposed idle target.

Verification: `task test`; `task build`; actual desktop checks over Notepad/browser. Restart to restore shapes after clearing. Do not interpret a static render as proof of interactive drawing latency.
Dependencies: 2. Scope: small, 1-2 files plus proof results.

## Checkpoint: first-level PoC

- [ ] Launch and see the automatic line and rectangle; use the underlying application while they remain visible.
- [ ] Static rendering, safe input switching, clear, quit, and idle checks have explicit results.
- [ ] Human reviews the primary-display PoC before later drawing or display/capture validation.

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

- [ ] Run, build, test, and clean commands work.
- [ ] Every proof has a result or an explicit reason it remains untested.
- [ ] Human decides whether to extend the prototype into an application.

## Results

Not run. Hardware configuration and primary sharing application still need to be recorded.
