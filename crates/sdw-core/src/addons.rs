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
    /// Store bases the user added. Each one serves a catalogue of packages;
    /// adding a base does not install or trust anything by itself.
    pub repos: Vec<String>,
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
    /// Add a store base once, without a trailing slash. Returns true when it
    /// was new.
    pub fn add_repo(&mut self, url: &str) -> bool {
        let url = url.trim().trim_end_matches('/');
        if url.is_empty() || self.repos.iter().any(|r| r == url) {
            return false;
        }
        self.repos.push(url.to_string());
        true
    }
    pub fn remove_repo(&mut self, url: &str) {
        let url = url.trim().trim_end_matches('/');
        self.repos.retain(|r| r.trim_end_matches('/') != url);
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
    install_css_named(src, stem)
}

fn install_css_named(src: &Path, stem: &str) -> Result<Addon, String> {
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
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let name = src.file_stem().and_then(|s| s.to_str()).unwrap_or("addon");
    install_bytes(name, &bytes)
}

/// Install a package from bytes that came from a URL or a zip.
///
/// The format is sniffed instead of trusted to an extension: a zip starts with
/// `PK`, a manifest is JSON, everything else is treated as theme CSS. That is
/// what makes "paste a link" work for folders zipped on any OS.
pub fn install_bytes(name: &str, bytes: &[u8]) -> Result<Addon, String> {
    if bytes.is_empty() {
        return Err("пустой файл".into());
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return install_zip_bytes(name, bytes);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "файл не похож на аддон".to_string())?;
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        let tmp =
            std::env::temp_dir().join(format!("sdw-addon-{}.json", crate::model::random_id(6)));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        let result = install_manifest(&tmp);
        let _ = std::fs::remove_file(&tmp);
        return result;
    }
    let tmp = std::env::temp_dir().join(format!("sdw-addon-{}.css", crate::model::random_id(6)));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let css_name = if name.trim().is_empty() {
        "theme"
    } else {
        name
    };
    let result = install_css_named(&tmp, css_name);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn install_zip_bytes(name: &str, bytes: &[u8]) -> Result<Addon, String> {
    let tmp = std::env::temp_dir().join(format!(
        "sdw-addon-{}-{}",
        crate::model::random_id(6),
        clean_id(name).unwrap_or_else(|_| "addon".into())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = extract_zip(bytes, &tmp).and_then(|()| {
        if tmp.join(MANIFEST).is_file() {
            return install_dir(&tmp);
        }
        // A zip may keep everything under one folder, or the manifest may sit
        // next to a README at the root of a repository archive.
        if let Ok(entries) = std::fs::read_dir(&tmp) {
            for entry in entries.flatten() {
                if entry.path().join(MANIFEST).is_file() {
                    return install_dir(&entry.path());
                }
            }
        }
        Err(format!("в архиве нет {MANIFEST}"))
    });
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Unpack a zip without an external `unzip`, rejecting absolute paths, `..`
/// escapes and archives that expand to something unreasonable.
fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("архив: {e}"))?;
    if archive.len() > 4000 {
        return Err("в архиве слишком много файлов".into());
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = file.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        total = total.saturating_add(file.size());
        if total > 64 * 1024 * 1024 {
            return Err("архив распаковывается больше чем на 64 МБ".into());
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut f).map_err(|e| e.to_string())?;
    }
    Ok(())
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
                    // {name} comes from the setting below, expanded per run.
                    command: "notify-send sd-widgets 'hello from an addon, {name}'".into(),
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

/// Values available to an addon's templates: each setting as `{key}` and as
/// `{settings.key}`, so a manifest reads naturally.
pub fn variables(cfg: &AddonsCfg, id: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    if let Some(values) = cfg.values.get(id) {
        for (key, value) in values {
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            vars.insert(key.clone(), text.clone());
            vars.insert(format!("settings.{key}"), text);
        }
    }
    vars
}

/// Fill `{key}` in every string of an action from the addon's settings. Called
/// right before the action runs, so editing a setting changes the next call
/// without reinstalling anything.
pub fn expand_action(action: &Action, vars: &BTreeMap<String, String>) -> Action {
    if vars.is_empty() {
        return action.clone();
    }
    let x = |text: &str| crate::util::expand(text, vars);
    match action {
        Action::Shell { command } => Action::Shell {
            command: x(command),
        },
        Action::Launch { target } => Action::Launch { target: x(target) },
        Action::Media { command } => Action::Media {
            command: x(command),
        },
        Action::Key { combo } => Action::Key { combo: x(combo) },
        Action::Url { url } => Action::Url { url: x(url) },
        Action::Remote { host, command } => Action::Remote {
            host: host.clone(),
            command: x(command),
        },
        Action::Ha {
            host,
            domain,
            service,
            entity,
            data,
        } => Action::Ha {
            host: host.clone(),
            domain: domain.clone(),
            service: service.clone(),
            entity: x(entity),
            data: expand_value(data, vars),
        },
        Action::Http {
            method,
            url,
            headers,
            body,
        } => Action::Http {
            method: method.clone(),
            url: x(url),
            headers: headers.iter().map(|(k, v)| (k.clone(), x(v))).collect(),
            body: x(body),
        },
        other => other.clone(),
    }
}

fn expand_value(value: &Value, vars: &BTreeMap<String, String>) -> Value {
    match value {
        Value::String(s) => Value::String(crate::util::expand(s, vars)),
        Value::Array(list) => Value::Array(list.iter().map(|v| expand_value(v, vars)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), expand_value(v, vars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// ---------------------------------------------------------------- store

/// One package listed by a store repository.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub scope: String,
    pub homepage: String,
    /// Where the package lives: an absolute URL or a path inside the repo.
    pub url: String,
    /// Optional SHA-256 of the download, verified before installing.
    pub sha256: String,
}

/// A store repository: a static JSON catalogue plus a base URL.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoCatalog {
    pub name: String,
    pub url: String,
    pub addons: Vec<CatalogEntry>,
}

/// Names tried under a store base, in order. The first two are obvious; the
/// last two match static hosts that expose a small API tree.
const CATALOG_NAMES: [&str; 4] = [
    "repo.json",
    "index.json",
    "api/v1/addons.json",
    "addons.json",
];

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("sd-widgets/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn get_text(url: &str) -> Result<String, String> {
    let mut r = agent()
        .get(url)
        .header("Accept", "application/json, text/plain")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(format!("{url}: HTTP {status}"));
    }
    Ok(text)
}

fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut r = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let status = r.status().as_u16();
    if status != 200 {
        return Err(format!("{url}: HTTP {status}"));
    }
    r.body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))
}

