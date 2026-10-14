#![cfg_attr(not(test), windows_subsystem = "windows")]

mod bezier;
mod config;
mod controls;
mod drawing;
mod input;
mod overlay;
mod strokes;
mod surface;
mod trail;
mod tray;
mod ui;

use config::Favorites;
use controls::{Action, Hotkeys};
use input::{DragCapture, Event, ExpiryTimer, MouseWheel};
use overlay::Overlay;
use std::{
    io::{IsTerminal, Write},
    path::Path,
    time::{Duration, Instant},
};
use strokes::{Strokes, Tool};
use surface::Surface;
use trail::Trail;
use tray::{Tray, TrayEvent};
use ui::{DesktopUi, MenuTool, UiEvent};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Console::{
            ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
            GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
        },
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::{Error, Result},
};

// Published stages remain usable; later checkpoints expose advanced tools and UI.
const STAGE: u8 = 3;

const STARTUP_HELP: &str = "\
Mado Screen
Starts in click-through mode. Press F4 or Ctrl+Alt+D to draw.
In drawing mode, drag with the primary mouse button to annotate.

Global shortcuts
  F4           Switch between drawing and click-through
  Ctrl+Alt+D   Switch between drawing and click-through
  Ctrl+Alt+C   Clear all ink, undoable for permanent ink
  Ctrl+Alt+Q   Quit

Drawing shortcuts
  Escape       Return to click-through so underlying apps receive input
  P            Pen; press again to toggle temporary ink (fades in 1 second)
  L            Straight line
  R            Rectangle outline
  A            Arrow; press again to switch head-first / tail-first
  O            Ellipse outline
  E            Erase a whole annotation
  C            Clear all ink, undoable for permanent ink
  X            Swap active and reserve colors
  Ctrl+Z       Undo
  Ctrl+Y       Redo
  [ / ]        Thinner / thicker brush (1 to 31 pixels)
  Mouse wheel  Scroll up for thicker ink, down for thinner ink
  1 / 2 / 3    Blue / orange / red
  4 / 5        Green / white

Floating menu
  Hover over the top-right tool icon to open the dark menu.
  Click tool icons, color slots or the width slider; letters show shortcuts.
  Click Drawing / Click-through to switch canvas input without a shortcut.
  Drag a palette color onto the active or reserve square; click the reserve to swap.
  Drag the tool icon or its grip to move the menu.
  Click the tray icon to switch modes; right-click it to hide the menu, clear or quit.

Default brush: orange, 5 pixels; reserve blue. Restart after changing the monitor layout.
Favorite colors are saved in ~/.mado/mado-screen/config.ini.
";

fn startup_help(styled: bool) -> String {
    if !styled {
        return STARTUP_HELP.to_owned();
    }
    const RESET: &str = "\x1b[0m";
    // Pi dark palette: mdCode/accent, mdLink and mdHeading, resolved from OKHSL.
    const SHORTCUT: &str = "\x1b[38;2;167;152;215m";
    const COMMAND: &str = "\x1b[38;2;105;173;208m";
    const TITLE: &str = "\x1b[1;38;2;205;154;34m";
    let mut help = String::new();
    for line in STARTUP_HELP.lines() {
        if matches!(
            line,
            "Mado Screen" | "Global shortcuts" | "Drawing shortcuts" | "Floating menu"
        ) {
            help.push_str(&format!("{TITLE}{line}{RESET}"));
        } else if let Some((key, description)) = line
            .strip_prefix("  ")
            .and_then(|row| row.split_once("   "))
        {
            // ponytail: fixed guide rows use padded columns and punctuation before
            // explanations. Use explicit spans if the guide's layout becomes dynamic.
            let command_start = description.len() - description.trim_start().len();
            let command_end = description
                .find([';', ',', '('])
                .unwrap_or(description.len());
            let command_end = description[..command_end].trim_end().len();
            help.push_str(&format!(
                "  {SHORTCUT}{key}{RESET}   {}{COMMAND}{}{RESET}{}",
                &description[..command_start],
                &description[command_start..command_end],
                &description[command_end..],
            ));
        } else {
            let mut text = line.to_owned();
            for shortcut in ["F4", "Ctrl+Alt+D", "Drawing / Click-through"] {
                text = text.replace(shortcut, &format!("{SHORTCUT}{shortcut}{RESET}"));
            }
            for command in ["Hover", "Click", "Drag"] {
                text = text.replace(
                    &format!("  {command} "),
                    &format!("  {COMMAND}{command}{RESET} "),
                );
            }
            help.push_str(&text);
        }
        help.push('\n');
    }
    help
}

fn print_startup_help() {
    // VT styling requires console output mode; redirected/unsupported output stays plain.
    // https://learn.microsoft.com/en-us/windows/console/setconsolemode
    let console = if std::io::stdout().is_terminal() {
        (|| -> Result<_> {
            let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE)? };
            let mut mode = Default::default();
            unsafe {
                GetConsoleMode(handle, &mut mode)?;
                SetConsoleMode(
                    handle,
                    mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
                )?;
            }
            Ok((handle, mode))
        })()
        .ok()
    } else {
        None
    };
    println!("{}", startup_help(console.is_some()));
    if let Some((handle, mode)) = console {
        let _ = std::io::stdout().flush();
        unsafe {
            let _ = SetConsoleMode(handle, mode);
        }
    }
}

