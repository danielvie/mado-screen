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

Run only Bézier tests and directly affected renderer/pen/trail/history/eraser regressions, plus optimized build and formatting/Clippy. No full test suite or desktop smoke. Record final smoothed measurements and any failures below before promotion.
