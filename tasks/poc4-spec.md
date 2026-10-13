# PoC 4 specification and autonomous execution

## Authorization

User requests undo/redo, color/thickness, visible tool/mode indicator, arrows/ellipses, whole-annotation selective erasing, and C opening a color picker at the mouse. User accepts whole-annotation erasing and asks autonomous execution through finish with a remote push after each completed stage. Routine decisions below use the proposed defaults; no additional approval gates are required during this autonomous run.

Rust 2024, native Win32, windows crate pinned 0.62.2, primary monitor. No new third-party dependencies or GPU rewrite. Multi-monitor, export, persistence, text, snapping, highlighter and installer remain excluded.

## Capability map

- annotation-model: styled annotations, current tool/style and active gesture. No Windows dependency.
- history: undo/redo and undoable clear. Depends on annotation-model.
- geometry: arrow/ellipse paths and stroke hit testing. Depends on annotation-model.
- eraser: whole-annotation removal, topmost-first, undoable drag transactions. Depends on history/geometry.
- desktop-ui: nonactivating status, modeless cursor-position RGB picker. Depends on annotation-model types.
- app-integration: scoped shortcuts, native resource lifecycle and UI/input routing. Depends on all above.

The capability map maps to a small implementation: strokes.rs holds pure model/history/geometry/hit testing; drawing.rs/surface.rs render styled paths; controls.rs owns registration; ui.rs owns native UI; main.rs composes. No generic framework or persistent settings.

## Shared contracts

strokes.rs exports Point=(i32,i32), Tool enum Pen/Line/Rectangle/Arrow/Ellipse/Eraser, Style {pub color:u32,pub thickness:u32} default opaque0xfff97316/5, Annotation {pub paths:Vec<Vec<Point>>,pub style:Style}. Strokes exports tool/select, style/set_color/set_thickness, begin(Point)->bool, append(Point)->bool, finish(), cancel()->bool, clear()->bool, undo()->bool, redo()->bool, is_active(), annotations()->&[Annotation]. Style colors are opaque ARGB; width clamped to odd1..31. Beginning an annotation freezes current style. Existing styles never change retroactively. Separate arrow branches are separate paths, never joined accidentally. Ellipse is an outline bounded by drag endpoints, works in every direction and has bounded tessellation. Eraser hit tests visible stroke geometry, not filled shape interiors; sparse eraser moves sweep previous/current positions. One erase drag is one history action. Pen interruption retains/commits ink; shape interruption cancels preview; eraser interruption commits removals. History actions first settle the gesture, new edits invalidate redo, clear is undoable. Full-scene snapshot history is acceptable for this PoC with a comment naming memory ceiling and delta-history upgrade.

drawing::render(pixels,width,drawing,annotations:&[Annotation]); Surface::present same parameter change. Renderer paints opaque per-annotation color and odd width, retains clipping/background alpha/no stale pixels.

controls::Action existing IDs1..6 preserved; add Undo7 Ctrl+Z, Redo8 Ctrl+Y, Thinner9 VK_OEM_4, Thicker10 VK_OEM_6, Blue11 key1, Orange12 key2, Red13 key3, Green14 key4, White15 key5, Arrow16 A, Ellipse17 O, Eraser18 E, Picker19 C. All new keys drawing-only; MOD_NOREPEAT. Hotkeys::drawing_keys(advanced:bool,picker:bool) permits staged delivery; basic keys4..15 always, advanced16..18 flag, picker19 flag. All-or-nothing registration with accurate key names and partial rollback. Permanent CtrlAltD/C/Q unchanged. Tools/colors remain selected across clear/mode changes.

ui.rs exports DesktopUi::new(owner:HWND,width:i32,height:i32)->Result<Self>, update(drawing:bool,tool:Tool,style:Style)->Result<()>, open_picker(color:u32)->Result<()>, close_picker(), is_picker_open()->bool, handle_message(&MSG)->bool, event(&MSG)->Option<UiEvent>, UiEvent::{Color(u32),Closed}. Modeless popup, no blocking native modal dialog. C invokes GetCursorPos, clamps to nearest monitor work area, supports arbitrary RGB plus presets; native labeled inputs/buttons keyboard-accessible, Escape/cancel preserve prior color. Popup may activate intentionally; status never activates and is click-through. Drop destroys owned windows/classes, no dangling userdata/GDI objects. UI posts value-only WM_APP events to main thread/window (main intercepts queued MSG); safe cleanup even if partial construction fails. Main suspends all drawing-only keys while picker open so text controls work, permanent toggle/clear/quit remain processed. Closing reacquires keys or safely returns click-through if registration conflicts. Opening picker cancels active shape/ends pen or eraser drag; no picker clicks paint canvas. No system input injection or foreground/cursor automation in checks.

## Stages and remote checkpoints

1. Styled annotations, thickness/color presets and undo/redo. Native input only Pen/Line/Rectangle; helper geometry for stage2 may already exist as independently-tested domain code.
2. Expose Arrow/Ellipse/Eraser and extend geometry/history desktop proof.
3. Visible status and C modeless picker, safe key suspension/reacquisition, final docs and checks.

Use one persistent integration worktree and Cargo cache, max4 isolated workers, serial private branch rebases/ff. Worker focused checks only. No full checks per delivery. At stage boundary after admitted work completes, run task build/test/check plus applicable safe smoke. Repair failures, promote clean main only when green, then git push origin main with no force. Record exact commits, push outcome and untested manual evidence. Never overwrite unrelated local/remote commits. Fetch before promotion; unexpected remote advance requires safe reconciliation or explicit blocker.

## Verification

Pure tests cover history branching/restoration/clear, frozen styles, every geometry direction/degenerate input, eraser overlap/interiors/sparse movement/undo grouping, styled raster width/color/alpha/clipping, scoped IDs/modifiers, popup positioning/color validation. Build: task build. Tests: task test. Check: task check. Runtime: scripts/smoke.ps1 -Exe target/debug/mado-screen-poc.exe -PostedStroke, updated for each stage. Only owned copied app messages/windows manipulated; physical keys/cursor/focus checks remain manual. User's PoC3 worked report is not exhaustive acceptance of new behavior.
