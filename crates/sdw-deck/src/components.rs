//! The external tools this app drives, and their installation.
//!
//! Nothing here is required to start: a missing tool hides its row or falls
//! back to another one (the sound page tries `pw-play`, then `paplay`, then
//! `aplay`). The settings page lists what is missing, what it is for, and
//! installs the chosen ones through the system package manager after asking
//! for the administrator password. On SteamOS the read-only root is turned off
//! for the length of the install and back on afterwards, even when the install
//! fails.
//!
//! Only the ids in [`IDS`] can be installed: nothing here runs a command that
//! came from the page.

use sdw_core::util;
use sdw_core::{t, Lang};
use std::io::Write;
use std::process::{Command, Stdio};

/// Every tool the app looks for, in the order the page shows them.
pub const IDS: [&str; 6] = ["player", "parec", "playerctl", "wtype", "xdotool", "grim"];

#[derive(Debug, Clone)]
pub struct Status {
    pub id: &'static str,
    pub present: bool,
    /// The binary that answered, when one did.
    pub found: String,
}

pub fn status() -> Vec<Status> {
    IDS.iter()
        .map(|id| {
            let found = alternatives(id)
                .iter()
                .copied()
                .find(|program| util::has(program))
                .unwrap_or("");
            Status {
                id,
                present: !found.is_empty(),
                found: found.to_string(),
            }
        })
        .collect()
}

fn alternatives(id: &str) -> &'static [&'static str] {
    match id {
        "player" => &["pw-play", "paplay", "aplay"],
        "parec" => &["parec"],
        "playerctl" => &["playerctl"],
        "wtype" => &["wtype"],
        "xdotool" => &["xdotool"],
        "grim" => &["grim"],
        _ => &[],
    }
}

pub fn title(lang: Lang, id: &str) -> &'static str {
    match id {
        "player" => t!(lang, "звук компьютера", "the computer's sound"),
        "parec" => t!(
            lang,
            "звук деки на компьютер",
            "the deck's sound to the computer"
        ),
        "playerctl" => t!(lang, "управление плеером", "media controls"),
        "wtype" => t!(lang, "ввод текста (wayland)", "typing (wayland)"),
        "xdotool" => t!(lang, "ввод текста (x11)", "typing (x11)"),
        "grim" => t!(lang, "снимки экрана", "screenshots"),
        _ => "",
    }
}

pub fn purpose(lang: Lang, id: &str) -> &'static str {
    match id {
        "player" => t!(
            lang,
            "играет поток звука с компьютера; без него страница звука молчит.",
            "plays the sound stream from the computer; without it the sound page is silent."
        ),
        "parec" => t!(
            lang,
            "записывает звук деки, когда компьютер смотрит на неё.",
            "records the deck's sound while the computer watches it."
        ),
        "playerctl" => t!(
            lang,
            "кнопки play, next и previous в медиа-вкладке.",
            "the play, next and previous buttons on the media page."
        ),
        "wtype" => t!(
            lang,
            "вводит текст и нажимает клавиши в биндах на wayland.",
            "types text and presses keys for binds on wayland."
        ),
        "xdotool" => t!(
            lang,
            "то же на x11: ввод текста и клавиши в биндах.",
            "the same on x11: typing and keys for binds."
        ),
        "grim" => t!(
            lang,
            "делает снимок экрана деки для бинда или страницы.",
            "takes a screenshot of the deck for a bind or the page."
        ),
        _ => "",
    }
}

fn manager() -> Option<&'static str> {
    for program in ["pacman", "apt-get", "dnf", "zypper"] {
        if util::has(program) {
            return Some(program);
        }
    }
    None
}

