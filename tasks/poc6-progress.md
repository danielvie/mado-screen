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

All deliveries were privately rebased and fast-forwarded serially without conflicts. Read-only review found no concrete lifecycle/history defect. Event-segment expiry is intentional; sparse segments disappear together. Physical rendering performance and exact observed timer latency remain untested.

## Gates

At 0c1c8b2: task build passed, task test 89 passed, task check formatting/Clippy passed, PowerShell smoke self-check passed. The hidden native timer check verifies expiry ID 2 delivery and cleanup without showing windows or changing focus/input. Pure tests inject monotonic times and establish exact two-second boundaries, pause behavior, styles, separate gestures and permanent history preservation.

Desktop attempt 1 failed in startup polling with a null expression; startup logs subsequently showed the app was ready. Logs: C:/Users/ae904f/AppData/Local/Temp/mado-poc6-7ef8bc91-3bfb-4d37-a1cb-c01cc53ff7c9.

Desktop attempt 2 captured pixels successfully, confirmed virtual origin (-1920, 0), extent 5760x1200, startup/style/status/idle, pen and line rendering, then failed the legacy rectangle corner assertion. Logs: C:/Users/ae904f/AppData/Local/Temp/mado-poc6-fd8298bf-8d73-4acf-8d15-8c690ab47c5c. No full desktop pass is claimed. A focused smoke-worker investigation is assigned; failures are not disabled. Copied child cleanup ran. No real mouse/key input or cursor/focus change was injected.

## Manual acceptance

- [ ] Repeat P and confirm indicator changes Pen / Temporary pen.
- [ ] Draw continuously longer than two seconds; the tail disappears while new ink remains solid.
- [ ] Release and pause; remaining ink disappears without restarting the countdown.
- [ ] Switch tool, open/close picker and toggle click-through; existing temporary ink continues expiring.
- [ ] Clear removes temporary ink immediately; undo cannot restore it and permanent redo survives.
- [ ] Repeat A and inspect both direction labels and arrowheads; old arrows keep their geometry.
- [ ] Test both features on all monitors and across seams; observe responsiveness.

## Delivery status

Implementation is assembled on integration/poc6. Final desktop smoke is pending investigation. Main remains unchanged until validation or explicit user acceptance authorizes promotion. Run the prototype with `task run` from C:/SANDBOX/REPOS/mado-screen.git.integration_poc6; use Ctrl+Alt+Q to close it before smoke. No remote push or installed-app replacement is requested.
