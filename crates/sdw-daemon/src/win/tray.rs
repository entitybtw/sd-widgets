//! The notification-area icon on Windows.
//!
//! A hidden window owns one `Shell_NotifyIconW` entry. A left click asks the
//! running daemon for its window through the same one-shot signal file the
//! second launch and the Linux tray already use, so the tray never needs to
//! reach into the GUI thread; a right click opens a small hard-edged menu with
//! the window, the local web page and quit. The message loop lives on its own
//! thread and ends on `PostThreadMessageW(WM_QUIT)`.

use crate::state::AppState;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ExtractIconW, ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NIN_SELECT, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, PostQuitMessage, PostThreadMessageW,
    RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON, MF_SEPARATOR,
    MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP, WNDCLASSW,
};

const WM_TRAY: u32 = WM_APP + 1;
/// The HTTP port, read by the menu thread when it opens the local page.
static PORT: AtomicU16 = AtomicU16::new(0);
const ID_OPEN: usize = 1;
const ID_WEB: usize = 2;
const ID_QUIT: usize = 3;

pub struct Tray {
    thread: u32,
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: posting a thread message to a thread this process started;
        // the loop leaves on WM_QUIT and removes the icon on its way out.
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Start the tray. `None` means the icon could not be created (no explorer, a
/// failed registration); the daemon keeps serving and simply has no tray.
pub fn start(state: Arc<AppState>) -> Option<Tray> {
    PORT.store(state.cfg().api_port, Ordering::Relaxed);
    let (tx, rx) = std::sync::mpsc::channel::<(bool, u32)>();
    std::thread::Builder::new()
        .name("sdw-tray".into())
        .spawn(move || {
            let thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            let ok = unsafe { run(thread) }.is_some();
            let _ = tx.send((ok, thread));
        })
        .ok()?;
    let (ok, thread) = rx.recv_timeout(std::time::Duration::from_secs(5)).ok()?;
    // The loop thread id is what Drop posts WM_QUIT to.
    ok.then_some(Tray { thread })
}

/// Create the icon and run the message loop until WM_QUIT. Returns the thread
/// id once the icon is in place, or `None` when creation failed.
unsafe fn run(thread: u32) -> Option<u32> {
    let hinst = GetModuleHandleW(None).ok()?;
    let instance = HINSTANCE(hinst.0);
    let class = WNDCLASSW {
        style: Default::default(),
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: HICON::default(),
        hCursor: Default::default(),
        hbrBackground: Default::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: w!("SdwDaemonTray"),
    };
    // SAFETY: the class is fully filled in and stays alive for the call.
    unsafe {
        RegisterClassW(&class);
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("SdwDaemonTray"),
            w!("sd-widgets"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .ok()?
    };

    let icon = unsafe { tray_icon() };
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    let tip = format!("sd-widgets · {}", sdw_core::util::hostname());
    for (slot, ch) in nid.szTip.iter_mut().zip(tip.encode_utf16()) {
        *slot = ch;
    }
    // SAFETY: `nid` is fully initialised and owns no pointers.
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return None;
    }

    let _ = thread;
    let mut msg = MSG::default();
    loop {
        // SAFETY: the loop owns the message; -1 means an error and ends it.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // SAFETY: the icon was added above; the handles are this thread's.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = DestroyWindow(hwnd);
        if !icon.is_invalid() {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
        }
    }
    Some(0)
}

/// The application icon from the running executable, the stock one otherwise.
unsafe fn tray_icon() -> HICON {
    let exe = std::env::current_exe().unwrap_or_default();
    let wide: Vec<u16> = exe
        .display()
        .to_string()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let from_exe = unsafe { ExtractIconW(None, PCWSTR(wide.as_ptr()), 0) };
    if !from_exe.is_invalid() {
        return from_exe;
    }
    // SAFETY: loading the stock application icon, always present.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::LoadIconW(
            None,
            windows::Win32::UI::WindowsAndMessaging::IDI_APPLICATION,
        )
        .unwrap_or_default()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lparam.0 as u32) & 0xffff;
            match event {
                WM_LBUTTONUP | NIN_SELECT => {
                    crate::config::request("show");
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    // SAFETY: a plain popup menu on this thread's window.
                    unsafe {
                        open_menu(hwnd);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match (wparam.0 & 0xffff) as usize {
                ID_OPEN => crate::config::request("show"),
                ID_WEB => {
                    let url = format!("http://127.0.0.1:{}", daemon_port());
                    let url: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
                    // SAFETY: opening a URL with the shell, no pointers outlive
                    // the call.
                    unsafe {
                        ShellExecuteW(
                            None,
                            w!("open"),
                            PCWSTR(url.as_ptr()),
                            None,
                            None,
                            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                        );
                    }
                }
                ID_QUIT => crate::config::request("quit"),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends the loop that owns this window.
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn open_menu(hwnd: HWND) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, ID_OPEN, w!("открыть окно")) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, ID_WEB, w!("веб-интерфейс")) };
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, ID_QUIT, w!("выйти")) };
    let _ = MF_SEPARATOR;
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    let _ = unsafe { SetForegroundWindow(hwnd) };
    let _ = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            point.x,
            point.y,
            Some(0),
            hwnd,
            None,
        )
    };
    let _ = unsafe { DestroyMenu(menu) };
}

/// The API port as the daemon configured it. The tray is started after the
/// config is read, so this is the same number the HTTP server binds.
fn daemon_port() -> u16 {
    crate::config::load().api_port
}
