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
| `task test-menu` | Only new floating-menu feature tests |
| `task test-menu-followup` | Only reference-layout and compact brush-tip feature tests, optimized |
| `task test-menu-live` | Targeted release-app menu checks, without physical input injection |
| `task test-clear-help` | Only quick-clear, shortcut-binding and terminal-help checks |
| `task check` | Formatting and Clippy |
| `task smoke` | Copied-app desktop checks, including 30-second idle sample |
| `task clean` / `cargo clean` | Remove build artifacts |

For gesture checks after building: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe -PostedStroke`. The debug executable is `target\debug\mado-screen-poc.exe`; the optimized executable is `target\release\mado-screen-poc.exe`.

## Controls

Startup prints a grouped guide to shortcuts and color-panel controls. Supported terminals show bold titles and Pi-theme colors for shortcuts and command labels; redirected or unsupported output stays plain text. Routine mode, tool, clear and shutdown events are silent; actionable errors still appear in the terminal.

Permanent global controls:
- `Ctrl+Alt+D`: drawing/click-through toggle.
- `Ctrl+Alt+C`: clear, undoable.
- `Ctrl+Alt+Q`: quit.

Drawing-mode-only controls:
- `P` selects Pen; press `P` while Pen is selected to toggle permanent/temporary ink.
- `A` selects Arrow; press `A` while Arrow is selected to toggle head-first/tail-first drawing.
- `L` straight line, `R` outline rectangle, `O` ellipse, `E` eraser.
- `X` clears all permanent and temporary ink, like `Ctrl+Alt+C`. Permanent ink can be restored with undo. `X` is inactive in click-through and while the picker is open.
- `Ctrl+Z` undo, `Ctrl+Y` redo.
- `[` thinner, `]` thicker, odd widths 1 through 31 physical pixels.
- `1` blue, `2` orange, `3` red, `4` green, `5` white.
- `C` opens the disk color picker at the mouse, clamped to the nearest monitor work area. Drag around the disk for hue and toward its center for less saturation. Adjust brightness below it, use a preset, or enter `#RRGGBB`. Apply or Enter commits; Cancel, Escape, close or an outside click preserves the old color.

Defaults are orange and 5 pixels. Each annotation retains the color and width chosen when it began. Style changes affect new work only. Tool/color/width survive clear and mode changes. Draw by dragging the logical primary mouse button; release commits. A line/rectangle preview is cancelled on mode/tool/style changes, while partial pen ink is retained. Repeating P or A settles the active gesture and toggles its variant; other same-tool or same-style choices do not interrupt it. Variant choices survive tool/mode changes and clear.

Undo/redo first settles any active gesture. Pen ink is committed, unfinished shapes are discarded, then history is applied. Clear is undoable; a new edit after undo discards the redo branch. Style/tool choices themselves are not history entries. All drawing-only registrations are released in click-through and on exit so underlying-app editing shortcuts work normally. Conflicts report the actual key and roll back partial registrations, preserving ink in click-through.

Arrows and ellipses use the same preview/commit/cancel behavior as rectangles. Arrows default to head-first: press at the head and drag toward the tail. Toggle A for tail-first: press at the tail and drag toward the head. Completed arrows never change direction. The larger arrowhead scales with the annotation's frozen stroke thickness; short arrows cap the head to fit the gesture. The eraser removes complete touched annotations, not filled interiors or parts of a stroke. A click erases the topmost hit; a moving sweep removes intersected annotations topmost-first, potentially multiple overlapping layers. One erase drag is one undoable action.

The floating menu uses a dark graphite theme. Its compact badge shows the selected tool and a separate far-right drag grip, with one overlapping brush-tip circle showing the current ink color and relative width. The circle is visible only when fully compact, is part of the hover area and is not a separate control. Hover opens the full menu directly: canvas-input toggle, stroke preview, width slider, two rows of eight fixed colors and clickable tool icons. Shortcut letters remain visible on the buttons; full names are in tooltips. Bottom buttons give visible hover feedback. The expanded header keeps the tool name on the left and the input toggle alongside it, without a separate compact-badge frame. Pen and Temporary pen clicks choose their exact variant; clicking an already-selected Arrow switches its drawing direction. The Drawing/Click-through button uses the same transition as Ctrl+Alt+D. Tool, color and width selection can prepare the next annotation in click-through without enabling canvas input.

