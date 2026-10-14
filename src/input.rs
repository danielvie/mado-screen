use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, GetCapture, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_LBUTTON,
                VK_RBUTTON,
            },
            WindowsAndMessaging::{
                CallNextHookEx, GetSystemMetrics, HC_ACTION, HHOOK, KillTimer, MSG, MSLLHOOKSTRUCT,
                PostMessageW, SM_SWAPBUTTON, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
                WH_MOUSE_LL, WHEEL_DELTA, WM_APP, WM_CANCELMODE, WM_CAPTURECHANGED, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_TIMER,
            },
        },
    },
    core::{Error, Result},
};

const DRAG_TIMER_ID: usize = 1;
const EXPIRY_TIMER_ID: usize = 2;
const WHEEL_EVENT: u32 = WM_APP + 43;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Down((i32, i32)),
    Move { point: (i32, i32), left_down: bool },
    Up((i32, i32)),
    Wheel(i32),
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
        WHEEL_EVENT => Some(Event::Wheel(message.wParam.0 as i32)),
        WM_CANCELMODE | WM_CAPTURECHANGED => Some(Event::Cancel),
        WM_TIMER if message.wParam.0 == DRAG_TIMER_ID => Some(Event::CheckRelease),
        WM_TIMER if message.wParam.0 == EXPIRY_TIMER_ID => Some(Event::Expire),
        _ => None,
    }
}

#[derive(Clone, Copy, Default)]
struct WheelState {
    owner: HWND,
    pending: i32,
}

thread_local! {
    static WHEEL_STATE: Cell<WheelState> = Cell::new(WheelState::default());
}

// WM_MOUSEWHEEL normally targets keyboard focus. This hook avoids activating the overlay.
// https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel
// Installed only while drawing; the callback only accumulates deltas and posts work.
// ponytail: long main-thread renders can exceed LowLevelHooksTimeout and remove the hook.
// Move it to a dedicated message-loop thread if rendering stalls approach that timeout.
// https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc
pub struct MouseWheel(HHOOK);

impl MouseWheel {
    pub fn begin(owner: HWND) -> Result<Self> {
        if owner.is_invalid() {
            return Err(Error::new(E_INVALIDARG, "Mouse wheel requires a window"));
        }
        let hook = unsafe {
            SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(wheel_proc),
                Some(HINSTANCE(GetModuleHandleW(None)?.0)),
                0,
            )?
        };
        WHEEL_STATE.set(WheelState { owner, pending: 0 });
        Ok(Self(hook))
    }
}

impl Drop for MouseWheel {
    fn drop(&mut self) {
        WHEEL_STATE.set(WheelState::default());
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
    }
}

fn wheel_steps(pending: &mut i32, delta: i16) -> i32 {
    *pending += i32::from(delta);
    let steps = *pending / WHEEL_DELTA as i32;
    *pending %= WHEEL_DELTA as i32;
    steps
}

unsafe extern "system" fn wheel_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && w.0 == WM_MOUSEWHEEL as usize {
        let mut state = WHEEL_STATE.get();
        if !state.owner.is_invalid() {
            let data = unsafe { &*(l.0 as *const MSLLHOOKSTRUCT) };
            let steps = wheel_steps(&mut state.pending, (data.mouseData >> 16) as i16);
            if steps == 0
                || unsafe {
                    PostMessageW(
                        Some(state.owner),
                        WHEEL_EVENT,
                        WPARAM(steps as usize),
                        LPARAM::default(),
                    )
                }
                .is_ok()
            {
                WHEEL_STATE.set(state);
                return LRESULT(1); // Consumed: do not also scroll the focused application.
            }
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
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
    fn wheel_accumulates_signed_partial_and_multiple_notches() {
        let mut pending = 0;
        for (delta, steps, remainder) in [
            (30, 0, 30),
            (60, 0, 90),
            (30, 1, 0),
            (-60, 0, -60),
            (-60, -1, 0),
            (240, 2, 0),
            (-360, -3, 0),
            (90, 0, 90),
            (-30, 0, 60),
            (-180, -1, 0),
            (0, 0, 0),
            (i16::MAX, 273, 7),
            (i16::MIN, -273, -1),
        ] {
            assert_eq!(wheel_steps(&mut pending, delta), steps);
            assert_eq!(pending, remainder);
        }
    }

    #[test]
    fn wheel_messages_decode_signed_steps_only_for_owner() {
        for steps in [i32::MIN, -3, -1, 0, 1, 2, i32::MAX] {
            let msg = message(WHEEL_EVENT, steps as usize, (-200, 300));
            assert_eq!(event(&msg, msg.hwnd), Some(Event::Wheel(steps)));
        }
        // Real wheel input is handled by the hook, never again by normal window routing.
        let msg = message(WM_MOUSEWHEEL, 120 << 16, (0, 0));
        assert_eq!(event(&msg, msg.hwnd), None);
    }

    #[test]
    fn wheel_hook_posts_without_focus_and_releases_state_on_drop() {
        use crate::overlay::Overlay;
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::GetFocus,
            WindowsAndMessaging::{GetForegroundWindow, PM_REMOVE, PeekMessageW, WM_MOUSEHWHEEL},
        };
        assert!(MouseWheel::begin(HWND::default()).is_err());
        let overlay = Overlay::new().unwrap(); // Hidden; no physical input is injected.
        let foreground = unsafe { GetForegroundWindow() };
        let focus = unsafe { GetFocus() };
        let wheel = MouseWheel::begin(overlay.hwnd()).unwrap();
        assert_eq!(WHEEL_STATE.get().owner, overlay.hwnd());
        for (delta, expected) in [(60, None), (60, Some(1)), (-240, Some(-2))] {
            let data = MSLLHOOKSTRUCT {
                mouseData: (delta as i16 as u16 as u32) << 16,
                ..Default::default()
            };
            assert_eq!(
                unsafe {
                    wheel_proc(
                        HC_ACTION as i32,
                        WPARAM(WM_MOUSEWHEEL as usize),
                        LPARAM(&data as *const _ as isize),
                    )
                }
                .0,
                1
            );
            let mut msg = MSG::default();
            let posted = unsafe {
                PeekMessageW(
                    &mut msg,
                    Some(overlay.hwnd()),
                    WHEEL_EVENT,
                    WHEEL_EVENT,
                    PM_REMOVE,
                )
            }
            .as_bool();
            assert_eq!(posted, expected.is_some());
            if let Some(steps) = expected {
                assert_eq!(event(&msg, overlay.hwnd()), Some(Event::Wheel(steps)));
            }
        }
        let state = WHEEL_STATE.get();
        for (code, kind) in [(-1, WM_MOUSEWHEEL), (0, WM_MOUSEHWHEEL), (0, WM_MOUSEMOVE)] {
            // Irrelevant hook events must not dereference lParam or consume input.
            unsafe { wheel_proc(code, WPARAM(kind as usize), LPARAM::default()) };
            assert_eq!(WHEEL_STATE.get().pending, state.pending);
        }
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
        assert_eq!(unsafe { GetFocus() }, focus);
        drop(wheel);
        assert!(WHEEL_STATE.get().owner.is_invalid());
        assert_eq!(WHEEL_STATE.get().pending, 0);
        unsafe { wheel_proc(0, WPARAM(WM_MOUSEWHEEL as usize), LPARAM::default()) };
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
            WHEEL_EVENT,
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
