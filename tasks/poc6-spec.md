# PoC 6: temporary pen and arrow direction

## Approved behavior

The user approved solid temporary ink, no fading in this PoC, approximately one-second lifetime from drawing time, and no temporary undo/redo entries. P selects Pen; P while Pen is selected toggles permanent/temporary. A selects Arrow; A while Arrow is selected toggles head-first/tail-first. The indicator names the current variant. Defaults remain permanent Pen and head-first Arrow. Variant choices persist across tool changes, clear and drawing/click-through toggles. Old annotations never change direction or style. Fading and other visual improvements are deferred.

Temporary ink keeps expiring after release, tool changes, picker opening and click-through. Clear removes it immediately. Temporary edits never change permanent history or its redo branch, and expiry cannot resurrect ink. Undo/redo settles the active temporary drag and operates on permanent history only. Eraser edits permanent annotations only; temporary ink expires or is cleared. These are implementation simplifications for this PoC, not new eraser/history semantics.

## Module contracts and ownership

- Model worker owns src/strokes.rs only. Keep existing Tool variants and APIs. Add temporary_pen() -> bool, arrow_tail_first() -> bool, toggle_temporary_pen() -> bool and toggle_arrow_direction() -> bool, where toggle settles any active permanent gesture and returns visible change. Add tool_label() -> &'static str, returning Pen, Temporary pen, Arrow head-first, Arrow tail-first or existing tool names. Existing select(same) remains a no-op; main owns repeated-key behavior. Reverse the endpoints passed into arrow geometry only when tail-first is selected. Default geometry remains unchanged; previews freeze direction because a toggle cancels an unfinished shape.
- Trail worker owns new src/trail.rs only. Import Annotation/Point/Style. Pure Trail with default, begin(point, style, now: Duration) -> bool, append(point, now) -> bool, finish(), clear() -> bool, expire(now) -> bool, is_active(), is_empty(), annotations() -> impl Iterator<Item=&Annotation>. Store independent opaque dot/segment annotations with birth time in a VecDeque. New segments use frozen style and expire at age >= 1 second. Event-segment granularity is intentional: one sparse input segment expires together. Never connect different gestures or retain an expired old anchor when appending after a pause of >= 1 second. No wall-clock calls, Windows, history or timer dependencies in Trail; tests inject times.
- Supervisor owns src/main.rs, src/drawing.rs, src/surface.rs, src/ui.rs, src/input.rs and docs/smoke. Route temporary Pen into Trail without touching Strokes history. Shared render replays permanent then temporary ink without copying the permanent scene. Preserve existing present/render APIs where practical. UI displays tool_label through a minimal label update API. A distinct Win32 timer with RAII lifetime runs only while temporary pixels remain and expires them independent of input/picker state. Kill it once empty and on shutdown. Existing capture watchdog timer ID 1 remains separate.

## Verification and execution

Follow docs/workflow-strategy.md: isolated sibling worktrees with underscores in suffix, at most four workers, one persistent integration worktree/cache, serial rebase/fast-forward delivery, focused worker checks, final combined gates after admitted work is assembled. Main and user local install changes stay untouched until validated promotion. If desktop capture is blocked, record it and offer the integration executable for manual acceptance instead of weakening checks or claiming a pass.

- Pure Trail tests: exact boundary, staggered expiry, expiry during drag and after release, no hover/duplicate bridge, pause/restart, frozen styles, clear and independent gestures.
- Model tests: toggles/labels/defaults, direction in all quadrants, degenerate arrows, toggle preview cancellation, frozen old geometry and permanent history.
- Main/render tests: repeated P/A routing, temporary operations preserve undo/redo, expired segments reveal underlying permanent pixels, clear, cancellation and timer filtering/lifetime. Native hidden resource tests never move cursor/focus or inject input.
- Gates: task build; task test; task check; powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/smoke.ps1 -SelfCheck; applicable desktop smoke with -PostedStroke if no existing overlay and capture available.
- User checks: toggle labels, draw continuously beyond one second, pause/release and observe expiry, switch modes/picker, permanent history retained, arrows both directions, multi-monitor behavior.

## Limits and sources

Rust 2024 and windows 0.62.2, no new dependency. Timer checks use monotonic elapsed time; timer dispatch is not an exact real-time deadline. CPU replay/upload now continues only while temporary ink exists and stops when empty. Full virtual-screen bitmap cost remains, so physical responsiveness needs observation.

Win32 timer ownership/filtering and timer latency are documented at https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-settimer and https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-killtimer. Monotonic time uses std::time::Instant: https://doc.rust-lang.org/std/time/struct.Instant.html.
