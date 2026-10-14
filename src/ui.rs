use crate::{
    picker::{Hsv, disk_pixel, hex, parse_hex},
    strokes::{Style, Tool},
};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                EM_SETLIMITTEXT, EM_SETMARGINS, ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX,
                InitCommonControlsEx, TOOLTIPS_CLASSW, TTF_SUBCLASS, TTF_TRANSPARENT, TTM_ACTIVATE,
                TTM_ADDTOOLW, TTM_NEWTOOLRECTW, TTM_POP, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP,
                TTS_NOPREFIX, TTTOOLINFOW, WM_MOUSELEAVE,
            },
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, Error, PCWSTR, Result, w},
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
const PANEL_BACKGROUND: u32 = 0xff191c20;
const PANEL_BORDER: u32 = 0xff444b52;
const PANEL_TEXT: u32 = 0xfff1eee8;
const PANEL_SECONDARY: u32 = 0xffadb4bb;
const PANEL_ACCENT: u32 = 0xff7cc7b4;
const PANEL_WIDTH: i32 = 440;
const PANEL_HEIGHT: i32 = 304;
const PANEL_ROW: i32 = 44;
const PANEL_SAMPLES: i32 = 4;
const PANEL_ANIMATION_TIMER: usize = 0x4d01;
const PANEL_DRAG_TIMER: usize = 0x4d02;
const PANEL_LEAVE_TIMER: usize = 0x4d03;
const PANEL_DURATION: Duration = Duration::from_millis(150);
const PANEL_PALETTE: [u32; 16] = [
    0xffffdf32, 0xffffb51b, 0xfff97316, 0xffe31635, 0xff673b92, 0xff9b4fc0, 0xffd50d81, 0xffb9085e,
    0xff35c7ef, 0xff0085dc, 0xff075cbf, 0xff83c400, 0xff00b85a, 0xfff1f1ef, 0xffbdbdbb, 0xff161616,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTool {
    Pen = 0,
    TemporaryPen = 1,
    Line = 2,
    Arrow = 3,
    Rectangle = 4,
    Ellipse = 5,
    Eraser = 6,
}

const PANEL_TOOLS: [(MenuTool, &str, &str, &str); 7] = [
    (
        MenuTool::Pen,
        "Pen",
        "P",
        "Pen. P selects pen, then P again toggles temporary ink. Click selects permanent pen.",
    ),
    (
        MenuTool::TemporaryPen,
        "Temp",
        "P",
        "Temporary pen. P selects pen, then P again toggles temporary ink. Click selects temporary pen.",
    ),
    (
        MenuTool::Line,
        "Line",
        "L",
        "Line. L selects a straight line.",
    ),
    (
        MenuTool::Arrow,
        "Arrow",
        "A",
        "Arrow. Repeat A or click selected Arrow to flip direction.",
    ),
    (
        MenuTool::Rectangle,
        "Rectangle",
        "R",
        "Rectangle. R selects an outline rectangle.",
    ),
    (
        MenuTool::Ellipse,
        "Ellipse",
        "O",
        "Ellipse. O selects an outline ellipse.",
    ),
    (
        MenuTool::Eraser,
        "Eraser",
        "E",
        "Eraser. E selects whole-annotation erasing.",
    ),
];

#[derive(Debug, PartialEq, Eq)]
pub enum UiEvent {
    Color(u32),
    Closed,
    OpenPicker,
    Tool(MenuTool),
    Width(u32),
    Toggle,
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
    tool: Cell<MenuTool>,
    tool_label: RefCell<String>,
    drawing: Cell<bool>,
    width: Cell<u32>,
    expanded: Cell<bool>,
    motion: Cell<PanelMotion>,
    animation_timer: Cell<bool>,
    leave_timer: Cell<bool>,
    circle_leave_point: Cell<Option<POINT>>,
    panel_pixels: RefCell<Vec<u32>>,
    #[cfg(test)]
    panel_upload: Cell<Option<(Instant, bool)>>,
    panel_anchor: Cell<POINT>,
    panel_upward: Cell<bool>,
    panel_drag: Cell<Option<PanelDrag>>,
    panel_capture: Cell<bool>,
    panel_guard: Cell<bool>,
    tooltip: Cell<HWND>,
    tooltip_hit: Cell<Option<PanelHit>>,
    hover: Cell<Option<PanelHit>>,
    tooltip_text: RefCell<Vec<u16>>,
    panel_scale: Cell<f64>,
    panel_dpi: Cell<u32>,
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
            tool: Cell::new(MenuTool::Pen),
            tool_label: RefCell::new("Pen".into()),
            drawing: Cell::new(false),
            width: Cell::new(Style::default().thickness),
            expanded: Cell::new(false),
            motion: Cell::new(PanelMotion::settled(false)),
            animation_timer: Cell::new(false),
            leave_timer: Cell::new(false),
            circle_leave_point: Cell::new(None),
            panel_pixels: RefCell::new(Vec::new()),
            #[cfg(test)]
            panel_upload: Cell::new(None),
            panel_anchor: Cell::new(POINT::default()),
            panel_upward: Cell::new(false),
            panel_drag: Cell::new(None),
            panel_capture: Cell::new(false),
            panel_guard: Cell::new(false),
            tooltip: Cell::new(HWND::default()),
            tooltip_hit: Cell::new(None),
            hover: Cell::new(None),
            tooltip_text: RefCell::new(Vec::new()),
            panel_scale: Cell::new(1.0),
            panel_dpi: Cell::new(96),
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
            ui.state.create_tooltip(instance)?;
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
        *self.state.tool_label.borrow_mut() = tool.into();
        self.state.tool.set(tool_icon(tool));
        self.state.drawing.set(drawing);
        self.state.width.set(style.thickness);
        self.state.color.set(style.color);
        // A queued request never changes selection. Only the owner's committed update does.
        if self.state.panel_drag.get().is_none()
            && self.state.pressed.get() != Some(PanelHit::Width)
        {
            self.state.pressed.set(None);
        }
        self.state.hide_tooltip();
        unsafe {
            let _ = SetWindowTextW(
                self.state.status.get(),
                PCWSTR(wide(&format!("Mado {tool}; {mode}")).as_ptr()),
            );
        }
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
            self.state.cancel_panel_gesture();
            self.state.hide_tooltip();
            let _ = self.state.resize_panel();
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
        self.state.destroy_panel_resources();
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
    Grip,
    Toggle,
    Current,
    Palette(usize),
    Width,
    Tool(MenuTool),
}

#[derive(Clone, Copy, Debug)]
struct PanelMotion {
    from: f64,
    to: f64,
    progress: f64,
    started: Instant,
}
impl PanelMotion {
    fn settled(open: bool) -> Self {
        let progress = if open { 1.0 } else { 0.0 };
        Self {
            from: progress,
            to: progress,
            progress,
            started: Instant::now(),
        }
    }
    fn moving(self) -> bool {
        self.progress != self.to
    }
    fn sample(mut self, now: Instant) -> Self {
        let t = (now.saturating_duration_since(self.started).as_secs_f64()
            / PANEL_DURATION.as_secs_f64())
        .clamp(0.0, 1.0);
        self.progress = if t == 1.0 {
            self.to
        } else {
            self.from + (self.to - self.from) * (1.0 - (1.0 - t).powi(3))
        };
        self
    }
    fn redirect(self, open: bool, animated: bool, now: Instant) -> Self {
        let current = self.sample(now);
        let to = if open { 1.0 } else { 0.0 };
        if !animated {
            return Self::settled(open);
        }
        Self {
            from: current.progress,
            to,
            progress: current.progress,
            started: now,
        }
    }
}

#[derive(Clone, Copy)]
struct PanelDrag {
    start: POINT,
    anchor: POINT,
    scale: f64,
    moving: bool,
}

struct PanelGeometry {
    width: i32,
    height: i32,
    radius: i32,
    body: RECT,
    tip_radius: f64,
    scale: f64,
    upward: bool,
    opacity: f64,
}
impl PanelGeometry {
    fn new(state: &State) -> Self {
        Self::with_motion(state, state.motion.get())
    }
    fn with_motion(state: &State, motion: PanelMotion) -> Self {
        let scale = state.panel_scale.get();
        let progress = motion.progress;
        let width = scaled(PANEL_ROW, scale)
            + scaled(
                ((PANEL_WIDTH - PANEL_ROW) as f64 * progress).round() as i32,
                scale,
            );
        let height = scaled(PANEL_ROW, scale)
            + scaled(
                ((PANEL_HEIGHT - PANEL_ROW) as f64 * progress).round() as i32,
                scale,
            );
        let tip_radius = if progress == 0.0 && motion.to == 0.0 && !state.expanded.get() {
            (5.0 + state.width.get().saturating_sub(1).min(30) as f64 * 0.2) * scale
        } else {
            0.0
        };
        let left = tip_radius.ceil() as i32;
        let bottom = (tip_radius - 4.0 * scale).max(0.0).ceil() as i32;
        Self {
            width: width + left,
            height: height + bottom,
            body: rect(left, 0, width, height),
            tip_radius,
            radius: scaled(6, scale).max(1),
            scale,
            upward: state.panel_upward.get(),
            opacity: progress,
        }
    }
    fn badge(&self) -> RECT {
        rect(
            self.body.right - scaled(44, self.scale),
            if self.upward {
                self.body.bottom - scaled(44, self.scale)
            } else {
                self.body.top
            },
            scaled(44, self.scale),
            scaled(44, self.scale),
        )
    }
    fn bounds(&self, hit: PanelHit) -> RECT {
        let s = self.scale;
        let ox = self.body.right - scaled(PANEL_WIDTH, s);
        let oy = if self.upward {
            self.body.bottom - scaled(PANEL_HEIGHT, s)
        } else {
            self.body.top
        };
        let body = if self.upward { -44 } else { 0 };
        let local = match hit {
            PanelHit::Grip => {
                let badge = self.badge();
                return rect(
                    badge.right - scaled(12, s),
                    badge.top + scaled(8, s),
                    scaled(10, s),
                    scaled(28, s),
                );
            }
            PanelHit::Toggle => rect(112, if self.upward { 267 } else { 7 }, 266, 31),
            PanelHit::Current => rect(24, 48 + body, 392, 32),
            PanelHit::Width => rect(24, 88 + body, 348, 32),
            PanelHit::Palette(i) => rect(
                37 + (i % 8) as i32 * 48,
                135 + body + (i / 8) as i32 * 46,
                30,
                30,
            ),
            PanelHit::Tool(tool) => {
                let i = PANEL_TOOLS
                    .iter()
                    .position(|entry| entry.0 == tool)
                    .unwrap();
                rect(14 + i as i32 * 60, 238 + body, 44, 36)
            }
        };
        rect(
            ox + scaled(local.left, s),
            oy + scaled(local.top, s),
            scaled(local.right - local.left, s),
            scaled(local.bottom - local.top, s),
        )
    }
    fn center(&self, hit: PanelHit) -> (i32, i32) {
        let r = self.bounds(hit);
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return false;
        }
        if self.tip_radius > 0.0 {
            let (cx, cy) = self.tip_center();
            // Include antialiased edge pixels, not only pixel centers. Native
            // hit testing still uses the uploaded alpha, so empty corners pass through.
            let dx = (cx - cx.clamp(x, x + 1)) as f64;
            let dy = (cy - cy.clamp(y, y + 1)) as f64;
            if dx * dx + dy * dy <= self.tip_radius.powi(2) {
                return true;
            }
        }
        let r = self.body;
        if x < r.left || x >= r.right || y < r.top || y >= r.bottom {
            return false;
        }
        let cx = x.clamp(r.left + self.radius, r.right - self.radius - 1);
        let cy = y.clamp(r.top + self.radius, r.bottom - self.radius - 1);
        (x - cx).pow(2) + (y - cy).pow(2) <= self.radius.pow(2)
    }
    fn tip_center(&self) -> (i32, i32) {
        let badge = self.badge();
        (badge.left, badge.top + scaled(40, self.scale))
    }
    fn destination(&self, anchor: POINT) -> POINT {
        let badge = self.badge();
        POINT {
            x: anchor.x - badge.right,
            y: anchor.y - badge.top,
        }
    }
    fn revealed(&self, hit: PanelHit) -> bool {
        let r = self.bounds(hit);
        hit == PanelHit::Grip
            || (self.opacity >= 0.85
                && r.left >= 0
                && r.top >= 0
                && r.right <= self.width
                && r.bottom <= self.height)
    }
    fn hit(&self, state: &State, x: i32, y: i32) -> Option<PanelHit> {
        if !self.contains(x, y) {
            return None;
        }
        let targets = [
            PanelHit::Grip,
            PanelHit::Toggle,
            PanelHit::Current,
            PanelHit::Width,
        ]
        .into_iter()
        .chain((0..16).map(PanelHit::Palette))
        .chain(PANEL_TOOLS.iter().map(|entry| PanelHit::Tool(entry.0)));
        for hit in targets {
            if !self.revealed(hit)
                || (!state.picker.get().is_invalid()
                    && !matches!(hit, PanelHit::Toggle | PanelHit::Grip))
            {
                continue;
            }
            let r = self.bounds(hit);
            let inside = if matches!(hit, PanelHit::Palette(_)) {
                let (cx, cy) = self.center(hit);
                (x - cx).pow(2) + (y - cy).pow(2) <= scaled(15, self.scale).pow(2)
            } else {
                x >= r.left && x < r.right && y >= r.top && y < r.bottom
            };
            if inside {
                return Some(hit);
            }
        }
        None
    }
    fn requested_width(&self, x: i32) -> u32 {
        let r = self.bounds(PanelHit::Width);
        let step = (((x - r.left) as f64 / (r.right - r.left).max(1) as f64).clamp(0.0, 1.0) * 15.0)
            .round() as u32;
        1 + 2 * step
    }
}

