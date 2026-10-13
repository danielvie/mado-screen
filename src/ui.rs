use crate::strokes::{Style, Tool};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_SETLIMITTEXT, EM_SETSEL},
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::{Error, PCWSTR, Result, w},
};

const UI_EVENT: u32 = WM_APP + 40;
const APPLY: usize = 1;
const CANCEL: usize = 2;
const PRESET: usize = 10;
const PRESETS: [(&str, u32); 5] = [
    ("&1 Blue", 0xff3b82f6),
    ("&2 Orange", 0xfff97316),
    ("&3 Red", 0xffef4444),
    ("&4 Green", 0xff22c55e),
    ("&5 White", 0xffffffff),
];
const PICKER_WIDTH: i32 = 350;
const PICKER_HEIGHT: i32 = 230;

#[derive(Debug, PartialEq, Eq)]
pub enum UiEvent {
    Color(u32),
    Closed,
}

// Each instance owns its classes, so independent instances cannot unregister each other.
struct WindowClass {
    name: Vec<u16>,
    instance: HINSTANCE,
}

impl WindowClass {
    fn new(instance: HINSTANCE, kind: &str, proc: WNDPROC) -> Result<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = wide(&format!(
            "Mado{kind}{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let class = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: instance,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
            hbrBackground: unsafe { GetSysColorBrush(COLOR_BTNFACE) },
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(Error::from_thread());
        }
        Ok(Self { name, instance })
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterClassW(PCWSTR(self.name.as_ptr()), Some(self.instance));
        }
    }
}

struct State {
    owner: HWND,
    status: Cell<HWND>,
    picker: Cell<HWND>,
    previous: Cell<HWND>,
    edits: Cell<[HWND; 3]>,
    hint: Cell<HWND>,
    text: RefCell<String>,
    color: Cell<u32>,
}

pub struct DesktopUi {
    state: Box<State>,
    status_class: WindowClass,
    picker_class: WindowClass,
    // Win32 window destruction and keyboard routing must stay on the creating thread.
    _thread: PhantomData<Rc<()>>,
}

