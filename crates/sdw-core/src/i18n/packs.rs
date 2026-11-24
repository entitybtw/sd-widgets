//! User language packs.
//!
//! A pack is one TOML file in the `lang` folder of the config directory. It
//! maps a line the app already writes to a line in another language:
//!
//! ```toml
//! code = "de"
//! name = "Deutsch"
//! author = "someone"
//!
//! [strings]
//! "Обзор" = "Übersicht"
//! "Overview" = "Übersicht"
//! ```
//!
//! A key may be the Russian text or the English one; both are written in the
//! same `t!` call, so a translator can work from whichever is easier. A line
//! the pack does not mention keeps the built-in text, so a half-finished pack
//! is still usable.
//!
//! The table is kept in memory as `&'static str` pairs (leaked once at load):
//! `tr` is called for every label on every frame and must not allocate. A
//! reload leaks the old copy; packs are small and reloads are rare.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// A pack as the window shows it: enough to list and select it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackInfo {
    pub code: String,
    pub name: String,
    pub author: String,
    /// How many lines the pack translates.
    pub lines: usize,
}

struct Pack {
    info: PackInfo,
    strings: HashMap<&'static str, &'static str>,
}

#[derive(Default)]
struct Store {
    packs: Vec<Pack>,
    active: Option<Pack>,
    errors: Vec<String>,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| Mutex::new(Store::default()))
}

/// The folder the app reads packs from.
pub fn dir() -> PathBuf {
    crate::util::config_dir().join("lang")
}

/// What a pack file looks like.
#[derive(Deserialize)]
struct File {
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    strings: HashMap<String, String>,
}

/// Parse one pack. `fallback` is the file stem, used when the file has no code.
fn parse(text: &str, fallback: &str) -> Result<Pack, String> {
    let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
    let code = file.code.trim();
    let code = if code.is_empty() { fallback } else { code };
    let code = code.trim().to_ascii_lowercase();
    if code.is_empty() {
        return Err("no code".into());
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("bad code: {code}"));
    }
    let name = if file.name.trim().is_empty() {
        code.to_uppercase()
    } else {
        file.name.trim().to_string()
    };
    let mut strings = HashMap::with_capacity(file.strings.len());
    for (key, value) in file.strings {
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        let key: &'static str = Box::leak(key.to_string().into_boxed_str());
        let value: &'static str = Box::leak(value.to_string().into_boxed_str());
        strings.insert(key, value);
    }
    Ok(Pack {
        info: PackInfo {
            code,
            name,
            author: file.author.trim().to_string(),
            lines: strings.len(),
        },
        strings,
    })
}

