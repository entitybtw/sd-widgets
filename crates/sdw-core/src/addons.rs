//! Declarative addon packages.
//!
//! An addon is a folder (or a single JSON/CSS file) in `~/.config/sd-widgets/addons`.
//! It carries a manifest and optional assets: a theme, a clock preset, home tiles,
//! sound files and daemon actions. Both the deck and the daemon read the same
//! format, so one package can extend either side or both.

use crate::model::{Action, ClockCfg};
use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "addon.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AddonManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub homepage: String,
    /// `deck`, `daemon` or `both`; informational, both sides honour it.
    pub scope: String,
    pub deck: DeckAddon,
    pub daemon: DaemonAddon,
    pub settings: Vec<AddonField>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeckAddon {
    /// Theme CSS file, relative to the addon folder.
    pub theme: String,
    /// Clock preset JSON file, relative to the addon folder.
    pub clock: String,
    /// Tiles added to the home grid.
    pub tiles: Vec<AddonTile>,
    /// Sound cue -> file, relative to the addon folder.
    pub sounds: BTreeMap<String, String>,
    /// Extra CSS appended after the theme.
    pub extra_css: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonAddon {
    pub actions: Vec<AddonAction>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AddonTile {
    pub title: String,
    pub subtitle: String,
    /// Material Symbols codepoint or a short text glyph.
    pub icon: String,
    pub accent: String,
    pub action: Action,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AddonAction {
    pub id: String,
    pub label: String,
    pub description: String,
    pub confirm: bool,
    pub action: Action,
}

/// One configurable value of an addon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AddonField {
    pub key: String,
    pub label: String,
    /// `text`, `number`, `bool`, `color` or `choice`.
    pub kind: String,
    pub default: Value,
    pub options: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub hint: String,
}

impl Default for AddonField {
    fn default() -> Self {
        Self {
            key: String::new(),
            label: String::new(),
            kind: "text".into(),
            default: Value::Null,
            options: Vec::new(),
            min: None,
            max: None,
            hint: String::new(),
        }
    }
}

/// Per-user state: which addons are on and what values the user gave them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AddonsCfg {
    pub enabled: Vec<String>,
    pub values: BTreeMap<String, BTreeMap<String, Value>>,
}

impl AddonsCfg {
    pub fn is_on(&self, id: &str) -> bool {
        self.enabled.iter().any(|x| x == id)
    }
    pub fn set(&mut self, id: &str, on: bool) {
        if on {
            if !self.is_on(id) {
                self.enabled.push(id.to_string());
            }
        } else {
            self.enabled.retain(|x| x != id);
        }
    }
    pub fn value(&self, id: &str, key: &str) -> Value {
        self.values
            .get(id)
            .and_then(|m| m.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    }
    pub fn set_value(&mut self, id: &str, key: &str, value: Value) {
        self.values
            .entry(id.to_string())
            .or_default()
            .insert(key.to_string(), value);
    }
}

/// A manifest plus the folder that holds its assets.
#[derive(Debug, Clone)]
pub struct Addon {
    pub dir: PathBuf,
    pub manifest: AddonManifest,
}

impl Addon {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn theme_css(&self) -> Option<String> {
        if self.manifest.deck.theme.trim().is_empty() {
            return None;
        }
        let css = std::fs::read_to_string(self.resolve(&self.manifest.deck.theme)).ok()?;
        let extra = &self.manifest.deck.extra_css;
        if extra.is_empty() {
            Some(css)
        } else {
            Some(format!("{css}\n{extra}"))
        }
    }

    pub fn clock(&self) -> Option<ClockCfg> {
        if self.manifest.deck.clock.trim().is_empty() {
            return None;
        }
        let raw = std::fs::read_to_string(self.resolve(&self.manifest.deck.clock)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn sound_file(&self, cue: &str) -> Option<PathBuf> {
        let rel = self.manifest.deck.sounds.get(cue)?;
        let path = self.resolve(rel);
        path.is_file().then_some(path)
    }

    fn resolve(&self, rel: &str) -> PathBuf {
        let path = Path::new(rel);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.dir.join(path)
        }
    }
}

/// Where addons live: `~/.config/sd-widgets/addons`.
pub fn root() -> PathBuf {
    util::config_dir().join("addons")
}

/// Every installed addon, sorted by name. Broken manifests are skipped.
pub fn list() -> Vec<Addon> {
    let dir = root();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let file = path.join(MANIFEST);
        let Ok(raw) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(mut manifest) = serde_json::from_str::<AddonManifest>(&raw) else {
            continue;
        };
        if manifest.id.trim().is_empty() {
            manifest.id = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("addon")
                .to_string();
        }
        out.push(Addon {
            dir: path,
            manifest,
        });
    }
    out.sort_by(|a, b| {
        a.manifest
            .name
            .to_lowercase()
            .cmp(&b.manifest.name.to_lowercase())
    });
    out
}

pub fn find(id: &str) -> Option<Addon> {
    list().into_iter().find(|a| a.id() == id)
}

/// Install an addon from a folder, a manifest JSON, a CSS file or a `.sdwaddon`
/// zip (unpacked with the system `unzip`). Returns the installed addon.
pub fn install(src: &Path) -> Result<Addon, String> {
    if !src.exists() {
        return Err(format!("нет такого пути: {}", src.display()));
    }
    if src.is_dir() {
        return install_dir(src);
    }
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "json" => install_manifest(src),
        "css" => install_css(src),
        "sdwaddon" | "zip" => install_zip(src),
        _ => Err("поддерживаются папка, addon.json, .css и .sdwaddon/.zip".into()),
    }
}

fn install_dir(src: &Path) -> Result<Addon, String> {
    let manifest_path = src.join(MANIFEST);
    if !manifest_path.is_file() {
        return Err(format!("в папке нет {MANIFEST}"));
    }
    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: AddonManifest =
        serde_json::from_str(&raw).map_err(|e| format!("addon.json: {e}"))?;
    let id = clean_id(&manifest.id)?;
    let dest = root().join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    copy_dir(src, &dest)?;
    let mut manifest = manifest;
    manifest.id = id;
    write_manifest(&dest, &manifest)?;
    load_at(dest)
}

fn install_manifest(src: &Path) -> Result<Addon, String> {
    let raw = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
    let manifest: AddonManifest =
        serde_json::from_str(&raw).map_err(|e| format!("addon.json: {e}"))?;
    let id = clean_id(&manifest.id)?;
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let dest = root().join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    // Copy the assets the manifest points at, keeping their relative names.
    let mut assets: Vec<&str> = Vec::new();
    if !manifest.deck.theme.is_empty() {
        assets.push(&manifest.deck.theme);
    }
    if !manifest.deck.clock.is_empty() {
        assets.push(&manifest.deck.clock);
    }
    for rel in manifest.deck.sounds.values() {
        assets.push(rel);
    }
    for rel in assets {
        let from = parent.join(rel);
        let to = dest.join(rel);
        if from.is_file() {
            if let Some(dir) = to.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::copy(&from, &to);
        }
    }
    let mut manifest = manifest;
    manifest.id = id;
    write_manifest(&dest, &manifest)?;
    load_at(dest)
}

fn install_css(src: &Path) -> Result<Addon, String> {
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("theme");
    let id = clean_id(stem)?;
    let dest = root().join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    std::fs::copy(src, dest.join("theme.css")).map_err(|e| e.to_string())?;
    let manifest = AddonManifest {
        id: id.clone(),
        name: stem.to_string(),
        version: "1.0.0".into(),
        scope: "deck".into(),
        deck: DeckAddon {
            theme: "theme.css".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    write_manifest(&dest, &manifest)?;
    load_at(dest)
}

fn install_zip(src: &Path) -> Result<Addon, String> {
    if !util::has("unzip") {
        return Err("для .sdwaddon нужен unzip в PATH (или распакуйте и укажите папку)".into());
    }
    let tmp = std::env::temp_dir().join(format!("sdw-addon-{}", crate::model::random_id(6)));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    util::run(
        "unzip",
        &[
            "-o",
            &src.display().to_string(),
            "-d",
            &tmp.display().to_string(),
        ],
    )
    .map_err(|e| format!("unzip: {e}"))?;
    // A zip may keep everything under one folder.
    if tmp.join(MANIFEST).is_file() {
        return install_dir(&tmp);
    }
    if let Ok(entries) = std::fs::read_dir(&tmp) {
        for entry in entries.flatten() {
            if entry.path().join(MANIFEST).is_file() {
                return install_dir(&entry.path());
            }
        }
    }
    Err(format!("в архиве нет {MANIFEST}"))
}

fn load_at(dir: PathBuf) -> Result<Addon, String> {
    let raw = std::fs::read_to_string(dir.join(MANIFEST)).map_err(|e| e.to_string())?;
    let manifest = serde_json::from_str(&raw).map_err(|e| format!("addon.json: {e}"))?;
    Ok(Addon { dir, manifest })
}

fn write_manifest(dir: &Path, manifest: &AddonManifest) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MANIFEST), text).map_err(|e| e.to_string())
}

