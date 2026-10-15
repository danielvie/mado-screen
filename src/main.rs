#![cfg_attr(not(test), windows_subsystem = "windows")]

mod bezier;
mod controls;
mod drawing;
mod input;
mod overlay;
mod settings;
mod strokes;
mod surface;
mod trail;
mod tray;
mod ui;

use controls::{Action, Hotkeys, Shortcuts};
use input::{DragCapture, Event, ExpiryTimer, MouseWheel};
use overlay::Overlay;
use settings::Favorites;
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
  P            Pen, permanent ink
  S            Smoke pen, temporary ink (default fade: 1 second)
  L            Straight line
  R            Rectangle outline
  A            Arrow; press again to switch head-first / tail-first
  O            Ellipse outline
  E            Erase a whole annotation
  C            Clear all ink, undoable for permanent ink
  X            Swap active and reserve colors
  T            Swap current and previously selected tool
  H            Hide / show the floating menu
  Ctrl+Z       Undo
  Ctrl+Y       Redo
  [ / ]        Thinner / thicker brush (1 to 31 pixels)
  Mouse wheel  Scroll up for thicker ink, down for thinner ink
  1 / 2 / 3    Blue / orange / red
  4 / 5        Green / white

Floating menu
  Hover over the top-right tool icon to open the dark menu.
  Click the pin to keep the menu expanded; click again to unpin.
  Click tool icons, color slots or the width slider; letters show shortcuts.
  Click Drawing / Click-through to switch canvas input without a shortcut.
  Drag a palette color onto the active or reserve square; click the reserve to swap.
  Drag the tool icon or its grip to move the menu.
  Click the tray icon to switch modes; right-click it to hide the menu, clear or quit.
  Choose Reload config in the tray menu to apply edited settings without restarting.
  Choose Panel, Edge rail or Radial ring under the tray's Menu layout submenu.
  Radial ring: click the badge or right-click while drawing; its minimize button closes it.

Default brush: orange, 5 pixels; reserve blue. Restart after changing the monitor layout.
Settings and favorite colors are saved in ~/.mado/mado-screen/settings.toml.
";

