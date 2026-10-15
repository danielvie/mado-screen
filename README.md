# Mado Screen PoC 6

Native Rust/Win32 annotation overlay across the Windows virtual desktop. Draw on any connected monitor, including across adjacent screen boundaries, with shared tools, styles and undo/redo. Startup is empty and click-through. PoC 7 adds rounded antialiased strokes, one-second temporary fading, larger arrowheads and smoother ellipse outlines. Tools, history and the floating menu are preserved. Scope and checks are in tasks/poc7-spec.md and tasks/poc7-progress.md.

## Commands

Windows 10 version 1703 or newer or Windows 11; stable Rust Windows MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK. Task is optional.

| Command | Purpose |
| --- | --- |
| `task run` / `cargo run` | Start the unoptimized debug overlay |
| `task run-release` / `cargo run --release` | Start the optimized overlay for normal use |
| `task perf` | Measure optimized offscreen pen input and rendering |
| `task build` / `cargo build` | Build |
| `task install` | Build a release and copy `mado-screen-poc.exe` to `~/.local/bin` |
| `task test` / `cargo test` | Pure model/render/input/control and native resource checks |
| `task test-settings` | Settings completion, remapping, reload, arrow size and preference preservation |
| `task test-config` | Focused parser, native remapping rollback, reload, arrow-size and tray checks |
| `task test-menu` | Only new floating-menu feature tests |
| `task test-menu-followup` | Only reference-layout and compact brush-tip feature tests, optimized |
| `task test-menu-live` | Targeted release-app menu checks, without physical input injection |
| `task test-clear-help` | Only quick-clear, shortcut-binding and terminal-help checks |
| `task check` | Formatting and Clippy |
| `task smoke` | Copied-app desktop checks, including 30-second idle sample |
| `task clean` / `cargo clean` | Remove build artifacts |

