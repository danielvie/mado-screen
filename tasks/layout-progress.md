# Compact panel and picker layout progress

## Scope and delivery

Approved contract: tasks/layout-spec.md. User approved the supplied layouts, then specified tool-icon-only compact state, primary monitor top-right anchoring, expansion to the left and five distinct session-only recent colors.

Base main: 25df40e. Supervisor uses integration/layout in a sibling worktree. Worker uses worker/layout-ui and owns src/ui.rs only. The supervisor updated documentation and the stale desktop-smoke style expectation at dea8b62. This changes the expected panel from mouse-transparent to interactive/nonactivating; desktop smoke is not run for this increment.

Primary README.md and Taskfile.yml install edits and the untracked docs/design/mado-screen-layout-and-use-cases.excalidraw are user-owned. A patch and drawing hash were saved under ignored target/layout-preserved before promotion. Installed app remains untouched.

## Completed checks

- PowerShell parser accepted scripts/smoke.ps1 after the style expectation update. This is a syntax check only, not a desktop test.
- git diff --check passed for the supervisor's documentation/smoke changes.
- A read-only routing review found the existing HWND-gated canvas input and Color/Closed/committed update_label routes sufficient. The supervisor extracted the unchanged Color settlement into apply_ui_color so it can be tested without creating a visible UI.
- cargo test recent_color_ passed 2 tests at c548001. Cases cover same/different color settlement for Pen/Line/Rectangle, unchanged old styles, ignored trailing Move/Up, temporary birth preservation and unchanged permanent redo. No canvas capture or native activation was required.

## Final delivery and verification

Worker delivery 77f03ff changed src/ui.rs only and was rebased serially to f1f55b2 after supervisor changes. Actual code and all six tool icons were inspected. The panel receives input through its rounded region, uses MA_NOACTIVATE, tracks native mouse leave without polling and posts existing Color events. Committed update_label state records a five-entry newest-first deduplicated color history. The color picker keeps native controls and existing focus/validation/Apply/Cancel semantics.

The first offscreen picker review found clipped hue/footer labels. The worker repaired layout and added actual GDI text-size assertions at 100% and 150%. Final picker previews at both scales and compact/expanded/recents panel previews passed visual inspection. They are retained under target/layout-previews in the primary checkout; these are hidden-window rasters, not desktop screenshots.

Assembled integration checks passed:
- cargo test ui::tests -- --nocapture: 4 tests. Includes panel anchor/hover/leave, occupied region, all distinct icon-only states, no focus/capture changes, stale clicks, five-color history, open-picker quick selection, candidate isolation, Cancel/Apply, invalid hex, native keyboard/control cleanup, clipping and DPI geometry.
- cargo test recent_color_: 2 parent routing tests.
- cargo build --release.
- cargo fmt --check.
- cargo clippy --all-targets -- -D warnings.
- git diff --check.
- PowerShell parser accepted the updated smoke script. Its stale startup panel lookup now uses the accessible tool/mode title; the canvas style assertion is unchanged. Desktop smoke did not run.

Only focused tests ran, not the complete 115-test suite. Promotion fast-forwards main after these gates; copy the optimized binary to target/release, not the installed app. Preserve local install edits and the untracked Excalidraw.

## Win32 sources

The worker fetched and checked official API documentation:
- TrackMouseEvent and native leave tracking: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackmouseevent
- Region ownership after success: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowrgn
- Layered-window shape hit-testing versus WS_EX_TRANSPARENT: https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows
- Deliver a click without activation: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate
- Primary monitor selection: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfrompoint
- Nonactivating position/size updates: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos

No full test suite, desktop smoke, visible-window launch, real input injection or cursor movement is authorized for this increment. Physical hover/click, picker interaction, foreground focus and mixed-DPI appearance remain user acceptance checks, not established by hidden-window tests.
