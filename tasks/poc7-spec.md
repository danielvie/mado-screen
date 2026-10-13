# PoC 7: rounded ink and fading

## Approved scope

The user approved the proposed visual increment and implementation. Round stroke ends and joins, antialiased edges, smoother ellipse outlines, linear temporary fading over the full one-second lifetime, and larger open arrowheads in both directions. Reference images show round line ends and smooth outlines. Gradients, shadows, spline smoothing, new settings, new dependencies and another rendering backend are excluded.

Keep permanent history, temporary-history separation, style freezing, picker behavior, click-through and virtual-desktop coordinates. Release does not restart temporary lifetime. Clear removes temporary ink immediately. Rectangle geometry stays rectangular; the shared round brush rounds its joins without changing its path.

## Implementation contracts

- src/drawing.rs renders permanent annotations into the existing CPU bitmap. Each path segment is a round capsule, with circular dots. Fixed 4 by 4 subpixel coverage antialiases edges. Union coverage within one annotation, then composite once. Separate annotations retain paint order.
- drawing::paint_trail accepts consecutive tuples of gesture identity, Annotation reference and 0..255 opacity. Within a gesture, take maximum faded opacity at each covered sample. Composite separate gestures normally. Premultiply RGB and source-over all BGRA channels. Do not fade the completed permanent bitmap.
- src/trail.rs retains independent per-input-part birth times, one-second expiry and pause protection. paint_parts(now) supplies linear opacity and gesture identity. Public timing remains caller-supplied monotonic Duration. Rounded 8-bit alpha can reach zero less than two milliseconds before model removal.
- src/main.rs animates ticks before picker routing in both input modes, uses one timestamp for each frame, presents final expiry, and drops the timer when empty. The existing 32ms timer is not a precise real-time deadline.
- src/surface.rs renders permanent ink, then timed trail parts, into the existing premultiplied BGRA DIB before UpdateLayeredWindow.
- src/strokes.rs uses arrow reach 48 + 6 * normalized thickness, capped at 75 percent of shaft length. Two open wings and frozen direction/style remain. Ellipse subdivision uses a 0.25px sagitta budget before integer rounding, a multiple of four steps and a 2048-step ceiling. The same paths drive rendering and eraser hit-testing.

## Boundaries and limits

Rust 2024, windows 0.62.2, no dependency changes. Match rustfmt and current module ownership. Clip candidate raster work to the viewport, retaining original endpoints for distance calculations so clipping cannot invent caps. Reusable scanline sample storage avoids whole-desktop supersampling.

Known ceilings: replay scans viewport-clipped rows against each group's segments, and still fills/uploads the full desktop bitmap. Bin segments or change backend only if physical lag justifies it. Sparse mouse segments retain one birth time, so the tail can show age steps. Round joins do not make sparse pointer paths into curves. Extreme ellipses can exceed the sagitta budget at the step ceiling; integer point rounding also contributes edge error.

## Verification

The user explicitly limits verification to new-feature tests and directly affected regressions for speed. This overrides the workflow's full-suite and desktop-smoke gate for this increment only. Keep isolated sibling worktrees, serial integration and promotion after the selected checks pass.

- New-feature check: cargo test poc7_
- Affected renderer check: cargo test drawing::
- Timing and history: cargo test trail::; cargo test tests::temporary_; cargo test tests::expired_temporary
- Affected DIB/geometry checks use named test filters, not the full suite.
- Build: cargo build
- Formatting/lint: cargo fmt --check; cargo clippy --all-targets -- -D warnings
- Script syntax parsing only. Desktop smoke and the full test suite are not run.
- User acceptance: round ends at thin/thick widths, softer edges, arrow proportions/direction, a full-second fading tail after release and in click-through/picker, responsiveness on the user's monitors.

Always preserve user install edits and report untested physical checks. Never inject system input, move the cursor, steal focus, replace the installed app, or push remotely without a request. Ask before widening visual scope or introducing a dependency/backend.

## Platform references

Existing premultiplied-alpha upload contract: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-blendfunction

Window position/upload: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow

Existing timer semantics: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-settimer and https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-killtimer
