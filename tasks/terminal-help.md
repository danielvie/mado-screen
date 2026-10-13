# Clear startup help and quiet terminal

Requested: clearly describe available controls at startup and remove routine event logs. Keep actionable errors. No drawing, shortcuts, ink or UI behavior changes.

Startup prints Mado Screen, initial click-through/drawing instructions, grouped global and drawing shortcuts, all tool names/variants, thickness and numbered colors, picker controls and top-right panel behavior. Routine mode/tool/style/clear/shutdown stdout is removed. Tool and mode status remains in the panel, not the terminal. Compiler/task-runner output is outside the application's logging and is unchanged.

Use sibling integration/terminal-help and worker/terminal-smoke from mainbe8a3a7. Parent owns main.rs/docs. Worker owns scripts/smoke.ps1 only. The smoke script must read native owned panel titles and actual canvas mouse-transparency style rather than removed logs; preserve pixel/routing checks. Parse/self-check only, no desktop smoke.

Parent source delivery3a08b49 includes startup help and a focused test checking all controls. Routine events leave only one startup stdout print; errors still use stderr. Worker deliveryd61fbf7 was rebased serially to95aa287. Actual script changes were reviewed: owned native panel title/style agreement replaces logs; stage-text gates are removed without omitting feature/pixel checks.

Assembled checks passed:
- cargo test startup_help_:1 test, checking control coverage and only one runtime stdout print, with stderr errors retained.
- cargo test style_actions_settle:1 directly affected routing regression.
- cargo build --release.
- cargo fmt --check.
- cargo clippy --all-targets -- -D warnings.
- Windows PowerShell scripts/smoke.ps1 -SelfCheck:7 pass summaries including native tool-title parsing and mode-style agreement.
- Windows PowerShell parser and git diff --check.

No desktop smoke or full Rust suite ran. Worker also checked PowerShell7 syntax, but its -SelfCheck failed on a pre-existing Add-Type Win32Exception reference issue reproduced on the base commit. That compatibility issue was not changed or labeled passed.

Boundaries: preserve local README.md/Taskfile.yml install edits and both untracked drawings. No full suite, real input, cursor/focus changes, app launch, installed-app replacement or remote push. Copy the release build to the project only; use a distinct filename if normal executable is locked.
