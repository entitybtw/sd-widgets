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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
    let menu_state = Arc::clone(&state);

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
            let menu = Menu {
                state: menu_state,
                revision: AtomicU32::new(1),
            };
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

    /// Left click on the icon: the same as the first menu entry.
    fn activate(&self, _x: i32, _y: i32) {
        crate::config::request("show");
    }

    /// Middle click and wheel have no meaning here, but a bar calls them and
    /// must not see "unknown method".
    fn secondary_activate(&self, _x: i32, _y: i32) {}

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}

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
    state: Arc<AppState>,
    /// Bumped when the menu changes; clients refetch on `LayoutUpdated`.
    revision: AtomicU32,
}

#[cfg(target_os = "linux")]
type MenuProps = HashMap<String, zbus::zvariant::OwnedValue>;
#[cfg(target_os = "linux")]
type MenuNode = (i32, MenuProps, Vec<zbus::zvariant::Value<'static>>);

/// Menu ids. The numbers are part of the D-Bus contract, not an enum.
#[cfg(target_os = "linux")]
const MENU_OPEN: i32 = 1;
#[cfg(target_os = "linux")]
const MENU_WEB: i32 = 2;
#[cfg(target_os = "linux")]
const MENU_SEPARATOR: i32 = 3;
#[cfg(target_os = "linux")]
const MENU_QUIT: i32 = 4;

#[cfg(target_os = "linux")]
impl Menu {
    fn lang(&self) -> Lang {
        Lang::from_code(&self.state.cfg().lang)
    }

    fn label_of(&self, id: i32) -> String {
        let lang = self.lang();
        match id {
            MENU_OPEN => t!(lang, "открыть окно", "open window").to_string(),
            MENU_WEB => t!(lang, "веб-интерфейс", "web interface").to_string(),
            MENU_QUIT => t!(lang, "выход", "quit").to_string(),
            _ => String::new(),
        }
    }

    /// The properties of one entry. `names` is what the client asked for; an
    /// empty list means everything. Unknown entries still answer with `type`
    /// so a client can drop them instead of drawing a blank row.
    fn props_of(&self, id: i32, names: &[String]) -> MenuProps {
        let mut all: MenuProps = HashMap::new();
        if id == 0 {
            // The root is a container, not an entry: giving it a type and an
            // empty label makes some clients draw a blank row above the menu.
            all.insert("children-display".into(), owned("submenu"));
            return all;
        }
        let separator = id == MENU_SEPARATOR;
        all.insert("label".into(), owned(self.label_of(id)));
        all.insert(
            "type".into(),
            owned(if separator { "separator" } else { "standard" }),
        );
        all.insert("enabled".into(), owned(!separator));
        all.insert("visible".into(), owned(true));
        if names.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|(k, _)| names.iter().any(|n| n == k))
            .collect()
    }

    /// One node of the tree. `depth` is the dbusmenu recursion depth: -1 means
    /// everything, 0 the node alone, n its n levels of children.
    fn node(&self, id: i32, depth: i32, names: &[String]) -> MenuNode {
        let expand = (id == 0 || id == -1) && (depth < 0 || depth > 0);
        let next = if depth > 0 { depth - 1 } else { depth };
        let children = if expand {
            [MENU_OPEN, MENU_WEB, MENU_SEPARATOR, MENU_QUIT]
                .into_iter()
                .map(|child| zbus::zvariant::Value::Structure(self.node(child, next, names).into()))
                .collect()
        } else {
            Vec::new()
        };
        let parent = if -1 == id { 0 } else { id };
        (parent, self.props_of(parent, names), children)
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

    /// The root node with its entries. Returning a node that already contains
    /// the root again (`(0, ..(0, ..)))`) is what made bars draw an empty
    /// menu: the tree must start at the requested parent and hold the entries
    /// directly.
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> (u32, MenuNode) {
        let node = self.node(parent_id, recursion_depth, &property_names);
        (self.revision.load(Ordering::Relaxed), node)
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, MenuProps)> {
        ids.into_iter()
            .map(|id| (id, self.props_of(id, &property_names)))
            .collect()
    }

    fn get_property(&self, id: i32, name: &str) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
        let names = [name.to_string()];
        match self.props_of(id, &names).into_iter().next() {
            Some((_, value)) => Ok(value),
            None => Err(zbus::fdo::Error::Failed(format!(
                "no property {name} on item {id}"
            ))),
        }
    }

    fn event(&self, id: i32, event_id: &str, _data: zbus::zvariant::Value<'_>, _timestamp: u32) {
        if event_id != "clicked" {
            return;
        }
        let lang = self.lang();
        let log = &self.state.logger;
        match id {
            MENU_OPEN => {
                // The window exists only in a GUI session. A daemon started
                // with --headless has no thread to open one, and a silent
                // request file would look like a dead button.
                if crate::config::window_marker().exists() {
                    log.info(t!(
                        lang,
                        "трей: окно открывается",
                        "tray: opening the window"
                    ));
                    crate::config::request("show");
                } else {
                    log.info(t!(
                        lang,
                        "трей: демон запущен без окна (--headless); запустите sdw-daemon без флага, чтобы вернуть окно",
                        "tray: the daemon runs without a window (--headless); start sdw-daemon without the flag to get it back"
                    ));
                }
            }
            MENU_WEB => {
                let url = format!("http://127.0.0.1:{}", self.state.cfg().api_port);
                log.info(format!(
                    "{} {url}",
                    t!(lang, "трей: веб-интерфейс", "tray: web interface")
                ));
                if sdw_core::util::has("xdg-open") {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&url)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
            MENU_QUIT => {
                log.info(t!(lang, "трей: выход", "tray: quit"));
                crate::config::request("quit");
            }
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