impl DesktopUi {
    pub fn new(owner: HWND, width: i32, height: i32) -> Result<Self> {
        if width <= 0 || height <= 0 {
            return Err(Error::new(E_INVALIDARG, "Invalid desktop dimensions"));
        }
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let ui = Self {
            state: Box::new(State {
                owner,
                status: Cell::new(HWND::default()),
                picker: Cell::new(HWND::default()),
                previous: Cell::new(HWND::default()),
                edits: Cell::new([HWND::default(); 3]),
                hint: Cell::new(HWND::default()),
                text: RefCell::new(String::new()),
                color: Cell::new(Style::default().color),
            }),
            status_class: WindowClass::new(instance, "Status", Some(status_proc))?,
            picker_class: WindowClass::new(instance, "Picker", Some(picker_proc))?,
            _thread: PhantomData,
        };
        unsafe {
            // Layered + TRANSPARENT passes mouse events to windows underneath, even
            // across threads. NOACTIVATE alone does not provide click-through.
            // https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW,
                PCWSTR(ui.status_class.name.as_ptr()),
                w!("Mado status"),
                WS_POPUP,
                (width - 330).max(0),
                16.min((height - 44).max(0)),
                314.min(width),
                44.min(height),
                Some(owner),
                None,
                Some(instance),
                Some(ui.state.as_ref() as *const State as _),
            )?;
            ui.state.status.set(hwnd);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 245, LWA_ALPHA)?;
            ui.update(false, Tool::Pen, Style::default())?;
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        Ok(ui)
    }

    pub fn update(&self, drawing: bool, tool: Tool, style: Style) -> Result<()> {
        let mode = if drawing { "Drawing" } else { "Click-through" };
        *self.state.text.borrow_mut() = format!("{mode} | {tool:?} | {} px", style.thickness);
        self.state.color.set(style.color);
        unsafe { InvalidateRect(Some(self.state.status.get()), None, true).ok() }
    }

    pub fn open_picker(&self, color: u32) -> Result<()> {
        if self.is_picker_open() {
            return Ok(());
        }
        unsafe {
            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor)?;
            // rcWork excludes taskbars. NEAREST handles cursor positions outside a monitor.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfrompoint
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info).ok()?;
            let (x, y, width, height) =
                picker_rect(cursor, info.rcWork, (PICKER_WIDTH, PICKER_HEIGHT));
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
                PCWSTR(self.picker_class.name.as_ptr()),
                w!("Mado RGB color"),
                WS_POPUP | WS_CAPTION | WS_SYSMENU,
                x,
                y,
                width,
                height,
                Some(self.state.owner),
                None,
                Some(self.picker_class.instance),
                Some(self.state.as_ref() as *const State as _),
            )?;
            self.state.picker.set(hwnd);
            // On any child-construction error destroy the complete partial child tree.
            if let Err(error) = self
                .size_picker(hwnd, cursor, info.rcWork)
                .and_then(|()| self.build_picker(hwnd, color))
            {
                self.close_picker();
                return Err(error);
            }
            self.state.previous.set(GetForegroundWindow());
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            if GetForegroundWindow() == hwnd {
                let edit = self.state.edits.get()[0];
                let _ = SetFocus(Some(edit));
                SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
        }
        Ok(())
    }

    fn size_picker(&self, hwnd: HWND, cursor: POINT, work: RECT) -> Result<()> {
        unsafe {
            // Reserve the client area after measuring this hidden window's monitor DPI.
            // A fixed outer height can clip Apply/Cancel under a scaled title bar.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-adjustwindowrectexfordpi
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: PICKER_WIDTH,
                bottom: PICKER_HEIGHT,
            };
            AdjustWindowRectExForDpi(
                &mut rect,
                WS_POPUP | WS_CAPTION | WS_SYSMENU,
                false,
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
                GetDpiForWindow(hwnd),
            )?;
            let (x, y, width, height) = picker_rect(
                cursor,
                work,
                (rect.right - rect.left, rect.bottom - rect.top),
            );
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        }
    }

    fn build_picker(&self, hwnd: HWND, color: u32) -> Result<()> {
        for (i, (label, _)) in PRESETS.iter().enumerate() {
            self.child(
                hwnd,
                w!("BUTTON"),
                label,
                WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                (12 + i as i32 * 64, 12, 62, 26),
                PRESET + i,
            )?;
        }
        let mut edits = [HWND::default(); 3];
        for (i, label) in ["&Red", "&Green", "&Blue"].iter().enumerate() {
            let x = 12 + i as i32 * 106;
            self.child(
                hwnd,
                w!("STATIC"),
                label,
                WINDOW_STYLE(0),
                (x, 54, 96, 20),
                30 + i,
            )?;
            edits[i] = self.child(
                hwnd,
                w!("EDIT"),
                "",
                WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_NUMBER as u32 | ES_AUTOHSCROLL as u32),
                (x, 76, 96, 26),
                40 + i,
            )?;
            unsafe {
                SendMessageW(edits[i], EM_SETLIMITTEXT, Some(WPARAM(3)), None);
            }
        }
        self.state.edits.set(edits);
        self.state.set_rgb(color)?;
        self.state.hint.set(self.child(
            hwnd,
            w!("STATIC"),
            "RGB values: 0 to 255",
            WINDOW_STYLE(0),
            (12, 110, 318, 24),
            50,
        )?);
        self.child(
            hwnd,
            w!("BUTTON"),
            "&Apply",
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            (150, 148, 84, 28),
            APPLY,
        )?;
        self.child(
            hwnd,
            w!("BUTTON"),
            "&Cancel",
            WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            (242, 148, 84, 28),
            CANCEL,
        )?;
        Ok(())
    }

    fn child(
        &self,
        parent: HWND,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        rect: (i32, i32, i32, i32),
        id: usize,
    ) -> Result<HWND> {
        let text = wide(text);
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                Some(parent),
                Some(HMENU(id as _)),
                Some(self.picker_class.instance),
                None,
            )?;
            // Stock fonts are borrowed, never deleted.
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
                Some(LPARAM(1)),
            );
            Ok(hwnd)
        }
    }

    pub fn close_picker(&self) {
        self.state.close_picker();
    }
    pub fn is_picker_open(&self) -> bool {
        !self.state.picker.get().is_invalid()
    }

    pub fn handle_message(&self, message: &MSG) -> bool {
        let picker = self.state.picker.get();
        if picker.is_invalid() {
            return false;
        }
        unsafe {
            if message.hwnd != picker && !IsChild(picker, message.hwnd).as_bool() {
                return false;
            }
            if message.message == WM_KEYDOWN {
                if message.wParam.0 == VK_ESCAPE.0 as usize {
                    self.close_picker();
                    return true;
                }
                if message.wParam.0 == VK_RETURN.0 as usize {
                    // Enter on a preset/Cancel must click that button, not apply RGB.
                    let id = GetDlgCtrlID(message.hwnd) as usize;
                    self.state.command(
                        if id == CANCEL || (PRESET..PRESET + PRESETS.len()).contains(&id) {
                            id
                        } else {
                            APPLY
                        },
                    );
                    return true;
                }
            }
            // Also works for non-dialog windows with controls. Consumed messages
            // must not be translated/dispatched again by main.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-isdialogmessagew
            IsDialogMessageW(picker, message).as_bool()
        }
    }

    pub fn event(&self, message: &MSG) -> Option<UiEvent> {
        decode_event(message, self.state.owner)
    }
}

