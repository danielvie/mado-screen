use windows::{
    Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
    },
    core::{Error, Result},
};

const TOGGLE: usize = 1;
const CLEAR: usize = 2;
const QUIT: usize = 3;

pub struct Hotkeys(Vec<i32>);

impl Hotkeys {
    pub fn register() -> Result<Self> {
        let mut hotkeys = Self(Vec::new());
        for (id, key) in [(TOGGLE, b'D'), (CLEAR, b'C'), (QUIT, b'Q')] {
            // Register before showing; partial registration is undone by Drop.
            unsafe {
                RegisterHotKey(None, id as i32, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, key as u32)
            }
            .map_err(|error| {
                Error::new(
                    error.code(),
                    format!(
                        "Could not register Ctrl+Alt+{}. Close the conflicting application and retry.",
                        key as char
                    ),
                )
            })?;
            hotkeys.0.push(id as i32);
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

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Clear,
    Quit,
}

pub fn action(id: usize) -> Option<Action> {
    match id {
        TOGGLE => Some(Action::Toggle),
        CLEAR => Some(Action::Clear),
        QUIT => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_ids_and_ignores_unknown_ids() {
        assert_eq!(action(1), Some(Action::Toggle));
        assert_eq!(action(2), Some(Action::Clear));
        assert_eq!(action(3), Some(Action::Quit));
        for id in [0, 4, usize::MAX] {
            assert_eq!(action(id), None);
        }
    }
}
