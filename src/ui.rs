#[cfg(test)]
use crate::controls::Shortcut as ActivationKey;
use crate::controls::{Shortcut, Shortcuts};
use crate::settings::Favorites;
use crate::strokes::{Style, Tool};
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
                ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TOOLTIPS_CLASSW,
                TTF_SUBCLASS, TTF_TRANSPARENT, TTM_ACTIVATE, TTM_ADDTOOLW, TTM_NEWTOOLRECTW,
                TTM_POP, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
                WM_MOUSELEAVE,
            },
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, Error, PCWSTR, Result, w},
};

const UI_EVENT: u32 = WM_APP + 40;
const PANEL_BACKGROUND: u32 = 0xff191c20;
const PANEL_BORDER: u32 = 0xff444b52;
const PANEL_TEXT: u32 = 0xfff1eee8;
const PANEL_SECONDARY: u32 = 0xffadb4bb;
const PANEL_ACCENT: u32 = 0xff7cc7b4;
const TRAY_EDGE: u32 = 0xff5a626a;
const PANEL_WIDTH: i32 = 440;
const PANEL_HEIGHT: i32 = 304;
const PANEL_ROW: i32 = 44;
// Edge rail: one badge-wide column that grows down (or up) from the badge.
const RAIL_HEIGHT: i32 = 656;
// Radial ring, logical px: square window, opaque disc, then ring radii from the center.
const RING_SIZE: i32 = 232;
const RING_EDGE: f64 = 113.0;
const RING_CENTER: f64 = 34.0;
const RING_TOOLS: (f64, f64) = (38.0, 76.0);
const RING_COLORS: (f64, f64) = (80.0, 106.0);
// Collapse button: center inset from the top-right corner, and radius.
const RING_COLLAPSE: (i32, f64) = (16, 11.0);
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
        "Pen. P or click selects permanent pen.",
    ),
    (
        MenuTool::TemporaryPen,
        "Smoke",
        "S",
        "Smoke pen. S or click selects temporary fading ink.",
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

// How the expanded menu is arranged. Every layout keeps the same compact badge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuLayout {
    #[default]
    Panel,
    Rail,
    Radial,
}

impl MenuLayout {
    pub const ALL: [Self; 3] = [Self::Panel, Self::Rail, Self::Radial];

