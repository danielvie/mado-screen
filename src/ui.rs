use crate::{
    picker::{Hsv, disk_pixel, hex, parse_hex},
    strokes::{Style, Tool},
};
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
            Controls::{EM_SETLIMITTEXT, EM_SETMARGINS, WM_MOUSELEAVE},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::{Error, PCWSTR, Result, w},
};

const UI_EVENT: u32 = WM_APP + 40;
const APPLY: usize = 1;
const CANCEL: usize = 2;
const CLOSE: usize = 3;
const DISK: usize = 4;
const BRIGHTNESS: usize = 5;
const HEX: usize = 6;
const HINT: usize = 7;
const HEX_LABEL: usize = 8;
const PRESET: usize = 10;
const PRESETS: [(&str, u32); 6] = [
    ("&1 Blue", 0xff3b82f6),
    ("&2 Orange", 0xfff97316),
    ("&3 Red", 0xffef4444),
    ("&4 Green", 0xff22c55e),
    ("&5 White", 0xffffffff),
    ("&6 Black", 0xff000000),
];
const PICKER_WIDTH: i32 = 300;
const PICKER_HEIGHT: i32 = 496;
const BACKGROUND: u32 = 0xffffffff;
const SURFACE: u32 = 0xfff6f8fb;
const BORDER: u32 = 0xffcbd5e1;
const FOREGROUND: u32 = 0xff334155;
const MUTED: u32 = 0xff64748b;
const BLUE: u32 = 0xff2563eb;
const ORANGE: u32 = 0xfff97316;
const PANEL_BACKGROUND: u32 = 0xff333940;
const PANEL_BLUE: u32 = 0xff58a6ef;
const PANEL_WIDTH: i32 = 440;
const PANEL_ROW: i32 = 44;
const PANEL_SAMPLES: i32 = 4;

#[derive(Debug, PartialEq, Eq)]
pub enum UiEvent {
    Color(u32),
    Closed,
    OpenPicker,
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

// Owned GDI objects are never deleted while selected into a DC.
struct GdiObject(HGDIOBJ);
impl Drop for GdiObject {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}
struct DiskCache {
    size: i32,
    value: f64,
    pixels: Rc<Vec<u32>>,
}
struct State {
    owner: HWND,
    status: Cell<HWND>,
    picker: Cell<HWND>,
    previous: Cell<HWND>,
    edit: Cell<HWND>,
    hint: Cell<HWND>,
    disk: Cell<HWND>,
    slider: Cell<HWND>,
    text: RefCell<String>,
    color: Cell<u32>,
    tool: Cell<Tool>,
    expanded: Cell<bool>,
    panel_width: Cell<i32>,
    panel_pixels: RefCell<Vec<u32>>,
    recents: RefCell<Vec<u32>>,
    next_recent: Cell<usize>,
    panel_scale: Cell<f64>,
    panel_monitor: Cell<RECT>,
    tracking: Cell<bool>,
    pressed: Cell<Option<PanelHit>>,
    candidate: Cell<Hsv>,
    valid: Cell<bool>,
    syncing: Cell<bool>,
    drag: Cell<HWND>,
    scale: Cell<f64>,
    font: Cell<HFONT>,
    edit_brush: GdiObject,
    cache: RefCell<Option<DiskCache>>,
}
impl State {
    fn new(owner: HWND) -> Result<Self> {
        let brush = unsafe { CreateSolidBrush(colorref(SURFACE)) };
        if brush.is_invalid() {
            return Err(Error::from_thread());
        }
        Ok(Self {
            owner,
            status: Cell::new(HWND::default()),
            picker: Cell::new(HWND::default()),
            previous: Cell::new(HWND::default()),
            edit: Cell::new(HWND::default()),
            hint: Cell::new(HWND::default()),
            disk: Cell::new(HWND::default()),
            slider: Cell::new(HWND::default()),
            text: RefCell::new(String::new()),
            color: Cell::new(Style::default().color),
            tool: Cell::new(Tool::Pen),
            expanded: Cell::new(false),
            panel_width: Cell::new(224),
            panel_pixels: RefCell::new(Vec::new()),
            recents: RefCell::new(vec![Style::default().color]),
            next_recent: Cell::new(0),
            panel_scale: Cell::new(1.0),
            panel_monitor: Cell::new(RECT::default()),
            tracking: Cell::new(false),
            pressed: Cell::new(None),
            candidate: Cell::new(Hsv::from_argb(Style::default().color)),
            valid: Cell::new(true),
            syncing: Cell::new(false),
            drag: Cell::new(HWND::default()),
            scale: Cell::new(1.0),
            font: Cell::new(HFONT::default()),
            edit_brush: GdiObject(brush.into()),
            cache: RefCell::new(None),
        })
    }
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            let font = self.font.replace(HFONT::default());
            if !font.is_invalid() {
                let _ = DeleteObject(font.into());
            }
        }
    }
}

