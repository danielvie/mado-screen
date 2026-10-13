# Arrow direction and head sizing

User requests larger heads that scale with stroke width, and head-first drawing. Press anchors the head; moving/released pointer sets the tail. Existing completed annotations retain their stored geometry and style. Lines/rectangles/ellipses and renderer behavior are unchanged.

Head axial length is 24 + 3 times the frozen odd stroke width, capped at 75% of the gesture length. Half-width is half that axial length. Default5px head is39px rather than the previous20px ceiling;31px head is117px on sufficiently long arrows. Degenerate gestures remain dots; extremely short/thick gestures cannot show a full arrow silhouette with the current square brush. No filled/tapered renderer rewrite is included.

Checks: direction/separate branches in all directions/extreme coordinates; head growth across all widths1..31; frozen style and anchored preview; short/zero gestures; undo/redo/cancel; thick raster wings separate fromshaft/tail; eraser no false connections; posted default and wider head-first arrows. Full task build/test/check and safe desktop smoke run before promotion and origin/main push.

Results: task test 62 passed, task build passed, task check formatting/Clippy passed. Actual pure renderer output at widths 1/5/15/31 and horizontal/diagonal directions was reviewed offscreen; wider heads are distinct and remain at the press point. No actual mouse/keyboard input, focus or cursor was manipulated.

Desktop smoke was blocked at its pre-existing-overlay guard because the user's overlay is running. It was not closed or modified, and no desktop pass is claimed for this change. Unit model/raster coverage and the offscreen visual check validate the two requested geometry changes. Updated posted-message smoke assertions are ready for a later run with no existing overlay. Restart the running overlay to load the new executable.