/// Name of a package taken from its URL: the file stem, without a doubled
/// `.sdwaddon` suffix or a query string.
fn name_from_url(url: &str) -> String {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/');
    let stem = path.rsplit('/').next().unwrap_or("addon");
    let stem = stem
        .strip_suffix(".sdwaddon")
        .or_else(|| stem.strip_suffix(".zip"))
        .or_else(|| stem.strip_suffix(".json"))
        .or_else(|| stem.strip_suffix(".css"))
        .unwrap_or(stem);
    if stem.is_empty() {
        "addon".into()
    } else {
        stem.to_string()
    }
}

fn join_url(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        rel.trim_start_matches('/')
    )
}

/// Parse a catalogue document: either `{ "addons": [...] }` (or the older
/// `extensions` spelling) or a bare array. Relative package URLs are resolved
/// against the repository base.
pub fn parse_catalog(base: &str, text: &str) -> Result<RepoCatalog, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("каталог: {e}"))?;
    let (name, list) = match &value {
        Value::Array(list) => (String::new(), list.clone()),
        Value::Object(map) => {
            let list = map
                .get("addons")
                .or_else(|| map.get("extensions"))
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| "в каталоге нет поля addons".to_string())?;
            (
                map.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                list,
            )
        }
        _ => return Err("каталог должен быть объектом или списком".into()),
    };
    let mut addons = Vec::new();
    for item in list {
        let mut entry: CatalogEntry =
            serde_json::from_value(item).map_err(|e| format!("элемент каталога: {e}"))?;
        if entry.id.trim().is_empty() {
            entry.id = name_from_url(&entry.url);
        }
        if entry.name.trim().is_empty() {
            entry.name = entry.id.clone();
        }
        if entry.url.trim().is_empty() {
            // A repo may list only ids and serve the packages from the usual
            // folder; the convention keeps the catalogue short.
            entry.url = format!("extensions/{}.sdwaddon", entry.id);
        }
        entry.url = join_url(base, &entry.url);
        if entry.id.trim().is_empty() || entry.url.trim().is_empty() {
            continue;
        }
        addons.push(entry);
    }
    Ok(RepoCatalog {
        name: if name.trim().is_empty() {
            base.to_string()
        } else {
            name
        },
        url: base.to_string(),
        addons,
    })
}

