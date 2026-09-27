//! Presents the CPU canvas with no GPU at all: softbuffer blits a shared-memory
//! image to the window, and the nearest-neighbour upscale happens here. Shows
//! per-pixel alpha where the window system takes it straight (X11 32-bit visuals).

use super::canvas::Canvas;
use super::damage::{self, Damage};
use anyhow::{Context as _, Result, anyhow};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::window::Window;

pub struct Cpu {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    size: (u32, u32),
    damage: Damage,
    /// True when the compositor will show the desktop through clear pixels.
    pub transparent: bool,
}

impl Cpu {
    /// `transparent`: the caller checked that this window system takes our alpha
    /// (see `cpu_shows_alpha`); otherwise clear pixels show as black.
    pub fn new(window: Arc<Window>, transparent: bool) -> Result<Cpu> {
        let size = window.inner_size();
        let context = softbuffer::Context::new(window.clone()).map_err(|e| anyhow!("{e}"))?;
        let surface = softbuffer::Surface::new(&context, window).map_err(|e| anyhow!("{e}"))?;
        let mut cpu = Cpu { surface, size: (0, 0), damage: Damage::default(), transparent };
        cpu.resize(size.width, size.height);
        Ok(cpu)
    }

    pub fn invalidate(&mut self) {
        self.damage.invalidate();
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) == self.size {
            return;
        }
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else { return };
        if self.surface.resize(nw, nh).is_ok() {
            self.size = (w, h);
        }
    }

    /// Draws `canvas` scaled by `scale` from the top-left corner, repainting
    /// and sending only what changed since the buffer was last shown.
    pub fn present(&mut self, canvas: &Canvas, scale: u32) -> Result<()> {
        let (w, h) = self.size;
        let mut buffer = self.surface.buffer_mut().map_err(|e| anyhow!("{e}")).context("softbuffer frame")?;
        let age = buffer.age();
        let presented = match damage::update(&mut self.damage, canvas, scale, &mut buffer, w as usize, h as usize, age)
        {
            None => buffer.present(),
            // Presented even when empty: the damage history counts every frame.
            Some(rects) => buffer.present_with_damage(
                &rects
                    .into_iter()
                    .filter_map(|(x, y, rw, rh)| {
                        Some(softbuffer::Rect { x, y, width: NonZeroU32::new(rw)?, height: NonZeroU32::new(rh)? })
                    })
                    .collect::<Vec<_>>(),
            ),
        };
        presented.map_err(|e| anyhow!("{e}"))
    }
}

/// Whether a softbuffer window can show per-pixel transparency: only X11
/// (32-bit visual, premultiplied ARGB). Wayland, macOS and Windows get XRGB.
pub fn cpu_shows_alpha(window: &Window) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    matches!(window.window_handle().map(|h| h.as_raw()), Ok(RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)))
}

/// Premultiplied `0xAARRGGBB`, the layout softbuffer (and X11 ARGB visuals) use.
#[inline]
pub(super) fn pack(p: &[u8]) -> u32 {
    let a = p[3] as u32;
    let pm = |c: u8| c as u32 * a / 255;
    a << 24 | pm(p[0]) << 16 | pm(p[1]) << 8 | pm(p[2])
}

/// Nearest-neighbour upscale of `canvas` into a `w`×`h` frame, anchored top-left.
/// Each canvas row is scaled once and then copied `scale` times; what the canvas
/// does not cover is cleared.
pub fn upscale(canvas: &Canvas, scale: u32, dst: &mut [u32], w: usize, h: usize) {
    if w == 0 || h == 0 {
        return;
    }
    let scale = scale.max(1) as usize;
    let (cw, ch) = (canvas.w as usize, canvas.h as usize);
    let len = (w * h).min(dst.len());
    let dst = &mut dst[..len];
    let covered_w = (cw * scale).min(w);
    let src = canvas.bytes();
    let mut row = vec![0u32; w];
    for (y, out) in dst.chunks_exact_mut(w).enumerate() {
        let sy = y / scale;
        if sy >= ch {
            out.fill(0);
            continue;
        }
        if y % scale == 0 {
            let line = &src[sy * cw * 4..(sy + 1) * cw * 4];
            for (x, px) in row[..covered_w].iter_mut().enumerate() {
                let sx = x / scale;
                *px = pack(&line[sx * 4..sx * 4 + 4]);
            }
        }
        out.copy_from_slice(&row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::canvas::{CLEAR, hex};

    const RED: u32 = 0xffff0000;
    const BLUE: u32 = 0xff0000ff;

    /// 2×2: red, blue / clear, white.
    fn checker() -> Canvas {
        let mut c = Canvas::new(2, 2);
        c.set(0, 0, hex(0xff0000));
        c.set(1, 0, hex(0x0000ff));
        c.set(0, 1, CLEAR);
        c.set(1, 1, hex(0xffffff));
        c
    }

    fn frame(c: &Canvas, scale: u32, w: usize, h: usize) -> Vec<u32> {
        let mut out = vec![0xdeadbeef; w * h];
        upscale(c, scale, &mut out, w, h);
        out
    }

    #[test]
    fn scale_one_copies_pixels_one_to_one() {
        assert_eq!(frame(&checker(), 1, 2, 2), vec![RED, BLUE, 0, 0xffffffff]);
    }

    #[test]
    fn each_canvas_pixel_becomes_a_scale_by_scale_block() {
        let f = frame(&checker(), 3, 6, 6);
        for y in 0..6 {
            for x in 0..6 {
                let want = match (x / 3, y / 3) {
                    (0, 0) => RED,
                    (1, 0) => BLUE,
                    (0, 1) => 0,
                    _ => 0xffffffff,
                };
                assert_eq!(f[y * 6 + x], want, "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn a_canvas_overhanging_the_window_is_cropped() {
        // The app rounds the canvas up (div_ceil), so it can be bigger than window/scale.
        assert_eq!(frame(&checker(), 2, 3, 3), vec![RED, RED, BLUE, RED, RED, BLUE, 0, 0, 0xffffffff]);
    }

    #[test]
    fn area_the_canvas_does_not_cover_is_cleared_not_left_stale() {
        let f = frame(&checker(), 1, 4, 3);
        assert_eq!(&f[0..4], &[RED, BLUE, 0, 0]);
        assert_eq!(&f[8..12], &[0, 0, 0, 0], "row below the canvas");
    }

    #[test]
    fn clear_pixels_are_fully_transparent_and_color_is_premultiplied() {
        assert_eq!(pack(&[255, 128, 7, 0]), 0, "no color leaks through a clear pixel");
        assert_eq!(pack(&[200, 100, 50, 255]), 0xffc86432);
        assert_eq!(pack(&[255, 255, 255, 128]), 0x80808080);
    }

    #[test]
    fn scale_zero_is_treated_as_one_and_short_buffers_do_not_panic() {
        assert_eq!(frame(&checker(), 0, 2, 2), vec![RED, BLUE, 0, 0xffffffff]);
        let mut short = vec![7u32; 3];
        upscale(&checker(), 1, &mut short, 2, 2);
        assert_eq!(short, vec![RED, BLUE, 7], "partial trailing row left untouched");
        upscale(&checker(), 1, &mut [], 0, 0);
    }
}