/// The packages a tool needs from one manager. The player group is a small
/// stack on purpose: whichever of the three players the distribution ships is
/// enough, so all of them are asked for and `--needed` skips what is there.
fn packages(id: &str, manager: &str) -> &'static [&'static str] {
    match (id, manager) {
        ("player", "pacman") => &["pipewire", "libpulse", "alsa-utils"],
        ("player", "apt-get") => &["pipewire-bin", "pulseaudio-utils", "alsa-utils"],
        ("player", "dnf") => &["pipewire-utils", "pulseaudio-utils", "alsa-utils"],
        ("player", "zypper") => &["pipewire-tools", "pulseaudio-utils", "alsa-utils"],
        ("parec", "pacman") => &["libpulse"],
        ("parec", "apt-get") => &["pulseaudio-utils"],
        ("parec", _) => &["pulseaudio-utils"],
        ("playerctl", _) => &["playerctl"],
        ("wtype", _) => &["wtype"],
        ("xdotool", _) => &["xdotool"],
        ("grim", _) => &["grim"],
        _ => &[],
    }
}

/// Install the given tools. Returns everything the commands printed, so the
/// page can show what happened; the caller shows the error when it fails.
pub fn install(ids: &[String], sudo_password: &str, lang: Lang) -> Result<String, String> {
    let Some(manager) = manager() else {
        return Err(t!(
            lang,
            "не найден пакетный менеджер — поставьте компоненты вручную",
            "no package manager — install the components by hand"
        )
        .into());
    };
    let mut names: Vec<&'static str> = Vec::new();
    for id in ids {
        if !IDS.contains(&id.as_str()) {
            continue;
        }
        for package in packages(id, manager) {
            if !names.contains(package) {
                names.push(package);
            }
        }
    }
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args: Vec<&str> = match manager {
        "pacman" => vec!["-S", "--needed", "--noconfirm"],
        "apt-get" => vec!["install", "-y"],
        "dnf" => vec!["install", "-y"],
        "zypper" => vec!["--non-interactive", "install"],
        _ => return Err("неизвестный пакетный менеджер".into()),
    };
    args.extend(names.iter().copied());
    let mut log = String::new();
    // SteamOS keeps its root read-only; packages go in between the two calls,
    // and the root is closed again even when the install fails.
    let steamos = util::has("steamos-readonly");
    let mut opened = false;
    if steamos {
        match run_privileged(sudo_password, "steamos-readonly", &["disable"], lang) {
            Ok(out) => {
                opened = true;
                log.push_str(&out);
            }
            Err(e) => return Err(format!("read-only root: {e}")),
        }
    }
    let result = run_privileged(sudo_password, manager, &args, lang);
    if opened {
        match run_privileged(sudo_password, "steamos-readonly", &["enable"], lang) {
            Ok(out) => log.push_str(&out),
            Err(e) => log.push_str(&format!("read-only root: {e}\n")),
        }
    }
    let out = result?;
    log.push_str(&out);
    Ok(log)
}

/// Run a command as root: directly when the user already is, otherwise through
/// `sudo -S` with the password on stdin. A failed direct run falls through to
/// sudo, because a writable root does not mean the command can run.
fn run_privileged(
    password: &str,
    program: &str,
    args: &[&str],
    lang: Lang,
) -> Result<String, String> {
    if let Ok(out) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
    }
    if password.is_empty() {
        return Err(t!(
            lang,
            "нужен пароль администратора",
            "the administrator password is needed"
        )
        .into());
    }
    let mut child = Command::new("sudo")
        .args(["-S", "-p", ""])
        .arg(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("sudo: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(text);
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !err.is_empty() {
        text.push_str(&err);
        text.push('\n');
    }
    Err(if err.is_empty() {
        format!("{program}: код {}", out.status.code().unwrap_or(-1))
    } else {
        err
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_title_and_a_package() {
        for id in IDS {
            for manager in ["pacman", "apt-get", "dnf", "zypper"] {
                assert!(
                    !packages(id, manager).is_empty(),
                    "{id} has no package for {manager}"
                );
            }
            assert!(!title(Lang::Ru, id).is_empty(), "{id} has no title");
            assert!(!purpose(Lang::En, id).is_empty(), "{id} has no purpose");
        }
    }

    #[test]
    fn an_unknown_id_installs_nothing() {
        assert!(install(&["not-a-tool".into()], "", Lang::Ru).is_ok());
    }
}
