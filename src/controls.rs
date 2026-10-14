use windows::{
    Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
        VK_OEM_4, VK_OEM_6,
    },
    core::{Error, Result},
};

const TOGGLE: usize = 1;
const CLEAR: usize = 2;
const QUIT: usize = 3;
const PEN: usize = 4;
const LINE: usize = 5;
const RECTANGLE: usize = 6;
const UNDO: usize = 7;
const REDO: usize = 8;
const THINNER: usize = 9;
const THICKER: usize = 10;
const BLUE: usize = 11;
const ORANGE: usize = 12;
const RED: usize = 13;
const GREEN: usize = 14;
const WHITE: usize = 15;
const ARROW: usize = 16;
const ELLIPSE: usize = 17;
const ERASER: usize = 18;
const PICKER: usize = 19;
const QUICK_CLEAR: usize = 20;

const PERMANENT_MODIFIERS: HOT_KEY_MODIFIERS =
    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0);
const HISTORY_MODIFIERS: HOT_KEY_MODIFIERS = HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_NOREPEAT.0);
// MOD_NOREPEAT suppresses repeat; no Alt/Ctrl/Shift/Win modifier is required.
// https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey
const BARE_MODIFIERS: HOT_KEY_MODIFIERS = MOD_NOREPEAT;

type Key = (usize, u16, HOT_KEY_MODIFIERS);
const PERMANENT_KEYS: [Key; 3] = [
    (TOGGLE, b'D' as u16, PERMANENT_MODIFIERS),
    (CLEAR, b'C' as u16, PERMANENT_MODIFIERS),
    (QUIT, b'Q' as u16, PERMANENT_MODIFIERS),
];
const BASIC_KEYS: [Key; 13] = [
    (PEN, b'P' as u16, BARE_MODIFIERS),
    (LINE, b'L' as u16, BARE_MODIFIERS),
    (RECTANGLE, b'R' as u16, BARE_MODIFIERS),
    (UNDO, b'Z' as u16, HISTORY_MODIFIERS),
    (REDO, b'Y' as u16, HISTORY_MODIFIERS),
    (THINNER, VK_OEM_4.0, BARE_MODIFIERS),
    (THICKER, VK_OEM_6.0, BARE_MODIFIERS),
    (BLUE, b'1' as u16, BARE_MODIFIERS),
    (ORANGE, b'2' as u16, BARE_MODIFIERS),
    (RED, b'3' as u16, BARE_MODIFIERS),
    (GREEN, b'4' as u16, BARE_MODIFIERS),
    (WHITE, b'5' as u16, BARE_MODIFIERS),
    (QUICK_CLEAR, b'X' as u16, BARE_MODIFIERS),
];
const ADVANCED_KEYS: [Key; 3] = [
    (ARROW, b'A' as u16, BARE_MODIFIERS),
    (ELLIPSE, b'O' as u16, BARE_MODIFIERS),
    (ERASER, b'E' as u16, BARE_MODIFIERS),
];
const PICKER_KEYS: [Key; 1] = [(PICKER, b'C' as u16, BARE_MODIFIERS)];

fn drawing_key_list(advanced: bool, picker: bool) -> impl Iterator<Item = Key> {
    BASIC_KEYS
        .into_iter()
        .chain(ADVANCED_KEYS.into_iter().take(if advanced { 3 } else { 0 }))
        .chain(PICKER_KEYS.into_iter().take(if picker { 1 } else { 0 }))
}

fn key_label(key: u16, modifiers: HOT_KEY_MODIFIERS) -> String {
    let prefix = if modifiers == PERMANENT_MODIFIERS {
        "Ctrl+Alt+"
    } else if modifiers == HISTORY_MODIFIERS {
        "Ctrl+"
    } else {
        ""
    };
    let name = match key {
        key if key == VK_OEM_4.0 => '[',
        key if key == VK_OEM_6.0 => ']',
        key => char::from_u32(key as u32).unwrap(),
    };
    format!("{prefix}{name}")
}

pub struct Hotkeys(Vec<i32>);

impl Hotkeys {
    pub fn register() -> Result<Self> {
        Self::register_keys(PERMANENT_KEYS)
    }

    pub fn drawing_keys(advanced: bool, picker: bool) -> Result<Self> {
        Self::register_keys(drawing_key_list(advanced, picker))
    }

