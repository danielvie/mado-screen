use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::{
            GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromPoint,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{Magnification::*, WindowsAndMessaging::*},
    },
    core::{Error, Result, w},
};

const TIMER_ID: usize = 3;
const HOST_STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_POPUP.0 | WS_BORDER.0 | WS_CLIPCHILDREN.0);
const HOST_EX_STYLE: WINDOW_EX_STYLE = WINDOW_EX_STYLE(
    WS_EX_TOPMOST.0
        | WS_EX_LAYERED.0
        | WS_EX_TRANSPARENT.0
        | WS_EX_NOACTIVATE.0
        | WS_EX_TOOLWINDOW.0,
);

// The control captures the live desktop without capturing itself. Its layered,
// transparent host passes mouse input through without taking keyboard focus.
// https://learn.microsoft.com/en-us/windows/win32/winauto/magapi/magapi-intro
pub struct Zoom {
    host: HWND,
    control: HWND,
    owner: HWND,
    timer: bool,
    factor: f32,
    size_scale: f32,
}

impl Zoom {
    pub fn new(owner: HWND) -> Result<Self> {
        if owner.is_invalid() {
            return Err(Error::new(E_INVALIDARG, "Zoom requires an owner window"));
        }
        unsafe {
            MagInitialize().ok()?;
            // Own initialization before any fallible work, including window creation.
            let mut zoom = Self {
                host: HWND::default(),
                control: HWND::default(),
                owner,
                timer: false,
                factor: 2.0,
                size_scale: 1.0,
            };
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            zoom.host = CreateWindowExW(
                HOST_EX_STYLE,
                w!("STATIC"),
                w!("Mado zoom"),
                HOST_STYLE,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )?;
            SetLayeredWindowAttributes(zoom.host, COLORREF(0), 255, LWA_ALPHA)?;
            zoom.control = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                WC_MAGNIFIER,
                w!(""),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                1,
                1,
                Some(zoom.host),
                None,
                Some(instance),
                None,
            )?;
            // Explicitly exclude the host as well as the automatically excluded control.
            MagSetWindowFilterList(zoom.control, MW_FILTERMODE_EXCLUDE, 1, &mut zoom.host).ok()?;
            zoom.set_factor(2.0)?;
            if SetTimer(Some(owner), TIMER_ID, 16, None) == 0 {
                return Err(Error::from_thread());
            }
            zoom.timer = true;
            Ok(zoom)
        }
    }

    pub fn is_tick(message: &MSG, owner: HWND) -> bool {
        message.hwnd == owner && message.message == WM_TIMER && message.wParam.0 == TIMER_ID
    }

    pub fn scroll(&mut self, steps: i32) -> Result<()> {
        self.set_factor(next_factor(self.factor, steps))?;
        self.update()
    }

    pub fn resize(&mut self, steps: i32) -> Result<()> {
        self.size_scale = next_size(self.size_scale, steps);
        self.update()
    }

    fn set_factor(&mut self, factor: f32) -> Result<()> {
        let mut transform = MAGTRANSFORM {
            v: [factor, 0.0, 0.0, 0.0, factor, 0.0, 0.0, 0.0, 1.0],
        };
        unsafe { MagSetWindowTransform(self.control, &mut transform).ok()? };
        self.factor = factor;
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        unsafe {
            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor)?;
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(
                MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .ok()?;
            let (width, height, source) =
                geometry(cursor, monitor.rcMonitor, self.factor, self.size_scale);
            let mut frame = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            AdjustWindowRectEx(&mut frame, HOST_STYLE, false, HOST_EX_STYLE)?;
            SetWindowPos(
                self.control,
                None,
                0,
                0,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )?;
            MagSetWindowSource(self.control, source).ok()?;
            SetWindowPos(
                self.host,
                Some(HWND_TOPMOST),
                cursor.x - width / 2 + frame.left,
                cursor.y - height / 2 + frame.top,
                frame.right - frame.left,
                frame.bottom - frame.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
            // Refresh even with a stationary cursor, so videos and app changes stay live.
            let _ = InvalidateRect(Some(self.control), None, true);
            Ok(())
        }
    }
}