pub struct DesktopUi {
    state: Box<State>,
    status_class: WindowClass,
    picker_class: WindowClass,
    widget_class: WindowClass,
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
            state: Box::new(State::new(owner)?),
            status_class: WindowClass::new(instance, "Status", Some(status_proc))?,
            picker_class: WindowClass::new(instance, "Picker", Some(picker_proc))?,
            widget_class: WindowClass::new(instance, "ColorControl", Some(widget_proc))?,
            _thread: PhantomData,
        };
        unsafe {
            // Only the occupied rounded region receives mouse input. Do not use
            // WS_EX_TRANSPARENT: it bypasses layered-window hit testing entirely.
            // https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                PCWSTR(ui.status_class.name.as_ptr()),
                w!("Mado selected tool"),
                WS_POPUP,
                0,
                0,
                PANEL_ROW,
                PANEL_ROW,
                Some(owner),
                None,
                Some(instance),
                Some(ui.state.as_ref() as *const State as _),
            )?;
            ui.state.status.set(hwnd);
            ui.state.anchor_panel(GetDpiForWindow(hwnd))?;
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
        self.update_label(drawing, &format!("{tool:?}"), style)
    }

    pub fn update_label(&self, drawing: bool, tool: &str, style: Style) -> Result<()> {
        let mode = if drawing { "Drawing" } else { "Click-through" };
        let text = format!("{mode} | {tool} | {} px", style.thickness);
        if *self.state.text.borrow() == text && self.state.color.get() == style.color {
            return Ok(());
        }
        *self.state.text.borrow_mut() = text;
        self.state.tool.set(tool_icon(tool));
        if self.state.color.replace(style.color) != style.color {
            let mut recents = self.state.recents.borrow_mut();
            if !recents.contains(&style.color) {
                if recents.len() < 5 {
                    recents.push(style.color);
                } else {
                    let slot = self.state.next_recent.get();
                    recents[slot] = style.color;
                    self.state.next_recent.set((slot + 1) % 5);
                }
            }
            self.state.pressed.set(None);
        }
        unsafe {
            let _ = SetWindowTextW(
                self.state.status.get(),
                PCWSTR(wide(&format!("Mado {tool}; {mode}")).as_ptr()),
            );
        }
        self.state.measure_panel()?;
        self.state.resize_panel()
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
                w!("Mado color picker"),
                WS_POPUP | WS_CLIPCHILDREN,
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
                let _ = SetFocus(Some(self.state.disk.get()));
            }
        }
        Ok(())
    }

    fn size_picker(&self, hwnd: HWND, cursor: POINT, work: RECT) -> Result<()> {
        let scale = fitting_scale(unsafe { GetDpiForWindow(hwnd) }, work);
        self.state.scale.set(scale);
        let (x, y, width, height) = picker_rect(
            cursor,
            work,
            (scaled(PICKER_WIDTH, scale), scaled(PICKER_HEIGHT, scale)),
        );
        unsafe {
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
        self.state.candidate.set(Hsv::from_argb(color));
        self.state.valid.set(true);
        self.state.disk.set(self.child(
            hwnd,
            PCWSTR(self.widget_class.name.as_ptr()),
            "Hue and saturation",
            WS_TABSTOP,
            DISK,
            true,
        )?);
        self.state.slider.set(self.child(
            hwnd,
            PCWSTR(self.widget_class.name.as_ptr()),
            "Brightness",
            WS_TABSTOP,
            BRIGHTNESS,
            true,
        )?);
        // A preceding native label names the native edit for accessibility.
        self.child(
            hwnd,
            w!("STATIC"),
            "&Hex",
            WINDOW_STYLE(0),
            HEX_LABEL,
            false,
        )?;
        self.state.edit.set(self.child(
            hwnd,
            w!("EDIT"),
            "",
            WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            HEX,
            false,
        )?);
        self.state
            .hint
            .set(self.child(hwnd, w!("STATIC"), "", WINDOW_STYLE(0), HINT, false)?);
        unsafe {
            SendMessageW(
                self.state.edit.get(),
                EM_SETLIMITTEXT,
                Some(WPARAM(32)),
                None,
            );
            SendMessageW(
                self.state.edit.get(),
                EM_SETMARGINS,
                Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
                Some(LPARAM((8 | (8 << 16)) as isize)),
            );
        }
        for (i, (label, _)) in PRESETS.iter().enumerate() {
            self.child(
                hwnd,
                w!("BUTTON"),
                label,
                WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                PRESET + i,
                false,
            )?;
        }
        for (id, label) in [(CANCEL, "&Cancel"), (APPLY, "&Apply"), (CLOSE, "Close")] {
            self.child(
                hwnd,
                w!("BUTTON"),
                label,
                WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                id,
                false,
            )?;
        }
        self.state.layout()?;
        rounded_region(
            hwnd,
            scaled(PICKER_WIDTH, self.state.scale.get()),
            scaled(PICKER_HEIGHT, self.state.scale.get()),
            scaled(18, self.state.scale.get()),
        )?;
        self.state.sync_candidate();
        Ok(())
    }

    fn child(
        &self,
        parent: HWND,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        id: usize,
        custom: bool,
    ) -> Result<HWND> {
        let text = wide(text);
        let (x, y, width, height) = control_rect(id);
        let s = self.state.scale.get();
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                scaled(x, s),
                scaled(y, s),
                scaled(width, s),
                scaled(height, s),
                Some(parent),
                Some(HMENU(id as _)),
                Some(self.picker_class.instance),
                if custom {
                    Some(self.state.as_ref() as *const State as _)
                } else {
                    None
                },
            )
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
                if self
                    .state
                    .key(GetDlgCtrlID(message.hwnd) as usize, message.wParam.0)
                {
                    return true;
                }
                if message.wParam.0 == VK_RETURN.0 as usize {
                    // Enter on a preset/Cancel clicks that button, not Apply.
                    let id = GetDlgCtrlID(message.hwnd) as usize;
                    self.state.command(
                        if id == CANCEL
                            || id == CLOSE
                            || (PRESET..PRESET + PRESETS.len()).contains(&id)
                        {
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
            for hwnd in [
                self.state.disk.get(),
                self.state.slider.get(),
                self.state.picker.get(),
                self.state.status.get(),
            ] {
                if !hwnd.is_invalid() {
                    // Detach before releasing Box, including a failed DestroyWindow.
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelHit {
    Current,
    Recent(u32),
}

struct PanelGeometry {
    width: i32,
    height: i32,
    radius: i32,
    scale: f64,
}
impl PanelGeometry {
    fn new(state: &State) -> Self {
        let scale = state.panel_scale.get();
        Self {
            width: if state.expanded.get() {
                state.panel_width.get()
            } else {
                scaled(PANEL_ROW, scale)
            },
            height: scaled(PANEL_ROW * if state.expanded.get() { 2 } else { 1 }, scale),
            radius: scaled(12, scale).max(1),
            scale,
        }
    }
    fn current(&self) -> (i32, i32) {
        (self.width - scaled(24, self.scale), scaled(22, self.scale))
    }
    fn recent(&self, index: usize) -> (i32, i32) {
        (
            self.width - scaled(24 + (4 - index) as i32 * 36, self.scale),
            scaled(66, self.scale),
        )
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return false;
        }
        let cx = x.clamp(self.radius, self.width - self.radius - 1);
        let cy = y.clamp(self.radius, self.height - self.radius - 1);
        (x - cx).pow(2) + (y - cy).pow(2) <= self.radius.pow(2)
    }
    fn hit(&self, state: &State, x: i32, y: i32) -> Option<PanelHit> {
        if !state.expanded.get() || !self.contains(x, y) {
            return None;
        }
        let inside = |center: (i32, i32)| {
            (x - center.0).pow(2) + (y - center.1).pow(2) <= scaled(9, self.scale).pow(2)
        };
        if inside(self.current()) {
            return Some(PanelHit::Current);
        }
        let recents = state.recents.borrow();
        for (i, color) in recents.iter().enumerate() {
            if inside(self.recent(i)) {
                return Some(PanelHit::Recent(*color));
            }
        }
        None
    }
}

fn tool_icon(label: &str) -> Tool {
    match label {
        "Line" => Tool::Line,
        "Rectangle" => Tool::Rectangle,
        "Ellipse" => Tool::Ellipse,
        "Eraser" => Tool::Eraser,
        label if label.starts_with("Arrow") => Tool::Arrow,
        _ => Tool::Pen,
    }
}

fn rounded_region(hwnd: HWND, width: i32, height: i32, radius: i32) -> Result<()> {
    unsafe {
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
        if region.is_invalid() {
            return Err(Error::from_thread());
        }
        // Ownership transfers only on success. This popup has no nonclient frame.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowrgn
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
            return Err(Error::from_thread());
        }
    }
    Ok(())
}

impl State {
    fn anchor_panel(&self, dpi: u32) -> Result<()> {
        unsafe {
            // The primary monitor includes the virtual-desktop origin, independent
            // of the overlay's virtual-desktop dimensions.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-monitorfrompoint
            let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info).ok()?;
            self.panel_monitor.set(info.rcMonitor);
            self.panel_scale.set(
                (dpi.max(96) as f64 / 96.0)
                    .min(
                        (info.rcMonitor.right - info.rcMonitor.left).max(1) as f64
                            / (PANEL_WIDTH + 32) as f64,
                    )
                    .min(
                        (info.rcMonitor.bottom - info.rcMonitor.top).max(1) as f64
                            / (2 * PANEL_ROW + 32) as f64,
                    ),
            );
        }
        self.measure_panel()?;
        self.resize_panel()
    }

    fn measure_panel(&self) -> Result<()> {
        let s = self.panel_scale.get();
        let surface = Bitmap::new(1, 1)?;
        let font = panel_font(s * PANEL_SAMPLES as f64)?;
        let text: Vec<u16> = self.text.borrow().encode_utf16().collect();
        let mut extent = SIZE::default();
        unsafe {
            let old = SelectObject(surface.dc, font.0);
            let result = GetTextExtentPoint32W(surface.dc, &text, &mut extent).ok();
            SelectObject(surface.dc, old);
            result?;
        }
        // Measure in the same font/DPI as painting. Five swatches always fit;
        // the existing maximum bounds allocation for unexpected long labels.
        // https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-gettextextentpoint32w
        self.panel_width.set(
            ((extent.cx + PANEL_SAMPLES - 1) / PANEL_SAMPLES + scaled(64, s))
                .clamp(scaled(224, s), scaled(PANEL_WIDTH, s)),
        );
        self.pressed.set(None);
        Ok(())
    }

    fn resize_panel(&self) -> Result<()> {
        let geometry = PanelGeometry::new(self);
        let monitor = self.panel_monitor.get();
        let margin = scaled(16, geometry.scale);
        let (surface, pixels) = panel_bitmap(self)?;
        let destination = POINT {
            x: monitor.right - margin - geometry.width,
            y: monitor.top + margin,
        };
        let size = SIZE {
            cx: geometry.width,
            cy: geometry.height,
        };
        let source = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // Per-pixel alpha supplies the shape. No binary region or global alpha.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow
        unsafe {
            UpdateLayeredWindow(
                self.status.get(),
                None,
                Some(&destination),
                Some(&size),
                Some(surface.dc),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )?;
        }
        *self.panel_pixels.borrow_mut() = pixels;
        Ok(())
    }

    fn panel_leave(&self) {
        self.tracking.set(false);
        self.pressed.set(None);
        if self.expanded.replace(false) {
            let _ = self.resize_panel();
        }
    }

    fn post(&self, event: UiEvent) {
        let (value, kind) = match event {
            UiEvent::Color(color) => (color as usize, 1),
            UiEvent::Closed => (0, 2),
            UiEvent::OpenPicker => (0, 3),
        };
        // No pointers in queued messages: dispatch may outlive either native window.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew
        unsafe {
            let _ = PostMessageW(Some(self.owner), UI_EVENT, WPARAM(value), LPARAM(kind));
        }
    }

    fn layout(&self) -> Result<()> {
        let s = self.scale.get();
        unsafe {
            let font = CreateFontW(
                -scaled(14, s),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            );
            if font.is_invalid() {
                return Err(Error::from_thread());
            }
            let old = self.font.replace(font);
            // WM_SETFONT/SetWindowPos can synchronously paint. No RefCell borrow here.
            for id in [DISK, BRIGHTNESS, HEX_LABEL, HEX, HINT, CANCEL, APPLY, CLOSE]
                .into_iter()
                .chain(PRESET..PRESET + PRESETS.len())
            {
                if let Ok(child) = GetDlgItem(Some(self.picker.get()), id as i32) {
                    let (x, y, width, height) = control_rect(id);
                    let _ = SetWindowPos(
                        child,
                        None,
                        scaled(x, s),
                        scaled(y, s),
                        scaled(width, s),
                        scaled(height, s),
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            }
            if !old.is_invalid() {
                let _ = DeleteObject(old.into());
            }
            let margin = scaled(8, s) as isize;
            SendMessageW(
                self.edit.get(),
                EM_SETMARGINS,
                Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
                Some(LPARAM(margin | (margin << 16))),
            );
        }
        Ok(())
    }

    fn repaint(&self) {
        unsafe {
            for hwnd in [self.picker.get(), self.disk.get(), self.slider.get()] {
                if !hwnd.is_invalid() {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            for id in PRESET..PRESET + PRESETS.len() {
                if let Ok(hwnd) = GetDlgItem(Some(self.picker.get()), id as i32) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }
    }

    fn sync_candidate(&self) {
        let hsv = self.candidate.get();
        self.valid.set(true);
        self.syncing.set(true);
        unsafe {
            let _ = SetWindowTextW(self.edit.get(), PCWSTR(wide(&hex(hsv.argb())).as_ptr()));
            let _ = SetWindowTextW(self.hint.get(), w!(""));
        }
        self.syncing.set(false);
        self.sync_names();
        self.repaint();
    }

    fn sync_names(&self) {
        let hsv = self.candidate.get();
        unsafe {
            let _ = SetWindowTextW(
                self.disk.get(),
                PCWSTR(
                    wide(&format!(
                        "Hue and saturation: {:.0} degrees, {:.0} percent; {}",
                        hsv.hue,
                        hsv.saturation * 100.0,
                        hex(hsv.argb())
                    ))
                    .as_ptr(),
                ),
            );
            let _ = SetWindowTextW(
                self.slider.get(),
                PCWSTR(wide(&format!("Brightness: {:.0} percent", hsv.value * 100.0)).as_ptr()),
            );
        }
    }

    fn hex_changed(&self) {
        if self.syncing.get() {
            return;
        }
        if let Some(color) = parse_hex(&window_text(self.edit.get())) {
            self.candidate.set(Hsv::from_argb(color));
            self.valid.set(true);
            unsafe {
                let _ = SetWindowTextW(self.hint.get(), w!(""));
            }
            self.sync_names();
        } else {
            self.valid.set(false);
            unsafe {
                let _ = SetWindowTextW(self.hint.get(), w!("Use #RRGGBB or RRGGBB."));
            }
        }
        self.repaint();
    }

    fn command(&self, id: usize) {
        if id == CANCEL || id == CLOSE {
            self.close_picker();
        } else if id == APPLY {
            // Validate the native edit again at the trust boundary, including pasted text.
            self.hex_changed();
            if self.valid.get() && !self.picker.get().is_invalid() {
                self.post(UiEvent::Color(self.candidate.get().argb()));
                self.close_picker();
            }
        } else if let Some((_, color)) = id.checked_sub(PRESET).and_then(|i| PRESETS.get(i)) {
            self.candidate.set(Hsv::from_argb(*color));
            self.sync_candidate();
        }
    }

    fn key(&self, id: usize, key: usize) -> bool {
        let mut hsv = self.candidate.get();
        match (id, key as u16) {
            (DISK, k) if k == VK_LEFT.0 => hsv.adjust(-3.0, 0.0, 0.0),
            (DISK, k) if k == VK_RIGHT.0 => hsv.adjust(3.0, 0.0, 0.0),
            (DISK, k) if k == VK_UP.0 => hsv.adjust(0.0, 0.02, 0.0),
            (DISK, k) if k == VK_DOWN.0 => hsv.adjust(0.0, -0.02, 0.0),
            (BRIGHTNESS, k) if k == VK_LEFT.0 => hsv.adjust(0.0, 0.0, -0.02),
            (BRIGHTNESS, k) if k == VK_RIGHT.0 => hsv.adjust(0.0, 0.0, 0.02),
            (BRIGHTNESS, k) if k == VK_HOME.0 => hsv.value = 0.0,
            (BRIGHTNESS, k) if k == VK_END.0 => hsv.value = 1.0,
            _ => return false,
        }
        self.candidate.set(hsv);
        self.sync_candidate();
        true
    }

    fn pointer(&self, hwnd: HWND, x: i32, y: i32) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rect);
        }
        let mut hsv = self.candidate.get();
        let id = unsafe { GetDlgCtrlID(hwnd) } as usize;
        if id == DISK {
            let (cx, cy, r) = disk_geometry(rect.right, rect.bottom, self.scale.get());
            hsv.pick_disk(x as f64 - cx, y as f64 - cy, r);
        } else if id == BRIGHTNESS {
            let inset = scaled(8, self.scale.get());
            hsv.value =
                ((x - inset) as f64 / (rect.right - 2 * inset).max(1) as f64).clamp(0.0, 1.0);
        } else {
            return;
        }
        self.candidate.set(hsv);
        self.sync_candidate();
    }

    fn stop_drag(&self) {
        let drag = self.drag.replace(HWND::default());
        unsafe {
            if !drag.is_invalid() && GetCapture() == drag {
                let _ = ReleaseCapture();
            }
        }
    }

    fn close_picker(&self) {
        let hwnd = self.picker.get();
        if hwnd.is_invalid() {
            return;
        }
        self.stop_drag();
        unsafe {
            // Never restore focus after another app was selected.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow
            if GetForegroundWindow() == hwnd && !self.previous.get().is_invalid() {
                let _ = SetForegroundWindow(self.previous.get());
            }
            let _ = DestroyWindow(hwnd);
        }
    }

    fn disk_pixels(&self, size: i32, value: f64) -> Rc<Vec<u32>> {
        let mut cache = self.cache.borrow_mut();
        if let Some(cache) = cache.as_ref()
            && cache.size == size
            && cache.value == value
        {
            return Rc::clone(&cache.pixels);
        }
        let radius = size as f64 / 2.0;
        let mut pixels = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                let dx = x as f64 + 0.5 - radius;
                let dy = y as f64 + 0.5 - radius;
                // One-pixel coverage antialiases only the disk's outer edge.
                let distance = dx.hypot(dy);
                let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
                let color = if coverage > 0.0 {
                    let ratio = radius / distance.max(radius);
                    disk_pixel(dx * ratio, dy * ratio, radius, value).unwrap_or(BACKGROUND)
                } else {
                    BACKGROUND
                };
                pixels.push(blend(BACKGROUND, color, coverage));
            }
        }
        let pixels = Rc::new(pixels);
        *cache = Some(DiskCache {
            size,
            value,
            pixels: Rc::clone(&pixels),
        });
        pixels
    }
}

fn decode_event(message: &MSG, owner: HWND) -> Option<UiEvent> {
    if message.hwnd != owner || message.message != UI_EVENT {
        return None;
    }
    match message.lParam.0 {
        1 if message.wParam.0 <= u32::MAX as usize => Some(UiEvent::Color(message.wParam.0 as u32)),
        2 => Some(UiEvent::Closed),
        3 if message.wParam.0 == 0 => Some(UiEvent::OpenPicker),
        _ => None,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn window_text(hwnd: HWND) -> String {
    let mut text = vec![0_u16; unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize + 1];
    let len = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..len])
}
fn colorref(argb: u32) -> COLORREF {
    COLORREF(((argb >> 16) & 255) | (argb & 0xff00) | ((argb & 255) << 16))
}
fn scaled(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
}
fn fitting_scale(dpi: u32, work: RECT) -> f64 {
    (dpi.max(96) as f64 / 96.0)
        .min((work.right - work.left).max(1) as f64 / PICKER_WIDTH as f64)
        .min((work.bottom - work.top).max(1) as f64 / PICKER_HEIGHT as f64)
}
fn control_rect(id: usize) -> (i32, i32, i32, i32) {
    match id {
        DISK => (32, 48, 236, 236),
        BRIGHTNESS => (12, 304, 276, 28),
        HEX_LABEL => (64, 330, 100, 16),
        HEX => (64, 346, 216, 26),
        HINT => (20, 375, 260, 16),
        CANCEL => (20, 440, 100, 32),
        APPLY => (132, 440, 148, 32),
        CLOSE => (252, 12, 28, 28),
        _ if (PRESET..PRESET + PRESETS.len()).contains(&id) => {
            (20 + (id - PRESET) as i32 * 46, 394, 30, 26)
        }
        _ => (0, 0, 0, 0),
    }
}
fn disk_geometry(width: i32, height: i32, scale: f64) -> (f64, f64, f64) {
    (
        width as f64 / 2.0,
        height as f64 / 2.0,
        (width.min(height) as f64 / 2.0 - 8.0 * scale).max(1.0),
    )
}
fn blend(a: u32, b: u32, coverage: f64) -> u32 {
    let mut color = 0xff000000;
    for shift in [0, 8, 16] {
        let a = ((a >> shift) & 255) as f64;
        let b = ((b >> shift) & 255) as f64;
        color |= ((a + (b - a) * coverage).round() as u32) << shift;
    }
    color
}
fn rect(x: i32, y: i32, width: i32, height: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}
fn logical_rect(s: f64, x: i32, y: i32, width: i32, height: i32) -> RECT {
    rect(
        scaled(x, s),
        scaled(y, s),
        scaled(width, s),
        scaled(height, s),
    )
}
fn fill(dc: HDC, rect: RECT, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(colorref(color));
        if !brush.is_invalid() {
            FillRect(dc, &rect, brush);
            let _ = DeleteObject(brush.into());
        }
    }
}
fn label(dc: HDC, text: &str, mut bounds: RECT, color: u32, flags: DRAW_TEXT_FORMAT) {
    unsafe {
        SetTextColor(dc, colorref(color));
        SetBkMode(dc, TRANSPARENT);
        let mut text: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(dc, &mut text, &mut bounds, flags);
    }
}
fn ellipse(dc: HDC, x: i32, y: i32, radius: i32, color: u32, width: i32, solid: bool) {
    unsafe {
        let pen = GdiObject(CreatePen(PS_SOLID, width.max(1), colorref(color)).into());
        let old_pen = SelectObject(dc, pen.0);
        let brush = solid.then(|| GdiObject(CreateSolidBrush(colorref(color)).into()));
        let old_brush = SelectObject(
            dc,
            brush.as_ref().map_or(GetStockObject(NULL_BRUSH), |b| b.0),
        );
        let _ = Ellipse(dc, x - radius, y - radius, x + radius + 1, y + radius + 1);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
    }
}
fn marker(dc: HDC, x: i32, y: i32, s: f64) {
    ellipse(dc, x, y, scaled(6, s), 0xffffffff, scaled(2, s), false);
    ellipse(dc, x, y, scaled(4, s), 0xff000000, scaled(2, s), false);
}
fn rounded(dc: HDC, bounds: RECT, radius: i32, background: u32, border: u32) {
    rounded_pen(dc, bounds, radius, background, border, 1);
}
fn rounded_pen(dc: HDC, bounds: RECT, radius: i32, background: u32, border: u32, width: i32) {
    unsafe {
        let pen = GdiObject(CreatePen(PS_SOLID, width.max(1), colorref(border)).into());
        let brush = GdiObject(CreateSolidBrush(colorref(background)).into());
        let old_pen = SelectObject(dc, pen.0);
        let old_brush = SelectObject(dc, brush.0);
        let _ = RoundRect(
            dc,
            bounds.left,
            bounds.top,
            bounds.right,
            bounds.bottom,
            radius * 2,
            radius * 2,
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
    }
}

// Shared by the panel renderer and offscreen tests. Restore selection before
// destroying the DC/bitmap, including when a paint or upload fails.
struct Bitmap {
    dc: HDC,
    _bitmap: GdiObject,
    old: HGDIOBJ,
    bits: *mut u32,
    len: usize,
}
impl Bitmap {
    fn new(width: i32, height: i32) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(Error::from_thread());
            }
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(
                Some(dc),
                &bitmap_info(width, height),
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            ) {
                Ok(bitmap) => bitmap,
                Err(error) => {
                    let _ = DeleteDC(dc);
                    return Err(error);
                }
            };
            Ok(Self {
                dc,
                _bitmap: GdiObject(bitmap.into()),
                old: SelectObject(dc, bitmap.into()),
                bits: bits.cast(),
                len: (width * height) as usize,
            })
        }
    }
    fn pixels(&self) -> &[u32] {
        unsafe {
            let _ = GdiFlush();
            std::slice::from_raw_parts(self.bits, self.len)
        }
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteDC(self.dc);
        }
    }
}
fn panel_font(scale: f64) -> Result<GdiObject> {
    let font = unsafe {
        CreateFontW(
            -scaled(14, scale),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            NONANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        )
    };
    if font.is_invalid() {
        Err(Error::from_thread())
    } else {
        Ok(GdiObject(font.into()))
    }
}
fn panel_bitmap(state: &State) -> Result<(Bitmap, Vec<u32>)> {
    let geometry = PanelGeometry::new(state);
    let n = PANEL_SAMPLES;
    let large = PanelGeometry {
        width: geometry.width * n,
        height: geometry.height * n,
        radius: geometry.radius * n,
        scale: geometry.scale * n as f64,
    };
    let sample = Bitmap::new(large.width, large.height)?;
    // ponytail: 4x box sampling is bounded by the existing 440x88 logical
    // maximum. Reuse GDI geometry; increase samples only if hardware review warrants it.
    fill(sample.dc, rect(0, 0, large.width, large.height), 0);
    unsafe {
        let brush = GdiObject(CreateSolidBrush(COLORREF(0xffffff)).into());
        let old_brush = SelectObject(sample.dc, brush.0);
        let old_pen = SelectObject(sample.dc, GetStockObject(NULL_PEN));
        let _ = RoundRect(
            sample.dc,
            0,
            0,
            large.width,
            large.height,
            large.radius * 2,
            large.radius * 2,
        );
        SelectObject(sample.dc, old_pen);
        SelectObject(sample.dc, old_brush);
    }
    let coverage: Vec<bool> = sample
        .pixels()
        .iter()
        .map(|pixel| pixel & 0xffffff != 0)
        .collect();
    paint_status(state, sample.dc, &large);
    let source = sample.pixels();
    let mut pixels = Vec::with_capacity((geometry.width * geometry.height) as usize);
    for y in 0..geometry.height {
        for x in 0..geometry.width {
            let mut sums = [0_u32; 4];
            for dy in 0..n {
                for dx in 0..n {
                    let i = ((y * n + dy) * large.width + x * n + dx) as usize;
                    if coverage[i] {
                        sums[3] += 245;
                        for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
                            sums[channel] += ((source[i] >> shift) & 255) * 245;
                        }
                    }
                }
            }
            let count = (n * n) as u32;
            let mut pixel = ((sums[3] + count / 2) / count) << 24;
            for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
                pixel |= ((sums[channel] + count * 255 / 2) / (count * 255)) << shift;
            }
            pixels.push(pixel);
        }
    }
    // GDI's RGB output does not maintain alpha. Average covered samples directly
    // into premultiplied BGRA for AC_SRC_ALPHA; never downsample straight alpha.
    // https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-blendfunction
    let surface = Bitmap::new(geometry.width, geometry.height)?;
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), surface.bits, pixels.len());
    }
    Ok((surface, pixels))
}

