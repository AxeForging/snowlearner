//! Coruja de gelo: once the snow is deep, the frost mage now and then summons
//! one ice owl. It flies across the screen dropping snowballs that pile up
//! more snow, then leaves through the far side.

use super::ice::ICE_PAL;
use crate::render::canvas::{Canvas, Rgba, hex};

/// How far the pile must be toward the ice line (`Scene::pile_level`) before
/// the mage can summon the owl: deep snow, but before the pile freezes over.
pub const PILE_LEVEL: f32 = 0.6;
/// Seconds between summons (± 20%, seeded by the scene).
pub const EVERY: f32 = 50.0;
/// Snowballs in the air at once, at most.
pub const MAX_SNOWBALLS: usize = 3;

const SPEED: f32 = 38.0;
const THROW_EVERY: f32 = 0.9;
const WIDTH: i32 = 9;
const GRAVITY: f32 = 90.0;

const PAL: &[(char, Rgba)] = &[
    ('W', hex(0xffffff)),
    ('L', hex(0xc8f4ff)),
    ('C', hex(0x8ad8f5)),
    ('D', hex(0x4ea2d8)),
    ('E', hex(0x141028)),
    ('Y', hex(0xffd64a)),
];
const WINGS_UP: &[&str] =
    &["D.......D", "CD.....DC", "LCDWWWDCL", ".LWEWEWL.", "..WWYWW..", "..LWWWL..", "...LCL...", "...D.D..."];
const WINGS_DOWN: &[&str] =
    &["...WWW...", "..WEWEW..", "..WWYWW..", "LCWWWWWCL", "CDLWWWLDC", "D..LCL..D", "...D.D..."];

pub struct Owl {
    pub x: f32,
    pub y: f32,
    pub dir: f32,
    t: f32,
    throw_in: f32,
}

impl Owl {
    /// Enters just off the left or right edge of a `w`-wide screen at height `y`.
    pub fn new(from_left: bool, w: f32, y: f32) -> Self {
        let (x, dir) = if from_left { (-(WIDTH as f32), 1.0) } else { (w, -1.0) };
        Owl { x, y, dir, t: 0.0, throw_in: THROW_EVERY * 0.5 }
    }

    /// Flies on; returns where a snowball leaves its talons, only while on screen.
    pub fn step(&mut self, dt: f32, w: f32) -> Option<(f32, f32)> {
        self.t += dt;
        self.x += self.dir * SPEED * dt;
        self.throw_in -= dt;
        let on_screen = self.x >= 0.0 && self.x <= w - WIDTH as f32;
        if self.throw_in <= 0.0 && on_screen {
            self.throw_in = THROW_EVERY;
            return Some((self.x + WIDTH as f32 / 2.0, self.y + 8.0));
        }
        None
    }

    /// Out through the far side.
    pub fn gone(&self, w: f32) -> bool {
        (self.dir > 0.0 && self.x > w) || (self.dir < 0.0 && self.x < -(WIDTH as f32))
    }

    pub fn draw(&self, c: &mut Canvas) {
        let bob = (self.t * 6.0).sin() * 1.5;
        let wings = if (self.t * 5.0) as i32 % 2 == 0 { WINGS_UP } else { WINGS_DOWN };
        c.sprite(wings, PAL, self.x.round() as i32, (self.y + bob).round() as i32, self.dir < 0.0);
    }
}

/// A snowball the owl drops: falls in an arc and adds snow where it lands.
pub struct Snowball {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
}

impl Snowball {
    pub fn new((x, y): (f32, f32), dir: f32) -> Self {
        Snowball { x, y, vx: dir * SPEED * 0.6, vy: 0.0 }
    }

    pub fn step(&mut self, dt: f32) {
        self.vy += GRAVITY * dt;
        self.x += self.vx * dt;
        self.y += self.vy * dt;
    }

    pub fn draw(&self, c: &mut Canvas) {
        let (x, y) = (self.x.round() as i32, self.y.round() as i32);
        c.sprite(&[".L.", "LWC", ".D."], ICE_PAL, x - 1, y - 1, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::font;

    #[test]
    fn crosses_the_screen_dropping_snowballs_then_leaves() {
        let mut o = Owl::new(true, 200.0, 20.0);
        let dt = 1.0 / 30.0;
        let mut drops = Vec::new();
        let mut steps = 0;
        while !o.gone(200.0) {
            if let Some(p) = o.step(dt, 200.0) {
                drops.push(p);
            }
            steps += 1;
            assert!(steps < 30 * 20, "never leaves");
        }
        assert!(drops.len() >= 4, "snowballs along the way: {}", drops.len());
        assert!(drops.iter().all(|&(x, _)| (0.0..=200.0).contains(&x)), "only while on screen");
        assert!(drops.windows(2).all(|w| w[1].0 > w[0].0), "spread across, left to right");
    }

    #[test]
    fn from_the_right_it_flies_left_and_leaves_on_the_left() {
        let mut o = Owl::new(false, 200.0, 20.0);
        for _ in 0..(30 * 10) {
            o.step(1.0 / 30.0, 200.0);
        }
        assert!(o.gone(200.0));
        assert!(o.x < 0.0);
    }

    #[test]
    fn a_snowball_falls() {
        let mut b = Snowball::new((50.0, 10.0), 1.0);
        for _ in 0..30 {
            b.step(1.0 / 30.0);
        }
        assert!(b.y > 40.0 && b.x > 50.0);
    }

    #[test]
    fn the_summon_line_is_pt_br_the_font_can_draw() {
        assert!(font::supports(crate::lang::T::MageOwl.get(crate::lang::Native::En)));
    }
}
