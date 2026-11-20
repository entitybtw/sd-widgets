//! what a bind actually does on the deck itself.
//!
//! remote and home-assistant actions are network calls and are handled by the app;
//! everything here is local: commands, apps, sound, brightness, media keys, typing.

use sdw_core::model::Action;
use sdw_core::util;
use sdw_core::{t, Lang};

pub fn run(action: &Action, lang: Lang) -> Result<String, String> {
    match action {
        Action::None => Ok(t!(lang, "ничего", "nothing").into()),
        Action::Shell { command } => util::run_shell(command).map(|out| {
            let out = out.trim();
            if out.is_empty() {
                t!(lang, "команда выполнена", "command finished").into()
            } else {
                util::truncate(out, 120)
            }
        }),
        Action::Launch { target } => launch(target, lang),
        Action::Volume { delta } => volume_delta(*delta, lang),
        Action::VolumeSet { percent } => volume_set(*percent, lang),
        Action::Mute => volume_mute(lang),
        Action::Brightness { delta } => brightness_delta(*delta, lang),
        Action::BrightnessSet { percent } => brightness_set(*percent, lang),
        Action::Media { command } => media(command, lang),
        Action::Key { combo } => key_combo(combo, lang),
        Action::Url { url } => {
            util::open_url(url).map(|_| t!(lang, "открыто", "opened").to_string())
        }
        Action::Page { .. } => Ok(t!(lang, "страница", "page").into()),
        Action::Remote { .. } => Ok(t!(lang, "компьютер", "computer").into()),
        Action::Ha { .. } => Ok("home assistant".into()),
        // HTTP runs in the background from the app, never on the UI thread.
        Action::Http { .. } => Err(t!(
            lang,
            "http выполняется в фоне",
            "http runs in the background"
        )
        .into()),
    }
}

fn launch(target: &str, lang: Lang) -> Result<String, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err(t!(lang, "пустое приложение", "empty app name").into());
    }
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("steam://")
    {
        return util::open_url(target)
            .map(|_| format!("{} {target}", t!(lang, "открыто", "opened")));
    }
    if target.ends_with(".desktop") {
        return util::spawn_detached("gtk-launch", &[target])
            .map(|_| format!("{} {target}", t!(lang, "запущено", "launched")));
    }
    if util::has("gtk-launch") {
        if util::run("gtk-launch", &[target]).is_ok() {
            return Ok(format!("{} {target}", t!(lang, "запущено", "launched")));
        }
    }
    util::spawn_detached(target, &[])
        .map(|_| format!("{} {target}", t!(lang, "запущено", "launched")))
}

fn volume_delta(delta: i32, lang: Lang) -> Result<String, String> {
    let step = format!(
        "{}%{}",
        delta.abs().max(1),
        if delta >= 0 { "+" } else { "-" }
    );
    if util::has("wpctl") {
        return util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &step])
            .map(|_| format!("{} {delta:+}%", t!(lang, "громкость", "volume")));
    }
    if util::has("pactl") {
        return util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &step])
            .map(|_| format!("{} {delta:+}%", t!(lang, "громкость", "volume")));
    }
    if util::has("amixer") {
        return util::run("amixer", &["sset", "Master", &step])
            .map(|_| format!("{} {delta:+}%", t!(lang, "громкость", "volume")));
    }
    Err(t!(lang, "нет wpctl, pactl или amixer", "no wpctl, pactl or amixer").into())
}

fn volume_set(percent: u8, lang: Lang) -> Result<String, String> {
    let value = format!("{}%", percent.min(100));
    if util::has("wpctl") {
        return util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &value])
            .map(|_| format!("{} {value}", t!(lang, "громкость", "volume")));
    }
    if util::has("pactl") {
        return util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &value])
            .map(|_| format!("{} {value}", t!(lang, "громкость", "volume")));
    }
    if util::has("amixer") {
        return util::run("amixer", &["sset", "Master", &value])
            .map(|_| format!("{} {value}", t!(lang, "громкость", "volume")));
    }
    Err(t!(lang, "нет wpctl, pactl или amixer", "no wpctl, pactl or amixer").into())
}

fn volume_mute(lang: Lang) -> Result<String, String> {
    if util::has("wpctl") {
        return util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .map(|_| t!(lang, "звук переключён", "sound toggled").to_string());
    }
    if util::has("pactl") {
        return util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .map(|_| t!(lang, "звук переключён", "sound toggled").to_string());
    }
    if util::has("amixer") {
        return util::run("amixer", &["sset", "Master", "toggle"])
            .map(|_| t!(lang, "звук переключён", "sound toggled").to_string());
    }
    Err(t!(lang, "нет wpctl, pactl или amixer", "no wpctl, pactl or amixer").into())
}

fn brightness_delta(delta: i32, lang: Lang) -> Result<String, String> {
    let step = format!(
        "{}%{}",
        delta.abs().max(1),
        if delta >= 0 { "+" } else { "-" }
    );
    if util::has("brightnessctl") {
        return util::run("brightnessctl", &["set", &step])
            .map(|_| format!("{} {delta:+}%", t!(lang, "яркость", "brightness")));
    }
    if util::has("light") {
        let arg = if delta >= 0 {
            format!("-A {}", delta.abs())
        } else {
            format!("-U {}", delta.abs())
        };
        return util::run_shell(&format!("light {arg}"))
            .map(|_| format!("{} {delta:+}%", t!(lang, "яркость", "brightness")));
    }
    Err(t!(
        lang,
        "нет brightnessctl или light",
        "no brightnessctl or light"
    )
    .into())
}

fn brightness_set(percent: u8, lang: Lang) -> Result<String, String> {
    let value = format!("{}%", percent.min(100));
    if util::has("brightnessctl") {
        return util::run("brightnessctl", &["set", &value])
            .map(|_| format!("{} {value}", t!(lang, "яркость", "brightness")));
    }
    if util::has("light") {
        return util::run("light", &["-S", &value])
            .map(|_| format!("{} {value}", t!(lang, "яркость", "brightness")));
    }
    Err(t!(
        lang,
        "нет brightnessctl или light",
        "no brightnessctl or light"
    )
    .into())
}

fn media(command: &str, lang: Lang) -> Result<String, String> {
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
        util::run("playerctl", &[action])
            .map(|_| format!("{}: {action}", t!(lang, "медиа", "media")))
    } else {
        Err(t!(lang, "playerctl не установлен", "playerctl is not installed").into())
    }
}

fn key_combo(combo: &str, lang: Lang) -> Result<String, String> {
    let combo = combo.trim();
    if combo.is_empty() {
        return Err(t!(lang, "пустая комбинация", "empty key combo").into());
    }
    let parts: Vec<&str> = combo
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(t!(lang, "пустая комбинация", "empty key combo").into());
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
        return util::run("wtype", &refs)
            .map(|_| format!("{} {combo}", t!(lang, "нажато", "pressed")));
    }
    if util::has("xdotool") {
        return util::run("xdotool", &["key", combo])
            .map(|_| format!("{} {combo}", t!(lang, "нажато", "pressed")));
    }
    Err(t!(lang, "нет wtype или xdotool", "no wtype or xdotool").into())
}

/// A one-line description used in the binds list.
pub fn describe(action: &Action) -> String {
    action.label()
}