/// Read every pack in [`dir`]. Missing folder is not an error: most machines
/// never add one.
pub fn load() -> Vec<PackInfo> {
    let mut loaded: Vec<Pack> = Vec::new();
    let mut errors = Vec::new();
    let dir = dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        if let Ok(mut store) = store().lock() {
            store.packs.clear();
            store.errors.clear();
            store.active = None;
        }
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("toml"))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for path in paths {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        match parse(&text, &stem) {
            Ok(pack) => {
                if loaded.iter().any(|p| p.info.code == pack.info.code) {
                    errors.push(format!(
                        "{}: повторяется код {}",
                        path.display(),
                        pack.info.code
                    ));
                    continue;
                }
                loaded.push(pack);
            }
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    loaded.sort_by(|a, b| a.info.name.to_lowercase().cmp(&b.info.name.to_lowercase()));
    let infos = loaded.iter().map(|p| p.info.clone()).collect();
    if let Ok(mut store) = store().lock() {
        // Keep the active pack pointing at the fresh copy with the same code.
        let active = store
            .active
            .as_ref()
            .map(|a| a.info.code.clone())
            .and_then(|code| loaded.iter().position(|p| p.info.code == code));
        store.active = active.map(|index| Pack {
            info: loaded[index].info.clone(),
            strings: loaded[index].strings.clone(),
        });
        store.packs = loaded;
        store.errors = errors;
    }
    infos
}

/// Every loaded pack, for the settings page.
pub fn list() -> Vec<PackInfo> {
    store()
        .lock()
        .map(|s| s.packs.iter().map(|p| p.info.clone()).collect())
        .unwrap_or_default()
}

/// Problems met while reading the folder, shown in the window so a broken
/// file is not silently ignored.
pub fn errors() -> Vec<String> {
    store().lock().map(|s| s.errors.clone()).unwrap_or_default()
}

pub fn is_pack(code: &str) -> bool {
    let code = code.trim().to_ascii_lowercase();
    store()
        .lock()
        .map(|s| s.packs.iter().any(|p| p.info.code == code))
        .unwrap_or(false)
}

/// Make a pack active, or turn all packs off with `None`.
pub fn set_active(code: Option<&str>) {
    let Ok(mut store) = store().lock() else {
        return;
    };
    store.active = code.and_then(|code| {
        let code = code.trim().to_ascii_lowercase();
        store
            .packs
            .iter()
            .find(|p| p.info.code == code)
            .map(|p| Pack {
                info: p.info.clone(),
                strings: p.strings.clone(),
            })
    });
}

pub fn active() -> Option<String> {
    store()
        .lock()
        .ok()
        .and_then(|s| s.active.as_ref().map(|p| p.info.code.clone()))
}

/// The translation of a `t!` pair, when an active pack has one.
///
/// The Russian text is tried first: it is the key of the Ukrainian table, so a
/// pack derived from it translates every line the app already has.
pub(crate) fn lookup(ru: &str, en: &str) -> Option<&'static str> {
    let store = store().lock().ok()?;
    let pack = store.active.as_ref()?;
    pack.strings
        .get(ru)
        .or_else(|| pack.strings.get(en))
        .copied()
}

/// A ready-to-fill template with every line the app can translate.
pub fn template() -> String {
    let mut out = String::from(
        "# sd-widgets language pack\n\
         # Fill the right-hand side; an empty line keeps the built-in text.\n\
         # Put this file into the folder below and press \"reload\" in settings.\n\
         code = \"xx\"\n\
         name = \"Your language\"\n\
         author = \"you\"\n\
         \n\
         [strings]\n",
    );
    for key in super::uk::keys() {
        out.push_str(&format!("\"{}\" = \"\"\n", escape(key)));
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_reads_keys_in_both_languages() {
        let pack = parse(
            "code = \"de\"\nname = \"Deutsch\"\nauthor = \"me\"\n\n[strings]\n\
             \"Обзор\" = \"Übersicht\"\n\"Timer\" = \"Zeitgeber\"\n",
            "de",
        )
        .expect("pack parses");
        assert_eq!(pack.info.code, "de");
        assert_eq!(pack.info.name, "Deutsch");
        assert_eq!(pack.info.lines, 2);
        assert_eq!(pack.strings.get("Обзор"), Some(&"Übersicht"));
        assert_eq!(pack.strings.get("Timer"), Some(&"Zeitgeber"));
    }

    #[test]
    fn a_file_without_a_code_uses_its_name() {
        let pack = parse("[strings]\n\"Обзор\" = \"X\"\n", "pl").expect("pack parses");
        assert_eq!(pack.info.code, "pl");
        assert_eq!(pack.info.name, "PL");
    }

    #[test]
    fn empty_lines_are_dropped_and_bad_files_reported() {
        let pack = parse("[strings]\n\"a\" = \"\"\n\"b\" = \"c\"\n", "x").unwrap();
        assert_eq!(pack.info.lines, 1);
        assert!(parse("code = \"bad code!\"", "x").is_err());
        assert!(parse("this is not toml", "x").is_err());
    }

    #[test]
    fn the_template_has_every_translatable_line() {
        let text = template();
        assert!(text.contains("[strings]"));
        for key in super::super::uk::keys().iter().take(5) {
            assert!(
                text.contains(&format!("\"{key}\" = \"\"")),
                "template misses {key}"
            );
        }
    }
}
