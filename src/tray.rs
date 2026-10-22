use crate::controls::Shortcuts;
use crate::ui::{MenuLayout, MenuTool, WindowClass, tool_icon, tray_icon_pixels};
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::{CreateBitmap, DeleteObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{Error, PCWSTR, Result, w},
};

const TRAY_EVENT: u32 = WM_APP + 41;
const TRAY_CALLBACK: u32 = WM_APP + 42;
const MENU_TOGGLE: usize = 1;
const MENU_CLEAR: usize = 2;
const MENU_QUIT: usize = 3;
const MENU_PANEL: usize = 4;
const MENU_RELOAD: usize = 5;
// One id per MenuLayout, in MenuLayout::ALL order.
const MENU_LAYOUT: usize = 6;

#[derive(Debug, PartialEq, Eq)]
pub enum TrayEvent {
    Toggle,
    Clear,
    Quit,
    Panel,
    Reload,
    Layout(MenuLayout),
}

struct State {
    owner: HWND,
    hwnd: Cell<HWND>,
    icon: Cell<HICON>,
    // Color is 0 in click-through; tool changes still update the dimmed icon.
    key: Cell<(bool, u32, MenuTool, bool)>,
    panel: Cell<bool>,
    shortcuts: Cell<Shortcuts>,
    layout: Cell<MenuLayout>,
    taskbar_created: u32,
}

impl State {
    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd.get(),
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: TRAY_CALLBACK,
            hIcon: self.icon.get(),
            ..Default::default()
        };
        let (drawing, _, tool, tail_first) = self.key.get();
        let mode = if drawing { "Drawing" } else { "Click-through" };
        let tool = match tool {
            MenuTool::Pen => "Pen",
            MenuTool::TemporaryPen => "Temporary pen",
            MenuTool::Line => "Line",
            MenuTool::Arrow if tail_first => "Arrow tail-first",
            MenuTool::Arrow => "Arrow head-first",
            MenuTool::Rectangle => "Rectangle",
            MenuTool::Ellipse => "Ellipse",
            MenuTool::Eraser => "Eraser",
        };
        let tip = format!("Mado Screen: {mode} | {tool}");
        for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16()) {
            *slot = unit;
        }
        data
    }

    fn add(&self) {
        let mut data = self.data();
        // Failure is not fatal: without Explorer, TaskbarCreated adds the icon later.
        // https://learn.microsoft.com/en-us/windows/win32/shell/taskbar#taskbar-creation-notification
        unsafe {
            if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
        }
    }

    fn post(&self, id: usize) {
        unsafe {
            let _ = PostMessageW(Some(self.owner), TRAY_EVENT, WPARAM(id), LPARAM(0));
        }
    }

    fn show_menu(&self, x: i32, y: i32) {
        let hwnd = self.hwnd.get();
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let check = if self.key.get().0 {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            };
            let _ = AppendMenuW(
                menu,
                MF_STRING | check,
                MENU_TOGGLE,
                PCWSTR(
                    menu_label(
                        "&Drawing mode",
                        &self.shortcuts.get().binding("toggle").label(),
                    )
                    .as_ptr(),
                ),
            );
            let panel = if self.panel.get() {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            };
            let _ = AppendMenuW(
                menu,
                MF_STRING | panel,
                MENU_PANEL,
                PCWSTR(
                    menu_label(
                        "Show floating &menu",
                        &format!("{} (drawing)", self.shortcuts.get().binding("menu").label()),
                    )
                    .as_ptr(),
                ),
            );
            // DestroyMenu below also destroys this attached submenu.
            if let Ok(layouts) = CreatePopupMenu() {
                for (layout, name) in MenuLayout::ALL.into_iter().zip([
                    w!("&Panel"),
                    w!("Edge &rail"),
                    w!("Radial &ring\tRight-click (drawing)"),
                ]) {
                    let _ = AppendMenuW(layouts, MF_STRING, MENU_LAYOUT + layout as usize, name);
                }
                let _ = CheckMenuRadioItem(
                    layouts,
                    MENU_LAYOUT as u32,
                    (MENU_LAYOUT + MenuLayout::ALL.len() - 1) as u32,
                    (MENU_LAYOUT + self.layout.get() as usize) as u32,
                    MF_BYCOMMAND.0,
                );
                if AppendMenuW(menu, MF_POPUP, layouts.0 as usize, w!("Menu &layout")).is_err() {
                    let _ = DestroyMenu(layouts);
                }
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                MENU_CLEAR,
                PCWSTR(
                    menu_label(
                        "&Clear all ink",
                        &self.shortcuts.get().binding("clear").label(),
                    )
                    .as_ptr(),
                ),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, MENU_RELOAD, w!("&Reload config"));
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                MENU_QUIT,
                PCWSTR(menu_label("&Quit", &self.shortcuts.get().binding("quit").label()).as_ptr()),
            );
            let align = if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 {
                TPM_RIGHTALIGN
            } else {
                TPM_LEFTALIGN
            };
            // Foreground first and WM_NULL after, so clicking elsewhere dismisses the menu.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackpopupmenu#remarks
            let _ = SetForegroundWindow(hwnd);
            let id = TrackPopupMenuEx(
                menu,
                (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | align).0,
                x,
                y,
                hwnd,
                None,
            )
            .0;
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            if id > 0 {
                self.post(id as usize);
            }
        }
    }
}

