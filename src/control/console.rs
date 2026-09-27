//! Windows: started from the Start menu or a shortcut, the app would bring a
//! black console window along. Release it when it was opened just for us;
//! keep it when the user ran `snowlearner` from their own terminal.

#[cfg(windows)]
pub fn release_own_console() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList};
    let mut ids = [0u32; 2];
    // SAFETY: the buffer is valid for `ids.len()` entries; FreeConsole has no preconditions.
    unsafe {
        if GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) == 1 {
            FreeConsole();
        }
    }
}

#[cfg(not(windows))]
pub fn release_own_console() {}
