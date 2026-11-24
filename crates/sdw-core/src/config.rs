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
    // The visualizer's starter look became cava-like: thin solid bars, a
    // one-point gap, no caps. Only a file that still has that untouched old
    // look is lifted; a customised one keeps every value.
    let viz = &mut cfg.viz;
    if viz.rev < 2 {
        let untouched = viz.bars == 14
            && viz.height == 18
            && viz.width == 124
            && viz.gap == 2
            && viz.opacity == 100
            && viz.style == "bars"
            && !viz.mirror
            && viz.peak
            && viz.color.trim().is_empty();
        if untouched {
            viz.bars = 16;
            viz.height = 22;
            viz.width = 148;
            viz.gap = 1;
            viz.peak = false;
        }
        viz.rev = 2;
    }
    // One server used to live in `ha_url`/`ha_token`; it becomes the first
    // instance and the old pair is cleared, so the two cannot disagree.
    if cfg.ha_instances.is_empty()
        && !(cfg.ha_url.trim().is_empty() && cfg.ha_token.trim().is_empty())
    {
        let id = crate::model::random_id(4);
        let instance = crate::model::HaInstance {
            id: id.clone(),
            name: String::new(),
            url: std::mem::take(&mut cfg.ha_url),
            token: std::mem::take(&mut cfg.ha_token),
        };
        cfg.ha_instances.push(instance);
        if cfg.ha_active.trim().is_empty() {
            cfg.ha_active = id;
        }
    }
    // A chosen id whose server is gone (removed by hand from the file) falls
    // back to the first one instead of leaving the page unconfigured.
    if !cfg.ha_active.is_empty() && !cfg.ha_instances.iter().any(|i| i.id == cfg.ha_active) {
        cfg.ha_active = cfg
            .ha_instances
            .first()
            .map(|i| i.id.clone())
            .unwrap_or_default();
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
    fn old_visualizer_starter_look_becomes_cava() {
        let mut cfg = DeckConfig::default();
        cfg.viz.rev = 0;
        cfg.viz.bars = 14;
        cfg.viz.height = 18;
        cfg.viz.width = 124;
        cfg.viz.gap = 2;
        cfg.viz.opacity = 100;
        cfg.viz.style = "bars".into();
        cfg.viz.mirror = false;
        cfg.viz.peak = true;
        let cfg = migrate(cfg);
        assert_eq!(cfg.viz.bars, 16);
        assert_eq!(cfg.viz.gap, 1);
        assert!(!cfg.viz.peak);
        assert_eq!(cfg.viz.rev, 2);

        // A look the user changed keeps every value.
        let mut custom = DeckConfig::default();
        custom.viz.rev = 0;
        custom.viz.bars = 20;
        custom.viz.gap = 3;
        custom.viz.peak = true;
        let custom = migrate(custom);
        assert_eq!(custom.viz.bars, 20);
        assert_eq!(custom.viz.gap, 3);
        assert!(custom.viz.peak);
        assert_eq!(custom.viz.rev, 2);
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

    #[test]
    fn the_old_single_home_assistant_becomes_the_first_instance() {
        let mut cfg = DeckConfig::default();
        cfg.ha_url = "http://homeassistant.local:8123".into();
        cfg.ha_token = "long-token".into();
        let cfg = migrate(cfg);
        assert_eq!(cfg.ha_instances.len(), 1);
        assert_eq!(cfg.ha_instances[0].url, "http://homeassistant.local:8123");
        assert_eq!(cfg.ha_instances[0].token, "long-token");
        assert_eq!(cfg.ha_active, cfg.ha_instances[0].id);
        assert!(cfg.ha_url.is_empty() && cfg.ha_token.is_empty());
        assert_eq!(cfg.ha_instances[0].label(), "homeassistant.local");
    }

    #[test]
    fn a_chosen_home_assistant_that_is_gone_falls_back_to_the_first() {
        let mut cfg = DeckConfig::default();
        cfg.ha_instances = vec![crate::model::HaInstance {
            id: "aaaa".into(),
            name: "home".into(),
            url: "http://ha.local:8123".into(),
            token: "t".into(),
        }];
        cfg.ha_active = "gone".into();
        let cfg = migrate(cfg);
        assert_eq!(cfg.ha_active, "aaaa");
        // No servers at all: the computer's connection is used, and an empty
        // id is exactly that.
        let mut cfg = DeckConfig::default();
        cfg.ha_active = "gone".into();
        assert!(migrate(cfg).ha_active.is_empty());
    }

    #[test]
    fn a_named_home_assistant_keeps_its_name() {
        let mut cfg = DeckConfig::default();
        cfg.ha_instances = vec![crate::model::HaInstance {
            id: "bbbb".into(),
            name: "  dacha ".into(),
            url: "https://example.org".into(),
            token: "t".into(),
        }];
        let cfg = migrate(cfg);
        assert_eq!(cfg.ha_instances[0].label(), "dacha");
        assert!(cfg.ha_instances[0].ready());
    }
}
