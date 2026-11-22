//! Persisted Deck-side configuration.

use crate::model::DeckConfig;
use std::path::PathBuf;

pub fn path() -> PathBuf {
    crate::util::config_dir().join("deck.json")
}

pub fn load() -> DeckConfig {
    let p = path();
    match std::fs::read_to_string(&p) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(cfg) => migrate(cfg),
            Err(e) => {
                eprintln!(
                    "sd-widgets: ignoring unreadable config {}: {e}",
                    p.display()
                );
                DeckConfig::default()
            }
        },
        Err(_) => DeckConfig::default(),
    }
}

/// Older files pinned the clock to the built-in blue/slate palette, and older
/// clock presets stored the badge as a readable name ("star") instead of the
/// Material symbol. The palette is handed back to the theme and the name to its
/// glyph; a value the user actually chose is left alone.
pub fn migrate(mut cfg: DeckConfig) -> DeckConfig {
    let clock = &mut cfg.clock;
    let fixed = crate::model::clock_icon(&clock.icon).map(str::to_string);
    match fixed {
        Some(glyph) if glyph != clock.icon => clock.icon = glyph,
        // Unknown text would paint letters into a 26 px badge; no icon is
        // better than a clipped one.
        None if !clock.icon.trim().is_empty() => clock.icon.clear(),
        _ => {}
    }
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b);
    if !clock.mono
        && !clock.follow_theme
        && same(&clock.accent, "#a6c8ff")
        && same(&clock.bg_top, "#121820")
        && same(&clock.bg_bottom, "#10141b")
    {
        clock.follow_theme = true;
        clock.accent.clear();
        clock.bg_top.clear();
        clock.bg_bottom.clear();
    }
    cfg
}

/// Write a field of the config file without touching the rest.
///
/// The file belongs to the deck app, which keeps it in memory and writes it
/// whole; another process that changed one field would be overwritten by the
/// next save. This reads the file, replaces the one line and writes it back,
/// so the two writers never lose each other's work.
pub fn set_field(
    path: &std::path::Path,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| "deck.json is not an object".to_string())?;
    obj.insert(key.to_string(), value);
    let out = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    crate::util::atomic_write(path, &out).map_err(|e| e.to_string())
}

pub fn save(cfg: &DeckConfig) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    crate::util::atomic_write(&path(), &json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_follows_theme_by_default() {
        let cfg = DeckConfig::default();
        assert!(cfg.clock.follow_theme);
        assert!(cfg.clock.accent.is_empty());
        assert!(cfg.clock.bg_top.is_empty());
        assert!(cfg.clock.bg_bottom.is_empty());
    }

    #[test]
    fn old_clock_palette_is_handed_back_to_the_theme() {
        let mut cfg = DeckConfig::default();
        cfg.clock.follow_theme = false;
        cfg.clock.accent = "#A6C8FF".into();
        cfg.clock.bg_top = "#121820".into();
        cfg.clock.bg_bottom = "#10141b".into();
        let cfg = migrate(cfg);
        assert!(cfg.clock.follow_theme);
        assert!(cfg.clock.accent.is_empty());

        let mut custom = DeckConfig::default();
        custom.clock.follow_theme = false;
        custom.clock.accent = "#ff0000".into();
        let custom = migrate(custom);
        assert!(!custom.clock.follow_theme);
        assert_eq!(custom.clock.accent, "#ff0000");
    }

    #[test]
    fn clock_badge_names_become_their_glyph() {
        let star = crate::model::CLOCK_ICONS[3].0;
        for name in ["star", "Star", "звезда"] {
            let mut cfg = DeckConfig::default();
            cfg.clock.icon = name.into();
            assert_eq!(migrate(cfg).clock.icon, star, "{name}");
        }
        // A glyph picked by the deck survives, an unknown word does not: it
        // would paint letters into the badge.
        let mut cfg = DeckConfig::default();
        cfg.clock.icon = "\u{e81a}".into();
        assert_eq!(migrate(cfg).clock.icon, "\u{e81a}");
        let mut cfg = DeckConfig::default();
        cfg.clock.icon = "not an icon".into();
        assert!(migrate(cfg).clock.icon.is_empty());
    }
}
