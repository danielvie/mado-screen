# PoC 6 implementation and validation

## Scope

User approved temporary solid Pen ink lasting about two seconds per input part, excluded from undo/redo; repeated P toggles Pen/Temporary pen and repeated A toggles head-first/tail-first arrows. Indicator names the variant. Choices persist across tool/mode changes. Fading and visual improvements are deferred. Contracts: tasks/poc6-spec.md.

## Deliveries and integration

Main remains ea2a265 during assembly; existing README.md/Taskfile.yml install changes are untouched. Sibling worktrees follow docs/workflow-strategy.md. One persistent integration worktree/cache: C:/SANDBOX/REPOS/mado-screen.git.integration_poc6.

- Specification 8cad237.
- Supervisor composition 471ed2a, timer ID 2, chained permanent/temporary replay, repeated-key routing and UI labels.
- Trail worker 7c2e6e2 -> 940a465, src/trail.rs only; 10 focused tests, rustfmt and standalone checks passed.
- Model worker 66fc989 -> acbf610, src/strokes.rs only; 26 focused tests, rustfmt and scoped Clippy passed.
- Supervisor verification a40f13c adds routing/history/pixel restoration checks, wider indicator and hidden timer ownership test.
- Smoke worker f6f7b04 -> 0c1c8b2, scripts/smoke.ps1 only; parse/self-check passed. Legacy synthetic actions avoid unintended P/A toggles; new checks cover temporary expiry, permanent redo and arrow direction.
- Smoke repair 3482b34 -> 0217bf3 fixes empty startup-log typing, bounded rectangle readiness and expected drawing-mode alpha-1 background comparisons. Click-through comparisons remain exact and moving samples remain inconclusive. Parse/self-check passed. No app timer or watchdog was disabled.

All deliveries were privately rebased and fast-forwarded serially without conflicts. Read-only review found no concrete lifecycle/history defect. Event-segment expiry is intentional; sparse segments disappear together. Physical rendering performance and exact observed timer latency remain untested.

## Gates

At 0c1c8b2: task build passed, task test 89 passed, task check formatting/Clippy passed, PowerShell smoke self-check passed. The hidden native timer check verifies expiry ID 2 delivery and cleanup without showing windows or changing focus/input. Pure tests inject monotonic times and establish exact two-second boundaries, pause behavior, styles, separate gestures and permanent history preservation.

Desktop attempt 1 failed in startup polling with a null expression; startup logs subsequently showed the app was ready. Logs: C:/Users/ae904f/AppData/Local/Temp/mado-poc6-7ef8bc91-3bfb-4d37-a1cb-c01cc53ff7c9.

Desktop attempt 2 captured pixels successfully, confirmed virtual origin (-1920, 0), extent 5760x1200, startup/style/status/idle, pen and line rendering, then failed the legacy rectangle corner assertion. Logs: C:/Users/ae904f/AppData/Local/Temp/mado-poc6-fd8298bf-8d73-4acf-8d15-8c690ab47c5c. That run did not pass. A focused smoke-worker investigation followed without disabling failures. Copied child cleanup ran. No real mouse/key input or cursor/focus change was injected.

## Final validation

Validated source and script commit: 0217bf3. task build passed; task test 89 passed; task check formatting/Clippy passed; smoke self-check passed. Full posted desktop smoke passed 232 checks with zero skipped on three physical monitors spanning 5760x1200 at origin (-1920, 0). Artifact: C:/Users/ae904f/AppData/Local/Temp/mado-poc6-155faf0b-849f-4679-a8eb-ca27651b4932/results.json. Idle process CPU delta was 0 seconds over 30 seconds.

Verified legacy shapes/styles/undo/redo/conflicts/cleanup, drawing on each monitor, adjacent-seam history, temporary solid overlap and expiry after release/in click-through, permanent redo preservation, saved variant selection, repeated P/A labels in logs and both arrow direction rasters. Hidden native UI checks validate variant text; painted label review, physical long-drag tail behavior, actual input timing and mixed-DPI focus remain manual. Earlier startup/rectangle/undo false positives were smoke-harness defects; repairs retained ink and timer assertions. No system keyboard/mouse input, cursor movement or focus automation was used. All copied smoke processes were cleaned up.

## One-second follow-up

The user reports PoC 6 working and requests reducing temporary ink lifetime from two seconds to one second. Updated the lifetime, boundary/pause tests, startup text, smoke waits and current documentation. Build, 89 tests, formatting/Clippy, self-check and 232 desktop checks with zero skipped completed before the user requested lighter testing. No additional test runs are needed for this change. Desktop artifact: C:/Users/ae904f/AppData/Local/Temp/mado-poc6-85a8fcad-66d2-4509-a9da-fd5dc4064ace/results.json. Earlier two-second evidence above remains historical.

## Manual acceptance

- [ ] Repeat P and confirm indicator changes Pen / Temporary pen.
- [ ] Draw continuously longer than one second; the tail disappears while new ink remains solid.
- [ ] Release and pause; remaining ink disappears without restarting the countdown.
- [ ] Switch tool, open/close picker and toggle click-through; existing temporary ink continues expiring.
- [ ] Clear removes temporary ink immediately; undo cannot restore it and permanent redo survives.
- [ ] Repeat A and inspect both direction labels and arrowheads; old arrows keep their geometry.
- [ ] Test both features on all monitors and across seams; observe responsiveness.

## Delivery status

Main fast-forwarded to 6afdbb0 after all required automated gates passed. The primary executable rebuilt successfully with cargo build. The primary checkout's README.md/Taskfile.yml install changes were restored unchanged. All completed worker and integration worktrees were removed. Manual acceptance remains open as listed above. No remote push, installed-app replacement or interruption of a running user overlay occurred.
