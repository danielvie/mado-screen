mod controls;
mod drawing;
mod input;
mod overlay;
mod picker;
mod strokes;
mod surface;
mod trail;
mod ui;

use controls::{Action, Hotkeys};
use input::{DragCapture, Event, ExpiryTimer};
use overlay::Overlay;
use std::time::{Duration, Instant};
use strokes::{Strokes, Tool};
use surface::Surface;
use trail::Trail;
use ui::{DesktopUi, UiEvent};
use windows::{
    Win32::{
        Foundation::HWND,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::{Error, Result},
};

// Published stages remain usable; later checkpoints expose advanced tools and UI.
const STAGE: u8 = 3;

fn main() -> Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let _hotkeys = Hotkeys::register()?;
    // Reverse drop order: drag, drawing keys, UI, window, surface, permanent keys.
    let mut surface;
    let overlay = Overlay::new()?;
    let (width, height) = overlay.size();
    surface = Surface::new(width, height)?;
    let mut drawing = false;
    let mut strokes = Strokes::default();
    let mut trail = Trail::default();
    let clock = Instant::now();
    surface.present(
        overlay.hwnd(),
        drawing,
        strokes.annotations().iter().chain(trail.annotations()),
    )?;
    overlay.show()?;
    // The canvas spans the virtual desktop, but the indicator stays on the primary screen.
    let ui = DesktopUi::new(
        overlay.hwnd(),
        unsafe { GetSystemMetrics(SM_CXSCREEN) },
        unsafe { GetSystemMetrics(SM_CYSCREEN) },
    )?;
    let mut tool_keys: Option<Hotkeys> = None;
    let mut drag: Option<DragCapture> = None;
    let mut expiry: Option<ExpiryTimer> = None;
    println!("PoC 6: stage {STAGE}, temporary pen and arrow direction toggles.");
    println!("Ctrl+Alt+D: toggle; Ctrl+Alt+C: clear; Ctrl+Alt+Q: quit.");
    println!("Drawing only: P selects pen; repeat P toggles temporary ink (2 seconds).");
    println!("L/R tools, Ctrl+Z/Y undo/redo, [/] thickness, 1..5 colors.");
    if STAGE >= 2 {
        println!(
            "Drawing only: A selects arrow; repeat A toggles head-first/tail-first. O ellipse, E eraser."
        );
    }
    println!(
        "Drawing only: C opens the disk picker at the mouse. Apply color or cancel with Escape."
    );
    let (left, top) = overlay.origin();
    println!("Virtual desktop: origin ({left}, {top}), {width} x {height} physical pixels.");
    println!("Restart after changing monitors, their layout or display scaling.");
    println!("Mode: click-through.");
    let mut message = MSG::default();
    loop {
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(Error::from_thread()),
            0 => break,
            _ => {}
        }
        // Expiry runs before picker routing and independent of drawing/click-through mode.
        if trail.expire(clock.elapsed()) {
            surface.present(
                overlay.hwnd(),
                drawing,
                strokes.annotations().iter().chain(trail.annotations()),
            )?;
        }
        if trail.is_empty() {
            drop(expiry.take());
        }
        if let Some(event) = ui.event(&message) {
            if let UiEvent::Color(color) = event {
                let changed = strokes.cancel();
                trail.finish();
                drop(drag.take());
                strokes.set_color(color);
                if changed {
                    surface.present(
                        overlay.hwnd(),
                        drawing,
                        strokes.annotations().iter().chain(trail.annotations()),
                    )?;
                }
            }
            restore_drawing_keys(
                &mut drawing,
                &mut tool_keys,
                &overlay,
                &mut surface,
                (&strokes, &trail),
                &ui,
            )?;
            ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
            continue;
        }
        if ui.handle_message(&message) {
            continue;
        }
        if message.message == WM_HOTKEY {
            let Some(action) = controls::action(message.wParam.0) else {
                continue;
            };
            match action {
                Action::Toggle => {
                    ui.close_picker();
                    if !drawing {
                        match Hotkeys::drawing_keys(true, true) {
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
                    trail.finish();
                    drop(drag.take());
                    drawing = !drawing;
                    overlay.set_capture(drawing)?;
                    surface.present(
                        overlay.hwnd(),
                        drawing,
                        strokes.annotations().iter().chain(trail.annotations()),
                    )?;
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
                    ui.close_picker();
                    strokes.cancel();
                    drop(drag.take());
                    strokes.clear();
                    trail.clear();
                    drop(expiry.take());
                    surface.present(
                        overlay.hwnd(),
                        drawing,
                        strokes.annotations().iter().chain(trail.annotations()),
                    )?;
                    println!("Strokes cleared. Clear is undoable.");
                }
                Action::Quit => break,
                Action::Picker if drawing && !ui.is_picker_open() => {
                    let changed = strokes.cancel();
                    trail.finish();
                    drop(drag.take());
                    drop(tool_keys.take());
                    if changed {
                        surface.present(
                            overlay.hwnd(),
                            drawing,
                            strokes.annotations().iter().chain(trail.annotations()),
                        )?;
                    }
                    if let Err(error) = ui.open_picker(strokes.style().color) {
                        eprintln!("Cannot open color picker: {error}. Ink preserved.");
                    }
                }
                action if drawing && !ui.is_picker_open() => {
                    // Identical style selections leave a temporary gesture intact too.
                    if interrupts_trail(&action, &strokes) {
                        trail.finish();
                    }
                    if apply_edit(action, &mut strokes, &mut drag) {
                        surface.present(
                            overlay.hwnd(),
                            drawing,
                            strokes.annotations().iter().chain(trail.annotations()),
                        )?;
                    }
                    println!(
                        "Tool: {}. Color: #{:06X}; width: {}.",
                        strokes.tool_label(),
                        strokes.style().color & 0xffffff,
                        strokes.style().thickness
                    );
                }
                _ => {}
            }
            restore_drawing_keys(
                &mut drawing,
                &mut tool_keys,
                &overlay,
                &mut surface,
                (&strokes, &trail),
                &ui,
            )?;
            ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
        } else if let Some(event) = input::event(&message, overlay.hwnd()) {
            if ui.is_picker_open() {
                // An outside canvas press dismisses the popup, never paints a stroke.
                if matches!(event, Event::Down(_)) {
                    ui.close_picker();
                    restore_drawing_keys(
                        &mut drawing,
                        &mut tool_keys,
                        &overlay,
                        &mut surface,
                        (&strokes, &trail),
                        &ui,
                    )?;
                    ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
                }
                continue;
            }
            if handle_canvas_input(
                event,
                drawing,
                (&mut strokes, &mut trail),
                &mut drag,
                overlay.hwnd(),
                clock.elapsed(),
            )? {
                surface.present(
                    overlay.hwnd(),
                    drawing,
                    strokes.annotations().iter().chain(trail.annotations()),
                )?;
            }
            if !trail.is_empty() && expiry.is_none() {
                expiry = Some(ExpiryTimer::begin(overlay.hwnd())?);
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

fn restore_drawing_keys(
    drawing: &mut bool,
    keys: &mut Option<Hotkeys>,
    overlay: &Overlay,
    surface: &mut Surface,
    scene: (&Strokes, &Trail),
    ui: &DesktopUi,
) -> Result<()> {
    let (strokes, trail) = scene;
    if *drawing && keys.is_none() && !ui.is_picker_open() {
        match Hotkeys::drawing_keys(true, true) {
            Ok(guard) => *keys = Some(guard),
            Err(error) => {
                eprintln!(
                    "Cannot resume drawing: {error}. Ink preserved; returning to click-through."
                );
                *drawing = false;
                overlay.set_capture(false)?;
                surface.present(
                    overlay.hwnd(),
                    false,
                    strokes.annotations().iter().chain(trail.annotations()),
                )?;
                println!("Mode: click-through.");
            }
        }
    }
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
            match tool {
                Tool::Pen => {
                    drop(drag.take());
                    return strokes.toggle_temporary_pen();
                }
                Tool::Arrow => {
                    drop(drag.take());
                    return strokes.toggle_arrow_direction();
                }
                _ => return false,
            }
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

fn interrupts_trail(action: &Action, strokes: &Strokes) -> bool {
    match action {
        Action::Blue => strokes.style().color != 0xff3b82f6,
        Action::Orange => strokes.style().color != 0xfff97316,
        Action::Red => strokes.style().color != 0xffef4444,
        Action::Green => strokes.style().color != 0xff22c55e,
        Action::White => strokes.style().color != 0xffffffff,
        Action::Thinner => strokes.style().thickness > 1,
        Action::Thicker => strokes.style().thickness < 31,
        _ => true,
    }
}

fn handle_canvas_input(
    event: Event,
    drawing: bool,
    scene: (&mut Strokes, &mut Trail),
    drag: &mut Option<DragCapture>,
    hwnd: HWND,
    now: Duration,
) -> Result<bool> {
    let (strokes, trail) = scene;
    if event == Event::Expire {
        return Ok(trail.expire(now));
    }
    if (drawing && strokes.tool() == Tool::Pen && strokes.temporary_pen()) || trail.is_active() {
        match event {
            Event::Down(point) if drawing => {
                strokes.cancel();
                trail.finish();
                drop(drag.take());
                *drag = Some(DragCapture::begin(hwnd)?);
                return Ok(trail.begin(point, strokes.style(), now));
            }
            Event::Move {
                point,
                left_down: true,
            } => return Ok(trail.append(point, now)),
            Event::Up(point) => {
                let changed = trail.append(point, now);
                trail.finish();
                drop(drag.take());
                return Ok(changed);
            }
            Event::Move {
                left_down: false, ..
            }
            | Event::Cancel => {
                trail.finish();
                drop(drag.take());
                return Ok(false);
            }
            Event::CheckRelease if drag.as_ref().is_some_and(DragCapture::lost_or_released) => {
                trail.finish();
                drop(drag.take());
                return Ok(false);
            }
            _ => return Ok(false),
        }
    }
    handle_input(event, drawing, strokes, drag, hwnd)
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
    fn repeated_tool_keys_toggle_only_the_selected_variant() {
        let mut strokes = Strokes::default();
        apply_edit(Action::Line, &mut strokes, &mut None);
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Pen");
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Temporary pen");
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Pen");
        apply_edit(Action::Arrow, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Arrow head-first");
        apply_edit(Action::Arrow, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Arrow tail-first");
        strokes.begin((10, 10));
        strokes.append((60, 10));
        assert!(apply_edit(Action::Arrow, &mut strokes, &mut None));
        assert_eq!(strokes.tool_label(), "Arrow head-first");
        assert!(strokes.annotations().is_empty());
    }

    #[test]
    fn temporary_input_and_expiry_preserve_permanent_redo() {
        let mut strokes = Strokes::default();
        strokes.begin((5, 5));
        strokes.finish();
        let scene = strokes.annotations().to_vec();
        assert!(strokes.undo());
        strokes.toggle_temporary_pen();
        let mut trail = Trail::default();
        trail.begin((20, 20), strokes.style(), Duration::ZERO);
        assert!(
            handle_canvas_input(
                Event::Move {
                    point: (30, 20),
                    left_down: true
                },
                true,
                (&mut strokes, &mut trail),
                &mut None,
                HWND::default(),
                Duration::from_millis(500)
            )
            .unwrap()
        );
        handle_canvas_input(
            Event::Up((40, 20)),
            true,
            (&mut strokes, &mut trail),
            &mut None,
            HWND::default(),
            Duration::from_millis(600),
        )
        .unwrap();
        assert!(!trail.is_active());
        assert!(strokes.annotations().is_empty());
        handle_canvas_input(
            Event::Expire,
            false,
            (&mut strokes, &mut trail),
            &mut None,
            HWND::default(),
            Duration::from_secs(3),
        )
        .unwrap();
        assert!(trail.is_empty());
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), scene);
        strokes.clear();
        trail.clear();
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), scene);
        assert!(trail.is_empty());
    }

    #[test]
    fn expired_temporary_pixels_reveal_permanent_ink() {
        let mut strokes = Strokes::default();
        strokes.begin((10, 10));
        strokes.finish();
        let mut trail = Trail::default();
        trail.begin(
            (10, 10),
            strokes::Style {
                color: 0xff3b82f6,
                thickness: 5,
            },
            Duration::ZERO,
        );
        let mut pixels = vec![0; 40 * 40];
        drawing::render(
            &mut pixels,
            40,
            false,
            strokes.annotations().iter().chain(trail.annotations()),
        );
        assert_eq!(pixels[10 * 40 + 10], 0xff3b82f6);
        assert!(trail.expire(Duration::from_secs(2)));
        drawing::render(
            &mut pixels,
            40,
            false,
            strokes.annotations().iter().chain(trail.annotations()),
        );
        assert_eq!(pixels[10 * 40 + 10], 0xfff97316);
        assert_eq!(pixels[0], 0);
    }

    #[test]
    fn temporary_cancellation_keeps_ink_but_stops_input() {
        let mut strokes = Strokes::default();
        strokes.toggle_temporary_pen();
        let mut trail = Trail::default();
        trail.begin((10, 10), strokes.style(), Duration::ZERO);
        handle_canvas_input(
            Event::Cancel,
            true,
            (&mut strokes, &mut trail),
            &mut None,
            HWND::default(),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(!trail.is_active());
        assert!(!trail.is_empty());
        assert!(
            !handle_canvas_input(
                Event::Move {
                    point: (20, 20),
                    left_down: true
                },
                true,
                (&mut strokes, &mut trail),
                &mut None,
                HWND::default(),
                Duration::from_secs(1)
            )
            .unwrap()
        );
        assert!(!interrupts_trail(&Action::Orange, &strokes));
        assert!(interrupts_trail(&Action::Pen, &strokes));
        assert!(trail.expire(Duration::from_secs(2)));
    }

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
    fn style_actions_settle_gestures_but_identical_choices_do_not() {
        let mut strokes = Strokes::default();
        strokes.begin((10, 10));
        assert!(!apply_edit(Action::Orange, &mut strokes, &mut None));
        assert!(strokes.is_active());
        assert!(!apply_edit(Action::Blue, &mut strokes, &mut None));
        assert!(!strokes.is_active());
        assert_eq!(strokes.annotations()[0].style.color, 0xfff97316);
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        strokes.select(Tool::Line);
        strokes.begin((20, 20));
        strokes.append((40, 40));
        assert!(apply_edit(Action::Thicker, &mut strokes, &mut None));
        assert!(!strokes.is_active());
        assert!(strokes.annotations().is_empty());
        assert_eq!(strokes.style().thickness, 7);
    }

    #[test]
    fn cross_monitor_paths_share_styles_and_one_history_action() {
        // A left-hand screen occupies client x=0..100 and the primary x=100..200.
        // The screen origin is not added to these canvas-local input coordinates.
        for tool in [
            Tool::Pen,
            Tool::Line,
            Tool::Rectangle,
            Tool::Arrow,
            Tool::Ellipse,
        ] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.set_color(0xff3b82f6);
            strokes.set_thickness(7);
            strokes.begin((80, 30));
            assert!(
                handle_input(
                    Event::Move {
                        point: (100, 40),
                        left_down: true
                    },
                    true,
                    &mut strokes,
                    &mut None,
                    HWND::default()
                )
                .unwrap()
            );
            assert!(
                handle_input(
                    Event::Up((120, 50)),
                    true,
                    &mut strokes,
                    &mut None,
                    HWND::default()
                )
                .unwrap()
            );
            let scene = strokes.annotations().to_vec();
            assert_eq!(scene.len(), 1);
            assert_eq!(scene[0].style.color, 0xff3b82f6);
            assert_eq!(scene[0].style.thickness, 7);
            assert!(scene[0].paths.iter().flatten().any(|point| point.0 < 100));
            assert!(scene[0].paths.iter().flatten().any(|point| point.0 > 100));
            assert!(apply_edit(Action::Undo, &mut strokes, &mut None));
            assert!(strokes.annotations().is_empty());
            assert!(apply_edit(Action::Redo, &mut strokes, &mut None));
            assert_eq!(strokes.annotations(), scene);
            strokes.clear();
            assert!(strokes.annotations().is_empty());
            assert!(apply_edit(Action::Undo, &mut strokes, &mut None));
            assert_eq!(strokes.annotations(), scene);
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