For gesture checks after building: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe -PostedStroke`. The debug executable is `target\debug\mado-screen-poc.exe`; the optimized executable is `target\release\mado-screen-poc.exe`.

## Controls

Both debug and release executables embed the default click-through tray badge at 16, 20, 24, 32, 48, 64, 128 and 256 pixels. The icon is fixed; the live tray still changes with drawing mode and ink color. Run `task icon` after changing the default tray artwork to regenerate `assets/mado-screen.ico`.

The executable starts without opening a terminal, in both debug and release builds. Use the tray icon and floating menu to control Mado; drawing shortcuts are unchanged. Startup help and diagnostics still write to standard output/error when those streams are redirected, but are not visible during a normal desktop launch.

These are the default bindings. Every keyboard shortcut can be remapped in `[shortcuts]`.

Permanent global controls:
- `F4` or `Ctrl+Alt+D`: drawing/click-through toggle. Configure `shortcuts.activation_key` and `shortcuts.toggle` independently. Mado reserves each configured combination system-wide while running. If the activation binding is unavailable at startup, it warns and continues with the configured toggle binding and tray controls.
- `Ctrl+Alt+C`: clear, undoable.
- `Ctrl+Alt+Q`: quit.

Drawing-mode-only controls:
- `Escape` returns to click-through so underlying applications receive input again. It does not enable drawing from click-through.
- `P` selects permanent Pen. Repeating `P` keeps permanent ink selected.
- `S` selects Smoke pen, the temporary fading pen. Repeating `S` keeps temporary ink selected.
- `A` selects Arrow; press `A` while Arrow is selected to toggle head-first/tail-first drawing.
- `L` straight line, `R` outline rectangle, `O` ellipse, `E` eraser.
- `C` clears all permanent and temporary ink, like `Ctrl+Alt+C`. Permanent ink can be restored with undo. `C` is inactive in click-through.
- `H` hides or shows the floating menu without changing drawing mode, the selected tool or ink. The tray menu checkmark stays in sync. `H` is inactive in click-through; use the tray's Show floating menu command there.
- `X` swaps the active and reserve favorite colors.
- `T` swaps the current and previously selected tool, including temporary-pen and arrow-direction variants. Press it again to switch back. Shortcut and menu selections both count; selecting the same tool again does not replace the previous selection. Before a second tool is selected, `T` does nothing. It is inactive in click-through.
- `Ctrl+Z` undo, `Ctrl+Y` redo.
- `[` thinner, `]` thicker, odd widths 1 through 31 physical pixels.
- Mouse wheel up increases thickness; down decreases it, in the same 2-pixel steps. High-resolution wheel input accumulates to a full notch. The menu updates immediately. Scrolling is consumed only in drawing mode, without changing keyboard focus; click-through restores normal app scrolling.
- `1` blue, `2` orange, `3` red, `4` green, `5` white.

Defaults are orange and 5 pixels. Each annotation retains the color and width chosen when it began. Style changes affect new work only. Tool/color/width survive clear and mode changes. Draw by dragging the logical primary mouse button; release commits. A line/rectangle preview is cancelled on mode/tool/style changes, while partial pen ink is retained. Repeating A settles the active gesture and toggles arrow direction. P and S select exact pen variants; repeating the selected pen shortcut does not interrupt its gesture. Other same-tool or same-style choices do not interrupt it. Variant choices survive tool/mode changes and clear.

Undo/redo first settles any active gesture. Pen ink is committed, unfinished shapes are discarded, then history is applied. Clear is undoable; a new edit after undo discards the redo branch. Style/tool choices themselves are not history entries. All drawing-only registrations are released in click-through and on exit so underlying-app editing shortcuts work normally. Conflicts report the actual key and roll back partial registrations, preserving ink in click-through.

Arrows and ellipses use the same preview/commit/cancel behavior as rectangles. Arrows default to head-first: press at the head and drag toward the tail. Toggle A for tail-first: press at the tail and drag toward the head. The tray and floating-menu arrow icons point down-left for head-first and up-right for tail-first, showing which end of a down-left-to-up-right drag gets the arrowhead. The tray tooltip names the direction. Completed arrows never change direction. The larger arrowhead scales with the annotation's frozen stroke thickness; short arrows cap the head to fit the gesture. The eraser removes complete touched annotations, not filled interiors or parts of a stroke. A click erases the topmost hit; a moving sweep removes intersected annotations topmost-first, potentially multiple overlapping layers. One erase drag is one undoable action.

The floating menu uses a dark graphite theme. Its compact badge shows the selected tool and a separate far-right drag grip, with one overlapping brush-tip circle showing the current ink color and relative width. The circle is visible only when fully compact, is part of the hover area and is not a separate control. Hover opens the full menu directly: canvas-input toggle, stroke preview, width slider, two rows of eight fixed colors and clickable tool icons. Shortcut letters remain visible on the buttons; full names are in tooltips. Bottom buttons give visible hover feedback. The expanded header keeps the tool name on the left and the input toggle alongside it, without a separate compact-badge frame. Pen and Smoke pen clicks choose their exact variant, matching P and S; clicking an already-selected Arrow switches its drawing direction. The Drawing/Click-through button uses the same transition as Ctrl+Alt+D. Tool, color and width selection can prepare the next annotation in click-through without enabling canvas input.

The compact badge shows ON/OFF the same way: while drawing, the pencil is bright and the brush-tip circle is filled with the ink; in click-through, the pencil is dimmed and the circle is a hollow gray ring that still shows the width.

Favorite colors: the expanded menu shows the active color (brush color, front square) overlapping the reserve color (back square). Drag a palette color onto either square to set it; click the reserve square or the swap arrow, or press `X`, to swap. The compact badge shows the reserve as a small square below the grip dots, filled while drawing and hollow in click-through. Both favorites start orange / blue and are saved to `~/.mado/mado-screen/settings.toml` on change.

The tray icon is a miniature of the compact badge: graphite tile, selected tool and brush-tip dot. It updates when you select Pen, Temporary pen, Line, Arrow, Rectangle, Ellipse or Eraser through shortcuts or the floating menu. Its tooltip names the tool and input mode. While drawing, the dot shows the current ink color; in click-through it is a hollow gray ring and the tool is dimmed, but still follows tool selection. Left-click the icon to switch modes. Right-click (or Shift+F10 / the Menu key after Win+B) for Drawing mode, Show floating menu, Clear all ink, Reload config and Quit. Unchecking Show floating menu hides the panel (for presenting or recording) until you check it again; drawing and shortcuts keep working. Its startup visibility follows `menu.visible_on_start`, which defaults to true. The icon returns automatically if Explorer restarts. Its size follows the startup display scale.

The fixed colors cover warm hues, cool hues and white/gray/black. Slots are populated at startup and never reorder. Click a slot to select its exact color; clicking the selected color has no effect. The stroke preview is informational. Colors with no exact slot match do not highlight an unrelated preset. Existing numeric color shortcuts remain unchanged. The separate color picker and its `C` shortcut have been removed because colors are available in the main menu.

The menu grows left from its fixed right badge over a short 150 ms transition; leaving reverses it unless pinned. Click the pin icon in the expanded header to keep the menu open, then click it again to restore hover-driven collapse. The active pin is upright with a highlighted border; the inactive pin is tilted. Pinning lasts for the current session, survives input-mode changes and restores the expanded menu after hiding and showing it. It is off at startup and is not saved to settings. Windows disabled-animation preferences are respected. If the Windows client-area-animation setting is off, this transition is intentionally instant. Drag only the grip, not tool/color controls; the panel keeps its current size while dragging. Placement is session-only and constrained to the destination monitor's usable area. Near the bottom it opens upward. The panel does not activate itself or take keyboard focus.

Menu layouts: right-click the tray icon and pick Panel, Edge rail or Radial ring under Menu layout. The choice applies immediately and is saved as `menu.layout` (`panel`, `rail` or `radial`). All three keep the same compact badge.

- Edge rail: a 44 px column that opens down from the badge (up near the bottom of the screen), with input toggle, pin, tools, favorites, two palette columns and a vertical width slider. Hover, pin and grip dragging work like the panel; hotkey letters are in the tooltips.
- Radial ring: hovering the badge does nothing. Click the badge, or right-click the canvas while drawing, to open a ring around that point: tools on the inner ring, the 16 colors outside, and the brush at its real width in the center. Click a slice to pick it, or the center to swap with the reserve; the ring then closes. Moving the pointer off the ring or clicking its corners also closes it. Use the mouse wheel to change width (it also works over the ring in click-through), and F4 / Ctrl+Alt+D to toggle input. The ring has no pin. When the menu is hidden, right-click still opens the ring, and the menu hides again when the ring closes.

## Settings

Mado uses `~/.mado/mado-screen/settings.toml`, under your Windows user profile. It creates a commented file on first launch. If `config.ini` exists and `settings.toml` does not, Mado migrates its favorite colors and retains the old file as a backup. Once created, `settings.toml` takes precedence. At startup, Mado adds missing sections and keys from its default settings, including every shortcut binding and `arrow.head_scale`. Existing values, comments and unknown entries are preserved. A complete file is not rewritten. If the update cannot be saved, Mado still uses your loaded values and defaults for missing settings.

```toml
[brush]
active_color = "#F97316"
reserve_color = "#0085DC"
thickness_px = 5

