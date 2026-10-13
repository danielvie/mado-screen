use windows::{
    Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
    },
    core::{Error, Result},
};

const TOGGLE: usize = 1;
const CLEAR: usize = 2;
const QUIT: usize = 3;
const PEN: usize = 4;
const LINE: usize = 5;
const RECTANGLE: usize = 6;

const PERMANENT_KEYS: [(usize, u8); 3] = [(TOGGLE, b'D'), (CLEAR, b'C'), (QUIT, b'Q')];
const TOOL_KEYS: [(usize, u8); 3] = [(PEN, b'P'), (LINE, b'L'), (RECTANGLE, b'R')];
const PERMANENT_MODIFIERS: HOT_KEY_MODIFIERS =
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0);
// MOD_NOREPEAT suppresses repeat; no Alt/Ctrl/Shift/Win modifier is required.
// https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey
const TOOL_MODIFIERS: HOT_KEY_MODIFIERS = MOD_NOREPEAT;

pub struct Hotkeys(Vec<i32>);

impl Hotkeys {
    pub fn register() -> Result<Self> {
        Self::register_keys(&PERMANENT_KEYS, PERMANENT_MODIFIERS)
    }

    pub fn drawing_tools() -> Result<Self> {
        Self::register_keys(&TOOL_KEYS, TOOL_MODIFIERS)
    }

    fn register_keys(keys: &[(usize, u8)], modifiers: HOT_KEY_MODIFIERS) -> Result<Self> {
        let mut hotkeys = Self(Vec::new());
        let prefix = if modifiers == PERMANENT_MODIFIERS {
            "Ctrl+Alt+"
        } else {
            ""
        };
        for &(id, key) in keys {
            // Each guard owns only successful registrations, including partial rollback.
            unsafe { RegisterHotKey(None, id as i32, modifiers, key as u32) }
            .map_err(|error| {
                Error::new(
                    error.code(),
                    format!(
                        "Could not register {prefix}{}. Close the conflicting application and retry.",
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
        // NULL and the owned ID free this thread's windowless registration.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-unregisterhotkey
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
    Pen,
    Line,
    Rectangle,
}

pub fn action(id: usize) -> Option<Action> {
    match id {
        TOGGLE => Some(Action::Toggle),
        CLEAR => Some(Action::Clear),
        QUIT => Some(Action::Quit),
        PEN => Some(Action::Pen),
        LINE => Some(Action::Line),
        RECTANGLE => Some(Action::Rectangle),
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
        assert_eq!(action(4), Some(Action::Pen));
        assert_eq!(action(5), Some(Action::Line));
        assert_eq!(action(6), Some(Action::Rectangle));
        for id in [0, 7, usize::MAX] {
            assert_eq!(action(id), None);
        }
    }

    #[test]
    fn permanent_keys_require_control_alt_and_suppress_repeat() {
        assert_eq!(PERMANENT_KEYS, [(1, b'D'), (2, b'C'), (3, b'Q')]);
        assert_eq!(PERMANENT_MODIFIERS, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT);
    }

    #[test]
    fn tool_keys_are_bare_and_suppress_repeat() {
        assert_eq!(TOOL_KEYS, [(4, b'P'), (5, b'L'), (6, b'R')]);
        assert_eq!(TOOL_MODIFIERS, MOD_NOREPEAT);
    }
}
