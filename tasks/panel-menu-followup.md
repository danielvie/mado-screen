# Floating menu reference and compact brush-tip follow-up

## Scope

The user reports mismatched control order, overly rounded elements, missing toolbar hover feedback, a visible compact-box outline inside the full menu and no visible animation. The user also revised section 8 of docs/design/mado-screen-panel-layout-and-transitions.excalidraw with one compact brush-tip circle.

Worktree: C:/SANDBOX/REPOS/mado-screen.git.floating_menu, feature/floating-menu, starting from b414fd5, which is now on main. Only this follow-up stays unintegrated until reviewed. The main checkout's install edits and latest design are preserved. No installed-app replacement, merge, remote push, real input injection or cursor movement. The user later authorized a more thorough menu-only test; one temporary release instance was opened for a scoped owned-window trial and closed afterward.

## Layout contract

- Header order: tool name on the left, input toggle to its right, selected-tool icon and grip at the far right. Show Ctrl+Alt+D alongside the toggle, on one line.
- Stroke preview below the header; no extra RGB/width text row. Width slider below the preview with the width number on the right.
- Keep the populated two rows of eight colors, separator and existing tool icons with shortcut badges. Remove extra bottom captions; keep full names and shortcuts in tooltips.
- Reduce outer menu/badge radius from 12 to 6 logical px and small control radii to 3 or 4. Color swatches remain circular.
- Add visible hover fills/outlines for bottom tools, including the selected tool, and clear them on leave/cancel. Only repaint on actual hover changes.
- The expanded/intermediate menu has one outer border. Do not draw the compact badge's inner frame; its icon and grip remain stationary and opaque.

## Compact brush-tip contract

Keep the 44 x 44 logical badge and neutral selected-tool icon. Add one circle centered at the badge's left edge and 40 logical px below its top. It overlaps the lower-left corner and shows committed ink color. Diameter grows linearly from 10 to 22 logical px for physical brush widths 1 to 31; its neutral outline is #737C85.

The popup's native bounds and alpha mask must include the exposed circle. The badge's screen-space anchor remains unchanged when the circle changes size or the menu expands upward/downward. The circle belongs to the hover area, not a click target or drag handle. Only the right grip drags.

Hide the circle immediately when expansion begins and during either transition. Restore it only at completed collapse with the latest committed style. Avoid cancelling initial circle hover when hiding it: the opening transition needs a short, nonblocking hover bridge while the final menu grows over the pointer.

## Animation diagnosis

Read-only SystemParametersInfo(SPI_GETCLIENTAREAANIMATION) query succeeded and returned false on this machine. This explains the current intentional instant transition. The user was asked whether Mado should animate despite disabled Windows client-area animations; no override or system-setting change has been approved yet.

Continue respecting the Windows preference. Improve the rendering path by calculating faded sample color once rather than once per RGB channel. Start the transition time after its first current-state frame is uploaded so expensive initial painting does not consume the 150 ms duration. Timer and endpoint behavior must remain bounded and deterministic.

## Verification

Implemented and tested in the worktree. Header/control order, reduced radii, icon-only toolbar, hover/press feedback and hover retained after click release were verified. The compact circle's alpha mask, hover behavior, current RGB/relative diameter, hidden-transition behavior and fixed anchor are covered by new tests.

Passed:
- `cargo test --release panel_menu_followup_ -- --test-threads=1 --nocapture`: 7 passed, 0 failed, 129 older tests filtered out.
- `cargo build --release`: passed without compiler warnings.
- `git diff --check`: passed.
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/menu-followup-check.ps1`: 15 targeted release-app checks passed at actual DPI scale 1.25. Owned menu requests reached every tool, repeated Arrow switched direction, both input-toggle transitions changed canvas styles, native full size was 550 x 380 and foreground focus was unchanged. The trial process exited cleanly. No system input was injected.

The targeted app trial also saved an actual desktop image of the expanded menu, inspected against the reference. Screenshot and results: target/panel-menu-followup/native-trial/desktop-full.png and results.json. An initial posted-coordinate click trial was inconclusive because the real outside cursor could trigger menu collapse; it was not counted as a pass. The final live trial validates queued UI requests and committed runtime state, not physical mouse delivery. Native hidden-window tests separately exercise the actual button hit/hover/press/release paths at 125%.

Testing found and fixed a real hover-feedback issue: mouse release cleared the hover while the pointer remained over a tool. A native 125% regression test now checks persistence. A first 125% anchor failure came from a test changing the DPI scale without resizing before an unchanged-style update; the setup was corrected, not the anchor assertion weakened.

Native timer test dispatches real WM_TIMER messages to a hidden 125% window, with animation enabled only through the test helper. Both directions produced multiple intermediate sizes and stopped at their endpoints. Output: target/panel-menu-followup/native-timer-125.txt. Opening settled around 208 ms and closing around 167 ms in the recorded run, including a diagnostic screenshot render/write. These are not physical animation latency or FPS guarantees, and Windows' setting was not changed.

Reviewed offscreen compact orange5/orange31/white/black, full normal/upward/hovered and transition snapshots, plus actual 125% renders. Paths are under target/panel-menu-followup. A single optimized full panel render measured about 20 ms at 100%; the original debug snapshot measured about 286 ms. Different build profiles and one sample cannot establish a before/after performance improvement. The redundant per-channel color calculation was removed without changing premultiplied rounding; no backend or caching rewrite was added.

The full suite, earlier menu tests, general desktop smoke and Clippy were not run. Physical pointer delivery, mixed-DPI dragging, outside-window capture release and assistive-technology behavior remain user/manual checks.

## Manual trial

1. Verify header and control order against the reference, smaller radii and absence of a nested compact outline.
2. Hover each bottom tool, including the selected one; confirm immediate visible feedback and its tooltip/shortcut.
3. While collapsed, change color and thickness using keyboard, palette and picker. Check the circle's latest color/relative size, white/black outlines and transparent areas.
4. Hover only the exposed circle. Confirm it opens the menu without flicker, while clicking it does not draw or select a tool.
5. With Windows client-area animations enabled, check both transition directions and quick reversal. If disabled, instant behavior remains intentional until an override is approved.
6. Drag the grip, test upward opening near the bottom and mixed-DPI placement. The badge must not move during a transition and the circle must not be clipped by the popup/work area.
