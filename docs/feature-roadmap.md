# Missing features and proposed improvements

## Current baseline

PoC 3 provides freehand pen, straight line and outline rectangle on the primary display, with fixed orange color and 5-pixel thickness. Shape previews commit on release. Tool or mode changes cancel unfinished shapes while preserving committed annotations and pen ink. Toggle, clear and quit are available through global shortcuts; P/L/R select tools only while drawing.

The user reports that PoC 3 worked. This is general desktop acceptance, not individual sign-off on every focus, capture-loss or latency scenario.

## Missing features

These are backlog candidates, not approved requirements or scheduled deliveries.

| Area | Missing capability |
| --- | --- |
| Editing | Undo/redo; erase individual annotations or parts of strokes |
| Appearance | Color selection; adjustable stroke thickness; translucent highlighter |
| Geometry | Arrows; ellipses; optional fills and snapping |
| Text | Text annotations |
| Visible controls | Tool/mode indicator; toolbar; tray controls |
| Preferences | Configurable shortcuts; saved preferences |
| Saving | Save/load annotation sessions; export an annotated image |
| Distribution | Packaged release or installer; optional launch at login |
| Displays | Drawing on multiple monitors; mixed-DPI and negative-coordinate layouts |

## Unverified behavior

These are checks to run, not features known to be absent or broken.

- Whole-display sharing and recording in the user's chosen application.
- Application-window capture, which may omit a separate overlay window.
- Borderless and exclusive fullscreen compatibility. Exclusive fullscreen coverage is not guaranteed.
- Focus and normal typing after mode changes, held-button clear/toggle/quit, capture loss and outside-display release.
- Pointer responsiveness with long drawings and high resolutions. No performance regression has been established.
- Other topmost windows and elevated applications.

Secure desktop and lock-screen drawing remain outside scope. No GPU rewrite is proposed without evidence that the current renderer is too slow.

## Deferred work

The user currently has no multi-monitor setup. Defer the previously proposed monitor/DPI PoC until suitable hardware is available. Keep its acceptance checks in the historical plan; do not claim they passed or remove them.

Sharing/recording validation can still run on one monitor. It needs the user's sharing application and a recording or recipient-side observation, not only a local overlay preview.

## Recommended next increment: undo and redo

This is a proposal awaiting approval. Undo avoids clearing the entire canvas after one mistake and requires no new toolbar, dependency or renderer backend.

Proposed scope:
- Undo the most recent completed pen stroke, line or rectangle.
- Redo annotations in their original order.
- A new annotation after undo discards the redo branch.
- An active drag is cancelled or ended before a history action; define the pen behavior before implementation.
- Decide whether clear is undoable before implementation.
- History shortcuts are active only in drawing mode, so ordinary editing shortcuts remain available in click-through.
- Existing tool selection, mode switching and safe shortcut-conflict behavior remain unchanged.

Acceptance should cover mixed annotation types, repeated undo/redo, empty history, redo invalidation, active gestures, clear semantics and shortcut registration scope. Desktop observation must confirm normal editing shortcuts return in click-through.

## Follow-on improvements

| Priority | Proposed increment | Value and boundary |
| --- | --- | --- |
| 2 | Color and thickness presets | A small palette and a few widths. Each annotation keeps the style chosen when it began; changing settings does not recolor previous work. No full settings dialog initially. |
| 3 | Nonactivating status indicator | Show drawing versus click-through, selected tool and current style without moving keyboard focus. Start with an indicator rather than a full toolbar. |
| 4 | Arrow and ellipse tools | Add common presentation shapes with the same preview/commit/cancel rules. Keep text and snapping separate. |
| 5 | Selective erasing | Remove a whole annotation first. Partial-stroke erasing is a separate, more complex capability. |

Run single-monitor sharing validation when the target application is known, rather than waiting for all these features. Saving, text, tray controls, preferences and packaging stay unscheduled until needed.

No new PoC number or implementation scope is approved by this document. Existing workflow rules remain in docs/workflow-strategy.md.
