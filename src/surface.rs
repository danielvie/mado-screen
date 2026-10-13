use std::{ffi::c_void, mem::size_of};
use windows::{
    Win32::{
        Foundation::{COLORREF, E_FAIL, E_INVALIDARG, HWND, POINT, SIZE},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow},
    },
    core::{Error, Result},
};

pub struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous_bitmap: HGDIOBJ,
    bits: *mut c_void,
    width: i32,
    height: i32,
}

impl Surface {
    pub fn new(width: i32, height: i32) -> Result<Self> {
        // Bound bytes to i32 so slice lengths and drawing coordinate arithmetic stay in range.
        if width <= 0
            || height <= 0
            || width
                .checked_mul(height)
                .and_then(|pixels| pixels.checked_mul(size_of::<u32>() as i32))
                .is_none()
        {
            return Err(Error::new(E_INVALIDARG, "Invalid surface dimensions"));
        }
        let mut surface = Self {
            dc: HDC::default(),
            bitmap: HBITMAP::default(),
            previous_bitmap: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
            width,
            height,
        };
        unsafe {
            surface.dc = CreateCompatibleDC(None);
            if surface.dc.is_invalid() {
                return Err(Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // Top-down pixels match screen coordinates.
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            surface.bitmap = CreateDIBSection(
                Some(surface.dc),
                &info,
                DIB_RGB_COLORS,
                &mut surface.bits,
                None,
                0,
            )?;
            surface.previous_bitmap = SelectObject(surface.dc, HGDIOBJ(surface.bitmap.0));
            if surface.previous_bitmap.is_invalid() || surface.bits.is_null() {
                return Err(Error::new(E_FAIL, "Could not select the overlay bitmap"));
            }
        }
        Ok(surface)
    }

    pub fn present<'a>(
        &mut self,
        hwnd: HWND,
        drawing: bool,
        annotations: impl IntoIterator<Item = &'a crate::strokes::Annotation>,
    ) -> Result<()> {
        unsafe {
            // This DIB owns width*height aligned u32 pixels until Surface::drop.
            let pixels = std::slice::from_raw_parts_mut(
                self.bits.cast::<u32>(),
                self.width as usize * self.height as usize,
            );
            crate::drawing::render(pixels, self.width as usize, drawing, annotations);
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
            // A null destination preserves the window's virtual-desktop position.
            // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow
            UpdateLayeredWindow(
                hwnd,
                None,
                None,
                Some(&size),
                Some(self.dc),
                Some(&origin),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strokes::{Annotation, Style};
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::{
        Win32::{
            Foundation::{HINSTANCE, LPARAM, LRESULT, RECT, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::{HiDpi::*, WindowsAndMessaging::*},
        },
        core::PCWSTR,
    };

    struct HiddenWindow {
        hwnd: HWND,
        instance: HINSTANCE,
        class_name: Vec<u16>,
    }

    impl HiddenWindow {
        fn new(x: i32, y: i32, width: i32, height: i32) -> Result<Self> {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            unsafe {
                let instance = HINSTANCE(GetModuleHandleW(None)?.0);
                let class_name: Vec<u16> = format!(
                    "MadoSurfaceTest{}-{}\0",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )
                .encode_utf16()
                .collect();
                let class = WNDCLASSW {
                    lpfnWndProc: Some(test_window_proc),
                    hInstance: instance,
                    lpszClassName: PCWSTR(class_name.as_ptr()),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    return Err(Error::from_thread());
                }
                // Own the class before creation so failure also unregisters it.
                let mut window = Self {
                    hwnd: HWND::default(),
                    instance,
                    class_name,
                };
                window.hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    PCWSTR(window.class_name.as_ptr()),
                    PCWSTR::null(),
                    WS_POPUP, // No WS_VISIBLE, show, focus or input calls.
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    Some(instance),
                    None,
                )?;
                Ok(window)
            }
        }
    }

    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            unsafe {
                if self.hwnd.is_invalid() || DestroyWindow(self.hwnd).is_ok() {
                    let _ = UnregisterClassW(PCWSTR(self.class_name.as_ptr()), Some(self.instance));
                }
            }
        }
    }

    unsafe extern "system" fn test_window_proc(
        hwnd: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, message, w, l) }
    }

    struct ThreadDpi(DPI_AWARENESS_CONTEXT);

    impl Drop for ThreadDpi {
        fn drop(&mut self) {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }

