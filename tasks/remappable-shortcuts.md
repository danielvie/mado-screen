# Remappable shortcuts and config reload

## Scope

- All 25 keyboard bindings are named entries in `[shortcuts]`, retaining current defaults and logical action IDs. Native registration IDs can change during replacement; app-local posted smoke actions keep their stable IDs.
- Accept one letter, digit, F1 through F11, Escape, Space, Tab, Enter, Backspace, Delete, Insert, Home, End, PageUp, PageDown, arrow keys, and common punctuation keys. Optional Ctrl, Alt, Shift and Win modifiers are case-insensitive. Reject repeated modifiers, unsupported keys, F12 and duplicate bindings.
- Missing settings use defaults. Invalid startup values use existing fallback behavior. Reload rejects invalid known values and duplicates without rewriting the file or changing live preferences.
- `[arrow].head_scale` accepts finite numbers from 0.1 through 4.0. Default 1.0 preserves current geometry. Scaling happens before the short-arrow cap. Existing annotations keep their geometry.
- Tray Reload config applies brush colors/width, arrow direction/size, fade duration, menu visibility and all shortcuts without restarting. Preserve drawing mode, selected tool, pin/position, completed ink, temporary birth times and history. Settle active input on successful reload.
- Shortcut changes are transactional, including swaps between bindings. Failed acquisition leaves old bindings untouched and rejects the reload. New combinations are acquired before obsolete registrations are released. Drop queued old hotkey messages after replacement.
- Report reload success/failure visibly through a tray notification. Updated help, tray labels, tooltips and shortcut badges reflect the active bindings; long badges use a compact key with the complete combination in the tooltip.
- Mouse wheel is not a keyboard shortcut and remains unchanged. No settings GUI, automatic file watching, pin persistence or position persistence.

## Checks

Parser and duplicate validation, defaults and settings completion/preservation, native hotkey replacement and rollback, startup and menu labels, tray command decoding, reload rejection and application with ink/history retained, arrow geometry and fade ages. Run full release tests, debug/release builds, formatting, Clippy and smoke self-check. Physical key delivery and tray interaction remain manual checks.

Validation passed 190 release tests, with one intentionally ignored icon generator, debug/release builds, formatting, Clippy, smoke self-check and diff whitespace checks. Offscreen default/remapped menu previews reviewed. Physical key/tray/notification delivery and assistive technology not tested. The full debug test run exceeded its 120-second limit during a native menu check and is not claimed passed. Review fixes preserve queued quit messages and distinguish committed settings from display-refresh warnings, with all display synchronization attempted. No installation or commit performed.
