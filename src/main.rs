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
    Win32::{
        Foundation::HWND,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::{Error, Result},
};

// Published stages remain usable; later checkpoints expose advanced tools and UI.
const STAGE: u8 = 2;

fn main() -> Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let _hotkeys = Hotkeys::register()?;
    // Reverse drop order: drag, drawing keys, window, surface, permanent keys.
    let mut surface;
    let overlay = Overlay::new()?;
    let (width, height) = overlay.size();
    surface = Surface::new(width, height)?;
    let mut drawing = false;
    let mut strokes = Strokes::default();
    let mut tool_keys: Option<Hotkeys> = None;
    let mut drag: Option<DragCapture> = None;
    surface.present(overlay.hwnd(), drawing, strokes.annotations())?;
    overlay.show()?;
    println!("PoC 4: stage {STAGE}, empty canvas, styled annotations and history.");
    println!("Ctrl+Alt+D: toggle; Ctrl+Alt+C: clear; Ctrl+Alt+Q: quit.");
    println!("Drawing only: P/L/R tools, Ctrl+Z/Y undo/redo, [/] thickness, 1..5 colors.");
    if STAGE >= 2 {
        println!("Drawing only: A arrow, O ellipse, E whole-annotation eraser.");
    }
    println!("Primary display: {width} x {height} physical pixels.");
    println!("Mode: click-through.");
    let mut message = MSG::default();
    loop {
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(Error::from_thread()),
            0 => break,
            _ => {}
        }
        if message.message == WM_HOTKEY {
            let Some(action) = controls::action(message.wParam.0) else {
                continue;
            };
            match action {
                Action::Toggle => {
                    if !drawing {
                        match Hotkeys::drawing_keys(STAGE >= 2, false) {
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
                    surface.present(overlay.hwnd(), drawing, strokes.annotations())?;
                    println!(
                        "Mode: {}.",
                        if drawing {
                            "drawing; background alpha 1/255"
                        } else {
                            "click-through"
                        }
                    );
                }
                Action::Clear => {
                    strokes.cancel();
                    drop(drag.take());
                    strokes.clear();
                    surface.present(overlay.hwnd(), drawing, strokes.annotations())?;
                    println!("Strokes cleared. Clear is undoable.");
                }
                Action::Quit => break,
                action if drawing => {
                    if apply_edit(action, &mut strokes, &mut drag) {
                        surface.present(overlay.hwnd(), drawing, strokes.annotations())?;
                    }
                    println!(
                        "Tool: {:?}. Color: #{:06X}; width: {}.",
                        strokes.tool(),
                        strokes.style().color & 0xffffff,
                        strokes.style().thickness
                    );
                }
                _ => {}
            }
        } else if let Some(event) = input::event(&message, overlay.hwnd()) {
            if handle_input(event, drawing, &mut strokes, &mut drag, overlay.hwnd())? {
                surface.present(overlay.hwnd(), drawing, strokes.annotations())?;
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

fn apply_edit(action: Action, strokes: &mut Strokes, drag: &mut Option<DragCapture>) -> bool {
    let tool = match action {
        Action::Pen => Some(Tool::Pen),
        Action::Line => Some(Tool::Line),
        Action::Rectangle => Some(Tool::Rectangle),
        Action::Arrow if STAGE >= 2 => Some(Tool::Arrow),
        Action::Ellipse if STAGE >= 2 => Some(Tool::Ellipse),
        Action::Eraser if STAGE >= 2 => Some(Tool::Eraser),
        _ => None,
    };
    if let Some(tool) = tool {
        if strokes.tool() == tool {
            return false;
        }
        drop(drag.take());
        return strokes.select(tool);
    }
    let color = match action {
        Action::Blue => Some(0xff3b82f6),
        Action::Orange => Some(0xfff97316),
        Action::Red => Some(0xffef4444),
        Action::Green => Some(0xff22c55e),
        Action::White => Some(0xffffffff),
        _ => None,
    };
    if let Some(color) = color {
        if strokes.style().color == color {
            return false;
        }
        drop(drag.take());
        let changed = strokes.cancel();
        strokes.set_color(color);
        return changed;
    }
    let thickness = match action {
        Action::Thinner => Some(strokes.style().thickness.saturating_sub(2).max(1)),
        Action::Thicker => Some((strokes.style().thickness + 2).min(31)),
        _ => None,
    };
    if let Some(thickness) = thickness {
        if strokes.style().thickness == thickness {
            return false;
        }
        drop(drag.take());
        let changed = strokes.cancel();
        strokes.set_thickness(thickness);
        return changed;
    }
    match action {
        Action::Undo | Action::Redo => {
            drop(drag.take());
            match action {
                Action::Undo => strokes.undo(),
                Action::Redo => strokes.redo(),
                _ => unreachable!(),
            }
        }
        _ => false,
    }
}

fn handle_input(
    event: Event,
    drawing: bool,
    strokes: &mut Strokes,
    drag: &mut Option<DragCapture>,
    hwnd: HWND,
) -> Result<bool> {
    match event {
        Event::Down(point) if drawing => {
            strokes.cancel();
            drop(drag.take());
            *drag = Some(DragCapture::begin(hwnd)?);
            Ok(strokes.begin(point))
        }
        Event::Move {
            point,
            left_down: true,
        } if drawing && strokes.is_active() => Ok(strokes.append(point)),
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
        assert!(strokes.annotations().is_empty());
        strokes.begin((1, 2));
        assert!(!route(movement((1, 2), true), true, &mut strokes));
        assert!(route(movement((3, 4), true), true, &mut strokes));
        assert!(route(Event::Up((5, 6)), true, &mut strokes));
        assert!(!strokes.is_active());
        assert_eq!(strokes.annotations()[0].paths[0], [(1, 2), (3, 4), (5, 6)]);
        assert!(!route(Event::Up((7, 8)), true, &mut strokes));
        for ending in [Event::Cancel, movement((9, 10), false)] {
            strokes.begin((7, 8));
            assert!(!route(ending, true, &mut strokes));
            assert!(!strokes.is_active());
            assert!(!route(movement((9, 10), true), true, &mut strokes));
            assert_eq!(strokes.annotations().last().unwrap().paths[0], [(7, 8)]);
        }
        strokes.clear();
        assert!(!route(movement((11, 12), true), true, &mut strokes));
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
            let committed = strokes.annotations().to_vec();
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
            assert_eq!(strokes.annotations(), committed);
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
            drawing::render(&mut pixels, 100, true, strokes.annotations());
            assert_eq!(pixels[old_pixel], 0xfff97316);
            strokes.append(if tool == Tool::Line {
                (70, 20)
            } else {
                (70, 70)
            });
            drawing::render(&mut pixels, 100, true, strokes.annotations());
            assert_eq!(pixels[old_pixel], 0x01000000);
            assert!(strokes.cancel());
            drawing::render(&mut pixels, 100, true, strokes.annotations());
            assert!(pixels.iter().all(|&pixel| pixel == 0x01000000));
        }
    }

    #[test]
    fn edit_actions_preserve_styles_and_restore_history() {
        let mut strokes = Strokes::default();
        strokes.begin((10, 10));
        strokes.finish();
        apply_edit(Action::Blue, &mut strokes, &mut None);
        apply_edit(Action::Thicker, &mut strokes, &mut None);
        strokes.begin((30, 30));
        strokes.finish();
        let all = strokes.annotations().to_vec();
        assert_eq!(all[0].style.color, 0xfff97316);
        assert_eq!(all[1].style.color, 0xff3b82f6);
        assert_eq!(all[1].style.thickness, 7);
        assert!(apply_edit(Action::Undo, &mut strokes, &mut None));
        assert_eq!(strokes.annotations().len(), 1);
        assert!(apply_edit(Action::Redo, &mut strokes, &mut None));
        assert_eq!(strokes.annotations(), all);
        strokes.clear();
        assert!(apply_edit(Action::Undo, &mut strokes, &mut None));
        assert_eq!(strokes.annotations(), all);
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
