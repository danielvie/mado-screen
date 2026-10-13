# Bézier trial and release rendering measurements

## Scope

The user confirmed release mode improves responsiveness and requested a Bézier trial with performance measurements. Apply mild streaming quadratic smoothing to permanent and temporary freehand Pen only. Shape tools, arrow proportions, width settings, one-second lifetime, history and monitor behavior stay unchanged. No new dependency, backend, system input injection or desktop automation. Physical curve feel and input-to-display latency require user acceptance.

The streaming pen finalizes a midpoint quadratic when enough distinct samples arrive and keeps a replaceable live tail to the latest input point. Flatten curves into the shared path representation so rendering and eraser use the same geometry. Bound subdivision. Do not wait until release or add a timed input buffer. Temporary finalized ink retains its birth time; new input must not refresh old finalized parts.

## Reproducible measurement

Run `task perf`. It compiles `scripts/perf.rs` with `rustc --edition 2024 -O` and runs without opening a window. Output is CSV with minimum, median and maximum milliseconds. `task run-release` runs the optimized overlay for manual review.

The workload uses deterministic 1.5-turn loops at 1920 by 1200 and 5760 by 1200 pixels. Test 24 input points at 5px and 240 input points at 21px. Rendering cases contain four permanent strokes. The fade case adds one timestamped trail from 0 to 950ms and paints it at 975ms. Each case warms up twice then measures nine repetitions. No sleep or frame cap. `black_box` retains work.

Input timings measure model construction, not Windows delivery. Rendering timings include bitmap clearing, path replay, antialiasing and trail compositing. They exclude DIB upload, desktop composition, mouse delivery and physical latency. The 5760-wide case is a synthetic multi-monitor bitmap, not a hardware display claim.

Baseline source is main 4f0d32a. Keep baseline source snapshots outside tracked code so later smoothing cannot accidentally enter the old measurement. Use the same compiler optimization, inputs and sample counts before and after. The task can be run in a checkout at 4f0d32a with the same measurement script for reproduction.

## Optimization attempts before smoothing

| Attempt | Heavy 5760-wide fade median | Outcome |
| --- | --- | --- |
| Baseline square-distance supersampling | 47.45, 48.73, 49.81 ms across three runs | Baseline |
| Bin segments by row and resolve touched spans | 51.99 and 51.05 ms | Rejected, slower on thick loops |
| Certainty checks for full interior/exterior pixels, edge supersampling unchanged, resolve touched spans | 25.56 and 26.58 ms | Kept, 17 renderer checks pass including the unclipped coverage reference |

The certainty margin 0.531px exceeds the distance from a pixel center to every sample in the existing 4 by 4 grid. A sample is accepted without individual distance checks only when the entire grid is inside the capsule, or rejected when entirely outside. Union, premultiplication and layer order are unchanged. Widely separated geometry can still resolve empty spans. Full desktop replay/upload remains.

| Workload | Baseline median range | Optimized median range, before Bézier |
| --- | --- | --- |
| 1920-wide, 24 points, 5px, fade | 7.50–7.86 ms | 4.63–4.92 ms |
| 1920-wide, 240 points, 21px, fade | 23.04–23.82 ms | 13.04–13.50 ms |
| 5760-wide, 24 points, 5px, fade | 21.25–21.93 ms | 13.82–14.40 ms |
| 5760-wide, 240 points, 21px, fade | 47.45–49.81 ms | 25.56–26.58 ms |

## Integration and checks

Working branches integration/bezier and worker/bezier use isolated sibling worktrees. The supervisor owns rendering and measurement; the worker owns Bézier helper and pen models. Serial integration only after worker delivery. User local README.md and Taskfile.yml install edits remain separate.

Run only Bézier tests and directly affected renderer/pen/trail/history/eraser regressions, plus optimized build and formatting/Clippy. No full test suite or desktop smoke.

Worker commit dedd3cf was rebased serially onto integration/bezier to bdcdb3a. It owns src/bezier.rs, strokes.rs, trail.rs and only the module declaration in main.rs. Supervisor reviewed the actual code, updated one obsolete raw-path expectation in the queued-input regression and added the Bézier module to the standalone measurement program. Main install edits and an independently created untracked Excalidraw file remain untouched.

## Combined Bézier results

Same workload and optimization, two independent runs, nine samples per case after two warmups:

| Fading workload | Baseline median range | Final Bézier plus optimized rendering |
| --- | --- | --- |
| 1920-wide, 24 points, 5px | 7.50–7.86 ms | 6.58–6.80 ms |
| 1920-wide, 240 points, 21px | 23.04–23.82 ms | 13.52–14.77 ms |
| 5760-wide, 24 points, 5px | 21.25–21.93 ms | 16.86–16.93 ms |
| 5760-wide, 240 points, 21px | 47.45–49.81 ms | 26.83–26.86 ms |

Smoothing itself adds work, especially for sparse loops requiring more subdivisions. Its model-construction medians are 0.004–0.015ms for the measured inputs. The separate optimized unsmoothed measurements above isolate the extra raster cost. This does not justify claiming Bézier smoothing is a speed optimization. It is a visual change, accompanied by a measured coverage-equivalent renderer improvement.

The heavy final 5760-wide case has maxima of 31.30 and 32.57ms in these runs, before upload. The 32ms timer interval is not a guaranteed rendering deadline. Full-desktop clearing, permanent replay and upload remain potential costs. If release interaction still lags, measure native presentation next, then consider caching unchanged permanent ink. No cache or new backend is introduced speculatively.

## Smoothing details and check results

Midpoints and control points use f64 to avoid i32 overflow. Flattening uses a 0.5px pre-rounding control-to-chord budget and a depth-12 ceiling, at most 4096 edges per quadratic. Integer rounding adds up to about 0.71px. Extreme coordinates can exceed the flatness budget at the ceiling. Reversals split rather than collapsing to a dot. A two-point gesture stays a straight line.

Permanent Pen finalizes only the previous tail and retains its prior path prefix, style and one history action. Temporary finalized pieces inherit the replaced tail's birth; new dots/live tails use the current input time. Duplicate points and release do not refresh ink. A one-second pause creates a separate dot rather than reconnecting expired ink. Piece opacity still has one birth time, not distance-interpolated ages; the one-second lifetime and sizes are unchanged.

Passed checks:
- Supervisor renderer checks, `cargo test drawing::`, 17 tests, including coverage comparison against the unclipped reference.
- Worker: `cargo test bezier_`, 9; `cargo test trail::tests`, 16; pen_ 5, history_ 6, eraser_ 8, temporary_ 7, styles_are_ 3, clear_ 5, variant_toggles_ 3, poc7_ticks_ 1. Filters overlap; these are not a unique full-suite count.
- Assembled integration: `cargo test bezier_`, 9, and `cargo test queued_events_ignore_hover_and_only_redraw_changed_points`, 1. The latter previously failed only because it expected the original raw triple; updated exact expected smoothed geometry and reran successfully.
- `cargo build --release`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` and `git diff --check`.
- `task --list` exposes perf and run-release.

Full suite, desktop smoke, real mouse delivery, visual curve acceptance and native upload timing remain untested for this follow-up. Intermittent Git Bash launch failures occurred before commands started; failed launches did not count as checks.
