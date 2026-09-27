//! Windows: started from the Start menu or a shortcut, the app would bring a
//! black console window along. Release it when it was opened just for us;
//! keep it when the user ran `snowlearner` from their own terminal.

use std::path::Path;

/// Frees a console opened just for us and sends stdout/stderr to `log`
/// (the previous run's is kept as `.log.old`).
///
/// `FreeConsole` leaves the std handles holding the closed console's handle
/// values. Windows hands those values to the next files opened — the history
/// database — and every `eprintln!` then wrote into it and corrupted it.
#[cfg(windows)]
pub fn release_own_console(log: &Path) {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{
        FreeConsole, GetConsoleProcessList, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    let mut ids = [0u32; 2];
    // SAFETY: the buffer is valid for `ids.len()` entries; FreeConsole has no preconditions.
    if unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) } != 1 || unsafe { FreeConsole() } == 0 {
        return;
    }
    // A null handle makes std drop the writes; the log file, if it opens, gets them.
    let out = open_log(log).map(|f| f.into_raw_handle()).unwrap_or(std::ptr::null_mut());
    // SAFETY: SetStdHandle only stores the value; the log handle is leaked on purpose
    // and stays open for the life of the process.
    unsafe {
        SetStdHandle(STD_INPUT_HANDLE, std::ptr::null_mut());
        SetStdHandle(STD_OUTPUT_HANDLE, out);
        SetStdHandle(STD_ERROR_HANDLE, out);
    }
}

#[cfg(not(windows))]
pub fn release_own_console(_log: &Path) {}

#[cfg_attr(not(windows), allow(dead_code))]
fn open_log(log: &Path) -> std::io::Result<std::fs::File> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::rename(log, log.with_extension("log.old"));
    std::fs::File::create(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_log_keeps_the_previous_run_as_old() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("data").join("snowlearner.log");
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "run 1").unwrap();
        open_log(&log).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "", "fresh log per run");
        assert_eq!(std::fs::read_to_string(dir.path().join("data/snowlearner.log.old")).unwrap(), "run 1");
    }

    #[test]
    fn the_log_folder_is_created_on_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("fresh").join("snowlearner.log");
        open_log(&log).unwrap();
        assert!(log.exists());
    }
}
