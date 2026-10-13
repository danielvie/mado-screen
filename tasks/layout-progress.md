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

## Pending gates

- Committed UI delivery and actual diff review.
- Hidden-native UI tests and directly affected input/color routing regressions.
- Offscreen visual review of compact, expanded, recent-color and picker states.
- Release build, cargo fmt --check and cargo clippy --all-targets -- -D warnings.
- Serial integration and promotion preserving user-owned files.

No full test suite, desktop smoke, visible-window launch, real input injection or cursor movement is authorized for this increment. Physical hover/click, picker interaction, foreground focus and mixed-DPI appearance remain user acceptance checks, not established by hidden-window tests.
