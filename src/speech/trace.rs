//! One timestamped line per step of the voice pipeline (spoken, mic opened,
//! loudness, what was heard), for finding out why an answer wasn't heard.
//! Only when stderr is not a terminal: the Windows log file
//! (`snowlearner config path`), a journal, or `2> speech.log`.

use std::io::IsTerminal;

pub fn line(msg: std::fmt::Arguments) {
    if !std::io::stderr().is_terminal() {
        eprintln!("{} {msg}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}
