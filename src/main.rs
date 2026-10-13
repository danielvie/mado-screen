mod controls;
mod drawing;
mod input;
mod overlay;
mod strokes;
mod surface;

use controls::{Action, Hotkeys};
use input::{DragCapture, Event};
use overlay::Overlay;
use strokes::{Strokes, Tool};
use surface::Surface;
use windows::{
    Win32::UI::{HiDpi::*, WindowsAndMessaging::*},
    core::{Error, Result},
};

fn main() -> Result<()> {
    // Must precede windows and DPI-dependent metrics; see tasks/plan.md sources.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let _hotkeys = Hotkeys::register()?;
    // Locals drop in reverse declaration order, even with late initialization.
    // Keep the window alive only while its surface exists, including error exits.
    let mut surface;
    let overlay = Overlay::new()?;
    let (width, height) = overlay.size();
    surface = Surface::new(width, height)?;
    let mut drawing = false;
    let mut strokes = Strokes::default();
    // Declared after the window so capture/timer cleanup precedes window destruction.
    let mut tool_keys: Option<Hotkeys> = None;
    let mut drag: Option<DragCapture> = None;
    surface.present(overlay.hwnd(), drawing, strokes.paths())?;
    overlay.show()?;

    println!("PoC 3: empty canvas, fixed orange 5-pixel pen, line and rectangle.");
    println!("Ctrl+Alt+D: toggle drawing; Ctrl+Alt+C: clear; Ctrl+Alt+Q: quit.");
    println!("Drawing mode only: P pen, L line, R rectangle. Drag to preview; release to commit.");
    println!("Primary display: {width} x {height} physical pixels.");
    println!("Mode: click-through.");

    // GetMessage blocks when idle; the watchdog runs only during an active drag.
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        match result {
            -1 => return Err(Error::from_thread()),
            0 => break,
            _ => {}
        }
        if message.message == WM_HOTKEY {
            match controls::action(message.wParam.0) {
                Some(Action::Toggle) => {
                    if !drawing {
                        match Hotkeys::drawing_tools() {
                            Ok(keys) => tool_keys = Some(keys),
                            Err(error) => {
                                eprintln!(
                                    "Cannot enter drawing mode: {error}. Ink preserved; still click-through."
                                );
                                continue;
                            }
                        }
                    } else {
                        drop(tool_keys.take());
                    }
                    strokes.cancel();
                    drop(drag.take());
                    drawing = !drawing;
                    overlay.set_capture(drawing)?;
                    surface.present(overlay.hwnd(), drawing, strokes.paths())?;
                    println!(
                        "Mode: {}.",
                        if drawing {
                            "drawing; background alpha 1/255"
                        } else {
                            "click-through"
                        }
                    );
                    if drawing {
                        println!(
                            "Tool: {:?}. P/L/R select tools instead of typing; other keys retain focus.",
                            strokes.tool()
                        );
                    }
                }
                Some(action @ (Action::Pen | Action::Line | Action::Rectangle)) if drawing => {
                    let tool = match action {
                        Action::Pen => Tool::Pen,
                        Action::Line => Tool::Line,
                        Action::Rectangle => Tool::Rectangle,
                        _ => unreachable!(),
                    };
                    if strokes.tool() != tool {
                        let changed = strokes.select(tool);
                        drop(drag.take());
                        if changed {
                            surface.present(overlay.hwnd(), drawing, strokes.paths())?;
                        }
                        println!("Tool: {tool:?}.");
                    }
                }
                Some(Action::Clear) => {
                    strokes.finish();
                    drop(drag.take());
                    strokes.clear();
                    surface.present(overlay.hwnd(), drawing, strokes.paths())?;
                    println!("Strokes cleared.");
                }
                Some(Action::Quit) => break,
                // A selection queued before disabling drawing must not change the tool.
                Some(Action::Pen | Action::Line | Action::Rectangle) | None => {}
            }
        } else if let Some(event) = input::event(&message, overlay.hwnd()) {
            if handle_input(event, drawing, &mut strokes, &mut drag, overlay.hwnd())? {
                surface.present(overlay.hwnd(), drawing, strokes.paths())?;
            }
        } else {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    println!("Overlay closed.");
    Ok(())
}

fn handle_input(
    event: Event,
    drawing: bool,
    strokes: &mut Strokes,
    drag: &mut Option<DragCapture>,
    hwnd: windows::Win32::Foundation::HWND,
) -> Result<bool> {
    match event {
        Event::Down(point) if drawing => {
            strokes.finish();
            drop(drag.take());
            *drag = Some(DragCapture::begin(hwnd)?);
            strokes.begin(point);
            Ok(true)
        }
        Event::Move { point, left_down } if drawing && strokes.is_active() && left_down => {
            Ok(strokes.append(point))
        }
        Event::Up(point) => {
            let changed = strokes.append(point);
            strokes.finish();
            drop(drag.take());
            Ok(changed)
        }
        Event::Move {
            left_down: false, ..
        } => {
            strokes.finish();
            drop(drag.take());
            Ok(false)
        }
        Event::Cancel => {
            let changed = strokes.cancel();
            drop(drag.take());
            Ok(changed)
        }
        Event::CheckRelease if drag.as_ref().is_some_and(DragCapture::lost_or_released) => {
            // Release outside the window commits the last preview; lost capture cancels it.
            let changed = if drag.as_ref().is_some_and(DragCapture::has_capture) {
                strokes.finish();
                false
            } else {
                strokes.cancel()
            };
            drop(drag.take());
            Ok(changed)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use windows::Win32::Foundation::HWND;

    #[test]
    fn queued_events_ignore_hover_and_only_redraw_changed_points() {
        let mut strokes = Strokes::default();
        let route = |event, drawing, strokes: &mut Strokes| {
            handle_input(event, drawing, strokes, &mut None, HWND::default()).unwrap()
        };
        let movement = |point, left_down| Event::Move { point, left_down };
        assert!(!route(Event::Down((1, 2)), false, &mut strokes));
        assert!(!route(movement((3, 4), true), true, &mut strokes));
        assert!(!route(Event::Up((5, 6)), true, &mut strokes));
        assert!(!route(Event::CheckRelease, true, &mut strokes));
        assert!(strokes.paths().is_empty());
        strokes.begin((1, 2));
        assert!(!route(movement((1, 2), true), true, &mut strokes));
        assert!(route(movement((3, 4), true), true, &mut strokes));
        assert!(route(Event::Up((5, 6)), true, &mut strokes));
        assert!(!strokes.is_active());
        assert_eq!(strokes.paths(), &[vec![(1, 2), (3, 4), (5, 6)]]);
        assert!(!route(Event::Up((7, 8)), true, &mut strokes));

        for ending in [Event::Cancel, movement((9, 10), false)] {
            strokes.begin((7, 8));
            assert!(!route(ending, true, &mut strokes));
            assert!(!strokes.is_active());
            assert!(!route(movement((9, 10), true), true, &mut strokes));
            assert_eq!(strokes.paths().last().unwrap(), &[(7, 8)]);
        }
        strokes.clear();
        assert!(!route(movement((11, 12), true), true, &mut strokes));
        assert!(strokes.paths().is_empty());
    }

    #[test]
    fn shape_release_commits_but_cancel_removes_the_preview() {
        for tool in [Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((10, 10));
            strokes.append((20, 20));
            assert!(
                handle_input(
                    Event::Up((30, 30)),
                    true,
                    &mut strokes,
                    &mut None,
                    HWND::default()
                )
                .unwrap()
            );
            assert!(!strokes.is_active());
            let committed = strokes.paths().to_vec();
            strokes.begin((40, 40));
            strokes.append((50, 50));
            assert!(
                handle_input(
                    Event::Cancel,
                    true,
                    &mut strokes,
                    &mut None,
                    HWND::default()
                )
                .unwrap()
            );
            assert_eq!(strokes.paths(), committed);
            assert!(
                !handle_input(
                    Event::Up((60, 60)),
                    true,
                    &mut strokes,
                    &mut None,
                    HWND::default()
                )
                .unwrap()
            );
        }
    }

    #[test]
    fn shape_preview_replacement_and_cancellation_leave_no_pixels() {
        for tool in [Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((20, 20));
            strokes.append((50, 50));
            let mut pixels = vec![0; 100 * 100];
            let old_pixel = if tool == Tool::Line {
                35 * 100 + 35
            } else {
                35 * 100 + 50
            };
            drawing::render(&mut pixels, 100, true, strokes.paths());
            assert_eq!(pixels[old_pixel], 0xfff97316);
            let end = if tool == Tool::Line {
                (70, 20)
            } else {
                (70, 70)
            };
            strokes.append(end);
            drawing::render(&mut pixels, 100, true, strokes.paths());
            assert_eq!(pixels[old_pixel], 0x01000000);
            assert!(strokes.cancel());
            drawing::render(&mut pixels, 100, true, strokes.paths());
            assert!(pixels.iter().all(|&pixel| pixel == 0x01000000));
        }
    }

    #[test]
    fn drag_drops_before_window_and_surface_on_quit_and_error() {
        struct Resource<'a>(&'static str, &'a RefCell<Vec<&'static str>>);
        impl Drop for Resource<'_> {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }

        for fail in [false, true] {
            let drops = RefCell::new(Vec::new());
            let result = (|| -> std::result::Result<(), ()> {
                let surface;
                let overlay = Resource("overlay", &drops);
                surface = Resource("surface", &drops);
                let tool_keys = Resource("tool keys", &drops);
                let mut drag = Some(Resource("drag", &drops));
                // Replacing a drag must release the old guard first.
                drop(drag.take());
                drag = Some(Resource("drag", &drops));
                let _ = (&overlay, &surface, &tool_keys, &drag);
                if fail {
                    return Err(());
                }
                Ok(())
            })();
            assert_eq!(result.is_err(), fail);
            assert_eq!(
                *drops.borrow(),
                ["drag", "drag", "tool keys", "overlay", "surface"]
            );
        }
    }
}