fn tool_icon(label: &str) -> MenuTool {
    match label {
        "Temporary pen" => MenuTool::TemporaryPen,
        "Line" => MenuTool::Line,
        "Rectangle" => MenuTool::Rectangle,
        "Ellipse" => MenuTool::Ellipse,
        "Eraser" => MenuTool::Eraser,
        label if label.starts_with("Arrow") => MenuTool::Arrow,
        _ => MenuTool::Pen,
    }
}

fn panel_scale(dpi: u32, work: RECT) -> f64 {
    (dpi.max(96) as f64 / 96.0)
        .min((work.right - work.left).max(1) as f64 / PANEL_WIDTH as f64)
        // Leave one physical pixel for independently rounded body/tip extents.
        .min((work.bottom - work.top - 1).max(1) as f64 / (PANEL_HEIGHT + 7) as f64)
}

// Anchor is the badge's screen-space right edge and top. Upward opening keeps it fixed.
fn panel_clamp(anchor: POINT, work: RECT, scale: f64) -> (POINT, bool) {
    let width = scaled(PANEL_WIDTH, scale);
    let height = scaled(PANEL_HEIGHT, scale);
    let badge = scaled(PANEL_ROW, scale);
    let tip_bottom = (7.0 * scale).ceil() as i32;
    let upward = anchor.y + height > work.bottom;
    let y = if upward {
        anchor
            .y
            .clamp(work.top + height - badge, work.bottom - badge - tip_bottom)
    } else {
        anchor.y.clamp(work.top, work.bottom - height)
    };
    (
        POINT {
            x: anchor.x.clamp(work.left + width, work.right),
            y,
        },
        upward,
    )
}

