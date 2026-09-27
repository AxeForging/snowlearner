//! Fogueira: a campfire the warrior builds. Thaws the snow around it, warms
//! him while he sits close, and hisses out when ice keeps landing on it.

use crate::render::canvas::{Canvas, bayer, hex};

pub const LIFETIME: f32 = 60.0;
pub const WARM_RADIUS: f32 = 16.0;

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
