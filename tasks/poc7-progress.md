# PoC 7 delivery

## Status

Approved implementation is complete. User visual acceptance remains open. Main promotion follows the explicitly reduced verification scope in tasks/poc7-spec.md. No remote push or installed-app replacement is requested.

## Deliveries and integration

- Base main: ab3e730, one-second solid temporary ink.
- Shapes worker: e8425b6, src/strokes.rs only. Serial fast-forward into integration/poc7. Focused arrow, ellipse, eraser and preview checks passed, 14 tests.
- Renderer worker wrote src/drawing.rs but stopped on an authentication error before finishing verification. Supervisor inspected the actual changes, completed tests and committed 5af72e9 on worker/renderer-poc7. Serial rebase onto e8425b6 produced e1d2fcd, then fast-forwarded integration/poc7. No worker remained active during that rebase.
- Supervisor composed Trail opacity/grouping, Surface rendering, timer animation and documentation in the persistent integration worktree. Added overlap/source-over tests missing from the interrupted renderer delivery.
- Worktrees are sibling directories mado-screen.git.integration_poc7, mado-screen.git.renderer_poc7 and mado-screen.git.shapes_poc7. Main install edits stay separate.

## Focused checks

Passed:

- cargo test drawing::, 14 renderer checks before the three additional overlap/fade checks.
- cargo test poc7_, 15 new-feature checks across renderer, timing, DIB composition, arrow/ellipse geometry and eraser/history.
- cargo test trail::, 12 timing/gesture checks.
- cargo test tests::temporary_, 2 input/history checks.
- cargo test tests::expired_temporary, 1 reveal-underlying-ink check.
- cargo test surface::tests::negative_and_outside, 1 clipping check.
- cargo test surface::tests::dib_renders, 1 native DIB/resource check without showing a window.
- cargo test tests::shape_preview, 2 affected preview/cancellation checks.
- Shape worker filters listed above cover 14 arrow/ellipse/eraser/preview tests.
- cargo build; cargo fmt --check; cargo clippy --all-targets -- -D warnings.
- PowerShell parser validates scripts/smoke.ps1 syntax only. Changed startup, fading and larger-wing expectations; the script was not executed against the desktop.

One new blending assertion initially expected the wrong green-channel rounding. Independent per-channel source-over arithmetic gives 0xff9a7a86 for half-opacity blue over orange; corrected that expectation and the focused checks passed. Clippy initially flagged constant-size chunk iteration, a needless borrow and test clones; fixed those rather than suppressing warnings.

The full test suite and extensive desktop smoke were intentionally not run at the user's request. Existing smoke assertions were updated but are not claimed as passed. No injected system input, cursor movement, focus change or overlay launch occurred in this delivery.

## Manual acceptance and limits

- [ ] Restart the rebuilt executable and compare rounded ends/joins and antialiased edges with the user's references.
- [ ] Observe the one-second fading tail during/after drag, with picker open and in click-through.
- [ ] Accept larger open arrowhead proportions in both directions and smooth ellipse outlines.
- [ ] Observe physical responsiveness across the user's monitors.

The user's interim screenshot still showed flat square ends before PoC 7 was promoted. It is not evidence of the new renderer.

Rounded rendering preserves pointer paths, without spline smoothing. Sparse input can retain angular bends and per-part age steps. Extreme ellipses exceed the 0.25px sagitta budget at the documented 2048-step ceiling. Full-desktop CPU replay/upload and group scan costs remain; real performance is unmeasured for this increment.
