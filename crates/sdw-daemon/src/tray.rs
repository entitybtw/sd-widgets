//! StatusNotifierItem tray icon for the daemon.
//!
//! Implemented straight on D-Bus so it works without GTK/Qt or libappindicator:
//! `org.kde.StatusNotifierItem` plus `com.canonical.dbusmenu`. The menu has an
//! "open window" action routed back into the GUI, a browser shortcut and quit.
//! When no StatusNotifierWatcher is running (plain sway/niri without a bar) the
//! daemon keeps running; there is simply nothing to attach the icon to.

use crate::state::AppState;
use sdw_core::t;
use sdw_core::Lang;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

pub struct TrayHandle {
    stop: Arc<AtomicBool>,
}

impl TrayHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for TrayHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the tray if the session bus and a StatusNotifierWatcher are present.
pub fn start(state: Arc<AppState>) -> Option<TrayHandle> {
    #[cfg(target_os = "linux")]
    {
        start_linux(state)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        None
    }
}

#[cfg(target_os = "linux")]
fn start_linux(state: Arc<AppState>) -> Option<TrayHandle> {
    use zbus::blocking::Connection;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let bus_name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let title = format!("sd-widgets · {}", sdw_core::util::hostname());
    let lang = Lang::from_code(&state.cfg().lang);
    let port = state.cfg().api_port;

    let (ready_tx, ready_rx) = channel::<bool>();
    let logger = state.logger.clone();
    let stop_for_thread = stop_thread.clone();
    std::thread::Builder::new()
        .name("sdw-tray".into())
        .spawn(move || {
            let sni = Sni {
                title,
                pixmaps: icon_pixmaps(),
                menu: menu_path(),
            };
            let menu = Menu { lang, port };
            let name_for_conn = bus_name.clone();
            let built = (move || -> zbus::Result<Connection> {
                let conn = Connection::session()?;
                conn.object_server().at("/StatusNotifierItem", sni)?;
                conn.object_server().at("/MenuBar", menu)?;
                conn.request_name(name_for_conn)?;
                Ok(conn)
            })();
            let conn = match built {
                Ok(c) => c,
                Err(e) => {
                    logger.info(format!("трей недоступен: {e}"));
                    let _ = ready_tx.send(false);
                    return;
                }
            };
            // The watcher is what makes the icon appear in the bar.
            let registered = conn.call_method(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                Some("org.kde.StatusNotifierWatcher"),
                "RegisterStatusNotifierItem",
                &(bus_name.as_str(),),
            );
            if let Err(e) = registered {
                logger.info(format!(
                    "трей: нет StatusNotifierWatcher ({e}); иконка в панели не появится"
                ));
                let _ = ready_tx.send(false);
                return;
            }
            logger.info("трей: иконка sd-widgets зарегистрирована");
            let _ = ready_tx.send(true);
            while !stop_for_thread.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
            }
            drop(conn);
        })
        .ok()?;

    let ok = ready_rx
        .recv_timeout(Duration::from_secs(3))
        .unwrap_or(false);
    if !ok {
        let _ = &stop; // the thread already stopped on its own
        return None;
    }
    Some(TrayHandle { stop })
}

#[cfg(target_os = "linux")]
fn menu_path() -> zbus::zvariant::OwnedObjectPath {
    zbus::zvariant::ObjectPath::from_static_str("/MenuBar")
        .expect("static menu path")
        .into()
}

