//! Fogueira: a campfire the warrior builds. Thaws the snow around it, warms
//! him while he sits close, and hisses out when ice keeps landing on it.
//! Also the flash where a fireball lands (`Flash`, #119).

use crate::render::canvas::{Canvas, bayer, hex};

pub const LIFETIME: f32 = 60.0;
pub const WARM_RADIUS: f32 = 16.0;

/// How long the flash where a fireball lands lasts (s, #119).
pub const FLASH_S: f32 = 0.3;

/// The flash where a fireball lands (#119): a dithered disc, white at the
/// heart, that swells a little and thins out in `FLASH_S`. Its size follows
/// the spell's tier (`Spell::flash_radius`). Always local, never the whole
/// screen; only light, it melts nothing.
pub struct Flash {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    t: f32,
}

impl Flash {
    pub fn new(x: f32, y: f32, radius: f32) -> Self {
        Flash { x, y, radius, t: 0.0 }
    }

    pub fn step(&mut self, dt: f32) {
        self.t += dt;
    }

    pub fn alive(&self) -> bool {
        self.t < FLASH_S
    }

    pub fn draw(&self, c: &mut Canvas) {
        if !self.alive() {
            return;
        }
        let k = 1.0 - self.t / FLASH_S;
        let r = self.radius * (0.7 + 0.3 * (1.0 - k));
        let reach = r.ceil() as i32;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let d = (dx as f32).hypot(dy as f32) / r;
                let (px, py) = (self.x.round() as i32 + dx, self.y.round() as i32 + dy);
                if d > 1.0 || bayer(px, py) >= k * (1.0 - d) * 1.6 {
                    continue;
                }
                c.set(px, py, hex(if d < 0.4 { 0xffffff } else { 0xfff4b0 }));
            }
        }
    }
}

pub struct Fire {
    pub x: f32,
    pub life: f32,
}

impl Fire {
    pub fn new(x: f32) -> Self {
        Fire { x, life: LIFETIME }
    }

    pub fn alive(&self) -> bool {
        self.life > 0.0
    }

    /// 0..1 — flames shrink in the last seconds.
    pub fn strength(&self) -> f32 {
        (self.life / 8.0).clamp(0.0, 1.0)
    }

    /// An ice cube landed on it.
    pub fn douse(&mut self) {
        self.life -= 15.0;
    }

    pub fn draw(&self, c: &mut Canvas, ground_y: f32, time: f32) {
        let (cx, gy) = (self.x.round() as i32, ground_y.round() as i32);
        let s = self.strength();
        // Warm light on the snow.
        for y in gy - 10..gy + 3 {
            for x in cx - 18..=cx + 18 {
                let d = (((x - cx) as f32 / 18.0).powi(2) + ((y - gy + 3) as f32 / 10.0).powi(2)).sqrt();
                if d < 1.0 && (1.0 - d) * 0.45 * s > bayer(x, y) && c.get(x, y).is_some_and(|p| p[3] != 0) {
                    c.set(x, y, hex(0xffb87a));
                }
            }
        }
        // Logs.
        for i in -4..=4 {
            c.set(cx + i, gy - 1, if i % 3 == 0 { hex(0x4a2a14) } else { hex(0x7a4a24) });
        }
        c.set(cx - 3, gy - 2, hex(0x7a4a24));
        c.set(cx + 3, gy - 2, hex(0x7a4a24));
        // Flame tongues.
        let fire = [hex(0xfff4b0), hex(0xffc13d), hex(0xff7a2a), hex(0xd93a2a)];
        for i in -3..=3 {
            let x = cx + i;
            let edge = 1.0 - (i.abs() as f32 / 4.0);
            let h = ((3.0 + ((time * 11.0 + x as f32 * 1.7).sin() + (time * 7.3 + x as f32 * 0.9).sin() + 2.0) * 1.4)
                * edge
                * s)
                .round() as i32;
            for k in 0..h {
                let idx = (k * 4 / h.max(1)).min(3) as usize;
                c.set(x, gy - 2 - k, fire[idx]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(f: &Flash) -> Vec<(i32, i32)> {
        let mut c = Canvas::new(80, 80);
        f.draw(&mut c);
        assert!(c.bytes().chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
        (0..80).flat_map(|y| (0..80).map(move |x| (x, y))).filter(|&(x, y)| c.opaque_in(x, y, 1, 1) > 0).collect()
    }

    #[test]
    fn a_flash_stays_inside_its_radius_and_is_gone_after_its_time() {
        let mut f = Flash::new(40.0, 40.0, 10.0);
        for _ in 0..(FLASH_S * 30.0) as i32 {
            let px = drawn(&f);
            assert!(px.iter().all(|&(x, y)| ((x - 40) as f32).hypot((y - 40) as f32) <= 10.5), "local");
            f.step(1.0 / 30.0);
        }
        f.step(0.1);
        assert!(!f.alive());
        assert!(drawn(&f).is_empty());
    }

    #[test]
    fn a_bigger_radius_lights_more() {
        let small = drawn(&Flash::new(40.0, 40.0, 6.0)).len();
        let big = drawn(&Flash::new(40.0, 40.0, 16.0)).len();
        assert!(small > 0 && big > small * 2, "{small} {big}");
    }
}