The fixed colors cover warm hues, cool hues and white/gray/black. Slots are populated at startup and never reorder or change after custom picker selections. Click a different slot to select its exact color; click a selected matching slot or the stroke preview to open the full picker. Custom colors with no exact slot match do not highlight an unrelated preset. Existing numeric color shortcuts remain unchanged.

The menu grows left from its fixed right badge over a short 150 ms transition; leaving reverses it. Windows disabled-animation preferences are respected. If the Windows client-area-animation setting is off, this transition is intentionally instant. Drag only the grip, not tool/color controls; the panel keeps its current size while dragging. Placement is session-only and constrained to the destination monitor's usable area. Near the bottom it opens upward. The panel does not activate itself or take keyboard focus. The modeless picker still intentionally takes focus for editing.

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

The floating-menu implementation and manual checklist are in tasks/panel-menu-progress.md; the reference-layout and compact brush-tip follow-up is recorded in tasks/panel-menu-followup.md. This increment runs only new panel_menu_ feature tests and a release build, not the full suite or desktop smoke. Physical clicks, focus, outside-window release and mixed-DPI placement remain manual checks. Earlier compact-panel checks are recorded in tasks/layout-progress.md.

The Bézier follow-up measures optimized offscreen rendering and model construction, not physical input latency or Windows bitmap upload. Reproduction and before/after results are in tasks/bezier-performance.md. Use release mode for visual and responsiveness checks.

PoC 6 passed build, 89 tests, formatting/Clippy, smoke self-check and 232 posted desktop checks with zero skipped on three monitors. Results are in tasks/poc6-progress.md. Pure tests inject elapsed time for exact expiry and history checks; physical smoothness, timing and shortcut delivery require user observation. Timer dispatch may be late under load, and replaying a full virtual bitmap while ink expires can affect responsiveness.

Historical PoC 5 build, 68 tests, formatting/Clippy and smoke self-check passed. The user confirms drawing on secondary monitors and authorizes integration. Desktop startup confirmed virtual window bounds, but pixel capture is blocked by the desktop session's access restrictions; full posted drawing smoke remains blocked, not passed. Cross-boundary gestures and detailed mixed-DPI acceptance remain untested. Results and rerun commands are in tasks/poc5-progress.md.

Historical PoC 4 with the approved disk picker passed build, 59 tests, formatting/Clippy and 79 desktop smoke checks with zero skipped on a 3440x1440 display. Indicator and offscreen native disk-pane visual review passed. The actual picker preview is docs/design/disk-color-picker-implemented.png. Initial idle CPU time was unchanged over 30 seconds. All three stages are pushed separately; evidence is in tasks/poc4-progress.md.

Unit tests do not launch the app. Native UI checks create only hidden windows to verify picker controls, invalid input, Apply/Cancel, value-only events and cleanup without changing focus. Desktop smoke uses a copied executable and only app-local posted messages/owned test windows. It does not move the cursor or inject real keyboard/mouse input. Fresh hidden/visible comparisons report moving background samples as skipped/untested, never passed. `scripts/smoke.ps1 -SelfCheck` tests that classifier without opening the app.

Physical mouse/key delivery on each monitor and across boundaries, visible picker drag/Tab/focus/deactivation, assistive-technology behavior, normal typing/focus after toggles, outside-display release, held-button interruption and hardware pointer latency require manual observation. Hidden native checks exercise disk/slider keyboard handlers, candidate/hex/presets, Apply/Cancel/capture loss, resource lifetime and 150% geometry; they do not establish real desktop interaction. PoC 3 worked according to the user; that does not establish these behaviors for PoC 4. Stage evidence and remote pushes are recorded in tasks/poc4-progress.md. Missing features/deferred compatibility remain in docs/feature-roadmap.md; execution rules remain in docs/workflow-strategy.md.
