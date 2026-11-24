//! Windows displays: what exists, where it sits and how to enable a virtual one.
//!
//! The monitor backend talks to the Win32 display API directly instead of
//! parsing PowerShell output: `EnumDisplayDevicesW` knows every adapter and
//! monitor, `EnumDisplaySettingsW` gives the mode and position of each, and
//! `ChangeDisplaySettingsExW` applies a new one. The Virtual Display Driver is
//! configured through its own XML and control pipe.

use crate::state::Logger;
use crate::vdd;
use sdw_core::model::DisplayOutput;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_TYPE,
    CDS_UPDATEREGISTRY, DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP,
    DISPLAY_DEVICE_MIRRORING_DRIVER, DISPLAY_DEVICE_PRIMARY_DEVICE, DISP_CHANGE_SUCCESSFUL,
    DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION, ENUM_CURRENT_SETTINGS,
    ENUM_DISPLAY_SETTINGS_MODE,
};

/// The control pipe the driver itself listens on. It is open to every user, so
/// reloading the adapter needs no administrator prompt.
const VDD_PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";

/// Every attached display, active or not.
pub fn outputs() -> Result<Vec<DisplayOutput>, String> {
    let mut out = Vec::new();
    for index in 0..32 {
        let mut adapter = DISPLAY_DEVICEW::default();
        adapter.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        // SAFETY: `adapter` is a valid, correctly sized DISPLAY_DEVICEW.
        let found =
            unsafe { EnumDisplayDevicesW(PCWSTR::null(), index, &mut adapter, 0) }.as_bool();
        if !found {
            break;
        }
        let flags = adapter.StateFlags;
        if !flags.contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP)
            || flags.contains(DISPLAY_DEVICE_MIRRORING_DRIVER)
        {
            continue;
        }
        let name = wide_text(&adapter.DeviceName);
        if name.is_empty() {
            continue;
        }
        let adapter_name = wide_text(&adapter.DeviceString);

        let mut monitor = DISPLAY_DEVICEW::default();
        monitor.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        // SAFETY: the adapter's device name is a NUL-terminated wide string.
        let _ =
            unsafe { EnumDisplayDevicesW(PCWSTR(adapter.DeviceName.as_ptr()), 0, &mut monitor, 0) };
        let monitor_name = wide_text(&monitor.DeviceString);
        let monitor_id = wide_text(&monitor.DeviceID);

        let mut mode = DEVMODEW::default();
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        // SAFETY: `mode` is a valid DEVMODEW and `name` is NUL-terminated.
        let active = unsafe {
            EnumDisplaySettingsW(
                PCWSTR(adapter.DeviceName.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
        }
        .as_bool();
        // SAFETY: the position lives in the union variant the API fills in.
        let position = unsafe { mode.Anonymous1.Anonymous2.dmPosition };
        let (width, height, refresh) = if active {
            (
                mode.dmPelsWidth,
                mode.dmPelsHeight,
                mode.dmDisplayFrequency as f32,
            )
        } else {
            (0, 0, 0.0)
        };

        let haystack = format!("{adapter_name} {monitor_name} {monitor_id}").to_lowercase();
        let virtual_output = ["virtual display", "virtualdisplay", "iddsample", "mttvdd"]
            .iter()
            .any(|needle| haystack.contains(needle));
        let description = match (adapter_name.is_empty(), monitor_name.is_empty()) {
            (false, false) => format!("{adapter_name} · {monitor_name}"),
            (false, true) => adapter_name,
            (true, false) => monitor_name,
            (true, true) => String::new(),
        };
        out.push(DisplayOutput {
            name,
            description,
            width,
            height,
            refresh,
            x: position.x,
            y: position.y,
            scale: 1.0,
            primary: flags.contains(DISPLAY_DEVICE_PRIMARY_DEVICE),
            virtual_output,
            enabled: active && width > 0 && height > 0,
        });
    }
    Ok(out)
}

/// Whether the Virtual Display Driver is installed and can be controlled.
pub fn vdd_ready() -> bool {
    vdd::settings_path().is_some() || pipe_open().is_ok()
}

/// Human-readable state for the settings window and the log.
pub fn vdd_status() -> String {
    match vdd::settings_path() {
        Some(path) => path.display().to_string(),
        None if pipe_open().is_ok() => "Virtual Display Driver (канал найден)".into(),
        None => String::new(),
    }
}

/// Prepare a real virtual monitor and put it at `x`,`y`, returning its device
/// name and the mode Windows accepted.
///
/// The driver reads its XML only when the adapter starts, so a new resolution
/// is written first and the adapter reloaded through the control pipe. When the
/// settings live in a protected folder the write asks for elevation once; the
/// reload itself never needs it.
pub fn prepare_virtual(
    width: u32,
    height: u32,
    refresh: u32,
    x: i32,
    y: i32,
    logger: &Logger,
) -> Result<(String, u32, u32, String), String> {
    let path = vdd::settings_path().ok_or_else(|| {
        "Virtual Display Driver не установлен. Установите его (winget install \
         VirtualDrivers.Virtual-Display-Driver) или включите зеркало экрана."
            .to_string()
    })?;

    let mut xml = std::fs::read_to_string(&path).unwrap_or_default();
    if xml.trim().is_empty() {
        xml = vdd::minimal(width, height, refresh);
    }
    let mut next = vdd::set_count(&xml, 1)?;
    next = vdd::ensure_mode(&next, width, height, refresh)?;
    let mut note = String::new();
    if next != xml || !path.is_file() {
        write_settings(&path, &next)?;
        reload_driver()
            .map_err(|e| format!("настройки записаны, но драйвер не перезагрузился: {e}"))?;
        note = "VDD настроен и перезагружен".into();
        logger.info(format!(
            "VDD: {}x{}@{} добавлен в {}",
            width,
            height,
            refresh,
            path.display()
        ));
        wait_for_vdd(None, 12_000)?;
    }

    let mut list = outputs().unwrap_or_default();
    let target = list
        .iter()
        .find(|o| o.virtual_output && o.enabled)
        .or_else(|| list.iter().find(|o| o.virtual_output))
        .map(|o| o.name.clone())
        .ok_or_else(|| {
            "Virtual Display Driver установлен, но Windows не показывает его монитор. \
             Проверьте, что в vdd_settings.xml есть <count>1</count>, и перезагрузите драйвер."
                .to_string()
        })?;

    // A new display often arrives in duplicate mode. Extending first gives it a
    // place of its own; the exact position is applied right after.
    let _ = std::process::Command::new("DisplaySwitch.exe")
        .arg("/extend")
        .status();
    std::thread::sleep(std::time::Duration::from_millis(900));
    list = outputs().unwrap_or(list);

    let (accepted_w, accepted_h, applied_note) = apply_mode(&target, width, height, refresh, x, y)?;
    if !applied_note.is_empty() {
        note = if note.is_empty() {
            applied_note
        } else {
            format!("{note}; {applied_note}")
        };
    }
    // Give the desktop a moment to move windows and publish the new rectangle.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let now = outputs().unwrap_or_default();
        if now
            .iter()
            .any(|o| o.name == target && o.enabled && o.x == x && o.y == y)
        {
            break;
        }
    }
    Ok((target, accepted_w, accepted_h, note))
}

