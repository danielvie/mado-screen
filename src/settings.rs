use crate::controls::{SHORTCUT_NAMES, Shortcut, Shortcuts};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Favorites {
    pub active: u32,
    pub reserve: u32,
}

impl Default for Favorites {
    fn default() -> Self {
        Self {
            active: 0xfff97316,
            reserve: 0xff0085dc,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub favorites: Favorites,
    pub thickness_px: u32,
    pub duration_ms: u64,
    pub tail_first: bool,
    pub visible_on_start: bool,
    pub shortcuts: Shortcuts,
    pub head_scale: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            favorites: Favorites::default(),
            thickness_px: 5,
            duration_ms: 1000,
            tail_first: false,
            visible_on_start: true,
            shortcuts: Shortcuts::default(),
            head_scale: 1.0,
        }
    }
}

const DEFAULT_DOCUMENT: &str = "\
# Edit startup settings while Mado is closed, then restart it.
# Colors, thickness and arrow direction save automatically when changed in Mado.
[brush]
active_color = \"#F97316\"
reserve_color = \"#0085DC\"
# Odd physical pixel widths from 1 to 31.
thickness_px = 5

[temporary_pen]
# Each stroke segment fades completely after this many milliseconds, 1 to 60000.
duration_ms = 1000

[arrow]
tail_first = false
# Multiply arrowhead size by 0.1 to 4.0, before the short-arrow cap.
head_scale = 1.0

[menu]
visible_on_start = true

[shortcuts]
# One key with optional Ctrl+Alt+Shift+Win modifiers. F12 is reserved by Windows.
activation_key = \"F4\"
toggle = \"Ctrl+Alt+D\"
clear = \"Ctrl+Alt+C\"
quit = \"Ctrl+Alt+Q\"
pen = \"P\"
smoke_pen = \"S\"
line = \"L\"
rectangle = \"R\"
arrow = \"A\"
ellipse = \"O\"
eraser = \"E\"
undo = \"Ctrl+Z\"
redo = \"Ctrl+Y\"
thinner = \"[\"
thicker = \"]\"
blue = \"1\"
orange = \"2\"
red = \"3\"
green = \"4\"
white = \"5\"
swap_colors = \"X\"
quick_clear = \"C\"
deactivate = \"Escape\"
previous_tool = \"T\"
menu = \"H\"
";

pub fn path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".mado/mado-screen/settings.toml"))
}

pub fn load(path: &Path) -> Settings {
    initialize(path).unwrap_or_else(|error| {
        eprintln!(
            "Could not load {}: {error}. Using default settings; existing file unchanged.",
            path.display()
        );
        Settings::default()
    })
}

