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

    pub fn present(&mut self, hwnd: HWND, drawing: bool, paths: &[Vec<(i32, i32)>]) -> Result<()> {
        unsafe {
            // This DIB owns width*height aligned u32 pixels until Surface::drop.
            let pixels = std::slice::from_raw_parts_mut(
                self.bits.cast::<u32>(),
                self.width as usize * self.height as usize,
            );
            crate::drawing::render(pixels, self.width as usize, drawing, paths);
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
            UpdateLayeredWindow(
                hwnd,
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
        let paths = [vec![(10, 15), (30, 30)], vec![(40, 20)]];
        for (drawing, supplied_paths) in [
            (false, &[][..]),
            (false, &paths[..]),
            (true, &paths[..]),
            (false, &paths[..]),
            (true, &[][..]),
            (false, &[][..]),
        ] {
            // A null HWND exercises rasterization and the upload error path without showing anything.
            assert!(
                surface
                    .present(HWND::default(), drawing, supplied_paths)
                    .is_err()
            );
            let pixels = unsafe { std::slice::from_raw_parts(surface.bits.cast::<u32>(), 80 * 60) };
            let background = if drawing { 0x01000000 } else { 0 };
            assert_eq!(pixels[0], background);
            assert_eq!(pixels[30 * 80 + 50], background);
            if supplied_paths.is_empty() {
                assert!(pixels.iter().all(|&pixel| pixel == background));
            } else {
                assert_eq!(pixels[15 * 80 + 10], 0xfff97316);
                assert_eq!(pixels[20 * 80 + 40], 0xfff97316);
                unsafe {
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
