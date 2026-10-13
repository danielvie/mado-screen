use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG, HWND},
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, GetCapture, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_LBUTTON,
                VK_RBUTTON,
            },
            WindowsAndMessaging::{
                GetSystemMetrics, KillTimer, MSG, SM_SWAPBUTTON, SetTimer, WM_CANCELMODE,
                WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_TIMER,
            },
        },
    },
    core::{Error, Result},
};

const DRAG_TIMER_ID: usize = 1;
const EXPIRY_TIMER_ID: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Down((i32, i32)),
    Move { point: (i32, i32), left_down: bool },
    Up((i32, i32)),
    Cancel,
    CheckRelease,
    Expire,
}

pub fn event(message: &MSG, hwnd: HWND) -> Option<Event> {
    const MK_LBUTTON: usize = 0x0001;
    if message.hwnd != hwnd {
        return None;
    }
    let point = (
        message.lParam.0 as i16 as i32,
        (message.lParam.0 >> 16) as i16 as i32,
    );
    match message.message {
        WM_LBUTTONDOWN => Some(Event::Down(point)),
        WM_MOUSEMOVE => Some(Event::Move {
            point,
            left_down: message.wParam.0 & MK_LBUTTON != 0,
        }),
        WM_LBUTTONUP => Some(Event::Up(point)),
        WM_CANCELMODE | WM_CAPTURECHANGED => Some(Event::Cancel),
        WM_TIMER if message.wParam.0 == DRAG_TIMER_ID => Some(Event::CheckRelease),
        WM_TIMER if message.wParam.0 == EXPIRY_TIMER_ID => Some(Event::Expire),
        _ => None,
    }
}

pub struct DragCapture {
    hwnd: HWND,
    timer_owned: bool,
}

impl DragCapture {
    pub fn begin(hwnd: HWND) -> Result<Self> {
        if hwnd.is_invalid() {
            return Err(Error::new(E_INVALIDARG, "Drag capture requires a window"));
        }
        unsafe {
            SetCapture(hwnd);
            // Own partial initialization so any failure releases only our capture.
            let mut capture = Self {
                hwnd,
                timer_owned: false,
            };
            if GetCapture() != hwnd {
                return Err(Error::new(E_FAIL, "Could not capture the mouse"));
            }
            if SetTimer(Some(hwnd), DRAG_TIMER_ID, 32, None) == 0 {
                return Err(Error::from_thread());
            }
            capture.timer_owned = true;
            Ok(capture)
        }
    }

    pub fn has_capture(&self) -> bool {
        unsafe { GetCapture() == self.hwnd }
    }

    pub fn lost_or_released(&self) -> bool {
        unsafe {
            // Async state uses physical buttons, unlike logical primary-button messages.
            let key = primary_button(GetSystemMetrics(SM_SWAPBUTTON));
            !self.has_capture() || GetAsyncKeyState(key.0 as i32) >= 0
        }
    }
}

impl Drop for DragCapture {
    fn drop(&mut self) {
        unsafe {
            if self.timer_owned {
                let _ = KillTimer(Some(self.hwnd), DRAG_TIMER_ID);
            }
            if GetCapture() == self.hwnd {
                let _ = ReleaseCapture();
            }
        }
    }
}

// Owned only while temporary ink remains; no permanent idle polling.
pub struct ExpiryTimer(HWND);

impl ExpiryTimer {
    pub fn begin(hwnd: HWND) -> Result<Self> {
        if hwnd.is_invalid() {
            return Err(Error::new(E_INVALIDARG, "Expiry timer requires a window"));
        }
        // Timer dispatch can be late. Expiry uses monotonic elapsed time, not tick counts.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-settimer
        if unsafe { SetTimer(Some(hwnd), EXPIRY_TIMER_ID, 32, None) } == 0 {
            return Err(Error::from_thread());
        }
        Ok(Self(hwnd))
    }
}

impl Drop for ExpiryTimer {
    fn drop(&mut self) {
        // Queued ticks may remain; the main loop only expires actual remaining ink.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-killtimer
        unsafe {
            let _ = KillTimer(Some(self.0), EXPIRY_TIMER_ID);
        }
    }
}

fn primary_button(swapped: i32) -> VIRTUAL_KEY {
    if swapped != 0 { VK_RBUTTON } else { VK_LBUTTON }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, WPARAM};

    fn message(kind: u32, w: usize, point: (i16, i16)) -> MSG {
        MSG {
            hwnd: HWND(std::ptr::dangling_mut()),
            message: kind,
            wParam: WPARAM(w),
            lParam: LPARAM(((point.1 as u16 as u32) << 16 | point.0 as u16 as u32) as isize),
            ..Default::default()
        }
    }

    #[test]
    fn mouse_coordinates_are_independently_signed() {
        for point in [
            (-1, 2),
            (3, -4),
            (i16::MIN, i16::MAX),
            (i16::MAX, i16::MIN),
            (-1, -1),
        ] {
            let expected = (point.0 as i32, point.1 as i32);
            for (kind, decoded) in [
                (WM_LBUTTONDOWN, Event::Down(expected)),
                (
                    WM_MOUSEMOVE,
                    Event::Move {
                        point: expected,
                        left_down: false,
                    },
                ),
                (WM_LBUTTONUP, Event::Up(expected)),
            ] {
                let msg = message(kind, 0, point);
                assert_eq!(event(&msg, msg.hwnd), Some(decoded));
            }
        }
    }

    #[test]
    fn move_checks_only_primary_button_flag() {
        for flags in [0, 1, 2, 3, 0xfffe, 0xffff] {
            let msg = message(WM_MOUSEMOVE, flags, (5, 6));
            assert_eq!(
                event(&msg, msg.hwnd),
                Some(Event::Move {
                    point: (5, 6),
                    left_down: flags & 1 != 0,
                })
            );
        }
    }

    #[test]
    fn cancellation_and_timer_filtering() {
        for kind in [WM_CANCELMODE, WM_CAPTURECHANGED] {
            let msg = message(kind, 0, (0, 0));
            assert_eq!(event(&msg, msg.hwnd), Some(Event::Cancel));
        }
        for timer in [0, 1, 2, usize::MAX] {
            let msg = message(WM_TIMER, timer, (0, 0));
            let expected = match timer {
                1 => Some(Event::CheckRelease),
                2 => Some(Event::Expire),
                _ => None,
            };
            assert_eq!(event(&msg, msg.hwnd), expected);
        }
        let msg = message(0, 1, (0, 0));
        assert_eq!(event(&msg, msg.hwnd), None);
    }

    #[test]
    fn ignores_other_windows_for_every_event() {
        for kind in [
            WM_LBUTTONDOWN,
            WM_MOUSEMOVE,
            WM_LBUTTONUP,
            WM_CANCELMODE,
            WM_CAPTURECHANGED,
            WM_TIMER,
        ] {
            let mut msg = message(kind, 1, (0, 0));
            let overlay = msg.hwnd;
            msg.hwnd = HWND(2usize as *mut _);
            assert_eq!(event(&msg, overlay), None);
            msg.hwnd = HWND::default();
            assert_eq!(event(&msg, overlay), None);
        }
    }

    #[test]
    fn swapped_primary_button_uses_physical_right() {
        assert_eq!(primary_button(0), VK_LBUTTON);
        for swapped in [1, -1, i32::MIN, i32::MAX] {
            assert_eq!(primary_button(swapped), VK_RBUTTON);
        }
    }
}