fn initialize(path: &Path) -> io::Result<Settings> {
    let document = match read_document(path)? {
        Some(mut document) => {
            let defaults = DEFAULT_DOCUMENT.parse::<DocumentMut>().map_err(invalid)?;
            let mut changed = false;
            // Only the settings in the default document are needed by Mado.
            for (section, default_table) in defaults.iter() {
                if !document.contains_key(section) {
                    document[section] = default_table.clone();
                    changed = true;
                    continue;
                }
                // Keep incompatible user sections intact; parsing supplies runtime defaults.
                let Some(table) = document[section].as_table_like_mut() else {
                    continue;
                };
                for (key, value) in default_table.as_table_like().unwrap().iter() {
                    if !table.contains_key(key) {
                        table.insert(key, value.clone());
                        changed = true;
                    }
                }
            }
            if changed && let Err(error) = write_document(path, &document) {
                eprintln!(
                    "Could not complete missing settings in {}: {error}. Loaded values and runtime defaults remain in use; existing file unchanged.",
                    path.display()
                );
            }
            document
        }
        None => {
            let mut document = DEFAULT_DOCUMENT.parse::<DocumentMut>().map_err(invalid)?;
            // New settings take precedence. The legacy file is retained as a backup.
            match fs::read_to_string(path.with_file_name("config.ini")) {
                Ok(text) => update_colors(&mut document, legacy_colors(&text))?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            write_document(path, &document)?;
            document
        }
    };
    Ok(parse(&document))
}

fn read_document(path: &Path) -> io::Result<Option<DocumentMut>> {
    match fs::read_to_string(path) {
        Ok(text) => text.parse::<DocumentMut>().map(Some).map_err(invalid),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16)
        .ok()
        .map(|color| 0xff000000 | color)
}

fn read_setting<T>(
    document: &DocumentMut,
    section: &str,
    key: &str,
    default: T,
    decode: impl FnOnce(&Item) -> Option<T>,
) -> T {
    let Some(table) = document.get(section) else {
        return default;
    };
    let Some(table) = table.as_table_like() else {
        eprintln!("Invalid settings section {section}; using defaults for that section.");
        return default;
    };
    let Some(item) = table.get(key) else {
        return default;
    };
    decode(item).unwrap_or_else(|| {
        eprintln!("Invalid setting {section}.{key}; using its default.");
        default
    })
}

fn parse(document: &DocumentMut) -> Settings {
    let defaults = Settings::default();
    Settings {
        favorites: Favorites {
            active: read_setting(
                document,
                "brush",
                "active_color",
                defaults.favorites.active,
                |item| color(item.as_str()?),
            ),
            reserve: read_setting(
                document,
                "brush",
                "reserve_color",
                defaults.favorites.reserve,
                |item| color(item.as_str()?),
            ),
        },
        thickness_px: read_setting(
            document,
            "brush",
            "thickness_px",
            defaults.thickness_px,
            |item| {
                let n = item.as_integer()?;
                ((1..=31).contains(&n) && n % 2 != 0).then_some(n as u32)
            },
        ),
        duration_ms: read_setting(
            document,
            "temporary_pen",
            "duration_ms",
            defaults.duration_ms,
            |item| {
                let n = item.as_integer()?;
                (1..=60000).contains(&n).then_some(n as u64)
            },
        ),
        tail_first: read_setting(
            document,
            "arrow",
            "tail_first",
            defaults.tail_first,
            Item::as_bool,
        ),
        visible_on_start: read_setting(
            document,
            "menu",
            "visible_on_start",
            defaults.visible_on_start,
            Item::as_bool,
        ),
        head_scale: read_setting(
            document,
            "arrow",
            "head_scale",
            defaults.head_scale,
            head_scale,
        ),
        shortcuts: {
            let mut shortcuts = defaults.shortcuts;
            for name in SHORTCUT_NAMES {
                shortcuts.set(
                    name,
                    read_setting(
                        document,
                        "shortcuts",
                        name,
                        defaults.shortcuts.binding(name),
                        |item| Shortcut::parse(item.as_str()?),
                    ),
                );
            }
            if let Err(error) = shortcuts.validate() {
                eprintln!("{error} Using default shortcuts.");
                shortcuts = defaults.shortcuts;
            }
            shortcuts
        },
    }
}

fn head_scale(item: &Item) -> Option<f64> {
    let n = item
        .as_float()
        .or_else(|| item.as_integer().map(|n| n as f64))?;
    (n.is_finite() && (0.1..=4.0).contains(&n)).then_some(n)
}

// Reload never repairs or rewrites the file, and rejects any invalid known setting.
pub fn reload(path: &Path) -> io::Result<Settings> {
    let document = read_document(path)?.ok_or_else(|| invalid("Settings file not found"))?;
    for section in ["brush", "temporary_pen", "arrow", "menu", "shortcuts"] {
        if let Some(item) = document.get(section) {
            let table = item
                .as_table_like()
                .ok_or_else(|| invalid(format!("{section} must be a table")))?;
            for (name, item) in table.iter() {
                let valid = match (section, name) {
                    ("brush", "active_color" | "reserve_color") => {
                        item.as_str().and_then(color).is_some()
                    }
                    ("brush", "thickness_px") => item
                        .as_integer()
                        .is_some_and(|n| (1..=31).contains(&n) && n % 2 != 0),
                    ("temporary_pen", "duration_ms") => {
                        item.as_integer().is_some_and(|n| (1..=60000).contains(&n))
                    }
                    ("arrow", "head_scale") => head_scale(item).is_some(),
                    ("arrow", "tail_first") | ("menu", "visible_on_start") => {
                        item.as_bool().is_some()
                    }
                    ("shortcuts", name) if SHORTCUT_NAMES.contains(&name) => {
                        item.as_str().and_then(Shortcut::parse).is_some()
                    }
                    _ => true,
                };
                if !valid {
                    return Err(invalid(format!("Invalid setting {section}.{name}")));
                }
            }
        }
    }
    let mut shortcuts = Shortcuts::default();
    if let Some(table) = document.get("shortcuts").and_then(Item::as_table_like) {
        for name in SHORTCUT_NAMES {
            if let Some(item) = table.get(name) {
                shortcuts.set(name, Shortcut::parse(item.as_str().unwrap()).unwrap());
            }
        }
    }
    shortcuts.validate().map_err(invalid)?;
    Ok(parse(&document))
}

fn legacy_colors(text: &str) -> Favorites {
    let mut favorites = Favorites::default();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=')
            && let Some(color) = color(value.trim())
        {
            match key.trim() {
                "active" => favorites.active = color,
                "reserve" => favorites.reserve = color,
                _ => {}
            }
        }
    }
    favorites
}

