# PoC 4 autonomous stage results

## Stage 1: styled annotations and history

Validated code b65c0e2. Build passed, 47 tests passed, formatting/Clippy passed. Desktop smoke passed 61 checks, zero skipped, on 3440x1440/16 logical processors; idle process CPU delta0seconds over30seconds. Artifact C:/Users/daniel/AppData/Local/Temp/mado-poc4-3b74a1ce-e0e2-421f-b72f-f0e30fd8642d/results.json.

Verified posted undo/redo/undoable clear, mixed frozen colors/widths, pen/line/rectangle and previous cancellation/conflict/cleanup checks. Basic drawing-only CtrlZ/Y, brackets and color1..5 are available. Arrow/ellipse/eraser domain code is tested but not exposed until stage2. Native UI remains stage3.

Serial deliveries in the same integration worktree/cache: controls d3dcbd6->f6bf6e0; model463b7cf->355b0ab; render131a207->c3e7a6a. No conflicts or per-delivery full checks. Three workers ran focused checks; the independently owned future-stage UI worker can continue without touching stage1 files.

Main promoted and origin/main push succeeded at eaea201 after these gates. No force pushes. No input injection, cursor movement or focus automation; actual physical keys/mouse and hardware latency remain manual.

## Stage 2: arrows, ellipses and erasing

Validated code7665c10. Build/formatting/Clippy passed,49tests passed. Desktop smoke71passed,zero skipped. Arrow shaft/tip, reversed ellipse bounds/interior, whole-annotation erasing, eraser undo/style restoration and empty-interior misses passed. Native idleCPU0.046875seconds over30seconds,0.00976percent normalized. Artifact C:/Users/daniel/AppData/Local/Temp/mado-poc4-38bfee1e-d3da-4936-8b14-52fbc7d0d982/results.json.

Repair81c5325->7665c10 handles all intersected annotations on sparse moving sweeps, topmost-first; stationary clicks remove only the topmost.4pxhit slop covers painted square-brush corners. One drag is one undoable history action. Focused18tests passed before serial integration; stage combined checks followed delivery. Main promotion and origin/main push succeeded at c7f7ad4 after these gates.

## Stage 3: indicator and cursor color picker

Completed, validated code 7f934a7. Build, formatting/Clippy passed; 55 tests passed, including a hidden native picker/control test. Desktop smoke passed 79 checks with zero skipped; idle CPU delta 0 seconds over 30 seconds. Artifact C:/Users/daniel/AppData/Local/Temp/mado-poc4-2003703b-356a-4ed5-aa2e-8481d0abc0f7/results.json. Read status.png: mode/tool/width/hex color and swatch are legible, with no clipping on the tested display.

UI delivery 7b023ce -> 1c33e57 added modeless native RGB controls and nonactivating/click-through status. Main suspends drawing-only keys while the popup is open; permanent toggle/clear/quit remain processed. Closure reacquires keys or returns safely to click-through on conflicts. Outside canvas clicks dismiss without drawing. Pointer-only userdata has stable ownership; queued UI events carry values only.

Repairs: final Clippy collapsible-match c140499 -> 4b053a3; hidden native controls/status finish gate 3fd5c40 -> b7d5541; status smoke lookup 7f934a7 -> 7f934a7. The failed visibility check was PowerShell converting null class to empty text, not the app hiding its indicator. C# performs the true null-class lookup; strict same-PID/owner/visibility checks remain. Diagnostic captures showed unchanged status and foreground through canvas sampling. No failing check was disabled.

The hidden native test verifies labeled RGB controls, presets, arbitrary Apply, invalid-channel rejection, Cancel/Close, event order/lifetime and class/window cleanup without showing windows or altering focus. Read-only skeptic found no concrete routing/lifetime bugs. Visible interactive picker focus/navigation/placement, physical shortcut delivery and held-button/capture timing remain manual; tests do not open/focus the live picker or inject system input.

Final main promotion and origin/main push follow these gates. Final record changes documentation only; application code is identical to validated 7f934a7.
