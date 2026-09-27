//! Night winter landscape for window mode (overlay mode shows your desktop instead).
//! Rendered once per size into a cached canvas; stars twinkle on top each frame.

use super::rng::{Rng, hash01};
use crate::render::canvas::{Canvas, bayer, hex};

/// Height of the snowy ground band below the walkable line.
pub const GROUND_BAND: i32 = 10;

pub struct Backdrop {
    pub sky: Canvas,
    pub land: Canvas,
    stars: Vec<(i32, i32, f32, f32, bool)>,
}

impl Backdrop {
    pub fn new(w: i32, h: i32) -> Self {
        let ground = h - GROUND_BAND;
        let mut sky = Canvas::new(w, h);
        let pal = [hex(0x07061a), hex(0x0e0c2c), hex(0x17143f), hex(0x221d55), hex(0x2e2869), hex(0x3b347c)];
        for y in 0..h {
            let t = (y as f32 / (ground as f32 * 0.85)).min(1.0) * (pal.len() - 1) as f32;
            let (i, f) = (t.floor() as usize, t.fract());
            for x in 0..w {
                sky.set(x, y, pal[(i + usize::from(f > bayer(x, y))).min(pal.len() - 1)]);
            }
        }
        let (mx, my, r) = (w as f32 * 0.8, h as f32 * 0.19, (h as f32 * 0.085).max(5.0));
        for y in (my - r * 2.0) as i32..=(my + r * 2.0) as i32 {
            for x in (mx - r * 2.0) as i32..=(mx + r * 2.0) as i32 {
                let (dx, dy) = (x as f32 - mx, y as f32 - my);
                let d = dx.hypot(dy);
                if d <= r {
                    let crater = hash01(x / 2, y / 2) > 0.8 && d < r - 1.5;
                    sky.set(x, y, if dx + dy > r * 0.5 || crater { hex(0xd8d1ad) } else { hex(0xf3eed2) });
                } else if d < r * 2.0 && (1.0 - (d - r) / r) * 0.45 > bayer(x, y) {
                    sky.set(x, y, hex(0x3f3886));
                }
            }
        }

        let mut land = Canvas::new(w, h);
        let hs = h as f32 / 90.0;
        for x in 0..w {
            let xf = x as f32 * 160.0 / w.max(1) as f32;
            let far = (ground as f32
                - 32.0 * hs
                - ((xf * 0.045 + 0.5).sin() * 7.0 + (xf * 0.11 + 2.0).sin() * 4.0) * hs) as i32;
            for y in far..ground {
                land.set(
                    x,
                    y,
                    if y < far + 2 && far < ground - (35.0 * hs) as i32 { hex(0x8a8fd4) } else { hex(0x2b2863) },
                );
            }
        }
        for x in 0..w {
            let xf = x as f32 * 160.0 / w.max(1) as f32;
            let near =
                (ground as f32 - 17.0 * hs - ((xf * 0.07 + 3.0).sin() * 5.0 + (xf * 0.17).sin() * 2.5) * hs) as i32;
            for y in near..ground {
                let cap = y < near + 2 && near < ground - (20.0 * hs) as i32;
                let col = if cap {
                    hex(0xb8c4f0)
                } else if y < near + 4 && bayer(x, y) > 0.5 {
                    hex(0x26235a)
                } else {
                    hex(0x1b1942)
                };
                land.set(x, y, col);
            }
        }
        let mut rng = Rng::new(7);
        let pines = (w / 25).max(3);
        for _ in 0..pines {
            let x = rng.range(0.0, w as f32) as i32;
            let ph = rng.range(7.0, 14.0) as i32;
            for k in 0..ph {
                let half = (k % 4 + k / 2) / 2;
                for i in -half..=half {
                    let col = if i == -half && k % 4 == 0 { hex(0xc9d4f5) } else { hex(0x12112e) };
                    land.set(x + i, ground - ph + k, col);
                }
            }
        }
        for y in ground..h {
            for x in 0..w {
                let col = if y == ground {
                    hex(0xeef5ff)
                } else if y == ground + 1 {
                    hex(0xd4e2fb)
                } else if (y - ground) as f32 / 14.0 > bayer(x, y) {
                    hex(0x8d9fd4)
                } else {
                    hex(0xb3c3ec)
                };
                land.set(x, y, if hash01(x, y) > 0.985 { hex(0xffffff) } else { col });
            }
        }

        let stars = (0..(w * h / 300).max(20))
            .filter_map(|_| {
                let (x, y) = (rng.range(0.0, w as f32) as i32, rng.range(0.0, ground as f32 * 0.6) as i32);
                let near_moon = ((x as f32 - mx).hypot(y as f32 - my)) < r * 2.2;
                (!near_moon).then(|| (x, y, rng.range(1.0, 4.0), rng.range(0.0, 7.0), rng.chance(0.12)))
            })
            .collect();
        Backdrop { sky, land, stars }
    }

    pub fn draw_sky(&self, c: &mut Canvas, time: f32) {
        c.blit(&self.sky, 0, 0);
        for &(x, y, sp, ph, big) in &self.stars {
            let b = ((time * sp + ph).sin() + 1.0) / 2.0;
            if b > 0.35 {
                c.set(x, y, if b > 0.8 { hex(0xffffff) } else { hex(0x8f96d8) });
            }
            if big && b > 0.85 {
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    c.set(x + dx, y + dy, hex(0x8f96d8));
                }
            }
        }
    }

    pub fn draw_land(&self, c: &mut Canvas) {
        c.blit(&self.land, 0, 0);
    }
}