/// Turn a virtual display off again, leaving the driver installed for the next
/// stream.
pub fn disable_virtual(name: &str) {
    if name.is_empty() || !name.starts_with(r"\\.\DISPLAY") {
        return;
    }
    let mut mode = DEVMODEW::default();
    mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    let device = wide(name);
    // Disabling the source is what the display settings page does; the mode
    // stays in the registry, so the next start finds its size again.
    // SAFETY: `device` is NUL-terminated and `mode` is correctly sized.
    let _ = unsafe {
        ChangeDisplaySettingsExW(
            PCWSTR(device.as_ptr()),
            Some(&mode),
            None,
            CDS_UPDATEREGISTRY,
            None,
        )
    };
    // SAFETY: applying with no device name commits the pending changes.
    let _ = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
}

// ---------------------------------------------------------------- modes

/// Apply size and position to one display. `width`×`height` is used when the
/// display advertises it; otherwise the nearest mode of the same shape wins and
/// the capture is scaled by ffmpeg anyway.
fn apply_mode(
    device: &str,
    width: u32,
    height: u32,
    refresh: u32,
    x: i32,
    y: i32,
) -> Result<(u32, u32, String), String> {
    let modes = modes(device);
    let chosen = choose_mode(&modes, width, height, refresh);
    let (mut w, mut h, mut hz) = chosen.unwrap_or((width, height, refresh));
    let mut note = String::new();
    if (w, h) != (width, height) && !modes.is_empty() {
        note = format!("VDD не отдаёт {width}x{height}, используется {w}x{h}");
    }
    let device_w = wide(device);
    let apply = |w: u32, h: u32, hz: u32| -> i32 {
        let mut mode = DEVMODEW::default();
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        mode.dmPelsWidth = w;
        mode.dmPelsHeight = h;
        mode.dmDisplayFrequency = hz;
        mode.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        // SAFETY: positions live in the union variant Windows fills in.
        mode.Anonymous1.Anonymous2.dmPosition.x = x;
        // SAFETY: same union, the y coordinate next to x.
        mode.Anonymous1.Anonymous2.dmPosition.y = y;
        // SAFETY: `device_w` is NUL-terminated, `mode` is a valid DEVMODEW.
        let rc = unsafe {
            ChangeDisplaySettingsExW(
                PCWSTR(device_w.as_ptr()),
                Some(&mode),
                None,
                CDS_UPDATEREGISTRY,
                None,
            )
        };
        rc.0
    };
    let mut rc = apply(w, h, hz);
    if rc != DISP_CHANGE_SUCCESSFUL.0 {
        // Some drivers accept a size but not a refresh rate; ask again for the
        // size alone before giving up.
        rc = apply(w, h, 0);
        if rc == DISP_CHANGE_SUCCESSFUL.0 {
            hz = 0;
        }
    }
    if rc != DISP_CHANGE_SUCCESSFUL.0 {
        // Keep the mode Windows already had and move the monitor anyway.
        let current = modes
            .iter()
            .find(|m| m.0 == width && m.1 == height)
            .copied();
        if let Some((cw, ch, chz)) = current {
            w = cw;
            h = ch;
            hz = chz;
            rc = apply(w, h, hz);
        }
    }
    // SAFETY: applying with no device name commits the pending changes.
    let _ = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
    if rc != DISP_CHANGE_SUCCESSFUL.0 {
        return Err(format!(
            "Windows не принял режим {w}x{h}@{} для {device} (код {rc})",
            if hz == 0 { 0 } else { hz }
        ));
    }
    Ok((w, h, note))
}

