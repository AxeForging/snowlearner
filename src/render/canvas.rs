//! CPU pixel canvas: every sprite, particle and panel is drawn here at the
//! virtual (low) resolution, then uploaded and upscaled with nearest filtering.

pub type Rgba = [u8; 4];

pub const CLEAR: Rgba = [0, 0, 0, 0];

/// `0xRRGGBB` → opaque color.
pub const fn hex(v: u32) -> Rgba {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
}

const BAYER: [f32; 16] = [0.5, 8.5, 2.5, 10.5, 12.5, 4.5, 14.5, 6.5, 3.5, 11.5, 1.5, 9.5, 15.5, 7.5, 13.5, 5.5];

/// Ordered-dither threshold in (0,1) for a pixel; the backbone of every
/// "transparent" effect, since the art only uses fully opaque pixels.
pub fn bayer(x: i32, y: i32) -> f32 {
    BAYER[(((y & 3) * 4) + (x & 3)) as usize] / 16.0
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        let (w, h) = (w.max(1), h.max(1));
        Self { w, h, px: vec![0; (w * h * 4) as usize] }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.px
    }

    pub fn clear(&mut self, c: Rgba) {
        for p in self.px.chunks_exact_mut(4) {
            p.copy_from_slice(&c);
        }
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let i = ((y * self.w + x) * 4) as usize;
            self.px[i..i + 4].copy_from_slice(&c);
        }
    }

    /// Float-positioned pixel, rounded like the rest of the art.
    #[inline]
    pub fn dot(&mut self, x: f32, y: f32, c: Rgba) {
        self.set(x.round() as i32, y.round() as i32, c);
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let i = ((y * self.w + x) * 4) as usize;
            Some([self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]])
        } else {
            None
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.set(xx, yy, c);
            }
        }
    }

    /// Draws a sprite of row strings; `.` is transparent, other chars are looked
    /// up in `pal`. `flip` mirrors horizontally (sprites face right by default).
    pub fn sprite(&mut self, rows: &[&str], pal: &[(char, Rgba)], x: i32, y: i32, flip: bool) {
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
        for (j, row) in rows.iter().enumerate() {
            for (i, ch) in row.chars().enumerate() {
                if ch == '.' {
                    continue;
                }
                if let Some((_, c)) = pal.iter().find(|(k, _)| *k == ch) {
                    let dx = if flip { width - 1 - i as i32 } else { i as i32 };
                    self.set(x + dx, y + j as i32, *c);
                }
            }
        }
    }

    /// Dithered radial glow: denser near the center, fading with distance.
    pub fn glow(&mut self, cx: f32, cy: f32, radius: f32, strength: f32, c: Rgba) {
        if radius <= 0.0 || strength <= 0.0 {
            return;
        }
        let (x0, x1) = ((cx - radius).floor() as i32, (cx + radius).ceil() as i32);
        let (y0, y1) = ((cy - radius).floor() as i32, (cy + radius).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                if d < radius && (1.0 - d / radius) * strength > bayer(x, y) {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// Copies `src` onto self at (x,y), skipping transparent pixels.
    pub fn blit(&mut self, src: &Canvas, x: i32, y: i32) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                let i = ((sy * src.w + sx) * 4) as usize;
                if src.px[i + 3] != 0 {
                    let c = [src.px[i], src.px[i + 1], src.px[i + 2], src.px[i + 3]];
                    self.set(x + sx, y + sy, c);
                }
            }
        }
    }

    /// Number of non-transparent pixels in a region (used by tests and the
    /// scene to reason about what is on screen).
    pub fn opaque_in(&self, x: i32, y: i32, w: i32, h: i32) -> usize {
        let mut n = 0;
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                if self.get(xx, yy).is_some_and(|c| c[3] != 0) {
                    n += 1;
                }
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_ignores_out_of_bounds_writes() {
        let mut c = Canvas::new(4, 4);
        c.set(-1, 0, hex(0xffffff));
        c.set(4, 4, hex(0xffffff));
        assert_eq!(c.opaque_in(0, 0, 4, 4), 0);
    }

    #[test]
    fn sprite_flip_mirrors_columns() {
        let mut c = Canvas::new(3, 1);
        c.sprite(&["A.."], &[('A', hex(0xff0000))], 0, 0, true);
        assert_eq!(c.get(2, 0), Some(hex(0xff0000)));
        assert_eq!(c.get(0, 0), Some(CLEAR));
    }

    #[test]
    fn rect_is_clipped_to_canvas() {
        let mut c = Canvas::new(5, 5);
        c.rect(3, 3, 10, 10, hex(0x00ff00));
        assert_eq!(c.opaque_in(0, 0, 5, 5), 4);
    }

    #[test]
    fn glow_is_denser_near_center() {
        let mut c = Canvas::new(40, 40);
        c.glow(20.0, 20.0, 16.0, 1.0, hex(0xffffff));
        let inner = c.opaque_in(16, 16, 8, 8);
        let outer = c.opaque_in(4, 18, 8, 4) * 2; // same area (64 px)
        assert!(inner > outer, "inner {inner} should exceed outer {outer}");
    }

    #[test]
    fn blit_skips_transparent_pixels() {
        let mut dst = Canvas::new(2, 1);
        dst.clear(hex(0x0000ff));
        let mut src = Canvas::new(2, 1);
        src.set(1, 0, hex(0xff0000));
        dst.blit(&src, 0, 0);
        assert_eq!(dst.get(0, 0), Some(hex(0x0000ff)));
        assert_eq!(dst.get(1, 0), Some(hex(0xff0000)));
    }
}
