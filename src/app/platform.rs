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

/// A screen area in physical pixels (what the OS reports per monitor, so
/// monitors with different scale factors line up as they are).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// One monitor as the OS reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub rect: Rect,
    pub primary: bool,
}

/// Where the overlay windows go: the full scene on one monitor, the weather
/// on every other one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub primary: Rect,
    pub weather: Vec<Rect>,
}

/// The layout for `monitors` when it differs from `current`: a monitor was
/// plugged in, unplugged, or changed size or place while the app runs.
pub fn replan(current: &Overlay, monitors: &[Monitor]) -> Option<Overlay> {
    Some(overlay_layout(monitors)).filter(|new| new != current)
}

/// Used when the OS reports no monitor at all.
pub const FALLBACK_SCREEN: Rect = Rect { x: 0, y: 0, w: 1280, h: 720 };

/// One overlay window per monitor. The full scene goes on the monitor the OS
/// calls primary (the first one if none is), the weather on the others.
/// Zero-sized monitors and mirrors (same area as one already taken) get no
/// window of their own.
pub fn overlay_layout(monitors: &[Monitor]) -> Overlay {
    let usable: Vec<&Monitor> = monitors.iter().filter(|m| m.rect.w > 0 && m.rect.h > 0).collect();
    let Some(main) = usable.iter().find(|m| m.primary).or(usable.first()) else {
        return Overlay { primary: FALLBACK_SCREEN, weather: Vec::new() };
    };
    let mut taken = vec![main.rect];
    for m in &usable {
        if !taken.contains(&m.rect) {
            taken.push(m.rect);
        }
    }
    Overlay { primary: taken[0], weather: taken.split_off(1) }
}

/// How to get hotkeys on desktops where the app cannot grab them itself.
pub fn shortcut_help(exe: &str, bindings: &[(&str, crate::control::ipc::Command)]) -> String {
    let mut out = String::from(
        "Wayland apps can't register global hotkeys. Bind desktop shortcuts instead\n\
         (GNOME: Settings → Keyboard → Custom Shortcuts; KDE: Shortcuts → Custom Shortcuts):\n",
    );
    for (key, cmd) in bindings {
        let sub = match cmd {
            crate::control::ipc::Command::Challenge => "say",
            other => other.as_str(),
        };
        out.push_str(&format!("    {key:<12} → \"{exe} {sub}\"\n"));
    }
    if crate::control::gnome::available() {
        out.push_str("  …or just run: snowlearner shortcuts install\n");
    }
    out.trim_end().to_string()
}

