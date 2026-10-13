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

/// The hotkeys that could not be registered: what each would do, and why not.
pub type Taken = Vec<(Command, String)>;

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    bindings: Vec<(HotKey, Command)>,
    pub taken: Taken,
}

impl Hotkeys {
    /// Must be called on the main thread (macOS requirement).
    pub fn register(specs: &[(&str, Command)]) -> Result<Hotkeys> {
        let manager = GlobalHotKeyManager::new().context("global hotkeys unavailable on this desktop")?;
        let (bindings, taken) = register_each(specs, |hk| Ok(manager.register(hk)?));
        Ok(Hotkeys { manager, bindings, taken })
    }

    /// Releases the hotkeys held and registers `specs` instead (the panel
    /// changed one). `taken` then says which of the new set are missing.
    pub fn rebind(&mut self, specs: &[(&str, Command)]) {
        for (hk, _) in self.bindings.drain(..) {
            if let Err(e) = self.manager.unregister(hk) {
                eprintln!("could not release a hotkey: {e}");
            }
        }
        (self.bindings, self.taken) = register_each(specs, |hk| Ok(self.manager.register(hk)?));
    }

    /// Drains pending key presses.
    pub fn poll(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state == HotKeyState::Pressed {
                if let Some((_, cmd)) = self.bindings.iter().find(|(hk, _)| hk.id() == ev.id) {
                    out.push(*cmd);
                }
            }
        }
        out
    }
}

/// Registers each hotkey on its own: one taken by another app (or mistyped)
/// loses only itself, not the menu key the click-through overlay depends on.
fn register_each(
    specs: &[(&str, Command)],
    mut register: impl FnMut(HotKey) -> Result<()>,
) -> (Vec<(HotKey, Command)>, Taken) {
    let mut bindings = Vec::new();
    let mut taken = Vec::new();
    for &(spec, cmd) in specs {
        match parse(spec).and_then(|hk| {
            register(hk).with_context(|| format!("could not register {spec} (taken by another app?)"))?;
            Ok(hk)
        }) {
            Ok(hk) => bindings.push((hk, cmd)),
            Err(e) => taken.push((cmd, format!("{e:#}"))),
        }
    }
    (bindings, taken)
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

    #[test]
    fn a_hotkey_taken_by_another_app_only_loses_itself() {
        let specs = [
            ("Ctrl+Alt+M", Command::Challenge),
            ("Ctrl+Alt+G", Command::Grab),
            ("Ctrl+Banana", Command::Summary),
            ("Ctrl+Alt+K", Command::Menu),
        ];
        let g = parse("Ctrl+Alt+G").unwrap().id();
        let (bound, taken) = register_each(&specs, |hk| {
            if hk.id() == g { Err(anyhow::anyhow!("HotKey already registered")) } else { Ok(()) }
        });
        let cmds: Vec<Command> = bound.iter().map(|(_, c)| *c).collect();
        assert_eq!(cmds, vec![Command::Challenge, Command::Menu]);
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].0, Command::Grab, "the action that lost its key is named");
        assert!(taken[0].1.contains("Ctrl+Alt+G") && taken[0].1.contains("already registered"), "{taken:?}");
        assert!(taken[1].1.contains("Ctrl+Banana"), "{taken:?}");
    }

    #[test]
    fn every_letter_and_digit_makes_a_hotkey() {
        for k in ('A'..='Z').chain('0'..='9') {
            assert!(parse(&format!("Ctrl+Alt+{k}")).is_ok(), "{k}");
        }
    }
}
