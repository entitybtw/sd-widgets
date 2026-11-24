//! Bluetooth on Windows through the WinRT device APIs.
//!
//! The daemon reads the paired devices Windows already knows about, their
//! connection state and the radio switch, and pairs or unpairs a device when
//! the deck asks. Windows owns the connection policy for headsets and
//! keyboards: it connects a paired device when something needs it, so there is
//! no "connect" call to make from here, and the page says so instead of
//! pretending a button did it.
//!
//! The cache and the job live here for the same reason as on Linux: a device
//! read costs several asynchronous WinRT calls, so a poll answers with the last
//! snapshot and the fresh one lands on the next poll.

use crate::win::{with_com, WaitAsync};

use sdw_core::model::{BluetoothDevice, BluetoothStatus};
use sdw_core::{t, Lang};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Devices::Bluetooth::{
    BluetoothAdapter, BluetoothConnectionStatus, BluetoothDevice as WinBluetoothDevice,
};
use windows::Devices::Enumeration::{DeviceInformation, DevicePairingResultStatus};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

struct Cached {
    status: BluetoothStatus,
    at: Instant,
    reading: bool,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

#[derive(Default)]
struct Job {
    seq: u64,
    running: bool,
    ok: bool,
    message: String,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

fn job_running() -> bool {
    JOB.lock()
        .ok()
        .and_then(|j| j.as_ref().map(|j| j.running))
        .unwrap_or(false)
}

/// Mark the snapshot stale without throwing the list away.
pub fn invalidate() {
    if let Ok(mut cache) = CACHE.lock() {
        if let Some(c) = cache.as_mut() {
            c.at = Instant::now() - Duration::from_secs(60);
        }
    }
}

pub fn status(lang: Lang) -> BluetoothStatus {
    let mut due = false;
    let running = job_running();
    let mut snapshot = if let Ok(mut cache) = CACHE.lock() {
        let c = cache.get_or_insert_with(|| Cached {
            status: BluetoothStatus::default(),
            at: Instant::now() - Duration::from_secs(60),
            reading: false,
        });
        let ttl = if running {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(4)
        };
        if c.at.elapsed() >= ttl && !c.reading {
            c.reading = true;
            due = true;
        }
        c.status.clone()
    } else {
        BluetoothStatus::default()
    };
    if let Ok(job) = JOB.lock() {
        if let Some(job) = job.as_ref() {
            snapshot.action_seq = job.seq;
            snapshot.action_running = job.running;
            snapshot.action_ok = job.ok;
            snapshot.action_message = job.message.clone();
        }
    }
    if due {
        std::thread::spawn(move || {
            refresh(lang);
        });
    }
    snapshot
}

/// Fill the cache in the background without waiting for a page to ask.
pub fn warm(lang: Lang) {
    let Ok(mut cache) = CACHE.lock() else {
        return;
    };
    match cache.as_mut() {
        Some(c) => {
            if c.reading {
                return;
            }
            c.reading = true;
        }
        None => {
            *cache = Some(Cached {
                status: BluetoothStatus::default(),
                at: Instant::now() - Duration::from_secs(60),
                reading: true,
            });
        }
    }
    drop(cache);
    std::thread::spawn(move || {
        refresh(lang);
    });
}

pub fn refresh(lang: Lang) -> BluetoothStatus {
    let out = read_status(lang);
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(Cached {
            status: out.clone(),
            at: Instant::now(),
            reading: false,
        });
    }
    out
}

fn read_status(lang: Lang) -> BluetoothStatus {
    let _ = lang;
    let mut out = BluetoothStatus::default();
    let result = with_com(|| -> Result<(), String> {
        let radio = bluetooth_radio()?;
        let Some(radio) = radio else {
            return Ok(());
        };
        out.available = true;
        out.powered = radio.State().map(|s| s == RadioState::On).unwrap_or(false);
        out.adapter = adapter_address();
        if !out.powered {
            return Ok(());
        }
        for paired in [true, false] {
            for device in known_devices(paired)? {
                out.devices.push(device);
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        out.error = e;
    }
    // Connected first, then paired, then by name.
    out.devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn bluetooth_radio() -> Result<Option<Radio>, String> {
    let radios = Radio::GetRadiosAsync()
        .map_err(|e| format!("Bluetooth: {e}"))?
        .wait()
        .map_err(|e| format!("Bluetooth: {e}"))?;
    let count = radios.Size().unwrap_or(0);
    for index in 0..count {
        let Ok(radio) = radios.GetAt(index) else {
            continue;
        };
        if radio
            .Kind()
            .map(|k| k == RadioKind::Bluetooth)
            .unwrap_or(false)
        {
            return Ok(Some(radio));
        }
    }
    Ok(None)
}

fn adapter_address() -> String {
    BluetoothAdapter::GetDefaultAsync()
        .and_then(|op| op.wait())
        .ok()
        .and_then(|adapter| adapter.BluetoothAddress().ok())
        .map(format_address)
        .unwrap_or_default()
}

/// Every device Windows knows about, paired or only seen before.
fn known_devices(paired: bool) -> Result<Vec<BluetoothDevice>, String> {
    let selector = WinBluetoothDevice::GetDeviceSelectorFromPairingState(paired)
        .map_err(|e| format!("Bluetooth: {e}"))?;
    let list = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|e| format!("Bluetooth: {e}"))?
        .wait()
        .map_err(|e| format!("Bluetooth: {e}"))?;
    let mut out = Vec::new();
    for index in 0..list.Size().unwrap_or(0) {
        let Ok(info) = list.GetAt(index) else {
            continue;
        };
        let name = info.Name().map(|s| s.to_string()).unwrap_or_default();
        let id = info.Id().map(|s| s.to_string()).unwrap_or_default();
        let device = WinBluetoothDevice::FromIdAsync(&HSTRING::from(&id))
            .and_then(|op| op.wait())
            .ok();
        let address = device
            .as_ref()
            .and_then(|d| d.BluetoothAddress().ok())
            .map(format_address)
            .filter(|a| !a.is_empty())
            .or_else(|| address_from_id(&id))
            .unwrap_or_default();
        let connected = device
            .as_ref()
            .and_then(|d| d.ConnectionStatus().ok())
            .map(|s| s == BluetoothConnectionStatus::Connected)
            .unwrap_or(false);
        out.push(BluetoothDevice {
            address,
            name,
            paired,
            connected,
            // Windows does not tell an app whether a paired device is in range;
            // only a live connection is a fact.
            available: connected,
            ..Default::default()
        });
    }
    Ok(out)
}

/// `00:11:22:33:44:55` from the 48-bit address.
fn format_address(value: u64) -> String {
    if value == 0 {
        return String::new();
    }
    let bytes = value.to_be_bytes();
    bytes[2..]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// The device id carries the address as `Bluetooth#Bluetooth00:11:...`; read it
/// when the radio is off and `FromIdAsync` cannot answer.
fn address_from_id(id: &str) -> Option<String> {
    let bytes = id.as_bytes();
    for start in 0..bytes.len().saturating_sub(16) {
        let candidate = &bytes[start..start + 17];
        let looks = candidate.iter().enumerate().all(|(i, b)| {
            if i % 3 == 2 {
                *b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        });
        if looks {
            return Some(String::from_utf8_lossy(candidate).to_ascii_uppercase());
        }
    }
    None
}

pub fn valid_address(address: &str) -> bool {
    address.len() == 17
        && address.chars().enumerate().all(|(i, c)| {
            if i % 3 == 2 {
                c == ':'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// Run one declared action on the calling thread.
pub fn action(action: &str, address: &str, lang: Lang) -> Result<String, String> {
    invalidate();
    let action = action.trim();
    let result = action_impl(action, address, lang);
    invalidate();
    schedule_refresh(lang);
    result
}

/// Start one declared action on its own thread and answer at once.
pub fn start(action: &str, address: &str, lang: Lang) -> Result<(), String> {
    let action = action.trim();
    let known = matches!(
        action,
        "scan"
            | "scan_off"
            | "power_on"
            | "power_off"
            | "power_cycle"
            | "pair_connect"
            | "connect"
            | "disconnect"
            | "pair"
            | "trust"
            | "untrust"
            | "remove"
            | "info"
    );
    if !known {
        return Err(format!(
            "{} {action}",
            t!(lang, "неизвестное действие:", "unknown action:")
        ));
    }
    let needs_address = !matches!(
        action,
        "scan" | "scan_off" | "power_on" | "power_off" | "power_cycle"
    );
    if needs_address && !valid_address(address) {
        return Err(t!(lang, "неверный адрес устройства", "invalid device address").to_string());
    }
    {
        let mut guard = JOB.lock().map_err(|_| "bluetooth job lock".to_string())?;
        let job = guard.get_or_insert_with(Job::default);
        if job.running {
            return Err(t!(
                lang,
                "предыдущая команда ещё выполняется",
                "the previous command is still running"
            )
            .to_string());
        }
        job.seq = job.seq.wrapping_add(1);
        job.running = true;
        job.ok = false;
        job.message.clear();
    }
    invalidate();
    let (action, address) = (action.to_string(), address.to_string());
    std::thread::spawn(move || {
        let result = action_impl(&action, &address, lang);
        let (ok, message) = match result {
            Ok(message) => (true, message),
            Err(e) => (false, e),
        };
        if let Ok(mut guard) = JOB.lock() {
            if let Some(job) = guard.as_mut() {
                job.running = false;
                job.ok = ok;
                job.message = message;
            }
        }
        invalidate();
        schedule_refresh(lang);
    });
    Ok(())
}

fn schedule_refresh(lang: Lang) {
    let due = if let Ok(mut cache) = CACHE.lock() {
        match cache.as_mut() {
            Some(c) => {
                if c.reading {
                    false
                } else {
                    c.reading = true;
                    true
                }
            }
            None => {
                *cache = Some(Cached {
                    status: BluetoothStatus::default(),
                    at: Instant::now() - Duration::from_secs(60),
                    reading: true,
                });
                true
            }
        }
    } else {
        false
    };
    if due {
        std::thread::spawn(move || {
            refresh(lang);
        });
    }
}

fn action_impl(action: &str, address: &str, lang: Lang) -> Result<String, String> {
    let invalid = || t!(lang, "неверный адрес устройства", "invalid device address").to_string();
    match action {
        "power_on" => set_power(true),
        "power_off" => set_power(false),
        "power_cycle" => {
            set_power(false)?;
            std::thread::sleep(Duration::from_millis(600));
            set_power(true)
        }
        // Windows keeps its own device list; a scan is what the Settings page
        // does. Saying this plainly is better than a button that lies.
        "scan" | "scan_off" => Err(t!(
            lang,
            "поиск новых устройств: Параметры → Bluetooth и устройства. Windows не отдаёт \
             приложениям живой список найденных устройств",
            "to look for new devices use Settings → Bluetooth & devices; Windows does not \
             give applications a live list of discovered devices"
        )
        .to_string()),
        "pair" | "pair_connect" => {
            if !valid_address(address) {
                return Err(invalid());
            }
            pair(address)?;
            Ok(message(lang, "pair", address))
        }
        "remove" => {
            if !valid_address(address) {
                return Err(invalid());
            }
            unpair(address)?;
            Ok(message(lang, "remove", address))
        }
        // Windows owns the connection policy: a paired headset is connected
        // when an application uses it, and nothing an app can call forces it.
        "connect" | "disconnect" | "trust" | "untrust" | "info" => Err(t!(
            lang,
            "Windows подключает спаренное устройство сам, когда оно нужно; \
             подключение вручную делается в Параметрах",
            "Windows connects a paired device by itself when something needs it; \
             manual connection lives in Settings"
        )
        .to_string()),
        other => Err(format!(
            "{} {other}",
            t!(lang, "неизвестное действие:", "unknown action:")
        )),
    }
}

fn set_power(on: bool) -> Result<String, String> {
    let radio = with_com(bluetooth_radio)?
        .ok_or_else(|| "в компьютере нет адаптера Bluetooth".to_string())?;
    let state = if on { RadioState::On } else { RadioState::Off };
    let access = radio
        .SetStateAsync(state)
        .and_then(|op| op.wait())
        .map_err(|e| format!("Bluetooth: {e}"))?;
    if access != RadioAccessStatus::Allowed {
        return Err(
            "Windows не разрешила переключить радио Bluetooth; включите его в Параметрах".into(),
        );
    }
    Ok(if on {
        "bluetooth включён".into()
    } else {
        "bluetooth выключен".into()
    })
}

/// The device id Windows uses for a pretty Bluetooth address.
fn device_id(address: &str) -> Result<HSTRING, String> {
    let compact = address.replace(':', "").to_ascii_uppercase();
    Ok(HSTRING::from(format!(
        "Bluetooth#Bluetooth{address}#{compact}"
    )))
}

fn pair(address: &str) -> Result<(), String> {
    with_com(|| {
        let info = DeviceInformation::CreateFromIdAsync(&device_id(address)?)
            .map_err(|e| format!("Bluetooth: {e}"))?
            .wait()
            .map_err(|e| format!("Bluetooth: {e}"))?;
        let pairing = info.Pairing().map_err(|e| format!("Bluetooth: {e}"))?;
        if pairing.IsPaired().unwrap_or(false) {
            return Ok(());
        }
        let result = pairing
            .PairAsync()
            .map_err(|e| format!("Bluetooth: {e}"))?
            .wait_for(Duration::from_secs(120))
            .map_err(|e| format!("Bluetooth: {e}"))?;
        match result.Status() {
            Ok(status)
                if status == DevicePairingResultStatus::Paired
                    || status == DevicePairingResultStatus::AlreadyPaired =>
            {
                Ok(())
            }
            Ok(status) => Err(format!("спаривание отклонено (код {})", status.0)),
            Err(e) => Err(format!("Bluetooth: {e}")),
        }
    })
}

fn unpair(address: &str) -> Result<(), String> {
    with_com(|| {
        let info = DeviceInformation::CreateFromIdAsync(&device_id(address)?)
            .map_err(|e| format!("Bluetooth: {e}"))?
            .wait()
            .map_err(|e| format!("Bluetooth: {e}"))?;
        let pairing = info.Pairing().map_err(|e| format!("Bluetooth: {e}"))?;
        pairing
            .UnpairAsync()
            .map_err(|e| format!("Bluetooth: {e}"))?
            .wait()
            .map_err(|e| format!("Bluetooth: {e}"))?;
        Ok(())
    })
}

fn message(lang: Lang, verb: &str, address: &str) -> String {
    let what = match verb {
        "pair" => t!(lang, "спарено", "paired"),
        "remove" => t!(lang, "удалено", "removed"),
        _ => t!(lang, "готово", "done"),
    };
    format!("{address}: {what}")
}
