//! What this desktop can do, and how the app adapts: overlay vs window,
//! global hotkeys vs a desktop shortcut calling `snowlearner say`.

use crate::config::settings::WindowMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Windows,
    MacOs,
    X11,
    Wayland,
    Unknown,
}

impl Session {
    pub fn detect() -> Session {
        if cfg!(windows) {
            return Session::Windows;
        }
        if cfg!(target_os = "macos") {
            return Session::MacOs;
        }
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if env("WAYLAND_DISPLAY").is_some() || env("XDG_SESSION_TYPE").as_deref() == Some("wayland") {
            Session::Wayland
        } else if env("DISPLAY").is_some() {
            Session::X11
        } else {
            Session::Unknown
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Session::Windows => "Windows",
            Session::MacOs => "macOS",
            Session::X11 => "Linux (X11)",
            Session::Wayland => "Linux (Wayland)",
            Session::Unknown => "unknown",
        }
    }

    /// Wayland does not let apps grab keys globally.
    pub fn global_hotkeys(self) -> bool {
        !matches!(self, Session::Wayland | Session::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub overlay: bool,
    /// Run through XWayland so always-on-top + click-through work on Wayland.
    pub force_x11: bool,
    pub note: Option<String>,
}

/// `xwayland`: an X server is reachable (DISPLAY is set) inside a Wayland session.
pub fn resolve(mode: WindowMode, session: Session, xwayland: bool) -> Resolved {
    let window = |note: Option<&str>| Resolved { overlay: false, force_x11: false, note: note.map(str::to_string) };
    match (mode, session) {
        (WindowMode::Window, _) => window(None),
        (WindowMode::Auto, Session::Wayland) => {
            window(Some("Wayland: running as a window. Use `--mode overlay` for the desktop overlay (via XWayland)."))
        }
        (WindowMode::Auto, Session::Unknown) => window(None),
        (WindowMode::Overlay, Session::Wayland) if xwayland => {
            Resolved { overlay: true, force_x11: true, note: Some("Wayland: overlay runs through XWayland.".into()) }
        }
        (WindowMode::Overlay, Session::Wayland) => {
            window(Some("Overlay needs XWayland (DISPLAY is not set); running as a window."))
        }
        (_, _) => Resolved { overlay: true, force_x11: false, note: None },
    }
}

/// How to get a hotkey on desktops where the app cannot grab one itself.
pub fn shortcut_help(exe: &str, hotkey: &str, summary_hotkey: &str) -> String {
    format!(
        "Wayland apps can't register global hotkeys. Bind desktop shortcuts instead:\n\
         \x20 GNOME: Settings → Keyboard → Custom Shortcuts → add\n\
         \x20   \"{exe} say\"      ({hotkey})\n\
         \x20   \"{exe} summary\"  ({summary_hotkey})\n\
         \x20 KDE: System Settings → Shortcuts → Custom Shortcuts, same commands."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_mode_is_an_overlay_where_supported() {
        for s in [Session::Windows, Session::MacOs, Session::X11] {
            assert!(resolve(WindowMode::Auto, s, false).overlay, "{s:?}");
        }
    }

    #[test]
    fn auto_mode_on_wayland_is_a_window_with_an_explanation() {
        let r = resolve(WindowMode::Auto, Session::Wayland, true);
        assert!(!r.overlay);
        assert!(r.note.unwrap().contains("--mode overlay"));
    }

    #[test]
    fn forced_overlay_on_wayland_goes_through_xwayland_or_degrades() {
        let r = resolve(WindowMode::Overlay, Session::Wayland, true);
        assert!(r.overlay && r.force_x11);
        let r = resolve(WindowMode::Overlay, Session::Wayland, false);
        assert!(!r.overlay && !r.force_x11);
        assert!(r.note.is_some());
    }

    #[test]
    fn window_mode_is_always_honored() {
        for s in [Session::Windows, Session::MacOs, Session::X11, Session::Wayland] {
            assert!(!resolve(WindowMode::Window, s, true).overlay);
        }
    }

    #[test]
    fn hotkeys_are_only_promised_where_they_work() {
        assert!(Session::X11.global_hotkeys());
        assert!(!Session::Wayland.global_hotkeys());
    }
}
