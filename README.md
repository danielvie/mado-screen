# Mado Screen PoC 6

Native Rust/Win32 annotation overlay across the Windows virtual desktop. Draw on any connected monitor, including across adjacent screen boundaries, with shared tools, styles and undo/redo. Startup is empty and click-through. PoC 7 adds rounded antialiased strokes, one-second temporary fading, larger arrowheads and smoother ellipse outlines. Tools, history, indicator and disk picker are preserved. Scope and checks are in tasks/poc7-spec.md and tasks/poc7-progress.md.

## Commands

Windows 10 version 1703 or newer or Windows 11; stable Rust Windows MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK. Task is optional.

| Command | Purpose |
| --- | --- |
| `task run` / `cargo run` | Start the unoptimized debug overlay |
| `task run-release` / `cargo run --release` | Start the optimized overlay for normal use |
| `task perf` | Measure optimized offscreen pen input and rendering |
| `task build` / `cargo build` | Build |
| `task test` / `cargo test` | Pure model/render/input/control and native resource checks |
| `task check` | Formatting and Clippy |
| `task smoke` | Copied-app desktop checks, including 30-second idle sample |
| `task clean` / `cargo clean` | Remove build artifacts |

For gesture checks after building: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe -PostedStroke`. The debug executable is `target\debug\mado-screen-poc.exe`; the optimized executable is `target\release\mado-screen-poc.exe`.

## Controls

Permanent global controls:
- `Ctrl+Alt+D`: drawing/click-through toggle.
- `Ctrl+Alt+C`: clear, undoable.
- `Ctrl+Alt+Q`: quit.

Drawing-mode-only controls:
- `P` selects Pen; press `P` while Pen is selected to toggle permanent/temporary ink.
- `A` selects Arrow; press `A` while Arrow is selected to toggle head-first/tail-first drawing.
- `L` straight line, `R` outline rectangle, `O` ellipse, `E` eraser.
- `Ctrl+Z` undo, `Ctrl+Y` redo.
- `[` thinner, `]` thicker, odd widths 1 through 31 physical pixels.
- `1` blue, `2` orange, `3` red, `4` green, `5` white.
- `C` opens the disk color picker at the mouse, clamped to the nearest monitor work area. Drag around the disk for hue and toward its center for less saturation. Adjust brightness below it, use a preset, or enter `#RRGGBB`. Apply or Enter commits; Cancel, Escape, close or an outside click preserves the old color.

Defaults are orange and 5 pixels. Each annotation retains the color and width chosen when it began. Style changes affect new work only. Tool/color/width survive clear and mode changes. Draw by dragging the logical primary mouse button; release commits. A line/rectangle preview is cancelled on mode/tool/style changes, while partial pen ink is retained. Repeating P or A settles the active gesture and toggles its variant; other same-tool or same-style choices do not interrupt it. Variant choices survive tool/mode changes and clear.

Undo/redo first settles any active gesture. Pen ink is committed, unfinished shapes are discarded, then history is applied. Clear is undoable; a new edit after undo discards the redo branch. Style/tool choices themselves are not history entries. All drawing-only registrations are released in click-through and on exit so underlying-app editing shortcuts work normally. Conflicts report the actual key and roll back partial registrations, preserving ink in click-through.

Arrows and ellipses use the same preview/commit/cancel behavior as rectangles. Arrows default to head-first: press at the head and drag toward the tail. Toggle A for tail-first: press at the tail and drag toward the head. Completed arrows never change direction. The larger arrowhead scales with the annotation's frozen stroke thickness; short arrows cap the head to fit the gesture. The eraser removes complete touched annotations, not filled interiors or parts of a stroke. A click erases the topmost hit; a moving sweep removes intersected annotations topmost-first, potentially multiple overlapping layers. One erase drag is one undoable action.

The primary monitor's top-right panel shows only the selected tool's icon when compact. Hover expands it to the left with drawing/click-through mode, tool variant, brush width, current-color swatch and up to five distinct recent colors in fixed slots. The width fits the status text and swatches. Click a different recent color to select it immediately; click the selected color or current-color swatch to open the full picker. Leaving the panel collapses it. Selecting an existing recent color never reorders the swatches. New colors fill empty slots, then replace the oldest added entry in place without moving the others. Recent colors are session-only and include the initial brush color and committed selections, not picker previews. The panel receives mouse input only in its occupied rounded area and does not activate itself or capture keyboard shortcuts.

The color picker uses a light rounded pane with a continuous hue/saturation disk, brightness slider, circular swatches, hex input and Cancel/Apply. The modeless picker may intentionally take keyboard focus. While open, drawing-only shortcuts are suspended so its hex input and normal editing shortcuts work; permanent toggle/clear/quit remain available. Closing restores drawing shortcuts, or safely returns to click-through if another application reserved a key meanwhile. Clicking the canvas outside the popup dismisses it without drawing. Focus is restored best-effort only if the picker still owns foreground focus, never if the user switched to another application.

## Temporary ink