/// A window's screen rect (physical pixels) in the overlay's scene pixels:
/// relative to the overlay's `origin`, divided by its `scale`, rounded outward
/// so nothing is left half covered.
pub fn to_scene(win: Rect, origin: (i32, i32), scale: u32) -> (i32, i32, i32, i32) {
    let s = scale.max(1) as i32;
    let (x, y) = (win.x - origin.0, win.y - origin.1);
    let (x0, y0) = (x.div_euclid(s), y.div_euclid(s));
    let (x1, y1) = ((x + win.w as i32 + s - 1).div_euclid(s), (y + win.h as i32 + s - 1).div_euclid(s));
    (x0, y0, x1 - x0, y1 - y0)
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
    fn shortcut_help_lists_one_command_per_binding() {
        use crate::control::ipc::Command;
        let help =
            shortcut_help("/bin/snowlearner", &[("Ctrl+Alt+M", Command::Challenge), ("Ctrl+Alt+K", Command::Menu)]);
        assert!(help.contains("Ctrl+Alt+M") && help.contains("\"/bin/snowlearner say\""));
        assert!(help.contains("\"/bin/snowlearner menu\""));
    }

    fn mon(x: i32, y: i32, w: u32, h: u32, primary: bool) -> Monitor {
        Monitor { rect: Rect { x, y, w, h }, primary }
    }

    #[test]
    fn no_monitor_reported_still_opens_one_full_scene_window() {
        let o = overlay_layout(&[]);
        assert_eq!(o.primary, FALLBACK_SCREEN);
        assert!(o.weather.is_empty());
    }

    #[test]
    fn one_monitor_gets_the_full_scene_and_no_weather_windows() {
        let o = overlay_layout(&[mon(0, 0, 1920, 1080, true)]);
        assert_eq!(o.primary, Rect { x: 0, y: 0, w: 1920, h: 1080 });
        assert!(o.weather.is_empty());
    }

    #[test]
    fn two_monitors_the_primary_runs_the_scene_the_other_gets_weather() {
        let o = overlay_layout(&[mon(0, 0, 1920, 1080, true), mon(1920, 0, 2560, 1440, false)]);
        assert_eq!(o.primary, Rect { x: 0, y: 0, w: 1920, h: 1080 });
        assert_eq!(o.weather, vec![Rect { x: 1920, y: 0, w: 2560, h: 1440 }]);
    }

    #[test]
    fn five_monitors_open_five_windows_one_scene_four_weather_in_os_order() {
        let ms = [
            mon(0, 0, 1920, 1080, false),
            mon(1920, 0, 1920, 1080, false),
            mon(3840, 0, 1920, 1080, true),
            mon(0, 1080, 1920, 1080, false),
            mon(1920, 1080, 1920, 1080, false),
        ];
        let o = overlay_layout(&ms);
        assert_eq!(o.primary, ms[2].rect);
        let expected: Vec<Rect> = [0, 1, 3, 4].iter().map(|&i| ms[i].rect).collect();
        assert_eq!(o.weather, expected);
    }

    #[test]
    fn the_primary_is_found_even_when_it_is_not_first_in_the_list() {
        let o = overlay_layout(&[mon(1920, 0, 1920, 1080, false), mon(0, 0, 1920, 1080, true)]);
        assert_eq!(o.primary.x, 0);
        assert_eq!(o.weather[0].x, 1920);
    }

    #[test]
    fn monitors_left_of_or_above_the_primary_keep_their_negative_positions() {
        let o = overlay_layout(&[
            mon(0, 0, 1920, 1080, true),
            mon(-2560, -360, 2560, 1440, false),
            mon(0, -1080, 1920, 1080, false),
        ]);
        assert_eq!(
            o.weather,
            vec![Rect { x: -2560, y: -360, w: 2560, h: 1440 }, Rect { x: 0, y: -1080, w: 1920, h: 1080 }]
        );
    }

    #[test]
    fn mixed_scale_monitors_are_placed_in_physical_pixels_untouched() {
        // A 4K laptop panel at 2x next to a 1080p monitor at 1x: rects pass
        // through as the OS reports them, no rescaling.
        let o = overlay_layout(&[mon(0, 0, 3840, 2160, true), mon(3840, 540, 1920, 1080, false)]);
        assert_eq!(o.primary, Rect { x: 0, y: 0, w: 3840, h: 2160 });
        assert_eq!(o.weather, vec![Rect { x: 3840, y: 540, w: 1920, h: 1080 }]);
    }

    #[test]
    fn with_no_primary_reported_the_first_monitor_runs_the_scene() {
        let o = overlay_layout(&[mon(1920, 0, 1920, 1080, false), mon(0, 0, 1920, 1080, false)]);
        assert_eq!(o.primary.x, 1920);
        assert_eq!(o.weather.len(), 1);
    }

    #[test]
    fn mirrored_and_zero_sized_monitors_get_no_extra_window() {
        let o = overlay_layout(&[
            mon(0, 0, 0, 0, true),
            mon(0, 0, 1920, 1080, false),
            mon(0, 0, 1920, 1080, false),
            mon(1920, 0, 1920, 1080, false),
            mon(1920, 0, 1920, 1080, false),
        ]);
        assert_eq!(o.primary, Rect { x: 0, y: 0, w: 1920, h: 1080 }, "a zero-sized primary is skipped");
        assert_eq!(o.weather, vec![Rect { x: 1920, y: 0, w: 1920, h: 1080 }]);
    }

    #[test]
    fn hotkeys_are_only_promised_where_they_work() {
        assert!(Session::X11.global_hotkeys());
        assert!(!Session::Wayland.global_hotkeys());
    }

    #[test]
    fn a_window_maps_into_scene_pixels_rounded_outward() {
        let panel = Rect { x: 1002, y: 401, w: 600, h: 301 };
        assert_eq!(to_scene(panel, (0, 0), 4), (250, 100, 151, 76));
        assert_eq!(to_scene(panel, (1000, 400), 1), (2, 1, 600, 301), "relative to the overlay's monitor");
        let left = Rect { x: -10, y: 0, w: 20, h: 8 };
        assert_eq!(to_scene(left, (0, 0), 4), (-3, 0, 6, 2), "partly left of the overlay");
        assert_eq!(to_scene(left, (0, 0), 0), (-10, 0, 20, 8), "a zero scale is treated as 1");
    }

    #[test]
    fn plugging_a_monitor_in_while_running_gives_it_snow_and_moves_nothing_else() {
        let laptop = mon(0, 0, 1920, 1200, true);
        let now = overlay_layout(&[laptop]);
        assert_eq!(replan(&now, &[laptop]), None, "nothing changed, nothing to redo");
        let new = replan(&now, &[laptop, mon(1920, 0, 2560, 1440, false)]).expect("a new monitor");
        assert_eq!(new.primary, now.primary);
        assert_eq!(new.weather, vec![Rect { x: 1920, y: 0, w: 2560, h: 1440 }]);
    }

    #[test]
    fn a_monitor_that_changes_size_or_place_gets_the_overlay_refitted() {
        let now = overlay_layout(&[mon(0, 0, 1920, 1200, true)]);
        let docked = replan(&now, &[mon(2560, 0, 2000, 1250, true), mon(0, 0, 2560, 1440, false)]).unwrap();
        assert_eq!(docked.primary, Rect { x: 2560, y: 0, w: 2000, h: 1250 }, "the scene follows its monitor");
        let unplugged = replan(&docked, &[mon(0, 0, 2000, 1250, true)]).unwrap();
        assert!(unplugged.weather.is_empty(), "the unplugged monitor's snow window goes away");
    }
}
