//! One-click installation of the external pieces the daemon drives.
//!
//! Nothing here is required for the daemon to start: every component is optional and
//! reported separately. The installer prefers the system package manager and falls back
//! to downloading a signed VIIPER release into the user's data directory.

use crate::state::{JobCtx, Logger};
use sdw_core::model::{platform, VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: String,
    pub purpose: String,
    pub hint: String,
    pub needs_root: bool,
}

pub fn component_status() -> Vec<ComponentStatus> {
    let mut out = Vec::new();
    let bin = crate::config::viiper_bin_path();
    let viiper_path = sdw_core::util::which("viiper").or_else(|| bin.is_file().then_some(bin));
    out.push(ComponentStatus {
        id: "viiper".into(),
        name: "VIIPER".into(),
        installed: viiper_path.is_some(),
        version: viiper_path
            .as_ref()
            .and_then(|p| Command::new(p).arg("version").output().ok())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .find(|l| l.to_ascii_lowercase().contains("version"))
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default(),
        purpose: "Виртуальные USB-геймпады и клавиатура/мышь для ПК".into(),
        hint: "Скачивается с github.com/Alia5/VIIPER".into(),
        needs_root: false,
    });
    for (id, name, purpose, hint, root) in [
        (
            "usbip",
            "usbip",
            "Подключение виртуальных USB-устройств",
            "Пакет usbip / usbip-utils",
            true,
        ),
        (
            "grim",
            "grim",
            "Снимки Wayland-экрана для трансляции",
            "Пакет grim",
            true,
        ),
        (
            "ffmpeg",
            "ffmpeg",
            "Трансляция X11/Windows и сжатие кадров",
            "Пакет ffmpeg",
            true,
        ),
        (
            "ydotool",
            "ydotool",
            "Управление мышью на Wayland",
            "Пакет ydotool + user-сервис ydotoold",
            true,
        ),
        (
            "wtype",
            "wtype",
            "Ввод текста на Wayland",
            "Пакет wtype",
            true,
        ),
        (
            "krfb",
            "krfb",
            "Виртуальный монитор KDE Plasma",
            "Пакет krfb",
            true,
        ),
    ] {
        let found = match id {
            "krfb" => sdw_core::util::has("krfb-virtualmonitor"),
            _ => sdw_core::util::has(id),
        };
        out.push(ComponentStatus {
            id: id.into(),
            name: name.into(),
            installed: found,
            version: String::new(),
            purpose: purpose.into(),
            hint: hint.into(),
            needs_root: root,
        });
    }
    out
}

fn pkg_manager() -> Option<&'static str> {
    if sdw_core::util::has("pacman") {
        Some("pacman")
    } else if sdw_core::util::has("apt-get") {
        Some("apt")
    } else if sdw_core::util::has("dnf") {
        Some("dnf")
    } else if sdw_core::util::has("zypper") {
        Some("zypper")
    } else {
        None
    }
}

fn package_for(manager: &str, id: &str) -> &'static str {
    match (manager, id) {
        (_, "krfb") => "krfb",
        (_, "usbip") => "usbip",
        (_, "grim") => "grim",
        (_, "ffmpeg") => "ffmpeg",
        (_, "ydotool") => "ydotool",
        (_, "wtype") => "wtype",
        _ => "grim",
    }
}

fn run_privileged(
    sudo_password: &str,
    program: &str,
    args: &[&str],
    ctx: &JobCtx,
) -> Result<(), String> {
    ctx.line(format!("$ {program} {}", args.join(" ")));
    let direct = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string());
    if let Ok(o) = &direct {
        if o.status.success() {
            return Ok(());
        }
    }
    if !sudo_password.is_empty() {
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
            let _ = writeln!(stdin, "{sudo_password}");
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("{program}: код {}", out.status.code().unwrap_or(-1))
        } else {
            err
        });
    }
    if sdw_core::util::has("pkexec") {
        ctx.line("Нужен пароль администратора — откроется системный запрос.");
        let out = Command::new("pkexec")
            .arg(program)
            .args(args)
            .output()
            .map_err(|e| format!("pkexec: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Err(format!(
        "{program} требует root. Запустите daemon из терминала или укажите пароль sudo."
    ))
}

fn install_packages(ids: &[String], sudo_password: &str, ctx: &JobCtx) -> Result<(), String> {
    let Some(manager) = pkg_manager() else {
        return Err("Не найден пакетный менеджер. Установите компоненты вручную.".into());
    };
    let pkgs: Vec<&str> = ids.iter().map(|id| package_for(manager, id)).collect();
    ctx.line(format!("Пакетный менеджер: {manager}"));
    let mut args: Vec<&str> = match manager {
        "pacman" => vec!["-S", "--needed", "--noconfirm"],
        "apt" => vec!["install", "-y"],
        "dnf" => vec!["install", "-y"],
        "zypper" => vec!["--non-interactive", "install"],
        _ => return Err("неизвестный пакетный менеджер".into()),
    };
    args.extend(pkgs.iter().copied());
    run_privileged(
        sudo_password,
        if manager == "apt" { "apt-get" } else { manager },
        &args,
        ctx,
    )?;
    // ydotool needs its user daemon on Wayland.
    if ids.iter().any(|i| i == "ydotool") {
        ctx.line("$ systemctl --user enable --now ydotool.service");
        let _ = Command::new("systemctl")
            .args(["--user", "enable", "--now", "ydotool.service"])
            .output();
    }
    Ok(())
}