impl Drop for DesktopUi {
    fn drop(&mut self) {
        self.close_picker();
        unsafe {
            for hwnd in [self.state.picker.get(), self.state.status.get()] {
                if !hwnd.is_invalid() {
                    // Detach before releasing Box, including a failed DestroyWindow.
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }
}

impl State {
    fn post(&self, event: UiEvent) {
        let (value, kind) = match event {
            UiEvent::Color(color) => (color as usize, 1),
            UiEvent::Closed => (0, 2),
        };
        // No pointers in queued messages: dispatch may outlive either native window.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew
        unsafe {
            let _ = PostMessageW(Some(self.owner), UI_EVENT, WPARAM(value), LPARAM(kind));
        }
    }

    fn set_rgb(&self, color: u32) -> Result<()> {
        for (hwnd, shift) in self.edits.get().iter().zip([16, 8, 0]) {
            let text = wide(&((color >> shift) & 255).to_string());
            unsafe {
                SetWindowTextW(*hwnd, PCWSTR(text.as_ptr()))?;
            }
        }
        Ok(())
    }

    fn command(&self, id: usize) {
        if id == CANCEL {
            self.close_picker();
        } else if id == APPLY {
            let mut values = [0_u8; 3];
            for (i, hwnd) in self.edits.get().iter().enumerate() {
                let mut text = [0_u16; 5];
                let len = unsafe { GetWindowTextW(*hwnd, &mut text) } as usize;
                // ES_NUMBER still permits nonnumeric pasted text. Validate on Apply.
                // https://learn.microsoft.com/en-us/windows/win32/controls/edit-control-styles
                let value = parse_channel(&String::from_utf16_lossy(&text[..len]));
                let Some(value) = value else {
                    unsafe {
                        let _ =
                            SetWindowTextW(self.hint.get(), w!("Use whole numbers from 0 to 255."));
                        let _ = SetFocus(Some(*hwnd));
                        SendMessageW(*hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                    }
                    return;
                };
                values[i] = value;
            }
            self.post(UiEvent::Color(rgb(values)));
            self.close_picker();
        } else if let Some((_, color)) = id.checked_sub(PRESET).and_then(|i| PRESETS.get(i))
            && self.set_rgb(*color).is_ok()
        {
            unsafe {
                let _ = SetWindowTextW(self.hint.get(), w!("RGB values: 0 to 255"));
            }
        }
    }

    fn close_picker(&self) {
        let hwnd = self.picker.get();
        if hwnd.is_invalid() {
            return;
        }
        unsafe {
            // Restore only while our picker is still foreground. Never steal focus
            // back from another app the user selected; restoration is best-effort.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow
            if GetForegroundWindow() == hwnd && !self.previous.get().is_invalid() {
                let _ = SetForegroundWindow(self.previous.get());
            }
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn decode_event(message: &MSG, owner: HWND) -> Option<UiEvent> {
    if message.hwnd != owner || message.message != UI_EVENT {
        return None;
    }
    match message.lParam.0 {
        1 if message.wParam.0 <= u32::MAX as usize => Some(UiEvent::Color(message.wParam.0 as u32)),
        2 => Some(UiEvent::Closed),
        _ => None,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn parse_channel(text: &str) -> Option<u8> {
    if text.is_empty() || text.len() > 3 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
fn rgb([r, g, b]: [u8; 3]) -> u32 {
    0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
fn picker_rect(cursor: POINT, work: RECT, size: (i32, i32)) -> (i32, i32, i32, i32) {
    let width = size.0.min((work.right - work.left).max(1));
    let height = size.1.min((work.bottom - work.top).max(1));
    (
        cursor.x.clamp(work.left, work.right - width),
        cursor.y.clamp(work.top, work.bottom - height),
        width,
        height,
    )
}

// The Box address never changes. Callbacks borrow shared State with interior
// mutability, so synchronous/reentrant Win32 calls do not alias a Rust &mut.
// https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw
unsafe fn state_for<'a>(hwnd: HWND, message: u32, l: LPARAM) -> Option<&'a State> {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(l.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref()
    }
}

unsafe extern "system" fn picker_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if let Some(state) = state_for(hwnd, message, l) {
            match message {
                WM_COMMAND if ((w.0 >> 16) & 0xffff) == BN_CLICKED as usize => {
                    state.command(w.0 & 0xffff);
                    return LRESULT(0);
                }
                WM_CLOSE => {
                    state.close_picker();
                    return LRESULT(0);
                }
                WM_NCDESTROY => {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    if state.picker.replace(HWND::default()) == hwnd {
                        state.edits.set([HWND::default(); 3]);
                        state.hint.set(HWND::default());
                        state.previous.set(HWND::default());
                        state.post(UiEvent::Closed);
                    }
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, message, w, l)
    }
}

unsafe extern "system" fn status_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if message == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if message == WM_NCHITTEST {
            return LRESULT(HTTRANSPARENT as isize);
        }
        if let Some(state) = state_for(hwnd, message, l) {
            match message {
                WM_PAINT => {
                    let mut paint = PAINTSTRUCT::default();
                    let dc = BeginPaint(hwnd, &mut paint);
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    FillRect(dc, &rect, GetSysColorBrush(COLOR_BTNFACE));
                    let color = state.color.get();
                    let brush = CreateSolidBrush(COLORREF(
                        ((color >> 16) & 255) | (color & 0xff00) | ((color & 255) << 16),
                    ));
                    if !brush.is_invalid() {
                        FillRect(
                            dc,
                            &RECT {
                                left: 10,
                                top: 12,
                                right: 30,
                                bottom: 32,
                            },
                            brush,
                        );
                        let _ = DeleteObject(brush.into());
                    }
                    let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
                    SetBkMode(dc, TRANSPARENT);
                    SetTextColor(dc, COLORREF(GetSysColor(COLOR_BTNTEXT)));
                    rect.left = 40;
                    rect.right -= 8;
                    let mut text: Vec<u16> = state.text.borrow().encode_utf16().collect();
                    DrawTextW(
                        dc,
                        &mut text,
                        &mut rect,
                        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                    );
                    SelectObject(dc, old_font);
                    let _ = EndPaint(hwnd, &paint);
                    return LRESULT(0);
                }
                WM_NCDESTROY => {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    state.status.set(HWND::default());
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, message, w, l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_ui_events_are_value_only_and_owner_filtered() {
        let owner = HWND::default();
        let mut message = MSG {
            hwnd: owner,
            message: UI_EVENT,
            wParam: WPARAM(0xff123456),
            lParam: LPARAM(1),
            ..Default::default()
        };
        assert_eq!(
            decode_event(&message, owner),
            Some(UiEvent::Color(0xff123456))
        );
        message.lParam = LPARAM(2);
        assert_eq!(decode_event(&message, owner), Some(UiEvent::Closed));
        message.lParam = LPARAM(3);
        assert_eq!(decode_event(&message, owner), None);
        message.message = WM_APP;
        assert_eq!(decode_event(&message, owner), None);
        message.message = UI_EVENT;
        message.hwnd = HWND(std::ptr::dangling_mut());
        assert_eq!(decode_event(&message, owner), None);
    }

    #[test]
    fn status_rejects_activation_and_hit_testing_without_native_windows() {
        unsafe {
            assert_eq!(
                status_proc(HWND::default(), WM_MOUSEACTIVATE, WPARAM(0), LPARAM(0)).0,
                MA_NOACTIVATE as isize
            );
            assert_eq!(
                status_proc(HWND::default(), WM_NCHITTEST, WPARAM(0), LPARAM(0)).0,
                HTTRANSPARENT as isize
            );
        }
    }

    #[test]
    fn channel_validation_and_argb_are_exact() {
        for value in 0..=255_u16 {
            assert_eq!(parse_channel(&value.to_string()), Some(value as u8));
        }
        for text in [
            "", "256", "999", "1000", "-1", "+1", " 1", "1 ", "1.0", "a", "１２",
        ] {
            assert_eq!(parse_channel(text), None, "{text}");
        }
        assert_eq!(parse_channel("007"), Some(7));
        assert_eq!(rgb([18, 52, 86]), 0xff123456);
        assert_eq!(rgb([0, 0, 0]), 0xff000000);
        assert_eq!(rgb([255, 255, 255]), 0xffffffff);
    }

    #[test]
    fn picker_stays_in_nearest_work_area_including_negative_coordinates() {
        for work in [
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            RECT {
                left: -1920,
                top: -1080,
                right: 0,
                bottom: -40,
            },
            RECT {
                left: 10,
                top: 20,
                right: 110,
                bottom: 100,
            },
        ] {
            for cursor in [
                POINT { x: -3000, y: -3000 },
                POINT { x: 40, y: 50 },
                POINT { x: 3000, y: 3000 },
            ] {
                let (x, y, width, height) =
                    picker_rect(cursor, work, (PICKER_WIDTH, PICKER_HEIGHT));
                assert!(x >= work.left && y >= work.top);
                assert!(x + width <= work.right && y + height <= work.bottom);
                assert!(width > 0 && height > 0);
            }
        }
        assert_eq!(
            picker_rect(
                POINT { x: 50, y: 60 },
                RECT {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1040
                },
                (PICKER_WIDTH, PICKER_HEIGHT)
            ),
            (50, 60, 350, 230)
        );
    }
}
