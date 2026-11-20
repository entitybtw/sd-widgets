//! The Deck's own volume and brightness.
//!
//! The deck talks to its computer for the computer's sound, but the pull-down
//! panel controls the Deck itself: the default output sink and the panel
//! backlight. PipeWire (`wpctl`) is used when present, with PulseAudio
//! (`pactl`) as the fallback; brightness goes through sysfs, then
//! `brightnessctl`, then `light`. Every path is optional: the panel hides a row
//! it cannot read instead of showing a dead slider.

use std::path::PathBuf;

// ---------------------------------------------------------------- volume

/// Current output volume and mute state, when the Deck has a readable sink.
pub fn volume() -> Option<(u8, bool)> {
    if sdw_core::util::has("wpctl") {
        if let Ok(text) = sdw_core::util::run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]) {
            if let Some(v) = parse_wpctl(&text) {
                return Some(v);
            }
        }
    }
    if sdw_core::util::has("pactl") {
        let level = sdw_core::util::run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])
            .ok()
            .and_then(|t| parse_pactl_volume(&t))?;
        let muted = sdw_core::util::run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])
            .ok()
            .map(|t| parse_pactl_mute(&t))
            .unwrap_or(false);
        return Some((level, muted));
    }
    None
}

/// Set the output volume, unmuting first so the change is audible.
pub fn set_volume(percent: u8) -> Result<(), String> {
    let percent = percent.min(100);
    if sdw_core::util::has("wpctl") {
        let value = format!("{percent}%");
        sdw_core::util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &value])?;
        let _ = sdw_core::util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"]);
        return Ok(());
    }
    if sdw_core::util::has("pactl") {
        let value = format!("{percent}%");
        sdw_core::util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &value])?;
        let _ = sdw_core::util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        return Ok(());
    }
    Err("Нет wpctl или pactl".into())
}

/// Flip the mute of the default sink; returns the new state.
pub fn toggle_mute() -> Result<bool, String> {
    let muted = volume().map(|(_, m)| m).unwrap_or(false);
    let target = if muted { "0" } else { "1" };
    if sdw_core::util::has("wpctl") {
        sdw_core::util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])?;
        return Ok(!muted);
    }
    if sdw_core::util::has("pactl") {
        sdw_core::util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", target])?;
        return Ok(!muted);
    }
    Err("Нет wpctl или pactl".into())
}

/// `Volume: 0.42` or `Volume: 0.42 [MUTED]` from `wpctl get-volume`.
fn parse_wpctl(text: &str) -> Option<(u8, bool)> {
    let raw = text.trim().strip_prefix("Volume:")?.trim();
    let muted = raw.contains("[MUTED]");
    let number = raw.split_whitespace().next()?.replace(',', ".");
    let level: f32 = number.parse().ok()?;
    Some(((level * 100.0).round().clamp(0.0, 100.0) as u8, muted))
}

/// The first `NN%` in `pactl get-sink-volume` output.
fn parse_pactl_volume(text: &str) -> Option<u8> {
    let mut rest = text;
    while let Some(at) = rest.find('%') {
        let before = &rest[..at];
        let digits: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            let number: String = digits.chars().rev().collect();
            if let Ok(level) = number.parse::<u8>() {
                return Some(level.min(100));
            }
        }
        rest = &rest[at + 1..];
    }
    None
}

fn parse_pactl_mute(text: &str) -> bool {
    text.trim()
        .strip_prefix("Mute:")
        .map(|v| v.trim().eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------- brightness

/// The first backlight with a readable maximum.
fn backlight() -> Option<(PathBuf, PathBuf, u32)> {
    let entries = std::fs::read_dir("/sys/class/backlight").ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        let max = std::fs::read_to_string(dir.join("max_brightness"))
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())?;
        if max == 0 {
            continue;
        }
        let current = dir.join("brightness");
        if current.exists() {
            return Some((dir, current, max));
        }
    }
    None
}

/// Current panel brightness in percent.
pub fn brightness() -> Option<u8> {
    if let Some((_, current, max)) = backlight() {
        if let Ok(text) = std::fs::read_to_string(current) {
            if let Ok(value) = text.trim().parse::<u32>() {
                return Some(percent_of(value, max));
            }
        }
    }
    if sdw_core::util::has("brightnessctl") {
        if let Ok(text) = sdw_core::util::run("brightnessctl", &["-m"]) {
            return parse_brightnessctl(&text);
        }
    }
    None
}

/// Set panel brightness in percent. Sysfs is tried first because it needs no
/// extra tool; a udev rule or group membership makes it writable on handhelds.
pub fn set_brightness(percent: u8) -> Result<(), String> {
    let percent = percent.clamp(1, 100);
    if let Some((_, current, max)) = backlight() {
        let value = ((max as f32) * (percent as f32) / 100.0).round().max(1.0) as u32;
        if std::fs::write(&current, format!("{value}\n")).is_ok() {
            return Ok(());
        }
    }
    if sdw_core::util::has("brightnessctl") {
        let value = format!("{percent}%");
        return sdw_core::util::run("brightnessctl", &["set", &value]).map(|_| ());
    }
    if sdw_core::util::has("light") {
        let value = percent.to_string();
        return sdw_core::util::run("light", &["-S", &value]).map(|_| ());
    }
    Err("Нет доступа к подсветке".into())
}

fn percent_of(value: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    ((value.min(max) as f32) * 100.0 / max as f32).round() as u8
}

/// `brightnessctl -m` prints `device,class,current,percent%`; the percentage is
/// the last field.
fn parse_brightnessctl(text: &str) -> Option<u8> {
    let line = text.lines().next()?.trim();
    let last = line.split(',').next_back()?.trim().trim_end_matches('%');
    last.parse::<u8>().ok().map(|p| p.min(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpctl_volume_is_parsed() {
        assert_eq!(parse_wpctl("Volume: 0.42\n"), Some((42, false)));
        assert_eq!(parse_wpctl("Volume: 0.13 [MUTED]"), Some((13, true)));
        assert_eq!(parse_wpctl("Volume: 1,00"), Some((100, false)));
        assert_eq!(parse_wpctl("garbage"), None);
    }

    #[test]
    fn pactl_volume_and_mute_are_parsed() {
        let text =
            "Volume: front-left: 8773 /  13% / -52,40 dB,   front-right: 8773 /  13% / -52,40 dB\n";
        assert_eq!(parse_pactl_volume(text), Some(13));
        assert!(parse_pactl_mute("Mute: yes\n"));
        assert!(!parse_pactl_mute("Mute: no\n"));
    }

    #[test]
    fn brightness_percentages_are_read() {
        assert_eq!(
            parse_brightnessctl("amdgpu_bl0,backlight,512,40%\n"),
            Some(40)
        );
        assert_eq!(percent_of(512, 1024), 50);
        assert_eq!(percent_of(1024, 1024), 100);
    }
}
