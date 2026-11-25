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
    #[cfg(windows)]
    {
        out.push(windows_component(
            "ffmpeg",
            "ffmpeg",
            "Трансляция экрана и звука на деку",
            "winget install --id Gyan.FFmpeg",
            false,
        ));
        let vdd = crate::win::display::vdd_ready();
        out.push(ComponentStatus {
            id: "vdd".into(),
            name: "Virtual Display Driver".into(),
            installed: vdd,
            version: if vdd {
                crate::win::display::vdd_status()
            } else {
                String::new()
            },
            purpose: "Виртуальный монитор для страницы «Экран»".into(),
            hint: "winget install --id VirtualDrivers.Virtual-Display-Driver".into(),
            needs_root: true,
        });
    }
    #[cfg(not(windows))]
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

#[cfg_attr(not(windows), allow(dead_code))]
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
    let bytes = r.body_mut().read_to_vec().map_err(|e| e.to_string())?;
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

// ---------------------------------------------------------------- Windows

/// `<data>/bin`, where portable tools downloaded on Windows live.
#[cfg(windows)]
fn bin_dir() -> std::path::PathBuf {
    sdw_core::util::data_dir().join("bin")
}

/// Put `<data>/bin` first on `PATH` so an ffmpeg the daemon downloaded is found
/// by the same `Command::new("ffmpeg")` calls the capture code already uses.
#[cfg(windows)]
pub fn prepend_bin_to_path() {
    let dir = bin_dir();
    if !dir.is_dir() {
        return;
    }
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut parts: Vec<std::ffi::OsString> = vec![dir.into_os_string()];
    parts.extend(std::env::split_paths(&old).map(|p| p.into_os_string()));
    if let Ok(joined) = std::env::join_paths(parts) {
        std::env::set_var("PATH", joined);
    }
}

/// Download the portable ffmpeg build and unpack just the two executables into
/// `<data>/bin`. No administrator rights and no package manager involved.
#[cfg(windows)]
fn install_ffmpeg_windows(ctx: &JobCtx) -> Result<(), String> {
    if sdw_core::util::has("ffmpeg") {
        ctx.line("ffmpeg уже доступен в PATH");
        return Ok(());
    }
    let url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
    let dest_dir = bin_dir();
    sdw_core::util::ensure_dir(&dest_dir).map_err(|e| e.to_string())?;
    let zip_path = sdw_core::util::data_dir().join("ffmpeg-download.zip");
    ctx.line(format!("Скачиваю {url}"));
    download(url, &zip_path)?;
    ctx.line("Распаковываю ffmpeg.exe и ffprobe.exe");
    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("архив: {e}"))?;
    let mut found = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        let base = name.rsplit('/').next().unwrap_or("");
        if base == "ffmpeg.exe" || base == "ffprobe.exe" {
            let out = dest_dir.join(base);
            let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
            found += 1;
        }
    }
    let _ = std::fs::remove_file(&zip_path);
    if found == 0 {
        return Err("в архиве нет ffmpeg.exe".into());
    }
    prepend_bin_to_path();
    ctx.line(format!("ffmpeg установлен в {}", dest_dir.display()));
    Ok(())
}

#[cfg(windows)]
fn winget_install(id: &str, ctx: &JobCtx) -> Result<(), String> {
    if !sdw_core::util::has("winget") {
        return Err(format!(
            "winget не найден. Установите вручную: winget install --id {id}"
        ));
    }
    ctx.line(format!("$ winget install --id {id} -e --silent"));
    let status = Command::new("winget")
        .args([
            "install",
            "--id",
            id,
            "-e",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ])
        .status()
        .map_err(|e| format!("winget: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("winget код {}", status.code().unwrap_or(-1)))
    }
}

/// One-click dependencies on Windows: ffmpeg as a portable download, the
/// Virtual Display Driver and usbipd through winget when it exists.
#[cfg(windows)]
fn install_windows_deps(ids: &[String], ctx: &JobCtx) -> Result<(), String> {
    for id in ids {
        match id.as_str() {
            "ffmpeg" => install_ffmpeg_windows(ctx)?,
            "vdd" => winget_install("VirtualDrivers.Virtual-Display-Driver", ctx)
                .map_err(|e| format!("Virtual Display Driver: {e}"))?,
            "usbip" => {
                winget_install("dorssel.usbipd-win", ctx).map_err(|e| format!("usbipd-win: {e}"))?
            }
            other => ctx.line(format!("{other}: отдельного пакета для Windows нет")),
        }
    }
    Ok(())
}

/// The release source for VIIPER: the configured mirror, or GitHub's
/// `Alia5/VIIPER` when the field is empty. A bad address falls back to GitHub
/// rather than leaving the person with no server.
fn viiper_remote(source: &str) -> sdw_core::update::Remote {
    let text = source.trim();
    let text = if text.is_empty() {
        "https://github.com/Alia5/VIIPER"
    } else {
        text
    };
    sdw_core::update::Remote::parse(text).unwrap_or_else(|_| {
        sdw_core::update::Remote::parse("https://github.com/Alia5/VIIPER").expect("github remote")
    })
}

/// The VIIPER asset the current OS and CPU need.
fn viiper_asset_name() -> Result<&'static str, String> {
    Ok(match (platform(), std::env::consts::ARCH) {
        ("linux", "x86_64") => "viiper-linux-amd64",
        ("linux", "aarch64") => "viiper-linux-arm64",
        ("windows", "x86_64") => "viiper-windows-amd64.exe",
        ("windows", "aarch64") => "viiper-windows-arm64.exe",
        _ => {
            return Err("Готовый бинарник VIIPER есть только для Linux и Windows x64/arm64".into())
        }
    })
}