[temporary_pen]
duration_ms = 1000

[arrow]
tail_first = false
head_scale = 1.0

[menu]
visible_on_start = true
layout = "panel"

[shortcuts]
activation_key = "F4"
toggle = "Ctrl+Alt+D"
clear = "Ctrl+Alt+C"
quit = "Ctrl+Alt+Q"
pen = "P"
smoke_pen = "S"
line = "L"
rectangle = "R"
arrow = "A"
ellipse = "O"
eraser = "E"
undo = "Ctrl+Z"
redo = "Ctrl+Y"
thinner = "["
thicker = "]"
blue = "1"
orange = "2"
red = "3"
green = "4"
white = "5"
swap_colors = "X"
quick_clear = "C"
deactivate = "Escape"
previous_tool = "T"
menu = "H"
```

Edit this file, save it, then choose **Reload config** from the tray menu, or restart Mado. Colors must be quoted `#RRGGBB` strings. Thickness must be an odd integer from 1 to 31 physical pixels. Duration must be an integer from 1 to 60000 milliseconds; `2500` gives a 2.5-second fade. `tail_first` and `visible_on_start` accept `true` or `false`. Arrow `head_scale` accepts a finite number from 0.1 to 4.0. `1.0` preserves today's size, `0.5` halves it and `2.0` doubles it. Heads still scale with brush thickness and are capped for short arrows. Completed arrows keep their original geometry.

