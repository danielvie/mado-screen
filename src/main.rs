mod controls;
mod drawing;
mod overlay;
mod surface;

use controls::{Action, Hotkeys};
use overlay::Overlay;
use surface::Surface;
use windows::{
    Win32::UI::{HiDpi::*, WindowsAndMessaging::*},
    core::{Error, Result},
};

fn main() -> Result<()> {
    // Must precede windows and DPI-dependent metrics; see tasks/plan.md sources.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let _hotkeys = Hotkeys::register()?;
    // Locals drop in reverse declaration order, even with late initialization.
    // Keep the window alive only while its surface exists, including error exits.
    let mut surface;
    let overlay = Overlay::new()?;
    let (width, height) = overlay.size();
    surface = Surface::new(width, height)?;
    let mut capture = false;
    surface.present(overlay.hwnd(), capture, &[])?;
    overlay.show()?;

    println!("PoC 1: automatic orange line and outline rectangle.");
    println!("Ctrl+Alt+D: toggle input capture; Ctrl+Alt+C: clear; Ctrl+Alt+Q: quit.");
    println!("Primary display: {width} x {height} physical pixels.");
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
            match controls::action(message.wParam.0) {
                Some(Action::Toggle) => {
                    capture = !capture;
                    overlay.set_capture(capture)?;
                    surface.present(overlay.hwnd(), capture, &[])?;
                    println!(
                        "Mode: {}.",
                        if capture {
                            "input capture, mouse clicks logged only; background alpha 1/255"
                        } else {
                            "click-through"
                        }
                    );
                }
                Some(Action::Clear) => {
                    surface.present(overlay.hwnd(), capture, &[])?;
                    println!("Shapes cleared. Restart to restore them.");
                }
                Some(Action::Quit) => break,
                None => {}
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    #[test]
    fn late_initialized_surface_outlives_window_on_success_and_error() {
        struct Resource<'a>(&'static str, &'a RefCell<Vec<&'static str>>);
        impl Drop for Resource<'_> {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }

        for fail in [false, true] {
            let drops = RefCell::new(Vec::new());
            let result = (|| -> Result<(), ()> {
                let surface;
                let overlay = Resource("overlay", &drops);
                surface = Resource("surface", &drops);
                let _ = (&overlay, &surface);
                if fail {
                    return Err(());
                }
                Ok(())
            })();
            assert_eq!(result.is_err(), fail);
            assert_eq!(*drops.borrow(), ["overlay", "surface"]);
        }
    }
}
