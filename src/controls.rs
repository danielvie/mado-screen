use windows::{
    Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
        UnregisterHotKey, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F4, VK_F11,
        VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5,
        VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_PRIOR,
        VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
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
const SWAP: usize = 19;
const QUICK_CLEAR: usize = 20;
const DEACTIVATE: usize = 21;
const FUNCTION_TOGGLE: usize = 22;
const PREVIOUS_TOOL: usize = 23;
const PANEL: usize = 24;
const TEMPORARY_PEN: usize = 25;
const ZOOM: usize = 26;

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
const BASIC_KEYS: [Key; 19] = [
    (PEN, b'P' as u16, BARE_MODIFIERS),
    (TEMPORARY_PEN, b'S' as u16, BARE_MODIFIERS),
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
    (QUICK_CLEAR, b'C' as u16, BARE_MODIFIERS),
    (SWAP, b'X' as u16, BARE_MODIFIERS),
    (DEACTIVATE, VK_ESCAPE.0, BARE_MODIFIERS),
    (PREVIOUS_TOOL, b'Q' as u16, BARE_MODIFIERS),
    (PANEL, b'T' as u16, BARE_MODIFIERS),
    (ZOOM, b'Z' as u16, BARE_MODIFIERS),
];
const ADVANCED_KEYS: [Key; 3] = [
    (ARROW, b'A' as u16, BARE_MODIFIERS),
    (ELLIPSE, b'O' as u16, BARE_MODIFIERS),
    (ERASER, b'E' as u16, BARE_MODIFIERS),
];
#[cfg(test)]
fn drawing_key_list(advanced: bool) -> impl Iterator<Item = Key> {
    BASIC_KEYS
        .into_iter()
        .chain(ADVANCED_KEYS.into_iter().take(if advanced { 3 } else { 0 }))
}

const NAMED_KEYS: &[(&str, u16)] = &[
    ("Escape", VK_ESCAPE.0),
    ("Space", VK_SPACE.0),
    ("Tab", VK_TAB.0),
    ("Enter", VK_RETURN.0),
    ("Backspace", VK_BACK.0),
    ("Delete", VK_DELETE.0),
    ("Insert", VK_INSERT.0),
    ("Home", VK_HOME.0),
    ("End", VK_END.0),
    ("PageUp", VK_PRIOR.0),
    ("PageDown", VK_NEXT.0),
    ("Left", VK_LEFT.0),
    ("Right", VK_RIGHT.0),
    ("Up", VK_UP.0),
    ("Down", VK_DOWN.0),
    ("[", VK_OEM_4.0),
    ("]", VK_OEM_6.0),
    (";", VK_OEM_1.0),
    ("/", VK_OEM_2.0),
    ("`", VK_OEM_3.0),
    ("\\", VK_OEM_5.0),
    ("'", VK_OEM_7.0),
    (",", VK_OEM_COMMA.0),
    (".", VK_OEM_PERIOD.0),
    ("-", VK_OEM_MINUS.0),
    ("=", VK_OEM_PLUS.0),
];

// Names follow the stable WM_HOTKEY action IDs used by posted-input checks.
pub const SHORTCUT_NAMES: [&str; 26] = [
    "toggle",
    "clear",
    "quit",
    "pen",
    "line",
    "rectangle",
    "undo",
    "redo",
    "thinner",
    "thicker",
    "blue",
    "orange",
    "red",
    "green",
    "white",
    "arrow",
    "ellipse",
    "eraser",
    "swap_colors",
    "quick_clear",
    "deactivate",
    "activation_key",
    "previous_tool",
    "menu",
    "smoke_pen",
    "zoom",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    key: u16,
    modifiers: HOT_KEY_MODIFIERS,
}

impl Default for Shortcut {
    fn default() -> Self {
        Self {
            key: VK_F4.0,
            modifiers: MOD_NOREPEAT,
        }
    }
}

#[cfg(test)]
type ActivationKey = Shortcut;
#[cfg(test)]
fn activation_binding(key: ActivationKey) -> [Key; 1] {
    [(FUNCTION_TOGGLE, key.key, key.modifiers)]
}

impl Shortcut {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('+').map(str::trim).collect::<Vec<_>>();
        let name = parts.pop()?;
        let mut modifiers = MOD_NOREPEAT;
        for part in parts {
            let flag = match part.to_ascii_lowercase().as_str() {
                "ctrl" => MOD_CONTROL,
                "alt" => MOD_ALT,
                "shift" => MOD_SHIFT,
                "win" => MOD_WIN,
                _ => return None,
            };
            if modifiers.0 & flag.0 != 0 {
                return None;
            }
            modifiers |= flag;
        }
        let key = if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() {
            name.as_bytes()[0].to_ascii_uppercase() as u16
        } else if let Some(number) = name
            .strip_prefix('F')
            .or_else(|| name.strip_prefix('f'))
            .and_then(|n| n.parse::<u16>().ok())
            .filter(|n| (1..=11).contains(n) && name[1..] == n.to_string())
        {
            VK_F1.0 + number - 1
        } else {
            NAMED_KEYS
                .iter()
                .find(|(label, _)| label.eq_ignore_ascii_case(name))?
                .1
        };
        Some(Self { key, modifiers })
    }

    pub fn key_label(self) -> String {
        if (VK_F1.0..=VK_F11.0).contains(&self.key) {
            format!("F{}", self.key - VK_F1.0 + 1)
        } else if let Some((label, _)) = NAMED_KEYS.iter().find(|(_, key)| *key == self.key) {
            (*label).into()
        } else {
            char::from_u32(self.key as u32).unwrap().to_string()
        }
    }

    pub fn label(self) -> String {
        let mut label = String::new();
        for (flag, name) in [
            (MOD_CONTROL, "Ctrl"),
            (MOD_ALT, "Alt"),
            (MOD_SHIFT, "Shift"),
            (MOD_WIN, "Win"),
        ] {
            if self.modifiers.0 & flag.0 != 0 {
                label.push_str(name);
                label.push('+');
            }
        }
        label.push_str(&self.key_label());
        label
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcuts([Shortcut; SHORTCUT_NAMES.len()]);

impl Default for Shortcuts {
    fn default() -> Self {
        let mut result = Self(
            [Shortcut {
                key: VK_F4.0,
                modifiers: MOD_NOREPEAT,
            }; SHORTCUT_NAMES.len()],
        );
        for (id, key, modifiers) in PERMANENT_KEYS
            .into_iter()
            .chain(BASIC_KEYS)
            .chain(ADVANCED_KEYS)
        {
            result.0[id - 1] = Shortcut { key, modifiers };
        }
        result
    }
}

impl From<Shortcut> for Shortcuts {
    fn from(key: Shortcut) -> Self {
        let mut shortcuts = Self::default();
        shortcuts.set("activation_key", key);
        shortcuts
    }
}

impl Shortcuts {
    pub fn binding(self, name: &str) -> Shortcut {
        self.0[SHORTCUT_NAMES.iter().position(|key| *key == name).unwrap()]
    }

    pub fn set(&mut self, name: &str, shortcut: Shortcut) {
        self.0[SHORTCUT_NAMES.iter().position(|key| *key == name).unwrap()] = shortcut;
    }

    pub fn validate(self) -> std::result::Result<(), String> {
        for (index, shortcut) in self.0.iter().enumerate() {
            if let Some(other) = self.0[..index].iter().position(|other| other == shortcut) {
                return Err(format!(
                    "Shortcut {} is assigned to both shortcuts.{} and shortcuts.{}.",
                    shortcut.label(),
                    SHORTCUT_NAMES[other],
                    SHORTCUT_NAMES[index]
                ));
            }
        }
        Ok(())
    }

    fn keys(self, drawing: bool) -> Vec<Key> {
        self.0
            .into_iter()
            .enumerate()
            .filter(|(index, _)| {
                drawing || matches!(index + 1, TOGGLE | CLEAR | QUIT | FUNCTION_TOGGLE)
            })
            .map(|(index, shortcut)| (index + 1, shortcut.key, shortcut.modifiers))
            .collect()
    }
}

fn key_label(key: u16, modifiers: HOT_KEY_MODIFIERS) -> String {
    Shortcut { key, modifiers }.label()
}

pub struct Hotkeys {
    bindings: Vec<(usize, usize, Shortcut)>,
}

impl Hotkeys {
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }
    pub fn register(shortcuts: Shortcuts) -> Result<Self> {
        shortcuts
            .validate()
            .map_err(|message| Error::new(windows::Win32::Foundation::E_INVALIDARG, message))?;
        let mut result = Self::register_keys(
            shortcuts
                .keys(false)
                .into_iter()
                .filter(|(id, _, _)| *id != FUNCTION_TOGGLE),
        )?;
        let key = shortcuts.binding("activation_key");
        match Self::register_keys([(FUNCTION_TOGGLE, key.key, key.modifiers)]) {
            Ok(mut activation) => result.bindings.append(&mut activation.bindings),
            Err(error) => eprintln!(
                "{error} Use {} or the tray menu.",
                shortcuts.binding("toggle").label()
            ),
        }
        Ok(result)
    }

    fn register_keys(keys: impl IntoIterator<Item = Key>) -> Result<Self> {
        let mut result = Self {
            bindings: Vec::new(),
        };
        for (id, key, modifiers) in keys {
            unsafe { RegisterHotKey(None, id as i32, modifiers, key as u32) }.map_err(|error| {
                Error::new(
                    error.code(),
                    format!(
                        "Could not register {}. Close the conflicting application and retry.",
                        key_label(key, modifiers)
                    ),
                )
            })?;
            result.bindings.push((id, id, Shortcut { key, modifiers }));
        }
        Ok(result)
    }

    // Acquire new combinations before releasing old ones. Retain combinations that
    // move between actions, so swaps and drawing/global reassignment are atomic.
    pub fn replace(&mut self, shortcuts: Shortcuts, drawing: bool) -> Result<()> {
        self.replace_keys(shortcuts, shortcuts.keys(drawing))
    }

    pub fn set_drawing(&mut self, shortcuts: Shortcuts, drawing: bool) -> Result<()> {
        let has_activation = self
            .bindings
            .iter()
            .any(|(_, action, _)| *action == FUNCTION_TOGGLE);
        let desired = shortcuts
            .keys(drawing)
            .into_iter()
            .filter(|(id, _, _)| *id != FUNCTION_TOGGLE || has_activation)
            .collect();
        self.replace_keys(shortcuts, desired)
    }

    fn replace_keys(&mut self, shortcuts: Shortcuts, desired: Vec<Key>) -> Result<()> {
        shortcuts
            .validate()
            .map_err(|message| Error::new(windows::Win32::Foundation::E_INVALIDARG, message))?;
        let mut pending = Self {
            bindings: Vec::new(),
        };
        let mut next = Vec::new();
        for (action_id, key, modifiers) in desired {
            let shortcut = Shortcut { key, modifiers };
            let id = if let Some(&(id, _, _)) =
                self.bindings.iter().find(|(_, _, old)| *old == shortcut)
            {
                id
            } else {
                let id = (1000..2000)
                    .find(|id| {
                        !self
                            .bindings
                            .iter()
                            .chain(&pending.bindings)
                            .any(|(used, _, _)| used == id)
                    })
                    .unwrap();
                let mut registered = Self::register_keys([(id, key, modifiers)])?;
                pending.bindings.append(&mut registered.bindings);
                id
            };
            next.push((id, action_id, shortcut));
        }
        for &(id, _, _) in &self.bindings {
            if !next.iter().any(|&(next_id, _, _)| next_id == id) {
                unsafe {
                    let _ = UnregisterHotKey(None, id as i32);
                }
            }
        }
        self.bindings = next;
        pending.bindings.clear();
        // KillTimer/UnregisterHotKey do not remove messages already queued.
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, WM_HOTKEY, WM_QUIT,
            };
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool() {
                // WM_QUIT bypasses PeekMessage's filter; never consume a close request.
                if message.message == WM_QUIT {
                    PostQuitMessage(message.wParam.0 as i32);
                    break;
                }
            }
        }
        Ok(())
    }

    pub fn action(&self, id: usize) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(registered, _, _)| *registered == id)
            .and_then(|(_, action_id, _)| action(*action_id))
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        for &(id, _, _) in &self.bindings {
            unsafe {
                let _ = UnregisterHotKey(None, id as i32);
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
    TemporaryPen,
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
    Swap,
    QuickClear,
    Deactivate,
    PreviousTool,
    Panel,
    Zoom,
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
        SWAP => Some(Action::Swap),
        QUICK_CLEAR => Some(Action::QuickClear),
        DEACTIVATE => Some(Action::Deactivate),
        FUNCTION_TOGGLE => Some(Action::Toggle),
        PREVIOUS_TOOL => Some(Action::PreviousTool),
        PANEL => Some(Action::Panel),
        TEMPORARY_PEN => Some(Action::TemporaryPen),
        ZOOM => Some(Action::Zoom),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remapped_shortcuts_parse_labels_modifiers_and_duplicates() {
        for (input, label) in [
            ("p", "P"),
            (" sHiFt + p ", "Shift+P"),
            ("ALT+ctrl+shift+win+f11", "Ctrl+Alt+Shift+Win+F11"),
            ("Ctrl+[", "Ctrl+["),
            ("Alt+Escape", "Alt+Escape"),
            ("Shift+;", "Shift+;"),
            ("Ctrl+=", "Ctrl+="),
            ("Win+PageDown", "Win+PageDown"),
        ] {
            let shortcut = Shortcut::parse(input).unwrap();
            assert_eq!(shortcut.label(), label);
            assert_ne!(shortcut.modifiers.0 & MOD_NOREPEAT.0, 0);
            assert_eq!(Shortcut::parse(label), Some(shortcut));
        }
        for (name, _) in NAMED_KEYS {
            assert!(Shortcut::parse(name).is_some(), "{name}");
        }
        for text in [
            "",
            "F12",
            "F04",
            "F0",
            "Ctrl+Ctrl+P",
            "Alt+Shift+Alt+P",
            "Ctrl++",
            "Ctrl+",
            "+P",
            "P+S",
            "é",
            "Shift+unknown",
        ] {
            assert!(Shortcut::parse(text).is_none(), "{text}");
        }
        let defaults = Shortcuts::default();
        assert!(defaults.validate().is_ok());
        assert_eq!(
            defaults
                .keys(false)
                .iter()
                .map(|(id, _, _)| *id)
                .collect::<Vec<_>>(),
            [1, 2, 3, 22]
        );
        assert_eq!(defaults.keys(true).len(), SHORTCUT_NAMES.len());
        for name in SHORTCUT_NAMES {
            let mut custom = defaults;
            custom.set(name, Shortcut::parse("Ctrl+Alt+Shift+F11").unwrap());
            assert!(custom.validate().is_ok());
            assert_eq!(custom.binding(name).label(), "Ctrl+Alt+Shift+F11");
        }
        let mut custom = defaults;
        custom.set("pen", defaults.binding("smoke_pen"));
        assert!(custom.validate().unwrap_err().contains("pen"));
        custom.set("smoke_pen", defaults.binding("pen"));
        assert!(custom.validate().is_ok());
        custom.set("clear", defaults.binding("pen"));
        assert!(custom.validate().is_err());
    }

    #[test]
    fn remapped_hotkeys_purge_stale_messages_but_preserve_quit() -> Result<()> {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::{
                MSG, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage, WM_HOTKEY, WM_QUIT,
            },
        };
        let mut hotkeys = Hotkeys::empty();
        let mut message = MSG::default();
        unsafe {
            // Creates the current thread's queue without opening a window.
            let _ = PeekMessageW(&mut message, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE);
            PostMessageW(None, WM_HOTKEY, WPARAM(PEN), LPARAM(1))?;
            PostMessageW(None, WM_HOTKEY, WPARAM(LINE), LPARAM(1))?;
        }
        hotkeys.replace_keys(Shortcuts::default(), Vec::new())?;
        assert!(
            !unsafe { PeekMessageW(&mut message, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }.as_bool()
        );
        unsafe {
            PostQuitMessage(37);
        }
        hotkeys.replace_keys(Shortcuts::default(), Vec::new())?;
        assert!(unsafe { PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE) }.as_bool());
        assert_eq!(message.message, WM_QUIT);
        assert_eq!(message.wParam.0, 37);
        Ok(())
    }

    #[test]
    fn remapped_hotkeys_swaps_rollback_and_release_owned_registrations() -> Result<()> {
        let modifiers = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        // Use available uncommon combinations, never inject system input.
        let mut guards = Vec::new();
        for key in b'A'..=b'Z' {
            if let Ok(guard) = Hotkeys::register_keys([(500 + key as usize, key as u16, modifiers)])
            {
                guards.push(guard);
                if guards.len() == 4 {
                    break;
                }
            }
        }
        assert_eq!(guards.len(), 4, "Need four available test hotkeys");
        let blocked = guards.pop().unwrap();
        let spare = guards.pop().unwrap();
        let (_, _, spare_key) = spare.bindings[0];
        drop(spare);
        let mut hotkeys = guards.remove(0);
        let mut second = guards.pop().unwrap();
        hotkeys.bindings.append(&mut second.bindings);
        let old = hotkeys.bindings.clone();
        let defaults = Shortcuts::default();
        let mut custom = defaults;
        custom.set("pen", old[1].2);
        custom.set("smoke_pen", old[0].2);
        hotkeys.replace_keys(
            custom,
            vec![
                (PEN, old[1].2.key, modifiers),
                (TEMPORARY_PEN, old[0].2.key, modifiers),
            ],
        )?;
        assert_eq!(hotkeys.action(old[0].0), Some(Action::TemporaryPen));
        assert_eq!(hotkeys.action(old[1].0), Some(Action::Pen));
        let active = hotkeys.bindings.clone();
        let blocked_key = blocked.bindings[0].2;
        custom.set("line", spare_key);
        custom.set("rectangle", blocked_key);
        assert!(
            hotkeys
                .replace_keys(
                    custom,
                    vec![
                        (PEN, old[1].2.key, modifiers),
                        (LINE, spare_key.key, modifiers),
                        (RECTANGLE, blocked_key.key, modifiers),
                    ]
                )
                .is_err()
        );
        assert_eq!(hotkeys.bindings, active);
        drop(Hotkeys::register_keys([(700, spare_key.key, modifiers)])?);
        assert!(Hotkeys::register_keys([(701, old[0].2.key, modifiers)]).is_err());
        drop(blocked);
        hotkeys.replace_keys(custom, vec![(LINE, spare_key.key, modifiers)])?;
        assert!(hotkeys.action(old[0].0).is_none());
        let new_id = hotkeys.bindings[0].0;
        assert_eq!(hotkeys.action(new_id), Some(Action::Line));
        drop(Hotkeys::register_keys([(702, old[0].2.key, modifiers)])?);
        drop(hotkeys);
        drop(Hotkeys::register_keys([(703, spare_key.key, modifiers)])?);
        Ok(())
    }

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
            Action::Swap,
            Action::QuickClear,
            Action::Deactivate,
            Action::Toggle,
            Action::PreviousTool,
            Action::Panel,
            Action::TemporaryPen,
            Action::Zoom,
        ];
        for (index, expected) in actions.into_iter().enumerate() {
            assert_eq!(action(index + 1), Some(expected));
        }
        for id in [0, 27, usize::MAX] {
            assert_eq!(action(id), None);
        }
    }

    #[test]
    fn zoom_is_remappable_drawing_only_and_distinct_from_undo() {
        let mut shortcuts = Shortcuts::default();
        assert_eq!(shortcuts.binding("zoom").label(), "Z");
        assert!(!shortcuts.keys(false).iter().any(|(id, _, _)| *id == ZOOM));
        assert!(
            shortcuts
                .keys(true)
                .contains(&(ZOOM, b'Z' as u16, MOD_NOREPEAT))
        );
        assert_eq!(shortcuts.binding("undo").label(), "Ctrl+Z");
        shortcuts.set("zoom", Shortcut::parse("Shift+Z").unwrap());
        assert!(shortcuts.validate().is_ok());
        assert!(
            shortcuts
                .keys(true)
                .contains(&(ZOOM, b'Z' as u16, MOD_SHIFT | MOD_NOREPEAT))
        );
        shortcuts.set("zoom", shortcuts.binding("undo"));
        assert!(shortcuts.validate().is_err());
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
            (25, b'S' as u16),
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
            (20, b'C' as u16),
            (19, b'X' as u16),
            (21, VK_ESCAPE.0),
            (23, b'Q' as u16),
            (24, b'T' as u16),
            (26, b'Z' as u16),
            (16, b'A' as u16),
            (17, b'O' as u16),
            (18, b'E' as u16),
        ];
        let keys: Vec<_> = drawing_key_list(true).collect();
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
    fn staged_lists_keep_basic_keys_and_gate_advanced() {
        for (advanced, expected) in [
            (
                false,
                [4, 25]
                    .into_iter()
                    .chain(5..=15)
                    .chain([20, 19, 21, 23, 24, 26])
                    .collect::<Vec<_>>(),
            ),
            (
                true,
                [4, 25]
                    .into_iter()
                    .chain(5..=15)
                    .chain([20, 19, 21, 23, 24, 26])
                    .chain(16..=18)
                    .collect(),
            ),
        ] {
            assert_eq!(
                drawing_key_list(advanced)
                    .map(|(id, _, _)| id)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn escape_deactivates_drawing_without_a_permanent_binding() {
        for advanced in [false, true] {
            let keys: Vec<_> = drawing_key_list(advanced).collect();
            assert!(keys.contains(&(DEACTIVATE, VK_ESCAPE.0, MOD_NOREPEAT)));
        }
        assert!(PERMANENT_KEYS.iter().all(|(_, key, _)| *key != VK_ESCAPE.0));
        assert_eq!(action(DEACTIVATE), Some(Action::Deactivate));
        assert_eq!(key_label(VK_ESCAPE.0, BARE_MODIFIERS), "Escape");
        assert_eq!(PERMANENT_KEYS[1], (CLEAR, b'C' as u16, PERMANENT_MODIFIERS));
    }

    #[test]
    fn pen_shortcuts_are_drawing_only_exact_variants_and_suppress_repeat() {
        for (id, key, expected) in [
            (PEN, b'P', Action::Pen),
            (TEMPORARY_PEN, b'S', Action::TemporaryPen),
        ] {
            for advanced in [false, true] {
                assert!(
                    drawing_key_list(advanced)
                        .any(|binding| binding == (id, key as u16, MOD_NOREPEAT))
                );
            }
            assert!(
                PERMANENT_KEYS
                    .into_iter()
                    .chain(activation_binding(ActivationKey::default()))
                    .all(|(_, vk, _)| vk != key as u16)
            );
            assert_eq!(action(id), Some(expected));
        }
    }

    #[test]
    fn previous_tool_is_drawing_only_and_suppresses_repeat() {
        for advanced in [false, true] {
            assert!(
                drawing_key_list(advanced)
                    .any(|key| key == (PREVIOUS_TOOL, b'Q' as u16, MOD_NOREPEAT))
            );
        }
        assert!(
            PERMANENT_KEYS
                .into_iter()
                .chain(activation_binding(ActivationKey::default()))
                .all(|(_, key, modifiers)| key != b'Q' as u16 || modifiers != BARE_MODIFIERS)
        );
        assert_eq!(action(PREVIOUS_TOOL), Some(Action::PreviousTool));
    }

    #[test]
    fn panel_shortcut_is_drawing_only_and_suppresses_repeat() {
        for advanced in [false, true] {
            assert!(
                drawing_key_list(advanced).any(|key| key == (PANEL, b'T' as u16, MOD_NOREPEAT))
            );
        }
        assert!(
            PERMANENT_KEYS
                .into_iter()
                .chain(activation_binding(ActivationKey::default()))
                .all(|(_, key, _)| key != b'T' as u16)
        );
        assert_eq!(action(PANEL), Some(Action::Panel));
        assert_eq!(key_label(b'T' as u16, BARE_MODIFIERS), "T");
        let defaults = Shortcuts::default();
        assert!(defaults.validate().is_ok());
        assert!(
            defaults
                .keys(true)
                .iter()
                .all(|(_, key, _)| *key != b'H' as u16)
        );
        for id in [PANEL, PREVIOUS_TOOL] {
            assert!(
                defaults
                    .keys(false)
                    .iter()
                    .all(|(action, _, _)| *action != id)
            );
        }
        assert!(
            defaults
                .keys(false)
                .contains(&(QUIT, b'Q' as u16, PERMANENT_MODIFIERS))
        );
    }

    #[test]
    fn f4_toggles_from_any_mode_without_modifiers() {
        assert_eq!(
            activation_binding(ActivationKey::default()),
            [(FUNCTION_TOGGLE, VK_F4.0, MOD_NOREPEAT)]
        );
        assert_eq!(action(FUNCTION_TOGGLE), Some(Action::Toggle));
        assert!(drawing_key_list(true).all(|(_, key, _)| key != VK_F4.0));
    }

    #[test]
    fn settings_activation_keys_validate_and_bind_without_drawing_collisions() {
        for number in 1..=11 {
            let label = format!("F{number}");
            let key = ActivationKey::parse(&label).unwrap();
            assert_eq!(ActivationKey::parse(&label.to_lowercase()), Some(key));
            assert_eq!(key.label(), label);
            assert_eq!(
                activation_binding(key),
                [(FUNCTION_TOGGLE, VK_F1.0 + number - 1, MOD_NOREPEAT)]
            );
            assert_eq!(key_label(key.key, BARE_MODIFIERS), label);
            assert!(drawing_key_list(true).all(|(_, vk, _)| vk != key.key));
            assert!(PERMANENT_KEYS.into_iter().all(|(_, vk, _)| vk != key.key));
        }
        for invalid in [
            "",
            "F0",
            "F12",
            "F24",
            "F04",
            "Ctrl+Ctrl+P",
            "Alt+",
            "Alt+F12",
        ] {
            assert_eq!(ActivationKey::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn labels_use_correct_modifier_prefixes_and_readable_brackets() {
        let expected = [
            "Ctrl+Alt+D",
            "Ctrl+Alt+C",
            "Ctrl+Alt+Q",
            "P",
            "S",
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
            "C",
            "X",
            "Escape",
            "Q",
            "T",
            "Z",
            "A",
            "O",
            "E",
            "F4",
        ];
        assert_eq!(
            PERMANENT_KEYS
                .into_iter()
                .chain(drawing_key_list(true))
                .chain(activation_binding(ActivationKey::default()))
                .map(|(_, key, modifiers)| key_label(key, modifiers))
                .collect::<Vec<_>>(),
            expected
        );
    }
}
