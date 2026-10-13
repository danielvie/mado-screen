# Features and remaining work

## Bézier and performance follow-up

Approved: trial streaming quadratic Bézier smoothing for both freehand pens and repeated release-mode rendering measurements. Keep shape tools, sizes and the one-second lifetime unchanged. Scope and evidence: tasks/bezier-performance.md. Offscreen CPU timings do not establish Windows upload cost or physical latency.

## PoC 7 scope

Approved: rounded caps/joins and antialiased stroke edges, linear temporary fading over one second, roughly doubled open arrowhead reach and smoother ellipse subdivision. Existing tools, styles, history and multi-monitor behavior remain. No gradients, shadows, curve smoothing or new dependencies. Scope and results: tasks/poc7-spec.md and tasks/poc7-progress.md.

## PoC 6 scope

Approved: solid temporary Pen, repeated P toggles permanent/temporary; repeated A toggles head-first/tail-first arrow direction. Indicator shows the variant. Temporary parts expire after about one second, including after release, and never enter undo/redo. Contracts/evidence: tasks/poc6-spec.md and tasks/poc6-progress.md.

## PoC 5 scope

The user approved drawing across multiple monitors and confirmed hardware is available. Sharing/recording stays deferred. PoC 5 uses one virtual-desktop overlay, shared annotation history and physical-pixel strokes across screens; all PoC 4 tools remain. Restart after monitor/layout/scaling changes. Specification and results are in tasks/poc5-spec.md and tasks/poc5-progress.md.

## PoC 4 scope

The user accepted PoC 3 and authorized autonomous PoC 4 delivery with validated remote checkpoints. Specification and stage evidence are in tasks/poc4-spec.md and tasks/poc4-progress.md.

PoC 4 covers:
- Pen, line, rectangle, arrow and outline ellipse, with live shape previews.
- Per-annotation color and odd thickness1..31; existing work retains its style.
- Undo/redo, including clear and grouped whole-annotation erasing.
- Whole-annotation eraser, topmost-first. Clicks erase one topmost hit; moving sweeps can remove multiple touched layers. Shape interiors are not filled hit targets.
- Visible click-through/nonactivating mode/tool/style indicator.
- Drawing-only C opens the approved modeless disk picker at the mouse, clamped to the work area. Hue/saturation disk, brightness, presets and exact hex input update a candidate until Apply.

All three stages are implemented and validated. Final code 7f934a7 passed 55 tests and 79 desktop checks with zero skipped, plus hidden native picker/control checks and visible indicator review. Stage pushes and manual limits are recorded in tasks/poc4-progress.md. This document does not replace those checks.

## Missing capabilities

These remain candidates, not approved requirements or scheduled deliveries.

| Area | Missing capability |
| --- | --- |
| Editing | Partial-stroke erasing; selecting/moving/resizing existing annotations |
| Appearance | Translucent highlighter and opacity settings |
| Geometry | Filled shapes and optional snapping |
| Text | Text annotations |
| Visible controls | Full toolbar and tray menu |
| Preferences | Configurable shortcuts and saved settings |
| Saving | Save/load annotation sessions; annotated-image export |
| Distribution | Packaged release or installer; optional launch at login |
| Displays | Live monitor/layout/scaling changes without restart; layouts beyond the signed client-coordinate limit |

## Unverified compatibility

Unverified does not mean broken. Verify separately:
- Whole-display sharing and recording in the user's chosen application.
- Application-window capture, which may omit a separate overlay window.
- Borderless and exclusive fullscreen; exclusive fullscreen coverage is not guaranteed.
- Real keys/mouse, focus restoration, held-button interruptions, capture loss and outside-display release for PoC 4.
- Interactive disk-picker keyboard navigation, visible placement, dragging and focus on the user's desktop. Hidden native checks and offscreen rendering pass; physical behavior and assistive-technology acceptance remain manual.
- Long-session memory and pointer responsiveness at high resolution. History snapshots clone the scene, and each changed event replays/uploads a full CPU bitmap.
- Elevated applications and other topmost windows.

Secure desktop and lock-screen drawing remain excluded. No GPU rewrite is justified without measured lag.

## Monitor acceptance and deferred sharing

Multi-monitor hardware is now available. PoC 5 automated checks cover the observed layout only. Physical gestures across seams, alternate above/below arrangements and mixed-DPI picker/focus behavior require manual acceptance. Live reconfiguration is deferred; restart the app after display changes.

The user deferred sharing/recording validation. When resumed, name the capture application and use a recording or recipient observation, not only a local preview.

## Proposed next improvements

PoC 7 is the approved visual increment. User visual acceptance and physical responsiveness remain to be observed. If daily use shows a concrete problem, choose one small increment from the remaining capabilities. A tray menu or saved preferences may improve convenience; partial-stroke erasing and text should remain separate increments because their behavior is substantially different.