pub struct Tray {
    state: Box<State>,
    class: WindowClass,
}

impl Tray {
    pub fn new(owner: HWND) -> Result<Self> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let tray = Self {
            state: Box::new(State {
                owner,
                hwnd: Cell::new(HWND::default()),
                icon: Cell::new(create_icon(false, MenuTool::Pen, false, 0)?),
                key: Cell::new((false, 0, MenuTool::Pen, false)),
                panel: Cell::new(true),
                shortcuts: Cell::new(Shortcuts::default()),
                layout: Cell::new(MenuLayout::default()),
                taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            }),
            class: WindowClass::new(instance, "Tray", Some(tray_proc))?,
        };
        unsafe {
            // Hidden top-level, not message-only: only top-level windows receive the
            // TaskbarCreated broadcast that re-adds the icon after Explorer restarts.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                PCWSTR(tray.class.name.as_ptr()),
                w!("Mado Screen tray"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some(tray.state.as_ref() as *const State as _),
            )?;
            tray.state.hwnd.set(hwnd);
        }
        tray.state.add();
        Ok(tray)
    }

    pub fn update(&self, drawing: bool, tool: &str, color: u32) -> Result<()> {
        let key = (
            drawing,
            if drawing { color } else { 0 },
            tool_icon(tool),
            tool == "Arrow tail-first",
        );
        if self.state.key.get() == key {
            return Ok(());
        }
        let old = self
            .state
            .icon
            .replace(create_icon(key.0, key.2, key.3, key.1)?);
        self.state.key.set(key);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.state.data());
            let _ = DestroyIcon(old);
        }
        Ok(())
    }

    // Only drives the menu checkmark; the owner shows or hides the panel itself.
    pub fn set_panel_visible(&self, visible: bool) {
        self.state.panel.set(visible);
    }

    pub fn set_shortcuts(&self, shortcuts: Shortcuts) {
        self.state.shortcuts.set(shortcuts);
    }

    pub fn notify_reload(&self, result: std::result::Result<&str, &str>) {
        let (title, message, flags) = match result {
            Ok(message) => ("Config reloaded", message, NIIF_INFO),
            Err(message) => ("Config reload failed", message, NIIF_ERROR),
        };
        self.notify(title, message, flags);
    }

    pub fn notify_zoom_error(&self, message: &str) {
        eprintln!("Zoom stopped: {message}");
        self.notify("Zoom unavailable", message, NIIF_ERROR);
    }

    fn notify(&self, title: &str, message: &str, flags: NOTIFY_ICON_INFOTIP_FLAGS) {
        let mut data = self.state.data();
        data.uFlags = NIF_INFO;
        for (slot, unit) in data
            .szInfoTitle
            .iter_mut()
            .take(63)
            .zip(title.encode_utf16())
        {
            *slot = unit;
        }
        for (slot, unit) in data.szInfo.iter_mut().take(255).zip(message.encode_utf16()) {
            *slot = unit;
        }
        data.dwInfoFlags = flags;
        // https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    // Only drives the radio mark; the owner applies and saves the layout.
    pub fn set_layout(&self, layout: MenuLayout) {
        self.state.layout.set(layout);
    }

    pub fn event(&self, message: &MSG) -> Option<TrayEvent> {
        decode_event(message, self.state.owner)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.state.data());
            let hwnd = self.state.hwnd.get();
            if !hwnd.is_invalid() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
            }
            let _ = DestroyIcon(self.state.icon.get());
        }
    }
}

fn menu_label(name: &str, shortcut: &str) -> Vec<u16> {
    format!("{name}\t{shortcut}")
        .encode_utf16()
        .chain([0])
        .collect()
}