// Freshly read the document so saving brush state cannot overwrite other manual settings.
// toml_edit preserves comments and formatting; changed values retain their decoration.
// https://docs.rs/toml_edit/0.22.27/toml_edit/
pub fn save_brush(
    path: &Path,
    favorites: Favorites,
    thickness_px: u32,
    tail_first: bool,
) -> io::Result<()> {
    if !(1..=31).contains(&thickness_px) || thickness_px.is_multiple_of(2) {
        return Err(invalid("Brush thickness must be odd and between 1 and 31"));
    }
    let mut document =
        read_document(path)?.unwrap_or(DEFAULT_DOCUMENT.parse::<DocumentMut>().map_err(invalid)?);
    update_colors(&mut document, favorites)?;
    let table = document["brush"].as_table_like_mut().unwrap();
    let key = "thickness_px";
    if table.get(key).is_some_and(|item| !item.is_value()) {
        return Err(invalid(
            "brush.thickness_px must be a value; refusing to overwrite it",
        ));
    }
    if table.get(key).and_then(Item::as_integer) != Some(i64::from(thickness_px)) {
        let mut value = Value::from(i64::from(thickness_px));
        if let Some(previous) = table.get(key).and_then(Item::as_value) {
            *value.decor_mut() = previous.decor().clone();
        }
        table.insert(key, Item::Value(value));
    }
    if !document.contains_key("arrow") {
        document["arrow"] = Item::Table(toml_edit::Table::new());
    }
    let table = document["arrow"]
        .as_table_like_mut()
        .ok_or_else(|| invalid("arrow must be a table; refusing to overwrite it"))?;
    let key = "tail_first";
    if table.get(key).is_some_and(|item| !item.is_value()) {
        return Err(invalid(
            "arrow.tail_first must be a value; refusing to overwrite it",
        ));
    }
    if table.get(key).and_then(Item::as_bool) != Some(tail_first) {
        let mut value = Value::from(tail_first);
        if let Some(previous) = table.get(key).and_then(Item::as_value) {
            *value.decor_mut() = previous.decor().clone();
        }
        table.insert(key, Item::Value(value));
    }
    write_document(path, &document)
}

fn update_colors(document: &mut DocumentMut, favorites: Favorites) -> io::Result<()> {
    if !document.contains_key("brush") {
        document["brush"] = Item::Table(toml_edit::Table::new());
    }
    let table = document["brush"]
        .as_table_like_mut()
        .ok_or_else(|| invalid("brush must be a table; refusing to overwrite it"))?;
    for (key, color) in [
        ("active_color", favorites.active),
        ("reserve_color", favorites.reserve),
    ] {
        let text = format!("#{:06X}", color & 0xffffff);
        if table.get(key).and_then(Item::as_str) == Some(text.as_str()) {
            continue;
        }
        if table.get(key).is_some_and(|item| !item.is_value()) {
            return Err(invalid(format!(
                "brush.{key} must be a value; refusing to overwrite it"
            )));
        }
        let mut value = Value::from(text);
        if let Some(previous) = table.get(key).and_then(Item::as_value) {
            *value.decor_mut() = previous.decor().clone();
        }
        table.insert(key, Item::Value(value));
    }
    Ok(())
}

