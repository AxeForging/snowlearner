//! What the learner sees when the app cannot start and has no console to
//! print to: on Windows, started from the Start menu, the console was released
//! (`console::release_own_console`) and the error would reach only the log.

/// Shows `body` in a message box titled `title` and waits for OK.
#[cfg(windows)]
pub fn error(title: &str, body: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (title, body) = (wide(title), wide(body));
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call; no owner window.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
pub fn error(_title: &str, _body: &str) {}