/// All modes a display advertises, as `(width, height, refresh)`.
fn modes(device: &str) -> Vec<(u32, u32, u32)> {
    let device_w = wide(device);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for index in 0..512u32 {
        let mut mode = DEVMODEW::default();
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        // SAFETY: `mode` is a valid DEVMODEW and the name is NUL-terminated.
        let ok = unsafe {
            EnumDisplaySettingsW(
                PCWSTR(device_w.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(index),
                &mut mode,
            )
        }
        .as_bool();
        if !ok {
            break;
        }
        let item = (mode.dmPelsWidth, mode.dmPelsHeight, mode.dmDisplayFrequency);
        if item.0 >= 320 && item.1 >= 240 && seen.insert(item) {
            out.push(item);
        }
    }
    out
}

/// The exchangeable half of [`apply_mode`]: no Win32, so it is unit-tested.
pub(crate) fn choose_mode(
    modes: &[(u32, u32, u32)],
    width: u32,
    height: u32,
    refresh: u32,
) -> Option<(u32, u32, u32)> {
    if modes.is_empty() {
        return None;
    }
    let distance = |m: &(u32, u32, u32)| {
        let dw = m.0.abs_diff(width);
        let dh = m.1.abs_diff(height);
        let hz = m.2.abs_diff(refresh);
        (dw + dh, hz)
    };
    if let Some(exact) = modes
        .iter()
        .filter(|m| m.0 == width && m.1 == height)
        .min_by_key(|m| m.2.abs_diff(refresh))
    {
        return Some(*exact);
    }
    // The same shape (16:10 for the Deck) at another size is a better picture
    // than a differently shaped mode with the same pixel count.
    let ratio = width as f32 / height.max(1) as f32;
    let same_shape = modes
        .iter()
        .filter(|m| ((m.0 as f32 / m.1.max(1) as f32) - ratio).abs() < 0.02)
        .min_by_key(|m| {
            (
                m.0.abs_diff(width) + m.1.abs_diff(height),
                m.2.abs_diff(refresh),
            )
        });
    if let Some(m) = same_shape {
        return Some(*m);
    }
    modes.iter().copied().min_by_key(|m| distance(m))
}

// ---------------------------------------------------------------- VDD control

/// Write the settings file, asking for elevation only when the folder is
/// protected (the default `C:\VirtualDisplayDriver` needs it).
fn write_settings(path: &std::path::Path, text: &str) -> Result<(), String> {
    if std::fs::write(path, text).is_ok() {
        return Ok(());
    }
    if !cfg!(windows) {
        return Err(format!("не удалось записать {}", path.display()));
    }
    let tmp = std::env::temp_dir().join(format!("sdw-vdd-{}.xml", sdw_core::model::random_id(6)));
    std::fs::write(&tmp, text).map_err(|e| format!("временный файл: {e}"))?;
    let backup = path.with_extension("xml.sdw-backup");
    let script =
        std::env::temp_dir().join(format!("sdw-vdd-{}.ps1", sdw_core::model::random_id(6)));
    let body = format!(
        "$ErrorActionPreference = 'Stop'\n\
         if (-not (Test-Path -LiteralPath '{backup}')) {{ Copy-Item -LiteralPath '{dest}' \
         -Destination '{backup}' -Force }}\n\
         Copy-Item -LiteralPath '{tmp}' -Destination '{dest}' -Force\n",
        backup = ps_quote(&backup.display().to_string()),
        dest = ps_quote(&path.display().to_string()),
        tmp = ps_quote(&tmp.display().to_string()),
    );
    std::fs::write(&script, body).map_err(|e| format!("сценарий: {e}"))?;
    let command = format!(
        "Start-Process -FilePath powershell.exe -Verb RunAs -Wait -ArgumentList \
         '-NoProfile','-ExecutionPolicy','Bypass','-File','{}'",
        ps_quote(&script.display().to_string())
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &command])
        .status()
        .map_err(|e| format!("powershell: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&script);
    let written = std::fs::read_to_string(path).unwrap_or_default();
    if !status.success() || written != text {
        return Err(
            "нет прав на запись vdd_settings.xml: подтвердите запрос администратора \
             или запустите демон от имени администратора"
                .into(),
        );
    }
    Ok(())
}

fn reload_driver() -> Result<(), String> {
    pipe_send("RELOAD_DRIVER")
}

/// Send one command to the driver's named pipe. It expects a NUL-terminated
/// UTF-16 message and answers with its log, which the daemon does not need.
fn pipe_send(command: &str) -> Result<(), String> {
    use std::io::Write;
    let mut pipe = pipe_open()?;
    let mut bytes = Vec::with_capacity(command.len() * 2 + 2);
    for unit in command.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 0]);
    pipe.write_all(&bytes)
        .map_err(|e| format!("канал VDD: {e}"))?;
    let _ = pipe.flush();
    Ok(())
}

