//! Start the daemon together with the desktop session.
//!
//! Two mechanisms, because no single one covers Linux: an XDG autostart entry
//! (GNOME, KDE, Xfce and anything that honours it) and a line in the
//! compositor's own config, since niri and Hyprland ignore XDG autostart.
//! Turning the switch off removes both.
//!
//! The command always carries `--hidden`: at login the daemon should serve the
//! Deck and the tray, not open a window over the desktop. The tray, `--show`
//! and a second launch still bring the window back.

use crate::state::Logger;
use std::path::{Path, PathBuf};

const NIRI_MARK: &str = "// sd-widgets: daemon autostart";
const HYPR_MARK: &str = "# sd-widgets: daemon autostart";

/// What [`apply`] did, so the window can say it instead of guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Written,
    Removed,
    /// A debug build: writing it into the session would point the login at
    /// `target/debug`, so nothing was touched.
    Skipped,
}

/// The command the session should run, quoted for a desktop entry and for
/// KDL/Hyprland configs. A `target/debug` binary is a development build and
/// must never write itself into the maintainer's session config; a release
/// build is what `make release` produces, and it is fine to autostart.
fn command() -> Option<(String, String, String)> {
    let exe = if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        PathBuf::from(appimage)
    } else {
        let exe = std::env::current_exe().ok()?;
        let debug = exe
            .components()
            .zip(exe.components().skip(1))
            .any(|(a, b)| a.as_os_str() == "target" && b.as_os_str() == "debug");
        if debug {
            return None;
        }
        exe
    };
    let path = exe.display().to_string();
    let sh = shell_quote(&path);
    let kdl = kdl_quote(&path);
    Some((
        format!("{sh} --hidden"),
        format!("spawn-at-startup {kdl} \"--hidden\""),
        format!("exec-once = {sh} --hidden"),
    ))
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c))
    {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn kdl_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// True when this line is one of ours: a spawn/exec for this daemon, including
/// entries written by an older version or the installer.
fn ours(line: &str) -> bool {
    let t = line.trim_start();
    let spawn = t.starts_with("spawn-at-startup") || t.starts_with("exec-once");
    spawn
        && (t.contains("sdw-daemon")
            || t.contains("sdwidgets")
            || t.contains("sd-widgets")
            || t.contains("sdw_deck"))
}

fn without_ours(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_mark = false;
    for line in text.lines() {
        let t = line.trim();
        if t == NIRI_MARK.trim() || t == HYPR_MARK.trim() {
            // The marker owns exactly the next non-empty line, whatever the
            // binary is called: an AppImage can have any file name.
            after_mark = true;
            continue;
        }
        if after_mark {
            if t.is_empty() {
                continue;
            }
            after_mark = false;
            continue;
        }
        if ours(line) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn write_if_changed(path: &Path, text: &str, logger: &Logger) {
    match std::fs::read_to_string(path) {
        Ok(old) if old == text => return,
        _ => {}
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, text) {
        Ok(()) => logger.info(format!("автозапуск: {}", path.display())),
        Err(e) => logger.warn(format!(
            "автозапуск: не удалось записать {}: {e}",
            path.display()
        )),
    }
}

fn remove_if_present(path: &Path, logger: &Logger) {
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(()) => logger.info(format!("автозапуск выключен: {}", path.display())),
            Err(e) => logger.warn(format!(
                "автозапуск: не удалось удалить {}: {e}",
                path.display()
            )),
        }
    }
}

fn xdg_entry(cmd: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=sd-widgets daemon\n\
         Comment=sd-widgets daemon in the background\n\
         Exec={cmd}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-sd-widgets-autostart=1\n"
    )
}

/// Put the session in the requested state. Safe to call on every start: it
/// only writes a file when the content actually changes.
pub fn apply(enabled: bool, logger: &Logger) -> Outcome {
    let entry = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart/sdw-daemon.desktop");
    if !enabled {
        remove_if_present(&entry, logger);
        for (path, _mark) in compositor_configs() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let next = without_ours(&text);
                if next != text {
                    write_if_changed(&path, &next, logger);
                }
            }
        }
        return Outcome::Removed;
    }

    let Some((xdg_cmd, niri_line, hypr_line)) = command() else {
        logger.info(
            "автозапуск: пропущен для отладочной сборки (запустите установленный \
             демон или AppImage, чтобы включить его)",
        );
        return Outcome::Skipped;
    };
    write_if_changed(&entry, &xdg_entry(&xdg_cmd), logger);

    let niri = crate::config::niri_config_path();
    if niri.exists() {
        if let Ok(text) = std::fs::read_to_string(&niri) {
            let body = without_ours(&text);
            let next = format!(
                "{NIRI_MARK}\n{niri_line}\n\n{}",
                body.trim_start_matches('\n')
            );
            write_if_changed(&niri, &next, logger);
        }
    }
    let hypr = crate::config::hypr_config_path();
    if hypr.exists() {
        if let Ok(text) = std::fs::read_to_string(&hypr) {
            let mut body = without_ours(&text).trim_start_matches('\n').to_string();
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            let next = format!("{body}\n{HYPR_MARK}\n{hypr_line}\n");
            write_if_changed(&hypr, &next, logger);
        }
    }
    Outcome::Written
}

fn compositor_configs() -> Vec<(PathBuf, &'static str)> {
    vec![
        (crate::config::niri_config_path(), NIRI_MARK),
        (crate::config::hypr_config_path(), HYPR_MARK),
    ]
}
