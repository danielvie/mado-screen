# Fixed recent-color slots

The user reports recent swatches move after selection and requests fixed positions. This supersedes newest-first ordering in the original layout contract.

Selecting an existing color changes the selection highlight without reordering entries or advancing replacement. New distinct committed colors fill empty slots from left to right. Once five slots are occupied, each new color replaces the oldest added slot in place, cycling through slots. Other swatches do not move. Geometry uses all five slot positions even while some slots are empty, so adding a color does not shift existing swatches.

Current-color click still requests the picker. Different-color click still selects directly. Preview and Cancel do not add colors. Storage remains session-only. Panel size, antialiasing, drawing and shortcuts are unchanged.

Implemented in src/ui.rs in sibling integration/fixed-colors from main5897992. No workers or concurrent file writers. Main stayed unchanged until validation. User README.md/Taskfile.yml install edits and both untracked Excalidraw files remain separate.

Checks passed:
- cargo test recent_colors_keep_slots:1 new hidden-native regression. It dispatches/commits selections, checks selected-color OpenPicker requests, verifies stable centers/order, fills all slots and checks replacement wraparound without recency advancement from reuse.
- cargo test ui::tests -- --test-threads=1:6 focused UI tests including the new test, existing alpha/layout/nonactivation and picker candidate/Cancel/Apply checks.
- cargo build --release.
- cargo fmt --check.
- cargo clippy --all-targets -- -D warnings.
- git diff --check.

No full suite, desktop smoke, visible app launch, real input injection or cursor movement. Physical acceptance remains for the user. Promote serially and copy only the local target/release executable; do not install or push remotely.