fn paint_status(state: &State, dc: HDC, geometry: &PanelGeometry) {
    let s = geometry.scale;
    fill(
        dc,
        rect(0, 0, geometry.width, geometry.height),
        PANEL_BACKGROUND,
    );
    let inset = scaled(1, s) / 2;
    rounded_pen(
        dc,
        rect(
            inset,
            inset,
            geometry.width - 2 * inset,
            geometry.height - 2 * inset,
        ),
        geometry.radius,
        PANEL_BACKGROUND,
        PANEL_BLUE,
        scaled(1, s),
    );
    unsafe {
        let saved = SaveDC(dc);
        if state.expanded.get() {
            let Ok(font) = panel_font(s) else {
                let _ = RestoreDC(dc, saved);
                return;
            };
            SelectObject(dc, font.0);
            label(
                dc,
                &state.text.borrow(),
                rect(
                    scaled(16, s),
                    0,
                    geometry.width - scaled(64, s),
                    scaled(PANEL_ROW, s),
                ),
                PANEL_BLUE,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
            let (x, y) = geometry.current();
            ellipse(dc, x, y, scaled(9, s), state.color.get(), 1, true);
            // Restore before dropping the selected font.
            let _ = RestoreDC(dc, saved);
            let recents = state.recents.borrow();
            for (i, color) in recents.iter().enumerate() {
                let (x, y) = geometry.recent(i);
                ellipse(dc, x, y, scaled(9, s), *color, 1, true);
                if *color == state.color.get() {
                    ellipse(dc, x, y, scaled(12, s), PANEL_BLUE, scaled(1, s), false);
                }
            }
        } else {
            let pen =
                GdiObject(CreatePen(PS_SOLID, scaled(2, s).max(1), colorref(PANEL_BLUE)).into());
            SelectObject(dc, pen.0);
            SelectObject(dc, GetStockObject(NULL_BRUSH));
            let line = |points: &[(i32, i32)]| {
                let points: Vec<POINT> = points
                    .iter()
                    .map(|(x, y)| POINT {
                        x: scaled(*x, s),
                        y: scaled(*y, s),
                    })
                    .collect();
                let _ = Polyline(dc, &points);
            };
            match state.tool.get() {
                Tool::Pen => {
                    line(&[(13, 30), (16, 23), (28, 11), (33, 16), (21, 28), (13, 30)]);
                    line(&[(25, 14), (30, 19)]);
                    line(&[(16, 23), (21, 28)]);
                }
                Tool::Arrow => {
                    line(&[(12, 32), (32, 12)]);
                    line(&[(20, 12), (32, 12), (32, 24)]);
                }
                Tool::Line => line(&[(12, 32), (32, 12)]),
                Tool::Rectangle => {
                    let _ = Rectangle(
                        dc,
                        scaled(11, s),
                        scaled(14, s),
                        scaled(33, s),
                        scaled(30, s),
                    );
                }
                Tool::Ellipse => {
                    let _ = Ellipse(
                        dc,
                        scaled(11, s),
                        scaled(14, s),
                        scaled(33, s),
                        scaled(30, s),
                    );
                }
                Tool::Eraser => {
                    line(&[(12, 25), (25, 12), (34, 21), (23, 32), (19, 32), (12, 25)]);
                    line(&[(18, 19), (28, 28)]);
                    line(&[(19, 32), (34, 32)]);
                }
            }
            let _ = RestoreDC(dc, saved);
        }
    }
}

fn paint_panel(state: &State, dc: HDC, bounds: RECT) {
    let s = state.scale.get();
    fill(dc, bounds, BACKGROUND);
    rounded(dc, bounds, scaled(18, s), BACKGROUND, BORDER);
    unsafe {
        let saved = SaveDC(dc);
        SelectObject(dc, state.font.get().into());
        label(
            dc,
            "Color",
            logical_rect(s, 20, 12, 170, 28),
            BLUE,
            DT_SINGLELINE | DT_VCENTER,
        );
        fill(dc, logical_rect(s, 226, 15, 20, 22), SURFACE);
        label(
            dc,
            "C",
            logical_rect(s, 226, 15, 20, 22),
            MUTED,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        label(
            dc,
            "Hue + saturation",
            logical_rect(s, 32, 28, 236, 20),
            FOREGROUND,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
        label(
            dc,
            "Apply saves. Cancel keeps the old color.",
            logical_rect(s, 12, 474, 276, 20),
            MUTED,
            DT_SINGLELINE | DT_VCENTER,
        );
        label(
            dc,
            "Brightness",
            logical_rect(s, 20, 285, 180, 20),
            MUTED,
            DT_SINGLELINE | DT_VCENTER,
        );
        label(
            dc,
            &format!("{:.0}%", state.candidate.get().value * 100.0),
            logical_rect(s, 220, 285, 60, 20),
            FOREGROUND,
            DT_RIGHT | DT_SINGLELINE | DT_VCENTER,
        );
        ellipse(
            dc,
            scaled(36, s),
            scaled(358, s),
            scaled(14, s),
            state.candidate.get().argb(),
            1,
            true,
        );
        ellipse(
            dc,
            scaled(36, s),
            scaled(358, s),
            scaled(14, s),
            BORDER,
            1,
            false,
        );
        fill(
            dc,
            logical_rect(s, 63, 345, 218, 28),
            if state.valid.get() {
                BORDER
            } else {
                0xffb91c1c
            },
        );
        // Hidden single-line EDIT does not WM_PRINT its client. Paint the same font,
        // text and margins underneath it for offscreen printing; WS_CLIPCHILDREN
        // excludes this area in the live pane, where the native EDIT paints itself.
        fill(dc, logical_rect(s, 64, 346, 216, 26), SURFACE);
        label(
            dc,
            &window_text(state.edit.get()),
            logical_rect(s, 72, 346, 200, 26),
            FOREGROUND,
            DT_SINGLELINE | DT_VCENTER,
        );
        let _ = RestoreDC(dc, saved);
    }
}
fn paint_widget(state: &State, hwnd: HWND, dc: HDC, bounds: RECT) {
    fill(dc, bounds, BACKGROUND);
    let hsv = state.candidate.get();
    let s = state.scale.get();
    let id = unsafe { GetDlgCtrlID(hwnd) } as usize;
    if id == DISK {
        let (cx, cy, radius) = disk_geometry(bounds.right, bounds.bottom, s);
        let size = (radius * 2.0).round() as i32;
        let pixels = state.disk_pixels(size, hsv.value);
        let info = bitmap_info(size, size);
        // Cache borrows ended before GDI. Negative biHeight is a top-down DIB.
        // https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createdibsection
        unsafe {
            StretchDIBits(
                dc,
                (cx - radius).round() as i32,
                (cy - radius).round() as i32,
                size,
                size,
                0,
                0,
                size,
                size,
                Some(pixels.as_ptr() as _),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
        let angle = (180.0 - hsv.hue).to_radians();
        marker(
            dc,
            (cx + radius * hsv.saturation * angle.cos()).round() as i32,
            (cy + radius * hsv.saturation * angle.sin()).round() as i32,
            s,
        );
    } else {
        let inset = scaled(8, s);
        let width = (bounds.right - 2 * inset).max(1);
        let top = scaled(9, s);
        let height = scaled(10, s).max(1);
        fill(dc, rect(inset - 1, top - 1, width + 2, height + 2), BORDER);
        for x in 0..width {
            let color = Hsv {
                value: x as f64 / (width - 1).max(1) as f64,
                ..hsv
            }
            .argb();
            fill(dc, rect(inset + x, top, 1, height), color);
        }
        marker(
            dc,
            inset + (hsv.value * width as f64).round() as i32,
            bounds.bottom / 2,
            s,
        );
    }
    unsafe {
        if GetFocus() == hwnd {
            let bounds = rect(1, 1, bounds.right - 2, bounds.bottom - 2);
            let _ = DrawFocusRect(dc, &bounds);
        }
    }
}
fn paint_button(state: &State, item: &windows::Win32::UI::Controls::DRAWITEMSTRUCT) {
    let dc = item.hDC;
    let id = item.CtlID as usize;
    let s = state.scale.get();
    unsafe {
        // Restore the supplied owner-draw DC, including font and text settings.
        // https://learn.microsoft.com/en-us/windows/win32/controls/wm-drawitem
        let saved = SaveDC(dc);
        SelectObject(dc, state.font.get().into());
        let pressed = item.itemState.0 & 1 != 0;
        if let Some((_, color)) = id.checked_sub(PRESET).and_then(|i| PRESETS.get(i)) {
            fill(dc, item.rcItem, BACKGROUND);
            let x = (item.rcItem.left + item.rcItem.right) / 2;
            let y = (item.rcItem.top + item.rcItem.bottom) / 2;
            let selected = *color == state.candidate.get().argb();
            ellipse(dc, x, y, scaled(9, s), *color, 1, true);
            ellipse(
                dc,
                x,
                y,
                scaled(9, s),
                if selected { BLUE } else { BORDER },
                scaled(if selected || pressed { 2 } else { 1 }, s),
                false,
            );
        } else {
            let background = if id == APPLY {
                if pressed { 0xffffe4cc } else { 0xfffff4e8 }
            } else if pressed {
                0xffe2e8f0
            } else if id == CLOSE {
                BACKGROUND
            } else {
                SURFACE
            };
            fill(dc, item.rcItem, BACKGROUND);
            rounded(
                dc,
                item.rcItem,
                scaled(6, s),
                background,
                if id == APPLY {
                    ORANGE
                } else if id == CLOSE {
                    BACKGROUND
                } else {
                    BORDER
                },
            );
            label(
                dc,
                if id == APPLY {
                    "Apply"
                } else if id == CLOSE {
                    "X"
                } else {
                    "Cancel"
                },
                item.rcItem,
                if id == APPLY { 0xffc2410c } else { FOREGROUND },
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
        }
        if item.itemState.0 & 16 != 0 {
            let mut focus = item.rcItem;
            let _ = InflateRect(&mut focus, -scaled(3, s), -scaled(3, s));
            let _ = DrawFocusRect(dc, &focus);
        }
        let _ = RestoreDC(dc, saved);
    }
}
fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
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
                WM_ERASEBKGND => return LRESULT(1),
                WM_PAINT | WM_PRINTCLIENT => {
                    let mut paint = PAINTSTRUCT::default();
                    let dc = if message == WM_PAINT {
                        BeginPaint(hwnd, &mut paint)
                    } else {
                        HDC(w.0 as _)
                    };
                    let mut bounds = RECT::default();
                    let _ = GetClientRect(hwnd, &mut bounds);
                    paint_panel(state, dc, bounds);
                    if message == WM_PAINT {
                        let _ = EndPaint(hwnd, &paint);
                    }
                    return LRESULT(0);
                }
                WM_DRAWITEM => {
                    paint_button(
                        state,
                        &*(l.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT),
                    );
                    return LRESULT(1);
                }
                WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
                    let dc = HDC(w.0 as _);
                    let child = HWND(l.0 as _);
                    SetTextColor(
                        dc,
                        colorref(if child == state.hint.get() {
                            0xffb91c1c
                        } else {
                            FOREGROUND
                        }),
                    );
                    let background = if child == state.edit.get() {
                        SURFACE
                    } else {
                        BACKGROUND
                    };
                    SetBkColor(dc, colorref(background));
                    if child == state.edit.get() {
                        return LRESULT(state.edit_brush.0.0 as isize);
                    }
                    // A real background brush erases cleared inline errors. Returning
                    // NULL_BRUSH would leave old static text behind under CLIPCHILDREN.
                    SetDCBrushColor(dc, colorref(BACKGROUND));
                    return LRESULT(GetStockObject(DC_BRUSH).0 as isize);
                }
                WM_COMMAND => {
                    let id = w.0 & 0xffff;
                    let notification = (w.0 >> 16) & 0xffff;
                    if notification == BN_CLICKED as usize {
                        state.command(id);
                    } else if id == HEX && notification == EN_CHANGE as usize {
                        state.hex_changed();
                    }
                    return LRESULT(0);
                }
                WM_ACTIVATE
                    if w.0 & 0xffff == WA_INACTIVE as usize && IsWindowVisible(hwnd).as_bool() =>
                {
                    state.close_picker();
                    return LRESULT(0);
                }
                WM_CANCELMODE => {
                    state.stop_drag();
                    return LRESULT(0);
                }
                WM_DPICHANGED => {
                    // Scale geometry and fonts together and clamp to the new work area.
                    // https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged
                    let suggested = &*(l.0 as *const RECT);
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let monitor = MonitorFromRect(suggested, MONITOR_DEFAULTTONEAREST);
                    if GetMonitorInfoW(monitor, &mut info).as_bool() {
                        state.stop_drag();
                        let s = fitting_scale((w.0 & 0xffff) as u32, info.rcWork);
                        state.scale.set(s);
                        let (x, y, width, height) = picker_rect(
                            POINT {
                                x: suggested.left,
                                y: suggested.top,
                            },
                            info.rcWork,
                            (scaled(PICKER_WIDTH, s), scaled(PICKER_HEIGHT, s)),
                        );
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            x,
                            y,
                            width,
                            height,
                            SWP_NOACTIVATE | SWP_NOZORDER,
                        );
                        let _ = state.layout();
                        let _ = rounded_region(hwnd, width, height, scaled(18, s));
                        state.repaint();
                    }
                    return LRESULT(0);
                }
                WM_CLOSE => {
                    state.close_picker();
                    return LRESULT(0);
                }
                WM_NCDESTROY => {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    if state.picker.replace(HWND::default()) == hwnd {
                        state.stop_drag();
                        state.edit.set(HWND::default());
                        state.hint.set(HWND::default());
                        state.disk.set(HWND::default());
                        state.slider.set(HWND::default());
                        state.previous.set(HWND::default());
                        state.cache.borrow_mut().take();
                        let font = state.font.replace(HFONT::default());
                        if !font.is_invalid() {
                            let _ = DeleteObject(font.into());
                        }
                        state.post(UiEvent::Closed);
                    }
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, message, w, l)
    }
}

unsafe extern "system" fn widget_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if let Some(state) = state_for(hwnd, message, l) {
            match message {
                WM_GETDLGCODE => return LRESULT(DLGC_WANTARROWS as isize),
                WM_KEYDOWN if state.key(GetDlgCtrlID(hwnd) as usize, w.0) => return LRESULT(0),
                WM_ERASEBKGND => return LRESULT(1),
                WM_PAINT | WM_PRINTCLIENT => {
                    let mut paint = PAINTSTRUCT::default();
                    let dc = if message == WM_PAINT {
                        BeginPaint(hwnd, &mut paint)
                    } else {
                        HDC(w.0 as _)
                    };
                    let mut bounds = RECT::default();
                    let _ = GetClientRect(hwnd, &mut bounds);
                    paint_widget(state, hwnd, dc, bounds);
                    if message == WM_PAINT {
                        let _ = EndPaint(hwnd, &paint);
                    }
                    return LRESULT(0);
                }
                WM_LBUTTONDOWN => {
                    let x = l.0 as u16 as i16 as i32;
                    let y = (l.0 >> 16) as u16 as i16 as i32;
                    let mut bounds = RECT::default();
                    let _ = GetClientRect(hwnd, &mut bounds);
                    let id = GetDlgCtrlID(hwnd) as usize;
                    let (cx, cy, radius) =
                        disk_geometry(bounds.right, bounds.bottom, state.scale.get());
                    if id == DISK && (x as f64 - cx).hypot(y as f64 - cy) > radius {
                        return LRESULT(0);
                    }
                    // Capture only a real visible drag. Hidden tests cannot move focus/capture.
                    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture
                    if IsWindowVisible(hwnd).as_bool() {
                        let _ = SetFocus(Some(hwnd));
                        SetCapture(hwnd);
                        if GetCapture() == hwnd {
                            state.drag.set(hwnd);
                        }
                    }
                    state.pointer(hwnd, x, y);
                    return LRESULT(0);
                }
                WM_MOUSEMOVE if state.drag.get() == hwnd && GetCapture() == hwnd => {
                    state.pointer(
                        hwnd,
                        l.0 as u16 as i16 as i32,
                        (l.0 >> 16) as u16 as i16 as i32,
                    );
                    return LRESULT(0);
                }
                WM_LBUTTONUP if state.drag.get() == hwnd => {
                    state.pointer(
                        hwnd,
                        l.0 as u16 as i16 as i32,
                        (l.0 >> 16) as u16 as i16 as i32,
                    );
                    state.stop_drag();
                    return LRESULT(0);
                }
                WM_CAPTURECHANGED => {
                    // Do not reacquire/release capture inside WM_CAPTURECHANGED.
                    // https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-capturechanged
                    if state.drag.get() == hwnd {
                        state.drag.set(HWND::default());
                    }
                    return LRESULT(0);
                }
                WM_CANCELMODE => {
                    state.stop_drag();
                    return LRESULT(0);
                }
                WM_SETFOCUS | WM_KILLFOCUS => {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                WM_NCDESTROY => {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    if state.drag.get() == hwnd {
                        state.stop_drag();
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
            // Keep delivering the click without activating or taking keyboard focus.
            // https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if let Some(state) = state_for(hwnd, message, l) {
            match message {
                WM_NCHITTEST => {
                    let mut point = POINT {
                        x: l.0 as u16 as i16 as i32,
                        y: (l.0 >> 16) as u16 as i16 as i32,
                    };
                    let _ = ScreenToClient(hwnd, &mut point);
                    let geometry = PanelGeometry::new(state);
                    let opaque = point.x >= 0
                        && point.y >= 0
                        && point.x < geometry.width
                        && point.y < geometry.height
                        && state
                            .panel_pixels
                            .borrow()
                            .get((point.y * geometry.width + point.x) as usize)
                            .is_some_and(|pixel| pixel >> 24 != 0);
                    return LRESULT(if opaque {
                        HTCLIENT as isize
                    } else {
                        HTTRANSPARENT as isize
                    });
                }
                WM_ERASEBKGND => return LRESULT(1),
                WM_PAINT => {
                    // ULW owns the cached pixels; validate without repainting a GDI client.
                    let mut paint = PAINTSTRUCT::default();
                    BeginPaint(hwnd, &mut paint);
                    let _ = EndPaint(hwnd, &paint);
                    return LRESULT(0);
                }
                WM_MOUSEMOVE => {
                    // Track leave once per entry; WM_MOUSELEAVE cancels the request.
                    // No hover timer, polling, capture or focus change is needed.
                    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackmouseevent
                    if !state.tracking.get() {
                        let mut tracking = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            ..Default::default()
                        };
                        state.tracking.set(TrackMouseEvent(&mut tracking).is_ok());
                    }
                    if !state.expanded.replace(true) {
                        let _ = state.resize_panel();
                    }
                    return LRESULT(0);
                }
                WM_MOUSELEAVE => {
                    state.panel_leave();
                    return LRESULT(0);
                }
                WM_LBUTTONDOWN => {
                    state.pressed.set(PanelGeometry::new(state).hit(
                        state,
                        l.0 as u16 as i16 as i32,
                        (l.0 >> 16) as u16 as i16 as i32,
                    ));
                    return LRESULT(0);
                }
                WM_LBUTTONUP => {
                    let hit = PanelGeometry::new(state).hit(
                        state,
                        l.0 as u16 as i16 as i32,
                        (l.0 >> 16) as u16 as i16 as i32,
                    );
                    if let Some(hit) = state
                        .pressed
                        .replace(None)
                        .filter(|pressed| Some(*pressed) == hit)
                    {
                        match hit {
                            PanelHit::Current => state.post(UiEvent::OpenPicker),
                            PanelHit::Recent(color) => state.post(if color == state.color.get() {
                                UiEvent::OpenPicker
                            } else {
                                UiEvent::Color(color)
                            }),
                        }
                    }
                    return LRESULT(0);
                }
                WM_CANCELMODE => {
                    state.pressed.set(None);
                    return LRESULT(0);
                }
                WM_DPICHANGED => {
                    let _ = state.anchor_panel((w.0 & 0xffff) as u32);
                    return LRESULT(0);
                }
                WM_DISPLAYCHANGE => {
                    let _ = state.anchor_panel(GetDpiForWindow(hwnd));
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

    struct HiddenRoot(HWND);
    impl Drop for HiddenRoot {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    unsafe extern "system" fn root_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, message, w, l) }
    }
    fn create_hidden(
        class: &WindowClass,
        owner: HWND,
        state: Option<&State>,
        width: i32,
        height: i32,
    ) -> Result<HWND> {
        unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                PCWSTR(class.name.as_ptr()),
                w!("Hidden picker test"),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                width,
                height,
                Some(owner),
                None,
                Some(class.instance),
                state.map(|s| s as *const State as _),
            )
        }
    }
    fn hidden_ui(owner: HWND) -> Result<DesktopUi> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        Ok(DesktopUi {
            state: Box::new(State::new(owner)?),
            status_class: WindowClass::new(instance, "TestStatus", Some(status_proc))?,
            picker_class: WindowClass::new(instance, "TestPicker", Some(picker_proc))?,
            widget_class: WindowClass::new(instance, "TestControl", Some(widget_proc))?,
            _thread: PhantomData,
        })
    }
    fn picker(ui: &DesktopUi, color: u32) -> Result<HWND> {
        let hwnd = create_hidden(
            &ui.picker_class,
            ui.state.owner,
            Some(&ui.state),
            PICKER_WIDTH,
            PICKER_HEIGHT,
        )?;
        ui.state.picker.set(hwnd);
        ui.build_picker(hwnd, color)?;
        Ok(hwnd)
    }
    fn events(owner: HWND) -> Vec<MSG> {
        let mut messages = Vec::new();
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, Some(owner), UI_EVENT, UI_EVENT, PM_REMOVE).as_bool() {
                messages.push(message);
            }
        }
        messages
    }
    fn decoded(owner: HWND) -> Vec<UiEvent> {
        events(owner)
            .iter()
            .filter_map(|m| decode_event(m, owner))
            .collect()
    }
    fn click(hwnd: HWND, id: usize) {
        // Notification only. BM_CLICK can move focus even on a hidden button.
        unsafe {
            let child = GetDlgItem(Some(hwnd), id as i32).unwrap();
            SendMessageW(
                hwnd,
                WM_COMMAND,
                Some(WPARAM(id)),
                Some(LPARAM(child.0 as isize)),
            );
        }
    }
    fn key(ui: &DesktopUi, hwnd: HWND, key: VIRTUAL_KEY) -> bool {
        ui.handle_message(&MSG {
            hwnd,
            message: WM_KEYDOWN,
            wParam: WPARAM(key.0 as usize),
            ..Default::default()
        })
    }
    fn point(x: i32, y: i32) -> LPARAM {
        LPARAM(((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize)
    }

    fn status(ui: &DesktopUi) -> Result<HWND> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(ui.status_class.name.as_ptr()),
                w!("Hidden selected tool test"),
                WS_POPUP,
                0,
                0,
                PANEL_ROW,
                PANEL_ROW,
                Some(ui.state.owner),
                None,
                Some(ui.status_class.instance),
                Some(ui.state.as_ref() as *const State as _),
            )?
        };
        ui.state.status.set(hwnd);
        ui.state.anchor_panel(96)?;
        ui.update(false, Tool::Pen, Style::default())?;
        Ok(hwnd)
    }
    fn panel_click(hwnd: HWND, center: (i32, i32)) {
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(center.0, center.1)));
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(center.0, center.1)));
        }
    }
    fn window_bounds(hwnd: HWND) -> Result<RECT> {
        let mut bounds = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &mut bounds)?;
        }
        Ok(bounds)
    }

    #[test]
    fn hidden_panel_hover_recents_anchor_nonactivation_and_previews() -> Result<()> {
        unsafe {
            let focus = GetFocus();
            let active = GetActiveWindow();
            let capture = GetCapture();
            let foreground = GetForegroundWindow();
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = WindowClass::new(instance, "PanelRoot", Some(root_proc))?;
            let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
            let ui = hidden_ui(root.0)?;
            let hwnd = status(&ui)?;
            let initial = window_bounds(hwnd)?;
            let monitor = ui.state.panel_monitor.get();
            assert_eq!(
                (initial.right, initial.top),
                (monitor.right - 16, monitor.top + 16)
            );
            assert_eq!(
                (initial.right - initial.left, initial.bottom - initial.top),
                (44, 44)
            );
            assert!(!IsWindowVisible(hwnd).as_bool());
            let styles = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            assert_ne!(styles & WS_EX_NOACTIVATE.0, 0);
            assert_eq!(styles & WS_EX_TRANSPARENT.0, 0);
            assert_eq!(
                SendMessageW(hwnd, WM_MOUSEACTIVATE, None, None).0,
                MA_NOACTIVATE as isize
            );
            assert_eq!(
                SendMessageW(
                    hwnd,
                    WM_NCHITTEST,
                    None,
                    Some(point(initial.left + 22, initial.top + 22))
                )
                .0,
                HTCLIENT as isize
            );
            assert_eq!(
                SendMessageW(
                    hwnd,
                    WM_NCHITTEST,
                    None,
                    Some(point(initial.left, initial.top))
                )
                .0,
                HTTRANSPARENT as isize
            );
            let region = GdiObject(CreateRectRgn(0, 0, 0, 0).into());
            assert_eq!(GetWindowRgn(hwnd, HRGN(region.0.0)).0, ERROR);
            assert_eq!(ui.state.panel_pixels.borrow()[0], 0);
            assert_eq!(ui.state.panel_pixels.borrow()[22 * 44 + 22] >> 24, 245);
            let partial = ui
                .state
                .panel_pixels
                .borrow()
                .iter()
                .position(|pixel| {
                    let alpha = pixel >> 24;
                    alpha > 0 && alpha < 245
                })
                .unwrap();
            assert_eq!(
                SendMessageW(
                    hwnd,
                    WM_NCHITTEST,
                    None,
                    Some(point(
                        initial.left + (partial % 44) as i32,
                        initial.top + (partial / 44) as i32
                    ))
                )
                .0,
                HTCLIENT as isize
            );
            assert_eq!(&*ui.state.recents.borrow(), &[Style::default().color]);
            let mut icon_images = Vec::new();
            for tool in [
                Tool::Pen,
                Tool::Arrow,
                Tool::Line,
                Tool::Rectangle,
                Tool::Ellipse,
                Tool::Eraser,
            ] {
                ui.update(false, tool, Style::default())?;
                let image = panel_preview(&ui, &format!("mado-panel-compact-{tool:?}"))?;
                // Compact paint is independent of mode, color and thickness: icon only.
                ui.update(
                    true,
                    tool,
                    Style {
                        color: 0xffabcdef,
                        thickness: 31,
                    },
                )?;
                let alternate = panel_bitmap(&ui.state)?.1;
                assert_eq!(image, alternate);
                assert!(!icon_images.contains(&image), "Tool icons must be distinct");
                assert!(!image.iter().any(|pixel| pixel & 0xffffff == 0xabcdef));
                icon_images.push(image);
                ui.state.anchor_panel(144)?;
                panel_preview(&ui, &format!("mado-panel-compact-{tool:?}-150"))?;
                ui.state.anchor_panel(96)?;
            }
            ui.update(true, Tool::Pen, Style::default())?;
            SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(22, 22)));
            assert!(ui.state.expanded.get());
            let expanded = window_bounds(hwnd)?;
            assert_eq!((expanded.right, expanded.top), (initial.right, initial.top));
            assert_eq!(expanded.right - expanded.left, ui.state.panel_width.get());
            assert!(expanded.right - expanded.left < 300);
            assert_eq!(expanded.bottom - expanded.top, 88);
            panel_preview(&ui, "mado-panel-expanded")?;
            ui.state.anchor_panel(144)?;
            panel_preview(&ui, "mado-panel-expanded-150")?;
            ui.state.anchor_panel(96)?;
            panel_click(hwnd, PanelGeometry::new(&ui.state).current());
            assert_eq!(decoded(root.0), [UiEvent::OpenPicker]);
            assert_eq!(window_bounds(hwnd)?.bottom - initial.top, 88);
            for color in [
                0xff112233, 0xff223344, 0xff334455, 0xff445566, 0xff556677, 0xff223344,
            ] {
                ui.update_label(
                    true,
                    "Arrow tail-first",
                    Style {
                        color,
                        thickness: 31,
                    },
                )?;
            }
            assert_eq!(
                &*ui.state.recents.borrow(),
                &[0xff445566, 0xff556677, 0xff112233, 0xff223344, 0xff334455]
            );
            panel_preview(&ui, "mado-panel-recents")?;
            let long_bounds = window_bounds(hwnd)?;
            assert_eq!(
                (long_bounds.right, long_bounds.top),
                (initial.right, initial.top)
            );
            assert!(long_bounds.right - long_bounds.left > expanded.right - expanded.left);
            ui.state.anchor_panel(144)?;
            panel_preview(&ui, "mado-panel-recents-150")?;
            ui.state.anchor_panel(96)?;
            ui.update_label(
                false,
                "Arrow tail-first",
                Style {
                    color: ui.state.color.get(),
                    thickness: 31,
                },
            )?;
            panel_preview(&ui, "mado-panel-long-click-through")?;
            ui.state.anchor_panel(144)?;
            panel_preview(&ui, "mado-panel-long-click-through-150")?;
            ui.state.anchor_panel(96)?;
            for i in 0..5 {
                let geometry = PanelGeometry::new(&ui.state);
                let center = geometry.recent(i);
                SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(center.0, center.1)));
                assert!(ui.state.expanded.get());
                let color = ui.state.recents.borrow()[i];
                panel_click(hwnd, center);
                assert_eq!(
                    decoded(root.0),
                    [if color == ui.state.color.get() {
                        UiEvent::OpenPicker
                    } else {
                        UiEvent::Color(color)
                    }]
                );
                assert!(!ui.is_picker_open());
            }
            // Unpaired, off-panel and stale press/release messages do nothing.
            let center = PanelGeometry::new(&ui.state).recent(0);
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(center.0, center.1)));
            panel_click(hwnd, (-1, -1));
            panel_click(hwnd, (1, 1));
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(center.0, center.1)));
            ui.update(
                true,
                Tool::Pen,
                Style {
                    color: 0xff778899,
                    thickness: 5,
                },
            )?;
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(center.0, center.1)));
            assert!(decoded(root.0).is_empty());
            panel_click(hwnd, PanelGeometry::new(&ui.state).current());
            assert_eq!(decoded(root.0), [UiEvent::OpenPicker]);
            assert!(ui.state.expanded.get());
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(center.0, center.1)));
            SendMessageW(hwnd, WM_MOUSELEAVE, None, None);
            assert!(!ui.state.expanded.get());
            assert!(ui.state.pressed.get().is_none());
            assert_eq!(window_bounds(hwnd)?, initial);
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(center.0, center.1)));
            assert!(decoded(root.0).is_empty());
            for dpi in [120, 144, 192] {
                ui.state.anchor_panel(dpi)?;
                SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(22, 22)));
                let before = window_bounds(hwnd)?;
                panel_click(hwnd, PanelGeometry::new(&ui.state).current());
                assert_eq!(decoded(root.0), [UiEvent::OpenPicker]);
                let after = window_bounds(hwnd)?;
                assert_eq!((before.right, before.top), (after.right, after.top));
                SendMessageW(hwnd, WM_MOUSELEAVE, None, None);
                let compact = window_bounds(hwnd)?;
                assert_eq!((compact.right, compact.top), (after.right, after.top));
                assert_eq!(compact.right, monitor.right - scaled(16, dpi as f64 / 96.0));
            }
            drop(ui);
            assert_eq!(
                (
                    GetFocus(),
                    GetActiveWindow(),
                    GetCapture(),
                    GetForegroundWindow()
                ),
                (focus, active, capture, foreground)
            );
        }
        Ok(())
    }

    #[test]
    fn panel_pixels_and_measured_layout_at_100_and_150_percent() -> Result<()> {
        let state = State::new(HWND::default())?;
        for scale in [1.0, 1.5] {
            state.panel_scale.set(scale);
            for tool in [
                Tool::Pen,
                Tool::Arrow,
                Tool::Line,
                Tool::Rectangle,
                Tool::Ellipse,
                Tool::Eraser,
            ] {
                state.tool.set(tool);
                let geometry = PanelGeometry::new(&state);
                let pixels = panel_bitmap(&state)?.1;
                let width = geometry.width as usize;
                let height = geometry.height as usize;
                assert_eq!(
                    (width, height),
                    (scaled(44, scale) as usize, scaled(44, scale) as usize)
                );
                for index in [0, width - 1, (height - 1) * width, pixels.len() - 1] {
                    assert_eq!(pixels[index], 0);
                }
                for pixel in &pixels {
                    let alpha = pixel >> 24;
                    assert!(alpha <= 245);
                    for shift in [0, 8, 16] {
                        assert!((pixel >> shift) & 255 <= alpha);
                    }
                }
                // Every perimeter scan is contiguous and has partial coverage at
                // the rounded ends, not a binary staircase or an opaque rectangle.
                for row in pixels.chunks(width) {
                    let first = row.iter().position(|pixel| pixel >> 24 != 0).unwrap();
                    let last = row.iter().rposition(|pixel| pixel >> 24 != 0).unwrap();
                    assert!(row[first..=last].iter().all(|pixel| pixel >> 24 != 0));
                }
                for x in 0..width {
                    let column: Vec<_> = (0..height).map(|y| pixels[y * width + x] >> 24).collect();
                    let first = column.iter().position(|alpha| *alpha != 0).unwrap();
                    let last = column.iter().rposition(|alpha| *alpha != 0).unwrap();
                    assert!(column[first..=last].iter().all(|alpha| *alpha != 0));
                }
                for (left, top) in [
                    (0, 0),
                    (width / 2, 0),
                    (0, height / 2),
                    (width / 2, height / 2),
                ] {
                    assert!(
                        (top..top + height / 2).any(|y| (left..left + width / 2).any(|x| {
                            let a = pixels[y * width + x] >> 24;
                            a > 0 && a < 245
                        }))
                    );
                }
                // Only icon geometry occupies this interior. Intermediate blue
                // values prove diagonal/curved strokes are sampled, not just the mask.
                let background_blue = ((PANEL_BACKGROUND & 255) * 245 + 127) / 255;
                let icon_blue = ((PANEL_BLUE & 255) * 245 + 127) / 255;
                let mixed = (scaled(10, scale)..scaled(35, scale))
                    .flat_map(|y| {
                        (scaled(10, scale)..scaled(35, scale))
                            .map(move |x| (y * geometry.width + x) as usize)
                    })
                    .filter(|i| {
                        let b = pixels[*i] & 255;
                        b > background_blue && b < icon_blue
                    })
                    .count();
                assert!(
                    mixed >= 8,
                    "{tool:?} at {scale}: {mixed} antialiased icon pixels"
                );
            }
            state.expanded.set(true);
            *state.text.borrow_mut() = "Drawing | Pen | 5 px".into();
            state.measure_panel()?;
            let short = state.panel_width.get();
            assert!(short < scaled(300, scale));
            let short_geometry = PanelGeometry::new(&state);
            for i in 0..5 {
                let (x, y) = short_geometry.recent(i);
                let radius = scaled(12, scale);
                assert!(x - radius >= scaled(16, scale));
                assert!(x + radius < short && y + radius < short_geometry.height);
                if i != 0 {
                    assert!(x - short_geometry.recent(i - 1).0 > radius * 2);
                }
            }
            *state.text.borrow_mut() = "Click-through | Arrow tail-first | 31 px".into();
            state.measure_panel()?;
            let long = state.panel_width.get();
            assert!(long > short && long < scaled(PANEL_WIDTH, scale));
            let geometry = PanelGeometry::new(&state);
            assert_eq!(geometry.height, scaled(88, scale));
            for i in 0..5 {
                let (x, y) = geometry.recent(i);
                let radius = scaled(12, scale);
                assert!(x - radius >= scaled(16, scale));
                assert!(x + radius < geometry.width && y + radius < geometry.height);
                if i != 0 {
                    assert!(x - geometry.recent(i - 1).0 > radius * 2);
                }
            }
            let surface = Bitmap::new(1, 1)?;
            let font = panel_font(scale * PANEL_SAMPLES as f64)?;
            let text: Vec<_> = state.text.borrow().encode_utf16().collect();
            let mut extent = SIZE::default();
            unsafe {
                let old = SelectObject(surface.dc, font.0);
                let result = GetTextExtentPoint32W(surface.dc, &text, &mut extent).ok();
                SelectObject(surface.dc, old);
                result?;
            }
            assert!(extent.cx <= (long - scaled(64, scale)) * PANEL_SAMPLES);
            state.expanded.set(false);
        }
        Ok(())
    }

    #[test]
    fn recent_colors_keep_slots_on_selection_and_replace_only_oldest() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "FixedRecentRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        unsafe {
            SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(22, 22)));
        }
        let choose = |color| {
            ui.update(
                true,
                Tool::Pen,
                Style {
                    color,
                    thickness: 5,
                },
            )
        };
        let initial = Style::default().color;
        let centers: Vec<_> = (0..5)
            .map(|i| PanelGeometry::new(&ui.state).recent(i))
            .collect();
        for color in [0xff112233, 0xff223344, 0xff334455, 0xff445566] {
            choose(color)?;
            assert_eq!(PanelGeometry::new(&ui.state).recent(0), centers[0]);
        }
        let slots = ui.state.recents.borrow().clone();
        assert_eq!(
            slots,
            [initial, 0xff112233, 0xff223344, 0xff334455, 0xff445566]
        );
        // Dispatch and commit each selection twice, including the selected-color
        // picker request. Every swatch keeps the same center and array index.
        for (i, color) in slots.iter().copied().enumerate().cycle().take(10) {
            let expected = if color == ui.state.color.get() {
                UiEvent::OpenPicker
            } else {
                UiEvent::Color(color)
            };
            panel_click(hwnd, centers[i]);
            assert_eq!(decoded(root.0), [expected]);
            choose(color)?;
            panel_click(hwnd, centers[i]);
            assert_eq!(decoded(root.0), [UiEvent::OpenPicker]);
            assert_eq!(&*ui.state.recents.borrow(), &slots);
            assert_eq!(PanelGeometry::new(&ui.state).recent(i), centers[i]);
        }
        choose(0xff556677)?;
        assert_eq!(
            &*ui.state.recents.borrow(),
            &[0xff556677, slots[1], slots[2], slots[3], slots[4]]
        );
        choose(slots[3])?; // Reusing a color must not advance the replacement slot.
        choose(0xff667788)?;
        assert_eq!(
            &*ui.state.recents.borrow(),
            &[0xff556677, 0xff667788, slots[2], slots[3], slots[4]]
        );
        for color in [0xff778899, 0xff8899aa, 0xff99aabb, 0xffaabbcc] {
            choose(color)?;
        }
        assert_eq!(
            &*ui.state.recents.borrow(),
            &[0xffaabbcc, 0xff667788, 0xff778899, 0xff8899aa, 0xff99aabb]
        );
        assert_eq!(ui.state.next_recent.get(), 1);
        for (i, center) in centers.iter().enumerate() {
            assert_eq!(PanelGeometry::new(&ui.state).recent(i), *center);
        }
        Ok(())
    }

    #[test]
    fn recent_commit_during_picker_preserves_candidate_cancel_and_apply() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "RecentPickerRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let status = status(&ui)?;
        let initial = Style::default().color;
        ui.update(
            true,
            Tool::Pen,
            Style {
                color: 0xff123456,
                thickness: 5,
            },
        )?;
        let hwnd = picker(&ui, initial)?;
        click(hwnd, PRESET);
        let candidate = ui.state.candidate.get();
        let history = ui.state.recents.borrow().clone();
        unsafe {
            SendMessageW(status, WM_MOUSEMOVE, None, Some(point(22, 22)));
        }
        panel_click(status, PanelGeometry::new(&ui.state).current());
        panel_click(status, PanelGeometry::new(&ui.state).recent(1));
        assert_eq!(decoded(root.0), [UiEvent::OpenPicker, UiEvent::OpenPicker]);
        assert_eq!(&*ui.state.recents.borrow(), &history);
        assert_eq!(ui.state.candidate.get(), candidate);
        panel_click(status, PanelGeometry::new(&ui.state).recent(0));
        assert_eq!(decoded(root.0), [UiEvent::Color(initial)]);
        assert!(ui.is_picker_open());
        assert_eq!(ui.state.candidate.get(), candidate);
        ui.update(true, Tool::Pen, Style::default())?;
        assert_eq!(&*ui.state.recents.borrow(), &[initial, 0xff123456]);
        assert_eq!(ui.state.candidate.get(), candidate);
        click(hwnd, CANCEL);
        assert_eq!(decoded(root.0), [UiEvent::Closed]);
        assert_eq!(ui.state.color.get(), initial);
        assert_eq!(&*ui.state.recents.borrow(), &[initial, 0xff123456]);
        let hwnd = picker(&ui, initial)?;
        click(hwnd, PRESET);
        click(hwnd, APPLY);
        assert_eq!(
            decoded(root.0),
            [UiEvent::Color(PRESETS[0].1), UiEvent::Closed]
        );
        // History records committed update_label state only, never a queued preview.
        assert_eq!(&*ui.state.recents.borrow(), &[initial, 0xff123456]);
        ui.update(
            true,
            Tool::Pen,
            Style {
                color: PRESETS[0].1,
                thickness: 5,
            },
        )?;
        assert_eq!(
            &*ui.state.recents.borrow(),
            &[initial, 0xff123456, PRESETS[0].1]
        );
        let hwnd = picker(&ui, initial)?;
        ui.state.post(UiEvent::Closed);
        assert_eq!(decoded(root.0), [UiEvent::Closed]);
        assert!(ui.is_picker_open());
        assert!(key(&ui, ui.state.slider.get(), VK_HOME));
        click(hwnd, CANCEL);
        assert_eq!(decoded(root.0), [UiEvent::Closed]);
        Ok(())
    }

    #[test]
    fn hidden_native_disk_picker_controls_candidate_events_and_lifetimes() -> Result<()> {
        // No new/open_picker, ShowWindow, input injection, cursor or focus-changing API.
        unsafe {
            let focus = GetFocus();
            let active = GetActiveWindow();
            let capture = GetCapture();
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let root_class = WindowClass::new(instance, "TestRoot", Some(root_proc))?;
            let root = HiddenRoot(create_hidden(&root_class, HWND::default(), None, 300, 460)?);
            let ui = hidden_ui(root.0)?;
            let status = status(&ui)?;
            assert_eq!(
                SendMessageW(status, WM_NCHITTEST, None, None).0,
                HTTRANSPARENT as isize
            );
            assert_eq!(
                SendMessageW(status, WM_MOUSEACTIVATE, None, None).0,
                MA_NOACTIVATE as isize
            );
            ui.update(
                false,
                Tool::Rectangle,
                Style {
                    color: 0xff123456,
                    thickness: 31,
                },
            )?;
            assert_eq!(
                &*ui.state.text.borrow(),
                "Click-through | Rectangle | 31 px"
            );
            let text_ptr = ui.state.text.borrow().as_ptr();
            ui.update(
                false,
                Tool::Rectangle,
                Style {
                    color: 0xff123456,
                    thickness: 31,
                },
            )?;
            assert_eq!(ui.state.text.borrow().as_ptr(), text_ptr);
            for tool in [
                "Pen",
                "Temporary pen",
                "Arrow head-first",
                "Arrow tail-first",
            ] {
                ui.update_label(false, tool, Style::default())?;
                assert_eq!(
                    &*ui.state.text.borrow(),
                    &format!("Click-through | {tool} | 5 px")
                );
            }
            let committed = ui.state.color.get();
            let hwnd = picker(&ui, 0xfff97316)?;
            let tab_ids = [
                DISK,
                BRIGHTNESS,
                HEX,
                PRESET,
                PRESET + 1,
                PRESET + 2,
                PRESET + 3,
                PRESET + 4,
                PRESET + 5,
                CANCEL,
                APPLY,
                CLOSE,
            ];
            for id in tab_ids {
                let child = GetDlgItem(Some(hwnd), id as i32)?;
                assert!(IsChild(hwnd, child).as_bool());
                assert!(!IsWindowVisible(child).as_bool());
                assert!(GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_TABSTOP.0 != 0);
                assert!(!window_text(child).is_empty());
            }
            for ids in tab_ids.windows(2) {
                let first = GetDlgItem(Some(hwnd), ids[0] as i32)?;
                assert_eq!(
                    GetNextDlgTabItem(hwnd, Some(first), false)?,
                    GetDlgItem(Some(hwnd), ids[1] as i32)?
                );
            }
            assert_eq!(window_text(ui.state.edit.get()), "#F97316");
            assert!(window_text(ui.state.disk.get()).starts_with("Hue and saturation:"));
            assert!(window_text(ui.state.slider.get()).starts_with("Brightness:"));
            let color_dc = CreateCompatibleDC(None);
            assert!(!color_dc.is_invalid());
            let brush = SendMessageW(
                hwnd,
                WM_CTLCOLOREDIT,
                Some(WPARAM(color_dc.0 as usize)),
                Some(LPARAM(ui.state.edit.get().0 as isize)),
            );
            assert_eq!(GetTextColor(color_dc), colorref(FOREGROUND));
            assert_eq!(GetBkColor(color_dc), colorref(SURFACE));
            let mut description = LOGBRUSH::default();
            assert!(
                GetObjectW(
                    HGDIOBJ(brush.0 as _),
                    size_of::<LOGBRUSH>() as i32,
                    Some(&mut description as *mut _ as _)
                ) > 0
            );
            assert_eq!(description.lbColor, colorref(SURFACE));
            let _ = DeleteDC(color_dc);
            let disk = ui.state.disk.get();
            let slider = ui.state.slider.get();
            let edit = ui.state.edit.get();
            for (i, (_, color)) in PRESETS.iter().enumerate() {
                click(hwnd, PRESET + i);
                assert_eq!(ui.state.candidate.get().argb(), *color);
                assert_eq!(window_text(edit), hex(*color));
                assert!(decoded(root.0).is_empty());
            }
            click(hwnd, PRESET + 1);
            let hsv = ui.state.candidate.get();
            SendMessageW(disk, WM_KEYDOWN, Some(WPARAM(VK_RIGHT.0 as usize)), None);
            assert!((ui.state.candidate.get().hue - hsv.hue - 3.0).abs() < 1e-10);
            SendMessageW(disk, WM_KEYDOWN, Some(WPARAM(VK_LEFT.0 as usize)), None);
            assert!((ui.state.candidate.get().hue - hsv.hue).abs() < 1e-10);
            SendMessageW(disk, WM_KEYDOWN, Some(WPARAM(VK_DOWN.0 as usize)), None);
            assert!((ui.state.candidate.get().saturation - hsv.saturation + 0.02).abs() < 1e-10);
            SendMessageW(disk, WM_KEYDOWN, Some(WPARAM(VK_UP.0 as usize)), None);
            assert!((ui.state.candidate.get().saturation - hsv.saturation).abs() < 1e-10);
            SendMessageW(slider, WM_KEYDOWN, Some(WPARAM(VK_HOME.0 as usize)), None);
            assert_eq!(ui.state.candidate.get().value, 0.0);
            SendMessageW(slider, WM_KEYDOWN, Some(WPARAM(VK_RIGHT.0 as usize)), None);
            assert_eq!(ui.state.candidate.get().value, 0.02);
            SendMessageW(slider, WM_KEYDOWN, Some(WPARAM(VK_LEFT.0 as usize)), None);
            assert_eq!(ui.state.candidate.get().value, 0.0);
            SendMessageW(slider, WM_KEYDOWN, Some(WPARAM(VK_END.0 as usize)), None);
            assert_eq!(ui.state.candidate.get().value, 1.0);
            SendMessageW(disk, WM_LBUTTONDOWN, None, Some(point(118, 118)));
            assert_eq!(ui.state.candidate.get().saturation, 0.0);
            assert_eq!(ui.state.candidate.get().argb(), 0xffffffff);
            assert_eq!(GetCapture(), capture);
            assert!(ui.state.drag.get().is_invalid());
            SendMessageW(disk, WM_LBUTTONDOWN, None, Some(point(8, 118)));
            assert_eq!(ui.state.candidate.get().argb(), 0xffff0000);
            ui.state.pointer(disk, -500, 118);
            assert_eq!(ui.state.candidate.get().saturation, 1.0);
            let before = ui.state.candidate.get();
            SendMessageW(disk, WM_LBUTTONDOWN, None, Some(point(0, 0)));
            SendMessageW(disk, WM_MOUSEMOVE, None, Some(point(118, 8)));
            assert_eq!(ui.state.candidate.get(), before);
            SendMessageW(slider, WM_LBUTTONDOWN, None, Some(point(8, 14)));
            assert_eq!(ui.state.candidate.get().value, 0.0);
            SendMessageW(slider, WM_LBUTTONDOWN, None, Some(point(268, 14)));
            assert_eq!(ui.state.candidate.get().value, 1.0);
            // Simulate a lost/cancelled drag flag, without acquiring system capture.
            ui.state.drag.set(disk);
            SendMessageW(
                disk,
                WM_CAPTURECHANGED,
                None,
                Some(LPARAM(root.0.0 as isize)),
            );
            assert!(ui.state.drag.get().is_invalid());
            SendMessageW(disk, WM_MOUSEMOVE, None, Some(point(118, 118)));
            assert_eq!(ui.state.candidate.get().saturation, 1.0);
            ui.state.drag.set(slider);
            SendMessageW(hwnd, WM_CANCELMODE, None, None);
            assert!(ui.state.drag.get().is_invalid());
            assert_eq!(GetCapture(), capture);
            SetWindowTextW(edit, w!("#123456"))?;
            assert!(ui.state.valid.get());
            assert_eq!(ui.state.candidate.get().argb(), 0xff123456);
            assert_eq!(ui.state.candidate.get(), Hsv::from_argb(0xff123456));
            for text in ["", "#bad", "gg0000", "# 123456", "#1234567", "１２３４５６"] {
                SetWindowTextW(edit, PCWSTR(wide(text).as_ptr()))?;
                assert!(!ui.state.valid.get(), "{text}");
                assert!(!window_text(ui.state.hint.get()).is_empty());
                click(hwnd, APPLY);
                assert!(ui.is_picker_open());
                assert!(decoded(root.0).is_empty());
                assert_eq!(ui.state.candidate.get().argb(), 0xff123456);
            }
            // SetWindowText bypasses EM_SETLIMITTEXT. Never validate a truncated prefix.
            let long_invalid = format!("#123456{}junk", " ".repeat(128));
            SetWindowTextW(edit, PCWSTR(wide(&long_invalid).as_ptr()))?;
            assert!(!ui.state.valid.get());
            click(hwnd, APPLY);
            assert!(decoded(root.0).is_empty());
            assert!(ui.is_picker_open());
            assert_eq!(ui.state.color.get(), committed);
            SetWindowTextW(edit, w!("abcdef"))?;
            assert!(ui.state.valid.get());
            assert!(window_text(ui.state.hint.get()).is_empty());
            assert!(key(
                &ui,
                GetDlgItem(Some(hwnd), (PRESET + 1) as i32)?,
                VK_RETURN
            ));
            assert!(ui.is_picker_open());
            assert_eq!(ui.state.candidate.get().argb(), 0xfff97316);
            assert!(decoded(root.0).is_empty());
            write_preview(&ui)?;
            let first_font = ui.state.font.get();
            ui.state.scale.set(1.5);
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                450,
                scaled(PICKER_HEIGHT, 1.5),
                SWP_NOACTIVATE | SWP_NOZORDER,
            )?;
            ui.state.layout()?;
            for id in tab_ids {
                let child = GetDlgItem(Some(hwnd), id as i32)?;
                let mut bounds = RECT::default();
                GetClientRect(child, &mut bounds)?;
                let (_, _, w, h) = control_rect(id);
                assert_eq!(
                    (bounds.right, bounds.bottom),
                    (scaled(w, 1.5), scaled(h, 1.5))
                );
            }
            let mut description = LOGFONTW::default();
            assert_eq!(
                GetObjectW(
                    first_font.into(),
                    size_of::<LOGFONTW>() as i32,
                    Some(&mut description as *mut _ as _)
                ),
                0
            );
            assert!(
                GetObjectW(
                    ui.state.font.get().into(),
                    size_of::<LOGFONTW>() as i32,
                    Some(&mut description as *mut _ as _)
                ) > 0
            );
            assert_eq!(description.lfHeight, -21);
            write_preview(&ui)?;
            assert!(key(&ui, slider, VK_HOME));
            assert_eq!(ui.state.candidate.get().value, 0.0);
            assert!(key(&ui, slider, VK_END));
            assert_eq!(ui.state.candidate.get().value, 1.0);
            click(hwnd, PRESET + 1);
            let font = ui.state.font.get();
            let bitmap = ui.state.disk_pixels(220, ui.state.candidate.get().value);
            let weak = Rc::downgrade(&bitmap);
            drop(bitmap);
            assert!(key(&ui, edit, VK_RETURN));
            let messages = events(root.0);
            assert_eq!(
                messages
                    .iter()
                    .filter_map(|m| ui.event(m))
                    .collect::<Vec<_>>(),
                [UiEvent::Color(0xfff97316), UiEvent::Closed]
            );
            assert_eq!(
                (messages[0].wParam.0, messages[0].lParam.0),
                (0xfff97316, 1)
            );
            assert_eq!((messages[1].wParam.0, messages[1].lParam.0), (0, 2));
            assert!(!IsWindow(Some(hwnd)).as_bool());
            for control in [disk, slider, edit] {
                assert!(!IsWindow(Some(control)).as_bool());
            }
            assert!(weak.upgrade().is_none());
            let mut description = LOGFONTW::default();
            assert_eq!(
                GetObjectW(
                    font.into(),
                    size_of::<LOGFONTW>() as i32,
                    Some(&mut description as *mut _ as _)
                ),
                0
            );
            ui.state.scale.set(1.0);
            for id in [CANCEL, CLOSE] {
                let hwnd = picker(&ui, 0xff0000ff)?;
                click(hwnd, PRESET);
                assert!(key(&ui, GetDlgItem(Some(hwnd), id as i32)?, VK_RETURN));
                assert_eq!(decoded(root.0), [UiEvent::Closed]);
            }
            let hwnd = picker(&ui, 0xff010203)?;
            click(hwnd, APPLY);
            assert_eq!(
                decoded(root.0),
                [UiEvent::Color(0xff010203), UiEvent::Closed]
            );
            ui.close_picker();
            assert!(decoded(root.0).is_empty());
            let hwnd = picker(&ui, 0xff0000ff)?;
            assert!(key(&ui, hwnd, VK_ESCAPE));
            assert_eq!(decoded(root.0), [UiEvent::Closed]);
            let hwnd = picker(&ui, 0xff0000ff)?;
            SendMessageW(hwnd, WM_CLOSE, None, None);
            assert_eq!(decoded(root.0), [UiEvent::Closed]);
            let hwnd = picker(&ui, 0xff0000ff)?;
            ui.state.drag.set(ui.state.disk.get());
            let brush = ui.state.edit_brush.0;
            let classes =
                [&ui.status_class, &ui.picker_class, &ui.widget_class].map(|c| c.name.clone());
            // Queued events contain values only and survive releasing State/classes.
            ui.state.post(UiEvent::Color(0xff010203));
            drop(ui);
            assert!(!IsWindow(Some(hwnd)).as_bool());
            assert!(!IsWindow(Some(status)).as_bool());
            let mut description = LOGBRUSH::default();
            assert_eq!(
                GetObjectW(
                    brush,
                    size_of::<LOGBRUSH>() as i32,
                    Some(&mut description as *mut _ as _)
                ),
                0
            );
            assert_eq!(
                decoded(root.0),
                [UiEvent::Color(0xff010203), UiEvent::Closed]
            );
            for name in classes {
                let mut info = WNDCLASSW::default();
                assert!(GetClassInfoW(Some(instance), PCWSTR(name.as_ptr()), &mut info).is_err());
            }
            assert_eq!(GetFocus(), focus);
            assert_eq!(GetActiveWindow(), active);
            assert_eq!(GetCapture(), capture);
            assert!(decoded(root.0).is_empty());
        }
        Ok(())
    }
    fn panel_preview(ui: &DesktopUi, name: &str) -> Result<Vec<u32>> {
        let geometry = PanelGeometry::new(&ui.state);
        let pixels = panel_bitmap(&ui.state)?.1;
        // Compose the exact premultiplied upload over a dark background for review.
        let composed: Vec<u32> = pixels
            .iter()
            .map(|pixel| {
                let alpha = pixel >> 24;
                let mut color = 0xff000000;
                for shift in [0, 8, 16] {
                    color |= (((pixel >> shift) & 255)
                        + (((0xff081421_u32 >> shift) & 255) * (255 - alpha) + 127) / 255)
                        << shift;
                }
                color
            })
            .collect();
        let path = std::env::temp_dir().join(format!("{name}.png"));
        std::fs::write(
            &path,
            png(geometry.width as u32, geometry.height as u32, &composed),
        )
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        println!("Offscreen preview: {}", path.display());
        Ok(pixels)
    }

    fn preview(
        width: i32,
        height: i32,
        name: &str,
        paint: impl FnOnce(HDC) -> Result<()>,
    ) -> Result<Vec<u32>> {
        {
            let surface = Bitmap::new(width, height)?;
            fill(surface.dc, rect(0, 0, width, height), BACKGROUND);
            paint(surface.dc)?;
            let pixels = surface.pixels().to_vec();
            let path = std::env::temp_dir().join(format!("{name}.png"));
            std::fs::write(&path, png(width as u32, height as u32, &pixels))
                .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
            println!("Offscreen preview: {}", path.display());
            drop(surface);
            Ok(pixels)
        }
    }

    fn write_preview(ui: &DesktopUi) -> Result<()> {
        let s = ui.state.scale.get();
        let width = scaled(PICKER_WIDTH, s);
        let height = scaled(PICKER_HEIGHT, s);
        let pixels = preview(
            width,
            height,
            if s == 1.0 {
                "mado-disk-picker-preview"
            } else {
                "mado-disk-picker-preview-150"
            },
            |dc| unsafe {
                let saved = SaveDC(dc);
                SelectObject(dc, ui.state.font.get().into());
                for (text, bounds) in [
                    ("Hue + saturation", logical_rect(s, 32, 28, 236, 20)),
                    (
                        "Apply saves. Cancel keeps the old color.",
                        logical_rect(s, 12, 474, 276, 20),
                    ),
                ] {
                    let mut measured = RECT::default();
                    let mut text: Vec<u16> = text.encode_utf16().collect();
                    DrawTextW(dc, &mut text, &mut measured, DT_CALCRECT | DT_SINGLELINE);
                    assert!(measured.right <= bounds.right - bounds.left);
                    assert!(measured.bottom <= bounds.bottom - bounds.top);
                    assert!(bounds.right <= width && bounds.bottom <= height);
                }
                let _ = RestoreDC(dc, saved);
                SendMessageW(
                    ui.state.picker.get(),
                    WM_PRINTCLIENT,
                    Some(WPARAM(dc.0 as usize)),
                    None,
                );
                for id in [DISK, BRIGHTNESS, HEX_LABEL, HEX, HINT, CANCEL, APPLY, CLOSE]
                    .into_iter()
                    .chain(PRESET..PRESET + PRESETS.len())
                {
                    let child = GetDlgItem(Some(ui.state.picker.get()), id as i32)?;
                    let (x, y, _, _) = control_rect(id);
                    let saved = SaveDC(dc);
                    let _ = SetViewportOrgEx(dc, scaled(x, s), scaled(y, s), None);
                    SendMessageW(
                        child,
                        WM_PRINT,
                        Some(WPARAM(dc.0 as usize)),
                        Some(LPARAM(
                            (PRF_CLIENT | PRF_NONCLIENT | PRF_ERASEBKGND) as isize,
                        )),
                    );
                    let _ = RestoreDC(dc, saved);
                }
                Ok(())
            },
        )?;
        assert_eq!(
            pixels[(scaled(358, s) * width + scaled(36, s)) as usize] & 0xffffff,
            0xf97316
        );
        let center = pixels[(scaled(166, s) * width + scaled(150, s)) as usize] & 0xffffff;
        for shift in [0, 8, 16] {
            assert!(((((center >> shift) & 255) as i32) - 249).abs() <= 3);
        }
        Ok(())
    }

    fn png(width: u32, height: u32, pixels: &[u32]) -> Vec<u8> {
        fn crc(bytes: &[u8]) -> u32 {
            let mut crc = !0_u32;
            for byte in bytes {
                crc ^= *byte as u32;
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb88320 & 0_u32.wrapping_sub(crc & 1));
                }
            }
            !crc
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let start = out.len();
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            out.extend_from_slice(&crc(&out[start..]).to_be_bytes());
        }
        // ponytail: uncompressed DEFLATE is larger but sufficient for one test image.
        let mut raw = Vec::new();
        for row in pixels.chunks(width as usize) {
            raw.push(0);
            for pixel in row {
                raw.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
            }
        }
        let mut zlib = vec![0x78, 0x01];
        let mut a = 1_u32;
        let mut b = 0_u32;
        let count = raw.chunks(65535).len();
        for (i, block) in raw.chunks(65535).enumerate() {
            zlib.push(u8::from(i + 1 == count));
            let len = block.len() as u16;
            zlib.extend_from_slice(&len.to_le_bytes());
            zlib.extend_from_slice(&(!len).to_le_bytes());
            zlib.extend_from_slice(block);
            for byte in block {
                a = (a + *byte as u32) % 65521;
                b = (b + a) % 65521;
            }
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"IDAT", &zlib);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn geometry_cache_and_value_only_event_filtering() -> Result<()> {
        let state = State::new(HWND::default())?;
        let pixels = state.disk_pixels(220, 1.0);
        assert!(Rc::ptr_eq(&pixels, &state.disk_pixels(220, 1.0)));
        assert!(!Rc::ptr_eq(&pixels, &state.disk_pixels(220, 0.5)));
        assert!(!Rc::ptr_eq(&pixels, &state.disk_pixels(440, 0.5)));
        assert_eq!(pixels[0], BACKGROUND);
        assert!(pixels[110 * 220 + 110] & 0xffffff > 0xfafafa);
        for work in [
            rect(0, 0, 1920, 1040),
            rect(-1920, -1080, 1920, 1040),
            rect(10, 20, 100, 80),
        ] {
            for dpi in [96, 120, 144, 192] {
                let scale = fitting_scale(dpi, work);
                let size = (scaled(PICKER_WIDTH, scale), scaled(PICKER_HEIGHT, scale));
                for cursor in [POINT { x: -3000, y: -3000 }, POINT { x: 3000, y: 3000 }] {
                    let (x, y, w, h) = picker_rect(cursor, work, size);
                    assert!(x >= work.left && y >= work.top);
                    assert!(x + w <= work.right && y + h <= work.bottom);
                    for id in [DISK, BRIGHTNESS, HEX, CANCEL, APPLY, CLOSE] {
                        let (cx, cy, cw, ch) = control_rect(id);
                        assert!(scaled(cx + cw, scale) <= w);
                        assert!(scaled(cy + ch, scale) <= h);
                    }
                }
            }
        }
        let owner = HWND(std::ptr::dangling_mut());
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
        message.wParam = WPARAM(0);
        assert_eq!(decode_event(&message, owner), Some(UiEvent::OpenPicker));
        for payload in [1, u32::MAX as usize, usize::MAX] {
            message.wParam = WPARAM(payload);
            assert_eq!(decode_event(&message, owner), None);
        }
        message.lParam = LPARAM(1);
        message.wParam = WPARAM(u32::MAX as usize + 1);
        assert_eq!(decode_event(&message, owner), None);
        message.message = WM_APP;
        assert_eq!(decode_event(&message, owner), None);
        message.message = UI_EVENT;
        assert_eq!(decode_event(&message, HWND::default()), None);
        Ok(())
    }
}