/// Fetch and parse the catalogue of a store base.
pub fn fetch_repo(base: &str) -> Result<RepoCatalog, String> {
    let base = base.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("адрес магазина должен начинаться с http:// или https://".into());
    }
    let candidates: Vec<String> = if base.ends_with(".json") {
        vec![base.to_string()]
    } else {
        CATALOG_NAMES.iter().map(|n| join_url(base, n)).collect()
    };
    let mut errors = Vec::new();
    for url in candidates {
        match get_text(&url) {
            Ok(text) => match parse_catalog(base, &text) {
                Ok(catalog) => return Ok(catalog),
                Err(e) => errors.push(format!("{url}: {e}")),
            },
            Err(e) => errors.push(e),
        }
    }
    let detail = errors.first().cloned().unwrap_or_default();
    Err(format!(
        "не удалось прочитать каталог {base}{}",
        if detail.is_empty() {
            String::new()
        } else {
            format!(" ({detail})")
        }
    ))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Download and install a package from any http(s) link. The bytes decide the
/// format: zip, manifest JSON or theme CSS.
pub fn install_url(url: &str) -> Result<Addon, String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("ссылка должна начинаться с http:// или https://".into());
    }
    let bytes = download(url, 32 * 1024 * 1024)?;
    install_bytes(&name_from_url(url), &bytes)
}

/// Install one catalogue entry, checking its checksum when the repo declares
/// one. A mismatch means the download is not what the catalogue promised.
pub fn install_entry(entry: &CatalogEntry) -> Result<Addon, String> {
    let url = entry.url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("ссылка должна начинаться с http:// или https://".into());
    }
    let bytes = download(url, 32 * 1024 * 1024)?;
    if !entry.sha256.trim().is_empty() {
        use sha2::{Digest, Sha256};
        let actual = hex(&Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(entry.sha256.trim()) {
            return Err(format!(
                "контрольная сумма не совпала: ожидалось {}, получено {actual}",
                entry.sha256.trim()
            ));
        }
    }
    install_bytes(&name_from_url(url), &bytes)
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
    fn catalog_resolves_relative_urls() {
        let text = r#"{"name":"Test repo","addons":[
            {"id":"lamp","name":"Lamp","url":"packs/lamp.sdwaddon"},
            {"id":"clock","version":"2","sha256":"00"}
        ]}"#;
        let catalog = parse_catalog("https://example.org/store", text).unwrap();
        assert_eq!(catalog.name, "Test repo");
        assert_eq!(catalog.addons.len(), 2);
        assert_eq!(
            catalog.addons[0].url,
            "https://example.org/store/packs/lamp.sdwaddon"
        );
        assert_eq!(
            catalog.addons[1].url,
            "https://example.org/store/extensions/clock.sdwaddon"
        );
        assert_eq!(catalog.addons[1].name, "clock");
    }

    #[test]
    fn catalog_accepts_a_bare_list() {
        let text = r#"[{"name":"A","url":"https://cdn.example/a.zip"}]"#;
        let catalog = parse_catalog("https://example.org", text).unwrap();
        assert_eq!(catalog.addons.len(), 1);
        assert_eq!(catalog.addons[0].id, "a");
    }

    #[test]
    fn package_names_come_from_the_url() {
        assert_eq!(
            name_from_url("https://x/y/My-Pack.sdwaddon?token=1"),
            "My-Pack"
        );
        assert_eq!(name_from_url("https://x/y/theme.css"), "theme");
        assert_eq!(name_from_url("https://x/y/"), "y");
    }

    #[test]
    fn fetches_a_catalogue_over_http() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let body = r#"{"name":"t","addons":[{"id":"x","url":"x.sdwaddon"}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        let catalog = fetch_repo(&base).unwrap();
        assert_eq!(catalog.addons.len(), 1);
        assert_eq!(catalog.addons[0].url, format!("{base}/x.sdwaddon"));
    }

    #[test]
    fn repos_are_added_once() {
        let mut cfg = AddonsCfg::default();
        assert!(cfg.add_repo("https://store.example/"));
        assert!(!cfg.add_repo("https://store.example"));
        assert_eq!(cfg.repos, vec!["https://store.example"]);
        cfg.remove_repo("https://store.example/");
        assert!(cfg.repos.is_empty());
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
