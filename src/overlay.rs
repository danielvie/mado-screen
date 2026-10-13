use windows::{
    Win32::{Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*},
    core::{Error, Result, w},
};

pub struct Overlay {
    hwnd: HWND,
    instance: HINSTANCE,
    width: i32,
    height: i32,
}

impl Overlay {
    pub fn new() -> Result<Self> {
        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN);
            let height = GetSystemMetrics(SM_CYSCREEN);
            if width <= 0 || height <= 0 {
                return Err(Error::new(
                    E_FAIL,
                    "Primary display has no usable dimensions",
                ));
            }
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
                0,
                0,
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

    pub fn show(&self) -> Result<()> {
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
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