    #[test]
    fn hidden_upload_preserves_screen_bounds_and_releases_windows_and_classes() {
        // Keep native bounds in physical pixels without changing other test threads.
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        assert!(!previous.0.is_null());
        let _dpi = ThreadDpi(previous);
        for (x, y) in [(143, 97), (-143, 97), (143, -97), (-143, -97)] {
            let window = HiddenWindow::new(x, y, 80, 60).unwrap();
            let mut before = RECT::default();
            unsafe {
                GetWindowRect(window.hwnd, &mut before).unwrap();
                assert!(!IsWindowVisible(window.hwnd).as_bool());
            }
            assert_eq!(
                before,
                RECT {
                    left: x,
                    top: y,
                    right: x + 80,
                    bottom: y + 60
                }
            );
            let mut surface = Surface::new(80, 60).unwrap();
            let annotations = [Annotation {
                paths: vec![vec![(10, 15), (30, 30)]],
                style: Style::default(),
            }];
            for drawing in [false, true, false] {
                surface.present(window.hwnd, drawing, &annotations).unwrap();
                let mut after = RECT::default();
                unsafe {
                    GetWindowRect(window.hwnd, &mut after).unwrap();
                    assert!(!IsWindowVisible(window.hwnd).as_bool());
                }
                assert_eq!(after, before);
                let pixels =
                    unsafe { std::slice::from_raw_parts(surface.bits.cast::<u32>(), 80 * 60) };
                assert_eq!(pixels[15 * 80 + 10], 0xfff97316);
            }
            drop(surface);
            let hwnd = window.hwnd;
            let instance = window.instance;
            let class_name = window.class_name.clone();
            drop(window);
            unsafe {
                assert!(!IsWindow(Some(hwnd)).as_bool());
                assert!(
                    GetClassInfoW(
                        Some(instance),
                        PCWSTR(class_name.as_ptr()),
                        &mut WNDCLASSW::default(),
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn negative_and_outside_points_remain_canvas_local_and_clip() {
        let mut surface = Surface::new(8, 6).unwrap();
        let annotations = [Annotation {
            paths: vec![
                vec![(i32::MIN, 3), (i32::MAX, 3)],
                vec![(-1, -1)],
                vec![(8, 6)],
                vec![(-20, -20), (-10, -10)],
                vec![(20, 20), (30, 30)],
                vec![(0, 0)],
                vec![(7, 5)],
            ],
            style: Style {
                color: 0xff3b82f6,
                thickness: 1,
            },
        }];
        for drawing in [false, true] {
            assert!(
                surface
                    .present(HWND::default(), drawing, &annotations)
                    .is_err()
            );
            let pixels = unsafe { std::slice::from_raw_parts(surface.bits.cast::<u32>(), 8 * 6) };
            for y in 0..6 {
                for x in 0..8 {
                    let expected = if y == 3 || (x, y) == (0, 0) || (x, y) == (7, 5) {
                        0xff3b82f6
                    } else if drawing {
                        0x01000000
                    } else {
                        0
                    };
                    assert_eq!(pixels[y * 8 + x], expected, "at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn invalid_dimensions_fail_before_allocation() {
        for (width, height) in [
            (0, 60),
            (80, 0),
            (-1, 60),
            (80, -1),
            (i32::MIN, i32::MIN),
            (i32::MAX, 1),
            (1, i32::MAX),
            (i32::MAX, i32::MAX),
            (32768, 32768),
        ] {
            assert_eq!(
                Surface::new(width, height).err().unwrap().code(),
                E_INVALIDARG
            );
        }
    }

    #[test]
    fn dib_renders_modes_and_releases_gdi_resources_without_a_window() {
        let mut surface = Surface::new(80, 60).unwrap();
        let bitmap = HGDIOBJ(surface.bitmap.0);
        let dc = HGDIOBJ(surface.dc.0);
        unsafe {
            let mut dib = DIBSECTION::default();
            assert_eq!(
                GetObjectW(
                    bitmap,
                    size_of::<DIBSECTION>() as i32,
                    Some((&mut dib as *mut DIBSECTION).cast()),
                ),
                size_of::<DIBSECTION>() as i32
            );
            assert_eq!(dib.dsBmih.biWidth, 80);
            // GetObject reports positive height; GetPixel below checks the row orientation.
            assert_eq!(dib.dsBmih.biHeight, 60);
            assert_eq!(dib.dsBmih.biBitCount, 32);
            assert_eq!(dib.dsBm.bmBits, surface.bits);
            assert_eq!(GetCurrentObject(surface.dc, OBJ_BITMAP), bitmap);
        }
        let annotations = [
            Annotation {
                paths: vec![vec![(10, 15), (30, 30)]],
                style: Style::default(),
            },
            Annotation {
                paths: vec![vec![(40, 20)]],
                style: Style {
                    color: 0xff3b82f6,
                    thickness: 1,
                },
            },
        ];
        for (drawing, supplied_annotations) in [
            (false, &[][..]),
            (false, &annotations[..]),
            (true, &annotations[..]),
            (false, &annotations[..]),
            (true, &[][..]),
            (false, &[][..]),
        ] {
            // A null HWND exercises rasterization and the upload error path without showing anything.
            assert!(
                surface
                    .present(HWND::default(), drawing, supplied_annotations)
                    .is_err()
            );
            let pixels = unsafe { std::slice::from_raw_parts(surface.bits.cast::<u32>(), 80 * 60) };
            let background = if drawing { 0x01000000 } else { 0 };
            assert_eq!(pixels[0], background);
            assert_eq!(pixels[30 * 80 + 50], background);
            if supplied_annotations.is_empty() {
                assert!(pixels.iter().all(|&pixel| pixel == background));
            } else {
                assert_eq!(pixels[15 * 80 + 10], 0xfff97316);
                assert_eq!(pixels[20 * 80 + 40], 0xff3b82f6);
                assert_eq!(pixels[15 * 80 + 8], 0xfff97316);
                assert_eq!(pixels[15 * 80 + 7], background);
                assert_eq!(pixels[20 * 80 + 39], background);
                assert_eq!(pixels[19 * 80 + 40], background);
                unsafe {
                    assert_eq!(GetPixel(surface.dc, 40, 20), COLORREF(0x00f6823b));
                    assert_eq!(GetPixel(surface.dc, 10, 15), COLORREF(0x001673f9));
                    assert_eq!(GetPixel(surface.dc, 10, 44), COLORREF(0));
                }
            }
            for &pixel in pixels {
                for shift in [0, 8, 16] {
                    assert!((pixel >> shift) & 0xff <= pixel >> 24);
                }
            }
        }
        drop(surface);
        unsafe {
            let mut info = BITMAP::default();
            assert_eq!(
                GetObjectW(
                    bitmap,
                    size_of::<BITMAP>() as i32,
                    Some((&mut info as *mut BITMAP).cast()),
                ),
                0
            );
            assert!(GetCurrentObject(HDC(dc.0), OBJ_BITMAP).is_invalid());
        }
    }
}