fn write_document(path: &Path, document: &DocumentMut) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let temporary = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    // Same-directory replacement never truncates the user's existing settings on failure.
    // ponytail: no cross-process edit lock; close Mado before editing settings.
    let result = (|| {
        file.write_all(document.to_string().as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(text: &str) -> DocumentMut {
        text.parse().unwrap()
    }

    #[test]
    fn settings_defaults_custom_values_and_invalid_fields() {
        assert_eq!(parse(&document(DEFAULT_DOCUMENT)), Settings::default());
        assert_eq!(parse(&document("")), Settings::default());
        let custom = parse(&document(
            "[brush]\nactive_color='#123456'\nreserve_color='#ABCDEF'\nthickness_px=31\n[temporary_pen]\nduration_ms=2500\n[arrow]\ntail_first=true\n[menu]\nvisible_on_start=false\n",
        ));
        assert_eq!(
            custom,
            Settings {
                favorites: Favorites {
                    active: 0xff123456,
                    reserve: 0xffabcdef
                },
                thickness_px: 31,
                duration_ms: 2500,
                tail_first: true,
                visible_on_start: false,
                shortcuts: Shortcuts::default(),
                head_scale: 1.0,
            }
        );
        for n in [-1, 0, 2, 32, i64::MAX] {
            assert_eq!(
                parse(&document(&format!("[brush]\nthickness_px={n}"))).thickness_px,
                5
            );
        }
        for n in [-1, 0, 60001, i64::MAX] {
            assert_eq!(
                parse(&document(&format!("[temporary_pen]\nduration_ms={n}"))).duration_ms,
                1000
            );
        }
        for n in [1, 1000, 60000] {
            assert_eq!(
                parse(&document(&format!("[temporary_pen]\nduration_ms={n}"))).duration_ms,
                n as u64
            );
        }
        assert_eq!(
            parse(&document(
                "[brush]\nactive_color='bad'\nreserve_color='#12345'\nthickness_px='5'\n[temporary_pen]\nduration_ms=1.5\n[arrow]\ntail_first='true'\n[menu]\nvisible_on_start=0"
            )),
            Settings::default()
        );
        assert_eq!(
            parse(&document(
                "brush=1\ntemporary_pen=[]\narrow=false\nmenu='hidden'"
            )),
            Settings::default()
        );
    }

    #[test]
    fn settings_activation_key_defaults_and_validation() {
        assert_eq!(
            parse(&document(""))
                .shortcuts
                .binding("activation_key")
                .label(),
            "F4"
        );
        for number in 1..=11 {
            let settings = parse(&document(&format!(
                "[shortcuts]\nactivation_key = 'f{number}'\n[temporary_pen]\nduration_ms = 2500"
            )));
            assert_eq!(
                settings.shortcuts.binding("activation_key").label(),
                format!("F{number}")
            );
            assert_eq!(settings.duration_ms, 2500);
        }
        for invalid in ["12", "true", "[]", "'F12'", "'P'", "''", "'Ctrl+Alt+D'"] {
            let settings = parse(&document(&format!(
                "[shortcuts]\nactivation_key = {invalid}\n[temporary_pen]\nduration_ms = 2500"
            )));
            assert_eq!(settings.shortcuts.binding("activation_key").label(), "F4");
            assert_eq!(settings.duration_ms, 2500);
        }
        assert_eq!(
            parse(&document("shortcuts = false"))
                .shortcuts
                .binding("activation_key")
                .label(),
            "F4"
        );
    }

    #[test]
    fn settings_reload_validates_every_known_value_without_rewriting() {
        let directory =
            std::env::temp_dir().join(format!("mado-settings-reload-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        assert!(reload(&path).is_err());
        fs::write(&path, DEFAULT_DOCUMENT).unwrap();
        assert_eq!(reload(&path).unwrap(), Settings::default());
        for name in SHORTCUT_NAMES {
            let mut custom = document(DEFAULT_DOCUMENT);
            custom["shortcuts"][name] = Item::Value(Value::from("Ctrl+Alt+Shift+F11"));
            fs::write(&path, custom.to_string()).unwrap();
            let loaded = reload(&path).unwrap();
            assert_eq!(loaded.shortcuts.binding(name).label(), "Ctrl+Alt+Shift+F11");
            assert_eq!(fs::read_to_string(&path).unwrap(), custom.to_string());
            custom["shortcuts"][name] = Item::Value(Value::from("Ctrl+Ctrl+P"));
            fs::write(&path, custom.to_string()).unwrap();
            assert!(reload(&path).is_err(), "{name}");
            assert_eq!(fs::read_to_string(&path).unwrap(), custom.to_string());
        }
        for scale in ["0.1", "0.5", "1", "2.0", "4.0"] {
            fs::write(&path, format!("[arrow]\nhead_scale={scale}")).unwrap();
            assert_eq!(
                reload(&path).unwrap().head_scale,
                scale.parse::<f64>().unwrap()
            );
        }
        for invalid in [
            "[brush",
            "brush=1",
            "[brush]\nactive_color='bad'",
            "[brush]\nreserve_color='bad'",
            "[brush]\nthickness_px=2",
            "[temporary_pen]\nduration_ms=0",
            "[arrow]\nhead_scale=nan",
            "[arrow]\nhead_scale=inf",
            "[arrow]\nhead_scale=0.09",
            "[arrow]\nhead_scale=4.01",
            "[arrow]\nhead_scale='2'",
            "[arrow]\ntail_first=0",
            "[menu]\nvisible_on_start='false'",
            "[shortcuts]\npen='S'",
            "[shortcuts]\npen='F4'",
            "[shortcuts]\nactivation_key='Ctrl+Alt+D'",
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(reload(&path).is_err(), "{invalid}");
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
        let text = "# hand edited\n[shortcuts]\npen='Shift+P' # preserve\n[arrow]\nhead_scale=2.0\n[custom]\nvalue='keep'";
        fs::write(&path, text).unwrap();
        let loaded = reload(&path).unwrap();
        save_brush(&path, loaded.favorites, 11, true).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("pen='Shift+P' # preserve"));
        assert_eq!(reload(&path).unwrap().head_scale, 2.0);
        assert_eq!(document(&saved)["custom"]["value"].as_str(), Some("keep"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_load_completes_every_missing_section_and_key() {
        let directory =
            std::env::temp_dir().join(format!("mado-settings-completion-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let defaults = document(DEFAULT_DOCUMENT);
        let mut incomplete = vec![document("")];
        for (section, table) in defaults.iter() {
            let mut missing_section = defaults.clone();
            missing_section.remove(section);
            incomplete.push(missing_section);
            for (key, _) in table.as_table_like().unwrap().iter() {
                let mut missing_key = defaults.clone();
                missing_key[section]
                    .as_table_like_mut()
                    .unwrap()
                    .remove(key);
                incomplete.push(missing_key);
            }
        }
        for partial in incomplete {
            fs::write(&path, partial.to_string()).unwrap();
            assert_eq!(initialize(&path).unwrap(), Settings::default());
            let completed = fs::read_to_string(&path).unwrap();
            let parsed = document(&completed);
            for (section, table) in defaults.iter() {
                for (key, value) in table.as_table_like().unwrap().iter() {
                    assert_eq!(
                        parsed[section][key].to_string(),
                        value.to_string(),
                        "{section}.{key}"
                    );
                }
            }
            assert_eq!(initialize(&path).unwrap(), Settings::default());
            assert_eq!(fs::read_to_string(&path).unwrap(), completed);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_completion_preserves_custom_invalid_and_unknown_entries() {
        let directory = std::env::temp_dir().join(format!(
            "mado-settings-completion-preservation-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let old = "# my settings\n[brush]\nactive_color = '#0085DC'\nreserve_color = '#F97316'\nthickness_px = 21\n[temporary_pen]\nduration_ms = 2500 # my fade\n[arrow]\ntail_first = true\n[menu]\nvisible_on_start = false\n";
        for extra in ["", "[shortcuts]\ncustom = 'keep me'\n"] {
            let original = format!("{old}{extra}");
            fs::write(&path, &original).unwrap();
            let loaded = initialize(&path).unwrap();
            assert_eq!(loaded.shortcuts.binding("activation_key").label(), "F4");
            assert_eq!(loaded.thickness_px, 21);
            assert_eq!(loaded.favorites.active, 0xff0085dc);
            assert_eq!(loaded.favorites.reserve, 0xfff97316);
            assert_eq!(loaded.duration_ms, 2500);
            assert!(loaded.tail_first);
            assert!(!loaded.visible_on_start);
            let completed = fs::read_to_string(&path).unwrap();
            assert!(completed.contains("# my settings") && completed.contains("# my fade"));
            let before = document(&original);
            let after = document(&completed);
            for (section, item) in before.iter() {
                for (key, value) in item.as_table_like().unwrap().iter() {
                    assert_eq!(after[section][key].to_string(), value.to_string());
                }
            }
        }
        for existing in [
            "[shortcuts]\nactivation_key = 'F9' # chosen key\n",
            "[shortcuts]\nactivation_key = 'F12' # invalid but keep it\n",
            "shortcuts = false\n",
            "[shortcuts.activation_key]\ncustom = 1\n",
            "brush = 1\n",
            "[brush]\nthickness_px = 'invalid' # keep it\n",
            "brush = { thickness_px = 21, custom = 'keep me' }\n",
        ] {
            fs::write(&path, existing).unwrap();
            let loaded = initialize(&path).unwrap();
            let completed = fs::read_to_string(&path).unwrap();
            let before = document(existing);
            let after = document(&completed);
            for (section, item) in before.iter() {
                if let Some(table) = item.as_table_like() {
                    for (key, value) in table.iter() {
                        assert_eq!(after[section][key].to_string(), value.to_string());
                    }
                } else {
                    assert_eq!(after[section].to_string(), item.to_string());
                }
            }
            assert_eq!(initialize(&path).unwrap(), loaded);
            assert_eq!(fs::read_to_string(&path).unwrap(), completed);
        }
        fs::write(&path, "[shortcuts").unwrap();
        assert!(initialize(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[shortcuts");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_completion_write_failure_keeps_loaded_preferences_and_original_file() {
        let directory = std::env::temp_dir().join(format!(
            "mado-settings-completion-write-failure-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let original = "[brush]\nthickness_px = 21\n";
        fs::write(&path, original).unwrap();
        // Occupy the temporary path to force a write failure without changing permissions.
        let temporary = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        fs::write(&temporary, "occupied").unwrap();
        let loaded = initialize(&path).unwrap();
        assert_eq!(loaded.thickness_px, 21);
        assert_eq!(loaded.shortcuts.binding("activation_key").label(), "F4");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::write(&path, DEFAULT_DOCUMENT).unwrap();
        assert_eq!(initialize(&path).unwrap(), Settings::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_DOCUMENT);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn settings_migrate_once_preserve_manual_edits_and_never_overwrite_invalid_toml() {
        let directory =
            std::env::temp_dir().join(format!("mado-settings-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.toml");
        let legacy = "active=#123456\nreserve = #aBcDeF\nother=#000000\n";
        fs::write(directory.join("config.ini"), legacy).unwrap();
        let settings = initialize(&path).unwrap();
        assert_eq!(
            settings.favorites,
            Favorites {
                active: 0xff123456,
                reserve: 0xffabcdef
            }
        );
        assert_eq!(
            fs::read_to_string(directory.join("config.ini")).unwrap(),
            legacy
        );
        fs::write(directory.join("config.ini"), "active=#000000").unwrap();
        assert_eq!(initialize(&path).unwrap(), settings);
        let manual = "# personal settings\n[brush]\nactive_color = '#123456' # keep inline comment\nreserve_color = '#ABCDEF'\nthickness_px = 9\ncustom = 'unchanged'\n[temporary_pen]\nduration_ms = 3000 # my fade\n[arrow]\ntail_first = true\n[menu]\nvisible_on_start = false\n[shortcuts]\nactivation_key = 'F9' # my key\n[extra]\ntext = 'keep me'\n";
        fs::write(&path, manual).unwrap();
        save_brush(
            &path,
            Favorites {
                active: 0xff000001,
                reserve: 0xffabcdef,
            },
            9,
            true,
        )
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep inline comment"));
        assert!(text.contains("# personal settings"));
        assert!(text.contains("duration_ms = 3000 # my fade"));
        assert!(text.contains("activation_key = 'F9' # my key"));
        assert!(text.contains("reserve_color = '#ABCDEF'"));
        let loaded = initialize(&path).unwrap();
        assert_eq!(
            (
                loaded.thickness_px,
                loaded.duration_ms,
                loaded.tail_first,
                loaded.visible_on_start
            ),
            (9, 3000, true, false)
        );
        assert_eq!(document(&text)["extra"]["text"].as_str(), Some("keep me"));
        save_brush(&path, loaded.favorites, 15, true).unwrap();
        let restored = initialize(&path).unwrap();
        assert_eq!(restored.thickness_px, 15);
        assert_eq!(restored.shortcuts.binding("activation_key").label(), "F9");
        assert_eq!(
            (
                restored.duration_ms,
                restored.tail_first,
                restored.visible_on_start
            ),
            (3000, true, false)
        );
        let before = fs::read_to_string(&path).unwrap();
        for width in [0, 2, 32, u32::MAX] {
            assert!(save_brush(&path, loaded.favorites, width, true).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), before);
        }
        for bad in [
            "[brush",
            "brush = 1",
            "[brush.active_color]\ncustom=1",
            "[brush.thickness_px]\ncustom=1",
            "arrow=1",
            "[arrow.tail_first]\ncustom=1",
        ] {
            fs::write(&path, bad).unwrap();
            assert!(save_brush(&path, Favorites::default(), 5, false).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), bad);
        }
        fs::remove_file(&path).unwrap();
        fs::remove_file(directory.join("config.ini")).unwrap();
        assert_eq!(initialize(&path).unwrap(), Settings::default());
        fs::remove_dir_all(directory).unwrap();
    }
}