    pub fn name(self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::Rail => "rail",
            Self::Radial => "radial",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|layout| layout.name().eq_ignore_ascii_case(text.trim()))
    }

    // Logical size of the expanded body anchored at the badge. The ring opens at
    // a separate point, so only its badge is anchored.
    fn anchored_size(self) -> (i32, i32) {
        match self {
            Self::Panel => (PANEL_WIDTH, PANEL_HEIGHT),
            Self::Rail => (PANEL_ROW, RAIL_HEIGHT),
            Self::Radial => (PANEL_ROW, PANEL_ROW),
        }
    }

    // The largest surface the layout shows, which bounds the DPI scale.
    fn largest_size(self) -> (i32, i32) {
        match self {
            Self::Radial => (RING_SIZE, RING_SIZE),
            layout => layout.anchored_size(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum UiEvent {
    Color(u32),
    Reserve(u32),
    Swap,
    Tool(MenuTool),
    Width(u32),
    Toggle,
}

// Each instance owns its classes, so independent instances cannot unregister each other.
pub(crate) struct WindowClass {
    pub(crate) name: Vec<u16>,
    instance: HINSTANCE,
}

impl WindowClass {
    pub(crate) fn new(instance: HINSTANCE, kind: &str, proc: WNDPROC) -> Result<Self> {
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
struct State {
    owner: HWND,
    status: Cell<HWND>,
    text: RefCell<String>,
    color: Cell<u32>,
    reserve: Cell<u32>,
    tool: Cell<MenuTool>,
    tool_label: RefCell<String>,
    drawing: Cell<bool>,
    shortcuts: Cell<Shortcuts>,
    width: Cell<u32>,
    expanded: Cell<bool>,
    pinned: Cell<bool>,
    // The owner's show/hide choice. A summoned ring may show a hidden menu briefly.
    visible: Cell<bool>,
    layout: Cell<MenuLayout>,
    radial_center: Cell<POINT>,
    radial_work: Cell<RECT>,
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
    swatch: Cell<Option<SwatchDrag>>,
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
}
impl State {
    fn new(owner: HWND) -> Result<Self> {
        Ok(Self {
            owner,
            status: Cell::new(HWND::default()),
            text: RefCell::new(String::new()),
            color: Cell::new(Style::default().color),
            reserve: Cell::new(Favorites::default().reserve),
            tool: Cell::new(MenuTool::Pen),
            tool_label: RefCell::new("Pen".into()),
            drawing: Cell::new(false),
            shortcuts: Cell::new(Shortcuts::default()),
            width: Cell::new(Style::default().thickness),
            expanded: Cell::new(false),
            pinned: Cell::new(false),
            visible: Cell::new(false),
            layout: Cell::new(MenuLayout::Panel),
            radial_center: Cell::new(POINT::default()),
            radial_work: Cell::new(RECT::default()),
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
            swatch: Cell::new(None),
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
        })
    }
}

pub struct DesktopUi {
    state: Box<State>,
    status_class: WindowClass,
    // Win32 window destruction and keyboard routing must stay on the creating thread.
    _thread: PhantomData<Rc<()>>,
}

impl DesktopUi {
    pub fn new(
        owner: HWND,
        width: i32,
        height: i32,
        visible: bool,
        shortcuts: impl Into<Shortcuts>,
    ) -> Result<Self> {
        if width <= 0 || height <= 0 {
            return Err(Error::new(E_INVALIDARG, "Invalid desktop dimensions"));
        }
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let ui = Self {
            state: Box::new(State::new(owner)?),
            status_class: WindowClass::new(instance, "Status", Some(status_proc))?,
            _thread: PhantomData,
        };
        ui.state.shortcuts.set(shortcuts.into());
        ui.state.visible.set(visible);
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
            if visible {
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
        if !self.state.holding() {
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

    pub fn set_shortcuts(&self, shortcuts: Shortcuts) -> Result<()> {
        self.state.cancel_panel_gesture();
        self.state.hide_tooltip();
        self.state.shortcuts.set(shortcuts);
        self.state.resize_panel()
    }

    pub fn set_reserve(&self, color: u32) -> Result<()> {
        if self.state.reserve.replace(color) == color {
            return Ok(());
        }
        self.state.resize_panel()
    }

    pub fn event(&self, message: &MSG) -> Option<UiEvent> {
        decode_event(message, self.state.owner)
    }

    // Hiding ends gestures; showing restores expansion only when pinned.
    pub fn set_visible(&self, visible: bool) -> Result<()> {
        let state = &self.state;
        let hwnd = state.status.get();
        state.visible.set(visible);
        if !visible {
            state.stop_leave_timer();
            state.cancel_panel_gesture();
            state.hide_tooltip();
            state.tracking.set(false);
            state.expanded.set(false);
            state.motion.set(PanelMotion::settled(false));
            state.sync_animation_timer();
            state.resize_panel()?;
        } else if state.pinned.get() {
            state.expanded.set(true);
            state.motion.set(PanelMotion::settled(true));
            state.resize_panel()?;
        }
        unsafe {
            if visible {
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )?;
            } else {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        Ok(())
    }

    // Switching layouts collapses to the badge; a pinned panel or rail reopens pinned.
    pub fn set_layout(&self, layout: MenuLayout) -> Result<()> {
        let state = &self.state;
        if state.layout.get() == layout {
            return Ok(());
        }
        state.stop_leave_timer();
        state.cancel_panel_gesture();
        state.hide_tooltip();
        state.tracking.set(false);
        state.layout.set(layout);
        // The ring has no pin and closes after each pick.
        if layout == MenuLayout::Radial {
            state.pinned.set(false);
        }
        let open = state.pinned.get() && state.visible.get();
        state.expanded.set(open);
        state.motion.set(PanelMotion::settled(open));
        state.sync_animation_timer();
        if !state.visible.get() {
            unsafe {
                let _ = ShowWindow(state.status.get(), SW_HIDE);
            }
        }
        // Scale and upward opening depend on the layout's size.
        state.place_panel(state.panel_anchor.get(), state.panel_dpi.get())
    }

    // Opens the radial ring around a point in the owner's client coordinates.
    // Only the radial layout responds; others ignore the request.
    pub fn summon(&self, point: (i32, i32)) {
        let mut screen = POINT {
            x: point.0,
            y: point.1,
        };
        unsafe {
            if !ClientToScreen(self.state.owner, &mut screen).as_bool() {
                return;
            }
        }
        self.state.open_radial(screen);
    }
}

impl Drop for DesktopUi {
    fn drop(&mut self) {
        self.state.destroy_panel_resources();
        unsafe {
            let hwnd = self.state.status.get();
            if !hwnd.is_invalid() {
                // Detach before releasing Box, including a failed DestroyWindow.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelHit {
    Grip,
    Pin,
    Toggle,
    Active,
    Reserve,
    Swap,
    Current,
    Palette(usize),
    Width,
    Tool(MenuTool),
    // Radial ring only: returns to the compact badge.
    Collapse,
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

// A pressed palette swatch. Screen points; it becomes a drag after 4 logical px.
#[derive(Clone, Copy)]
struct SwatchDrag {
    start: POINT,
    point: POINT,
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
    layout: MenuLayout,
    // The open radial ring: a square window at `ring_origin` (screen) instead of the badge.
    ring: bool,
    ring_origin: POINT,
}
impl PanelGeometry {
    fn new(state: &State) -> Self {
        Self::with_motion(state, state.motion.get())
    }
    fn with_motion(state: &State, motion: PanelMotion) -> Self {
        let scale = state.panel_scale.get();
        let progress = motion.progress;
        let layout = state.layout.get();
        if layout == MenuLayout::Radial && progress > 0.0 {
            // The ring never animates; it is either the badge or fully open.
            let side = scaled(RING_SIZE, scale);
            let center = state.radial_center.get();
            let work = state.radial_work.get();
            let clamp = |value: i32, low: i32, high: i32| value.min(high - side).max(low);
            return Self {
                width: side,
                height: side,
                radius: 0,
                body: rect(0, 0, side, side),
                tip_radius: 0.0,
                scale,
                upward: false,
                opacity: 1.0,
                layout,
                ring: true,
                ring_origin: POINT {
                    x: clamp(center.x - side / 2, work.left, work.right),
                    y: clamp(center.y - side / 2, work.top, work.bottom),
                },
            };
        }
        let (full_width, full_height) = layout.anchored_size();
        let width = scaled(PANEL_ROW, scale)
            + scaled(
                ((full_width - PANEL_ROW) as f64 * progress).round() as i32,
                scale,
            );
        let height = scaled(PANEL_ROW, scale)
            + scaled(
                ((full_height - PANEL_ROW) as f64 * progress).round() as i32,
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
            layout,
            ring: false,
            ring_origin: POINT::default(),
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
        if self.ring {
            let (x, y) = self.center(hit);
            return rect(
                x - scaled(10, s),
                y - scaled(10, s),
                scaled(20, s),
                scaled(20, s),
            );
        }
        let (full_width, full_height) = self.layout.anchored_size();
        let ox = self.body.right - scaled(full_width, s);
        let oy = if self.upward {
            self.body.bottom - scaled(full_height, s)
        } else {
            self.body.top
        };
        let place = |local: RECT| {
            rect(
                ox + scaled(local.left, s),
                oy + scaled(local.top, s),
                scaled(local.right - local.left, s),
                scaled(local.bottom - local.top, s),
            )
        };
        let body = if self.upward { -44 } else { 0 };
        if self.layout == MenuLayout::Rail && hit != PanelHit::Grip {
            return place(rail_bounds(hit, body));
        }
        let local = match hit {
            // The whole 44 x 44 badge (tool icon + grip) is the drag handle.
            PanelHit::Grip => return self.badge(),
            PanelHit::Pin => rect(358, if self.upward { 267 } else { 7 }, 31, 31),
            PanelHit::Toggle => rect(112, if self.upward { 267 } else { 7 }, 234, 31),
            // Favorites: the active square overlaps the reserve, like a paint program.
            PanelHit::Active => rect(26, 48 + body, 22, 22),
            PanelHit::Reserve => rect(40, 58 + body, 22, 22),
            PanelHit::Swap => rect(52, 46 + body, 12, 12),
            PanelHit::Current => rect(92, 48 + body, 324, 32),
            PanelHit::Collapse => rect(0, 0, 0, 0),
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
        place(local)
    }
    fn center(&self, hit: PanelHit) -> (i32, i32) {
        if self.ring {
            let (cx, cy) = self.ring_center();
            let (radius, angle) = match hit {
                PanelHit::Tool(tool) => (
                    (RING_TOOLS.0 + RING_TOOLS.1) / 2.0,
                    ring_angle(tool_index(tool), PANEL_TOOLS.len()),
                ),
                PanelHit::Palette(i) => (
                    (RING_COLORS.0 + RING_COLORS.1) / 2.0,
                    ring_angle(i, PANEL_PALETTE.len()),
                ),
                PanelHit::Collapse => return self.collapse_center(),
                PanelHit::Grip => return self.ring_grip_center(),
                _ => (0.0, 0.0),
            };
            let radius = radius * self.scale;
            return (
                cx + (radius * angle.cos()).round() as i32,
                cy + (radius * angle.sin()).round() as i32,
            );
        }
        let r = self.bounds(hit);
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return false;
        }
        if self.tip_radius > 0.0 {
            let (cx, cy) = self.tip_center();
            // Native hit testing still uses uploaded alpha, so empty corners pass through.
            if circle_contains_pixel(cx, cy, self.tip_radius, x, y) {
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
    // Compact reserve square: inside the badge, below the grip dots.
    fn reserve_square(&self) -> RECT {
        let badge = self.badge();
        rect(
            badge.left + scaled(32, self.scale),
            badge.top + scaled(32, self.scale),
            scaled(8, self.scale),
            scaled(8, self.scale),
        )
    }
    fn ring_center(&self) -> (i32, i32) {
        (
            (self.body.left + self.body.right) / 2,
            (self.body.top + self.body.bottom) / 2,
        )
    }
    // Collapse button in the ring window's top-left corner, outside the disc.
    fn collapse_center(&self) -> (i32, i32) {
        let inset = scaled(RING_COLLAPSE.0, self.scale);
        (inset, inset)
    }
    // Drag handle mirrored in the top-right corner.
    fn ring_grip_center(&self) -> (i32, i32) {
        (
            self.width - scaled(RING_COLLAPSE.0, self.scale),
            scaled(RING_COLLAPSE.0, self.scale),
        )
    }
    // Corner buttons first; the center swaps colors, the inner ring holds tools,
    // the outer ring colors.
    fn ring_hit(&self, x: i32, y: i32) -> Option<PanelHit> {
        let reach = (RING_COLLAPSE.1 + 1.0) * self.scale;
        for (hit, (bx, by)) in [
            (PanelHit::Collapse, self.collapse_center()),
            (PanelHit::Grip, self.ring_grip_center()),
        ] {
            if f64::from(x - bx).hypot(f64::from(y - by)) <= reach {
                return Some(hit);
            }
        }
        let (cx, cy) = self.ring_center();
        let dx = f64::from(x - cx) / self.scale;
        let dy = f64::from(y - cy) / self.scale;
        let distance = dx.hypot(dy);
        let sector = |count: usize| {
            let step = std::f64::consts::TAU / count as f64;
            ((dy.atan2(dx) + std::f64::consts::FRAC_PI_2 + step / 2.0)
                .rem_euclid(std::f64::consts::TAU)
                / step) as usize
                % count
        };
        if distance <= RING_CENTER {
            Some(PanelHit::Swap)
        } else if distance <= (RING_TOOLS.1 + RING_COLORS.0) / 2.0 {
            Some(PanelHit::Tool(PANEL_TOOLS[sector(PANEL_TOOLS.len())].0))
        } else if distance <= RING_COLORS.1 + 4.0 {
            Some(PanelHit::Palette(sector(PANEL_PALETTE.len())))
        } else {
            None
        }
    }
    fn destination(&self, anchor: POINT) -> POINT {
        if self.ring {
            return self.ring_origin;
        }
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
    fn hit(&self, x: i32, y: i32) -> Option<PanelHit> {
        if !self.contains(x, y) {
            return None;
        }
        if self.ring {
            return self.ring_hit(x, y);
        }
        // Active precedes Reserve: the front square owns their overlap.
        let targets = [
            PanelHit::Grip,
            PanelHit::Pin,
            PanelHit::Toggle,
            PanelHit::Active,
            PanelHit::Reserve,
            PanelHit::Swap,
            PanelHit::Current,
            PanelHit::Width,
        ]
        .into_iter()
        .chain((0..16).map(PanelHit::Palette))
        .chain(PANEL_TOOLS.iter().map(|entry| PanelHit::Tool(entry.0)));
        for hit in targets {
            if !self.revealed(hit) {
                continue;
            }
            let r = self.bounds(hit);
            let inside = if matches!(hit, PanelHit::Palette(_)) {
                let (cx, cy) = self.center(hit);
                let radius = if self.layout == MenuLayout::Rail {
                    8
                } else {
                    15
                };
                (x - cx).pow(2) + (y - cy).pow(2) <= scaled(radius, self.scale).pow(2)
            } else {
                x >= r.left && x < r.right && y >= r.top && y < r.bottom
            };
            if inside {
                return Some(hit);
            }
        }
        None
    }
    // The panel slider grows to the right; the rail slider grows upward.
    fn requested_width(&self, x: i32, y: i32) -> u32 {
        let r = self.bounds(PanelHit::Width);
        let fraction = if self.layout == MenuLayout::Rail {
            (r.bottom - y) as f64 / (r.bottom - r.top).max(1) as f64
        } else {
            (x - r.left) as f64 / (r.right - r.left).max(1) as f64
        };
        let step = (fraction.clamp(0.0, 1.0) * 15.0).round() as u32;
        1 + 2 * step
    }
}

fn tool_index(tool: MenuTool) -> usize {
    PANEL_TOOLS
        .iter()
        .position(|entry| entry.0 == tool)
        .unwrap()
}

// Middle angle of sector `index`, clockwise from the top in screen coordinates.
fn ring_angle(index: usize, count: usize) -> f64 {
    -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * index as f64 / count as f64
}

// Rail positions in logical px. `body` moves content above the badge when opening upward.
fn rail_bounds(hit: PanelHit, body: i32) -> RECT {
    match hit {
        PanelHit::Toggle => rect(7, 48 + body, 30, 28),
        PanelHit::Pin => rect(7, 78 + body, 30, 28),
        PanelHit::Tool(tool) => rect(5, 116 + body + tool_index(tool) as i32 * 32, 34, 30),
        PanelHit::Active => rect(9, 349 + body, 18, 18),
        PanelHit::Reserve => rect(19, 359 + body, 18, 18),
        // Two columns: warm colors left, cool and neutral colors right.
        PanelHit::Palette(i) => rect(
            6 + (i / 8) as i32 * 16,
            385 + body + (i % 8) as i32 * 18,
            16,
            16,
        ),
        PanelHit::Width => rect(12, 558 + body, 20, 90),
        // Not on the rail: clicking the reserve swaps, and the badge tip shows the stroke.
        PanelHit::Grip | PanelHit::Swap | PanelHit::Current | PanelHit::Collapse => {
            rect(0, 0, 0, 0)
        }
    }
}

// Match the pixel's area, including partially covered antialiased circle edges.
fn circle_contains_pixel(cx: i32, cy: i32, radius: f64, x: i32, y: i32) -> bool {
    let dx = f64::from(cx) - f64::from(cx.clamp(x, x.saturating_add(1)));
    let dy = f64::from(cy) - f64::from(cy.clamp(y, y.saturating_add(1)));
    dx * dx + dy * dy <= radius * radius
}

pub(crate) fn tool_icon(label: &str) -> MenuTool {
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

fn tool_shortcut(tool: MenuTool) -> &'static str {
    match tool {
        MenuTool::Pen => "pen",
        MenuTool::TemporaryPen => "smoke_pen",
        MenuTool::Line => "line",
        MenuTool::Arrow => "arrow",
        MenuTool::Rectangle => "rectangle",
        MenuTool::Ellipse => "ellipse",
        MenuTool::Eraser => "eraser",
    }
}

fn shortcut_badge(shortcut: Shortcut) -> String {
    let label = shortcut.label();
    if label.len() <= 2 {
        label
    } else {
        let key = shortcut.key_label();
        if key.len() <= 3 {
            key
        } else {
            key.chars().take(2).collect()
        }
    }
}

fn panel_scale(dpi: u32, work: RECT, layout: MenuLayout) -> f64 {
    let (width, height) = layout.largest_size();
    (dpi.max(96) as f64 / 96.0)
        .min((work.right - work.left).max(1) as f64 / width as f64)
        // Leave one physical pixel for independently rounded body/tip extents.
        .min((work.bottom - work.top - 1).max(1) as f64 / (height + 7) as f64)
}

// Anchor is the badge's screen-space right edge and top. Upward opening keeps it fixed.
fn panel_clamp(anchor: POINT, work: RECT, scale: f64, layout: MenuLayout) -> (POINT, bool) {
    let (width, height) = layout.anchored_size();
    let width = scaled(width, scale);
    let height = scaled(height, scale);
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
            let scale = panel_scale(dpi, info.rcWork, self.layout.get());
            self.panel_scale.set(scale);
            let (anchor, upward) = panel_clamp(
                POINT {
                    x: info.rcWork.right - scaled(16, scale),
                    y: info.rcWork.top + scaled(16, scale),
                },
                info.rcWork,
                scale,
                self.layout.get(),
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
        let scale = panel_scale(dpi, info.rcWork, self.layout.get());
        let (anchor, upward) = panel_clamp(anchor, info.rcWork, scale, self.layout.get());
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

    // Grip moves, started sliders and pressed swatches keep the menu past the pointer.
    fn holding(&self) -> bool {
        self.panel_drag.get().is_some()
            || self.swatch.get().is_some()
            || self.pressed.get() == Some(PanelHit::Width)
    }

    fn start_panel_reveal(&self, open: bool, animated: bool) {
        if self.holding() || (!open && self.pinned.get()) {
            return;
        }
        let radial = self.layout.get() == MenuLayout::Radial;
        if radial {
            // Hover never opens the ring; only open_radial does. Closing is instant
            // because the ring and badge sit at different positions, not one growing shape.
            if open {
                return;
            }
            if self.expanded.replace(false) {
                self.hide_tooltip();
                self.motion.set(PanelMotion::settled(false));
                self.sync_animation_timer();
                let _ = self.resize_panel();
                if !self.visible.get() {
                    unsafe {
                        let _ = ShowWindow(self.status.get(), SW_HIDE);
                    }
                }
            }
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

    // Shows the ring centered on a screen point, clamped to that point's work area.
    // A hidden menu shows only while the ring is open.
    fn open_radial(&self, center: POINT) {
        let hwnd = self.status.get();
        if self.layout.get() != MenuLayout::Radial || self.holding() || hwnd.is_invalid() {
            return;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let found = unsafe {
            GetMonitorInfoW(
                MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST),
                &mut info,
            )
            .as_bool()
        };
        let work = if found {
            info.rcWork
        } else {
            self.panel_monitor.get()
        };
        // ponytail: the ring keeps the badge monitor's scale. Mixed-DPI setups may
        // show it slightly large or small on another monitor until it is dragged there.
        self.radial_work.set(work);
        self.radial_center.set(center);
        self.stop_leave_timer();
        self.hide_tooltip();
        self.expanded.set(true);
        self.motion.set(PanelMotion::settled(true));
        self.sync_animation_timer();
        let _ = self.resize_panel();
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        self.refresh_panel_hover();
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
        circle_contains_pixel(
            anchor.x - scaled(44, scale),
            anchor.y + scaled(40, scale),
            radius,
            screen.x,
            screen.y,
        )
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
        // Pinned menus and the ring stay open; the ring closes from its own button.
        if self.pinned.get() || self.layout.get() == MenuLayout::Radial {
            self.stop_leave_timer();
            return;
        }
        let mut screen = POINT::default();
        if unsafe { GetCursorPos(&mut screen).is_ok() }
            && (self.opening_hover_bridge(screen) || self.defer_circle_leave(screen))
        {
            return;
        }
        if !self.holding() {
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
                wide("Mado selected tool. Drag the tool icon to move the menu.");
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
            PanelHit::Grip if PanelGeometry::new(self).ring => "Drag to move the ring.".to_string(),
            PanelHit::Grip => "Drag the tool icon to move the menu.".to_string(),
            PanelHit::Pin => if self.pinned.get() {
                "Unpin menu. The menu is pinned open."
            } else {
                "Pin menu. Keep the menu expanded when the pointer leaves."
            }
            .into(),
            PanelHit::Toggle => format!(
                "Drawing / Click-through. {} or {} toggles canvas input.",
                self.shortcuts.get().binding("activation_key").label(),
                self.shortcuts.get().binding("toggle").label()
            ),
            PanelHit::Active => format!(
                "Active color {}. Drop a palette color here to draw with it.",
                hex(self.color.get())
            ),
            PanelHit::Reserve => format!(
                "Reserve color {}. Click or press {} to swap. Drop a palette color here to keep it.",
                hex(self.reserve.get()),
                self.shortcuts.get().binding("swap_colors").label()
            ),
            PanelHit::Swap => format!(
                "Swap active and reserve colors. {} while drawing.",
                self.shortcuts.get().binding("swap_colors").label()
            ),
            PanelHit::Current => format!(
                "Selected stroke: {} at {} px.",
                hex(self.color.get()),
                self.width.get()
            ),
            PanelHit::Width => "Stroke width. Odd widths from 1 to 31 px.".into(),
            PanelHit::Collapse => {
                "Collapse to the compact badge. Click the badge to reopen the ring.".into()
            }
            PanelHit::Palette(i) => format!(
                "Color {}. Click to select, or drag onto a favorite square.",
                hex(PANEL_PALETTE[i])
            ),
            PanelHit::Tool(tool) => {
                let entry = PANEL_TOOLS.iter().find(|entry| entry.0 == tool).unwrap();
                let shortcut = self.shortcuts.get().binding(tool_shortcut(tool)).label();
                let text = entry
                    .3
                    .split_whitespace()
                    .map(|word| {
                        if word == entry.2 {
                            shortcut.as_str()
                        } else {
                            word
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                if tool == MenuTool::Arrow {
                    format!("{text} Current tool: {}.", self.tool_label.borrow())
                } else {
                    text
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
        if matches!(hit, Some(PanelHit::Tool(_) | PanelHit::Pin)) {
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
            // The open ring moves its own center; the badge moves the anchor.
            let ring = PanelGeometry::new(self).ring;
            self.panel_drag.set(Some(PanelDrag {
                start: point,
                anchor: if ring {
                    self.radial_center.get()
                } else {
                    self.panel_anchor.get()
                },
                scale: self.panel_scale.get(),
                moving: false,
            }));
        }
        if matches!(hit, Some(PanelHit::Palette(_))) {
            self.swatch.set(Some(SwatchDrag {
                start: point,
                point,
                moving: false,
            }));
        }
        unsafe {
            // Background nonactivating popups cannot rely on capture outside their bounds.
            // This guard exists only during a grip, slider or swatch gesture, never while idle.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture
            if hit.is_some() && IsWindowVisible(hwnd).as_bool() {
                SetCapture(hwnd);
                self.panel_capture.set(GetCapture() == hwnd);
                if matches!(
                    hit,
                    Some(PanelHit::Grip | PanelHit::Width | PanelHit::Palette(_))
                ) && self.panel_capture.get()
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
            let moved = POINT {
                x: drag.anchor.x + point.x - drag.start.x,
                y: drag.anchor.y + point.y - drag.start.y,
            };
            if PanelGeometry::new(self).ring {
                // Follow the pointer's monitor so the ring can cross screens.
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if unsafe {
                    GetMonitorInfoW(MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST), &mut info)
                        .as_bool()
                } {
                    self.radial_work.set(info.rcWork);
                }
                self.radial_center.set(moved);
            } else {
                self.panel_anchor.set(moved);
            }
            self.panel_drag.set(Some(drag));
            let _ = self.resize_panel();
        }
    }

    fn move_swatch(&self, point: POINT) {
        let Some(mut drag) = self.swatch.get() else {
            return;
        };
        if !drag.moving
            && ((point.x - drag.start.x) as f64).hypot((point.y - drag.start.y) as f64)
                >= scaled(4, self.panel_scale.get()) as f64
        {
            drag.moving = true;
            self.hide_tooltip();
        }
        drag.point = point;
        self.swatch.set(Some(drag));
        if drag.moving {
            let _ = self.resize_panel();
        }
    }

    // The favorite square under a started swatch drag, for drop feedback.
    fn swatch_target(&self) -> Option<PanelHit> {
        let drag = self.swatch.get().filter(|drag| drag.moving)?;
        let geometry = PanelGeometry::new(self);
        let origin = geometry.destination(self.panel_anchor.get());
        geometry
            .hit(drag.point.x - origin.x, drag.point.y - origin.y)
            .filter(|hit| matches!(hit, PanelHit::Active | PanelHit::Reserve))
    }

    fn cancel_panel_gesture(&self) {
        let pressed = self.pressed.replace(None);
        let hover = self.hover.replace(None);
        let swatch = self.swatch.replace(None);
        if matches!(pressed.or(hover), Some(PanelHit::Tool(_) | PanelHit::Pin))
            || swatch.is_some_and(|drag| drag.moving)
        {
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
                let geometry = PanelGeometry::new(self);
                if geometry.ring {
                    // Keep the clamped position so the next drag has no dead zone.
                    let origin = geometry.ring_origin;
                    self.radial_center.set(POINT {
                        x: origin.x + geometry.width / 2,
                        y: origin.y + geometry.height / 2,
                    });
                } else {
                    let _ = self.place_panel(self.panel_anchor.get(), self.panel_dpi.get());
                }
            }
        }
    }

    fn finish_panel_gesture(&self, hit: Option<PanelHit>, point: POINT) {
        let pressed = self.pressed.get();
        let dragged = self.swatch.get().is_some_and(|drag| drag.moving);
        let moved = self.panel_drag.get().is_some_and(|drag| drag.moving);
        let width = PanelGeometry::new(self).requested_width(point.x, point.y);
        let ring = PanelGeometry::new(self).ring;
        self.cancel_panel_gesture();
        if pressed == Some(PanelHit::Width) {
            self.post(UiEvent::Width(width));
        }
        if let (true, Some(PanelHit::Palette(i))) = (dragged, pressed) {
            // A started drag is never a click; dropping anywhere else cancels it.
            match hit {
                Some(PanelHit::Active) => self.post(UiEvent::Color(PANEL_PALETTE[i])),
                Some(PanelHit::Reserve) => self.post(UiEvent::Reserve(PANEL_PALETTE[i])),
                _ => {}
            }
        } else if let Some(target) = pressed.filter(|pressed| Some(*pressed) == hit) {
            match target {
                // Clicking the radial badge opens the ring around the badge.
                PanelHit::Grip if !moved && !ring && self.layout.get() == MenuLayout::Radial => {
                    let geometry = PanelGeometry::new(self);
                    let origin = geometry.destination(self.panel_anchor.get());
                    let badge = geometry.badge();
                    self.open_radial(POINT {
                        x: origin.x + (badge.left + badge.right) / 2,
                        y: origin.y + (badge.top + badge.bottom) / 2,
                    });
                    return;
                }
                PanelHit::Grip => {}
                PanelHit::Pin => {
                    self.pinned.set(!self.pinned.get());
                    self.stop_leave_timer();
                    self.hide_tooltip();
                    let _ = self.resize_panel();
                }
                PanelHit::Toggle => self.post(UiEvent::Toggle),
                PanelHit::Active => {}
                PanelHit::Reserve | PanelHit::Swap => self.post(UiEvent::Swap),
                PanelHit::Current => {}
                PanelHit::Palette(i) => self.post(UiEvent::Color(PANEL_PALETTE[i])),
                PanelHit::Width => {}
                PanelHit::Tool(tool) => self.post(UiEvent::Tool(tool)),
                PanelHit::Collapse => {
                    self.reveal_panel(false);
                    return;
                }
            }
        }
        // Resample only after ending a held-size gesture, retaining feedback at release.
        self.refresh_panel_hover();
        self.set_panel_hover(if self.tracking.get() { hit } else { None });
        // The open ring stays until its collapse button is clicked.
        if !ring {
            self.reveal_panel(self.tracking.get());
        }
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
            UiEvent::Reserve(color) => (color as usize, 2),
            UiEvent::Swap => (0, 3),
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
}

fn decode_event(message: &MSG, owner: HWND) -> Option<UiEvent> {
    if message.hwnd != owner || message.message != UI_EVENT {
        return None;
    }
    match message.lParam.0 {
        1 if message.wParam.0 <= u32::MAX as usize => Some(UiEvent::Color(message.wParam.0 as u32)),
        2 if message.wParam.0 <= u32::MAX as usize => {
            Some(UiEvent::Reserve(message.wParam.0 as u32))
        }
        3 if message.wParam.0 == 0 => Some(UiEvent::Swap),
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
fn hex(color: u32) -> String {
    format!("#{:06X}", color & 0xffffff)
}
fn colorref(argb: u32) -> COLORREF {
    COLORREF(((argb >> 16) & 255) | (argb & 0xff00) | ((argb & 255) << 16))
}
fn scaled(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
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
        layout: geometry.layout,
        ring: geometry.ring,
        ring_origin: geometry.ring_origin,
    };
    let sample = Bitmap::new(large.width, large.height)?;
    // ponytail: 4x box sampling is bounded by the 440x304 logical panel and
    // 44x656 rail maxima. Reuse GDI geometry; increase samples only if hardware review warrants it.
    fill(sample.dc, rect(0, 0, large.width, large.height), 0);
    unsafe {
        let brush = GdiObject(CreateSolidBrush(COLORREF(0xffffff)).into());
        let old_brush = SelectObject(sample.dc, brush.0);
        let old_pen = SelectObject(sample.dc, GetStockObject(NULL_PEN));
        if large.ring {
            let (x, y) = large.ring_center();
            let r = (RING_EDGE * large.scale).round() as i32;
            let _ = Ellipse(sample.dc, x - r, y - r, x + r, y + r);
            let r = (RING_COLLAPSE.1 * large.scale).round() as i32;
            for (x, y) in [large.collapse_center(), large.ring_grip_center()] {
                let _ = Ellipse(sample.dc, x - r, y - r, x + r, y + r);
            }
        } else {
            let _ = RoundRect(
                sample.dc,
                large.body.left,
                large.body.top,
                large.body.right,
                large.body.bottom,
                large.radius * 2,
                large.radius * 2,
            );
        }
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

fn panel_icon(dc: HDC, tool: MenuTool, tail_first: bool, origin: (i32, i32), s: f64, color: u32) {
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
            if tail_first {
                line(&[(12, 6), (24, 6), (24, 18)]);
            } else {
                line(&[(4, 14), (4, 26), (16, 26)]);
            }
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

// Tray mark "mini badge" on a 16-unit grid: graphite tile, the selected panel tool and a
// brush-tip dot. Drawing fills the dot with the ink; click-through shows a gray ring.
// Returns straight-alpha BGRA rows, top-down, as 32-bit icon color bitmaps expect.
pub(crate) fn tray_icon_pixels(
    size: i32,
    drawing: bool,
    tool: MenuTool,
    tail_first: bool,
    color: u32,
) -> Result<Vec<u32>> {
    let n = PANEL_SAMPLES;
    let u = (size * n) as f64 / 16.0;
    let at = |v: f64| (v * u).round() as i32;
    let tile = RECT {
        left: at(2.0),
        top: 0,
        right: at(16.0),
        bottom: at(14.0),
    };
    let dot = (at(3.5), at(12.5));
    let sample = Bitmap::new(size * n, size * n)?;
    fill(sample.dc, rect(0, 0, size * n, size * n), 0);
    rounded_pen(sample.dc, tile, at(3.0), 0xffffffff, 0xffffffff, 1);
    ellipse(sample.dc, dot.0, dot.1, at(3.5), 0xffffffff, 1, true);
    let coverage: Vec<bool> = sample.pixels().iter().map(|p| p & 0xffffff != 0).collect();
    // Inset by half the edge pen so the whole 1-unit edge stays inside the coverage.
    // GDI strokes right/bottom one sample inside the exclusive bound, hence the +1.
    let half = at(0.5);
    let inner = RECT {
        left: tile.left + half,
        top: tile.top + half,
        right: tile.right - half + 1,
        bottom: tile.bottom - half + 1,
    };
    rounded_pen(
        sample.dc,
        inner,
        at(2.5),
        PANEL_BACKGROUND,
        TRAY_EDGE,
        at(1.0),
    );
    // The panel pencil spans (4..24, 6..25); center it on (9.5, 7) at 9 units wide.
    let s = 9.0 * u / 20.0;
    let origin = (at(9.5) - scaled(14, s), at(7.0) - (15.5 * s).round() as i32);
    let ink = if drawing { PANEL_TEXT } else { PANEL_SECONDARY };
    panel_icon(sample.dc, tool, tail_first, origin, s, ink);
    ellipse(sample.dc, dot.0, dot.1, at(3.5), PANEL_BACKGROUND, 1, true);
    if drawing {
        // A thin rim keeps black ink visible on the graphite tile.
        ellipse(sample.dc, dot.0, dot.1, at(2.5), TRAY_EDGE, 1, true);
        ellipse(sample.dc, dot.0, dot.1, at(2.0), color, 1, true);
    } else {
        ellipse(sample.dc, dot.0, dot.1, at(2.5), PANEL_SECONDARY, 1, true);
        ellipse(sample.dc, dot.0, dot.1, at(1.5), PANEL_BACKGROUND, 1, true);
    }
    let source = sample.pixels();
    let mut pixels = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut sums = [0_u32; 4];
            for dy in 0..n {
                for dx in 0..n {
                    let i = ((y * n + dy) * size * n + x * n + dx) as usize;
                    if coverage[i] {
                        sums[3] += 1;
                        for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
                            sums[channel] += (source[i] >> shift) & 255;
                        }
                    }
                }
            }
            let mut pixel = 0;
            if sums[3] > 0 {
                let count = (n * n) as u32;
                pixel = ((sums[3] * 255 + count / 2) / count) << 24;
                for (channel, shift) in [0, 8, 16].into_iter().enumerate() {
                    pixel |= ((sums[channel] + sums[3] / 2) / sums[3]) << shift;
                }
            }
            pixels.push(pixel);
        }
    }
    Ok(pixels)
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
    if geometry.ring {
        paint_ring(state, dc, geometry);
        return;
    }
    if geometry.layout == MenuLayout::Rail && geometry.opacity > 0.0 {
        paint_rail(state, dc, geometry);
    } else if geometry.opacity > 0.0
        && let Ok(font) = panel_font(s)
    {
        unsafe {
            let saved = SaveDC(dc);
            SelectObject(dc, font.0);
            paint_pin(state, dc, geometry.bounds(PanelHit::Pin), s);
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
                &state.shortcuts.get().binding("activation_key").label(),
                rect(
                    toggle.right - scaled(72, s),
                    toggle.top,
                    scaled(64, s),
                    scaled(31, s),
                ),
                PANEL_SECONDARY,
                DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_END_ELLIPSIS,
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
            let small_font = panel_font(s * 0.72);
            let hover = state.hover.get();
            let active = geometry.bounds(PanelHit::Active);
            let reserve = geometry.bounds(PanelHit::Reserve);
            paint_favorites(state, dc, geometry);
            let swap = geometry.bounds(PanelHit::Swap);
            let arrow = |points: &[(i32, i32)]| {
                panel_line(
                    dc,
                    points,
                    s,
                    (swap.left, swap.top),
                    if hover == Some(PanelHit::Swap) {
                        PANEL_TEXT
                    } else {
                        PANEL_SECONDARY
                    },
                    (1.5 * s).round() as i32,
                )
            };
            // Bent two-headed arrow from the active square to the reserve square.
            arrow(&[(1, 3), (9, 3), (9, 11)]);
            arrow(&[(3, 1), (1, 3), (3, 5)]);
            arrow(&[(7, 9), (9, 11), (11, 9)]);
            let key = rect(
                reserve.right + scaled(6, s),
                active.top + scaled(8, s),
                scaled(13, s),
                scaled(17, s),
            );
            rounded_pen(
                dc,
                key,
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
                &shortcut_badge(state.shortcuts.get().binding("swap_colors")),
                key,
                PANEL_TEXT,
                DT_SINGLELINE | DT_CENTER | DT_VCENTER,
            );
            SelectObject(dc, font.0);
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
                PANEL_TEXT,
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
            for (tool, _, _, _) in PANEL_TOOLS {
                let r = geometry.bounds(PanelHit::Tool(tool));
                paint_tool(state, dc, tool, r, s);
                let key = shortcut_badge(state.shortcuts.get().binding(tool_shortcut(tool)));
                let badge_width = if key.len() > 2 { 22 } else { 13 };
                let badge = rect(
                    r.right - scaled(badge_width + 2, s),
                    r.top + scaled(4, s),
                    scaled(badge_width, s),
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
                    &key,
                    badge,
                    PANEL_TEXT,
                    DT_SINGLELINE | DT_CENTER | DT_VCENTER,
                );
                SelectObject(dc, font.0);
            }
            paint_swatch_drag(state, dc, s);
            let _ = RestoreDC(dc, saved);
        }
    }
    // The icon/grip stay opaque; only the body has a border, never an inner badge.
    let badge = geometry.badge();
    panel_icon(
        dc,
        state.tool.get(),
        *state.tool_label.borrow() == "Arrow tail-first",
        (badge.left + scaled(2, s), badge.top + scaled(6, s)),
        s,
        // ON/OFF matches the tray mark: bright while drawing, dimmed in click-through.
        if state.drawing.get() {
            PANEL_TEXT
        } else {
            PANEL_SECONDARY
        },
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
        // Reserve square follows the tip's ON/OFF rule: filled while drawing, hollow otherwise.
        let (inside, border, pen) = if state.drawing.get() {
            (state.reserve.get(), 0xff737c85, scaled(1, s))
        } else {
            (PANEL_BACKGROUND, PANEL_SECONDARY, (1.5 * s).round() as i32)
        };
        rounded_pen(dc, geometry.reserve_square(), 0, inside, border, pen);
        let (x, y) = geometry.tip_center();
        let r = geometry.tip_radius.round() as i32;
        // Drawing: ink-filled dot. Click-through: hollow gray ring; size still shows width.
        // The pen straddles the coverage edge, so only the inner half of the ring shows.
        let (pen_width, pen_color, fill) = if state.drawing.get() {
            (1.25, 0xff737c85, state.color.get())
        } else {
            (3.0, PANEL_SECONDARY, PANEL_BACKGROUND)
        };
        unsafe {
            let pen = GdiObject(
                CreatePen(
                    PS_SOLID,
                    (pen_width * s).round().max(1.0) as i32,
                    colorref(pen_color),
                )
                .into(),
            );
            let brush = GdiObject(CreateSolidBrush(colorref(fill)).into());
            let old_pen = SelectObject(dc, pen.0);
            let old_brush = SelectObject(dc, brush.0);
            let _ = Ellipse(dc, x - r, y - r, x + r, y + r);
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
        }
    }
}

// Upright and highlighted while pinned, tilted otherwise.
fn paint_pin(state: &State, dc: HDC, pin: RECT, s: f64) {
    let pinned = state.pinned.get();
    let hovered = state.hover.get() == Some(PanelHit::Pin);
    let pressed = state.pressed.get() == Some(PanelHit::Pin);
    rounded_pen(
        dc,
        pin,
        scaled(4, s),
        if pressed {
            0xff426c5f
        } else if pinned {
            0xff263b36
        } else if hovered {
            0xff30363d
        } else {
            PANEL_BACKGROUND
        },
        if pinned {
            PANEL_ACCENT
        } else if hovered {
            PANEL_SECONDARY
        } else {
            PANEL_BORDER
        },
        scaled(1, s),
    );
    let points = [
        (15, 27),
        (15, 21),
        (8, 21),
        (12, 17),
        (12, 11),
        (10, 6),
        (21, 6),
        (19, 11),
        (19, 17),
        (23, 21),
        (15, 21),
    ];
    let points = points.map(|(x, y)| {
        if pinned {
            (x, y)
        } else {
            let dx = f64::from(x - 15);
            let dy = f64::from(y - 15);
            (
                (15.0 + (dx + dy) * std::f64::consts::FRAC_1_SQRT_2).round() as i32,
                (15.0 + (dy - dx) * std::f64::consts::FRAC_1_SQRT_2).round() as i32,
            )
        }
    });
    panel_line(
        dc,
        &points,
        s,
        (pin.left, pin.top),
        if pinned { PANEL_TEXT } else { PANEL_SECONDARY },
        scaled(2, s),
    );
}

// Favorites: reserve behind, active in front with a background gap.
fn paint_favorites(state: &State, dc: HDC, geometry: &PanelGeometry) {
    let s = geometry.scale;
    let target = state.swatch_target();
    let hover = state.hover.get();
    let active = geometry.bounds(PanelHit::Active);
    let reserve = geometry.bounds(PanelHit::Reserve);
    let edge = |hit, normal| {
        if target == Some(hit) {
            (PANEL_ACCENT, scaled(2, s))
        } else if hover == Some(hit) {
            (PANEL_TEXT, scaled(1, s))
        } else {
            (normal, scaled(1, s))
        }
    };
    let (border, pen) = edge(PanelHit::Reserve, PANEL_SECONDARY);
    rounded_pen(dc, reserve, 0, state.reserve.get(), border, pen);
    let gap = scaled(2, s);
    fill(
        dc,
        rect(
            active.left - gap,
            active.top - gap,
            active.right - active.left + 2 * gap,
            active.bottom - active.top + 2 * gap,
        ),
        PANEL_BACKGROUND,
    );
    let (border, pen) = edge(PanelHit::Active, PANEL_TEXT);
    rounded_pen(dc, active, 0, state.color.get(), border, pen);
}

fn tool_fill(selected: bool, hovered: bool, pressed: bool) -> u32 {
    if pressed {
        0xff426c5f
    } else if hovered && selected {
        0xff36584d
    } else if hovered {
        0xff30363d
    } else {
        0xff263b36
    }
}

fn paint_tool(state: &State, dc: HDC, tool: MenuTool, r: RECT, s: f64) {
    let hit = PanelHit::Tool(tool);
    let selected = state.tool.get() == tool;
    let hovered = state.hover.get() == Some(hit);
    let pressed = state.pressed.get() == Some(hit);
    if selected || hovered || pressed {
        rounded_pen(
            dc,
            r,
            scaled(3, s),
            tool_fill(selected, hovered, pressed),
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
        *state.tool_label.borrow() == "Arrow tail-first",
        (r.left + scaled(2, s), r.top + scaled(2, s)),
        s,
        if selected {
            PANEL_TEXT
        } else {
            PANEL_SECONDARY
        },
    );
}

fn paint_swatch_drag(state: &State, dc: HDC, s: f64) {
    if let (Some(drag), Some(PanelHit::Palette(i))) = (
        state.swatch.get().filter(|drag| drag.moving),
        state.pressed.get(),
    ) {
        // Painting uses the supersampled frame; its scale ratio is the sample factor.
        let n = (s / state.panel_scale.get()).round() as i32;
        let origin = PanelGeometry::new(state).destination(state.panel_anchor.get());
        let (x, y) = ((drag.point.x - origin.x) * n, (drag.point.y - origin.y) * n);
        ellipse(dc, x, y, scaled(10, s), PANEL_PALETTE[i], 1, true);
        ellipse(dc, x, y, scaled(10, s), PANEL_TEXT, scaled(1, s), false);
    }
}

// One column: input toggle, pin, tools, favorites, two palette columns and a
// vertical width slider. Hotkey letters live in the tooltips to keep it narrow.
fn paint_rail(state: &State, dc: HDC, geometry: &PanelGeometry) {
    let s = geometry.scale;
    let Ok(font) = panel_font(s) else {
        return;
    };
    unsafe {
        let saved = SaveDC(dc);
        SelectObject(dc, font.0);
        let divider = |y: i32| {
            fill(
                dc,
                rect(
                    geometry.body.left + scaled(8, s),
                    y,
                    geometry.body.right - geometry.body.left - scaled(16, s),
                    scaled(1, s),
                ),
                PANEL_BORDER,
            )
        };
        let drawing = state.drawing.get();
        let toggle = geometry.bounds(PanelHit::Toggle);
        rounded_pen(
            dc,
            toggle,
            scaled(4, s),
            if state.hover.get() == Some(PanelHit::Toggle) {
                0xff30363d
            } else {
                PANEL_BACKGROUND
            },
            if drawing { PANEL_ACCENT } else { PANEL_BORDER },
            scaled(1, s),
        );
        let switch = rect(
            toggle.left + scaled(5, s),
            toggle.top + scaled(8, s),
            scaled(20, s),
            scaled(12, s),
        );
        rounded_pen(
            dc,
            switch,
            scaled(6, s),
            PANEL_BACKGROUND,
            if drawing {
                PANEL_ACCENT
            } else {
                PANEL_SECONDARY
            },
            scaled(1, s),
        );
        ellipse(
            dc,
            switch.left + scaled(if drawing { 14 } else { 6 }, s),
            switch.top + scaled(6, s),
            scaled(4, s),
            PANEL_ACCENT,
            1,
            true,
        );
        paint_pin(state, dc, geometry.bounds(PanelHit::Pin), s);
        divider(geometry.bounds(PanelHit::Tool(MenuTool::Pen)).top - scaled(5, s));
        for (tool, ..) in PANEL_TOOLS {
            paint_tool(state, dc, tool, geometry.bounds(PanelHit::Tool(tool)), s);
        }
        divider(geometry.bounds(PanelHit::Active).top - scaled(6, s));
        paint_favorites(state, dc, geometry);
        for (i, color) in PANEL_PALETTE.iter().copied().enumerate() {
            let (x, y) = geometry.center(PanelHit::Palette(i));
            ellipse(dc, x, y, scaled(7, s), color, 1, true);
            ellipse(dc, x, y, scaled(7, s), PANEL_SECONDARY, scaled(1, s), false);
            if color & 0xffffff == state.color.get() & 0xffffff {
                ellipse(dc, x, y, scaled(8, s), PANEL_ACCENT, scaled(2, s), false);
            }
        }
        let slider = geometry.bounds(PanelHit::Width);
        divider(slider.top - scaled(24, s));
        label(
            dc,
            &state.width.get().to_string(),
            rect(
                geometry.body.left,
                slider.top - scaled(21, s),
                geometry.body.right - geometry.body.left,
                scaled(18, s),
            ),
            PANEL_TEXT,
            DT_SINGLELINE | DT_CENTER | DT_VCENTER,
        );
        let cx = (slider.left + slider.right) / 2;
        let track = |top: i32, color: u32| {
            fill(
                dc,
                rect(cx - scaled(2, s), top, scaled(4, s), slider.bottom - top),
                color,
            )
        };
        let thumb = slider.bottom
            - ((slider.bottom - slider.top) as f64
                * (state.width.get().saturating_sub(1).min(30) as f64 / 30.0))
                .round() as i32;
        track(slider.top, PANEL_BORDER);
        track(thumb, PANEL_ACCENT);
        ellipse(dc, cx, thumb, scaled(8, s), 0xff364147, 1, true);
        ellipse(dc, cx, thumb, scaled(5, s), PANEL_ACCENT, 1, true);
        paint_swatch_drag(state, dc, s);
        let _ = RestoreDC(dc, saved);
    }
}

// Filled ring sector between two radii and two angles, with an optional outline.
fn annulus(
    dc: HDC,
    center: (i32, i32),
    radii: (f64, f64),
    angles: (f64, f64),
    color: u32,
    border: Option<(u32, i32)>,
) {
    let steps = ((angles.1 - angles.0).abs() * 24.0).ceil().max(2.0) as usize;
    let point = |radius: f64, k: usize| {
        let angle = angles.0 + (angles.1 - angles.0) * k as f64 / steps as f64;
        POINT {
            x: center.0 + (radius * angle.cos()).round() as i32,
            y: center.1 + (radius * angle.sin()).round() as i32,
        }
    };
    let points: Vec<POINT> = (0..=steps)
        .map(|k| point(radii.1, k))
        .chain((0..=steps).rev().map(|k| point(radii.0, k)))
        .collect();
    unsafe {
        let brush = GdiObject(CreateSolidBrush(colorref(color)).into());
        let pen = border.map(|(color, width)| {
            GdiObject(CreatePen(PS_SOLID, width.max(1), colorref(color)).into())
        });
        let old_brush = SelectObject(dc, brush.0);
        let old_pen = SelectObject(dc, pen.as_ref().map_or(GetStockObject(NULL_PEN), |p| p.0));
        let _ = Polygon(dc, &points);
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
    }
}

fn arc(dc: HDC, center: (i32, i32), radius: f64, angles: (f64, f64), color: u32, width: i32) {
    let steps = ((angles.1 - angles.0).abs() * 24.0).ceil().max(2.0) as usize;
    let points: Vec<POINT> = (0..=steps)
        .map(|k| {
            let angle = angles.0 + (angles.1 - angles.0) * k as f64 / steps as f64;
            POINT {
                x: center.0 + (radius * angle.cos()).round() as i32,
                y: center.1 + (radius * angle.sin()).round() as i32,
            }
        })
        .collect();
    unsafe {
        let pen = GdiObject(CreatePen(PS_SOLID, width.max(1), colorref(color)).into());
        let old = SelectObject(dc, pen.0);
        let _ = Polyline(dc, &points);
        SelectObject(dc, old);
    }
}

// Radial ring: 16 colors outside, 7 tools inside, and the brush at true width in
// the center (click swaps with the reserve). Toggle and pin stay in the tray and F4.
fn paint_ring(state: &State, dc: HDC, geometry: &PanelGeometry) {
    let s = geometry.scale;
    let center = geometry.ring_center();
    let (cx, cy) = center;
    let hover = state.hover.get();
    let pressed = state.pressed.get();
    fill(
        dc,
        rect(0, 0, geometry.width, geometry.height),
        PANEL_BACKGROUND,
    );
    ellipse(
        dc,
        cx,
        cy,
        (RING_EDGE * s).round() as i32 - scaled(1, s),
        PANEL_BORDER,
        scaled(1, s),
        false,
    );
    let sector = |index: usize, count: usize, inner: f64| {
        let half = std::f64::consts::PI / count as f64;
        // About 1.5 logical px of background between neighbors at the inner edge.
        let gap = 1.5 / inner;
        let middle = ring_angle(index, count);
        (middle - half + gap, middle + half - gap)
    };
    let colors = (RING_COLORS.0 * s, RING_COLORS.1 * s);
    for (i, color) in PANEL_PALETTE.iter().copied().enumerate() {
        let angles = sector(i, PANEL_PALETTE.len(), RING_COLORS.0);
        annulus(
            dc,
            center,
            colors,
            angles,
            color,
            Some((TRAY_EDGE, scaled(1, s))),
        );
        let outline = if color & 0xffffff == state.color.get() & 0xffffff {
            Some(PANEL_ACCENT)
        } else if hover == Some(PanelHit::Palette(i)) {
            Some(PANEL_TEXT)
        } else {
            None
        };
        if let Some(outline) = outline {
            arc(
                dc,
                center,
                (RING_COLORS.1 + 4.0) * s,
                angles,
                outline,
                scaled(2, s),
            );
        }
    }
    let tools = (RING_TOOLS.0 * s, RING_TOOLS.1 * s);
    for (i, (tool, ..)) in PANEL_TOOLS.into_iter().enumerate() {
        let hit = PanelHit::Tool(tool);
        let selected = state.tool.get() == tool;
        let hovered = hover == Some(hit);
        let active = pressed == Some(hit);
        let angles = sector(i, PANEL_TOOLS.len(), RING_TOOLS.0);
        let background = if selected || hovered || active {
            tool_fill(selected, hovered, active)
        } else {
            0xff23272c
        };
        annulus(dc, center, tools, angles, background, None);
        if selected {
            arc(
                dc,
                center,
                (RING_TOOLS.1 - 1.5) * s,
                angles,
                PANEL_ACCENT,
                scaled(2, s),
            );
        }
        let (x, y) = geometry.center(hit);
        panel_icon(
            dc,
            tool,
            *state.tool_label.borrow() == "Arrow tail-first",
            (x - scaled(14, s), y - scaled(16, s)),
            s,
            if selected {
                PANEL_TEXT
            } else {
                PANEL_SECONDARY
            },
        );
    }
    // Collapse button: a small graphite disc with a minimize bar.
    // Corner buttons: minimize bar top-left, drag grip dots top-right.
    let button = (RING_COLLAPSE.1 * s).round() as i32;
    for (hit, (bx, by)) in [
        (PanelHit::Collapse, geometry.collapse_center()),
        (PanelHit::Grip, geometry.ring_grip_center()),
    ] {
        let hovered = hover == Some(hit);
        let ink = if hovered { PANEL_TEXT } else { PANEL_SECONDARY };
        ellipse(
            dc,
            bx,
            by,
            button,
            if pressed == Some(hit) {
                0xff426c5f
            } else if hovered {
                0xff30363d
            } else {
                0xff23272c
            },
            1,
            true,
        );
        ellipse(dc, bx, by, button - scaled(1, s), ink, scaled(1, s), false);
        if hit == PanelHit::Collapse {
            panel_line(dc, &[(-5, 0), (5, 0)], s, (bx, by), ink, scaled(2, s));
        } else {
            for dx in [-2, 2] {
                for dy in [-4, 0, 4] {
                    let (x, y) = (bx + scaled(dx, s), by + scaled(dy, s));
                    ellipse(dc, x, y, scaled(1, s).max(1), ink, 1, true);
                }
            }
        }
    }
    let middle = if pressed == Some(PanelHit::Swap) {
        0xff426c5f
    } else if hover == Some(PanelHit::Swap) {
        0xff30363d
    } else {
        0xff23272c
    };
    ellipse(
        dc,
        cx,
        cy,
        (RING_CENTER * s).round() as i32,
        middle,
        1,
        true,
    );
    // Reserve behind, active in front at the true stroke width. A gray rim keeps
    // black ink visible on the graphite center.
    let rim = 0xff737c85;
    let (rx, ry) = (cx + scaled(17, s), cy - scaled(17, s));
    ellipse(dc, rx, ry, scaled(5, s), rim, 1, true);
    ellipse(dc, rx, ry, scaled(4, s), state.reserve.get(), 1, true);
    // Thin brushes keep a readable dot that still outsizes the reserve.
    let radius = (scaled(state.width.get() as i32, s) / 2).max(scaled(6, s));
    let ay = cy - scaled(5, s);
    ellipse(dc, cx, ay, radius + scaled(2, s), middle, 1, true);
    ellipse(dc, cx, ay, radius + scaled(1, s), rim, 1, true);
    ellipse(dc, cx, ay, radius, state.color.get(), 1, true);
    if let Ok(font) = panel_font(s * 0.8) {
        unsafe {
            let saved = SaveDC(dc);
            SelectObject(dc, font.0);
            label(
                dc,
                &state.width.get().to_string(),
                rect(
                    cx - scaled(20, s),
                    cy + scaled(13, s),
                    scaled(40, s),
                    scaled(16, s),
                ),
                PANEL_SECONDARY,
                DT_SINGLELINE | DT_CENTER | DT_VCENTER,
            );
            let _ = RestoreDC(dc, saved);
        }
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
                        state.post(UiEvent::Width(geometry.requested_width(x, y)));
                    } else if state.swatch.get().is_some() {
                        state.move_swatch(screen);
                    } else {
                        if geometry.contains(x, y) && state.allow_hover_reveal(screen) {
                            state.stop_leave_timer();
                            state.reveal_panel(true);
                        }
                        let geometry = PanelGeometry::new(state);
                        let origin = geometry.destination(state.panel_anchor.get());
                        let hit = geometry.hit(screen.x - origin.x, screen.y - origin.y);
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
                    let hit = PanelGeometry::new(state).hit(point.x, point.y);
                    let mut screen = point;
                    let _ = ClientToScreen(hwnd, &mut screen);
                    state.begin_panel_gesture(hit, screen);
                    return LRESULT(0);
                }
                WM_LBUTTONUP => {
                    let x = l.0 as u16 as i16 as i32;
                    let y = (l.0 >> 16) as u16 as i16 as i32;
                    let geometry = PanelGeometry::new(state);
                    let hit = geometry.hit(x, y);
                    state.tracking.set(geometry.contains(x, y));
                    state.finish_panel_gesture(hit, POINT { x, y });
                    return LRESULT(0);
                }
                WM_MOUSEWHEEL if PanelGeometry::new(state).ring => {
                    // Drawing mode's wheel hook already sizes the brush everywhere. This
                    // covers a ring opened in click-through. ponytail: partial notches
                    // from precision touchpads are ignored rather than accumulated.
                    let steps = (w.0 >> 16) as u16 as i16 as i32 / WHEEL_DELTA as i32;
                    if steps != 0 {
                        let width = (state.width.get() as i32 + 2 * steps).clamp(1, 31);
                        state.post(UiEvent::Width(width as u32));
                    }
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
                            state.finish_panel_gesture(None, point);
                        } else {
                            state.cancel_panel_gesture();
                            state.refresh_panel_hover();
                            state.reveal_panel(state.tracking.get());
                        }
                    } else if state.panel_drag.get().is_some() {
                        state.move_panel_grip(point);
                    } else if state.swatch.get().is_some() {
                        state.move_swatch(point);
                    } else if state.pressed.get() == Some(PanelHit::Width) {
                        let _ = ScreenToClient(hwnd, &mut point);
                        let geometry = PanelGeometry::new(state);
                        state.post(UiEvent::Width(geometry.requested_width(point.x, point.y)));
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

    const ORANGE: u32 = PANEL_PALETTE[2];

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
                w!("Hidden UI test"),
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
            _thread: PhantomData,
        })
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
    fn thread_windows(hwnd: HWND) -> Result<Vec<HWND>> {
        unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
            unsafe { &mut *(data.0 as *mut Vec<HWND>) }.push(hwnd);
            BOOL(1)
        }
        let mut windows = Vec::<HWND>::new();
        unsafe {
            EnumThreadWindows(
                GetWindowThreadProcessId(hwnd, None),
                Some(collect),
                LPARAM(&mut windows as *mut Vec<HWND> as isize),
            )
            .ok()?;
        }
        windows.sort_by_key(|hwnd| hwnd.0 as usize);
        Ok(windows)
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
                rect(112, if upward { 267 } else { 7 }, 234, 31)
            );
            assert_eq!(g.bounds(PanelHit::Current), rect(92, 48 + offset, 324, 32));
            assert_eq!(g.bounds(PanelHit::Active), rect(26, 48 + offset, 22, 22));
            assert_eq!(g.bounds(PanelHit::Reserve), rect(40, 58 + offset, 22, 22));
            // The front square owns the overlap; the exposed corner still hits Reserve.
            assert_eq!(g.hit(44, 66 + offset), Some(PanelHit::Active));
            assert_eq!(g.hit(56, 76 + offset), Some(PanelHit::Reserve));
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
                PanelHit::Active,
                PanelHit::Reserve,
                PanelHit::Swap,
                PanelHit::Current,
                PanelHit::Width,
            ]
            .into_iter()
            .chain((0..16).map(PanelHit::Palette))
            .chain(PANEL_TOOLS.map(|entry| PanelHit::Tool(entry.0)))
            {
                let (x, y) = g.center(target);
                assert_eq!(g.hit(x, y), Some(target));
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
            assert_eq!(g.hit(x - 2, y), None);
            assert!(!g.contains(0, 0));
            // ON/OFF feedback: ink-filled tip and bright pencil only while drawing.
            let badge = g.badge();
            let pencil = |pixels: &[u32]| {
                pixels
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        let (px, py) = (
                            (*i % g.width as usize) as i32,
                            (*i / g.width as usize) as i32,
                        );
                        px >= badge.left
                            && px < badge.left + 30
                            && py >= badge.top
                            && py < badge.top + 36
                    })
                    .any(|(_, pixel)| *pixel == PANEL_TEXT)
            };
            state.drawing.set(true);
            let on = panel_bitmap(&state)?.1;
            assert_eq!(on[(y * g.width + x - 2) as usize], state.color.get());
            assert!(pencil(&on));
            state.drawing.set(false);
            let pixels = panel_bitmap(&state)?.1;
            assert_eq!(pixels[(y * g.width + x - 2) as usize], PANEL_BACKGROUND);
            assert!(!pencil(&pixels));
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
    fn settings_hidden_menu_startup_stays_hidden_during_style_updates() -> Result<()> {
        let ui = DesktopUi::new(HWND::default(), 1920, 1080, false, ActivationKey::default())?;
        let hwnd = ui.state.status.get();
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        ui.update_label(
            false,
            "Arrow tail-first",
            Style {
                color: 0xff123456,
                thickness: 9,
            },
        )?;
        ui.set_reserve(0xffabcdef)?;
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        assert_eq!(ui.state.width.get(), 9);
        assert_eq!(ui.state.tool_label.borrow().as_str(), "Arrow tail-first");
        Ok(())
    }

    #[test]
    fn panel_arrow_direction_changes_compact_and_expanded_glyphs() -> Result<()> {
        let state = State::new(HWND::default())?;
        state.tool.set(MenuTool::Arrow);
        for scale in [1.0, 1.25, 2.0] {
            state.panel_scale.set(scale);
            for drawing in [false, true] {
                state.drawing.set(drawing);
                for expanded in [false, true] {
                    state.expanded.set(expanded);
                    state.motion.set(PanelMotion::settled(expanded));
                    let g = PanelGeometry::new(&state);
                    *state.tool_label.borrow_mut() = "Arrow head-first".into();
                    let head = panel_bitmap(&state)?.1;
                    *state.tool_label.borrow_mut() = "Arrow tail-first".into();
                    let tail = panel_bitmap(&state)?.1;
                    assert_ne!(head, tail);
                    assert!(head.iter().zip(&tail).all(|(a, b)| a >> 24 == b >> 24));
                    let differs_in = |r: RECT| {
                        (r.top..r.bottom).any(|y| {
                            (r.left..r.right).any(|x| {
                                let i = (y * g.width + x) as usize;
                                head[i] != tail[i]
                            })
                        })
                    };
                    assert!(differs_in(g.badge()));
                    if expanded {
                        assert!(differs_in(g.bounds(PanelHit::Tool(MenuTool::Arrow))));
                    }
                    if scale == 1.25 && drawing {
                        for (label, name) in [
                            ("Arrow head-first", "head-first"),
                            ("Arrow tail-first", "tail-first"),
                        ] {
                            *state.tool_label.borrow_mut() = label.into();
                            followup_preview(
                                &state,
                                &format!(
                                    "arrow-{name}-{}",
                                    if expanded { "expanded" } else { "compact" }
                                ),
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn settings_reload_shortcuts_refreshes_badges_tooltips_without_focus_or_collapse() -> Result<()>
    {
        let ui = DesktopUi::new(HWND::default(), 1920, 1080, false, Shortcuts::default())?;
        menu_open(&ui)?;
        ui.state.pinned.set(true);
        let anchor = ui.state.panel_anchor.get();
        let focus = unsafe { GetFocus() };
        let (before, _) = followup_preview(&ui.state, "shortcuts-default")?;
        let mut shortcuts = Shortcuts::default();
        shortcuts.set("activation_key", Shortcut::parse("Ctrl+Alt+F10").unwrap());
        shortcuts.set("toggle", Shortcut::parse("Alt+D").unwrap());
        shortcuts.set("pen", Shortcut::parse("Shift+B").unwrap());
        shortcuts.set("smoke_pen", Shortcut::parse("F11").unwrap());
        shortcuts.set("swap_colors", Shortcut::parse("Alt+V").unwrap());
        ui.set_shortcuts(shortcuts)?;
        for tool in [MenuTool::Pen, MenuTool::TemporaryPen, MenuTool::Arrow] {
            ui.state.panel_tooltip(Some(PanelHit::Tool(tool)));
            let text = String::from_utf16_lossy(&ui.state.tooltip_text.borrow());
            assert!(text.contains(&shortcuts.binding(tool_shortcut(tool)).label()));
            assert!(text.starts_with(match tool {
                MenuTool::Pen => "Pen.",
                MenuTool::TemporaryPen => "Smoke pen.",
                _ => "Arrow.",
            }));
        }
        ui.state.panel_tooltip(Some(PanelHit::Toggle));
        assert!(
            String::from_utf16_lossy(&ui.state.tooltip_text.borrow())
                .contains("Ctrl+Alt+F10 or Alt+D")
        );
        ui.state.panel_tooltip(Some(PanelHit::Reserve));
        assert!(String::from_utf16_lossy(&ui.state.tooltip_text.borrow()).contains("Alt+V"));
        assert_eq!(shortcut_badge(shortcuts.binding("pen")), "B");
        assert_eq!(shortcut_badge(shortcuts.binding("smoke_pen")), "F11");
        let (after, _) = followup_preview(&ui.state, "shortcuts-remapped")?;
        assert_ne!(before, after);
        assert!(ui.state.pinned.get() && ui.state.expanded.get());
        assert_eq!(ui.state.panel_anchor.get(), anchor);
        assert_eq!(unsafe { GetFocus() }, focus);
        assert!(!unsafe { IsWindowVisible(ui.state.status.get()) }.as_bool());
        Ok(())
    }

    #[test]
    fn settings_activation_key_updates_menu_label_and_tooltip() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "ActivationKeyRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = DesktopUi::new(
            root.0,
            1920,
            1080,
            false,
            ActivationKey::parse("F10").unwrap(),
        )?;
        assert!(!unsafe { IsWindowVisible(ui.state.status.get()) }.as_bool());
        ui.state.panel_tooltip(Some(PanelHit::Toggle));
        assert_eq!(
            String::from_utf16_lossy(&ui.state.tooltip_text.borrow()),
            "Drawing / Click-through. F10 or Ctrl+Alt+D toggles canvas input.\0"
        );
        menu_open(&ui)?;
        let (custom, _) = followup_preview(&ui.state, "activation-f10")?;
        ui.state.shortcuts.set(Shortcuts::default());
        let (default, _) = followup_preview(&ui.state, "activation-f4")?;
        assert_ne!(custom, default);
        Ok(())
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
                "Smoke pen"
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
            // The ink-filled tip is the drawing-mode state.
            ui.update_label(
                true,
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
        panel_click(
            hwnd,
            PanelGeometry::new(&ui.state).center(PanelHit::Palette(10)),
        );
        assert_eq!(decoded(root.0), [UiEvent::Color(PANEL_PALETTE[10])]);
        assert_eq!(ui.state.color.get(), PANEL_PALETTE[10]);
        ui.update_label(
            true,
            "Arrow tail-first",
            Style {
                color: 0xff123456,
                thickness: 15,
            },
        )?;
        // Width keyboard commits use the same update_label path as palette commits.
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
            // The ink-filled tip is the drawing-mode state.
            ui.update_label(
                true,
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
        // The favorite squares own the row's left end.
        for y in scaled(40, s)..scaled(49, s) {
            for x in scaled(92, s)..scaled(380, s) {
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
    fn panel_menu_followup_antialiased_circle_edge_keeps_hover_bridge() -> Result<()> {
        let _dpi = followup_thread_dpi()?;
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "FollowupCircleEdgeRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        status(&ui)?;
        ui.state.panel_scale.set(1.25);
        ui.state.panel_dpi.set(120);
        ui.state.panel_upward.set(true);
        let work = ui.state.panel_monitor.get();
        ui.state.panel_anchor.set(POINT {
            x: work.right - scaled(16, 1.25),
            y: work.bottom - scaled(80, 1.25),
        });
        ui.update_label(
            false,
            "Pen",
            Style {
                color: ORANGE,
                thickness: 31,
            },
        )?;
        let compact = PanelGeometry::new(&ui.state);
        let pixels = panel_bitmap(&ui.state)?.1;
        let edge = (1, 55);
        assert!(pixels[(edge.1 * compact.width + edge.0) as usize] >> 24 > 0);
        assert!(compact.contains(edge.0, edge.1));
        assert_eq!(compact.hit(edge.0, edge.1), None);
        let origin = compact.destination(ui.state.panel_anchor.get());
        let screen = POINT {
            x: origin.x + edge.0,
            y: origin.y + edge.1,
        };
        assert!(
            ui.state.initial_circle_contains(screen),
            "Visible edge must join the hover bridge"
        );
        for (i, pixel) in pixels.iter().enumerate() {
            let x = (i % compact.width as usize) as i32;
            let y = (i / compact.width as usize) as i32;
            if pixel >> 24 != 0 && (x < compact.body.left || y >= compact.body.bottom) {
                assert!(ui.state.initial_circle_contains(POINT {
                    x: origin.x + x,
                    y: origin.y + y
                }));
            }
        }
        ui.state.start_panel_reveal(true, true);
        assert!(ui.state.opening_hover_bridge(screen));
        ui.state.motion.set(PanelMotion::settled(true));
        ui.state.sync_animation_timer();
        assert!(!ui.state.target_contains(screen, true));
        assert!(ui.state.defer_circle_leave(screen));
        assert!(ui.state.leave_timer.get());
        ui.state.finish_circle_leave(screen);
        assert!(!ui.state.leave_timer.get());
        assert!(!ui.state.expanded.get());
        assert!(!ui.state.allow_hover_reveal(screen));
        assert!(!ui.state.allow_hover_reveal(screen));
        assert!(ui.state.allow_hover_reveal(POINT {
            x: screen.x + 1,
            ..screen
        }));
        assert!(!ui.state.animation_timer.get() && !ui.state.panel_guard.get());
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
            let scale = panel_scale(dpi, work, MenuLayout::Panel);
            state.panel_scale.set(scale);
            let (anchor, upward) = panel_clamp(
                POINT {
                    x: work.right + 50,
                    y: work.bottom + 50,
                },
                work,
                scale,
                MenuLayout::Panel,
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
            ["P", "S", "L", "A", "R", "O", "E"]
        );
        assert_eq!(tool_icon("Temporary pen"), MenuTool::TemporaryPen);
        assert_eq!(tool_icon("Arrow tail-first"), MenuTool::Arrow);
        assert!(
            PANEL_TOOLS[0]
                .3
                .contains("P or click selects permanent pen")
        );
        assert!(
            PANEL_TOOLS[1]
                .3
                .contains("S or click selects temporary fading ink")
        );
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
                    PanelHit::Active,
                    PanelHit::Reserve,
                    PanelHit::Swap,
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
                    assert_eq!(g.hit(x, y), Some(target));
                }
                assert_eq!(g.hit(-1, 0), None);
                assert_eq!(g.hit(g.width, g.height), None);
                assert_eq!(g.hit(0, 0), None);
                let slider = g.bounds(PanelHit::Width);
                assert_eq!(g.requested_width(slider.left - 100, 0), 1);
                assert_eq!(g.requested_width(slider.right + 100, 0), 31);
                for x in slider.left..slider.right {
                    let width = g.requested_width(x, 0);
                    assert!((1..=31).contains(&width) && width % 2 == 1);
                }
                state.motion.set(PanelMotion::settled(false));
                let compact = PanelGeometry::new(&state);
                assert_eq!(
                    compact.hit(
                        compact.center(PanelHit::Grip).0,
                        compact.center(PanelHit::Grip).1
                    ),
                    Some(PanelHit::Grip)
                );
                // The whole badge, including the tool icon, is the drag handle.
                for geometry in [&g, &compact] {
                    let b = geometry.badge();
                    for (x, y) in [
                        (b.left + scaled(8, s), b.top + scaled(8, s)),
                        (b.left + scaled(20, s), b.top + scaled(22, s)),
                        (b.right - scaled(8, s), b.bottom - scaled(8, s)),
                    ] {
                        assert_eq!(geometry.hit(x, y), Some(PanelHit::Grip));
                    }
                }
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
        message.lParam = LPARAM(1);
        message.wParam = WPARAM(0xff123456);
        assert_eq!(
            decode_event(&message, owner),
            Some(UiEvent::Color(0xff123456))
        );
        message.wParam = WPARAM(u32::MAX as usize + 1);
        assert_eq!(decode_event(&message, owner), None);
        message.lParam = LPARAM(6);
        message.wParam = WPARAM(0);
        assert_eq!(decode_event(&message, owner), Some(UiEvent::Toggle));
        message.wParam = WPARAM(1);
        assert_eq!(decode_event(&message, owner), None);
        message.lParam = LPARAM(2);
        message.wParam = WPARAM(0xff0085dc);
        assert_eq!(
            decode_event(&message, owner),
            Some(UiEvent::Reserve(0xff0085dc))
        );
        message.wParam = WPARAM(u32::MAX as usize + 1);
        assert_eq!(decode_event(&message, owner), None);
        message.lParam = LPARAM(3);
        message.wParam = WPARAM(0);
        assert_eq!(decode_event(&message, owner), Some(UiEvent::Swap));
        message.wParam = WPARAM(1);
        assert_eq!(decode_event(&message, owner), None);
        for kind in [0, 7, -1] {
            message.lParam = LPARAM(kind);
            for payload in [0, 1, u32::MAX as usize, usize::MAX] {
                message.wParam = WPARAM(payload);
                assert_eq!(decode_event(&message, owner), None);
            }
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
                let s = panel_scale(dpi, work, MenuLayout::Panel);
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
                    let (anchor, upward) = panel_clamp(point, work, s, MenuLayout::Panel);
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
            assert!(
                String::from_utf16_lossy(&ui.state.tooltip_text.borrow()).contains("S or click")
            );
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
    fn panel_menu_native_requests_committed_selection_and_cancellation() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuRequestsRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.state.create_tooltip(instance)?;
        menu_open(&ui)?;
        let windows = thread_windows(root.0)?;
        let focus = unsafe {
            (
                GetFocus(),
                GetActiveWindow(),
                GetCapture(),
                GetForegroundWindow(),
            )
        };
        let selected = PanelGeometry::new(&ui.state).center(PanelHit::Palette(2));
        assert_eq!(ui.state.color.get(), PANEL_PALETTE[2]);
        for _ in 0..3 {
            panel_click(hwnd, selected);
        }
        assert_eq!(
            decoded(root.0),
            [
                UiEvent::Color(PANEL_PALETTE[2]),
                UiEvent::Color(PANEL_PALETTE[2]),
                UiEvent::Color(PANEL_PALETTE[2])
            ]
        );
        assert_eq!(ui.state.color.get(), PANEL_PALETTE[2]);
        assert_eq!(thread_windows(root.0)?, windows);
        ui.state.panel_tooltip(Some(PanelHit::Palette(2)));
        assert_eq!(
            String::from_utf16_lossy(&ui.state.tooltip_text.borrow()),
            "Color #F97316. Click to select, or drag onto a favorite square.\0"
        );
        let preview = PanelGeometry::new(&ui.state).center(PanelHit::Current);
        assert_eq!(
            PanelGeometry::new(&ui.state).hit(preview.0, preview.1),
            Some(PanelHit::Current)
        );
        ui.state.panel_tooltip(Some(PanelHit::Current));
        assert_eq!(
            String::from_utf16_lossy(&ui.state.tooltip_text.borrow()),
            "Selected stroke: #F97316 at 5 px.\0"
        );
        for _ in 0..3 {
            panel_click(hwnd, preview);
        }
        assert!(decoded(root.0).is_empty());
        assert_eq!(thread_windows(root.0)?, windows);
        assert_eq!(
            unsafe {
                (
                    GetFocus(),
                    GetActiveWindow(),
                    GetCapture(),
                    GetForegroundWindow(),
                )
            },
            focus
        );
        assert_eq!(
            unsafe { SendMessageW(hwnd, WM_MOUSEACTIVATE, None, None).0 },
            MA_NOACTIVATE as isize
        );
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
        for (i, &color) in PANEL_PALETTE.iter().enumerate() {
            panel_click(
                hwnd,
                PanelGeometry::new(&ui.state).center(PanelHit::Palette(i)),
            );
            assert_eq!(decoded(root.0), [UiEvent::Color(color)]);
        }
        panel_click(
            hwnd,
            PanelGeometry::new(&ui.state).center(PanelHit::Current),
        );
        assert!(decoded(root.0).is_empty());
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
        assert_eq!(thread_windows(root.0)?, windows);
        assert_eq!(
            unsafe {
                (
                    GetFocus(),
                    GetActiveWindow(),
                    GetCapture(),
                    GetForegroundWindow(),
                )
            },
            focus
        );
        for window in windows {
            assert!(!unsafe { IsWindowVisible(window).as_bool() });
        }
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
    fn panel_menu_favorites_drag_drop_swap_and_compact_reserve() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuFavoritesRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.state.create_tooltip(instance)?;
        menu_open(&ui)?;
        let g = PanelGeometry::new(&ui.state);
        let send = |message, (x, y): (i32, i32)| unsafe {
            SendMessageW(hwnd, message, None, Some(point(x, y)));
        };
        let drag = |from: usize, path: &[(i32, i32)]| {
            send(WM_LBUTTONDOWN, g.center(PanelHit::Palette(from)));
            for &step in path {
                send(WM_MOUSEMOVE, step);
            }
            send(WM_LBUTTONUP, *path.last().unwrap());
        };
        let active = g.center(PanelHit::Active);
        let reserve = g.center(PanelHit::Reserve);
        drag(3, &[reserve]);
        assert_eq!(decoded(root.0), [UiEvent::Reserve(PANEL_PALETTE[3])]);
        drag(5, &[active]);
        assert_eq!(decoded(root.0), [UiEvent::Color(PANEL_PALETTE[5])]);
        // A started drag never clicks, even when dropped back on its own swatch.
        let source = g.center(PanelHit::Palette(7));
        drag(7, &[(source.0 + 20, source.1), source]);
        assert!(decoded(root.0).is_empty());
        // Under the 4 px threshold the press is still a palette click.
        drag(7, &[(source.0 + 2, source.1)]);
        assert_eq!(decoded(root.0), [UiEvent::Color(PANEL_PALETTE[7])]);

        // The ghost follows the pointer, the target highlights and the menu stays open.
        let empty = (420, 175);
        assert_eq!(g.hit(empty.0, empty.1), None);
        send(WM_LBUTTONDOWN, g.center(PanelHit::Palette(0)));
        send(WM_MOUSEMOVE, empty);
        let ghost = (empty.1 * g.width + empty.0) as usize;
        assert_eq!(ui.state.panel_pixels.borrow()[ghost], PANEL_PALETTE[0]);
        send(WM_MOUSEMOVE, reserve);
        assert_eq!(ui.state.swatch_target(), Some(PanelHit::Reserve));
        unsafe {
            SendMessageW(hwnd, WM_MOUSELEAVE, None, None);
        }
        assert!(ui.state.expanded.get());
        unsafe {
            SendMessageW(hwnd, WM_CANCELMODE, None, None);
        }
        assert!(ui.state.swatch.get().is_none() && decoded(root.0).is_empty());
        menu_open(&ui)?;
        assert_eq!(ui.state.panel_pixels.borrow()[ghost], PANEL_BACKGROUND);

        // The reserve square and the arrow swap; the active square is only a drop target.
        panel_click(hwnd, reserve);
        panel_click(hwnd, g.center(PanelHit::Swap));
        panel_click(hwnd, active);
        assert_eq!(decoded(root.0), [UiEvent::Swap, UiEvent::Swap]);
        ui.state.panel_tooltip(Some(PanelHit::Reserve));
        assert!(String::from_utf16_lossy(&ui.state.tooltip_text.borrow()).contains("#0085DC"));

        // Compact: the reserve square sits inside the badge, filled only while drawing.
        ui.set_reserve(0xff123456)?;
        ui.state.expanded.set(false);
        ui.state.motion.set(PanelMotion::settled(false));
        for (drawing, expected) in [(true, 0xff123456), (false, PANEL_BACKGROUND)] {
            ui.update_label(drawing, "Pen", Style::default())?;
            let compact = PanelGeometry::new(&ui.state);
            let square = compact.reserve_square();
            let (x, y) = (
                (square.left + square.right) / 2,
                (square.top + square.bottom) / 2,
            );
            assert_eq!(compact.hit(x, y), Some(PanelHit::Grip));
            assert_eq!(
                ui.state.panel_pixels.borrow()[(y * compact.width + x) as usize],
                expected
            );
        }
        Ok(())
    }

    #[test]
    fn panel_menu_pin_is_expanded_only_and_has_distinct_states() -> Result<()> {
        let state = State::new(HWND::default())?;
        assert!(!state.pinned.get());
        for scale in [1.0, 1.25, 2.0] {
            state.panel_scale.set(scale);
            for upward in [false, true] {
                state.panel_upward.set(upward);
                state.expanded.set(false);
                state.motion.set(PanelMotion::settled(false));
                assert!(!PanelGeometry::new(&state).revealed(PanelHit::Pin));
                state.expanded.set(true);
                state.motion.set(PanelMotion::settled(true));
                let geometry = PanelGeometry::new(&state);
                let (x, y) = geometry.center(PanelHit::Pin);
                assert_eq!(geometry.hit(x, y), Some(PanelHit::Pin));
                let toggle = geometry.bounds(PanelHit::Toggle);
                let pin = geometry.bounds(PanelHit::Pin);
                assert!(toggle.right <= pin.left && pin.right <= geometry.badge().left);
                state.pinned.set(false);
                let (normal, _) = followup_preview(&state, "pin-off")?;
                state.pinned.set(true);
                let (pinned, _) = followup_preview(&state, "pin-on")?;
                assert!(normal.iter().zip(&pinned).enumerate().any(|(i, (a, b))| {
                    let x = i as i32 % geometry.width;
                    let y = i as i32 / geometry.width;
                    a != b && x >= pin.left && x < pin.right && y >= pin.top && y < pin.bottom
                }));
            }
        }
        Ok(())
    }

    #[test]
    fn panel_menu_pin_keeps_open_without_timers_and_preserves_hide_show() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "PinRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.state.create_tooltip(instance)?;
        menu_open(&ui)?;
        let pin = PanelGeometry::new(&ui.state).center(PanelHit::Pin);
        let focus = unsafe { GetFocus() };
        ui.state
            .begin_panel_gesture(Some(PanelHit::Pin), POINT::default());
        ui.state
            .finish_panel_gesture(None, POINT { x: pin.0, y: pin.1 });
        assert!(
            !ui.state.pinned.get(),
            "Releasing outside must not toggle the pin"
        );
        panel_click(hwnd, pin);
        assert!(ui.state.pinned.get());
        ui.state.panel_tooltip(Some(PanelHit::Pin));
        assert!(String::from_utf16_lossy(&ui.state.tooltip_text.borrow()).contains("Unpin menu"));
        ui.state.tracking.set(false);
        unsafe {
            SendMessageW(hwnd, WM_MOUSELEAVE, None, None);
        }
        ui.state.finish_circle_leave(POINT {
            x: -10000,
            y: -10000,
        });
        ui.state.start_panel_reveal(false, false);
        assert!(ui.state.expanded.get());
        assert_eq!(ui.state.motion.get().progress, 1.0);
        assert!(!ui.state.animation_timer.get() && !ui.state.leave_timer.get());
        ui.update_label(true, "Temporary pen", Style::default())?;
        assert!(ui.state.expanded.get() && ui.state.pinned.get());
        ui.state
            .begin_panel_gesture(Some(PanelHit::Width), POINT::default());
        ui.state.panel_leave();
        assert_eq!(ui.state.pressed.get(), Some(PanelHit::Width));
        ui.state.cancel_panel_gesture();
        ui.set_visible(false)?;
        assert!(ui.state.pinned.get() && !ui.state.expanded.get());
        ui.set_visible(true)?;
        assert!(ui.state.expanded.get() && ui.state.pinned.get());
        ui.set_visible(false)?;
        menu_open(&ui)?;
        panel_click(hwnd, PanelGeometry::new(&ui.state).center(PanelHit::Pin));
        assert!(!ui.state.pinned.get());
        ui.state.panel_tooltip(Some(PanelHit::Pin));
        assert!(String::from_utf16_lossy(&ui.state.tooltip_text.borrow()).contains("Pin menu"));
        ui.state.start_panel_reveal(false, false);
        assert!(!ui.state.expanded.get());
        assert!(
            decoded(root.0).is_empty(),
            "Pinning must not send drawing commands"
        );
        assert_eq!(unsafe { GetFocus() }, focus);
        Ok(())
    }

    #[test]
    fn panel_menu_hide_collapses_and_show_restores() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "MenuHideRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.set_visible(true)?;
        menu_open(&ui)?;
        ui.set_visible(false)?;
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        assert!(!ui.state.expanded.get() && !ui.state.tracking.get());
        let g = PanelGeometry::new(&ui.state);
        assert_eq!(g.body.right - g.body.left, PANEL_ROW);
        ui.set_visible(true)?;
        assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());
        assert!(decoded(root.0).is_empty());
        Ok(())
    }

    #[test]
    fn panel_layout_rail_targets_fit_hit_and_slide_vertically() -> Result<()> {
        let state = State::new(HWND::default())?;
        state.layout.set(MenuLayout::Rail);
        for s in [1.0, 1.5, 2.0] {
            state.panel_scale.set(s);
            for upward in [false, true] {
                state.panel_upward.set(upward);
                state.motion.set(PanelMotion::settled(false));
                let compact = PanelGeometry::new(&state);
                assert_eq!((compact.width, compact.height), {
                    state.layout.set(MenuLayout::Panel);
                    let panel = PanelGeometry::new(&state);
                    state.layout.set(MenuLayout::Rail);
                    (panel.width, panel.height)
                });
                state.motion.set(PanelMotion::settled(true));
                let g = PanelGeometry::new(&state);
                assert_eq!((g.width, g.height), (scaled(44, s), scaled(656, s)));
                let targets: Vec<_> = [
                    PanelHit::Grip,
                    PanelHit::Pin,
                    PanelHit::Toggle,
                    PanelHit::Active,
                    PanelHit::Reserve,
                    PanelHit::Width,
                ]
                .into_iter()
                .chain((0..16).map(PanelHit::Palette))
                .chain(PANEL_TOOLS.map(|entry| PanelHit::Tool(entry.0)))
                .collect();
                for target in targets {
                    let r = g.bounds(target);
                    assert!(
                        r.left >= 0 && r.top >= 0 && r.right <= g.width && r.bottom <= g.height,
                        "{target:?} at {s} upward={upward}"
                    );
                    let (x, y) = g.center(target);
                    assert_eq!(g.hit(x, y), Some(target), "{target:?} at {s}");
                }
                for y in 0..g.height {
                    for x in 0..g.width {
                        assert!(!matches!(
                            g.hit(x, y),
                            Some(PanelHit::Swap | PanelHit::Current)
                        ));
                    }
                }
                // The badge stays at the rail's top, or bottom when opening upward.
                let badge = g.badge();
                assert_eq!(badge.top == 0, !upward);
                let slider = g.bounds(PanelHit::Width);
                assert_eq!(g.requested_width(0, slider.bottom + 100), 1);
                assert_eq!(g.requested_width(0, slider.top - 100), 31);
                let mut previous = 31;
                for y in slider.top..slider.bottom {
                    let width = g.requested_width(0, y);
                    assert!((1..=31).contains(&width) && width % 2 == 1);
                    assert!(width <= previous);
                    previous = width;
                }
            }
        }
        Ok(())
    }

    #[test]
    fn panel_layout_ring_sectors_hit_and_window_clamps_to_work_area() -> Result<()> {
        let state = State::new(HWND::default())?;
        state.layout.set(MenuLayout::Radial);
        let work = rect(-1600, 0, 1600, 900);
        state.radial_work.set(work);
        for s in [1.0, 1.25, 2.0] {
            state.panel_scale.set(s);
            // The badge never animates into the ring; compact stays the plain badge.
            state.motion.set(PanelMotion::settled(false));
            assert!(!PanelGeometry::new(&state).ring);
            state.motion.set(PanelMotion::settled(true));
            for center in [
                POINT { x: -800, y: 450 },
                POINT { x: -1600, y: 0 },
                POINT { x: 5000, y: 5000 },
            ] {
                state.radial_center.set(center);
                let g = PanelGeometry::new(&state);
                let side = scaled(RING_SIZE, s);
                assert!(g.ring && (g.width, g.height) == (side, side));
                let origin = g.destination(state.panel_anchor.get());
                assert!(origin.x >= work.left && origin.x + side <= work.right);
                assert!(origin.y >= work.top && origin.y + side <= work.bottom);
                if center.x == -800 {
                    assert_eq!(origin.x + side / 2, center.x);
                    assert_eq!(origin.y + side / 2, center.y);
                }
                let (cx, cy) = g.ring_center();
                assert_eq!(g.hit(cx, cy), Some(PanelHit::Swap));
                for target in (0..16)
                    .map(PanelHit::Palette)
                    .chain(PANEL_TOOLS.map(|entry| PanelHit::Tool(entry.0)))
                {
                    let (x, y) = g.center(target);
                    assert_eq!(g.hit(x, y), Some(target), "{target:?} at {s}");
                }
                // Pen and the first color sit at the top; order runs clockwise.
                assert!(g.center(PanelHit::Tool(MenuTool::Pen)).1 < cy);
                assert!(g.center(PanelHit::Palette(4)).0 > cx);
                // The top corners hold collapse (left) and grip (right) buttons.
                let (bx, by) = g.collapse_center();
                assert_eq!(g.hit(bx, by), Some(PanelHit::Collapse));
                assert!(bx < cx && g.ring_grip_center().0 > cx);
                assert!(g.hit(0, side - 1).is_none() && g.hit(side - 1, side - 1).is_none());
            }
        }
        Ok(())
    }

    #[test]
    fn panel_layout_ring_picks_close_and_hidden_menu_hides_again() -> Result<()> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let class = WindowClass::new(instance, "LayoutRingRoot", Some(root_proc))?;
        let root = HiddenRoot(create_hidden(&class, HWND::default(), None, 1, 1)?);
        let ui = hidden_ui(root.0)?;
        let hwnd = status(&ui)?;
        ui.set_visible(true)?;
        menu_open(&ui)?;
        ui.state.pinned.set(true);
        // Rail keeps a pinned menu open; the ring has no pin and starts compact.
        ui.set_layout(MenuLayout::Rail)?;
        assert!(ui.state.expanded.get());
        let rail = panel_preview(&ui, "layout-rail-100")?;
        let g = PanelGeometry::new(&ui.state);
        for (i, color) in PANEL_PALETTE.iter().enumerate() {
            let (x, y) = g.center(PanelHit::Palette(i));
            assert_eq!(rail[(y * g.width + x) as usize], *color);
        }
        ui.set_layout(MenuLayout::Radial)?;
        assert!(!ui.state.pinned.get() && !ui.state.expanded.get());
        assert!(!PanelGeometry::new(&ui.state).ring);
        // Hover alone never opens the ring.
        ui.state.reveal_panel(true);
        assert!(!ui.state.expanded.get());
        ui.set_visible(false)?;
        let open = |center: POINT| -> Result<()> {
            ui.state.radial_work.set(ui.state.panel_monitor.get());
            ui.state.radial_center.set(center);
            ui.state.expanded.set(true);
            ui.state.motion.set(PanelMotion::settled(true));
            ui.state.resize_panel()?;
            // Returns the previous visibility, not success.
            let _ = unsafe { ShowWindow(hwnd, SW_SHOWNA) };
            Ok(())
        };
        let work = ui.state.panel_monitor.get();
        let middle = POINT {
            x: (work.left + work.right) / 2,
            y: (work.top + work.bottom) / 2,
        };
        open(middle)?;
        let ring = panel_preview(&ui, "layout-ring-100")?;
        let g = PanelGeometry::new(&ui.state);
        assert_eq!(ring[0], 0, "corners pass clicks through");
        for (i, color) in PANEL_PALETTE.iter().enumerate() {
            let (x, y) = g.center(PanelHit::Palette(i));
            assert_eq!(ring[(y * g.width + x) as usize], *color);
        }
        // Picks, swaps and pointer leave keep the ring open.
        panel_click(hwnd, g.center(PanelHit::Tool(MenuTool::Ellipse)));
        assert_eq!(decoded(root.0), [UiEvent::Tool(MenuTool::Ellipse)]);
        let (cx, cy) = g.ring_center();
        panel_click(hwnd, (cx, cy));
        assert_eq!(decoded(root.0), [UiEvent::Swap]);
        ui.state.panel_leave();
        assert!(ui.state.expanded.get());
        // The top-right grip drags the open ring; it never reopens or collapses it.
        assert_eq!(
            g.hit(g.ring_grip_center().0, g.ring_grip_center().1),
            Some(PanelHit::Grip)
        );
        let (start, anchor) = (ui.state.radial_center.get(), ui.state.panel_anchor.get());
        ui.state
            .begin_panel_gesture(Some(PanelHit::Grip), POINT::default());
        ui.state.move_panel_grip(POINT { x: -60, y: 40 });
        let moved = POINT {
            x: start.x - 60,
            y: start.y + 40,
        };
        assert_eq!(ui.state.radial_center.get(), moved);
        ui.state.cancel_panel_gesture();
        assert!(ui.state.expanded.get() && PanelGeometry::new(&ui.state).ring);
        assert_eq!(ui.state.panel_anchor.get(), anchor, "the badge stays put");
        // Only the collapse button returns to the badge; a hidden menu hides again.
        panel_click(hwnd, g.collapse_center());
        assert!(decoded(root.0).is_empty());
        assert!(!ui.state.expanded.get());
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        // The real summon path opens regardless of where the pointer is.
        ui.set_visible(true)?;
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor)? };
        ui.state.open_radial(cursor);
        assert!(ui.state.expanded.get());
        ui.state.reveal_panel(false);
        assert!(unsafe { IsWindowVisible(hwnd) }.as_bool());
        // Other layouts ignore summon requests.
        ui.set_layout(MenuLayout::Panel)?;
        ui.state.open_radial(cursor);
        assert!(!PanelGeometry::new(&ui.state).ring);
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
        // Exclude the rounded join, the brush-tip overlap and the compact reserve square.
        for y in 8..31 {
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
    fn hidden_native_panel_updates_events_and_lifetimes() -> Result<()> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let root_class = WindowClass::new(instance, "TestRoot", Some(root_proc))?;
            let root = HiddenRoot(create_hidden(&root_class, HWND::default(), None, 1, 1)?);
            let ui = hidden_ui(root.0)?;
            let hwnd = status(&ui)?;
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
            let name = ui.status_class.name.clone();
            // Queued events contain values only and survive releasing State/classes.
            ui.state.post(UiEvent::Color(0xff010203));
            let messages = events(root.0);
            assert_eq!(
                (messages[0].wParam.0, messages[0].lParam.0),
                (0xff010203, 1)
            );
            assert_eq!(ui.event(&messages[0]), Some(UiEvent::Color(0xff010203)));
            ui.state.post(UiEvent::Color(0xff010203));
            drop(ui);
            assert!(!IsWindow(Some(hwnd)).as_bool());
            assert_eq!(decoded(root.0), [UiEvent::Color(0xff010203)]);
            let mut info = WNDCLASSW::default();
            assert!(GetClassInfoW(Some(instance), PCWSTR(name.as_ptr()), &mut info).is_err());
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
}
