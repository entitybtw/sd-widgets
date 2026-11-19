//! what a bind actually does on the deck itself.
//!
//! remote and home-assistant actions are network calls and are handled by the app;
//! everything here is local: commands, apps, sound, brightness, media keys, typing.

use sdw_core::model::Action;
use sdw_core::util;

pub fn run(action: &Action) -> Result<String, String> {
    match action {
        Action::None => Ok("ничего".into()),
        Action::Shell { command } => util::run_shell(command).map(|out| {
            let out = out.trim();
            if out.is_empty() {
                "команда выполнена".into()
            } else {
                util::truncate(out, 120)
            }
        }),
        Action::Launch { target } => launch(target),
        Action::Volume { delta } => volume_delta(*delta),
        Action::VolumeSet { percent } => volume_set(*percent),
        Action::Mute => volume_mute(),
        Action::Brightness { delta } => brightness_delta(*delta),
        Action::BrightnessSet { percent } => brightness_set(*percent),
        Action::Media { command } => media(command),
        Action::Key { combo } => key_combo(combo),
        Action::Url { url } => util::open_url(url).map(|_| "открыто".into()),
        Action::Page { .. } => Ok("страница".into()),
        Action::Remote { .. } => Ok("компьютер".into()),
        Action::Ha { .. } => Ok("home assistant".into()),
    }
}

fn launch(target: &str) -> Result<String, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("пустое приложение".into());
    }
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("steam://")
    {
        return util::open_url(target).map(|_| format!("открыто {target}"));
    }
    if target.ends_with(".desktop") {
        return util::spawn_detached("gtk-launch", &[target]).map(|_| format!("запущено {target}"));
    }
    if util::has("gtk-launch") {
        if util::run("gtk-launch", &[target]).is_ok() {
            return Ok(format!("запущено {target}"));
        }
    }
    util::spawn_detached(target, &[]).map(|_| format!("запущено {target}"))
}

fn volume_delta(delta: i32) -> Result<String, String> {
    let step = format!(
        "{}%{}",
        delta.abs().max(1),
        if delta >= 0 { "+" } else { "-" }
    );
    if util::has("wpctl") {
        return util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &step])
            .map(|_| format!("громкость {delta:+}%"));
    }
    if util::has("pactl") {
        return util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &step])
            .map(|_| format!("громкость {delta:+}%"));
    }
    if util::has("amixer") {
        return util::run("amixer", &["sset", "Master", &step])
            .map(|_| format!("громкость {delta:+}%"));
    }
    Err("нет wpctl, pactl или amixer".into())
}

fn volume_set(percent: u8) -> Result<String, String> {
    let value = format!("{}%", percent.min(100));
    if util::has("wpctl") {
        return util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &value])
            .map(|_| format!("громкость {value}"));
    }
    if util::has("pactl") {
        return util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &value])
            .map(|_| format!("громкость {value}"));
    }
    if util::has("amixer") {
        return util::run("amixer", &["sset", "Master", &value])
            .map(|_| format!("громкость {value}"));
    }
    Err("нет wpctl, pactl или amixer".into())
}

fn volume_mute() -> Result<String, String> {
    if util::has("wpctl") {
        return util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .map(|_| "звук переключён".into());
    }
    if util::has("pactl") {
        return util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .map(|_| "звук переключён".into());
    }
    if util::has("amixer") {
        return util::run("amixer", &["sset", "Master", "toggle"])
            .map(|_| "звук переключён".into());
    }
    Err("нет wpctl, pactl или amixer".into())
}

fn brightness_delta(delta: i32) -> Result<String, String> {
    let step = format!(
        "{}%{}",
        delta.abs().max(1),
        if delta >= 0 { "+" } else { "-" }
    );
    if util::has("brightnessctl") {
        return util::run("brightnessctl", &["set", &step]).map(|_| format!("яркость {delta:+}%"));
    }
    if util::has("light") {
        let arg = if delta >= 0 {
            format!("-A {}", delta.abs())
        } else {
            format!("-U {}", delta.abs())
        };
        return util::run_shell(&format!("light {arg}")).map(|_| format!("яркость {delta:+}%"));
    }
    Err("нет brightnessctl или light".into())
}

fn brightness_set(percent: u8) -> Result<String, String> {
    let value = format!("{}%", percent.min(100));
    if util::has("brightnessctl") {
        return util::run("brightnessctl", &["set", &value]).map(|_| format!("яркость {value}"));
    }
    if util::has("light") {
        return util::run("light", &["-S", &value]).map(|_| format!("яркость {value}"));
    }
    Err("нет brightnessctl или light".into())
}

fn media(command: &str) -> Result<String, String> {
    let action = match command {
        "play" | "play_pause" | "pause" | "next" | "previous" | "prev" | "stop" => {
            if command == "prev" {
                "previous"
            } else if command == "play_pause" {
                "play-pause"
            } else {
                command
            }
        }
        other => other,
    };
    if util::has("playerctl") {
        util::run("playerctl", &[action]).map(|_| format!("медиа: {action}"))
    } else {
        Err("playerctl не установлен".into())
    }
}

fn key_combo(combo: &str) -> Result<String, String> {
    let combo = combo.trim();
    if combo.is_empty() {
        return Err("пустая комбинация".into());
    }
    let parts: Vec<&str> = combo
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("пустая комбинация".into());
    }
    let (mods, key) = parts.split_at(parts.len() - 1);
    let key = key[0];
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && util::has("wtype") {
        let mut args: Vec<String> = Vec::new();
        for m in mods {
            let lower = m.to_lowercase();
            let m = match lower.as_str() {
                "ctrl" | "control" => "ctrl",
                "alt" => "alt",
                "shift" => "shift",
                "super" | "meta" | "win" => "logo",
                other => other,
            };
            args.push("-M".into());
            args.push(m.into());
        }
        args.push("-k".into());
        args.push(key.into());
        for m in mods.iter().rev() {
            let lower = m.to_lowercase();
            let m = match lower.as_str() {
                "ctrl" | "control" => "ctrl",
                "alt" => "alt",
                "shift" => "shift",
                "super" | "meta" | "win" => "logo",
                other => other,
            };
            args.push("-m".into());
            args.push(m.into());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        return util::run("wtype", &refs).map(|_| format!("нажато {combo}"));
    }
    if util::has("xdotool") {
        return util::run("xdotool", &["key", combo]).map(|_| format!("нажато {combo}"));
    }
    Err("нет wtype или xdotool".into())
}

/// A one-line description used in the binds list.
pub fn describe(action: &Action) -> String {
    action.label()
}
