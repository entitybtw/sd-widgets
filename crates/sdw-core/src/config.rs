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
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("sd-widgets: ignoring unreadable config {}: {e}", p.display());
                DeckConfig::default()
            }
        },
        Err(_) => DeckConfig::default(),
    }
}

pub fn save(cfg: &DeckConfig) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    crate::util::atomic_write(&path(), &json).map_err(|e| e.to_string())
}
