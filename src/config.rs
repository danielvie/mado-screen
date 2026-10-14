// Favorite colors persist as `key=#RRGGBB` lines in ~/.mado/mado-screen/config.ini.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

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

pub fn path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".mado/mado-screen/config.ini"))
}

// A missing file is the first run. Unreadable or invalid entries fall back to defaults.
pub fn load(path: &Path) -> Favorites {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Favorites::default(),
        Err(error) => {
            eprintln!(
                "Could not read {}: {error}. Using default colors.",
                path.display()
            );
            Favorites::default()
        }
    }
}

// ponytail: a plain overwrite; a crash mid-write only resets two colors to defaults.
pub fn save(path: &Path, favorites: Favorites) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(path, format(favorites))
}

fn parse(text: &str) -> Favorites {
    let mut favorites = Favorites::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(color) = value
            .trim()
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        else {
            continue;
        };
        match key.trim() {
            "active" => favorites.active = 0xff000000 | color,
            "reserve" => favorites.reserve = 0xff000000 | color,
            _ => {}
        }
    }
    favorites
}

fn format(favorites: Favorites) -> String {
    format!(
        "active=#{:06X}\nreserve=#{:06X}\n",
        favorites.active & 0xffffff,
        favorites.reserve & 0xffffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_round_trip_and_reject_invalid_entries() {
        let custom = Favorites {
            active: 0xff123456,
            reserve: 0xffabcdef,
        };
        assert_eq!(format(custom), "active=#123456\nreserve=#ABCDEF\n");
        assert_eq!(parse(&format(custom)), custom);
        assert_eq!(parse(""), Favorites::default());
        assert_eq!(
            parse("active = #00ff00\nreserve=#+12345\nreserve=0085dc\nother=#111111\nnoise"),
            Favorites {
                active: 0xff00ff00,
                ..Favorites::default()
            }
        );
        let path = std::env::temp_dir()
            .join(format!("mado-config-test-{}", std::process::id()))
            .join("nested/config.ini");
        assert_eq!(load(&path), Favorites::default());
        save(&path, custom).unwrap();
        assert_eq!(load(&path), custom);
        fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }
}