impl Drop for Zoom {
    fn drop(&mut self) {
        unsafe {
            if self.timer {
                let _ = KillTimer(Some(self.owner), TIMER_ID);
            }
            if !self.host.is_invalid() {
                let _ = DestroyWindow(self.host); // Also destroys the child control.
            }
            let _ = MagUninitialize();
        }
    }
}

fn next_factor(factor: f32, steps: i32) -> f32 {
    (factor * 1.25_f32.powi(steps.clamp(-32, 32))).clamp(1.0, 8.0)
}

fn next_size(size_scale: f32, steps: i32) -> f32 {
    (size_scale * 1.25_f32.powi(steps.clamp(-32, 32))).clamp(0.25, 2.0)
}

fn geometry(cursor: POINT, monitor: RECT, factor: f32, size_scale: f32) -> (i32, i32, RECT) {
    let mw = (monitor.right - monitor.left).max(1);
    let mh = (monitor.bottom - monitor.top).max(1);
    // Start at one third of the monitor, capped at 640 pixels on its longer axis.
    // Resize both axes together without changing the magnification transform.
    let scale = (1.0_f64 / 3.0).min(640.0 / mw.max(mh) as f64) * f64::from(size_scale);
    let width = (mw as f64 * scale).round().max(1.0) as i32;
    let height = (mh as f64 * scale).round().max(1.0) as i32;
    let sw = (width as f32 / factor).round().max(1.0) as i32;
    let sh = (height as f32 / factor).round().max(1.0) as i32;
    // Keep focus at the cursor even at an edge, rather than shifting the source.
    let left = cursor.x - sw / 2;
    let top = cursor.y - sh / 2;
    (
        width,
        height,
        RECT {
            left,
            top,
            right: left + sw,
            bottom: top + sh,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_hidden_native_control_transform_exclusion_and_cleanup() -> Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
        let owner = crate::overlay::Overlay::new()?;
        let focus = unsafe { GetFocus() };
        assert!(Zoom::new(HWND::default()).is_err());
        // No update(): initialization and teardown do not show either window.
        for _ in 0..3 {
            let mut zoom = Zoom::new(owner.hwnd())?;
            let host = zoom.host;
            let control = zoom.control;
            unsafe {
                assert!(!IsWindowVisible(host).as_bool());
                assert_eq!(
                    GetWindowLongPtrW(host, GWL_EXSTYLE) as u32 & HOST_EX_STYLE.0,
                    HOST_EX_STYLE.0,
                );
                let mut transform = MAGTRANSFORM::default();
                MagGetWindowTransform(control, &mut transform).ok()?;
                assert_eq!(transform.v[0], 2.0);
                zoom.set_factor(4.0)?;
                MagGetWindowTransform(control, &mut transform).ok()?;
                assert_eq!(transform.v, [4.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 1.0]);
                let mut mode = MW_FILTERMODE::default();
                let mut excluded = HWND::default();
                assert_eq!(
                    MagGetWindowFilterList(control, &mut mode, 1, &mut excluded),
                    1
                );
                assert_eq!(mode, MW_FILTERMODE_EXCLUDE);
                assert_eq!(excluded, host);
                // Other desktop apps may change foreground during this test.
                // Only our own windows must never acquire it.
                assert_ne!(GetForegroundWindow(), host);
                assert_ne!(GetForegroundWindow(), control);
                drop(zoom);
                assert!(!IsWindow(Some(host)).as_bool());
                assert!(!IsWindow(Some(control)).as_bool());
                assert!(KillTimer(Some(owner.hwnd()), TIMER_ID).is_err());
                assert_eq!(GetFocus(), focus);
            }
        }
        Ok(())
    }

    #[test]
    fn zoom_ticks_match_only_the_owned_timer() {
        let owner = HWND(std::ptr::dangling_mut());
        let mut message = MSG {
            hwnd: owner,
            message: WM_TIMER,
            wParam: WPARAM(TIMER_ID),
            ..Default::default()
        };
        assert!(Zoom::is_tick(&message, owner));
        assert!(!Zoom::is_tick(&message, HWND::default()));
        message.wParam = WPARAM(2);
        assert!(!Zoom::is_tick(&message, owner));
        message.wParam = WPARAM(TIMER_ID);
        message.message = WM_MOUSEMOVE;
        assert!(!Zoom::is_tick(&message, owner));
    }

    #[test]
    fn zoom_geometry_follows_monitor_aspect_and_cursor_at_edges() {
        for (mw, mh) in [(1920, 1080), (3440, 1440), (1080, 1920), (3840, 2160)] {
            let monitor = RECT {
                left: -mw,
                top: -mh,
                right: 0,
                bottom: 0,
            };
            for cursor in [POINT { x: -mw, y: -mh }, POINT { x: -1, y: -1 }] {
                for factor in [1.0, 2.0, 8.0] {
                    let (w, h, source) = geometry(cursor, monitor, factor, 1.0);
                    assert!(w <= 640 && h <= 640);
                    assert!((w as f64 / h as f64 - mw as f64 / mh as f64).abs() < 0.01);
                    assert_eq!(source.left + (source.right - source.left) / 2, cursor.x);
                    assert_eq!(source.top + (source.bottom - source.top) / 2, cursor.y);
                    assert!(
                        ((source.right - source.left) as f32 * factor - w as f32).abs() <= factor
                    );
                    assert!(
                        ((source.bottom - source.top) as f32 * factor - h as f32).abs() <= factor
                    );
                }
            }
        }
    }

    #[test]
    fn zoom_resize_preserves_aspect_focus_and_magnification() {
        for (mw, mh) in [(1920, 1080), (1080, 1920), (3440, 1440), (3840, 2160)] {
            let monitor = RECT {
                left: -mw,
                top: -mh,
                right: 0,
                bottom: 0,
            };
            let cursor = POINT { x: -mw, y: -mh };
            for factor in [1.0, 2.0, 8.0] {
                let (base_w, base_h, _) = geometry(cursor, monitor, factor, 1.0);
                for size in [0.25, 0.8, 1.0, 1.25, 2.0] {
                    let (w, h, source) = geometry(cursor, monitor, factor, size);
                    assert!((w as f32 - base_w as f32 * size).abs() <= 1.0);
                    assert!((h as f32 - base_h as f32 * size).abs() <= 1.0);
                    // Aspect differs by at most one rounded pixel on either axis.
                    assert!((w * mh - h * mw).abs() <= mw + mh);
                    assert_eq!(source.left + (source.right - source.left) / 2, cursor.x);
                    assert_eq!(source.top + (source.bottom - source.top) / 2, cursor.y);
                    assert!(
                        ((source.right - source.left) as f32 * factor - w as f32).abs() <= factor
                    );
                    assert!(
                        ((source.bottom - source.top) as f32 * factor - h as f32).abs() <= factor
                    );
                }
            }
        }
    }

    #[test]
    fn zoom_resize_steps_are_reversible_and_bounded() {
        assert_eq!(next_size(1.0, 0), 1.0);
        assert_eq!(next_size(1.0, 1), 1.25);
        assert_eq!(next_size(1.25, -1), 1.0);
        assert_eq!(next_size(1.0, i32::MAX), 2.0);
        assert_eq!(next_size(1.0, i32::MIN), 0.25);
        assert_eq!(next_size(2.0, 1), 2.0);
        assert_eq!(next_size(0.25, -1), 0.25);
    }

    #[test]
    fn zoom_steps_are_reversible_and_bounded() {
        assert_eq!(next_factor(2.0, 0), 2.0);
        assert_eq!(next_factor(2.0, 1), 2.5);
        assert_eq!(next_factor(2.5, -1), 2.0);
        assert_eq!(next_factor(2.0, i32::MAX), 8.0);
        assert_eq!(next_factor(2.0, i32::MIN), 1.0);
    }
}