fn decode_event(message: &MSG, owner: HWND) -> Option<TrayEvent> {
    if message.hwnd != owner || message.message != TRAY_EVENT {
        return None;
    }
    match message.wParam.0 {
        MENU_TOGGLE => Some(TrayEvent::Toggle),
        MENU_CLEAR => Some(TrayEvent::Clear),
        MENU_QUIT => Some(TrayEvent::Quit),
        MENU_PANEL => Some(TrayEvent::Panel),
        MENU_RELOAD => Some(TrayEvent::Reload),
        id => MenuLayout::ALL
            .get(id.wrapping_sub(MENU_LAYOUT))
            .map(|layout| TrayEvent::Layout(*layout)),
    }
}

// ponytail: sized once from SM_CXSMICON (system DPI). A later DPI change keeps the
// startup size until restart, like the rest of the monitor layout.
fn create_icon(drawing: bool, tool: MenuTool, tail_first: bool, color: u32) -> Result<HICON> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let pixels = tray_icon_pixels(size, drawing, tool, tail_first, color)?;
    // All-zero AND mask: the 32-bit color bitmap's alpha supplies the shape.
    let mask_bits = vec![0_u8; ((size as usize).div_ceil(16) * 2) * size as usize];
    unsafe {
        let color = CreateBitmap(size, size, 1, 32, Some(pixels.as_ptr().cast()));
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr().cast()));
        let icon = if color.is_invalid() || mask.is_invalid() {
            Err(Error::from_thread())
        } else {
            CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: color,
            })
        };
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

