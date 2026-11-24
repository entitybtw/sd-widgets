//! Windows implementations of the pieces that talk to the operating system.
//!
//! Each submodule replaces the Linux one through `#[cfg(windows)]` in
//! `main.rs`, so the rest of the daemon (the HTTP API, the window, the model)
//! is shared and only the platform layer differs.

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

pub mod display;
pub mod notify;
pub mod tray;

/// WinRT asynchronous calls, waited on from a normal blocking thread.
///
/// `windows-future` keeps its blocking `join` private and only publishes
/// `Status`/`GetResults`, so this polls the status the way the C++ samples do
/// and hands back the value or the failure.
pub(crate) trait WaitAsync {
    type Output;

    fn wait(&self) -> windows::core::Result<Self::Output> {
        self.wait_for(std::time::Duration::from_secs(15))
    }

    fn wait_for(&self, timeout: std::time::Duration) -> windows::core::Result<Self::Output>;
}

impl<T: windows::core::RuntimeType + 'static> WaitAsync for windows_future::IAsyncOperation<T> {
    type Output = T;

    fn wait_for(&self, timeout: std::time::Duration) -> windows::core::Result<T> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.Status() {
                Ok(windows_future::AsyncStatus::Completed) => return self.GetResults(),
                // The failure itself lives in the result; asking for it gives
                // the original HRESULT and its message.
                Ok(windows_future::AsyncStatus::Error) => return self.GetResults(),
                Ok(windows_future::AsyncStatus::Canceled) => {
                    return Err(windows::core::Error::new(
                        windows::Win32::Foundation::E_ABORT,
                        "операция отменена",
                    ));
                }
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            if std::time::Instant::now() >= deadline {
                return Err(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    "операция не завершилась вовремя",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Runs `f` on a COM-initialized thread. WASAPI and WinRT both need it on every
/// thread that touches them, and the daemon's HTTP threads never initialize COM
/// themselves.
pub(crate) fn with_com<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    // SAFETY: a plain COM initialization of the calling thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // RPC_E_CHANGED_MODE means the thread already lives in another apartment;
    // the APIs still work from there, and uninitializing would be wrong.
    let owned = hr.is_ok();
    let result = f();
    if owned {
        // SAFETY: matches the successful `CoInitializeEx` above.
        unsafe { CoUninitialize() };
    }
    result
}

/// Read a COM string that may be NULL and is NUL-terminated.
pub(crate) fn wide_string(text: windows::core::PWSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: the bound stops a broken provider from walking off the
    // allocation; COM strings are NUL-terminated.
    let mut len = 0usize;
    unsafe {
        while len < 4096 && *text.0.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text.0, len))
    }
}