Each temporary dot/input segment fades linearly over one second from when it was drawn, then disappears. Expiry continues during a long drag, after release, while the picker is open and in click-through. It never restarts the lifetime on release. Clear removes temporary ink immediately. Changing tool/style or undo/redo ends its drag without converting it into permanent ink.

Temporary ink is drawn above permanent annotations, is not undoable and does not discard permanent redo history. Undo/redo affects permanent history only. The eraser edits permanent annotations; temporary ink expires or is cleared. Each received input segment has one age, so sparse movement can show larger steps along the fading tail. Parts of one gesture do not build up opacity at overlapping joins. Separate gestures layer normally. A timer redraws fading ink only while temporary pixels remain; it stops when empty. Permanent strokes stay opaque inside their antialiased edges. Both pens use mild streaming quadratic Bézier smoothing. A live tail reaches the newest pointer point without a timed input buffer. Completed curve geometry is shared with eraser hit-testing; shape tools remain unchanged. Smoothing slightly changes freehand paths.

## Behavior and limits

The overlay does not activate itself. Drawing mode has background alpha 1/255 because fully transparent layered-window pixels otherwise pass input through. This may slightly darken the desktop. Click-through allows normal application interaction while annotations remain visible.

A capture guard and 32ms watchdog exist only during an active drag. Physical release with capture retained commits the last preview; capture loss cancels unfinished shapes. Timer latency is not guaranteed under load. Clear/toggle/quit release capture safely; no held-button resumption occurs without a new press.

One overlay and CPU bitmap span the virtual desktop, including gaps between monitors. Negative screen origins are supported; annotation coordinates are local physical pixels, so brush widths do not change at a mixed-DPI boundary. Toggle, clear, erasing and history apply across all screens. Mirrored displays show the same canvas. Restart after connecting/disconnecting monitors or changing layout/scaling.

The renderer fills/replays/uploads the whole virtual bitmap on changes. Snapshot history and many stroke points can use substantial memory and slow drawing; extra screens and gaps increase bitmap cost. No idle redraw loop or GPU rewrite is introduced. Layouts larger than 32767 pixels on either axis are rejected to prevent signed mouse-coordinate wrapping, and surface allocation remains bounded. Physical mixed-DPI input and long-session responsiveness need user testing. Sharing/recording is deferred. Elevated applications and other topmost windows remain unverified. Exclusive fullscreen coverage is not guaranteed; secure desktop and lock screen are excluded.

## Verification

The compact panel/color-picker layout contract is in tasks/layout-spec.md; focused checks and manual limits are recorded in tasks/layout-progress.md. Hidden native checks and offscreen previews do not establish physical hover, focus or mixed-DPI interaction.

The Bézier follow-up measures optimized offscreen rendering and model construction, not physical input latency or Windows bitmap upload. Reproduction and before/after results are in tasks/bezier-performance.md. Use release mode for visual and responsiveness checks.

PoC 6 passed build, 89 tests, formatting/Clippy, smoke self-check and 232 posted desktop checks with zero skipped on three monitors. Results are in tasks/poc6-progress.md. Pure tests inject elapsed time for exact expiry and history checks; physical smoothness, timing and shortcut delivery require user observation. Timer dispatch may be late under load, and replaying a full virtual bitmap while ink expires can affect responsiveness.

Historical PoC 5 build, 68 tests, formatting/Clippy and smoke self-check passed. The user confirms drawing on secondary monitors and authorizes integration. Desktop startup confirmed virtual window bounds, but pixel capture is blocked by the desktop session's access restrictions; full posted drawing smoke remains blocked, not passed. Cross-boundary gestures and detailed mixed-DPI acceptance remain untested. Results and rerun commands are in tasks/poc5-progress.md.

Historical PoC 4 with the approved disk picker passed build, 59 tests, formatting/Clippy and 79 desktop smoke checks with zero skipped on a 3440x1440 display. Indicator and offscreen native disk-pane visual review passed. The actual picker preview is docs/design/disk-color-picker-implemented.png. Initial idle CPU time was unchanged over 30 seconds. All three stages are pushed separately; evidence is in tasks/poc4-progress.md.

Unit tests do not launch the app. Native UI checks create only hidden windows to verify picker controls, invalid input, Apply/Cancel, value-only events and cleanup without changing focus. Desktop smoke uses a copied executable and only app-local posted messages/owned test windows. It does not move the cursor or inject real keyboard/mouse input. Fresh hidden/visible comparisons report moving background samples as skipped/untested, never passed. `scripts/smoke.ps1 -SelfCheck` tests that classifier without opening the app.

Physical mouse/key delivery on each monitor and across boundaries, visible picker drag/Tab/focus/deactivation, assistive-technology behavior, normal typing/focus after toggles, outside-display release, held-button interruption and hardware pointer latency require manual observation. Hidden native checks exercise disk/slider keyboard handlers, candidate/hex/presets, Apply/Cancel/capture loss, resource lifetime and 150% geometry; they do not establish real desktop interaction. PoC 3 worked according to the user; that does not establish these behaviors for PoC 4. Stage evidence and remote pushes are recorded in tasks/poc4-progress.md. Missing features/deferred compatibility remain in docs/feature-roadmap.md; execution rules remain in docs/workflow-strategy.md.