Shortcut values are case-insensitive strings with one letter, digit, F1 through F11, a named key or punctuation key. Optional modifiers are `Ctrl`, `Alt`, `Shift` and `Win`, joined with `+`. Examples: `pen = "Shift+P"`, `smoke_pen = "F9"`, `toggle = "Ctrl+Alt+Space"`. Named keys are Escape, Space, Tab, Enter, Backspace, Delete, Insert, Home, End, PageUp, PageDown, Left, Right, Up and Down. Supported punctuation is `[`, `]`, `;`, `/`, backtick, backslash, apostrophe, comma, period, minus and equals. Use `Shift+=` for the plus key. Punctuation uses Windows virtual keys, so its physical position follows the keyboard layout. F12 is reserved by Windows. Repeated modifiers, unsupported keys and duplicate combinations are rejected. Bare and modified versions of the same key are distinct. Remapping does not change whether a command is global or drawing-only; the mouse wheel is unchanged.

Reload validates the complete file and checks every shortcut before applying changes. Invalid values, duplicate bindings or keys owned by another app reject the reload and preserve the active configuration. A tray notification reports success or failure. If settings apply but a display refresh fails, it reports that warning separately and attempts the remaining display updates. Reload does not rewrite the file. Missing settings use defaults; unknown entries are preserved. A successful reload settles active drawing, applies brush colors/width, arrow direction/size, fade duration, menu visibility and shortcut bindings, and preserves drawing mode, selected tool, menu pin/position, completed ink and undo/redo history. Existing temporary segments keep their birth times and use the new fade duration, so a shorter duration can expire them immediately.

At startup, invalid existing values or incompatible sections use defaults without overwriting those values. Duplicate combinations restore the default shortcut set. Invalid TOML syntax uses all defaults and leaves the file untouched. Diagnostics also write to standard error, normally hidden during desktop launches.