#[cfg(target_os = "linux")]
struct Sni {
    title: String,
    pixmaps: Vec<(i32, i32, Vec<u8>)>,
    menu: zbus::zvariant::OwnedObjectPath,
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Sni {
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "sd-widgets"
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        &self.title
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        "sdw-widgets"
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        self.pixmaps.clone()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::zvariant::OwnedObjectPath {
        self.menu.clone()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
        (
            "sd-widgets".into(),
            self.pixmaps.clone(),
            "sd-widgets".into(),
            self.title.clone(),
        )
    }
}

#[cfg(target_os = "linux")]
struct Menu {
    lang: Lang,
    port: u16,
}

#[cfg(target_os = "linux")]
impl Menu {
    fn open_label(&self) -> &'static str {
        t!(self.lang, "открыть окно", "open window")
    }
    fn web_label(&self) -> &'static str {
        t!(self.lang, "веб-интерфейс", "web interface")
    }
    fn quit_label(&self) -> &'static str {
        t!(self.lang, "выход", "quit")
    }

    /// One menu entry as a dbusmenu `(ia{sv}av)` structure.
    fn item(&self, id: i32, label: &str, enabled: bool) -> zbus::zvariant::Value<'static> {
        let mut props: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
        props.insert("label".into(), owned(label.to_string()));
        props.insert("enabled".into(), owned(enabled));
        props.insert("visible".into(), owned(true));
        if label.is_empty() {
            props.insert("type".into(), owned("separator"));
        } else {
            props.insert("type".into(), owned("standard"));
        }
        let children: Vec<zbus::zvariant::Value<'static>> = Vec::new();
        zbus::zvariant::Value::Structure(zbus::zvariant::Structure::from((id, props, children)))
    }

    fn layout(&self) -> zbus::zvariant::Value<'static> {
        let children = vec![
            self.item(1, self.open_label(), true),
            self.item(2, self.web_label(), true),
            self.item(3, "", false),
            self.item(4, self.quit_label(), true),
        ];
        let props: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
        zbus::zvariant::Value::Structure(zbus::zvariant::Structure::from((0, props, children)))
    }
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    fn get_layout(
        &self,
        _parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (
        u32,
        (
            i32,
            HashMap<String, zbus::zvariant::OwnedValue>,
            Vec<zbus::zvariant::Value<'static>>,
        ),
    ) {
        (1, (0, HashMap::new(), vec![self.layout()]))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, zbus::zvariant::OwnedValue>)> {
        let mut out = Vec::new();
        for id in ids {
            let (label, enabled) = match id {
                1 => (self.open_label(), true),
                2 => (self.web_label(), true),
                4 => (self.quit_label(), true),
                _ => ("", false),
            };
            let mut props: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
            props.insert("label".into(), owned(label));
            props.insert("enabled".into(), owned(enabled));
            props.insert("visible".into(), owned(true));
            out.push((id, props));
        }
        out
    }

    fn get_property(&self, id: i32, name: &str) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
        let (label, enabled) = match id {
            1 => (self.open_label(), true),
            2 => (self.web_label(), true),
            4 => (self.quit_label(), true),
            _ => ("", false),
        };
        Ok(match name {
            "label" => owned(label),
            "enabled" => owned(enabled),
            "visible" => owned(true),
            "type" => owned(if id == 3 { "separator" } else { "standard" }),
            _ => owned(""),
        })
    }

    fn event(&self, id: i32, event_id: &str, _data: zbus::zvariant::Value<'_>, _timestamp: u32) {
        if event_id != "clicked" {
            return;
        }
        match id {
            1 => crate::config::request("show"),
            2 => {
                let url = format!("http://127.0.0.1:{}", self.port);
                let _ = std::process::Command::new("xdg-open")
                    .arg(&url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            4 => crate::config::request("quit"),
            _ => {}
        }
    }

    fn about_to_show(&self, _id: i32) -> bool {
        true
    }
}

#[cfg(target_os = "linux")]
fn owned(value: impl Into<zbus::zvariant::Value<'static>>) -> zbus::zvariant::OwnedValue {
    zbus::zvariant::OwnedValue::try_from(value.into())
        .unwrap_or_else(|_| zbus::zvariant::OwnedValue::try_from(0u8).unwrap())
}

/// Decode the shipped 64 px icon into the ARGB32 big-endian pixmap list that
/// StatusNotifierItem expects. Alpha is premultiplied.
#[cfg(target_os = "linux")]
fn icon_pixmaps() -> Vec<(i32, i32, Vec<u8>)> {
    const PNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/icons/sd-widgets-64.png"
    ));
    let Ok(img) = image::load_from_memory(PNG) else {
        return Vec::new();
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for px in rgba.pixels() {
        let a = u32::from(px[3]);
        data.push((u32::from(px[0]) * a / 255) as u8);
        data.push((u32::from(px[1]) * a / 255) as u8);
        data.push((u32::from(px[2]) * a / 255) as u8);
        data.push(px[3]);
    }
    vec![(w as i32, h as i32, data)]
}