unsafe extern "system" fn tray_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(l.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if let Some(state) = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() {
            if message == TRAY_CALLBACK {
                // Version 4: LOWORD(lParam) is the event; wParam holds the anchor point.
                // https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw
                match l.0 as u32 & 0xffff {
                    NIN_SELECT => state.post(MENU_TOGGLE),
                    WM_CONTEXTMENU => {
                        state.show_menu(w.0 as u16 as i16 as i32, (w.0 >> 16) as u16 as i16 as i32)
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            if message == state.taskbar_created && message != 0 {
                state.add();
                return LRESULT(0);
            }
        }
        DefWindowProcW(hwnd, message, w, l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::LibraryLoader::{
        FindResourceW, LoadResource, LockResource, SizeofResource,
    };

    #[test]
    #[ignore = "regenerates the checked-in executable icon"]
    fn generate_executable_icon() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let sizes = [16_u32, 20, 24, 32, 48, 64, 128, 256];
        let mut directory = vec![0, 0, 1, 0, sizes.len() as u8, 0];
        let mut images = Vec::new();
        for size in sizes {
            let pixels = tray_icon_pixels(size as i32, false, MenuTool::Pen, false, 0)?;
            let stride = size.div_ceil(32) * 4;
            let length = 40 + size * size * 4 + stride * size;
            let offset = 6 + sizes.len() as u32 * 16 + images.len() as u32;
            directory.extend_from_slice(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0]);
            directory.extend_from_slice(&length.to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            // ICO DIBs store bottom-up BGRA rows, followed by a DWORD-aligned AND mask.
            // https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapinfoheader
            for value in [
                40,
                size,
                size * 2,
                0x0020_0001,
                0,
                size * size * 4,
                0,
                0,
                0,
                0,
            ] {
                images.extend_from_slice(&value.to_le_bytes());
            }
            for y in (0..size).rev() {
                for x in 0..size {
                    images.extend_from_slice(&pixels[(y * size + x) as usize].to_le_bytes());
                }
            }
            for y in (0..size).rev() {
                let mut row = vec![0_u8; stride as usize];
                for x in 0..size {
                    if pixels[(y * size + x) as usize] >> 24 == 0 {
                        row[(x / 8) as usize] |= 0x80 >> (x % 8);
                    }
                }
                images.extend_from_slice(&row);
            }
        }
        directory.extend_from_slice(&images);
        std::fs::write("assets/mado-screen.ico", directory)?;
        Ok(())
    }

    #[test]
    fn executable_icon_matches_default_tray() -> Result<()> {
        let icon = include_bytes!("../assets/mado-screen.ico");
        let word = |at| u16::from_le_bytes(icon[at..at + 2].try_into().unwrap());
        let dword = |at| u32::from_le_bytes(icon[at..at + 4].try_into().unwrap());
        assert_eq!(&icon[..6], &[0, 0, 1, 0, 8, 0]);
        let module = unsafe { GetModuleHandleW(None)? };
        // Inspect the actual linked resources, not just the source ICO.
        // https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-findresourcew
        let resource = |id, kind| -> Result<&[u8]> {
            unsafe {
                let info = FindResourceW(Some(module), PCWSTR(id as _), kind);
                assert!(!info.is_invalid(), "missing executable icon resource {id}");
                let size = SizeofResource(Some(module), info) as usize;
                let data = LockResource(LoadResource(Some(module), info)?);
                assert!(!data.is_null());
                Ok(std::slice::from_raw_parts(data.cast(), size))
            }
        };
        let group = resource(1_usize, RT_GROUP_ICON)?;
        assert_eq!(&group[..6], &icon[..6]);
        for entry in 0..word(4) as usize {
            let at = 6 + entry * 16;
            let size = if icon[at] == 0 { 256 } else { icon[at] as i32 };
            let length = dword(at + 8) as usize;
            let offset = dword(at + 12) as usize;
            let image = &icon[offset..offset + length];
            assert_eq!(dword(offset), 40);
            assert_eq!(dword(offset + 4), size as u32);
            assert_eq!(dword(offset + 8), size as u32 * 2);
            assert_eq!(word(offset + 12), 1);
            assert_eq!(word(offset + 14), 32);
            let pixels = tray_icon_pixels(size, false, MenuTool::Pen, false, 0)?;
            for y in 0..size {
                for x in 0..size {
                    let stored = dword(offset + 40 + ((size - 1 - y) * size + x) as usize * 4);
                    assert_eq!(
                        stored,
                        pixels[(y * size + x) as usize],
                        "{size}px at {x},{y}"
                    );
                }
            }
            let group_at = 6 + entry * 14;
            assert_eq!(&group[group_at..group_at + 12], &icon[at..at + 12]);
            let id = u16::from_le_bytes(group[group_at + 12..group_at + 14].try_into().unwrap());
            assert_eq!(resource(id as usize, RT_ICON)?, image);
        }
        Ok(())
    }

    #[test]
    fn tray_menu_labels_follow_remapped_shortcuts() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set(
            "toggle",
            crate::controls::Shortcut::parse("Alt+F10").unwrap(),
        );
        let label = menu_label("&Drawing mode", &shortcuts.binding("toggle").label());
        assert_eq!(String::from_utf16_lossy(&label), "&Drawing mode\tAlt+F10\0");
        assert_eq!(
            String::from_utf16_lossy(&menu_label("&Quit", "Shift+Q")),
            "&Quit\tShift+Q\0"
        );
    }

    #[test]
    fn tray_menu_ids_decode_only_for_owner() {
        let owner = HWND(1 as _);
        let mut message = MSG {
            hwnd: owner,
            message: TRAY_EVENT,
            ..Default::default()
        };
        for (id, event) in [
            (MENU_TOGGLE, TrayEvent::Toggle),
            (MENU_CLEAR, TrayEvent::Clear),
            (MENU_QUIT, TrayEvent::Quit),
            (MENU_PANEL, TrayEvent::Panel),
            (MENU_RELOAD, TrayEvent::Reload),
            (MENU_LAYOUT, TrayEvent::Layout(MenuLayout::Panel)),
            (MENU_LAYOUT + 1, TrayEvent::Layout(MenuLayout::Rail)),
            (MENU_LAYOUT + 2, TrayEvent::Layout(MenuLayout::Radial)),
        ] {
            message.wParam = WPARAM(id);
            assert_eq!(decode_event(&message, owner), Some(event));
        }
        for id in [0, MENU_LAYOUT + 3, usize::MAX] {
            message.wParam = WPARAM(id);
            assert_eq!(decode_event(&message, owner), None);
        }
        message.wParam = WPARAM(MENU_QUIT);
        assert_eq!(decode_event(&message, HWND::default()), None);
        message.message = TRAY_CALLBACK;
        assert_eq!(decode_event(&message, owner), None);
    }

    #[test]
    fn tray_arrow_direction_changes_glyph_without_changing_dot_or_shape() -> Result<()> {
        for size in [16, 20, 24, 32] {
            for drawing in [false, true] {
                let head = tray_icon_pixels(size, drawing, MenuTool::Arrow, false, 0xff0085dc)?;
                let tail = tray_icon_pixels(size, drawing, MenuTool::Arrow, true, 0xff0085dc)?;
                assert_ne!(head, tail, "Arrow variants match at {size}px");
                assert!(head.iter().zip(&tail).all(|(a, b)| a >> 24 == b >> 24));
                let u = size as f64 / 16.0;
                let dot = ((12.5 * u) as i32 * size + (3.5 * u) as i32) as usize;
                assert_eq!(head[dot], tail[dot]);
                create_icon(drawing, MenuTool::Arrow, true, 0xff0085dc)
                    .map(|icon| unsafe { DestroyIcon(icon) })??;
            }
        }
        Ok(())
    }

    #[test]
    fn tray_tools_have_distinct_icons_in_both_modes() -> Result<()> {
        let tools = [
            MenuTool::Pen,
            MenuTool::TemporaryPen,
            MenuTool::Line,
            MenuTool::Arrow,
            MenuTool::Rectangle,
            MenuTool::Ellipse,
            MenuTool::Eraser,
        ];
        for size in [16, 20, 24, 32] {
            for drawing in [false, true] {
                let mut rendered = Vec::new();
                for tool in tools {
                    let pixels = tray_icon_pixels(size, drawing, tool, false, 0xff0085dc)?;
                    assert_eq!(pixels.len(), (size * size) as usize);
                    assert_eq!(pixels[0] >> 24, 0);
                    for (previous, image) in &rendered {
                        assert_ne!(&pixels, image, "{tool:?} matches {previous:?} at {size}px");
                        // Only the glyph changes, not the icon's alpha/shape.
                        assert!(pixels.iter().zip(image).all(|(a, b)| a >> 24 == b >> 24));
                    }
                    let u = size as f64 / 16.0;
                    let dot = ((12.5 * u) as i32 * size + (3.5 * u) as i32) as usize;
                    assert_eq!(pixels[dot], if drawing { 0xff0085dc } else { 0xff191c20 });
                    if !drawing {
                        assert_eq!(
                            pixels,
                            tray_icon_pixels(size, false, tool, false, 0xfff97316)?
                        );
                    }
                    create_icon(drawing, tool, false, 0xff0085dc)
                        .map(|icon| unsafe { DestroyIcon(icon) })??;
                    rendered.push((tool, pixels));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn tray_tooltips_follow_selected_tool_and_mode() {
        let state = State {
            owner: HWND::default(),
            hwnd: Cell::new(HWND::default()),
            icon: Cell::new(HICON::default()),
            key: Cell::new((false, 0, MenuTool::Pen, false)),
            panel: Cell::new(true),
            shortcuts: Cell::new(Shortcuts::default()),
            layout: Cell::new(MenuLayout::default()),
            taskbar_created: 0,
        };
        for (label, tool, name) in [
            ("Pen", MenuTool::Pen, "Pen"),
            ("Temporary pen", MenuTool::TemporaryPen, "Temporary pen"),
            ("Line", MenuTool::Line, "Line"),
            ("Arrow head-first", MenuTool::Arrow, "Arrow head-first"),
            ("Arrow tail-first", MenuTool::Arrow, "Arrow tail-first"),
            ("Rectangle", MenuTool::Rectangle, "Rectangle"),
            ("Ellipse", MenuTool::Ellipse, "Ellipse"),
            ("Eraser", MenuTool::Eraser, "Eraser"),
        ] {
            assert_eq!(tool_icon(label), tool);
            for drawing in [false, true] {
                state
                    .key
                    .set((drawing, 0xff0085dc, tool, label == "Arrow tail-first"));
                let data = state.data();
                let end = data.szTip.iter().position(|&unit| unit == 0).unwrap();
                let tip = String::from_utf16(&data.szTip[..end]).unwrap();
                let mode = if drawing { "Drawing" } else { "Click-through" };
                assert_eq!(tip, format!("Mado Screen: {mode} | {name}"));
            }
        }
    }

    #[test]
    fn tray_icon_shows_ink_dot_and_click_through_ring() -> Result<()> {
        for size in [16, 20, 24, 32] {
            let u = size as f64 / 16.0;
            let px = |x: f64, y: f64| ((y * u) as i32 * size + (x * u) as i32) as usize;
            let on = tray_icon_pixels(size, true, MenuTool::Pen, false, 0xff0085dc)?;
            let off = tray_icon_pixels(size, false, MenuTool::Pen, false, 0xff0085dc)?;
            assert_eq!(on.len(), (size * size) as usize);
            // Top-left corner is outside both tile and dot.
            assert_eq!(on[0] >> 24, 0);
            // Tile interior is opaque graphite in both states.
            assert_eq!(on[px(13.0, 12.0)], 0xff191c20);
            // Dot center: ink while drawing, hollow graphite in click-through.
            assert_eq!(on[px(3.5, 12.5)], 0xff0085dc);
            assert_eq!(off[px(3.5, 12.5)], 0xff191c20);
            // Click-through never depends on the ink color.
            assert_eq!(
                off,
                tray_icon_pixels(size, false, MenuTool::Pen, false, 0xfff97316)?
            );
            create_icon(true, MenuTool::Pen, false, 0xff0085dc)
                .map(|icon| unsafe { DestroyIcon(icon) })??;
        }
        Ok(())
    }
}
