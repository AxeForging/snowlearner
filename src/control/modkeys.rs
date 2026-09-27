//! "Is Ctrl+Alt held right now?" — read globally, without focus, where the OS
//! allows it (Windows, macOS, X11). Wayland never exposes this to apps, so
//! there the shortcut cheat sheet shows on orb hover instead.

/// None = this desktop can't tell us.
pub fn ctrl_alt_held() -> Option<bool> {
    imp::ctrl_alt_held()
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU};

    pub fn ctrl_alt_held() -> Option<bool> {
        // High bit set = key is down.
        let down = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
        Some(down(VK_CONTROL) && down(VK_MENU))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }
    const COMBINED_SESSION: i32 = 0;
    const CONTROL: u64 = 0x40000;
    const ALTERNATE: u64 = 0x80000;

    pub fn ctrl_alt_held() -> Option<bool> {
        let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION) };
        Some(flags & CONTROL != 0 && flags & ALTERNATE != 0)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::sync::OnceLock;
    use x11_dl::xlib;

    struct X {
        lib: xlib::Xlib,
        display: *mut xlib::Display,
        codes: [[u8; 2]; 2],
    }
    // The display is only touched from the UI thread that owns the app.
    unsafe impl Send for X {}
    unsafe impl Sync for X {}

    fn x() -> Option<&'static X> {
        static X11: OnceLock<Option<X>> = OnceLock::new();
        X11.get_or_init(|| {
            // Under Wayland, XWayland only reports keys for X windows in focus: useless.
            if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_none() {
                return None;
            }
            let lib = xlib::Xlib::open().ok()?;
            let display = unsafe { (lib.XOpenDisplay)(std::ptr::null()) };
            if display.is_null() {
                return None;
            }
            let code = |sym: u32| unsafe { (lib.XKeysymToKeycode)(display, sym as u64) };
            let codes = [
                [code(x11_dl::keysym::XK_Control_L), code(x11_dl::keysym::XK_Control_R)],
                [code(x11_dl::keysym::XK_Alt_L), code(x11_dl::keysym::XK_Alt_R)],
            ];
            Some(X { lib, display, codes })
        })
        .as_ref()
    }

    pub fn ctrl_alt_held() -> Option<bool> {
        let x = x()?;
        let mut keys = [0 as std::os::raw::c_char; 32];
        unsafe { (x.lib.XQueryKeymap)(x.display, keys.as_mut_ptr()) };
        let down = |code: u8| code != 0 && (keys[(code / 8) as usize] as u8) & (1 << (code % 8)) != 0;
        Some(x.codes[0].iter().any(|c| down(*c)) && x.codes[1].iter().any(|c| down(*c)))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn answers_without_panicking_on_any_desktop() {
        // No keys are held during tests; where supported it must say "not held".
        assert_ne!(super::ctrl_alt_held(), Some(true));
    }
}
