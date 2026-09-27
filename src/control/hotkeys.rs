//! System-wide hotkeys (Windows, macOS, X11). On Wayland apps cannot grab
//! global keys; bind a desktop shortcut to `snowlearner say` instead.

use anyhow::{Context, Result};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

pub use crate::control::ipc::Command;

/// Parses "Ctrl+Alt+M" style strings (case-insensitive).
pub fn parse(spec: &str) -> Result<HotKey> {
    spec.replace(' ', "").parse::<HotKey>().map_err(|e| anyhow::anyhow!("invalid hotkey {spec:?}: {e}"))
}

pub struct Hotkeys {
    _manager: GlobalHotKeyManager,
    bindings: Vec<(u32, Command)>,
}

impl Hotkeys {
    /// Must be called on the main thread (macOS requirement).
    pub fn register(challenge: &str, summary: &str) -> Result<Hotkeys> {
        let manager = GlobalHotKeyManager::new().context("global hotkeys unavailable on this desktop")?;
        let mut bindings = Vec::new();
        for (spec, cmd) in [(challenge, Command::Challenge), (summary, Command::Summary)] {
            let hk = parse(spec)?;
            manager.register(hk).with_context(|| format!("could not register {spec} (taken by another app?)"))?;
            bindings.push((hk.id(), cmd));
        }
        Ok(Hotkeys { _manager: manager, bindings })
    }

    /// Drains pending key presses.
    pub fn poll(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state == HotKeyState::Pressed {
                if let Some((_, cmd)) = self.bindings.iter().find(|(id, _)| *id == ev.id) {
                    out.push(*cmd);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_parse() {
        assert!(parse("Ctrl+Alt+M").is_ok());
        assert!(parse("ctrl + alt + j").is_ok());
        assert_ne!(parse("Ctrl+Alt+M").unwrap().id(), parse("Ctrl+Alt+J").unwrap().id());
    }

    #[test]
    fn garbage_hotkeys_are_rejected_with_the_spec_in_the_message() {
        let err = format!("{:#}", parse("Ctrl+Banana").unwrap_err());
        assert!(err.contains("Ctrl+Banana"));
    }
}
