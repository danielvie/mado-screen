# Mado Screen PoC 4

Native Rust/Win32 annotation overlay for the primary display. Startup is empty and click-through. PoC 4 adds history, styled pen/line/rectangle/arrow/ellipse annotations, whole-annotation erasing, a visible status indicator and a mouse-position RGB picker. Delivery evidence is in tasks/poc4-progress.md; specification is in tasks/poc4-spec.md.

## Commands

Windows 10 version 1703 or newer or Windows 11; stable Rust Windows MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK. Task is optional.

| Command | Purpose |
| --- | --- |
| `task run` / `cargo run` | Start the overlay |
| `task build` / `cargo build` | Build |
| `task test` / `cargo test` | Pure model/render/input/control and native resource checks |
| `task check` | Formatting and Clippy |
| `task smoke` | Copied-app desktop checks, including 30-second idle sample |
| `task clean` / `cargo clean` | Remove build artifacts |

For gesture checks after building: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe -PostedStroke`. The primary executable is `target\debug\mado-screen-poc.exe`.

## Controls

Permanent global controls:
- `Ctrl+Alt+D`: drawing/click-through toggle.
- `Ctrl+Alt+C`: clear, undoable.
- `Ctrl+Alt+Q`: quit.

Drawing-mode-only controls:
- `P` pen, `L` straight line, `R` outline rectangle, `A` arrow, `O` ellipse, `E` eraser.
- `Ctrl+Z` undo, `Ctrl+Y` redo.
- `[` thinner, `]` thicker, odd widths 1 through 31 physical pixels.
- `1` blue, `2` orange, `3` red, `4` green, `5` white.
- `C` opens the color picker at the current mouse position, clamped to the nearest monitor work area. Use presets or enter red/green/blue values from 0 to 255, then Apply. Cancel or Escape preserves the old color.

Defaults are orange and 5 pixels. Each annotation retains the color and width chosen when it began. Style changes affect new work only. Tool/color/width survive clear and mode changes. Draw by dragging the logical primary mouse button; release commits. A line/rectangle preview is cancelled on mode/tool/style changes, while partial pen ink is retained. Same-tool or same-style selection does not interrupt a gesture.

Undo/redo first settles any active gesture. Pen ink is committed, unfinished shapes are discarded, then history is applied. Clear is undoable; a new edit after undo discards the redo branch. Style/tool choices themselves are not history entries. All drawing-only registrations are released in click-through and on exit so underlying-app editing shortcuts work normally. Conflicts report the actual key and roll back partial registrations, preserving ink in click-through.

Arrows and ellipses use the same preview/commit/cancel behavior as rectangles. The eraser removes complete touched annotations, not filled interiors or parts of a stroke. A click erases the topmost hit; a moving sweep removes intersected annotations topmost-first, potentially multiple overlapping layers. One erase drag is one undoable action.

The top-right indicator shows drawing/click-through mode, selected tool, brush width and color swatch. It is click-through and nonactivating.

The modeless picker may intentionally take keyboard focus. While open, drawing-only shortcuts are suspended so its RGB controls and normal editing shortcuts work; permanent toggle/clear/quit remain available. Closing restores drawing shortcuts, or safely returns to click-through if another application reserved a key meanwhile. Clicking the canvas outside the popup dismisses it without drawing. Focus is restored best-effort only if the picker still owns foreground focus, never if the user switched to another application.

## Behavior and limits

The overlay does not activate itself. Drawing mode has background alpha 1/255 because fully transparent layered-window pixels otherwise pass input through. This may slightly darken the desktop. Click-through allows normal application interaction while annotations remain visible.

A capture guard and 32ms watchdog exist only during an active drag. Physical release with capture retained commits the last preview; capture loss cancels unfinished shapes. Timer latency is not guaranteed under load. Clear/toggle/quit release capture safely; no held-button resumption occurs without a new press.

The renderer fills/replays/uploads a full-screen CPU bitmap on changes. Snapshot history and many stroke points can use substantial memory and slow drawing. No idle redraw loop or GPU rewrite is introduced. Only the primary monitor is covered; restart after display changes. Multiple monitors, mixed-DPI layouts, sharing, elevated applications and other topmost windows remain unverified. Exclusive fullscreen coverage is not guaranteed; secure desktop and lock screen are excluded.

## Verification

Final PoC 4 passed build, 55 tests, formatting/Clippy and 79 desktop smoke checks with zero skipped on a 3440x1440 display. Indicator visual review passed. Initial idle CPU time was unchanged over 30 seconds. All three stages are pushed separately; evidence is in tasks/poc4-progress.md.

Unit tests do not launch the app. One native UI test creates only hidden windows to verify RGB controls, invalid input, Apply/Cancel, value-only events and cleanup without changing focus. Desktop smoke uses a copied executable and only app-local posted messages/owned test windows. It does not move the cursor or inject real keyboard/mouse input. Fresh hidden/visible comparisons report moving background samples as skipped/untested, never passed. `scripts/smoke.ps1 -SelfCheck` tests that classifier without opening the app.

Physical mouse/key delivery, normal typing/focus after toggles, outside-display release, held-button interruption and hardware pointer latency require manual observation. PoC 3 worked according to the user; that does not establish these behaviors for PoC 4. Stage evidence and remote pushes are recorded in tasks/poc4-progress.md. Missing features/deferred compatibility remain in docs/feature-roadmap.md; execution rules remain in docs/workflow-strategy.md.