fn pipe_open() -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(VDD_PIPE)
        .map_err(|e| format!("VDD-канал недоступен: {e}"))
}

/// Wait for the driver to publish a virtual monitor. A fresh adapter takes a
/// few seconds on a slow machine, so this polls instead of trusting one sleep.
fn wait_for_vdd(before: Option<&[String]>, timeout_ms: u64) -> Result<String, String> {
    let started = std::time::Instant::now();
    let mut last = Vec::new();
    while started.elapsed() < std::time::Duration::from_millis(timeout_ms) {
        if let Ok(list) = outputs() {
            last = list.clone();
            if let Some(found) = list.iter().find(|o| {
                o.virtual_output
                    && o.enabled
                    && before.map(|b| !b.contains(&o.name)).unwrap_or(true)
            }) {
                return Ok(found.name.clone());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if let Some(any) = last.iter().find(|o| o.virtual_output) {
        return Ok(any.name.clone());
    }
    Err("VDD перезагружен, но виртуальный монитор не появился".into())
}

// ---------------------------------------------------------------- helpers

fn wide(text: &str) -> Vec<u16> {
    let mut out: Vec<u16> = std::ffi::OsStr::new(text).encode_wide().collect::<Vec<_>>();
    out.push(0);
    out
}

fn wide_text(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

/// PowerShell single-quoted string: a quote is doubled, everything else is
/// literal.
fn ps_quote(text: &str) -> String {
    text.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::choose_mode;

    #[test]
    fn an_exact_mode_wins() {
        let modes = [(1920, 1080, 60), (1280, 800, 60), (1280, 800, 75)];
        assert_eq!(choose_mode(&modes, 1280, 800, 70), Some((1280, 800, 75)));
    }

    #[test]
    fn the_same_shape_is_next_best() {
        let modes = [(1920, 1080, 60), (1600, 1000, 60)];
        assert_eq!(choose_mode(&modes, 1280, 800, 60), Some((1600, 1000, 60)));
    }

    #[test]
    fn anything_is_better_than_nothing() {
        let modes = [(1920, 1080, 60), (1024, 768, 60)];
        assert_eq!(choose_mode(&modes, 1280, 800, 60), Some((1920, 1080, 60)));
        assert_eq!(choose_mode(&[], 1280, 800, 60), None);
    }
}