`layout` accepts `"panel"`, `"rail"` or `"radial"`; it is also saved when chosen from the tray, and Reload applies it. Favorite colors, brush thickness and arrow direction save automatically. Thickness changes from the mouse wheel, slider and `[` / `]` are remembered. Arrow-direction changes from `A`, the menu and `T` are remembered even when you switch to another tool. The next launch restores the last saved thickness and arrow direction. Fade duration, menu visibility, arrowhead scale and shortcut mappings are file preferences; using tools or the menu does not overwrite them. Reload applies their current file values. Brush saves reread the TOML document and preserve other settings and comments using [toml_edit](https://docs.rs/toml_edit/0.22.27/toml_edit/). Writes use a temporary file followed by same-directory replacement rather than truncating the existing settings. There is no cross-process edit lock; simultaneous file edits are not supported.

## Temporary ink

Each temporary dot/input segment fades linearly over the configured duration from when it was drawn, then disappears. The default is one second. Expiry continues during a long drag, after release and in click-through. It never restarts the lifetime on release. Clear removes temporary ink immediately. Changing tool/style or undo/redo ends its drag without converting it into permanent ink.

Temporary ink is drawn above permanent annotations, is not undoable and does not discard permanent redo history. Undo/redo affects permanent history only. The eraser edits permanent annotations; temporary ink expires or is cleared. Each received input segment has one age, so sparse movement can show larger steps along the fading tail. Parts of one gesture do not build up opacity at overlapping joins. Separate gestures layer normally. A timer redraws fading ink only while temporary pixels remain; it stops when empty. Permanent strokes stay opaque inside their antialiased edges. Both pens use mild streaming quadratic Bézier smoothing. A live tail reaches the newest pointer point without a timed input buffer. Completed curve geometry is shared with eraser hit-testing; shape tools remain unchanged. Smoothing slightly changes freehand paths.

## Behavior and limits

The overlay does not activate itself. Drawing mode has background alpha 1/255 because fully transparent layered-window pixels otherwise pass input through. This may slightly darken the desktop. Click-through allows normal application interaction while annotations remain visible.

A capture guard and 32ms watchdog exist only during an active drag. Physical release with capture retained commits the last preview; capture loss cancels unfinished shapes. Timer latency is not guaranteed under load. Clear/toggle/quit release capture safely; no held-button resumption occurs without a new press.

One overlay and CPU bitmap span the virtual desktop, including gaps between monitors. Negative screen origins are supported; annotation coordinates are local physical pixels, so brush widths do not change at a mixed-DPI boundary. Toggle, clear, erasing and history apply across all screens. Mirrored displays show the same canvas. Restart after connecting/disconnecting monitors or changing layout/scaling.

The renderer fills/replays/uploads the whole virtual bitmap on changes. Snapshot history and many stroke points can use substantial memory and slow drawing; extra screens and gaps increase bitmap cost. No idle redraw loop or GPU rewrite is introduced. Layouts larger than 32767 pixels on either axis are rejected to prevent signed mouse-coordinate wrapping, and surface allocation remains bounded. Physical mixed-DPI input and long-session responsiveness need user testing. Sharing/recording is deferred. Elevated applications and other topmost windows remain unverified. Exclusive fullscreen coverage is not guaranteed; secure desktop and lock screen are excluded.

## Verification

Remappable shortcuts, arrowhead scaling and config reload passed 190 release-mode tests, with the icon-generation test intentionally ignored, plus debug/release builds, formatting, Clippy and the smoke self-check. Checks cover every configurable binding, duplicate/type/range rejection, settings completion and file preservation, native registration swaps and rollback, native message routing, queued-hotkey cleanup with quit preservation, click-through release, active-gesture/history preservation, existing temporary ages, both arrow directions and short-arrow caps, updated help/tooltips/badges and tray command decoding. Offscreen remapped-menu previews were reviewed. Physical key delivery, tray selection and notification display, assistive technology and full desktop smoke remain untested. The full debug test run exceeded its 120-second limit during a native menu check; debug tests are not claimed passed.

Menu pinning and dedicated P/S pen shortcuts passed all 171 release-mode tests, with the icon-generation test intentionally ignored, plus debug/release builds, formatting, Clippy and the smoke self-check. Pin tests cover expanded-only hit targets at 100/125/200 percent scale in both opening directions, distinct pin artwork, cancelled clicks, pointer-leave and delayed-collapse suppression, unchanged focus/drawing commands, held sliders and hide/show restoration. Shortcut checks cover drawing-only P/S registration, repeat suppression, exact variant selection, uninterrupted same-variant gestures, preserved ink/temporary ages and previous-tool swaps. Offscreen menu previews were reviewed. Physical clicks, key delivery, assistive technology and desktop smoke remain manual or unrun checks.

Load-time settings completion passed all 167 release-mode tests, with the icon-generation test intentionally ignored, plus the release build, formatting and Clippy. Checks remove every known section and key in turn, confirm default insertion and repeat-load stability, preserve custom values/comments/unknown entries and invalid structures, and force a write failure to verify the original file and loaded preferences survive.

Configurable activation-key checks passed all 164 release-mode tests, with the icon-generation test intentionally ignored, plus the release build, formatting and Clippy. Tests cover F1 through F11, invalid-value fallback, unchanged drawing bindings, preservation through brush saves, updated startup help and a hidden-window menu label/tooltip check. Physical key delivery remains a manual check.

Executable-icon worktree promotion fast-forwarded local `main` to `636de68`, including the icon resource and the minimal lint fixes required by the promotion checks. Restored local settings/tool/shortcut changes remain uncommitted. Icon tests were adapted to the current tool-aware renderer. Brush thickness and arrow direction now save with favorite colors and restore on startup; other settings and comments remain intact. The previous four Clippy blockers were fixed without disabling checks. Combined validation passed debug/release builds, formatting, Clippy and 160 tests; the icon-generation test remains intentionally ignored. Preference tests cover thickness changes through the wheel/slider/keys and arrow changes through `A`, menu selection and `T`, followed by reload.

TOML settings passed all 157 release-mode tests, debug/release builds and formatting. New checks cover defaults, range/type validation, one-time legacy color migration, comment/manual-setting preservation, refusal to overwrite malformed TOML or incompatible brush structures, custom fade/expiry/pause boundaries and hidden-menu startup without a visible flash. Clippy still reports the four existing warnings. No live drawing acceptance check was run.

The drawing-only `H` menu toggle passed all 154 release-mode tests, the release build and formatting. Binding checks cover the bare key, repeat suppression and release in click-through; the existing native menu check covers hiding, collapsing and restoring the panel. Keyboard and tray commands share the visibility transaction and update the same checkmark. Physical `H` delivery remains a manual check.

Arrow-direction icons and the `T` previous-tool swap passed all 153 release-mode tests, the release build and formatting. New checks cover repeated swaps across shortcut/menu selections and tool variants, unchanged completed ink/history, drawing-only `T` registration, distinct tray arrows at 16/20/24/32 pixels, and distinct compact/expanded menu arrows at 100/125/200 percent scale. Offscreen compact previews were reviewed. Live keyboard/tray behavior still needs manual testing. Clippy continues to report the same four existing warnings.

Tool-dependent tray icons passed all 148 release-mode tests, the release build and formatting. New checks confirm seven distinct tool images at 16/20/24/32 pixels in both input modes, unchanged icon shape and ink dot, and matching tool/mode tooltip text. Live taskbar appearance still needs manual testing.

Mouse-wheel thickness control passed all 146 release-mode tests, debug/release builds and formatting. New checks cover signed/partial wheel deltas, drawing-only changes, width limits, preserved ink/history/temporary ages, hidden-window message routing and hook cleanup without focus changes. Physical wheel delivery still needs manual testing. The wheel hook runs on the main message-loop thread; very long rendering stalls can exceed Windows' hook timeout and disable it until drawing mode is re-entered. Clippy still reports the four existing warnings below.

Removing the standalone picker passed all 133 release-mode tests, the release build, formatting, the smoke self-check and PowerShell parsing. Hidden menu tests cover repeated selected-color clicks, inert preview clicks and unchanged window/focus state. The main-menu preview was reviewed offscreen. Clippy reports four pre-existing warnings under the current toolchain, also reproduced on the unchanged `db791fd` baseline. No visible app launch or physical-input acceptance check was run.

The floating-menu implementation and manual checklist are in tasks/panel-menu-progress.md; the reference-layout and compact brush-tip follow-up is recorded in tasks/panel-menu-followup.md. That menu increment ran only new panel_menu_ feature tests and a release build, not the full suite or desktop smoke. Physical clicks, focus, outside-window release and mixed-DPI placement remain manual checks. Earlier compact-panel checks are recorded in tasks/layout-progress.md.

The Bézier follow-up measures optimized offscreen rendering and model construction, not physical input latency or Windows bitmap upload. Reproduction and before/after results are in tasks/bezier-performance.md. Use release mode for visual and responsiveness checks.

PoC 6 passed build, 89 tests, formatting/Clippy, smoke self-check and 232 posted desktop checks with zero skipped on three monitors. Results are in tasks/poc6-progress.md. Pure tests inject elapsed time for exact expiry and history checks; physical smoothness, timing and shortcut delivery require user observation. Timer dispatch may be late under load, and replaying a full virtual bitmap while ink expires can affect responsiveness.

Historical PoC 5 build, 68 tests, formatting/Clippy and smoke self-check passed. The user confirms drawing on secondary monitors and authorizes integration. Desktop startup confirmed virtual window bounds, but pixel capture is blocked by the desktop session's access restrictions; full posted drawing smoke remains blocked, not passed. Cross-boundary gestures and detailed mixed-DPI acceptance remain untested. Results and rerun commands are in tasks/poc5-progress.md.

Historical PoC 4 with the approved disk picker passed build, 59 tests, formatting/Clippy and 79 desktop smoke checks with zero skipped on a 3440x1440 display. Indicator and offscreen native disk-pane visual review passed. The actual picker preview is docs/design/disk-color-picker-implemented.png. Initial idle CPU time was unchanged over 30 seconds. All three stages are pushed separately; evidence is in tasks/poc4-progress.md.

Unit tests do not launch the app. Native UI checks create only hidden windows to verify menu controls, committed selections, pointer-free events, nonactivation and cleanup. Desktop smoke uses a copied executable and only app-local posted messages/owned test windows. It does not move the cursor or inject real keyboard/mouse input. Fresh hidden/visible comparisons report moving background samples as skipped/untested, never passed. `scripts/smoke.ps1 -SelfCheck` tests that classifier without opening the app.

Physical mouse/key delivery on each monitor and across boundaries, assistive-technology behavior, normal typing/focus after toggles, outside-display release, held-button interruption and hardware pointer latency require manual observation. Hidden native checks do not establish real desktop interaction. Historical picker validation remains recorded in tasks/poc4-progress.md; the picker is no longer part of the app. Missing features/deferred compatibility remain in docs/feature-roadmap.md; execution rules remain in docs/workflow-strategy.md.
