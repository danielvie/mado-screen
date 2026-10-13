# Floating menu implementation

## Scope and workspace

User approved the full reference-led menu and new graphite theme, with tests only for new features and manual desktop acceptance favored. Worktree: `C:/SANDBOX/REPOS/mado-screen.git.floating_menu`. Branch: `feature/floating-menu`, based on `ce3bcc5`.

The original checkout and installed executable are not changed by implementation. No merge, remote push, visible app launch, desktop input injection or cursor movement is authorized for this loop.

## Delivery

Implemented and ready for a manual trial. Source changes are limited to src/ui.rs and src/main.rs. Existing canvas rendering, stroke geometry, temporary-ink lifetime and global shortcut bindings remain unchanged.

Delivered features:
- Graphite panel, warm-white text, neutral borders and teal UI highlights.
- Compact selected-tool icon plus separate far-right grip.
- Direct compact-to-full-menu hover transition, with reversible 150 ms motion and disabled-animation preference.
- Mouse-selectable tools with shortcut letters and exact Pen/Temporary selections.
- Canvas input toggle sharing Ctrl+Alt+D behavior.
- Two fixed rows of eight colors, stroke preview/picker access and odd width slider.
- Session-only grip placement, clamped to monitor work areas with a fixed right badge.

## Verification

Passed against the assembled worktree:

- `cargo test panel_menu_ -- --test-threads=1`: 14 passed, 0 failed, 115 older tests filtered out. Seven main-loop tests and seven UI tests.
- `cargo build --release`: passed without compiler warnings. Executable: `target/release/mado-screen-poc.exe`.
- `git diff --check`: passed. Git emits only the repository's LF/CRLF conversion notices.
- Changed Rust files formatted directly with rustfmt, without formatting unrelated modules.

The first filtered-test attempt stopped at compilation while the UI replacement was in progress; no tests ran in that attempt. SDK types and obsolete panel-test references were corrected. The full suite, legacy regression filters, Clippy and desktop smoke were not run, per user scope.

The new tests cover exact tool variants, idempotence, frozen history/styles, temporary ages, Arrow direction, invalid width requests, input-mode registration conflict, fixed palette/shortcut metadata, geometry at 100/150/200 percent, typed event validation, reversible/reduced-motion transitions, timer cleanup, grip clamping, hidden native resources, picker gating, same-target clicks and slider release outside bounds. Hidden windows do not prove real mouse/focus behavior.

Reviewed actual offscreen renders:
- `target/panel-menu/panel-menu-full-100.png`: opaque graphite body, both color rows, shortcut badges, toggle and selected tool.
- `target/panel-menu/panel-menu-upward-100.png`: upward layout with 31px custom stroke, Click-through toggle and Arrow tail-first label.

Four old UI tests asserting the removed five-recents/measured-width implementation were replaced by seven new panel_menu_ tests, not disabled: hidden_panel_hover_recents_anchor_nonactivation_and_previews, panel_pixels_and_measured_layout_at_100_and_150_percent, recent_colors_keep_slots_on_selection_and_replace_only_oldest, and recent_commit_during_picker_preserves_candidate_cancel_and_apply. Legacy picker tests remain present and compile, but were not run.

Task command for future checks: `task test-menu`.

## Manual trial

From the worktree, run `cargo run --release`, or start `target/release/mado-screen-poc.exe`. If an older Mado Screen instance owns the global hotkeys, quit it yourself before starting this branch's binary. No installed executable is replaced.

1. Hover the badge. Check the tool icon and separate right grip, dark colors, full menu and smooth reversal on leaving/re-entering.
2. Click every tool. Confirm the shortcut letter, selected highlight and resulting annotation. Pen/Temporary clicks select exactly; selected Arrow click changes its direction.
3. Select colors across both rows, including black/white. Adjust width. Existing work must retain its style; slots must stay in place.
4. Open the picker from the preview/selected color. Apply a custom color and cancel a later change. Check normal editing and restored shortcuts.
5. Click Drawing/Click-through in both directions. Type and click in an underlying application while click-through. Ink remains visible and the panel remains accessible.
6. Drag only the grip across monitors and near each edge. Check no canvas mark, no keyboard focus change, no jump during expansion and safe release outside the panel.
7. Disable Windows animations and check instant switching. Observe idle responsiveness after closing the menu and after temporary ink expires.

Physical input delivery, outside-window release, mixed-DPI placement, tooltip ergonomics and assistive-technology behavior remain untested until observed by the user.
