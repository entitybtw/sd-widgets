//! Mirror Windows toast notifications into the shared store.
//!
//! `UserNotificationListener` is the documented read API. It asks the user once
//! for permission; when permission is not granted (or the build has no package
//! identity, which some Windows versions require) the store says so instead of
//! retrying. The listener polls its own list rather than subscribing to
//! `NotificationChanged`, because one poll per second and a half is cheaper
//! than a callback that has to marshal into a `Send` thread.
//!
//! Windows keeps its own id for a toast; the deck speaks the store's id. Both
//! are kept side by side so "dismiss" on the deck also removes the toast from
//! the Windows notification centre.

use crate::notifications::NotificationStore;
use crate::win::{with_com, WaitAsync};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};

static LISTENER: OnceLock<Mutex<Option<UserNotificationListener>>> = OnceLock::new();
static SYSTEM_IDS: OnceLock<Mutex<HashMap<u64, u32>>> = OnceLock::new();

fn ids() -> &'static Mutex<HashMap<u64, u32>> {
    SYSTEM_IDS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start polling. Returns at once; the work runs on its own thread.
pub fn start(store: Arc<NotificationStore>, stop: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new()
        .name("sdw-notify".into())
        .spawn(move || {
            let _ = with_com(|| {
                run(&store, &stop);
                Ok(())
            });
        });
}

fn run(store: &Arc<NotificationStore>, stop: &Arc<AtomicBool>) {
    let listener = match UserNotificationListener::Current() {
        Ok(listener) => listener,
        Err(e) => {
            store.set_listening(
                false,
                Some(format!("Windows: слушатель уведомлений недоступен: {e}")),
            );
            return;
        }
    };
    let access = match listener.RequestAccessAsync().and_then(|op| op.wait()) {
        Ok(status) => status,
        Err(e) => {
            store.set_listening(
                false,
                Some(format!("Windows: нет доступа к уведомлениям: {e}")),
            );
            return;
        }
    };
    if access != UserNotificationListenerAccessStatus::Allowed {
        store.set_listening(
            false,
            Some(
                "Windows: доступ к уведомлениям не выдан — разрешите его в \
                 Параметры → Конфиденциальность → Уведомления"
                    .into(),
            ),
        );
        return;
    }
    if let Ok(mut slot) = LISTENER.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(listener.clone());
    }
    store.set_listening(true, None);

    while !stop.load(Ordering::Relaxed) {
        if store.enabled() {
            poll(store, &listener);
        }
        // A toast appears when Windows decides to show it, not on a frame
        // boundary; 1.5 s is fast enough for a deck on the same desk.
        for _ in 0..15 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

fn poll(store: &NotificationStore, listener: &UserNotificationListener) {
    let Ok(op) = listener.GetNotificationsAsync(NotificationKinds::Toast) else {
        return;
    };
    let Ok(list) = op.wait() else {
        return;
    };
    let Ok(count) = list.Size() else {
        return;
    };
    // The system list is newest first; the store appends, so walk it backwards
    // and let the ring buffer end up oldest to newest like every other source.
    for index in (0..count).rev() {
        let Ok(item) = list.GetAt(index) else {
            continue;
        };
        let Ok(system_id) = item.Id() else {
            continue;
        };
        if ids()
            .lock()
            .map(|map| map.values().any(|v| *v == system_id))
            .unwrap_or(true)
        {
            continue;
        }
        let app = item
            .AppInfo()
            .and_then(|info| info.DisplayInfo())
            .and_then(|display| display.DisplayName())
            .map(|name| name.to_string())
            .unwrap_or_default();
        let (summary, body) = texts(&item);
        if summary.is_empty() && body.is_empty() {
            continue;
        }
        let id = store.record(&app, &summary, &body, "", 0, false, 0, &[], &[], None);
        if id != 0 {
            if let Ok(mut map) = ids().lock() {
                map.insert(id, system_id);
            }
        }
    }
}

/// Title and body of a toast. Windows puts the title in the first text element
/// and the lines under it in the rest; a toast without a binding still has an
/// app name, which the caller has already taken.
fn texts(item: &UserNotification) -> (String, String) {
    let mut lines: Vec<String> = Vec::new();
    if let Ok(visual) = item.Notification().and_then(|n| n.Visual()) {
        if let Ok(template) = KnownNotificationBindings::ToastGeneric() {
            if let Ok(binding) = visual.GetBinding(&template) {
                if let Ok(elements) = binding.GetTextElements() {
                    if let Ok(count) = elements.Size() {
                        for index in 0..count {
                            if let Ok(element) = elements.GetAt(index) {
                                if let Ok(text) = element.Text() {
                                    let text = text.to_string();
                                    if !text.trim().is_empty() {
                                        lines.push(text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let summary = lines.first().cloned().unwrap_or_default();
    let body = lines.into_iter().skip(1).collect::<Vec<_>>().join("\n");
    (summary, body)
}

/// Remove a toast from the Windows notification centre when it is dismissed on
/// the deck. A missing mapping (a toast from before this run) is not an error.
pub fn dismiss(store_id: u64) {
    let system_id = ids()
        .lock()
        .ok()
        .and_then(|map| map.get(&store_id).copied());
    let Some(system_id) = system_id else {
        return;
    };
    let Some(listener) = LISTENER
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
    else {
        return;
    };
    let _ = listener.RemoveNotification(system_id);
}