    fn register_keys(keys: impl IntoIterator<Item = Key>) -> Result<Self> {
        let mut hotkeys = Self(Vec::new());
        for (id, key, modifiers) in keys {
            // One guard owns all successful registrations, including mixed-modifier rollback.
            unsafe { RegisterHotKey(None, id as i32, modifiers, key as u32) }.map_err(|error| {
                Error::new(
                    error.code(),
                    format!(
                        "Could not register {}. Close the conflicting application and retry.",
                        key_label(key, modifiers)
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
    Undo,
    Redo,
    Thinner,
    Thicker,
    Blue,
    Orange,
    Red,
    Green,
    White,
    Arrow,
    Ellipse,
    Eraser,
    Picker,
    QuickClear,
}

pub fn action(id: usize) -> Option<Action> {
    match id {
        TOGGLE => Some(Action::Toggle),
        CLEAR => Some(Action::Clear),
        QUIT => Some(Action::Quit),
        PEN => Some(Action::Pen),
        LINE => Some(Action::Line),
        RECTANGLE => Some(Action::Rectangle),
        UNDO => Some(Action::Undo),
        REDO => Some(Action::Redo),
        THINNER => Some(Action::Thinner),
        THICKER => Some(Action::Thicker),
        BLUE => Some(Action::Blue),
        ORANGE => Some(Action::Orange),
        RED => Some(Action::Red),
        GREEN => Some(Action::Green),
        WHITE => Some(Action::White),
        ARROW => Some(Action::Arrow),
        ELLIPSE => Some(Action::Ellipse),
        ERASER => Some(Action::Eraser),
        PICKER => Some(Action::Picker),
        QUICK_CLEAR => Some(Action::QuickClear),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_ids_and_ignores_unknown_ids() {
        let actions = [
            Action::Toggle,
            Action::Clear,
            Action::Quit,
            Action::Pen,
            Action::Line,
            Action::Rectangle,
            Action::Undo,
            Action::Redo,
            Action::Thinner,
            Action::Thicker,
            Action::Blue,
            Action::Orange,
            Action::Red,
            Action::Green,
            Action::White,
            Action::Arrow,
            Action::Ellipse,
            Action::Eraser,
            Action::Picker,
            Action::QuickClear,
        ];
        for (index, expected) in actions.into_iter().enumerate() {
            assert_eq!(action(index + 1), Some(expected));
        }
        for id in [0, 21, usize::MAX] {
            assert_eq!(action(id), None);
        }
    }

    #[test]
    fn permanent_keys_require_control_alt_and_suppress_repeat() {
        assert_eq!(
            PERMANENT_KEYS.map(|(id, key, _)| (id, key)),
            [(1, b'D' as u16), (2, b'C' as u16), (3, b'Q' as u16)]
        );
        for (_, _, modifiers) in PERMANENT_KEYS {
            assert_eq!(modifiers, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT);
        }
    }

    #[test]
    fn drawing_keys_have_exact_bindings_and_modifiers() {
        let expected = [
            (4, b'P' as u16),
            (5, b'L' as u16),
            (6, b'R' as u16),
            (7, b'Z' as u16),
            (8, b'Y' as u16),
            (9, VK_OEM_4.0),
            (10, VK_OEM_6.0),
            (11, b'1' as u16),
            (12, b'2' as u16),
            (13, b'3' as u16),
            (14, b'4' as u16),
            (15, b'5' as u16),
            (20, b'X' as u16),
            (16, b'A' as u16),
            (17, b'O' as u16),
            (18, b'E' as u16),
            (19, b'C' as u16),
        ];
        let keys: Vec<_> = drawing_key_list(true, true).collect();
        assert_eq!(
            keys.iter()
                .map(|&(id, key, _)| (id, key))
                .collect::<Vec<_>>(),
            expected
        );
        for (id, _, modifiers) in keys {
            let expected = if matches!(id, 7 | 8) {
                MOD_CONTROL | MOD_NOREPEAT
            } else {
                MOD_NOREPEAT
            };
            assert_eq!(modifiers, expected, "hotkey ID {id}");
        }
    }

    #[test]
    fn staged_lists_keep_basic_keys_and_gate_advanced_and_picker_independently() {
        for (advanced, picker, expected) in [
            (false, false, (4..=15).chain([20]).collect::<Vec<_>>()),
            (true, false, (4..=15).chain([20]).chain(16..=18).collect()),
            (false, true, (4..=15).chain([20, 19]).collect()),
            (true, true, (4..=15).chain([20]).chain(16..=19).collect()),
        ] {
            assert_eq!(
                drawing_key_list(advanced, picker)
                    .map(|(id, _, _)| id)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn labels_use_correct_modifier_prefixes_and_readable_brackets() {
        let expected = [
            "Ctrl+Alt+D",
            "Ctrl+Alt+C",
            "Ctrl+Alt+Q",
            "P",
            "L",
            "R",
            "Ctrl+Z",
            "Ctrl+Y",
            "[",
            "]",
            "1",
            "2",
            "3",
            "4",
            "5",
            "X",
            "A",
            "O",
            "E",
            "C",
        ];
        assert_eq!(
            PERMANENT_KEYS
                .into_iter()
                .chain(drawing_key_list(true, true))
                .map(|(_, key, modifiers)| key_label(key, modifiers))
                .collect::<Vec<_>>(),
            expected
        );
    }
}