pub fn remove(id: &str) -> Result<(), String> {
    let id = clean_id(id)?;
    let dir = root().join(id);
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Save the current theme CSS as a small theme addon.
pub fn create_theme(name: &str, css: &str) -> Result<Addon, String> {
    let id = clean_id(name)?;
    let dest = root().join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("theme.css"), css).map_err(|e| e.to_string())?;
    let manifest = AddonManifest {
        id: id.clone(),
        name: name.to_string(),
        version: "1.0.0".into(),
        author: "you".into(),
        description: "тема, сохранённая из приложения".into(),
        scope: "deck".into(),
        deck: DeckAddon {
            theme: "theme.css".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    write_manifest(&dest, &manifest)?;
    load_at(dest)
}

/// Save the current clock as a clock addon.
pub fn create_clock(name: &str, cfg: &ClockCfg) -> Result<Addon, String> {
    let id = clean_id(name)?;
    let dest = root().join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("clock.json"), json).map_err(|e| e.to_string())?;
    let manifest = AddonManifest {
        id: id.clone(),
        name: name.to_string(),
        version: "1.0.0".into(),
        author: "you".into(),
        description: "часы, сохранённые из приложения".into(),
        scope: "deck".into(),
        deck: DeckAddon {
            clock: "clock.json".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    write_manifest(&dest, &manifest)?;
    load_at(dest)
}

/// A complete example the user can copy, import and edit.
pub fn example_manifest() -> AddonManifest {
    AddonManifest {
        id: "example".into(),
        name: "Example addon".into(),
        version: "1.0.0".into(),
        author: "you".into(),
        description: "Tiles, a theme and a daemon action in one file.".into(),
        homepage: String::new(),
        scope: "both".into(),
        deck: DeckAddon {
            theme: String::new(),
            clock: String::new(),
            tiles: vec![AddonTile {
                title: "Open the site".into(),
                subtitle: "a deck-side tile".into(),
                icon: "\u{e8b6}".into(),
                accent: "#3fd2ff".into(),
                action: Action::Url {
                    url: "https://git.entitybtw.ru/entitybtw/sd-widgets".into(),
                },
            }],
            sounds: BTreeMap::new(),
            extra_css: String::new(),
        },
        daemon: DaemonAddon {
            actions: vec![AddonAction {
                id: "notify".into(),
                label: "Send a test notification".into(),
                description: "runs on the computer".into(),
                confirm: false,
                action: Action::Shell {
                    command: "notify-send sd-widgets 'hello from an addon'".into(),
                },
            }],
        },
        settings: vec![AddonField {
            key: "name".into(),
            label: "Your name".into(),
            kind: "text".into(),
            default: Value::String("deck".into()),
            ..Default::default()
        }],
    }
}

fn clean_id(raw: &str) -> Result<String, String> {
    let id: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let id = id.trim_matches(['-', '.', '_']).to_string();
    if id.is_empty() {
        return Err("у аддона должно быть имя из букв, цифр, '-', '_' или '.'".into());
    }
    if id.contains("..") {
        return Err("недопустимое имя аддона".into());
    }
    Ok(id)
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else if from.is_file() {
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Addon action info for the API/status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddonInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub scope: String,
    pub enabled: bool,
    pub actions: Vec<AddonActionInfo>,
    pub tiles: u32,
    pub has_theme: bool,
    pub has_clock: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddonActionInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub confirm: bool,
}

pub fn info(cfg: &AddonsCfg) -> Vec<AddonInfo> {
    list()
        .into_iter()
        .map(|a| AddonInfo {
            id: a.manifest.id.clone(),
            name: a.manifest.name.clone(),
            version: a.manifest.version.clone(),
            description: a.manifest.description.clone(),
            scope: a.manifest.scope.clone(),
            enabled: cfg.is_on(&a.manifest.id),
            actions: a
                .manifest
                .daemon
                .actions
                .iter()
                .map(|x| AddonActionInfo {
                    id: x.id.clone(),
                    label: x.label.clone(),
                    description: x.description.clone(),
                    confirm: x.confirm,
                })
                .collect(),
            tiles: a.manifest.deck.tiles.len() as u32,
            has_theme: !a.manifest.deck.theme.trim().is_empty(),
            has_clock: !a.manifest.deck.clock.trim().is_empty(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_manifest_round_trips() {
        let manifest = example_manifest();
        let text = serde_json::to_string_pretty(&manifest).expect("serialize");
        let back: AddonManifest = serde_json::from_str(&text).expect("parse");
        assert_eq!(back.id, "example");
        assert_eq!(back.deck.tiles.len(), 1);
        assert_eq!(back.daemon.actions.len(), 1);
        assert_eq!(back.settings.len(), 1);
    }

    #[test]
    fn ids_become_file_safe() {
        assert_eq!(clean_id(" My Theme ").unwrap(), "my-theme");
        assert_eq!(clean_id("theme.v2").unwrap(), "theme.v2");
        assert!(clean_id("..").is_err());
        assert!(clean_id("").is_err());
    }

    #[test]
    fn addon_config_tracks_state() {
        let mut cfg = AddonsCfg::default();
        assert!(!cfg.is_on("x"));
        cfg.set("x", true);
        assert!(cfg.is_on("x"));
        cfg.set("x", false);
        assert!(!cfg.is_on("x"));
        cfg.set_value("x", "name", serde_json::json!("deck"));
        assert_eq!(cfg.value("x", "name"), serde_json::json!("deck"));
        assert!(cfg.value("x", "missing").is_null());
    }
}