fn fetch_text(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .user_agent(concat!("sdw-widgets/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut r = agent.get(url).call().map_err(|e| e.to_string())?;
    r.body_mut().read_to_string().map_err(|e| e.to_string())
}

fn install_viiper(ctx: &JobCtx, logger: &Logger, source: &str) -> Result<(), String> {
    let remote = viiper_remote(source);
    let asset_name = viiper_asset_name()?;
    ctx.line(format!("Источник VIIPER: {}", remote.label()));
    let info = sdw_core::update::check_at(&remote)
        .map_err(|e| format!("не удалось получить список релизов: {e}"))?;
    ctx.line(format!("Последний релиз VIIPER: {}", info.latest));
    let asset = info
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .cloned()
        .ok_or_else(|| format!("в релизе {} нет файла {asset_name}", info.latest))?;
    let sums = info
        .assets
        .iter()
        .find(|a| a.name == format!("{asset_name}.SHA256SUMS"))
        .cloned();
    let dest = crate::config::viiper_bin_path();
    ctx.line(format!("Скачиваю {}", asset.url));
    let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    sdw_core::update::download(&asset, &dest, progress).map_err(|e| format!("скачивание: {e}"))?;
    match sums {
        Some(sums) => match fetch_text(&sums.url) {
            Ok(text) => {
                if let Some(expected) = text
                    .lines()
                    .find(|l| l.contains(asset_name))
                    .and_then(|l| l.split_whitespace().next())
                {
                    let bytes = std::fs::read(&dest).map_err(|e| e.to_string())?;
                    if verify_sha256(&bytes, expected) {
                        ctx.line("SHA-256 совпал");
                    } else {
                        let _ = std::fs::remove_file(&dest);
                        return Err("SHA-256 не совпал: скачивание отменено".into());
                    }
                } else {
                    ctx.line("в SHA256SUMS нет строки для файла, проверка пропущена");
                }
            }
            Err(e) => ctx.line(format!("SHA256SUMS не прочитан ({e}), проверка пропущена")),
        },
        None => ctx.line("SHA256SUMS недоступен, проверка пропущена"),
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

/// One tool found through `PATH`, for the Windows list.
#[cfg(windows)]
fn windows_component(
    id: &str,
    name: &str,
    purpose: &str,
    hint: &str,
    needs_root: bool,
) -> ComponentStatus {
    let found = sdw_core::util::which(id);
    let version = found
        .as_ref()
        .and_then(|path| Command::new(path).arg("-version").output().ok())
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    ComponentStatus {
        id: id.into(),
        name: name.into(),
        installed: found.is_some(),
        version,
        purpose: purpose.into(),
        hint: hint.into(),
        needs_root,
    }
}

/// Install the selected components. Runs inside a [`crate::state::JobManager`] job.
pub fn install(
    components: &[String],
    sudo_password: &str,
    viiper_source: &str,
    ctx: &JobCtx,
    logger: &Logger,
) -> Result<(), String> {
    ctx.line(format!("SD Widgets {VERSION} · установка"));
    let mut need_packages = Vec::new();
    for c in components {
        match c.as_str() {
            "viiper" => {
                if let Err(e) = install_viiper(ctx, logger, viiper_source) {
                    ctx.error(format!("VIIPER: {e}"));
                    return Err(e);
                }
            }
            "usbip" | "grim" | "ffmpeg" | "ydotool" | "wtype" | "krfb" | "vdd" => {
                need_packages.push(c.clone());
            }
            other => ctx.line(format!("Неизвестный компонент: {other}")),
        }
    }
    if !need_packages.is_empty() {
        #[cfg(windows)]
        {
            if let Err(e) = install_windows_deps(&need_packages, ctx) {
                ctx.error(format!("windows deps: {e}"));
                return Err(e);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = sudo_password;
            if let Err(e) = install_packages(&need_packages, sudo_password, ctx) {
                ctx.error(format!("пакеты: {e}"));
                return Err(e);
            }
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