fn panel_animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfow
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    enabled.as_bool()
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
        self.panel_dpi.set(dpi);
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
            self.panel_monitor.set(info.rcWork);
            let scale = panel_scale(dpi, info.rcWork);
            self.panel_scale.set(scale);
            let (anchor, upward) = panel_clamp(
                POINT {
                    x: info.rcWork.right - scaled(16, scale),
                    y: info.rcWork.top + scaled(16, scale),
                },
                info.rcWork,
                scale,
            );
            self.panel_anchor.set(anchor);
            self.panel_upward.set(upward);
        }
        self.resize_panel()
    }

    fn place_panel(&self, anchor: POINT, dpi: u32) -> Result<()> {
        self.panel_dpi.set(dpi);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetMonitorInfoW(
                MonitorFromPoint(
                    POINT {
                        x: anchor.x - 1,
                        y: anchor.y,
                    },
                    MONITOR_DEFAULTTONEAREST,
                ),
                &mut info,
            )
            .ok()?;
        }
        let scale = panel_scale(dpi, info.rcWork);
        let (anchor, upward) = panel_clamp(anchor, info.rcWork, scale);
        self.panel_scale.set(scale);
        self.panel_monitor.set(info.rcWork);
        self.panel_anchor.set(anchor);
        self.panel_upward.set(upward);
        self.resize_panel()
    }

    fn resize_panel(&self) -> Result<()> {
        let geometry = PanelGeometry::new(self);
        if self.status.get().is_invalid() {
            return Ok(());
        }
        let anchor = self.panel_anchor.get();
        let (surface, pixels) = panel_bitmap(self)?;
        let destination = geometry.destination(anchor);
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
        #[cfg(test)]
        self.panel_upload
            .set(Some((Instant::now(), self.animation_timer.get())));
        self.tooltip_rect();
        Ok(())
    }

    fn reveal_panel(&self, open: bool) {
        self.start_panel_reveal(open, panel_animations_enabled());
    }

    fn start_panel_reveal(&self, open: bool, animated: bool) {
        if self.panel_drag.get().is_some() || self.pressed.get() == Some(PanelHit::Width) {
            return;
        }
        if self.expanded.replace(open) == open
            && self.motion.get().to == if open { 1.0 } else { 0.0 }
        {
            return;
        }
        self.hide_tooltip();
        if self.animation_timer.replace(false) {
            unsafe {
                let _ = KillTimer(Some(self.status.get()), PANEL_ANIMATION_TIMER);
            }
        }
        let now = Instant::now();
        self.motion
            .set(self.motion.get().redirect(open, animated, now));
        // Upload the current image before starting the clock. Rendering it is not
        // part of the 150 ms transition, particularly when reversing a large frame.
        let _ = self.resize_panel();
        let mut motion = self.motion.get();
        motion.started = Instant::now();
        self.motion.set(motion);
        self.sync_animation_timer();
        if motion.moving() && !self.motion.get().moving() {
            let _ = self.resize_panel();
        }
    }

    fn sync_animation_timer(&self) {
        let hwnd = self.status.get();
        let moving = self.motion.get().moving() && !hwnd.is_invalid();
        unsafe {
            // KillTimer leaves queued messages behind; the flag gates their dispatch.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-settimer
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-killtimer
            if moving && !self.animation_timer.get() {
                self.animation_timer
                    .set(SetTimer(Some(hwnd), PANEL_ANIMATION_TIMER, 15, None) != 0);
                if !self.animation_timer.get() {
                    self.motion.set(PanelMotion::settled(self.expanded.get()));
                }
            } else if !moving && self.animation_timer.replace(false) {
                let _ = KillTimer(Some(hwnd), PANEL_ANIMATION_TIMER);
            }
        }
    }

    fn opening_hover_bridge(&self, screen: POINT) -> bool {
        let motion = self.motion.get();
        if motion.to != 1.0 || !motion.moving() {
            return false;
        }
        // The first reveal removes the circle's padding. Keep that hover alive
        // until the growing body reaches it; transparent pixels still pass through.
        self.target_contains(screen, true) || self.initial_circle_contains(screen)
    }

    fn target_contains(&self, screen: POINT, open: bool) -> bool {
        let geometry = PanelGeometry::with_motion(self, PanelMotion::settled(open));
        let origin = geometry.destination(self.panel_anchor.get());
        geometry.contains(screen.x - origin.x, screen.y - origin.y)
    }

    fn initial_circle_contains(&self, screen: POINT) -> bool {
        let scale = self.panel_scale.get();
        let anchor = self.panel_anchor.get();
        let radius = (5.0 + self.width.get().saturating_sub(1).min(30) as f64 * 0.2) * scale;
        ((screen.x - anchor.x + scaled(44, scale)) as f64).powi(2)
            + ((screen.y - anchor.y - scaled(40, scale)) as f64).powi(2)
            <= radius.powi(2)
    }

    fn allow_hover_reveal(&self, screen: POINT) -> bool {
        if self.circle_leave_point.get() == Some(screen) {
            return false;
        }
        self.circle_leave_point.set(None);
        true
    }

    fn stop_leave_timer(&self) {
        if self.leave_timer.replace(false) {
            unsafe {
                let _ = KillTimer(Some(self.status.get()), PANEL_LEAVE_TIMER);
            }
        }
    }

    fn defer_circle_leave(&self, screen: POINT) -> bool {
        if !self.expanded.get() || !self.initial_circle_contains(screen) {
            self.stop_leave_timer();
            return false;
        }
        if self.target_contains(screen, true) {
            self.track_panel_leave();
            return true;
        }
        // One finite grace period for the exposed upward circle cap. After collapse,
        // the same stationary pointer cannot immediately reopen it. No idle polling.
        if !self.leave_timer.get() && !self.status.get().is_invalid() {
            self.leave_timer.set(unsafe {
                SetTimer(Some(self.status.get()), PANEL_LEAVE_TIMER, 250, None) != 0
            });
        }
        self.leave_timer.get()
    }

    fn finish_circle_leave(&self, screen: POINT) {
        self.stop_leave_timer();
        if !self.target_contains(screen, true) {
            if self.initial_circle_contains(screen) {
                self.circle_leave_point.set(Some(screen));
            }
            self.reveal_panel(false);
        }
    }

    fn panel_leave(&self) {
        self.tracking.set(false);
        self.set_panel_hover(None);
        self.hide_tooltip();
        let mut screen = POINT::default();
        if unsafe { GetCursorPos(&mut screen).is_ok() }
            && (self.opening_hover_bridge(screen) || self.defer_circle_leave(screen))
        {
            return;
        }
        if self.panel_drag.get().is_none() && self.pressed.get() != Some(PanelHit::Width) {
            self.cancel_panel_gesture();
            self.reveal_panel(false);
        }
    }

    fn create_tooltip(&self, instance: HINSTANCE) -> Result<()> {
        unsafe {
            InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_WIN95_CLASSES,
            })
            .ok()?;
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                TOOLTIPS_CLASSW,
                None,
                WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                0,
                0,
                0,
                0,
                Some(self.status.get()),
                None,
                Some(instance),
                None,
            )?;
            self.tooltip.set(hwnd);
            *self.tooltip_text.borrow_mut() =
                wide("Mado selected tool. Drag the six-dot grip to move the menu.");
            let info = self.tooltip_info();
            if SendMessageW(
                hwnd,
                TTM_ADDTOOLW,
                None,
                Some(LPARAM(&info as *const _ as isize)),
            )
            .0 == 0
            {
                let _ = DestroyWindow(hwnd);
                self.tooltip.set(HWND::default());
                return Err(Error::from_thread());
            }
            self.hide_tooltip();
        }
        Ok(())
    }

    fn tooltip_info(&self) -> TTTOOLINFOW {
        // The pointer stays owned by State. Never hold its RefCell borrow in SendMessage.
        let text = self.tooltip_text.borrow().as_ptr() as *mut u16;
        TTTOOLINFOW {
            // The process uses the default common-controls version, which accepts V2 size.
            cbSize: std::mem::offset_of!(TTTOOLINFOW, lpReserved) as u32,
            uFlags: TTF_SUBCLASS | TTF_TRANSPARENT,
            hwnd: self.status.get(),
            uId: 1,
            rect: rect(
                0,
                0,
                PanelGeometry::new(self).width,
                PanelGeometry::new(self).height,
            ),
            lpszText: windows::core::PWSTR(text),
            ..Default::default()
        }
    }

    fn tooltip_rect(&self) {
        let hwnd = self.tooltip.get();
        if hwnd.is_invalid() {
            return;
        }
        let info = self.tooltip_info();
        unsafe {
            SendMessageW(
                hwnd,
                TTM_NEWTOOLRECTW,
                None,
                Some(LPARAM(&info as *const _ as isize)),
            );
        }
    }

    fn hide_tooltip(&self) {
        self.tooltip_hit.set(None);
        let hwnd = self.tooltip.get();
        if !hwnd.is_invalid() {
            unsafe {
                SendMessageW(hwnd, TTM_POP, None, None);
                SendMessageW(hwnd, TTM_ACTIVATE, Some(WPARAM(0)), None);
            }
        }
    }

    fn panel_tooltip(&self, hit: Option<PanelHit>) {
        if self.tooltip_hit.get() == hit {
            return;
        }
        self.hide_tooltip();
        let hwnd = self.tooltip.get();
        let Some(hit) = hit.filter(|_| !hwnd.is_invalid()) else {
            return;
        };
        let description = match hit {
            PanelHit::Grip => "Drag the six-dot grip to move the menu.".to_string(),
            PanelHit::Toggle => "Drawing / Click-through. Ctrl+Alt+D toggles canvas input.".into(),
            PanelHit::Current => format!(
                "Selected stroke: {} at {} px. Click for the color picker. C opens it in drawing mode.",
                hex(self.color.get()),
                self.width.get()
            ),
            PanelHit::Width => "Stroke width. Odd widths from 1 to 31 px.".into(),
            PanelHit::Palette(i) => format!(
                "Color {}. Click to select; click the matching selected color for the picker.",
                hex(PANEL_PALETTE[i])
            ),
            PanelHit::Tool(tool) => {
                let text = PANEL_TOOLS.iter().find(|entry| entry.0 == tool).unwrap().3;
                if tool == MenuTool::Arrow {
                    format!("{text} Current tool: {}.", self.tool_label.borrow())
                } else {
                    text.into()
                }
            }
        };
        *self.tooltip_text.borrow_mut() = wide(&description);
        self.tooltip_hit.set(Some(hit));
        let info = self.tooltip_info();
        unsafe {
            SendMessageW(
                hwnd,
                TTM_UPDATETIPTEXTW,
                None,
                Some(LPARAM(&info as *const _ as isize)),
            );
            SendMessageW(hwnd, TTM_ACTIVATE, Some(WPARAM(1)), None);
        }
    }

    fn set_panel_hover(&self, hit: Option<PanelHit>) -> bool {
        if self.hover.replace(hit) == hit {
            return false;
        }
        let _ = self.resize_panel();
        true
    }

    fn begin_panel_gesture(&self, hit: Option<PanelHit>, point: POINT) {
        self.cancel_panel_gesture();
        self.hide_tooltip();
        self.pressed.set(hit);
        self.hover.set(hit);
        if matches!(hit, Some(PanelHit::Tool(_))) {
            let _ = self.resize_panel();
        }
        let hwnd = self.status.get();
        if hit == Some(PanelHit::Grip) {
            // Hold the current interpolation, including a half-revealed menu.
            let mut motion = self.motion.get();
            motion.from = motion.progress;
            motion.to = motion.progress;
            self.motion.set(motion);
            self.sync_animation_timer();
            self.panel_drag.set(Some(PanelDrag {
                start: point,
                anchor: self.panel_anchor.get(),
                scale: self.panel_scale.get(),
                moving: false,
            }));
        }
        unsafe {
            // Background nonactivating popups cannot rely on capture outside their bounds.
            // This guard exists only during a grip or slider gesture, never while idle.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture
            if hit.is_some() && IsWindowVisible(hwnd).as_bool() {
                SetCapture(hwnd);
                self.panel_capture.set(GetCapture() == hwnd);
                if matches!(hit, Some(PanelHit::Grip | PanelHit::Width)) && self.panel_capture.get()
                {
                    self.panel_guard
                        .set(SetTimer(Some(hwnd), PANEL_DRAG_TIMER, 16, None) != 0);
                    if !self.panel_guard.get() {
                        self.cancel_panel_gesture();
                    }
                } else if !self.panel_capture.get() {
                    self.cancel_panel_gesture();
                }
            }
        }
    }

    fn move_panel_grip(&self, point: POINT) {
        let Some(mut drag) = self.panel_drag.get() else {
            return;
        };
        if !drag.moving
            && ((point.x - drag.start.x) as f64).hypot((point.y - drag.start.y) as f64)
                >= scaled(4, drag.scale) as f64
        {
            drag.moving = true;
        }
        if drag.moving {
            self.panel_anchor.set(POINT {
                x: drag.anchor.x + point.x - drag.start.x,
                y: drag.anchor.y + point.y - drag.start.y,
            });
            self.panel_drag.set(Some(drag));
            let _ = self.resize_panel();
        }
    }

    fn cancel_panel_gesture(&self) {
        let pressed = self.pressed.replace(None);
        let hover = self.hover.replace(None);
        if matches!(pressed.or(hover), Some(PanelHit::Tool(_))) {
            let _ = self.resize_panel();
        }
        let drag = self.panel_drag.replace(None);
        let hwnd = self.status.get();
        unsafe {
            if self.panel_guard.replace(false) {
                let _ = KillTimer(Some(hwnd), PANEL_DRAG_TIMER);
            }
            if self.panel_capture.replace(false) && GetCapture() == hwnd {
                let _ = ReleaseCapture();
            }
            if drag.is_some_and(|drag| drag.moving) && !hwnd.is_invalid() {
                let _ = self.place_panel(self.panel_anchor.get(), self.panel_dpi.get());
            }
        }
    }

    fn finish_panel_gesture(&self, hit: Option<PanelHit>, x: i32) {
        let pressed = self.pressed.get();
        let width = PanelGeometry::new(self).requested_width(x);
        self.cancel_panel_gesture();
        if pressed == Some(PanelHit::Width) && self.picker.get().is_invalid() {
            self.post(UiEvent::Width(width));
        }
        if let Some(target) = pressed.filter(|pressed| Some(*pressed) == hit) {
            match target {
                PanelHit::Grip => {}
                PanelHit::Toggle => self.post(UiEvent::Toggle),
                PanelHit::Current => self.post(UiEvent::OpenPicker),
                PanelHit::Palette(i) => self.post(
                    if PANEL_PALETTE[i] & 0xffffff == self.color.get() & 0xffffff {
                        UiEvent::OpenPicker
                    } else {
                        UiEvent::Color(PANEL_PALETTE[i])
                    },
                ),
                PanelHit::Width => {}
                PanelHit::Tool(tool) => self.post(UiEvent::Tool(tool)),
            }
        }
        // Resample only after ending a held-size gesture, retaining feedback at release.
        self.refresh_panel_hover();
        self.set_panel_hover(if self.tracking.get() { hit } else { None });
        self.reveal_panel(self.tracking.get());
    }

    fn track_panel_leave(&self) {
        let mut tracking = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.status.get(),
            ..Default::default()
        };
        self.tracking
            .set(unsafe { TrackMouseEvent(&mut tracking).is_ok() });
    }

    fn refresh_panel_hover(&self) {
        let hwnd = self.status.get();
        if unsafe { IsWindowVisible(hwnd).as_bool() } {
            let mut point = POINT::default();
            let inside = unsafe {
                GetCursorPos(&mut point).is_ok() && ScreenToClient(hwnd, &mut point).as_bool()
            } && PanelGeometry::new(self).contains(point.x, point.y);
            self.tracking.set(false);
            if inside {
                self.track_panel_leave();
            }
        }
    }

    fn destroy_panel_resources(&self) {
        self.stop_leave_timer();
        self.cancel_panel_gesture();
        let hwnd = self.status.get();
        unsafe {
            if self.animation_timer.replace(false) {
                let _ = KillTimer(Some(hwnd), PANEL_ANIMATION_TIMER);
            }
            let tooltip = self.tooltip.replace(HWND::default());
            if !tooltip.is_invalid() {
                let _ = DestroyWindow(tooltip);
            }
        }
        self.tooltip_hit.set(None);
    }

    fn post(&self, event: UiEvent) {
        let (value, kind) = match event {
            UiEvent::Color(color) => (color as usize, 1),
            UiEvent::Closed => (0, 2),
            UiEvent::OpenPicker => (0, 3),
            UiEvent::Tool(tool) => (tool as usize, 4),
            UiEvent::Width(width) => (width as usize, 5),
            UiEvent::Toggle => (0, 6),
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
        2 if message.wParam.0 == 0 => Some(UiEvent::Closed),
        3 if message.wParam.0 == 0 => Some(UiEvent::OpenPicker),
        4 => match message.wParam.0 {
            0 => Some(UiEvent::Tool(MenuTool::Pen)),
            1 => Some(UiEvent::Tool(MenuTool::TemporaryPen)),
            2 => Some(UiEvent::Tool(MenuTool::Line)),
            3 => Some(UiEvent::Tool(MenuTool::Arrow)),
            4 => Some(UiEvent::Tool(MenuTool::Rectangle)),
            5 => Some(UiEvent::Tool(MenuTool::Ellipse)),
            6 => Some(UiEvent::Tool(MenuTool::Eraser)),
            _ => None,
        },
        5 if (1..=31).contains(&message.wParam.0) && message.wParam.0 % 2 == 1 => {
            Some(UiEvent::Width(message.wParam.0 as u32))
        }
        6 if message.wParam.0 == 0 => Some(UiEvent::Toggle),
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
        body: rect(
            geometry.body.left * n,
            geometry.body.top * n,
            (geometry.body.right - geometry.body.left) * n,
            (geometry.body.bottom - geometry.body.top) * n,
        ),
        tip_radius: geometry.tip_radius * n as f64,
        scale: geometry.scale * n as f64,
        upward: geometry.upward,
        opacity: geometry.opacity,
    };
    let sample = Bitmap::new(large.width, large.height)?;
    // ponytail: 4x box sampling is bounded by the 440x304 logical
    // maximum. Reuse GDI geometry; increase samples only if hardware review warrants it.
    fill(sample.dc, rect(0, 0, large.width, large.height), 0);
    unsafe {
        let brush = GdiObject(CreateSolidBrush(COLORREF(0xffffff)).into());
        let old_brush = SelectObject(sample.dc, brush.0);
        let old_pen = SelectObject(sample.dc, GetStockObject(NULL_PEN));
        let _ = RoundRect(
            sample.dc,
            large.body.left,
            large.body.top,
            large.body.right,
            large.body.bottom,
            large.radius * 2,
            large.radius * 2,
        );
        if large.tip_radius > 0.0 {
            let (x, y) = large.tip_center();
            let r = large.tip_radius.round() as i32;
            let _ = Ellipse(sample.dc, x - r, y - r, x + r, y + r);
        }
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
    let badge = large.badge();
    let mut pixels = Vec::with_capacity((geometry.width * geometry.height) as usize);
    for y in 0..geometry.height {
        for x in 0..geometry.width {
            let mut sums = [0_u32; 4];
            for dy in 0..n {
                for dx in 0..n {
                    let i = ((y * n + dy) * large.width + x * n + dx) as usize;
                    if coverage[i] {
                        sums[3] += 1;
                        let sx = x * n + dx;
                        let sy = y * n + dy;
                        let color = if geometry.opacity == 0.0
                            || (sx >= badge.left
                                && sx < badge.right
                                && sy >= badge.top
                                && sy < badge.bottom)
                        {
                            source[i]
                        } else {
                            blend(PANEL_BACKGROUND, source[i], geometry.opacity)
                        };
                        for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
                            sums[channel] += (color >> shift) & 255;
                        }
                    }
                }
            }
            let count = (n * n) as u32;
            let mut pixel = ((sums[3] * 255 + count / 2) / count) << 24;
            for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
                pixel |= ((sums[channel] + count / 2) / count) << shift;
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

fn panel_line(dc: HDC, points: &[(i32, i32)], s: f64, origin: (i32, i32), color: u32, width: i32) {
    unsafe {
        let pen = GdiObject(CreatePen(PS_SOLID, width.max(1), colorref(color)).into());
        let old = SelectObject(dc, pen.0);
        let points: Vec<POINT> = points
            .iter()
            .map(|(x, y)| POINT {
                x: origin.0 + scaled(*x, s),
                y: origin.1 + scaled(*y, s),
            })
            .collect();
        let _ = Polyline(dc, &points);
        SelectObject(dc, old);
    }
}

fn panel_icon(dc: HDC, tool: MenuTool, origin: (i32, i32), s: f64, color: u32) {
    let line = |points: &[(i32, i32)]| panel_line(dc, points, s, origin, color, scaled(2, s));
    match tool {
        MenuTool::Pen | MenuTool::TemporaryPen => {
            line(&[(4, 25), (7, 18), (19, 6), (24, 11), (12, 23), (4, 25)]);
            line(&[(16, 9), (21, 14)]);
            line(&[(7, 18), (12, 23)]);
            if tool == MenuTool::TemporaryPen {
                line(&[(25, 18), (25, 26)]);
                line(&[(22, 22), (28, 22)]);
            }
        }
        MenuTool::Arrow => {
            line(&[(4, 26), (24, 6)]);
            line(&[(12, 6), (24, 6), (24, 18)]);
        }
        MenuTool::Line => line(&[(4, 26), (24, 6)]),
        MenuTool::Rectangle => line(&[(3, 9), (25, 9), (25, 25), (3, 25), (3, 9)]),
        MenuTool::Ellipse => unsafe {
            let pen = GdiObject(CreatePen(PS_SOLID, scaled(2, s).max(1), colorref(color)).into());
            let old = SelectObject(dc, pen.0);
            let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            let _ = Ellipse(
                dc,
                origin.0 + scaled(3, s),
                origin.1 + scaled(9, s),
                origin.0 + scaled(25, s),
                origin.1 + scaled(25, s),
            );
            SelectObject(dc, brush);
            SelectObject(dc, old);
        },
        MenuTool::Eraser => {
            line(&[(4, 20), (17, 7), (26, 16), (15, 27), (11, 27), (4, 20)]);
            line(&[(10, 14), (20, 23)]);
            line(&[(11, 27), (26, 27)]);
        }
    }
}

fn paint_status(state: &State, dc: HDC, geometry: &PanelGeometry) {
    let s = geometry.scale;
    let inset = scaled(1, s) / 2;
    fill(
        dc,
        rect(0, 0, geometry.width, geometry.height),
        PANEL_BACKGROUND,
    );
    rounded_pen(
        dc,
        rect(
            geometry.body.left + inset,
            geometry.body.top + inset,
            geometry.body.right - geometry.body.left - 2 * inset,
            geometry.body.bottom - geometry.body.top - 2 * inset,
        ),
        geometry.radius,
        PANEL_BACKGROUND,
        PANEL_BORDER,
        scaled(1, s),
    );
    let disabled = !state.picker.get().is_invalid();
    let text = if disabled {
        PANEL_SECONDARY
    } else {
        PANEL_TEXT
    };
    if geometry.opacity > 0.0 {
        if let Ok(font) = panel_font(s) {
            unsafe {
                let saved = SaveDC(dc);
                SelectObject(dc, font.0);
                let toggle = geometry.bounds(PanelHit::Toggle);
                rounded_pen(
                    dc,
                    toggle,
                    scaled(4, s),
                    PANEL_BACKGROUND,
                    if state.drawing.get() {
                        PANEL_ACCENT
                    } else {
                        PANEL_BORDER
                    },
                    scaled(1, s),
                );
                let switch = rect(
                    toggle.left + scaled(8, s),
                    toggle.top + scaled(7, s),
                    scaled(28, s),
                    scaled(16, s),
                );
                rounded_pen(
                    dc,
                    switch,
                    scaled(4, s),
                    PANEL_BACKGROUND,
                    if state.drawing.get() {
                        PANEL_ACCENT
                    } else {
                        PANEL_SECONDARY
                    },
                    scaled(1, s),
                );
                ellipse(
                    dc,
                    switch.left + scaled(if state.drawing.get() { 20 } else { 8 }, s),
                    switch.top + scaled(8, s),
                    scaled(5, s),
                    PANEL_ACCENT,
                    1,
                    true,
                );
                label(
                    dc,
                    if state.drawing.get() {
                        "Drawing"
                    } else {
                        "Click-through"
                    },
                    rect(
                        toggle.left + scaled(42, s),
                        toggle.top,
                        scaled(128, s),
                        scaled(31, s),
                    ),
                    PANEL_TEXT,
                    DT_SINGLELINE | DT_VCENTER,
                );
                let header = rect(
                    toggle.left - scaled(90, s),
                    toggle.top + scaled(6, s),
                    scaled(84, s),
                    scaled(18, s),
                );
                label(
                    dc,
                    PANEL_TOOLS
                        .iter()
                        .find(|entry| entry.0 == state.tool.get())
                        .map(|entry| entry.1)
                        .unwrap_or("Pen"),
                    header,
                    PANEL_SECONDARY,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
                let preview = geometry.bounds(PanelHit::Current);
                label(
                    dc,
                    "Ctrl+Alt+D",
                    rect(
                        toggle.right - scaled(104, s),
                        toggle.top,
                        scaled(96, s),
                        scaled(31, s),
                    ),
                    PANEL_SECONDARY,
                    DT_SINGLELINE | DT_VCENTER,
                );
                let y = (preview.top + preview.bottom) / 2;
                let width = scaled(state.width.get() as i32, s).max(1);
                let inset = (width + 1) / 2;
                let left = preview.left + inset;
                let right = preview.right - inset;
                panel_line(
                    dc,
                    &[(0, 0), (((right - left) as f64 / s).round() as i32, 0)],
                    s,
                    (left, y),
                    state.color.get(),
                    width,
                );
                ellipse(dc, left, y, width / 2, state.color.get(), 1, true);
                ellipse(dc, right, y, width / 2, state.color.get(), 1, true);
                let slider = geometry.bounds(PanelHit::Width);
                let cy = (slider.top + slider.bottom) / 2;
                fill(
                    dc,
                    rect(
                        slider.left,
                        cy - scaled(2, s),
                        slider.right - slider.left,
                        scaled(4, s),
                    ),
                    PANEL_BORDER,
                );
                let thumb = slider.left
                    + ((slider.right - slider.left) as f64
                        * (state.width.get().saturating_sub(1).min(30) as f64 / 30.0))
                        .round() as i32;
                fill(
                    dc,
                    rect(
                        slider.left,
                        cy - scaled(2, s),
                        thumb - slider.left,
                        scaled(4, s),
                    ),
                    PANEL_ACCENT,
                );
                ellipse(dc, thumb, cy, scaled(10, s), 0xff364147, 1, true);
                ellipse(dc, thumb, cy, scaled(6, s), PANEL_ACCENT, 1, true);
                label(
                    dc,
                    &state.width.get().to_string(),
                    rect(
                        slider.right + scaled(22, s),
                        slider.top,
                        scaled(30, s),
                        scaled(32, s),
                    ),
                    text,
                    DT_SINGLELINE | DT_RIGHT | DT_VCENTER,
                );
                for (i, color) in PANEL_PALETTE.iter().copied().enumerate() {
                    let (x, y) = geometry.center(PanelHit::Palette(i));
                    ellipse(dc, x, y, scaled(14, s), color, 1, true);
                    ellipse(
                        dc,
                        x,
                        y,
                        scaled(14, s),
                        PANEL_SECONDARY,
                        scaled(1, s),
                        false,
                    );
                    if color & 0xffffff == state.color.get() & 0xffffff {
                        ellipse(dc, x, y, scaled(18, s), PANEL_ACCENT, scaled(2, s), false);
                    }
                }
                let tools = geometry.bounds(PanelHit::Tool(MenuTool::Pen));
                fill(
                    dc,
                    rect(
                        tools.left,
                        tools.top - scaled(8, s),
                        scaled(408, s),
                        scaled(1, s),
                    ),
                    PANEL_BORDER,
                );
                let small_font = panel_font(s * 0.72);
                for (tool, _, key, _) in PANEL_TOOLS {
                    let hit = PanelHit::Tool(tool);
                    let r = geometry.bounds(hit);
                    let selected = state.tool.get() == tool;
                    let hovered = !disabled && state.hover.get() == Some(hit);
                    let pressed = !disabled && state.pressed.get() == Some(hit);
                    if selected || hovered || pressed {
                        rounded_pen(
                            dc,
                            r,
                            scaled(3, s),
                            if pressed {
                                0xff426c5f
                            } else if hovered && selected {
                                0xff36584d
                            } else if hovered {
                                0xff30363d
                            } else {
                                0xff263b36
                            },
                            if selected && (hovered || pressed) {
                                PANEL_TEXT
                            } else if selected || pressed {
                                PANEL_ACCENT
                            } else {
                                PANEL_SECONDARY
                            },
                            scaled(1, s),
                        );
                    }
                    panel_icon(
                        dc,
                        tool,
                        (r.left + scaled(2, s), r.top + scaled(2, s)),
                        s,
                        if selected && !disabled {
                            PANEL_TEXT
                        } else {
                            PANEL_SECONDARY
                        },
                    );
                    let badge = rect(
                        r.right - scaled(15, s),
                        r.top + scaled(4, s),
                        scaled(13, s),
                        scaled(17, s),
                    );
                    rounded_pen(
                        dc,
                        badge,
                        scaled(2, s),
                        PANEL_BACKGROUND,
                        PANEL_BORDER,
                        scaled(1, s),
                    );
                    if let Ok(ref small) = small_font {
                        SelectObject(dc, small.0);
                    }
                    label(
                        dc,
                        key,
                        badge,
                        PANEL_TEXT,
                        DT_SINGLELINE | DT_CENTER | DT_VCENTER,
                    );
                    SelectObject(dc, font.0);
                }
                let _ = RestoreDC(dc, saved);
            }
        }
    }
    // The icon/grip stay opaque; only the body has a border, never an inner badge.
    let badge = geometry.badge();
    panel_icon(
        dc,
        state.tool.get(),
        (badge.left + scaled(2, s), badge.top + scaled(6, s)),
        s,
        PANEL_TEXT,
    );
    for dy in [16, 21, 26] {
        for dx in [35, 39] {
            ellipse(
                dc,
                badge.left + scaled(dx, s),
                badge.top + scaled(dy, s),
                scaled(1, s).max(1),
                PANEL_SECONDARY,
                1,
                true,
            );
        }
    }
    if geometry.tip_radius > 0.0 {
        let (x, y) = geometry.tip_center();
        let r = geometry.tip_radius.round() as i32;
        unsafe {
            let pen = GdiObject(
                CreatePen(
                    PS_SOLID,
                    (1.25 * s).round().max(1.0) as i32,
                    colorref(0xff737c85),
                )
                .into(),
            );
            let brush = GdiObject(CreateSolidBrush(colorref(state.color.get())).into());
            let old_pen = SelectObject(dc, pen.0);
            let old_brush = SelectObject(dc, brush.0);
            let _ = Ellipse(dc, x - r, y - r, x + r, y + r);
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
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
                        let _ = state.resize_panel();
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
                    let x = l.0 as u16 as i16 as i32;
                    let y = (l.0 >> 16) as u16 as i16 as i32;
                    let geometry = PanelGeometry::new(state);
                    let mut screen = POINT { x, y };
                    let _ = ClientToScreen(hwnd, &mut screen);
                    if geometry.contains(x, y) && !state.tracking.get() {
                        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackmouseevent
                        state.track_panel_leave();
                    }
                    if state.panel_drag.get().is_some() {
                        let mut point = POINT { x, y };
                        let _ = ClientToScreen(hwnd, &mut point);
                        state.move_panel_grip(point);
                    } else if state.pressed.get() == Some(PanelHit::Width) {
                        state.post(UiEvent::Width(geometry.requested_width(x)));
                    } else {
                        if geometry.contains(x, y) {
                            if state.allow_hover_reveal(screen) {
                                state.stop_leave_timer();
                                state.reveal_panel(true);
                            }
                        }
                        let geometry = PanelGeometry::new(state);
                        let origin = geometry.destination(state.panel_anchor.get());
                        let hit = geometry.hit(state, screen.x - origin.x, screen.y - origin.y);
                        state.set_panel_hover(hit);
                        state.panel_tooltip(hit);
                    }
                    return LRESULT(0);
                }
                WM_MOUSELEAVE => {
                    state.panel_leave();
                    return LRESULT(0);
                }
                WM_LBUTTONDOWN => {
                    let point = POINT {
                        x: l.0 as u16 as i16 as i32,
                        y: (l.0 >> 16) as u16 as i16 as i32,
                    };
                    let hit = PanelGeometry::new(state).hit(state, point.x, point.y);
                    let mut screen = point;
                    let _ = ClientToScreen(hwnd, &mut screen);
                    state.begin_panel_gesture(hit, screen);
                    return LRESULT(0);
                }
                WM_LBUTTONUP => {
                    let x = l.0 as u16 as i16 as i32;
                    let y = (l.0 >> 16) as u16 as i16 as i32;
                    let geometry = PanelGeometry::new(state);
                    let hit = geometry.hit(state, x, y);
                    state.tracking.set(geometry.contains(x, y));
                    state.finish_panel_gesture(hit, x);
                    return LRESULT(0);
                }
                WM_TIMER if w.0 == PANEL_ANIMATION_TIMER && state.animation_timer.get() => {
                    state.motion.set(state.motion.get().sample(Instant::now()));
                    state.sync_animation_timer();
                    let _ = state.resize_panel();
                    if !state.motion.get().moving() && state.expanded.get() {
                        state.refresh_panel_hover();
                        if !state.tracking.get() {
                            state.panel_leave();
                        }
                    }
                    return LRESULT(0);
                }
                WM_TIMER if w.0 == PANEL_LEAVE_TIMER && state.leave_timer.get() => {
                    state.stop_leave_timer();
                    let mut screen = POINT::default();
                    if GetCursorPos(&mut screen).is_ok() {
                        state.finish_circle_leave(screen);
                    }
                    return LRESULT(0);
                }
                WM_TIMER if w.0 == PANEL_DRAG_TIMER && state.panel_guard.get() => {
                    let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
                        VK_RBUTTON
                    } else {
                        VK_LBUTTON
                    };
                    let mut point = POINT::default();
                    let captured = GetCapture() == hwnd;
                    let down = GetAsyncKeyState(key.0 as i32) < 0;
                    let positioned = GetCursorPos(&mut point).is_ok();
                    if !captured || !down || !positioned {
                        // Outside release commits only a started slider, never a button.
                        if captured
                            && !down
                            && positioned
                            && state.pressed.get() == Some(PanelHit::Width)
                        {
                            let _ = ScreenToClient(hwnd, &mut point);
                            state.finish_panel_gesture(None, point.x);
                        } else {
                            state.cancel_panel_gesture();
                            state.refresh_panel_hover();
                            state.reveal_panel(state.tracking.get());
                        }
                    } else if state.panel_drag.get().is_some() {
                        state.move_panel_grip(point);
                    } else if state.pressed.get() == Some(PanelHit::Width) {
                        let _ = ScreenToClient(hwnd, &mut point);
                        let geometry = PanelGeometry::new(state);
                        state.post(UiEvent::Width(geometry.requested_width(point.x)));
                    }
                    return LRESULT(0);
                }
                WM_CAPTURECHANGED => {
                    // Never release or reacquire from inside the capture-change callback.
                    // https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-capturechanged
                    let active = state.panel_capture.get()
                        || state.pressed.get().is_some()
                        || state.panel_drag.get().is_some();
                    state.panel_capture.set(false);
                    state.cancel_panel_gesture();
                    if active {
                        state.refresh_panel_hover();
                        state.reveal_panel(state.tracking.get());
                    }
                    return LRESULT(0);
                }
                WM_CANCELMODE => {
                    state.cancel_panel_gesture();
                    state.refresh_panel_hover();
                    state.reveal_panel(state.tracking.get());
                    return LRESULT(0);
                }
                WM_DPICHANGED => {
                    // Keep the dragged size until release, then use the destination DPI.
                    state.panel_dpi.set((w.0 & 0xffff) as u32);
                    if state.panel_drag.get().is_none() {
                        state.cancel_panel_gesture();
                        let _ = state.place_panel(state.panel_anchor.get(), state.panel_dpi.get());
                    }
                    return LRESULT(0);
                }
                WM_DISPLAYCHANGE => {
                    state.cancel_panel_gesture();
                    let _ = state.place_panel(state.panel_anchor.get(), GetDpiForWindow(hwnd));
                    return LRESULT(0);
                }
                WM_SETTINGCHANGE if !panel_animations_enabled() => {
                    state.motion.set(PanelMotion::settled(state.expanded.get()));
                    state.sync_animation_timer();
                    let _ = state.resize_panel();
                    return LRESULT(0);
                }
                WM_DESTROY => {
                    state.destroy_panel_resources();
                    return LRESULT(0);
                }
                WM_NCDESTROY => {
                    state.destroy_panel_resources();
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

    fn menu_open(ui: &DesktopUi) -> Result<()> {
        ui.state.expanded.set(true);
        ui.state.tracking.set(true);
        ui.state.motion.set(PanelMotion::settled(true));
        ui.state.sync_animation_timer();
        ui.state.resize_panel()
    }

    #[test]
    fn panel_menu_followup_layout_circle_and_reduced_radius() -> Result<()> {
        let state = State::new(HWND::default())?;
        for upward in [false, true] {
            state.panel_upward.set(upward);
            state.motion.set(PanelMotion::settled(true));
            let g = PanelGeometry::new(&state);
            assert_eq!((g.width, g.height, g.radius), (440, 304, 6));
            let offset = if upward { -44 } else { 0 };
            assert_eq!(
                g.bounds(PanelHit::Toggle),
                rect(112, if upward { 267 } else { 7 }, 266, 31)
            );
            assert_eq!(g.bounds(PanelHit::Current), rect(24, 48 + offset, 392, 32));
            assert_eq!(g.center(PanelHit::Width), (198, 104 + offset));
            assert_eq!(g.center(PanelHit::Palette(0)), (52, 150 + offset));
            assert_eq!(g.center(PanelHit::Palette(15)), (388, 196 + offset));
            for (i, (tool, _, _, _)) in PANEL_TOOLS.iter().enumerate() {
                assert_eq!(
                    g.bounds(PanelHit::Tool(*tool)),
                    rect(14 + 60 * i as i32, 238 + offset, 44, 36)
                );
            }
            for target in [
                PanelHit::Grip,
                PanelHit::Toggle,
                PanelHit::Current,
                PanelHit::Width,
            ]
            .into_iter()
            .chain((0..16).map(PanelHit::Palette))
            .chain(PANEL_TOOLS.map(|entry| PanelHit::Tool(entry.0)))
            {
                let (x, y) = g.center(target);
                assert_eq!(g.hit(&state, x, y), Some(target));
            }
        }
        // Removing the redundant *255/div255 preserves premultiplied RGB rounding.
        for sum in 0_u32..=16 * 255 {
            assert_eq!((sum + 8) / 16, (sum * 255 + 16 * 255 / 2) / (16 * 255));
        }
        state.panel_upward.set(false);
        state.motion.set(PanelMotion::settled(false));
        for (width, diameter) in [(1, 10.0), (5, 11.6), (15, 15.6), (31, 22.0)] {
            state.width.set(width);
            let g = PanelGeometry::new(&state);
            assert!((2.0 * g.tip_radius - diameter).abs() < 1e-9);
            assert_eq!(g.body.right - g.body.left, 44);
            assert_eq!(g.body.bottom - g.body.top, 44);
            assert_eq!(g.width, 44 + g.tip_radius.ceil() as i32);
            assert_eq!(g.height, 44 + (g.tip_radius - 4.0).ceil() as i32);
            let (x, y) = g.tip_center();
            assert_eq!((x, y), (g.badge().left, g.badge().top + 40));
            assert!(g.contains(x - 2, y));
            assert_eq!(g.hit(&state, x - 2, y), None);
            assert!(!g.contains(0, 0));
            let pixels = panel_bitmap(&state)?.1;
            assert_eq!(pixels[(y * g.width + x - 2) as usize], state.color.get());
            assert_eq!(pixels[0], 0);
            assert!(pixels.iter().any(|pixel| (1..255).contains(&(pixel >> 24))));
            for (i, pixel) in pixels.iter().enumerate() {
                if pixel >> 24 != 0 && (i % g.width as usize) < g.body.left as usize {
                    assert!(
                        g.contains((i % g.width as usize) as i32, (i / g.width as usize) as i32)
                    );
                }
                for shift in [0, 8, 16] {
                    assert!((pixel >> shift) & 255 <= pixel >> 24);
                }
            }
        }
        Ok(())
    }

    struct FollowupThreadDpi(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
    impl Drop for FollowupThreadDpi {
        fn drop(&mut self) {
            unsafe { windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0) };
        }
    }
    fn followup_thread_dpi() -> Result<FollowupThreadDpi> {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
        };
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if previous.0.is_null() {
            return Err(Error::from_thread());
        }
        Ok(FollowupThreadDpi(previous))
    }

    fn followup_preview(state: &State, name: &str) -> Result<(Vec<u32>, Duration)> {
        let geometry = PanelGeometry::new(state);
        let start = Instant::now();
        let pixels = panel_bitmap(state)?.1;
        let elapsed = start.elapsed();
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
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/panel-menu-followup");
        std::fs::create_dir_all(&directory).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        std::fs::write(
            directory.join(format!("{name}.png")),
            png(geometry.width as u32, geometry.height as u32, &composed),
        )
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        Ok((pixels, elapsed))
    }

    #[test]
    fn panel_menu_followup_hover_press_tooltips_and_no_idle_repaint() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "FollowupHoverRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.state.create_tooltip(instance)?;
        menu_open(&ui)?;
        let g = PanelGeometry::new(&ui.state);
        let (normal, _) = followup_preview(&ui.state, "full-normal")?;
        let sample = |hit| {
            let r = g.bounds(hit);
            ((r.bottom - 4) * g.width + r.left + 5) as usize
        };
        let selected = PanelHit::Tool(MenuTool::Pen);
        let unselected = PanelHit::Tool(MenuTool::Line);
        for hit in [selected, unselected] {
            let (x, y) = g.center(hit);
            unsafe {
                SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(x, y)));
            }
            assert_eq!(ui.state.hover.get(), Some(hit));
            assert_ne!(
                ui.state.panel_pixels.borrow()[sample(hit)],
                normal[sample(hit)]
            );
            let r = g.bounds(hit);
            let edge = (((r.top + r.bottom) / 2) * g.width + r.left) as usize;
            assert_ne!(ui.state.panel_pixels.borrow()[edge], normal[edge]);
            let upload = ui.state.panel_upload.get();
            unsafe {
                SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(x, y)));
            }
            assert_eq!(ui.state.panel_upload.get(), upload);
            ui.state.hide_tooltip();
            assert_eq!(ui.state.hover.get(), Some(hit));
            assert!(!ui.state.set_panel_hover(Some(hit)));
            assert_eq!(ui.state.panel_upload.get(), upload);
            let hovered = ui.state.panel_pixels.borrow()[sample(hit)];
            unsafe {
                SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(x, y)));
            }
            assert_ne!(ui.state.panel_pixels.borrow()[sample(hit)], hovered);
            unsafe {
                SendMessageW(hwnd, WM_CANCELMODE, None, None);
            }
            assert_eq!(ui.state.hover.get(), None);
            assert_eq!(ui.state.pressed.get(), None);
            assert_eq!(ui.state.tool.get(), MenuTool::Pen);
            assert!(decoded(root.0).is_empty());
            menu_open(&ui)?;
        }
        ui.state.set_panel_hover(Some(unselected));
        followup_preview(&ui.state, "full-with-hover")?;
        for (tool, _, key, _) in PANEL_TOOLS {
            ui.state.panel_tooltip(Some(PanelHit::Tool(tool)));
            let tooltip = String::from_utf16_lossy(&ui.state.tooltip_text.borrow());
            let name = if tool == MenuTool::TemporaryPen {
                "Temporary pen"
            } else {
                PANEL_TOOLS[tool as usize].1
            };
            assert!(tooltip.contains(name) && tooltip.contains(key));
        }
        // With no inner badge outline, its left and bottom joins are flat background.
        let badge = g.badge();
        assert_eq!(
            normal[(30 * g.width + badge.left) as usize],
            PANEL_BACKGROUND
        );
        assert_eq!(
            normal[((badge.bottom - 1) * g.width + badge.left + 10) as usize],
            PANEL_BACKGROUND
        );
        assert_ne!(
            normal[(20 * g.width + g.width - 1) as usize],
            PANEL_BACKGROUND
        );
        unsafe {
            SendMessageW(hwnd, WM_MOUSELEAVE, None, None);
        }
        assert_eq!(ui.state.hover.get(), None);
        assert!(!ui.state.panel_capture.get() && !ui.state.panel_guard.get());
        drop(ui);
        assert!(!unsafe { IsWindow(Some(hwnd)).as_bool() });
        Ok(())
    }

    #[test]
    fn panel_menu_followup_circle_commits_mask_hover_and_gesture_boundaries() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "FollowupCommitRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        let before = unsafe {
            (
                GetFocus(),
                GetActiveWindow(),
                GetCapture(),
                GetForegroundWindow(),
            )
        };
        let mut timings = String::from(
            "Offscreen panel_bitmap timings, one frame per snapshot; not animation deadlines.\n",
        );
        for (name, color, width) in [
            ("compact-orange-5", ORANGE, 5),
            ("compact-orange-31", ORANGE, 31),
            ("compact-white", 0xfff1f1ef, 15),
            ("compact-black", 0xff161616, 31),
        ] {
            ui.state.motion.set(PanelMotion::settled(false));
            ui.state.expanded.set(false);
            ui.update_label(
                false,
                "Pen",
                Style {
                    color,
                    thickness: width,
                },
            )?;
            let g = PanelGeometry::new(&ui.state);
            let (pixels, elapsed) = followup_preview(&ui.state, name)?;
            timings.push_str(&format!(
                "{name}: {:.3} ms\n",
                elapsed.as_secs_f64() * 1000.0
            ));
            let (x, y) = g.tip_center();
            assert_eq!(pixels[(y * g.width + x - 2) as usize], color);
            assert!(
                pixels
                    .iter()
                    .enumerate()
                    .any(|(i, pixel)| i % (g.width as usize) < g.body.left as usize
                        && pixel >> 24 != 0)
            );
            let bounds = window_bounds(hwnd)?;
            let anchor = ui.state.panel_anchor.get();
            assert_eq!(
                (bounds.left + g.badge().right, bounds.top + g.badge().top),
                (anchor.x, anchor.y)
            );
            assert_eq!(
                unsafe {
                    SendMessageW(
                        hwnd,
                        WM_NCHITTEST,
                        None,
                        Some(point(bounds.left + x - 2, bounds.top + y)),
                    )
                    .0
                },
                HTCLIENT as isize
            );
            assert_eq!(
                unsafe {
                    SendMessageW(
                        hwnd,
                        WM_NCHITTEST,
                        None,
                        Some(point(bounds.left, bounds.top)),
                    )
                    .0
                },
                HTTRANSPARENT as isize
            );
            panel_click(hwnd, (x - 2, y));
            assert!(decoded(root.0).is_empty());
            assert!(ui.state.panel_drag.get().is_none());
        }
        menu_open(&ui)?;
        let (pixels, elapsed) = followup_preview(&ui.state, "full-normal")?;
        timings.push_str(&format!(
            "full-normal: {:.3} ms\n",
            elapsed.as_secs_f64() * 1000.0
        ));
        let g = PanelGeometry::new(&ui.state);
        for (i, color) in PANEL_PALETTE.iter().enumerate() {
            let (x, y) = g.center(PanelHit::Palette(i));
            assert_eq!(pixels[(y * g.width + x) as usize], *color);
        }
        panel_click(hwnd, g.center(PanelHit::Palette(10)));
        assert_eq!(decoded(root.0), [UiEvent::Color(PANEL_PALETTE[10])]);
        assert_eq!(ui.state.color.get(), 0xff161616);
        ui.update_label(
            true,
            "Arrow tail-first",
            Style {
                color: PANEL_PALETTE[10],
                thickness: 15,
            },
        )?;
        let picker_hwnd = picker(&ui, ui.state.color.get())?;
        let disabled_geometry = PanelGeometry::new(&ui.state);
        for hit in [
            PanelHit::Current,
            PanelHit::Width,
            PanelHit::Palette(2),
            PanelHit::Tool(MenuTool::Line),
        ] {
            let (x, y) = disabled_geometry.center(hit);
            assert_eq!(disabled_geometry.hit(&ui.state, x, y), None);
            panel_click(hwnd, (x, y));
        }
        assert!(decoded(root.0).is_empty());
        unsafe {
            SetWindowTextW(ui.state.edit.get(), w!("#123456"))?;
        }
        assert_eq!(ui.state.color.get(), PANEL_PALETTE[10]);
        click(picker_hwnd, APPLY);
        assert_eq!(
            decoded(root.0),
            [UiEvent::Color(0xff123456), UiEvent::Closed]
        );
        ui.update_label(
            true,
            "Arrow tail-first",
            Style {
                color: 0xff123456,
                thickness: 15,
            },
        )?;
        // Width keyboard commits use the same update_label path as palette/picker commits.
        ui.update_label(
            true,
            "Arrow tail-first",
            Style {
                color: 0xff123456,
                thickness: 31,
            },
        )?;
        let now = Instant::now();
        ui.state.expanded.set(false);
        let close = PanelMotion::settled(true).redirect(false, true, now);
        for elapsed in [Duration::ZERO, Duration::from_millis(75)] {
            ui.state.motion.set(close.sample(now + elapsed));
            assert_eq!(PanelGeometry::new(&ui.state).tip_radius, 0.0);
        }
        ui.state.motion.set(close.sample(now + PANEL_DURATION));
        ui.state.resize_panel()?;
        let compact = PanelGeometry::new(&ui.state);
        assert_eq!(compact.tip_radius, 11.0);
        let (x, y) = compact.tip_center();
        assert_eq!(
            ui.state.panel_pixels.borrow()[(y * compact.width + x - 2) as usize],
            0xff123456
        );
        assert_eq!(ui.state.tool.get(), MenuTool::Arrow);
        menu_open(&ui)?;
        let slider = PanelGeometry::new(&ui.state).center(PanelHit::Width);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(slider.0, slider.1)));
            SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(-50, -50)));
            SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(500, 350)));
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(-50, -50)));
        }
        assert_eq!(
            decoded(root.0),
            [UiEvent::Width(1), UiEvent::Width(31), UiEvent::Width(1)]
        );
        assert!(!ui.state.panel_capture.get() && !ui.state.panel_guard.get());
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/panel-menu-followup/timings.txt"),
            timings,
        )
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        assert_eq!(
            unsafe {
                (
                    GetFocus(),
                    GetActiveWindow(),
                    GetCapture(),
                    GetForegroundWindow(),
                )
            },
            before
        );
        Ok(())
    }

    #[test]
    fn panel_menu_followup_first_frame_anchor_bridge_and_elapsed_animation() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "FollowupAnimationRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        let anchor = ui.state.panel_anchor.get();
        for upward in [false, true] {
            ui.state.panel_upward.set(upward);
            ui.state.expanded.set(false);
            ui.state.motion.set(PanelMotion::settled(false));
            ui.state.sync_animation_timer();
            ui.state.resize_panel()?;
            let compact = PanelGeometry::new(&ui.state);
            let compact_pixels = panel_bitmap(&ui.state)?.1;
            let tip_screen = POINT {
                x: anchor.x - 44,
                y: anchor.y + 45,
            };
            let origin = compact.destination(anchor);
            assert!(compact.contains(tip_screen.x - origin.x, tip_screen.y - origin.y));
            ui.state.start_panel_reveal(true, true);
            let start = ui.state.motion.get();
            let upload = ui.state.panel_upload.get().unwrap();
            assert!(upload.0 <= start.started && !upload.1);
            assert!(ui.state.animation_timer.get());
            assert_eq!(start.progress, 0.0);
            assert_eq!(PanelGeometry::new(&ui.state).tip_radius, 0.0);
            assert_eq!(window_bounds(hwnd)?.right - window_bounds(hwnd)?.left, 44);
            assert!(ui.state.opening_hover_bridge(tip_screen));
            assert!(!ui.state.opening_hover_bridge(POINT {
                x: anchor.x + 10,
                y: anchor.y - 10
            }));
            for elapsed in [
                Duration::ZERO,
                Duration::from_millis(35),
                Duration::from_millis(75),
                PANEL_DURATION,
            ] {
                ui.state.motion.set(start.sample(start.started + elapsed));
                ui.state.resize_panel()?;
                let g = PanelGeometry::new(&ui.state);
                let bounds = window_bounds(hwnd)?;
                assert_eq!(
                    (bounds.left + g.badge().right, bounds.top + g.badge().top),
                    (anchor.x, anchor.y)
                );
                assert_eq!(
                    (bounds.right - bounds.left, bounds.bottom - bounds.top),
                    (g.width, g.height)
                );
                if elapsed == Duration::from_millis(35) {
                    assert!(g.width > 44 && g.width < 440 && g.height > 44 && g.height < 304);
                    assert!(g.opacity > 0.0 && g.opacity < 1.0);
                    let pixels = panel_bitmap(&ui.state)?.1;
                    // The neutral icon/grip interior stays opaque and frozen during fade.
                    let badge = g.badge();
                    for y in 8..30 {
                        for x in 8..41 {
                            assert_eq!(
                                pixels[((badge.top + y) * g.width + badge.left + x) as usize],
                                compact_pixels[((compact.badge().top + y) * compact.width
                                    + compact.badge().left
                                    + x) as usize]
                            );
                        }
                    }
                    followup_preview(
                        &ui.state,
                        if upward {
                            "intermediate-upward"
                        } else {
                            "intermediate"
                        },
                    )?;
                }
            }
            assert!(!ui.state.opening_hover_bridge(tip_screen));
            let final_geometry = PanelGeometry::new(&ui.state);
            let final_origin = final_geometry.destination(anchor);
            assert_eq!(
                final_geometry
                    .contains(tip_screen.x - final_origin.x, tip_screen.y - final_origin.y),
                !upward
            );
            ui.state.sync_animation_timer();
            assert!(!ui.state.animation_timer.get());
            if upward {
                followup_preview(&ui.state, "full-upward")?;
            }
            ui.state.expanded.set(false);
            let collapse = ui.state.motion.get().redirect(false, true, Instant::now());
            ui.state
                .motion
                .set(collapse.sample(collapse.started + Duration::from_millis(75)));
            assert_eq!(PanelGeometry::new(&ui.state).tip_radius, 0.0);
            ui.state
                .motion
                .set(collapse.sample(collapse.started + PANEL_DURATION));
            ui.state.resize_panel()?;
            let g = PanelGeometry::new(&ui.state);
            let bounds = window_bounds(hwnd)?;
            assert!(g.tip_radius > 0.0);
            assert_eq!(
                (bounds.left + g.badge().right, bounds.top + g.badge().top),
                (anchor.x, anchor.y)
            );
        }
        // Instant upward reveal grants one finite grace, then latches the same
        // screen point. Moving clears the latch without input injection.
        let tail = POINT {
            x: anchor.x - 44,
            y: anchor.y + 45,
        };
        ui.state.panel_upward.set(true);
        ui.state.expanded.set(false);
        ui.state.start_panel_reveal(true, false);
        assert!(ui.state.defer_circle_leave(tail));
        assert!(ui.state.leave_timer.get());
        ui.state.finish_circle_leave(tail);
        assert!(!ui.state.leave_timer.get());
        assert!(!ui.state.expanded.get());
        let stopped = ui.state.panel_upload.get();
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(PANEL_LEAVE_TIMER)), None);
        }
        assert_eq!(ui.state.panel_upload.get(), stopped);
        assert!(!ui.state.allow_hover_reveal(tail));
        assert!(!ui.state.allow_hover_reveal(tail));
        assert!(ui.state.allow_hover_reveal(POINT {
            x: tail.x - 1,
            ..tail
        }));
        ui.state.panel_upward.set(false);
        ui.state.start_panel_reveal(true, false);
        assert!(ui.state.defer_circle_leave(tail));
        assert!(!ui.state.leave_timer.get());
        ui.state.finish_circle_leave(tail);
        assert!(ui.state.expanded.get());
        // A held grip at the zero-progress opening frame must not restore the tip.
        ui.state.motion.set(PanelMotion {
            from: 0.0,
            to: 0.0,
            progress: 0.0,
            started: Instant::now(),
        });
        ui.state
            .begin_panel_gesture(Some(PanelHit::Grip), POINT::default());
        assert_eq!(PanelGeometry::new(&ui.state).tip_radius, 0.0);
        ui.state.cancel_panel_gesture();
        ui.state.expanded.set(false);
        ui.state.start_panel_reveal(true, false);
        assert_eq!(ui.state.motion.get().progress, 1.0);
        assert!(!ui.state.animation_timer.get());
        let before = ui.state.panel_upload.get();
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(PANEL_ANIMATION_TIMER)), None);
        }
        assert_eq!(ui.state.panel_upload.get(), before);
        Ok(())
    }

    #[test]
    fn panel_menu_followup_native_125_percent_controls_and_preview() -> Result<()> {
        let _dpi = followup_thread_dpi()?;
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "Followup125Root", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.state.panel_scale.set(1.25);
        ui.state.panel_dpi.set(120);
        // Apply the simulated DPI before testing an unchanged default-style update.
        ui.state.resize_panel()?;
        let anchor = ui.state.panel_anchor.get();
        for (color, width) in [
            (ORANGE, 5),
            (ORANGE, 31),
            (0xfff1f1ef, 15),
            (0xff161616, 31),
        ] {
            ui.state.expanded.set(false);
            ui.state.motion.set(PanelMotion::settled(false));
            ui.update_label(
                false,
                "Pen",
                Style {
                    color,
                    thickness: width,
                },
            )?;
            let g = PanelGeometry::new(&ui.state);
            let bounds = window_bounds(hwnd)?;
            assert_eq!((g.body.right - g.body.left, g.body.bottom), (55, 55));
            assert_eq!(
                (bounds.left + g.badge().right, bounds.top + g.badge().top),
                (anchor.x, anchor.y)
            );
            let (pixels, _) = followup_preview(
                &ui.state,
                &format!("compact-125-{width}-{:06x}", color & 0xffffff),
            )?;
            let (x, y) = g.tip_center();
            assert_eq!(pixels[(y * g.width + x) as usize], color);
            for (i, pixel) in pixels.iter().enumerate() {
                if pixel >> 24 != 0 && i % (g.width as usize) < g.body.left as usize {
                    let px = (i % g.width as usize) as i32;
                    let py = (i / g.width as usize) as i32;
                    assert!(
                        g.contains(px, py),
                        "Visible circle pixel lacks hover at {px},{py}"
                    );
                }
            }
        }
        menu_open(&ui)?;
        ui.update_label(true, "Pen", Style::default())?;
        assert_eq!(
            (
                PanelGeometry::new(&ui.state).width,
                PanelGeometry::new(&ui.state).height
            ),
            (550, 380)
        );
        let (pixels, _) = followup_preview(&ui.state, "full-125-drawing")?;
        let g = PanelGeometry::new(&ui.state);
        let s = g.scale;
        assert_eq!(
            pixels[(scaled(64, s) * g.width + scaled(220, s)) as usize],
            ORANGE
        );
        assert_eq!(
            pixels[(scaled(104, s) * g.width + scaled(300, s)) as usize],
            PANEL_BORDER
        );
        let thumb = g.bounds(PanelHit::Width).left
            + ((g.bounds(PanelHit::Width).right - g.bounds(PanelHit::Width).left) as f64 * 4.0
                / 30.0)
                .round() as i32;
        assert_eq!(
            pixels[(scaled(104, s) * g.width + thumb) as usize],
            PANEL_ACCENT
        );
        // The reference has no extra RGB/width row between its header and preview.
        for y in scaled(40, s)..scaled(49, s) {
            for x in scaled(24, s)..scaled(380, s) {
                assert_eq!(pixels[(y * g.width + x) as usize], PANEL_BACKGROUND);
            }
        }
        for (tool, _, _, _) in PANEL_TOOLS {
            let g = PanelGeometry::new(&ui.state);
            let (x, y) = g.center(PanelHit::Tool(tool));
            unsafe { SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(x, y))) };
            assert_eq!(ui.state.hover.get(), Some(PanelHit::Tool(tool)));
            panel_click(hwnd, (x, y));
            assert_eq!(decoded(root.0), [UiEvent::Tool(tool)]);
            assert_eq!(
                ui.state.hover.get(),
                Some(PanelHit::Tool(tool)),
                "Mouse release must keep feedback while still over the tool"
            );
        }
        ui.update_label(
            false,
            "Arrow tail-first",
            Style {
                color: 0xff123456,
                thickness: 31,
            },
        )?;
        let g = PanelGeometry::new(&ui.state);
        assert!(!ui.state.drawing.get());
        panel_click(hwnd, g.center(PanelHit::Toggle));
        assert_eq!(decoded(root.0), [UiEvent::Toggle]);
        assert!(
            !ui.state.drawing.get(),
            "Requests must not optimistically flip mode"
        );
        followup_preview(&ui.state, "full-125-click-through")?;
        for (i, color) in PANEL_PALETTE.iter().enumerate() {
            panel_click(hwnd, g.center(PanelHit::Palette(i)));
            assert_eq!(decoded(root.0), [UiEvent::Color(*color)]);
        }
        ui.state
            .set_panel_hover(Some(PanelHit::Tool(MenuTool::Line)));
        followup_preview(&ui.state, "full-125-hover")?;
        let (x, y) = g.center(PanelHit::Width);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(x, y)));
            SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(-100, -100)));
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(650, 450)));
        }
        assert_eq!(decoded(root.0), [UiEvent::Width(1), UiEvent::Width(31)]);
        assert!(!ui.state.panel_guard.get() && !ui.state.panel_capture.get());
        assert!(ui.state.pressed.get().is_none());
        Ok(())
    }

    #[test]
    fn panel_menu_followup_real_timer_frames_125_percent() -> Result<()> {
        let _dpi = followup_thread_dpi()?;
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "FollowupTimer125Root", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.state.panel_scale.set(1.25);
        ui.state.tracking.set(true);
        let anchor = ui.state.panel_anchor.get();
        let mut samples = String::from(
            "Actual native WM_TIMER frames at 125%, hidden window; animation forced only in this test. Includes one diagnostic render/file write per transition, not a hardware FPS measurement.\n",
        );
        for open in [true, false] {
            ui.state.tracking.set(open);
            ui.state.start_panel_reveal(open, true);
            let began = ui.state.motion.get().started;
            let deadline = began + Duration::from_secs(3);
            let mut frames = 0;
            let mut intermediate = 0;
            while ui.state.animation_timer.get() && Instant::now() < deadline {
                let mut message = MSG::default();
                if unsafe { PeekMessageW(&mut message, Some(hwnd), WM_TIMER, WM_TIMER, PM_REMOVE) }
                    .as_bool()
                {
                    unsafe { DispatchMessageW(&message) };
                    if message.wParam.0 == PANEL_ANIMATION_TIMER {
                        let g = PanelGeometry::new(&ui.state);
                        let bounds = window_bounds(hwnd)?;
                        assert_eq!(
                            (bounds.left + g.badge().right, bounds.top + g.badge().top),
                            (anchor.x, anchor.y)
                        );
                        frames += 1;
                        if g.opacity > 0.0 && g.opacity < 1.0 {
                            intermediate += 1;
                            if intermediate == 1 {
                                followup_preview(
                                    &ui.state,
                                    if open {
                                        "timer-opening-125"
                                    } else {
                                        "timer-closing-125"
                                    },
                                )?;
                            }
                        }
                        samples.push_str(&format!("open={open} frame={frames} elapsed={:.1}ms progress={:.4} size={}x{}\n", began.elapsed().as_secs_f64() * 1000.0, g.opacity, g.width, g.height));
                    }
                } else {
                    // Wait for this thread's timer queue, not a fixed animation frame count.
                    unsafe { MsgWaitForMultipleObjectsEx(None, 20, QS_TIMER, MWMO_INPUTAVAILABLE) };
                }
            }
            assert!(
                !ui.state.animation_timer.get(),
                "Native transition timer failed to settle"
            );
            assert!(
                intermediate > 0,
                "Native transition jumped directly to endpoint"
            );
            assert_eq!(ui.state.motion.get().progress, if open { 1.0 } else { 0.0 });
            assert!(frames > 1);
        }
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/panel-menu-followup");
        std::fs::write(directory.join("native-timer-125.txt"), samples)
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        assert!(!ui.state.leave_timer.get() && !ui.state.panel_guard.get());
        Ok(())
    }

    #[test]
    fn panel_menu_followup_clamp_circle_extents_and_dpi() -> Result<()> {
        let state = State::new(HWND::default())?;
        state.width.set(31);
        for (work, dpi) in [
            (rect(0, 0, 1920, 1040), 96),
            (rect(0, 0, 1920, 1040), 120),
            (rect(-1600, -900, 1600, 860), 144),
            (rect(10, 20, 600, 180), 192),
        ] {
            let scale = panel_scale(dpi, work);
            state.panel_scale.set(scale);
            let (anchor, upward) = panel_clamp(
                POINT {
                    x: work.right + 50,
                    y: work.bottom + 50,
                },
                work,
                scale,
            );
            state.panel_upward.set(upward);
            for open in [false, true] {
                state.motion.set(PanelMotion::settled(open));
                let g = PanelGeometry::new(&state);
                let origin = g.destination(anchor);
                assert!(origin.x >= work.left && origin.y >= work.top);
                assert!(origin.x + g.width <= work.right && origin.y + g.height <= work.bottom);
                assert_eq!(
                    (origin.x + g.badge().right, origin.y + g.badge().top),
                    (anchor.x, anchor.y)
                );
            }
        }
        Ok(())
    }

    #[test]
    fn panel_menu_fixed_palette_variants_keys_and_bounded_targets() -> Result<()> {
        assert_eq!(
            PANEL_PALETTE,
            [
                0xffffdf32, 0xffffb51b, 0xfff97316, 0xffe31635, 0xff673b92, 0xff9b4fc0, 0xffd50d81,
                0xffb9085e, 0xff35c7ef, 0xff0085dc, 0xff075cbf, 0xff83c400, 0xff00b85a, 0xfff1f1ef,
                0xffbdbdbb, 0xff161616,
            ]
        );
        assert_eq!(
            PANEL_TOOLS.map(|entry| entry.2),
            ["P", "P", "L", "A", "R", "O", "E"]
        );
        assert_eq!(tool_icon("Temporary pen"), MenuTool::TemporaryPen);
        assert_eq!(tool_icon("Arrow tail-first"), MenuTool::Arrow);
        assert!(PANEL_TOOLS[0].3.contains("P again"));
        assert!(PANEL_TOOLS[1].3.contains("P again"));
        assert!(PANEL_TOOLS[3].3.contains("selected Arrow"));
        let state = State::new(HWND::default())?;
        for s in [1.0, 1.5, 2.0] {
            state.panel_scale.set(s);
            for upward in [false, true] {
                state.panel_upward.set(upward);
                state.motion.set(PanelMotion::settled(true));
                let g = PanelGeometry::new(&state);
                assert_eq!((g.width, g.height), (scaled(440, s), scaled(304, s)));
                let targets: Vec<_> = [
                    PanelHit::Grip,
                    PanelHit::Toggle,
                    PanelHit::Current,
                    PanelHit::Width,
                ]
                .into_iter()
                .chain((0..16).map(PanelHit::Palette))
                .chain(PANEL_TOOLS.map(|entry| PanelHit::Tool(entry.0)))
                .collect();
                for target in targets {
                    let r = g.bounds(target);
                    assert!(
                        r.left >= 0 && r.top >= 0 && r.right <= g.width && r.bottom <= g.height
                    );
                    let (x, y) = g.center(target);
                    assert_eq!(g.hit(&state, x, y), Some(target));
                }
                assert_eq!(g.hit(&state, -1, 0), None);
                assert_eq!(g.hit(&state, g.width, g.height), None);
                assert_eq!(g.hit(&state, 0, 0), None);
                let slider = g.bounds(PanelHit::Width);
                assert_eq!(g.requested_width(slider.left - 100), 1);
                assert_eq!(g.requested_width(slider.right + 100), 31);
                for x in slider.left..slider.right {
                    let width = g.requested_width(x);
                    assert!((1..=31).contains(&width) && width % 2 == 1);
                }
                state.motion.set(PanelMotion::settled(false));
                let compact = PanelGeometry::new(&state);
                assert_eq!(
                    compact.hit(
                        &state,
                        compact.center(PanelHit::Grip).0,
                        compact.center(PanelHit::Grip).1
                    ),
                    Some(PanelHit::Grip)
                );
                assert!(!compact.revealed(PanelHit::Toggle));
                let mut motion = PanelMotion::settled(true);
                motion.progress = 0.5;
                state.motion.set(motion);
                assert!(!PanelGeometry::new(&state).revealed(PanelHit::Width));
            }
        }
        Ok(())
    }

    #[test]
    fn panel_menu_typed_message_decoding_rejects_invalid_requests() {
        let owner = HWND(std::ptr::dangling_mut());
        let mut message = MSG {
            hwnd: owner,
            message: UI_EVENT,
            lParam: LPARAM(4),
            ..Default::default()
        };
        for (id, entry) in PANEL_TOOLS.iter().enumerate() {
            message.wParam = WPARAM(id);
            assert_eq!(decode_event(&message, owner), Some(UiEvent::Tool(entry.0)));
        }
        for id in [7, 255, usize::MAX] {
            message.wParam = WPARAM(id);
            assert_eq!(decode_event(&message, owner), None);
        }
        message.lParam = LPARAM(5);
        for width in 0..=33 {
            message.wParam = WPARAM(width);
            assert_eq!(
                decode_event(&message, owner),
                if (1..=31).contains(&width) && width % 2 == 1 {
                    Some(UiEvent::Width(width as u32))
                } else {
                    None
                }
            );
        }
        message.wParam = WPARAM(usize::MAX);
        assert_eq!(decode_event(&message, owner), None);
        for (kind, event) in [
            (2, UiEvent::Closed),
            (3, UiEvent::OpenPicker),
            (6, UiEvent::Toggle),
        ] {
            message.lParam = LPARAM(kind);
            message.wParam = WPARAM(0);
            assert_eq!(decode_event(&message, owner), Some(event));
            message.wParam = WPARAM(1);
            assert_eq!(decode_event(&message, owner), None);
        }
        for kind in [0, 7, -1] {
            message.lParam = LPARAM(kind);
            assert_eq!(decode_event(&message, owner), None);
        }
        message.lParam = LPARAM(4);
        message.wParam = WPARAM(0);
        assert_eq!(decode_event(&message, HWND::default()), None);
        message.message = WM_APP;
        assert_eq!(decode_event(&message, owner), None);
    }

    #[test]
    fn panel_menu_animation_endpoints_reversal_reduced_motion_no_idle_timer() -> Result<()> {
        let now = Instant::now();
        let open = PanelMotion::settled(false).redirect(true, true, now);
        assert!(open.moving());
        let halfway = open.sample(now + Duration::from_millis(75));
        assert!((halfway.progress - 0.875).abs() < 1e-8);
        let reverse = halfway.redirect(false, true, now + Duration::from_millis(75));
        assert_eq!(reverse.progress, halfway.progress);
        let close = reverse.sample(now + Duration::from_millis(110));
        let reopen = close.redirect(true, true, now + Duration::from_millis(110));
        assert_eq!(close.progress, reopen.progress);
        let end = reopen.sample(now + Duration::from_millis(260));
        assert_eq!(end.progress, 1.0);
        assert!(!end.moving());
        assert_eq!(open.redirect(false, false, now).progress, 0.0);
        assert_eq!(open.redirect(true, false, now).progress, 1.0);
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuTimerRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        assert!(!ui.state.animation_timer.get());
        ui.state.motion.set(open);
        ui.state.sync_animation_timer();
        assert!(ui.state.animation_timer.get());
        ui.state.motion.set(end);
        ui.state.sync_animation_timer();
        assert!(!ui.state.animation_timer.get());
        let before = ui.state.motion.get().progress;
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(PANEL_ANIMATION_TIMER)), None);
        }
        assert_eq!(ui.state.motion.get().progress, before);
        assert!(!ui.state.panel_guard.get());
        // A ReleaseCapture callback after our flags were cleared must not restart motion.
        unsafe {
            SendMessageW(hwnd, WM_CAPTURECHANGED, None, None);
        }
        assert_eq!(ui.state.motion.get().progress, before);
        assert!(!ui.state.animation_timer.get());
        Ok(())
    }

    #[test]
    fn panel_menu_drag_clamp_anchor_dpi_and_hidden_resources() -> Result<()> {
        for work in [
            rect(0, 0, 1920, 1040),
            rect(-1600, -900, 1600, 860),
            rect(10, 20, 240, 180),
        ] {
            for dpi in [96, 144, 192] {
                let s = panel_scale(dpi, work);
                for point in [
                    POINT {
                        x: work.left - 900,
                        y: work.top - 900,
                    },
                    POINT {
                        x: work.right + 900,
                        y: work.bottom + 900,
                    },
                ] {
                    let (anchor, upward) = panel_clamp(point, work, s);
                    let top = anchor.y
                        - if upward {
                            scaled(PANEL_HEIGHT - PANEL_ROW, s)
                        } else {
                            0
                        };
                    assert!(anchor.x - scaled(PANEL_WIDTH, s) >= work.left);
                    assert!(anchor.x <= work.right && top >= work.top);
                    assert!(top + scaled(PANEL_HEIGHT, s) <= work.bottom);
                }
            }
        }
        unsafe {
            let before = (
                GetFocus(),
                GetActiveWindow(),
                GetCapture(),
                GetForegroundWindow(),
            );
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = WindowClass::new(instance, "MenuResourceRoot", Some(root_proc))?;
            let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
            let ui = hidden_ui(root.0)?;
            let hwnd = status(&ui)?;
            ui.state.create_tooltip(instance)?;
            let tooltip = ui.state.tooltip.get();
            let initial = window_bounds(hwnd)?;
            assert_eq!(
                (initial.right - initial.left, initial.bottom - initial.top),
                (50, 46)
            );
            assert_eq!(
                (initial.right, initial.top),
                (ui.state.panel_anchor.get().x, ui.state.panel_anchor.get().y)
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
            let partial = ui
                .state
                .panel_pixels
                .borrow()
                .iter()
                .position(|pixel| {
                    let alpha = pixel >> 24;
                    alpha > 0 && alpha < 255
                })
                .unwrap();
            assert_eq!(
                SendMessageW(
                    hwnd,
                    WM_NCHITTEST,
                    None,
                    Some(point(
                        initial.left + (partial % 50) as i32,
                        initial.top + (partial / 50) as i32
                    ))
                )
                .0,
                HTCLIENT as isize
            );
            assert!(!IsWindowVisible(hwnd).as_bool());
            assert!(!IsWindowVisible(tooltip).as_bool());
            assert_eq!(
                SendMessageW(hwnd, WM_MOUSEACTIVATE, None, None).0,
                MA_NOACTIVATE as isize
            );
            let anchor = ui.state.panel_anchor.get();
            let start = POINT { x: 100, y: 100 };
            ui.state.begin_panel_gesture(Some(PanelHit::Grip), start);
            ui.state.move_panel_grip(POINT { x: 103, y: 100 });
            assert_eq!(ui.state.panel_anchor.get(), anchor);
            ui.state.move_panel_grip(POINT { x: 110, y: 110 });
            assert_eq!(
                ui.state.panel_anchor.get(),
                POINT {
                    x: anchor.x + 10,
                    y: anchor.y + 10
                }
            );
            assert_eq!(
                (
                    PanelGeometry::new(&ui.state).width,
                    PanelGeometry::new(&ui.state).height
                ),
                (50, 46)
            );
            assert!(!ui.state.panel_guard.get());
            assert!(!ui.state.panel_capture.get());
            SendMessageW(hwnd, WM_CANCELMODE, None, None);
            assert!(ui.state.panel_drag.get().is_none());
            menu_open(&ui)?;
            ui.state
                .panel_tooltip(Some(PanelHit::Tool(MenuTool::TemporaryPen)));
            assert!(String::from_utf16_lossy(&ui.state.tooltip_text.borrow()).contains("P again"));
            for upward in [false, true] {
                ui.state.panel_upward.set(upward);
                for progress in [0.0, 0.5, 1.0] {
                    let mut motion = PanelMotion::settled(true);
                    motion.from = progress;
                    motion.to = progress;
                    motion.progress = progress;
                    ui.state.motion.set(motion);
                    ui.state.resize_panel()?;
                    let bounds = window_bounds(hwnd)?;
                    let badge = PanelGeometry::new(&ui.state).badge();
                    let anchor = ui.state.panel_anchor.get();
                    assert_eq!(
                        (bounds.left + badge.right, bounds.top + badge.top),
                        (anchor.x, anchor.y)
                    );
                }
            }
            ui.state.panel_upward.set(false);
            menu_open(&ui)?;
            let center = PanelGeometry::new(&ui.state).center(PanelHit::Width);
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(center.0, center.1)));
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(-10, -10)));
            assert!(!ui.state.tracking.get());
            assert!(!ui.state.expanded.get());
            assert!(ui.state.pressed.get().is_none());
            assert!(!ui.state.panel_guard.get());
            // Drain the closing transition without dispatching desktop input.
            let end = ui.state.motion.get().started + PANEL_DURATION;
            ui.state.motion.set(ui.state.motion.get().sample(end));
            ui.state.sync_animation_timer();
            assert!(!ui.state.animation_timer.get());
            drop(ui);
            assert!(!IsWindow(Some(hwnd)).as_bool());
            assert!(!IsWindow(Some(tooltip)).as_bool());
            assert_eq!(
                (
                    GetFocus(),
                    GetActiveWindow(),
                    GetCapture(),
                    GetForegroundWindow()
                ),
                before
            );
        }
        Ok(())
    }

    #[test]
    fn panel_menu_native_requests_committed_selection_picker_gating_and_cancellation() -> Result<()>
    {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuRequestsRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        menu_open(&ui)?;
        for entry in PANEL_TOOLS {
            panel_click(
                hwnd,
                PanelGeometry::new(&ui.state).center(PanelHit::Tool(entry.0)),
            );
            assert_eq!(decoded(root.0), [UiEvent::Tool(entry.0)]);
            assert_eq!(ui.state.tool.get(), MenuTool::Pen);
        }
        panel_click(hwnd, PanelGeometry::new(&ui.state).center(PanelHit::Toggle));
        assert_eq!(decoded(root.0), [UiEvent::Toggle]);
        assert!(!ui.state.drawing.get());
        for i in 0..16 {
            panel_click(
                hwnd,
                PanelGeometry::new(&ui.state).center(PanelHit::Palette(i)),
            );
            assert_eq!(
                decoded(root.0),
                [if i == 2 {
                    UiEvent::OpenPicker
                } else {
                    UiEvent::Color(PANEL_PALETTE[i])
                }]
            );
        }
        panel_click(
            hwnd,
            PanelGeometry::new(&ui.state).center(PanelHit::Current),
        );
        assert_eq!(decoded(root.0), [UiEvent::OpenPicker]);
        panel_click(hwnd, PanelGeometry::new(&ui.state).center(PanelHit::Width));
        assert_eq!(decoded(root.0), [UiEvent::Width(17)]);
        assert_eq!(ui.state.width.get(), 5);
        ui.update_label(
            true,
            "Temporary pen",
            Style {
                color: 0xff123456,
                thickness: 31,
            },
        )?;
        assert_eq!(
            (
                ui.state.tool.get(),
                ui.state.color.get(),
                ui.state.width.get(),
                ui.state.drawing.get()
            ),
            (MenuTool::TemporaryPen, 0xff123456, 31, true)
        );
        let g = PanelGeometry::new(&ui.state);
        let a = g.center(PanelHit::Tool(MenuTool::Line));
        let b = g.center(PanelHit::Tool(MenuTool::Arrow));
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(a.0, a.1)));
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(b.0, b.1)));
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(a.0, a.1)));
            SendMessageW(hwnd, WM_CAPTURECHANGED, None, None);
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(a.0, a.1)));
        }
        assert!(decoded(root.0).is_empty());
        let picker_hwnd = picker(&ui, ui.state.color.get())?;
        let candidate = ui.state.candidate.get();
        for target in [
            PanelHit::Current,
            PanelHit::Palette(0),
            PanelHit::Width,
            PanelHit::Tool(MenuTool::Eraser),
        ] {
            panel_click(hwnd, PanelGeometry::new(&ui.state).center(target));
        }
        assert!(decoded(root.0).is_empty());
        panel_click(hwnd, PanelGeometry::new(&ui.state).center(PanelHit::Toggle));
        assert_eq!(decoded(root.0), [UiEvent::Toggle]);
        assert_eq!(ui.state.candidate.get(), candidate);
        click(picker_hwnd, CANCEL);
        assert_eq!(decoded(root.0), [UiEvent::Closed]);
        Ok(())
    }

    #[test]
    fn panel_menu_started_slider_clamps_outside_bounds_without_activating_buttons() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuOutsideSliderRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        menu_open(&ui)?;
        let g = PanelGeometry::new(&ui.state);
        let center = g.center(PanelHit::Width);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(center.0, center.1)));
            SendMessageW(hwnd, WM_MOUSEMOVE, None, Some(point(-50, -50)));
            SendMessageW(
                hwnd,
                WM_MOUSEMOVE,
                None,
                Some(point(g.width + 50, g.height + 50)),
            );
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(-50, -50)));
        }
        assert_eq!(
            decoded(root.0),
            [UiEvent::Width(1), UiEvent::Width(31), UiEvent::Width(1)]
        );
        assert_eq!(ui.state.width.get(), 5);
        assert!(ui.state.pressed.get().is_none());
        assert!(!ui.state.tracking.get());
        assert!(!ui.state.panel_guard.get());
        menu_open(&ui)?;
        let center = PanelGeometry::new(&ui.state).center(PanelHit::Tool(MenuTool::Line));
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(point(center.0, center.1)));
            SendMessageW(hwnd, WM_LBUTTONUP, None, Some(point(-50, -50)));
        }
        assert!(decoded(root.0).is_empty());
        Ok(())
    }

    #[test]
    fn panel_menu_offscreen_preview_premultiplied_badge_and_black_border() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuPixelsRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        status(&ui)?;
        let compact_geometry = PanelGeometry::new(&ui.state);
        let compact = panel_bitmap(&ui.state)?.1;
        menu_open(&ui)?;
        let pixels = panel_preview(&ui, "panel-menu-full-100")?;
        let g = PanelGeometry::new(&ui.state);
        let (x, y) = g.center(PanelHit::Palette(15));
        let center = pixels[(y * g.width + x) as usize];
        assert_eq!(center, PANEL_PALETTE[15]);
        assert_ne!(pixels[(y * g.width + x - 14) as usize], center);
        for (i, color) in PANEL_PALETTE.iter().enumerate() {
            let (x, y) = g.center(PanelHit::Palette(i));
            assert_eq!(pixels[(y * g.width + x) as usize], *color);
        }
        for pixel in &pixels {
            let alpha = pixel >> 24;
            for shift in [0, 8, 16] {
                assert!((pixel >> shift) & 255 <= alpha);
            }
        }
        let full_badge: Vec<_> = (0..44)
            .flat_map(|y| (0..44).map(move |x| (y * 440 + 396 + x) as usize))
            .map(|i| pixels[i])
            .collect();
        // Exclude the rounded join and the brush-tip overlap below the icon.
        for y in 8..32 {
            for x in 8..40 {
                assert_eq!(
                    compact[y * compact_geometry.width as usize
                        + compact_geometry.body.left as usize
                        + x],
                    full_badge[y * 44 + x]
                );
            }
        }
        ui.state.panel_upward.set(true);
        ui.update_label(
            false,
            "Arrow tail-first",
            Style {
                color: 0xff123456,
                thickness: 31,
            },
        )?;
        panel_preview(&ui, "panel-menu-upward-100")?;
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
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/panel-menu");
        std::fs::create_dir_all(&directory).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        let path = directory.join(format!("{name}.png"));
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
        message.wParam = WPARAM(0);
        assert_eq!(decode_event(&message, owner), Some(UiEvent::Closed));
        message.lParam = LPARAM(3);
        message.wParam = WPARAM(1);
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
