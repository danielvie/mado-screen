# Remove standalone color picker recovery

Supervisor: none

All admitted work is terminal. User requested preserving the working dark main-menu palette and removing the mismatched standalone color picker. Implemented in parent working tree C:/SANDBOX/REPOS/mado-screen.git on main db791fd, left uncommitted as the user did not request a commit/push. Initial tracked checkout was clean; .pi/ was already untracked.

Explore ab1933a8-e180-4cc completed read-only mapping. Worker 7c14dac9-30b0-42f completed src/ui.rs only at bf926b09ac001553b023ee0d56c41d359e4b6539, branch worker/remove-standalone-picker-ui / pi-agent-7c14dac9-30b0-42f. Actual entire UI diff reviewed before restoring that file into parent's worktree. Worktree cleaned by harness. Parent removed picker routing and C binding in main.rs/controls.rs, deleted src/picker.rs, updated current README/roadmap and smoke wording. Existing Taskfile.yml preserved. Historical design/spec records kept.

Behavior: 16 fixed menu colors/layout/rendering preserved; palette selection posts Color and owner no-ops identical color; preview remains informational and posts no event. Removed picker classes/controls/state/cache/rendering/HSV/hex input/shortcuts/suspended-key routing. Ctrl+Alt+C remains clear. Protocol kinds 1/4/5/6 unchanged, obsolete kinds 2/3 rejected.

Passed assembled checks:
- cargo test --release -- --test-threads=1: 133 passed.
- cargo build --release, artifact target/release/mado-screen-poc.exe.
- cargo fmt --check.
- git diff --check.
- Windows PowerShell smoke.ps1 -SelfCheck: 7 pass summaries, no app opened.
- Windows PowerShell parser for smoke.ps1.
- Offscreen full-125-drawing and compact-125-5-f97316 previews reviewed, retained design.
- Earlier standalone controls check: 6 tests; worker UI-only harness: 16 tests.

cargo clippy --all-targets -- -D warnings FAILED on four pre-existing warnings: ui.rs nested ifs, main.rs width%2, and UI test palette index loop. Reproduced exact four on unchanged db791fd in ignored target/picker-removal-baseline, transcript target/picker-removal-baseline-clippy.txt. No warning disabled and no unrelated lint refactor made. README records checks/limits.

No visible app launch, physical input, focus/cursor changes, process stop, installed-app replacement or remote push. Physical acceptance remains user testing. Parent code/diff reviewed and no picker entry points remain in src/scripts. Final response should report removal, preserved palette, C removed/Ctrl+Alt+C kept, release build and133 tests passed, Clippy baseline warnings. Run with task run-release; installed app unchanged.