fn startup_help(styled: bool, shortcuts: impl Into<Shortcuts>) -> String {
    let shortcuts = shortcuts.into();
    let activation = shortcuts.binding("activation_key").label();
    let toggle = shortcuts.binding("toggle").label();
    let defaults = Shortcuts::default();
    let mut guide = String::new();
    for line in STARTUP_HELP.lines() {
        let line = if line.starts_with("Starts in") {
            format!("Starts in click-through mode. Press {activation} or {toggle} to draw.")
        } else if let Some((key, description)) = line
            .strip_prefix("  ")
            .and_then(|row| row.split_once("   "))
        {
            let name = controls::SHORTCUT_NAMES
                .iter()
                .find(|name| defaults.binding(name).label() == key);
            let key = if key == "F4" {
                activation.clone()
            } else if let Some(name) = name {
                shortcuts.binding(name).label()
            } else {
                key.split(" / ")
                    .map(|part| {
                        controls::SHORTCUT_NAMES
                            .iter()
                            .find(|name| defaults.binding(name).label() == part)
                            .map_or_else(|| part.to_owned(), |name| shortcuts.binding(name).label())
                    })
                    .collect::<Vec<_>>()
                    .join(" / ")
            };
            format!("  {key:<10}   {}", description.trim_start())
        } else {
            line.to_owned()
        };
        guide.push_str(&line);
        guide.push('\n');
    }
    if !styled {
        return guide;
    }
    const RESET: &str = "\x1b[0m";
    // Pi dark palette: mdCode/accent, mdLink and mdHeading, resolved from OKHSL.
    const SHORTCUT: &str = "\x1b[38;2;167;152;215m";
    const COMMAND: &str = "\x1b[38;2;105;173;208m";
    const TITLE: &str = "\x1b[1;38;2;205;154;34m";
    let mut help = String::new();
    for line in guide.lines() {
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
            for shortcut in [
                activation.as_str(),
                toggle.as_str(),
                "Drawing / Click-through",
            ] {
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

fn print_startup_help(shortcuts: Shortcuts) {
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
    println!("{}", startup_help(console.is_some(), shortcuts));
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
    let settings_path = settings::path();
    let mut startup = settings_path
        .as_deref()
        .map(settings::load)
        .unwrap_or_default();
    let mut hotkeys = Hotkeys::register(startup.shortcuts)?;
    // Reverse drop order: drag, drawing keys, tray, UI, window, surface, permanent keys.
    let mut surface;
    let overlay = Overlay::new()?;
    let (width, height) = overlay.size();
    surface = Surface::new(width, height)?;
    let mut drawing = false;
    let mut strokes = Strokes::default();
    let mut saved = (startup.favorites, startup.thickness_px, startup.tail_first);
    strokes.set_color(saved.0.active);
    strokes.set_thickness(startup.thickness_px);
    strokes.set_arrow_head_scale(startup.head_scale);
    if startup.tail_first {
        strokes.toggle_arrow_direction();
    }
    let mut reserve = saved.0.reserve;
    let mut trail = Trail::new(Duration::from_millis(startup.duration_ms));
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
        startup.visible_on_start,
        startup.shortcuts,
    )?;
    ui.set_reserve(reserve)?;
    ui.set_layout(startup.layout)?;
    ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
    let tray = Tray::new(overlay.hwnd())?;
    let mut panel_visible = startup.visible_on_start;
    tray.set_panel_visible(panel_visible);
    tray.set_shortcuts(startup.shortcuts);
    tray.set_layout(startup.layout);
    tray.update(drawing, strokes.tool_label(), strokes.style().color)?;
    let mut tool_keys: Option<MouseWheel> = None;
    let mut drag: Option<DragCapture> = None;
    let mut expiry: Option<ExpiryTimer> = None;
    print_startup_help(startup.shortcuts);
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
                        (&mut hotkeys, startup.shortcuts),
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
            tray.update(drawing, strokes.tool_label(), strokes.style().color)?;
            persist(
                &mut saved,
                strokes.style(),
                reserve,
                strokes.arrow_tail_first(),
                settings_path.as_deref(),
            );
            continue;
        }
        // Tray commands reuse the same transactions as Ctrl+Alt+D/C/Q.
        if let Some(event) = tray.event(&message) {
            match event {
                TrayEvent::Toggle => toggle_drawing(
                    &mut drawing,
                    &mut tool_keys,
                    (&mut hotkeys, startup.shortcuts),
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
                TrayEvent::Panel => toggle_panel(&mut panel_visible, &ui, &tray)?,
                TrayEvent::Reload => {
                    match load_replacement(settings_path.as_deref(), &mut hotkeys, drawing) {
                        Err(error) => {
                            eprintln!("Config reload failed: {error}");
                            tray.notify_reload(Err(&error));
                        }
                        Ok(next) => {
                            apply_reloaded_settings(
                                next,
                                &mut strokes,
                                &mut trail,
                                &mut drag,
                                &mut reserve,
                            );
                            startup = next;
                            saved = (next.favorites, next.thickness_px, next.tail_first);
                            trail.expire(now);
                            if trail.is_empty() {
                                drop(expiry.take());
                            }
                            // Config is committed. Attempt every display update even if one fails.
                            tray.set_shortcuts(next.shortcuts);
                            tray.set_layout(next.layout);
                            let visibility = if panel_visible != next.visible_on_start {
                                let result = ui.set_visible(next.visible_on_start);
                                if result.is_ok() {
                                    panel_visible = next.visible_on_start;
                                }
                                tray.set_panel_visible(panel_visible);
                                result
                            } else {
                                Ok(())
                            };
                            let updates = [
                                ui.set_shortcuts(next.shortcuts),
                                ui.set_layout(next.layout),
                                ui.set_reserve(reserve),
                                visibility,
                                ui.update_label(drawing, strokes.tool_label(), strokes.style()),
                                tray.update(drawing, strokes.tool_label(), strokes.style().color),
                                surface.present_scene(
                                    overlay.hwnd(),
                                    drawing,
                                    strokes.annotations(),
                                    &trail,
                                    now,
                                ),
                            ];
                            let errors = updates
                                .into_iter()
                                .filter_map(|result| result.err())
                                .map(|error| error.to_string())
                                .collect::<Vec<_>>();
                            if errors.is_empty() {
                                tray.notify_reload(Ok("Settings and shortcuts are now active."));
                            } else {
                                let warning = format!(
                                    "Config applied, but display refresh failed: {}",
                                    errors.join("; ")
                                );
                                eprintln!("{warning}");
                                tray.notify_reload(Ok(&warning));
                            }
                        }
                    }
                    continue;
                }
                TrayEvent::Layout(layout) => {
                    ui.set_layout(layout)?;
                    tray.set_layout(layout);
                    if let Some(path) = settings_path.as_deref()
                        && let Err(error) = settings::save_layout(path, layout)
                    {
                        eprintln!("Could not save {}: {error}", path.display());
                    }
                }
            }
            ui.update_label(drawing, strokes.tool_label(), strokes.style())?;
            tray.update(drawing, strokes.tool_label(), strokes.style().color)?;
            continue;
        }
        if message.message == WM_HOTKEY {
            let action = hotkey_action(&message, &hotkeys);
            let Some(action) = action else {
                continue;
            };
            match action {
                action if is_mode_toggle_action(&action, drawing) => {
                    toggle_drawing(
                        &mut drawing,
                        &mut tool_keys,
                        (&mut hotkeys, startup.shortcuts),
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
                Action::Panel if drawing => toggle_panel(&mut panel_visible, &ui, &tray)?,
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
            tray.update(drawing, strokes.tool_label(), strokes.style().color)?;
            persist(
                &mut saved,
                strokes.style(),
                reserve,
                strokes.arrow_tail_first(),
                settings_path.as_deref(),
            );
        } else if let Some(event) = input::event(&message, overlay.hwnd()) {
            // The canvas only receives clicks while drawing; other layouts ignore it.
            // A stroke in progress keeps the capture, so the ring waits for its end.
            if let Event::Context(point) = event {
                if drag.is_none() {
                    ui.summon(point);
                }
                continue;
            }
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
                persist(
                    &mut saved,
                    strokes.style(),
                    reserve,
                    strokes.arrow_tail_first(),
                    settings_path.as_deref(),
                );
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

fn hotkey_action(message: &MSG, hotkeys: &Hotkeys) -> Option<Action> {
    if message.message != WM_HOTKEY {
        return None;
    }
    if message.lParam.0 == 0 {
        // App-local posted actions retain the stable IDs for smoke checks.
        controls::action(message.wParam.0)
    } else {
        hotkeys.action(message.wParam.0)
    }
}

fn load_replacement(
    path: Option<&Path>,
    hotkeys: &mut Hotkeys,
    drawing: bool,
) -> std::result::Result<settings::Settings, String> {
    let path = path.ok_or_else(|| "Cannot locate the settings file.".to_owned())?;
    let next = settings::reload(path).map_err(|error| error.to_string())?;
    hotkeys
        .replace(next.shortcuts, true)
        .map_err(|error| error.to_string())?;
    if !drawing {
        hotkeys
            .replace(next.shortcuts, false)
            .map_err(|error| error.to_string())?;
    }
    Ok(next)
}

fn apply_reloaded_settings(
    next: settings::Settings,
    strokes: &mut Strokes,
    trail: &mut Trail,
    drag: &mut Option<DragCapture>,
    reserve: &mut u32,
) {
    strokes.cancel();
    drop(drag.take());
    trail.set_lifetime(Duration::from_millis(next.duration_ms));
    strokes.set_color(next.favorites.active);
    strokes.set_thickness(next.thickness_px);
    strokes.set_arrow_head_scale(next.head_scale);
    if strokes.arrow_tail_first() != next.tail_first {
        strokes.toggle_arrow_direction();
    }
    *reserve = next.favorites.reserve;
}

// Tray and keyboard share one visibility state; hiding the UI never alters the canvas.
fn toggle_panel(visible: &mut bool, ui: &DesktopUi, tray: &Tray) -> Result<()> {
    let next = !*visible;
    ui.set_visible(next)?;
    *visible = next;
    tray.set_panel_visible(next);
    Ok(())
}

// The visible input toggle, Ctrl+Alt+D and drawing-only Escape use this transaction.
fn toggle_drawing(
    drawing: &mut bool,
    keys: &mut Option<MouseWheel>,
    bindings: (&mut Hotkeys, Shortcuts),
    overlay: &Overlay,
    surface: &mut Surface,
    scene: (&mut Strokes, &mut Trail, Duration),
    drag: &mut Option<DragCapture>,
) -> Result<()> {
    let (strokes, trail, now) = scene;
    let (hotkeys, shortcuts) = bindings;
    let entering = !*drawing;
    let pending = if entering {
        match MouseWheel::begin(overlay.hwnd())
            .and_then(|wheel| hotkeys.set_drawing(shortcuts, true).map(|_| wheel))
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
    if !entering {
        hotkeys.set_drawing(shortcuts, false)?;
    }
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
fn persist(
    saved: &mut (Favorites, u32, bool),
    style: strokes::Style,
    reserve: u32,
    tail_first: bool,
    path: Option<&Path>,
) {
    let current = (
        Favorites {
            active: style.color,
            reserve,
        },
        style.thickness,
        tail_first,
    );
    if *saved == current {
        return;
    }
    if let Some(path) = path
        && let Err(error) = settings::save_brush(path, current.0, current.1, current.2)
    {
        eprintln!("Could not save {}: {error}", path.display());
        return;
    }
    *saved = current;
}

// Pen shortcuts and menu choices select exact variants; repeated A flips direction.
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
    let mut changed = if tool == Tool::Pen {
        strokes.select_pen(temporary)
    } else {
        strokes.select(tool)
    };
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
    if !(1..=31).contains(&width) || width.is_multiple_of(2) || width == strokes.style().thickness {
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
    if action == Action::PreviousTool {
        if !strokes.has_previous_tool() {
            return false;
        }
        drop(drag.take());
        return strokes.swap_previous_tool();
    }
    if matches!(action, Action::Pen | Action::TemporaryPen) {
        let temporary = action == Action::TemporaryPen;
        if strokes.tool() == Tool::Pen && strokes.temporary_pen() == temporary {
            return false;
        }
        drop(drag.take());
        return strokes.select_pen(temporary);
    }
    let tool = match action {
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
        Action::Pen => strokes.tool() != Tool::Pen || strokes.temporary_pen(),
        Action::TemporaryPen => strokes.tool() != Tool::Pen || !strokes.temporary_pen(),
        Action::PreviousTool => strokes.has_previous_tool(),
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
    use controls::Shortcut as ActivationKey;
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
            "S            Smoke pen",
            "Click the pin to keep the menu expanded",
            "L            Straight line",
            "R            Rectangle",
            "A            Arrow",
            "O            Ellipse",
            "E            Erase",
            "C            Clear all ink",
            "X            Swap active and reserve",
            "T            Swap current and previously selected tool",
            "H            Hide / show the floating menu",
            "F4           Switch between drawing",
            "~/.mado/mado-screen/settings.toml",
            "Ctrl+Z",
            "Ctrl+Y",
            "[ / ]",
            "Mouse wheel",
            "1 / 2 / 3",
            "4 / 5",
            "Hover over the top-right tool icon",
            "Menu layout submenu",
            "Radial ring: click the badge",
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
        let plain = startup_help(false, ActivationKey::default());
        let styled = startup_help(true, ActivationKey::default());
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
    fn settings_remapped_shortcuts_update_every_help_row() {
        let defaults = Shortcuts::default();
        for name in controls::SHORTCUT_NAMES {
            let mut shortcuts = defaults;
            shortcuts.set(
                name,
                controls::Shortcut::parse("Ctrl+Alt+Shift+F11").unwrap(),
            );
            let help = startup_help(false, shortcuts);
            assert!(help.contains("Ctrl+Alt+Shift+F11"), "{name}");
            let styled = startup_help(true, shortcuts);
            assert!(
                styled
                    .lines()
                    .any(|line| line.contains("Ctrl+Alt+Shift+F11")
                        && line.contains("\x1b[38;2;167;152;215m")),
                "{name}"
            );
        }
        let mut shortcuts = defaults;
        shortcuts.set("pen", controls::Shortcut::parse("Shift+P").unwrap());
        shortcuts.set("smoke_pen", controls::Shortcut::parse("Alt+S").unwrap());
        assert!(startup_help(false, shortcuts).contains("Shift+P      Pen"));
        assert!(startup_help(false, shortcuts).contains("Alt+S        Smoke pen"));
    }

    #[test]
    fn settings_reload_rejects_bad_file_or_conflicting_hotkey_before_gesture_changes() {
        let directory =
            std::env::temp_dir().join(format!("mado-live-reload-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let mut shortcuts = Shortcuts::default();
        for (name, key) in controls::SHORTCUT_NAMES.iter().zip(b'A'..=b'Y') {
            shortcuts.set(
                name,
                controls::Shortcut::parse(&format!("Ctrl+Alt+Shift+{}", key as char)).unwrap(),
            );
        }
        let config = controls::SHORTCUT_NAMES
            .iter()
            .map(|name| format!("{name} = '{}'", shortcuts.binding(name).label()))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            &path,
            format!("[shortcuts]\n{config}\n[arrow]\nhead_scale=2.0"),
        )
        .unwrap();
        let mut hotkeys = Hotkeys::empty();
        let loaded = load_replacement(Some(&path), &mut hotkeys, false).unwrap();
        assert_eq!(loaded.shortcuts, shortcuts);
        assert_eq!(loaded.head_scale, 2.0);
        let mut strokes = Strokes::default();
        strokes.begin((1, 1));
        strokes.append((10, 10));
        let ink = strokes.annotations().to_vec();
        let mut trail = Trail::default();
        trail.begin((20, 20), strokes.style(), Duration::from_millis(100));
        let temporary = trail.annotations().cloned().collect::<Vec<_>>();
        std::fs::write(&path, "[arrow]\nhead_scale=nan").unwrap();
        assert!(load_replacement(Some(&path), &mut hotkeys, false).is_err());
        assert!(strokes.is_active() && trail.is_active());
        assert_eq!(strokes.annotations(), ink);
        assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), temporary);
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
        };
        let modifiers = MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
        unsafe { RegisterHotKey(None, 900, modifiers, b'Z' as u32) }.unwrap();
        let conflicted = config.replace("pen = 'Ctrl+Alt+Shift+D'", "pen = 'Ctrl+Alt+Shift+Z'");
        std::fs::write(&path, format!("[shortcuts]\n{conflicted}")).unwrap();
        let result = load_replacement(Some(&path), &mut hotkeys, false);
        unsafe { UnregisterHotKey(None, 900) }.unwrap();
        assert!(result.is_err());
        assert!(strokes.is_active() && trail.is_active());
        assert_eq!(strokes.annotations(), ink);
        // A rejected reload retains global keys but must not reserve drawing-only keys.
        unsafe { RegisterHotKey(None, 901, modifiers, b'D' as u32) }.unwrap();
        unsafe { UnregisterHotKey(None, 901) }.unwrap();
        assert!(unsafe { RegisterHotKey(None, 902, modifiers, b'A' as u32) }.is_err());
        std::fs::write(&path, format!("[shortcuts]\n{config}")).unwrap();
        assert!(load_replacement(Some(&path), &mut hotkeys, true).is_ok());
        assert!(unsafe { RegisterHotKey(None, 903, modifiers, b'D' as u32) }.is_err());
        assert!(load_replacement(None, &mut hotkeys, true).is_err());
        drop(hotkeys);
        unsafe { RegisterHotKey(None, 904, modifiers, b'A' as u32) }.unwrap();
        unsafe { UnregisterHotKey(None, 904) }.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_reload_routes_native_messages_across_global_drawing_swaps() {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        let directory =
            std::env::temp_dir().join(format!("mado-routing-reload-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let mut shortcuts = Shortcuts::default();
        for (name, key) in controls::SHORTCUT_NAMES.iter().zip(b'A'..=b'Y') {
            shortcuts.set(
                name,
                controls::Shortcut::parse(&format!("Ctrl+Alt+Shift+{}", key as char)).unwrap(),
            );
        }
        let mut hotkeys = Hotkeys::empty();
        for _ in 0..3 {
            let config = controls::SHORTCUT_NAMES
                .iter()
                .map(|name| format!("{name} = '{}'", shortcuts.binding(name).label()))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, format!("[shortcuts]\n{config}")).unwrap();
            load_replacement(Some(&path), &mut hotkeys, true).unwrap();
            let pen_id = (1..2000)
                .find(|id| hotkeys.action(*id) == Some(Action::Pen))
                .unwrap();
            let toggle_id = (1..2000)
                .find(|id| hotkeys.action(*id) == Some(Action::Toggle))
                .unwrap();
            let mut native = MSG {
                message: WM_HOTKEY,
                wParam: WPARAM(pen_id),
                lParam: LPARAM(1),
                ..Default::default()
            };
            assert_eq!(hotkey_action(&native, &hotkeys), Some(Action::Pen));
            let pen = shortcuts.binding("pen");
            shortcuts.set("pen", shortcuts.binding("toggle"));
            shortcuts.set("toggle", pen);
            let config = controls::SHORTCUT_NAMES
                .iter()
                .map(|name| format!("{name} = '{}'", shortcuts.binding(name).label()))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, format!("[shortcuts]\n{config}")).unwrap();
            load_replacement(Some(&path), &mut hotkeys, true).unwrap();
            assert_eq!(hotkey_action(&native, &hotkeys), Some(Action::Toggle));
            native.wParam = WPARAM(toggle_id);
            assert_eq!(hotkey_action(&native, &hotkeys), Some(Action::Pen));
            hotkeys.set_drawing(shortcuts, false).unwrap();
            assert_eq!(hotkey_action(&native, &hotkeys), None);
            native.wParam = WPARAM(pen_id);
            assert_eq!(hotkey_action(&native, &hotkeys), Some(Action::Toggle));
            hotkeys.set_drawing(shortcuts, true).unwrap();
            let current_pen_id = (1..2000)
                .find(|id| hotkeys.action(*id) == Some(Action::Pen))
                .unwrap();
            native.wParam = WPARAM(current_pen_id);
            assert_eq!(hotkey_action(&native, &hotkeys), Some(Action::Pen));
            native.lParam = LPARAM(0);
            native.wParam = WPARAM(4);
            assert_eq!(hotkey_action(&native, &hotkeys), Some(Action::Pen));
            native.message = WM_MOUSEMOVE;
            assert_eq!(hotkey_action(&native, &hotkeys), None);
        }
        drop(hotkeys);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_reload_applies_preferences_without_replacing_ink_history_or_selection() {
        for tool in [Tool::Pen, Tool::Arrow, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(Tool::Arrow);
            strokes.begin((0, 0));
            strokes.append((1000, 0));
            strokes.finish();
            let permanent = strokes.annotations().to_vec();
            strokes.select(Tool::Line);
            strokes.begin((10, 50));
            strokes.append((100, 50));
            strokes.finish();
            assert!(strokes.undo());
            let mut trail = Trail::default();
            trail.begin((20, 20), strokes.style(), Duration::from_millis(100));
            trail.append((30, 30), Duration::from_millis(200));
            let temporary = trail.annotations().cloned().collect::<Vec<_>>();
            strokes.select(tool);
            strokes.begin((200, 200));
            strokes.append((300, 300));
            let mut next = settings::Settings::default();
            next.favorites.active = 0xff123456;
            next.favorites.reserve = 0xffabcdef;
            next.thickness_px = 11;
            next.duration_ms = 2000;
            next.head_scale = 2.0;
            next.tail_first = true;
            let mut reserve = 0;
            apply_reloaded_settings(next, &mut strokes, &mut trail, &mut None, &mut reserve);
            assert_eq!(strokes.tool(), tool);
            assert_eq!(strokes.annotations()[0], permanent[0]);
            assert_eq!(strokes.style().color, next.favorites.active);
            assert_eq!(strokes.style().thickness, 11);
            assert_eq!(strokes.arrow_head_scale(), 2.0);
            assert!(strokes.arrow_tail_first());
            assert_eq!(reserve, next.favorites.reserve);
            assert!(!strokes.is_active() && !trail.is_active());
            assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), temporary);
            assert!(!trail.expire(Duration::from_millis(2099)));
            assert!(trail.expire(Duration::from_millis(2100)));
            if tool != Tool::Pen {
                assert_eq!(strokes.annotations(), permanent);
                assert!(strokes.redo());
                assert_eq!(strokes.annotations().len(), 2);
            } else {
                assert_eq!(strokes.annotations().len(), 2);
                assert!(strokes.undo());
                assert_eq!(strokes.annotations(), permanent);
            }
        }
    }

    #[test]
    fn settings_activation_key_updates_startup_help() {
        for label in ["F1", "F9", "F10", "F11"] {
            let key = ActivationKey::parse(label).unwrap();
            let plain = startup_help(false, key);
            assert!(plain.contains(&format!("Press {label} or Ctrl+Alt+D")));
            assert!(plain.contains(&format!("  {label:<13}Switch between drawing")));
            assert!(!plain.contains("F4"));
            let styled = startup_help(true, key);
            assert!(styled.contains(&format!("\x1b[38;2;167;152;215m{label}\x1b[0m")));
        }
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
            .join("settings.toml");
        let defaults = Favorites::default();
        let mut saved = (defaults, 5, false);
        let mut style = strokes::Style::default();
        persist(&mut saved, style, defaults.reserve, false, Some(&path));
        assert!(!path.exists(), "Unchanged brush must not write");
        style.color = 0xff123456;
        persist(&mut saved, style, 0xff0085dc, false, Some(&path));
        assert_eq!(settings::load(&path).favorites, saved.0);
        assert_eq!(saved.0.active, 0xff123456);
        style.thickness = 13;
        persist(&mut saved, style, 0xff0085dc, false, Some(&path));
        assert_eq!(settings::load(&path).thickness_px, 13);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn settings_thickness_persists_from_wheel_slider_and_keys() {
        let path = std::env::temp_dir()
            .join(format!("mado-thickness-test-{}", std::process::id()))
            .join("settings.toml");
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        let mut saved = (Favorites::default(), 5, false);
        let reserve = saved.0.reserve;
        handle_canvas_input(
            Event::Wheel(1),
            true,
            (&mut strokes, &mut trail),
            &mut None,
            HWND::default(),
            Duration::ZERO,
        )
        .unwrap();
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert_eq!(settings::load(&path).thickness_px, 7);
        apply_menu_width(15, &mut strokes, &mut trail, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert_eq!(settings::load(&path).thickness_px, 15);
        apply_edit(Action::Thinner, &mut strokes, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert_eq!(settings::load(&path).thickness_px, 13);
        apply_edit(Action::Thicker, &mut strokes, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        let startup = settings::load(&path);
        let mut restarted = Strokes::default();
        restarted.set_thickness(startup.thickness_px);
        assert_eq!(restarted.style().thickness, 15);
        // A failed save must remain retryable, never marked successfully persisted.
        std::fs::write(&path, "[brush").unwrap();
        let before = saved;
        strokes.set_thickness(17);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert_eq!(saved, before);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[brush");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn settings_arrow_direction_persists_from_shortcut_menu_and_previous_tool() {
        let path = std::env::temp_dir()
            .join(format!("mado-arrow-preference-test-{}", std::process::id()))
            .join("settings.toml");
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        let mut saved = (Favorites::default(), 5, false);
        let reserve = saved.0.reserve;
        apply_edit(Action::Arrow, &mut strokes, &mut None);
        apply_edit(Action::Arrow, &mut strokes, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert!(settings::load(&path).tail_first);
        // Choosing another tool keeps the remembered arrow preference.
        apply_menu_tool(MenuTool::Pen, &mut strokes, &mut trail, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert!(settings::load(&path).tail_first);
        apply_menu_tool(MenuTool::Arrow, &mut strokes, &mut trail, &mut None);
        apply_menu_tool(MenuTool::Arrow, &mut strokes, &mut trail, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        assert!(!settings::load(&path).tail_first);
        apply_edit(Action::PreviousTool, &mut strokes, &mut None);
        persist(
            &mut saved,
            strokes.style(),
            reserve,
            strokes.arrow_tail_first(),
            Some(&path),
        );
        let startup = settings::load(&path);
        assert!(startup.tail_first);
        let mut restarted = Strokes::default();
        if startup.tail_first {
            restarted.toggle_arrow_direction();
        }
        assert!(!restarted.has_previous_tool());
        restarted.select(Tool::Arrow);
        assert_eq!(restarted.tool_label(), "Arrow tail-first");
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
    fn panel_menu_shortcuts_and_clicks_select_exact_pen_variants() {
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        for choice in [MenuTool::Pen, MenuTool::TemporaryPen] {
            apply_menu_tool(choice, &mut strokes, &mut trail, &mut None);
            for (action, temporary) in [
                (Action::Pen, false),
                (Action::Pen, false),
                (Action::TemporaryPen, true),
                (Action::TemporaryPen, true),
            ] {
                apply_edit(action, &mut strokes, &mut None);
                assert_eq!(strokes.tool(), Tool::Pen);
                assert_eq!(strokes.temporary_pen(), temporary);
            }
        }
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
        let shortcuts = Shortcuts::default();
        let mut hotkeys = Hotkeys::empty();
        let result = toggle_drawing(
            &mut drawing,
            &mut keys,
            (&mut hotkeys, shortcuts),
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
    fn previous_tool_swaps_menu_and_shortcut_choices_with_variants() {
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        let mut drag = None;
        assert!(!strokes.has_previous_tool());
        assert!(!interrupts_trail(&Action::PreviousTool, &strokes));
        assert!(!apply_edit(Action::PreviousTool, &mut strokes, &mut drag));
        apply_edit(Action::Arrow, &mut strokes, &mut drag);
        for label in ["Pen", "Arrow head-first", "Pen", "Arrow head-first"] {
            apply_edit(Action::PreviousTool, &mut strokes, &mut drag);
            assert_eq!(strokes.tool_label(), label);
        }
        // Repeating A is itself a variant selection; T restores the exact previous variant.
        apply_edit(Action::Arrow, &mut strokes, &mut drag);
        assert_eq!(strokes.tool_label(), "Arrow tail-first");
        for label in ["Arrow head-first", "Arrow tail-first"] {
            apply_edit(Action::PreviousTool, &mut strokes, &mut drag);
            assert_eq!(strokes.tool_label(), label);
        }
        apply_menu_tool(MenuTool::TemporaryPen, &mut strokes, &mut trail, &mut drag);
        assert_eq!(strokes.tool_label(), "Temporary pen");
        for label in ["Arrow tail-first", "Temporary pen"] {
            apply_edit(Action::PreviousTool, &mut strokes, &mut drag);
            assert_eq!(strokes.tool_label(), label);
        }
        // Exact menu pen selection is one transaction, not Pen followed by variant toggle.
        apply_menu_tool(MenuTool::Pen, &mut strokes, &mut trail, &mut drag);
        for label in ["Temporary pen", "Pen"] {
            apply_edit(Action::PreviousTool, &mut strokes, &mut drag);
            assert_eq!(strokes.tool_label(), label);
        }
        apply_menu_tool(MenuTool::Line, &mut strokes, &mut trail, &mut drag);
        apply_menu_tool(MenuTool::Line, &mut strokes, &mut trail, &mut drag);
        apply_edit(Action::PreviousTool, &mut strokes, &mut drag);
        assert_eq!(strokes.tool_label(), "Pen"); // Re-selecting the same tool keeps history.
        let style = strokes.style();
        strokes.clear();
        apply_edit(Action::PreviousTool, &mut strokes, &mut drag);
        assert_eq!(strokes.tool_label(), "Line");
        assert_eq!(strokes.style(), style);
    }

    #[test]
    fn previous_tool_settles_gestures_without_changing_completed_ink() {
        for tool in [
            Tool::Pen,
            Tool::Line,
            Tool::Arrow,
            Tool::Rectangle,
            Tool::Eraser,
        ] {
            let mut strokes = Strokes::default();
            strokes.begin((1, 1));
            strokes.finish();
            let completed = strokes.annotations().to_vec();
            strokes.select(Tool::Ellipse);
            strokes.select(tool);
            strokes.begin((20, 20));
            strokes.append((40, 40));
            let changed = apply_edit(Action::PreviousTool, &mut strokes, &mut None);
            assert_eq!(
                changed,
                matches!(tool, Tool::Line | Tool::Arrow | Tool::Rectangle)
            );
            assert!(!strokes.is_active());
            assert_eq!(strokes.tool(), Tool::Ellipse);
            assert_eq!(&strokes.annotations()[..completed.len()], completed);
            if tool == Tool::Pen {
                assert_eq!(strokes.annotations().len(), 2);
                let all = strokes.annotations().to_vec();
                assert!(strokes.undo());
                assert_eq!(strokes.annotations(), completed);
                assert!(strokes.redo());
                assert_eq!(strokes.annotations(), all);
            } else {
                assert_eq!(strokes.annotations(), completed);
            }
            let settled = strokes.annotations().to_vec();
            apply_edit(Action::PreviousTool, &mut strokes, &mut None);
            assert_eq!(strokes.tool(), tool);
            assert_eq!(strokes.annotations(), settled);
        }
        let mut strokes = Strokes::default();
        let mut trail = Trail::default();
        apply_menu_tool(MenuTool::Arrow, &mut strokes, &mut trail, &mut None);
        apply_menu_tool(MenuTool::TemporaryPen, &mut strokes, &mut trail, &mut None);
        trail.begin((10, 10), strokes.style(), Duration::ZERO);
        trail.append((20, 20), Duration::from_millis(100));
        let parts = trail.annotations().cloned().collect::<Vec<_>>();
        assert!(interrupts_trail(&Action::PreviousTool, &strokes));
        trail.finish(); // Same pre-edit routing as a drawing hotkey in the main loop.
        apply_edit(Action::PreviousTool, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Arrow head-first");
        assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), parts);
        assert!(!trail.is_active());
        apply_edit(Action::PreviousTool, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Temporary pen");
    }

    #[test]
    fn pen_shortcuts_preserve_same_variant_gestures_and_settle_switches() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 1));
        strokes.append((10, 10));
        assert!(!interrupts_trail(&Action::Pen, &strokes));
        assert!(!apply_edit(Action::Pen, &mut strokes, &mut None));
        assert!(strokes.is_active());
        assert!(interrupts_trail(&Action::TemporaryPen, &strokes));
        apply_edit(Action::TemporaryPen, &mut strokes, &mut None);
        assert!(!strokes.is_active());
        assert_eq!(strokes.annotations().len(), 1);
        let permanent = strokes.annotations().to_vec();
        let mut trail = Trail::default();
        trail.begin((20, 20), strokes.style(), Duration::ZERO);
        trail.append((30, 30), Duration::from_millis(100));
        let parts = trail.annotations().cloned().collect::<Vec<_>>();
        assert!(!interrupts_trail(&Action::TemporaryPen, &strokes));
        apply_edit(Action::TemporaryPen, &mut strokes, &mut None);
        assert!(trail.is_active());
        assert!(interrupts_trail(&Action::Pen, &strokes));
        trail.finish();
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(trail.annotations().cloned().collect::<Vec<_>>(), parts);
        assert_eq!(strokes.annotations(), permanent);
        assert!(!trail.is_active());
        apply_edit(Action::PreviousTool, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Temporary pen");
        apply_edit(Action::Line, &mut strokes, &mut None);
        apply_edit(Action::TemporaryPen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Temporary pen");
        apply_edit(Action::Line, &mut strokes, &mut None);
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Pen");
    }

    #[test]
    fn repeated_tool_keys_only_toggle_arrow_direction() {
        let mut strokes = Strokes::default();
        apply_edit(Action::Line, &mut strokes, &mut None);
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Pen");
        apply_edit(Action::Pen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Pen");
        apply_edit(Action::TemporaryPen, &mut strokes, &mut None);
        assert_eq!(strokes.tool_label(), "Temporary pen");
        apply_edit(Action::TemporaryPen, &mut strokes, &mut None);
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
