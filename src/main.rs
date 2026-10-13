mod drawing;

use std::{ffi::c_void, io::Write, mem::size_of};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::{Error, Result, w},
};

const TOGGLE: i32 = 1;
const CLEAR: i32 = 2;
const QUIT: i32 = 3;

fn main() -> Result<()> {
    // Must precede windows and DPI-dependent metrics; see tasks/plan.md sources.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let _hotkeys = Hotkeys::register()?;
    let mut overlay = Overlay::new()?;
    println!("PoC 1: automatic orange line and outline rectangle.");
    println!("Ctrl+Alt+D: toggle input capture; Ctrl+Alt+C: clear; Ctrl+Alt+Q: quit.");
    println!(
        "Primary display: {} x {} physical pixels.",
        overlay.width, overlay.height
    );
    println!("Mode: click-through. No interactive drawing.");

    // GetMessage blocks when idle; there is no frame timer or polling loop.
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        match result {
            -1 => return Err(Error::from_thread()),
            0 => break,
            _ => {}
        }
        if message.message == WM_HOTKEY {
            match message.wParam.0 as i32 {
                TOGGLE => overlay.toggle_input()?,
                CLEAR => {
                    overlay.cleared = true;
                    overlay.render()?;
                    println!("Shapes cleared. Restart to restore them.");
                }
                QUIT => break,
                _ => {}
            }
        } else {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    println!("Overlay closed.");
    Ok(())
}

struct Hotkeys(Vec<i32>);

impl Hotkeys {
    fn register() -> Result<Self> {
        let mut hotkeys = Self(Vec::new());
        for (id, key) in [(TOGGLE, b'D'), (CLEAR, b'C'), (QUIT, b'Q')] {
            // Register before showing; partial registration is undone by Drop.
            unsafe { RegisterHotKey(None, id, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, key as u32) }
                .map_err(|error| Error::new(error.code(), format!(
                    "Could not register Ctrl+Alt+{}. Close the conflicting application and retry.",
                    key as char
                )))?;
            hotkeys.0.push(id);
        }
        Ok(hotkeys)
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        for &id in &self.0 {
            unsafe {
                let _ = UnregisterHotKey(None, id);
            }
        }
    }
}

// Own the native resources so startup/render failures also remove the overlay.
struct Overlay {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    previous_bitmap: HGDIOBJ,
    bits: *mut c_void,
    width: i32,
    height: i32,
    capture: bool,
    cleared: bool,
}

impl Overlay {
    fn new() -> Result<Self> {
        unsafe {
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
            let mut overlay = Self {
                hwnd: HWND::default(),
                dc: HDC::default(),
                bitmap: HBITMAP::default(),
                previous_bitmap: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                width: GetSystemMetrics(SM_CXSCREEN),
                height: GetSystemMetrics(SM_CYSCREEN),
                capture: false,
                cleared: false,
            };
            if overlay.width <= 0 || overlay.height <= 0 {
                return Err(Error::new(
                    E_FAIL,
                    "Primary display has no usable dimensions",
                ));
            }
            overlay.hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!("Mado Screen PoC 1"),
                WS_POPUP,
                0,
                0,
                overlay.width,
                overlay.height,
                None,
                None,
                Some(instance),
                None,
            )?;
            overlay.dc = CreateCompatibleDC(None);
            if overlay.dc.is_invalid() {
                return Err(Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: overlay.width,
                    biHeight: -overlay.height, // Top-down pixels match screen coordinates.
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            overlay.bitmap = CreateDIBSection(
                Some(overlay.dc),
                &info,
                DIB_RGB_COLORS,
                &mut overlay.bits,
                None,
                0,
            )?;
            overlay.previous_bitmap = SelectObject(overlay.dc, HGDIOBJ(overlay.bitmap.0));
            if overlay.previous_bitmap.is_invalid() || overlay.bits.is_null() {
                return Err(Error::new(E_FAIL, "Could not select the overlay bitmap"));
            }
            overlay.render()?;
            SetWindowPos(
                overlay.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                overlay.width,
                overlay.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
            Ok(overlay)
        }
    }

    fn render(&mut self) -> Result<()> {
        unsafe {
            // This DIB owns width*height aligned u32 pixels until Overlay::drop.
            let pixels = std::slice::from_raw_parts_mut(
                self.bits.cast::<u32>(),
                self.width as usize * self.height as usize,
            );
            drawing::render(pixels, self.width as usize, self.capture, self.cleared);
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let origin = POINT::default();
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow
            UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&origin),
                Some(&size),
                Some(self.dc),
                Some(&origin),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    fn toggle_input(&mut self) -> Result<()> {
        self.capture = !self.capture;
        unsafe {
            let current = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let transparent = WS_EX_TRANSPARENT.0 as isize;
            let style = if self.capture {
                current & !transparent
            } else {
                current | transparent
            };
            // A zero return can mean success; last error disambiguates it.
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
            )?;
        }
        self.render()?;
        println!(
            "Mode: {}.",
            if self.capture {
                "input capture, mouse clicks logged only; background alpha 1/255"
            } else {
                "click-through"
            }
        );
        Ok(())
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_invalid() {
                let _ = DestroyWindow(self.hwnd);
            }
            if !self.previous_bitmap.is_invalid() {
                SelectObject(self.dc, self.previous_bitmap);
            }
            if !self.bitmap.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            }
            if !self.dc.is_invalid() {
                let _ = DeleteDC(self.dc);
            }
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEWHEEL => {
            // Signed 16-bit coordinates; wheel messages use screen, clicks use client coordinates.
            let x = l.0 as i16;
            let y = (l.0 >> 16) as i16;
            let action = match message {
                WM_LBUTTONDOWN => "left down, client",
                WM_LBUTTONUP => "left up, client",
                _ => "wheel, screen",
            };
            // Never panic across the Win32 callback if the console pipe closes.
            let _ = writeln!(std::io::stdout(), "Mouse {action}: ({x}, {y})");
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}