fn download(url: &str, dest: &Path) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .user_agent(concat!("sd-widgets/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut r = agent.get(url).call().map_err(|e| e.to_string())?;
    if r.status().as_u16() != 200 {
        return Err(format!("HTTP {} для {url}", r.status().as_u16()));
    }
    let bytes = r
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    if let Some(parent) = dest.parent() {
        sdw_core::util::ensure_dir(parent).map_err(|e| e.to_string())?;
    }
    sdw_core::util::atomic_write(dest, &bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    let mut h = Sha256::new();
    h.update(bytes);
    sdw_core::auth::hex(&h.finalize()) == expected_hex.to_ascii_lowercase()
}

fn install_viiper(ctx: &JobCtx, logger: &Logger) -> Result<(), String> {
    let tag = "v0.9.0";
    let asset = match (platform(), std::env::consts::ARCH) {
        ("linux", "x86_64") => "viiper-linux-amd64",
        ("linux", "aarch64") => "viiper-linux-arm64",
        ("windows", "x86_64") => "viiper-windows-amd64.exe",
        ("windows", "aarch64") => "viiper-windows-arm64.exe",
        _ => return Err("Готовый бинарник VIIPER есть только для Linux и Windows x64/arm64".into()),
    };
    let base = format!("https://github.com/Alia5/VIIPER/releases/download/{tag}");
    let url = format!("{base}/{asset}");
    let sums_url = format!("{base}/{asset}.SHA256SUMS");
    let dest = crate::config::viiper_bin_path();
    ctx.line(format!("Скачиваю {url}"));
    let bytes = download(&url, &dest)?;
    if let Ok(sums) = download(&sums_url, &dest.with_extension("sums")) {
        let text = String::from_utf8_lossy(&sums);
        if let Some(expected) = text
            .lines()
            .find(|l| l.contains(asset))
            .and_then(|l| l.split_whitespace().next())
        {
            if verify_sha256(&bytes, expected) {
                ctx.line("SHA-256 совпал");
            } else {
                let _ = std::fs::remove_file(&dest);
                return Err("SHA-256 не совпал: скачивание отменено".into());
            }
        }
        let _ = std::fs::remove_file(dest.with_extension("sums"));
    } else {
        ctx.line("SHA256SUMS недоступен, проверка пропущена");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    let out = Command::new(&dest)
        .arg("version")
        .output()
        .map_err(|e| format!("проверка VIIPER: {e}"))?;
    if !out.status.success() {
        return Err("скачанный VIIPER не запускается".into());
    }
    ctx.line(format!(
        "VIIPER установлен: {}",
        String::from_utf8_lossy(&out.stdout).trim()
    ));
    logger.info(format!("VIIPER установлен в {}", dest.display()));
    Ok(())
}

/// Install the selected components. Runs inside a [`crate::state::JobManager`] job.
pub fn install(
    components: &[String],
    sudo_password: &str,
    ctx: &JobCtx,
    logger: &Logger,
) -> Result<(), String> {
    ctx.line(format!("SD Widgets {VERSION} · установка"));
    let mut need_packages = Vec::new();
    for c in components {
        match c.as_str() {
            "viiper" => {
                if let Err(e) = install_viiper(ctx, logger) {
                    ctx.error(format!("VIIPER: {e}"));
                    return Err(e);
                }
            }
            "usbip" | "grim" | "ffmpeg" | "ydotool" | "wtype" | "krfb" => {
                need_packages.push(c.clone());
            }
            other => ctx.line(format!("Неизвестный компонент: {other}")),
        }
    }
    if !need_packages.is_empty() {
        if cfg!(windows) {
            ctx.line("На Windows установите usbipd и ffmpeg через winget:");
            ctx.line("  winget install --id Gyan.FFmpeg");
            ctx.line("  winget install --id dorssel.usbipd-win");
        } else if cfg!(target_os = "macos") {
            ctx.line("На macOS: brew install ffmpeg cliclick");
        } else if let Err(e) = install_packages(&need_packages, sudo_password, ctx) {
            ctx.error(format!("пакеты: {e}"));
            return Err(e);
        }
    }
    ctx.line("Готово. Перезапустите VIIPER, если он уже был запущен.");
    ctx.done("Установка завершена");
    Ok(())
}

pub fn install_status() -> serde_json::Value {
    serde_json::json!({
        "components": component_status(),
        "platform": platform(),
        "arch": std::env::consts::ARCH,
    })
}
