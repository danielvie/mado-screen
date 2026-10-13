# Panel edge rendering and direct color access

## Requested follow-up

The user reports broken/jagged corners in the compact panel, asks to improve resolution, remove the extra click needed to show recent colors, and open the picker when clicking the same selected color.

Keep the compact icon-only 44 logical pixel size and primary top-right anchor. Improve edge/icon antialiasing. The user additionally requested removing the expanded panel's empty space; measure the status text in the actual font/DPI and size the expanded panel to text plus current swatch/padding, with enough minimum width for five recent swatches. Do not retain the old fixed 440px width. Short Pen labels should use a smaller panel, while longer Arrow variant labels must fit without clipping. Expanded hover immediately displays mode/tool/thickness/current color and the recent-color row. No intermediate toggle state. Clicking a different recent color selects it. Clicking the current swatch or the matching selected recent color requests the full picker. Leave still collapses. Five distinct session-only recent colors and existing picker Apply/Cancel remain.

Antialias the whole panel including the outer rounded alpha edge. The previous binary SetWindowRgn clipping and low-resolution GDI pen painting can produce jagged corners. Use a small antialiased premultiplied bitmap for the layered panel; preserve transparent corners and nonactivation. Supersampled existing GDI geometry is sufficient, no dependency or GPU/backend rewrite. Do not change canvas rendering or color-picker disk geometry. No idle polling loop.

## Interface and ownership

Sibling integration/panel-polish and worker/panel-polish worktrees from 578dfd9. UI worker owns src/ui.rs only. Parent owns main.rs, docs, affected smoke assertions and serial integration.

Add UiEvent::OpenPicker, no payload, encoded as kind 3/value 0 in the existing UI_EVENT protocol. Parent handles it through the existing picker opening workflow, settling active pen/shape/trail, dropping drag and drawing shortcut guards before opening. Open requests do not change color/history. Requests work from the visible panel in drawing and click-through; C remains drawing-only. If a picker is already open, do nothing, do not recreate or reset its candidate. Failed opens restore drawing shortcuts through existing state checks. Never activate for hover or direct quick selection. Full picker activation remains intentional existing behavior.

Remove only the now-unused recents_open toggle state. Keep captured press/release hit validation, stale-coordinate suppression, native leave handling, committed color recording and clear distinctions between picker requests and color selection. Update UI hidden/native tests and emit refreshed all-tool icon/expanded previews at 100% and 150%. Test transparent/partial-alpha corner pixels, premultiplication, continuous rounded perimeter and icon edge coverage. Validate a hidden layered-window upload, no visible app or focus/cursor changes.

## Validation and boundaries

Focused tests only: ui::tests, main picker-request/selection routing and directly affected capture/history regressions; release build, cargo fmt --check, cargo clippy --all-targets -- -D warnings, git diff --check. No full suite or desktop smoke. Read actual delivery and offscreen images before promotion. Record any failures honestly.

Main's local README.md and Taskfile.yml edits and untracked Excalidraw are preserved. No remote push, installed-app replacement, real input injection, cursor motion or app launch. Physical corner appearance and hover ergonomics require later user acceptance. Win32 alpha hit-testing and UpdateLayeredWindow protocol should be verified against official docs.

## Results

UI worker delivery dd2fcc4 was rebased serially to d11ab60 onto parent main/docs delivery 1d19b4e. Actual code, all new alpha/measurement paths and exported previews were reviewed. Main handles OpenPicker without mutating color; it shares existing picker gesture/shortcut settlement and ignores requests while the picker remains open. The drawing-only C shortcut retains its previous restriction.

Panel rendering uses bounded 4x GDI supersampling, covered-sample premultiplied BGRA averaging and UpdateLayeredWindow. No global-alpha call or binary window region remains on the panel. The full picker is unchanged. Expanded width uses selected-font measurement; reviewed Pen preview is 224px and the longest tested Click-through/Arrow label 292px at100%, previously440px. Compact is still44 logical pixels. Alpha-zero corners pass through; partial alpha pixels remain occupied. Previews at100/150% over a dark background are retained under target/panel-polish-previews.

Assembled integration passed:
- cargo test ui::tests -- --test-threads=1:5 tests.
- cargo test panel_picker_request:1 test, preserving brush color/history and temporary births while settling input.
- cargo test recent_color_:2 directly affected regression tests.
- cargo build --release.
- cargo fmt --check.
- cargo clippy --all-targets -- -D warnings.
- git diff --check.

UI checks cover the hidden ULW upload, transparent and partial-alpha corners, contiguous perimeter coverage, premultiplication, mixed icon-edge pixels, measured short/long labels at100/150%, all tool icons, immediate recents, matching-color OpenPicker, different-color Color, stale clicks and existing picker candidate/Cancel/Apply behavior.

Worker reruns observed a differing global foreground HWND at the end of one hidden-panel check, with local focus/active/capture unchanged. The assertion was retained. The assembled run passed5/5 without a weakened assertion. This does not establish physical focus behavior under all concurrent desktop activity.

No full suite, desktop smoke, visible app launch, real input injection or cursor movement ran. Physical appearance, hit routing and picker behavior remain user acceptance checks. Installed app and user-owned README.md/Taskfile.yml install edits and the untracked Excalidraw are unchanged.

Verified official sources:
- Layered pixel upload:https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow
- Alpha hit-testing and SetLayeredWindowAttributes conflict:https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows
- Premultiplied alpha:https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-blendfunction
- Selected-font text measurement:https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-gettextextentpoint32w
