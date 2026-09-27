//! What puts a window's canvas on screen. The CPU (softbuffer) by default, so
//! the app runs on machines without a usable GPU and never loads a graphics
//! driver; the GPU only for see-through windows the CPU path can't show.

use super::canvas::Canvas;
use super::cpu::{self, Cpu};
#[cfg(feature = "gpu")]
use super::gpu::Gpu;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use winit::window::Window;

/// Forces a presenter: `cpu` or `gpu`.
pub const RENDERER_ENV: &str = "SNOWLEARNER_RENDERER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Cpu,
    Gpu,
}

impl Renderer {
    pub fn parse(s: &str) -> Option<Renderer> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cpu" => Some(Renderer::Cpu),
            "gpu" => Some(Renderer::Gpu),
            _ => None,
        }
    }
}

/// Presenters to try, in order. `cpu_alpha`: the CPU path can show this
/// window's transparency. `gpu_built`: this binary has the `gpu` feature.
pub fn plan(want_transparent: bool, cpu_alpha: bool, gpu_built: bool, forced: Option<Renderer>) -> Vec<Renderer> {
    let first = match forced {
        Some(r) => r,
        None if want_transparent && !cpu_alpha => Renderer::Gpu,
        None => Renderer::Cpu,
    };
    let second = if first == Renderer::Cpu { Renderer::Gpu } else { Renderer::Cpu };
    [first, second].into_iter().filter(|r| gpu_built || *r == Renderer::Cpu).collect()
}

pub enum Screen {
    Cpu(Cpu),
    #[cfg(feature = "gpu")]
    Gpu(Box<Gpu>),
}

impl Screen {
    pub fn new(window: Arc<Window>, want_transparent: bool) -> Result<Screen> {
        let forced = std::env::var(RENDERER_ENV).ok().and_then(|v| Renderer::parse(&v));
        let cpu_alpha = cpu::cpu_shows_alpha(&window);
        let mut errors = Vec::new();
        for r in plan(want_transparent, cpu_alpha, cfg!(feature = "gpu"), forced) {
            let made = match r {
                Renderer::Cpu => Cpu::new(window.clone(), want_transparent && cpu_alpha).map(Screen::Cpu),
                #[cfg(feature = "gpu")]
                Renderer::Gpu => Gpu::new(window.clone(), want_transparent).map(|g| Screen::Gpu(Box::new(g))),
                #[cfg(not(feature = "gpu"))]
                Renderer::Gpu => continue,
            };
            match made {
                Ok(screen) => return Ok(screen),
                Err(e) => errors.push(format!("{r:?}: {e:#}")),
            }
        }
        Err(anyhow!("no way to draw this window ({})", errors.join("; ")))
    }

    pub fn present(&mut self, canvas: &Canvas, scale: u32) {
        match self {
            // A dropped frame is redrawn 33 ms later; nothing to report.
            Screen::Cpu(c) => drop(c.present(canvas, scale)),
            #[cfg(feature = "gpu")]
            Screen::Gpu(g) => g.present(canvas, scale),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            Screen::Cpu(c) => c.resize(width, height),
            #[cfg(feature = "gpu")]
            Screen::Gpu(g) => g.resize(width, height),
        }
    }

    /// True when the compositor will show the desktop through clear pixels.
    pub fn transparent(&self) -> bool {
        match self {
            Screen::Cpu(c) => c.transparent,
            #[cfg(feature = "gpu")]
            Screen::Gpu(g) => g.transparent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Renderer::{Cpu as C, Gpu as G};

    #[test]
    fn opaque_windows_never_touch_the_gpu_unless_the_cpu_fails() {
        assert_eq!(plan(false, false, true, None), vec![C, G]);
        assert_eq!(plan(false, true, true, None), vec![C, G]);
    }

    #[test]
    fn see_through_windows_stay_on_the_cpu_where_it_shows_alpha() {
        // X11, including the Wayland overlay that runs through XWayland.
        assert_eq!(plan(true, true, true, None), vec![C, G]);
    }

    #[test]
    fn see_through_windows_elsewhere_try_the_gpu_then_degrade_to_cpu() {
        assert_eq!(plan(true, false, true, None), vec![G, C]);
    }

    #[test]
    fn builds_without_the_gpu_feature_only_ever_use_the_cpu() {
        for (t, a) in [(false, false), (true, false), (true, true)] {
            assert_eq!(plan(t, a, false, None), vec![C]);
        }
        assert_eq!(plan(true, false, false, Some(G)), vec![C], "forcing the GPU can't add one");
    }

    #[test]
    fn the_env_override_goes_first_and_keeps_the_other_as_fallback() {
        assert_eq!(plan(false, false, true, Some(G)), vec![G, C]);
        assert_eq!(plan(true, false, true, Some(C)), vec![C, G]);
    }

    #[test]
    fn renderer_names_parse_loosely_and_reject_the_rest() {
        assert_eq!(Renderer::parse(" CPU "), Some(C));
        assert_eq!(Renderer::parse("gpu"), Some(G));
        assert_eq!(Renderer::parse("vulkan"), None);
        assert_eq!(Renderer::parse(""), None);
    }
}
