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

## X shortcut and highlighted help follow-up

Implemented directly on main at the user's request, without another worktree.

- `X` clears permanent and temporary ink while drawing and the picker is closed. `Ctrl+Alt+C` keeps its existing global behavior. Both use the same clear handler, including capture/timer cleanup and undoable permanent ink. Tool and style remain unchanged.
- `X` has its own hotkey ID 20, uses the existing drawing-only registration guard and suppresses key repeat. Stale queued X actions are ignored in click-through and while editing the picker.
- Help titles use bold styling. The initial teal/yellow highlighting was replaced by the Pi dark palette below. Explanations remain normal text, with the original column spacing preserved.
- Enable virtual-terminal output only for a supported terminal. Flush the guide and restore the previous console mode afterward. Redirected and unsupported output remains plain text. No new dependency was added; only the existing windows crate's console feature is enabled.

Windows output-mode and styling references:
- https://learn.microsoft.com/en-us/windows/console/setconsolemode
- https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences

Verification:
- `task test-clear-help`: 9 focused release tests passed, covering bindings, drawing/picker guards, clear/undo/redo, active gesture settlement, style preservation, heading/shortcut/command styling and unchanged plain text.
- `cargo build --release`, `cargo fmt --check`, `task --list` and `git diff --check`: passed.
- `cargo clippy --bin mado-screen-poc -- -D warnings`: blocked by three pre-existing warnings. Two nested-if warnings are in unchanged src/ui.rs; the even-width expression in apply_menu_width is also unchanged from main. These unrelated lines were not modified.

The styling test saves target/terminal-help/help-plain.txt and help-styled.ansi for inspection without starting the overlay. Full tests and desktop smoke were not run. Physical X delivery and the terminal's actual bold-font rendering remain manual checks. Local install edits and untracked design files are preserved; no application was launched, installed or pushed.

## Pi palette correction

The user rejected the bright teal/yellow pairing. User settings at C:/Users/ae904f/.pi/agent/settings.json select `dark`, with no project theme override. Colors come from C:/SANDBOX/APP/pi/theme/dark.json:

- Shortcuts: mdCode/accent violet, `okhsl(295 50% 67%)`, resolved to #A798D7.
- Command labels: mdLink blue, `okhsl(232 54% 67%)`, resolved to #69ADD0.
- Bold titles: mdHeading gold, `okhsl(83 88% 67%)`, resolved to #CD9A22.

This copies the active palette; Mado does not load Pi settings at runtime. Normal text keeps the terminal default. No shortcut behavior, spacing or plain-output behavior changed. Conversion used the OKHSL author's reference implementation: https://raw.githubusercontent.com/bottosson/bottosson.github.io/master/misc/colorpicker/colorconversion.js.

Two focused startup-help tests passed, including exact palette assertions and removal of the old colors. The normal release path was locked and its build failed with access denied; no running application was stopped. `cargo build --release --target-dir target/pi-palette-check`, `cargo fmt --check` and `git diff --check` passed. The validated executable is target/pi-palette-check/release/mado-screen-poc.exe. No full suite, desktop trial, install or push ran.