fn is_mode_toggle_action(action: &Action, drawing: bool) -> bool {
    *action == Action::Toggle || (*action == Action::Deactivate && drawing)
}

fn is_clear_action(action: &Action, drawing: bool) -> bool {
    *action == Action::Clear || (*action == Action::QuickClear && drawing)
}

fn clear_ink(
    strokes: &mut Strokes,
    trail: &mut Trail,
    drag: &mut Option<DragCapture>,
    expiry: &mut Option<ExpiryTimer>,
) {
    strokes.cancel();
    drop(drag.take());
    strokes.clear();
    trail.clear();
    drop(expiry.take());
}

fn main() -> Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let _hotkeys = Hotkeys::register()?;
    let _function_key = Hotkeys::function_toggle()
        .map_err(|error| eprintln!("{error} F4 is unavailable; use Ctrl+Alt+D."))
        .ok();
    // Reverse drop order: drag, drawing keys, tray, UI, window, surface, permanent keys.
    let mut surface;
    let overlay = Overlay::new()?;
    let (width, height) = overlay.size();
    surface = Surface::new(width, height)?;
    let mut drawing = false;
    let mut strokes = Strokes::default();
    let config_path = config::path();
    let mut saved = config_path.as_deref().map(config::load).unwrap_or_default();
    strokes.set_color(saved.active);
    let mut reserve = saved.reserve;
    let mut trail = Trail::default();
    let clock = Instant::now();
    surface.present_scene(
        overlay.hwnd(),
        drawing,
        strokes.annotations(),
        &trail,
        clock.elapsed(),
    )?;
    overlay.show()?;
    // Start the floating menu on the primary screen; grip dragging can move it later.
    let ui = DesktopUi::new(
        overlay.hwnd(),
        unsafe { GetSystemMetrics(SM_CXSCREEN) },
        unsafe { GetSystemMetrics(SM_CYSCREEN) },
    )?;
    ui.set_reserve(reserve)?;
    ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
    let tray = Tray::new(overlay.hwnd())?;
    let mut panel_visible = true;
    tray.update(drawing, strokes.style().color)?;
    let mut tool_keys: Option<(Hotkeys, MouseWheel)> = None;
    let mut drag: Option<DragCapture> = None;
    let mut expiry: Option<ExpiryTimer> = None;
    print_startup_help();
    let mut message = MSG::default();
    loop {
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(Error::from_thread()),
            0 => break,
            _ => {}
        }
        // Animate before UI routing, including click-through. Use one frame time.
        let now = clock.elapsed();
        let tick = input::event(&message, overlay.hwnd()) == Some(Event::Expire);
        if animate_trail(&mut trail, now, tick) {
            surface.present_scene(overlay.hwnd(), drawing, strokes.annotations(), &trail, now)?;
        }
        if trail.is_empty() {
            drop(expiry.take());
        }
        if tick {
            continue;
        }
        if let Some(event) = ui.event(&message) {
            let changed = match event {
                UiEvent::Color(color) => apply_ui_color(color, &mut strokes, &mut trail, &mut drag),
                UiEvent::Tool(tool) => apply_menu_tool(tool, &mut strokes, &mut trail, &mut drag),
                UiEvent::Width(width) => {
                    apply_menu_width(width, &mut strokes, &mut trail, &mut drag)
                }
                UiEvent::Toggle => {
                    toggle_drawing(
                        &mut drawing,
                        &mut tool_keys,
                        &overlay,
                        &mut surface,
                        (&mut strokes, &mut trail, now),
                        &mut drag,
                    )?;
                    false
                }
                UiEvent::Reserve(color) => {
                    reserve = color | 0xff000000;
                    false
                }
                UiEvent::Swap => swap_colors(&mut reserve, &mut strokes, &mut trail, &mut drag),
            };
            if changed {
                surface.present_scene(
                    overlay.hwnd(),
                    drawing,
                    strokes.annotations(),
                    &trail,
                    now,
                )?;
            }
            ui.set_reserve(reserve)?;
            ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
            tray.update(drawing, strokes.style().color)?;
            persist(
                &mut saved,
                strokes.style().color,
                reserve,
                config_path.as_deref(),
            );
            continue;
        }
        // Tray commands reuse the same transactions as Ctrl+Alt+D/C/Q.
        if let Some(event) = tray.event(&message) {
            match event {
                TrayEvent::Toggle => toggle_drawing(
                    &mut drawing,
                    &mut tool_keys,
                    &overlay,
                    &mut surface,
                    (&mut strokes, &mut trail, now),
                    &mut drag,
                )?,
                TrayEvent::Clear => {
                    clear_ink(&mut strokes, &mut trail, &mut drag, &mut expiry);
                    surface.present_scene(
                        overlay.hwnd(),
                        drawing,
                        strokes.annotations(),
                        &trail,
                        now,
                    )?;
                }
                TrayEvent::Quit => break,
                TrayEvent::Panel => {
                    panel_visible = !panel_visible;
                    ui.set_visible(panel_visible)?;
                    tray.set_panel_visible(panel_visible);
                }
            }
            ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
            tray.update(drawing, strokes.style().color)?;
            continue;
        }
        if message.message == WM_HOTKEY {
            let Some(action) = controls::action(message.wParam.0) else {
                continue;
            };
            match action {
                action if is_mode_toggle_action(&action, drawing) => {
                    toggle_drawing(
                        &mut drawing,
                        &mut tool_keys,
                        &overlay,
                        &mut surface,
                        (&mut strokes, &mut trail, now),
                        &mut drag,
                    )?;
                }
                action if is_clear_action(&action, drawing) => {
                    clear_ink(&mut strokes, &mut trail, &mut drag, &mut expiry);
                    surface.present_scene(
                        overlay.hwnd(),
                        drawing,
                        strokes.annotations(),
                        &trail,
                        now,
                    )?;
                }
                Action::Quit => break,
                Action::Swap if drawing => {
                    if swap_colors(&mut reserve, &mut strokes, &mut trail, &mut drag) {
                        surface.present_scene(
                            overlay.hwnd(),
                            drawing,
                            strokes.annotations(),
                            &trail,
                            now,
                        )?;
                    }
                }
                action if drawing => {
                    // Identical style selections leave a temporary gesture intact too.
                    if interrupts_trail(&action, &strokes) {
                        trail.finish();
                    }
                    if apply_edit(action, &mut strokes, &mut drag) {
                        surface.present_scene(
                            overlay.hwnd(),
                            drawing,
                            strokes.annotations(),
                            &trail,
                            now,
                        )?;
                    }
                }
                _ => {}
            }
            ui.set_reserve(reserve)?;
            ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
            tray.update(drawing, strokes.style().color)?;
            persist(
                &mut saved,
                strokes.style().color,
                reserve,
                config_path.as_deref(),
            );
        } else if let Some(event) = input::event(&message, overlay.hwnd()) {
            if handle_canvas_input(
                event,
                drawing,
                (&mut strokes, &mut trail),
                &mut drag,
                overlay.hwnd(),
                now,
            )? {
                surface.present_scene(
                    overlay.hwnd(),
                    drawing,
                    strokes.annotations(),
                    &trail,
                    now,
                )?;
            }
            if matches!(event, Event::Wheel(_)) {
                ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
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

    Ok(())
}

// The visible input toggle, Ctrl+Alt+D and drawing-only Escape use this transaction.
fn toggle_drawing(
    drawing: &mut bool,
    keys: &mut Option<(Hotkeys, MouseWheel)>,
    overlay: &Overlay,
    surface: &mut Surface,
    scene: (&mut Strokes, &mut Trail, Duration),
    drag: &mut Option<DragCapture>,
) -> Result<()> {
    let (strokes, trail, now) = scene;
    let entering = !*drawing;
    let pending = if entering {
        match Hotkeys::drawing_keys(true)
            .and_then(|keys| MouseWheel::begin(overlay.hwnd()).map(|wheel| (keys, wheel)))
        {
            Ok(keys) => Some(keys),
            Err(error) => {
                eprintln!(
                    "Cannot enter drawing mode: {error}. Ink preserved; still click-through."
                );
                return Ok(());
            }
        }
    } else {
        None
    };
    strokes.cancel();
    trail.finish();
    drop(drag.take());
    overlay.set_capture(entering)?;
    *keys = pending;
    *drawing = entering;
    surface.present_scene(overlay.hwnd(), entering, strokes.annotations(), trail, now)
}

// Palette changes settle active gestures before applying the new color.
fn apply_ui_color(
    color: u32,
    strokes: &mut Strokes,
    trail: &mut Trail,
    drag: &mut Option<DragCapture>,
) -> bool {
    if color == strokes.style().color {
        return false;
    }
    let changed = strokes.cancel();
    trail.finish();
    drop(drag.take());
    strokes.set_color(color);
    changed
}

// X and the menu's reserve square exchange the brush color with the reserve favorite.
fn swap_colors(
    reserve: &mut u32,
    strokes: &mut Strokes,
    trail: &mut Trail,
    drag: &mut Option<DragCapture>,
) -> bool {
    let active = strokes.style().color;
    let changed = apply_ui_color(*reserve, strokes, trail, drag);
    *reserve = active;
    changed
}

// Writes only on change, so idle loops and repeated selections never touch the disk.
fn persist(saved: &mut Favorites, active: u32, reserve: u32, path: Option<&Path>) {
    let current = Favorites { active, reserve };
    if *saved == current {
        return;
    }
    *saved = current;
    if let Some(path) = path
        && let Err(error) = config::save(path, current)
    {
        eprintln!("Could not save {}: {error}", path.display());
    }
}

// Menu choices are exact selections, unlike repeated P/A hotkey actions.
fn apply_menu_tool(
    choice: MenuTool,
    strokes: &mut Strokes,
    trail: &mut Trail,
    drag: &mut Option<DragCapture>,
) -> bool {
    let tool = match choice {
        MenuTool::Pen | MenuTool::TemporaryPen => Tool::Pen,
        MenuTool::Line => Tool::Line,
        MenuTool::Arrow => Tool::Arrow,
        MenuTool::Rectangle => Tool::Rectangle,
        MenuTool::Ellipse => Tool::Ellipse,
        MenuTool::Eraser => Tool::Eraser,
    };
    let temporary = choice == MenuTool::TemporaryPen;
    let variant_changes = tool == Tool::Pen && strokes.temporary_pen() != temporary;
    let flip_arrow = tool == Tool::Arrow && strokes.tool() == Tool::Arrow;
    if strokes.tool() == tool && !variant_changes && !flip_arrow {
        return false;
    }
    trail.finish();
    drop(drag.take());
    let mut changed = strokes.select(tool);
    if variant_changes {
        changed |= strokes.toggle_temporary_pen();
    }
    if flip_arrow {
        changed |= strokes.toggle_arrow_direction();
    }
    changed
}

fn apply_menu_width(
    width: u32,
    strokes: &mut Strokes,
    trail: &mut Trail,
    drag: &mut Option<DragCapture>,
) -> bool {
    if !(1..=31).contains(&width) || width % 2 == 0 || width == strokes.style().thickness {
        return false;
    }
    trail.finish();
    drop(drag.take());
    let changed = strokes.cancel();
    strokes.set_thickness(width);
    changed
}

fn animate_trail(trail: &mut Trail, now: Duration, tick: bool) -> bool {
    let expired = trail.expire(now);
    expired || (tick && !trail.is_empty())
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
        return Ok(animate_trail(trail, now, true));
    }
    if let Event::Wheel(steps) = event {
        if !drawing {
            return Ok(false);
        }
        let width =
            (i64::from(strokes.style().thickness) + i64::from(steps) * 2).clamp(1, 31) as u32;
        return Ok(apply_menu_width(width, strokes, trail, drag));
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
    fn startup_help_lists_shortcuts_and_panel_without_event_chatter() {
        assert!(STARTUP_HELP.starts_with("Mado Screen\nStarts in click-through mode."));
        for command in [
            "Ctrl+Alt+D",
            "Ctrl+Alt+C",
            "Ctrl+Alt+Q",
            "Escape       Return to click-through",
            "P            Pen",
            "L            Straight line",
            "R            Rectangle",
            "A            Arrow",
            "O            Ellipse",
            "E            Erase",
            "C            Clear all ink",
            "X            Swap active and reserve",
            "F4           Switch between drawing",
            "~/.mado/mado-screen/config.ini",
            "Ctrl+Z",
            "Ctrl+Y",
            "[ / ]",
            "Mouse wheel",
            "1 / 2 / 3",
            "4 / 5",
            "Hover over the top-right tool icon",
            "Click tool icons, color slots or the width slider",
            "1 second",
        ] {
            assert!(
                STARTUP_HELP.contains(command),
                "Missing instruction: {command}"
            );
        }
        for old_log in [
            "PoC 7",
            "Mode:",
            "Tool:",
            "Overlay closed.",
            "picker",
            "Apply",
            "Cancel",
        ] {
            assert!(!STARTUP_HELP.contains(old_log));
        }
        let runtime = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert_eq!(
            runtime
                .lines()
                .filter(|line| line.trim_start().starts_with("println!("))
                .count(),
            1,
            "Only the startup guide should print routine output"
        );
        assert!(
            runtime.contains("eprintln!("),
            "Actionable errors must remain visible"
        );
    }

    #[test]
    fn startup_help_styling_preserves_text_and_highlights_controls() {
        let plain = startup_help(false);
        let styled = startup_help(true);
        let mut unstyled = String::new();
        let mut characters = styled.chars();
        while let Some(character) = characters.next() {
            if character == '\x1b' {
                assert_eq!(characters.next(), Some('['));
                assert!(characters.by_ref().any(|character| character == 'm'));
            } else {
                unstyled.push(character);
            }
        }
        assert_eq!(plain, STARTUP_HELP);
        assert_eq!(unstyled, plain);
        assert!(!plain.contains('\x1b'));
        for heading in [
            "Mado Screen",
            "Global shortcuts",
            "Drawing shortcuts",
            "Floating menu",
        ] {
            assert!(styled.contains(&format!("\x1b[1;38;2;205;154;34m{heading}\x1b[0m")));
        }
        for key in [
            "F4",
            "Ctrl+Alt+D",
            "Ctrl+Alt+C",
            "Ctrl+Alt+Q",
            "Escape",
            "P",
            "L",
            "R",
            "A",
            "O",
            "E",
            "C",
            "X",
            "Ctrl+Z",
            "Ctrl+Y",
            "[ / ]",
            "1 / 2 / 3",
            "4 / 5",
            "Drawing / Click-through",
        ] {
            assert!(styled.contains(&format!("\x1b[38;2;167;152;215m{key}\x1b[0m")));
        }
        for command in ["Pen", "Arrow", "Hover", "Click", "Drag"] {
            assert!(styled.contains(&format!("\x1b[38;2;105;173;208m{command}\x1b[0m")));
        }
        assert!(!styled.contains("\x1b[38;2;124;199;180m"));
        assert!(!styled.contains("\x1b[38;2;255;223;50m"));
        // Save both output paths for inspecting styling without starting the overlay.
        let artifacts =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/terminal-help");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("help-plain.txt"), plain).unwrap();
        std::fs::write(artifacts.join("help-styled.ansi"), styled).unwrap();
    }

    #[test]
    fn escape_only_deactivates_and_never_enters_drawing() {
        for drawing in [false, true] {
            assert!(is_mode_toggle_action(&Action::Toggle, drawing));
            assert_eq!(is_mode_toggle_action(&Action::Deactivate, drawing), drawing);
            assert!(!is_mode_toggle_action(&Action::Pen, drawing));
        }
    }

    #[test]
    fn quick_clear_only_accepts_c_when_drawing() {
        for drawing in [false, true] {
            assert!(is_clear_action(&Action::Clear, drawing));
            assert_eq!(is_clear_action(&Action::QuickClear, drawing), drawing);
            assert!(!is_clear_action(&Action::Pen, drawing));
        }
    }

    #[test]
    fn favorite_swap_exchanges_colors_settles_gestures_and_persists_once() {
        let mut strokes = Strokes::default();
        // Like palette changes, a swap discards an unfinished shape.
        strokes.select(Tool::Line);
        strokes.begin((10, 10));
        let mut trail = Trail::default();
        let mut reserve = 0xff0085dc;
        assert!(swap_colors(
            &mut reserve,
            &mut strokes,
            &mut trail,
            &mut None
        ));
        assert_eq!((strokes.style().color, reserve), (0xff0085dc, 0xfff97316));
        assert!(!strokes.is_active() && strokes.annotations().is_empty());
        assert!(!swap_colors(
            &mut reserve,
            &mut strokes,
            &mut trail,
            &mut None
        ));
        assert_eq!((strokes.style().color, reserve), (0xfff97316, 0xff0085dc));
        // Equal favorites leave an active gesture alone.
        reserve = strokes.style().color;
        strokes.begin((10, 10));
        assert!(!swap_colors(
            &mut reserve,
            &mut strokes,
            &mut trail,
            &mut None
        ));
        assert!(strokes.is_active());

        let path = std::env::temp_dir()
            .join(format!("mado-persist-test-{}", std::process::id()))
            .join("config.ini");
        let defaults = Favorites::default();
        let mut saved = defaults;
        persist(&mut saved, defaults.active, defaults.reserve, Some(&path));
        assert!(!path.exists(), "Unchanged favorites must not write");
        persist(&mut saved, 0xff123456, 0xff0085dc, Some(&path));
        assert_eq!(config::load(&path), saved);
        assert_eq!(saved.active, 0xff123456);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn quick_clear_removes_all_ink_and_preserves_undo_tool_and_style() {
        for tool in [Tool::Pen, Tool::Line] {
            let mut strokes = Strokes::default();
            strokes.begin((10, 10));
            strokes.finish();
            let committed = strokes.annotations().to_vec();
            strokes.select(tool);
            strokes.set_color(0xff35c7ef);
            strokes.set_thickness(31);
            let style = strokes.style();
            strokes.begin((20, 20));
            strokes.append((40, 30));
            let before_clear = strokes.annotations().to_vec();
            let mut trail = Trail::default();
            trail.begin((20, 20), style, Duration::ZERO);
            let mut drag = None;
            let mut expiry = None;
            clear_ink(&mut strokes, &mut trail, &mut drag, &mut expiry);
            assert!(strokes.annotations().is_empty() && !strokes.is_active());
            assert!(trail.is_empty() && !trail.is_active());
            assert!(drag.is_none() && expiry.is_none());
            assert_eq!(strokes.tool(), tool);
            assert_eq!(strokes.style(), style);
            assert!(strokes.undo());
            assert_eq!(
                strokes.annotations(),
                if tool == Tool::Pen {
                    &before_clear
                } else {
                    &committed
                }
            );
            assert!(trail.is_empty(), "Temporary ink must not return with undo");
            assert!(strokes.redo());
            assert!(strokes.annotations().is_empty());
            clear_ink(&mut strokes, &mut trail, &mut drag, &mut expiry);
            assert!(
                strokes.undo(),
                "An empty clear must not add a history entry"
            );
            assert!(!strokes.annotations().is_empty());
        }
    }

    #[test]
    fn panel_menu_tools_select_exact_variants_without_changing_history() {
        let mut strokes = Strokes::default();
        strokes.begin((2, 2));
        strokes.finish();
        let scene = strokes.annotations().to_vec();
        assert!(strokes.undo());
        let mut trail = Trail::default();
        let choices = [
            (MenuTool::TemporaryPen, "Temporary pen"),
            (MenuTool::Line, "Line"),
            (MenuTool::Rectangle, "Rectangle"),
            (MenuTool::Ellipse, "Ellipse"),
            (MenuTool::Eraser, "Eraser"),
            (MenuTool::Pen, "Pen"),
            (MenuTool::TemporaryPen, "Temporary pen"),
            (MenuTool::Pen, "Pen"),
        ];
        for (choice, label) in choices {
            assert!(!apply_menu_tool(
                choice,
                &mut strokes,
                &mut trail,
                &mut None
            ));
            assert_eq!(strokes.tool_label(), label);
            assert!(strokes.annotations().is_empty());
        }
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), scene);
    }

    #[test]
    fn panel_menu_identical_choice_preserves_active_input() {
        let mut strokes = Strokes::default();
        strokes.begin((10, 10));
        let mut trail = Trail::default();
        assert!(!apply_menu_tool(
            MenuTool::Pen,
            &mut strokes,
            &mut trail,
            &mut None
        ));
        assert!(strokes.is_active());
        strokes.cancel();
        strokes.toggle_temporary_pen();
        trail.begin((20, 20), strokes.style(), Duration::ZERO);
        assert!(!apply_menu_tool(
            MenuTool::TemporaryPen,
            &mut strokes,
            &mut trail,
            &mut None
        ));
        assert!(trail.is_active());
    }

    #[test]
    fn panel_menu_tool_changes_settle_shapes_but_retain_pen_and_temporary_ages() {
        for tool in [Tool::Pen, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((10, 10));
            strokes.append((30, 30));
            let mut trail = Trail::default();
            trail.begin((50, 50), strokes.style(), Duration::ZERO);
            let parts = trail.annotations().cloned().collect::<Vec<_>>();
            assert_eq!(
                apply_menu_tool(MenuTool::Ellipse, &mut strokes, &mut trail, &mut None),
                tool != Tool::Pen
            );
            assert_eq!(strokes.annotations().len(), usize::from(tool == Tool::Pen));
            assert!(!strokes.is_active() && !trail.is_active());
            assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), parts);
            assert!(trail.expire(Duration::from_secs(1)));
        }
    }

    #[test]
    fn panel_menu_arrow_click_flips_only_selected_arrow_and_freezes_old_geometry() {
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        apply_menu_tool(MenuTool::Arrow, &mut strokes, &mut trail, &mut None);
        assert_eq!(strokes.tool_label(), "Arrow head-first");
        strokes.begin((30, 30));
        strokes.append((100, 30));
        strokes.finish();
        let old = strokes.annotations().to_vec();
        apply_menu_tool(MenuTool::Arrow, &mut strokes, &mut trail, &mut None);
        assert_eq!(strokes.tool_label(), "Arrow tail-first");
        assert_eq!(strokes.annotations(), old);
        apply_menu_tool(MenuTool::Line, &mut strokes, &mut trail, &mut None);
        apply_menu_tool(MenuTool::Arrow, &mut strokes, &mut trail, &mut None);
        assert_eq!(strokes.tool_label(), "Arrow tail-first");
    }

    #[test]
    fn panel_menu_width_rejects_bad_values_and_freezes_old_style() {
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        strokes.begin((10, 10));
        for width in [0, 2, 32, u32::MAX, 5] {
            assert!(!apply_menu_width(
                width,
                &mut strokes,
                &mut trail,
                &mut None
            ));
            assert!(strokes.is_active());
            assert_eq!(strokes.style().thickness, 5);
        }
        assert!(!apply_menu_width(11, &mut strokes, &mut trail, &mut None));
        assert!(!strokes.is_active());
        assert_eq!(strokes.style().thickness, 11);
        assert_eq!(strokes.annotations()[0].style.thickness, 5);
        strokes.select(Tool::Rectangle);
        strokes.begin((20, 20));
        assert!(apply_menu_width(31, &mut strokes, &mut trail, &mut None));
        assert_eq!(strokes.style().thickness, 31);
        assert_eq!(strokes.annotations().len(), 1);
    }

    #[test]
    fn panel_menu_shortcuts_and_clicks_keep_distinct_pen_behavior() {
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        apply_menu_tool(MenuTool::TemporaryPen, &mut strokes, &mut trail, &mut None);
        assert!(strokes.temporary_pen());
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert!(!strokes.temporary_pen());
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert!(strokes.temporary_pen());
        apply_menu_tool(MenuTool::Pen, &mut strokes, &mut trail, &mut None);
        assert!(!strokes.temporary_pen());
    }

    #[test]
    fn panel_menu_mode_conflict_preserves_ink_and_click_through() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
        };
        // Reserve a drawing key only if not already owned by the running app.
        let reserved = unsafe { RegisterHotKey(None, 200, MOD_NOREPEAT, b'L' as u32) }.is_ok();
        let overlay = Overlay::new().unwrap();
        // Conflict is rejected before any upload; no desktop-sized bitmap is needed.
        let mut surface = Surface::new(1, 1).unwrap();
        let mut strokes = Strokes::default();
        strokes.begin((10, 10));
        strokes.finish();
        let old = strokes.annotations().to_vec();
        let mut trail = Trail::default();
        trail.begin((20, 20), strokes.style(), Duration::ZERO);
        let temporary = trail.annotations().cloned().collect::<Vec<_>>();
        let mut drawing = false;
        let mut keys = None;
        let result = toggle_drawing(
            &mut drawing,
            &mut keys,
            &overlay,
            &mut surface,
            (&mut strokes, &mut trail, Duration::ZERO),
            &mut None,
        );
        if reserved {
            unsafe { UnregisterHotKey(None, 200) }.unwrap();
        }
        result.unwrap();
        assert!(!drawing && keys.is_none());
        assert_eq!(strokes.annotations(), old);
        assert!(trail.is_active());
        assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), temporary);
    }

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
    fn poc7_ticks_repaint_surviving_ink_and_final_expiry_then_stop() {
        let mut trail = Trail::default();
        trail.begin((10, 10), strokes::Style::default(), Duration::ZERO);
        trail.finish();
        assert!(!animate_trail(
            &mut trail,
            Duration::from_millis(100),
            false
        ));
        assert!(
            handle_canvas_input(
                Event::Expire,
                false,
                (&mut Strokes::default(), &mut trail),
                &mut None,
                HWND::default(),
                Duration::from_millis(500),
            )
            .unwrap()
        );
        assert_eq!(
            trail
                .paint_parts(Duration::from_millis(500))
                .next()
                .unwrap()
                .2,
            128
        );
        assert!(animate_trail(&mut trail, Duration::from_secs(1), true));
        assert!(!animate_trail(&mut trail, Duration::from_secs(2), true));
        assert!(trail.is_empty());
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
        drawing::render(&mut pixels, 40, false, strokes.annotations());
        drawing::paint_trail(&mut pixels, 40, trail.paint_parts(Duration::ZERO));
        assert_eq!(pixels[10 * 40 + 10], 0xff3b82f6);
        assert!(trail.expire(Duration::from_secs(1)));
        drawing::render(&mut pixels, 40, false, strokes.annotations());
        drawing::paint_trail(&mut pixels, 40, trail.paint_parts(Duration::from_secs(1)));
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
        assert!(trail.expire(Duration::from_secs(1)));
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
        assert_eq!(strokes.annotations()[0].paths[0], [(1, 2), (4, 5), (5, 6)]);
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
    fn palette_selected_color_keeps_active_gestures() {
        for tool in [Tool::Pen, Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((10, 10));
            strokes.append((30, 30));
            let before = strokes.annotations().to_vec();
            let style = strokes.style();
            let mut trail = Trail::default();
            trail.begin((50, 50), style, Duration::ZERO);
            let parts = trail.annotations().cloned().collect::<Vec<_>>();
            assert!(!apply_ui_color(
                style.color,
                &mut strokes,
                &mut trail,
                &mut None
            ));
            assert!(strokes.is_active() && trail.is_active());
            assert_eq!(strokes.style(), style);
            assert_eq!(strokes.annotations(), before);
            assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), parts);
        }
    }

    #[test]
    fn palette_color_settles_gestures_and_ignores_trailing_canvas_input() {
        for tool in [Tool::Pen, Tool::Line, Tool::Rectangle] {
            for color in [0xff35c7ef, 0xff123456] {
                let mut strokes = Strokes::default();
                strokes.select(tool);
                strokes.begin((10, 10));
                strokes.append((30, 30));
                let mut trail = Trail::default();
                let mut drag = None;
                assert_eq!(
                    apply_ui_color(color, &mut strokes, &mut trail, &mut drag),
                    tool != Tool::Pen
                );
                assert_eq!(strokes.style().color, color);
                assert!(!strokes.is_active());
                let settled = strokes.annotations().to_vec();
                assert_eq!(settled.len(), usize::from(tool == Tool::Pen));
                if tool == Tool::Pen {
                    assert_eq!(settled[0].style.color, 0xfff97316);
                }
                for event in [
                    Event::Move {
                        point: (50, 50),
                        left_down: true,
                    },
                    Event::Up((60, 60)),
                ] {
                    assert!(
                        !handle_canvas_input(
                            event,
                            true,
                            (&mut strokes, &mut trail),
                            &mut drag,
                            HWND::default(),
                            Duration::ZERO,
                        )
                        .unwrap()
                    );
                }
                assert_eq!(strokes.annotations(), settled);
            }
        }
    }

    #[test]
    fn palette_color_keeps_temporary_births_and_permanent_redo() {
        let mut strokes = Strokes::default();
        strokes.begin((5, 5));
        strokes.finish();
        let permanent = strokes.annotations().to_vec();
        assert!(strokes.undo());
        let mut trail = Trail::default();
        trail.begin((10, 10), strokes.style(), Duration::ZERO);
        trail.append((30, 30), Duration::from_millis(100));
        let parts = trail.annotations().cloned().collect::<Vec<_>>();
        let mut drag = None;
        assert!(!apply_ui_color(
            0xff123456,
            &mut strokes,
            &mut trail,
            &mut drag
        ));
        assert!(!trail.is_active());
        assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), parts);
        for event in [
            Event::Move {
                point: (50, 50),
                left_down: true,
            },
            Event::Up((60, 60)),
        ] {
            assert!(
                !handle_canvas_input(
                    event,
                    true,
                    (&mut strokes, &mut trail),
                    &mut drag,
                    HWND::default(),
                    Duration::from_millis(500),
                )
                .unwrap()
            );
        }
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), permanent);
        assert!(trail.expire(Duration::from_millis(1000)));
        assert_eq!(trail.annotations().count(), 1);
        assert!(trail.expire(Duration::from_millis(1100)));
        assert!(trail.is_empty());
    }

    #[test]
    fn mouse_wheel_changes_width_only_when_drawing_and_clamps() {
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        let mut drag = None;
        for (drawing, steps, expected) in [
            (false, 1, 5),
            (false, -1, 5),
            (true, 1, 7),
            (true, -1, 5),
            (true, 3, 11),
            (true, i32::MAX, 31),
            (true, 1, 31),
            (true, i32::MIN, 1),
            (true, -1, 1),
            (true, 0, 1),
            (true, 2, 5),
        ] {
            assert!(
                !handle_canvas_input(
                    Event::Wheel(steps),
                    drawing,
                    (&mut strokes, &mut trail),
                    &mut drag,
                    HWND::default(),
                    Duration::ZERO,
                )
                .unwrap()
            ); // Width alone changes the menu, not canvas pixels.
            assert_eq!(strokes.style().thickness, expected);
        }
        // No-op scrolling must not settle a gesture, including at the width limits.
        for (drawing, width, steps) in [(false, 5, 1), (true, 31, 1), (true, 1, -1), (true, 5, 0)] {
            strokes.set_thickness(width);
            strokes.begin((10, 10));
            let before = strokes.annotations().to_vec();
            trail.begin((20, 20), strokes.style(), Duration::ZERO);
            assert!(
                !handle_canvas_input(
                    Event::Wheel(steps),
                    drawing,
                    (&mut strokes, &mut trail),
                    &mut drag,
                    HWND::default(),
                    Duration::ZERO,
                )
                .unwrap()
            );
            assert!(strokes.is_active() && trail.is_active());
            assert_eq!(strokes.annotations(), before);
            strokes.finish();
            trail.finish();
        }
    }

    #[test]
    fn mouse_wheel_settles_gestures_preserves_ink_and_temporary_births() {
        for tool in [Tool::Pen, Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((10, 10));
            strokes.append((30, 30));
            let mut trail = Trail::default();
            trail.begin((50, 50), strokes.style(), Duration::ZERO);
            trail.append((60, 60), Duration::from_millis(100));
            let parts = trail.annotations().cloned().collect::<Vec<_>>();
            let mut drag = None;
            assert_eq!(
                handle_canvas_input(
                    Event::Wheel(1),
                    true,
                    (&mut strokes, &mut trail),
                    &mut drag,
                    HWND::default(),
                    Duration::from_millis(100),
                )
                .unwrap(),
                tool != Tool::Pen
            );
            assert_eq!(strokes.style().thickness, 7);
            assert!(!strokes.is_active() && !trail.is_active());
            assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), parts);
            let settled = strokes.annotations().to_vec();
            assert_eq!(settled.len(), usize::from(tool == Tool::Pen));
            if tool == Tool::Pen {
                assert_eq!(settled[0].style.thickness, 5);
                assert!(strokes.undo());
                assert!(
                    !handle_canvas_input(
                        Event::Wheel(-1),
                        true,
                        (&mut strokes, &mut trail),
                        &mut drag,
                        HWND::default(),
                        Duration::from_millis(100),
                    )
                    .unwrap()
                );
                assert!(strokes.redo());
                assert_eq!(strokes.annotations(), settled);
            }
            for event in [
                Event::Move {
                    point: (70, 70),
                    left_down: true,
                },
                Event::Up((80, 80)),
            ] {
                assert!(
                    !handle_canvas_input(
                        event,
                        true,
                        (&mut strokes, &mut trail),
                        &mut drag,
                        HWND::default(),
                        Duration::from_millis(100),
                    )
                    .unwrap()
                );
            }
            assert_eq!(strokes.annotations(), settled);
            assert!(trail.expire(Duration::from_millis(1000)));
            assert_eq!(trail.annotations().count(), 1);
            assert!(trail.expire(Duration::from_millis(1100)));
            assert!(trail.is_empty());
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
