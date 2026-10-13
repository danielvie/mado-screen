use windows::{
    Win32::{Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*},
    core::{Error, Result, w},
};

pub struct Overlay {
    hwnd: HWND,
    instance: HINSTANCE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Overlay {
    pub fn new() -> Result<Self> {
        unsafe {
            // The virtual desktop origin can be negative; drawing stays canvas-local.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            validate_bounds(x, y, width, height)?;
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_CROSS)?,
                lpszClassName: w!("MadoScreenPoc1"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(Error::from_thread());
            }
            // Own the class before creation so a failed creation also unregisters it.
            let mut overlay = Self {
                hwnd: HWND::default(),
                instance,
                x,
                y,
                width,
                height,
            };
            overlay.hwnd = CreateWindowExW(
                // Set topmost before the first bitmap upload, not only when showing.
                WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!("Mado Screen PoC 1"),
                WS_POPUP, // Hidden until show(), after rendering and shortcut registration.
                x,
                y,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )?;
            Ok(overlay)
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub fn origin(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub fn show(&self) -> Result<()> {
        let (x, y) = self.origin();
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                self.width,
                self.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    }

    pub fn set_capture(&self, capture: bool) -> Result<()> {
        unsafe {
            let current = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let style = capture_style(current, capture);
            // A zero return can mean success; last error disambiguates it.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw
            SetLastError(ERROR_SUCCESS);
            if SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style) == 0
                && GetLastError() != ERROR_SUCCESS
            {
                return Err(Error::from_thread());
            }
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            )
        }
    }
}

fn validate_bounds(x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    // Mouse messages use signed 16-bit client coordinates.
    // https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousemove
    let dimensions = 1..=i16::MAX as i32;
    if !dimensions.contains(&width) || !dimensions.contains(&height) {
        return Err(Error::new(
            E_FAIL,
            "Virtual desktop dimensions must be 1..=32767 pixels per axis for signed client mouse coordinates",
        ));
    }
    if x.checked_add(width).is_none() || y.checked_add(height).is_none() {
        return Err(Error::new(
            E_FAIL,
            "Virtual desktop origin plus dimensions overflows screen coordinates",
        ));
    }
    Ok(())
}

fn capture_style(current: isize, capture: bool) -> isize {
    let transparent = WS_EX_TRANSPARENT.0 as isize;
    if capture {
        current & !transparent
    } else {
        current | transparent
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            // Never unregister a class while its window remains alive.
            if self.hwnd.is_invalid() || DestroyWindow(self.hwnd).is_ok() {
                let _ = UnregisterClassW(w!("MadoScreenPoc1"), Some(self.instance));
            }
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_CLOSE => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_bounds_accept_negative_left_above_and_positive_origins() {
        for (x, y, width, height) in [
            (-1920, 0, 3840, 1080),
            (0, -1080, 1920, 2160),
            (-1920, -1080, 3840, 2160),
            (100, 200, 1920, 1080),
            (0, 0, 1, 1),
            (0, 0, 32767, 32767),
            (i32::MIN, i32::MIN, 1, 1),
            (i32::MAX - 1, i32::MAX - 1, 1, 1),
        ] {
            assert!(validate_bounds(x, y, width, height).is_ok());
        }
    }

    #[test]
    fn virtual_bounds_reject_unusable_and_oversized_dimensions() {
        for invalid in [i32::MIN, -1, 0, 32768, i32::MAX] {
            for (width, height) in [(invalid, 1080), (1920, invalid)] {
                let error = validate_bounds(0, 0, width, height).unwrap_err();
                assert!(error.message().contains("1..=32767"));
                assert!(error.message().contains("signed client mouse coordinates"));
            }
        }
    }

    #[test]
    fn virtual_bounds_reject_origin_plus_dimension_overflow() {
        for (x, y, width, height) in [
            (i32::MAX, 0, 1, 1),
            (0, i32::MAX, 1, 1),
            (i32::MAX - 32766, 0, 32767, 1),
            (0, i32::MAX - 32766, 1, 32767),
        ] {
            let error = validate_bounds(x, y, width, height).unwrap_err();
            assert!(error.message().contains("overflows screen coordinates"));
        }
    }

    #[test]
    fn capture_changes_only_transparency_and_is_idempotent() {
        let transparent = WS_EX_TRANSPARENT.0 as isize;
        let initial = (WS_EX_TOPMOST
            | WS_EX_LAYERED
            | WS_EX_TRANSPARENT
            | WS_EX_NOACTIVATE
            | WS_EX_TOOLWINDOW)
            .0 as isize;
        for current in [0, initial, -1, isize::MIN, isize::MAX] {
            for capture in [false, true] {
                let style = capture_style(current, capture);
                assert_eq!(style & !transparent, current & !transparent);
                assert_eq!(style & transparent == 0, capture);
                assert_eq!(capture_style(style, capture), style);
            }
        }
    }

    #[test]
    fn mouse_activation_is_rejected_without_creating_a_window() {
        let result = unsafe {
            window_proc(
                HWND::default(),
                WM_MOUSEACTIVATE,
                WPARAM::default(),
                LPARAM::default(),
            )
        };
        assert_eq!(result.0, MA_NOACTIVATE as isize);
    }
}
