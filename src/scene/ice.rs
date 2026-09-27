//! Thrown ice cubes and every small particle (shards, steam, sparks, embers).

use crate::render::canvas::{Canvas, Rgba, bayer, hex};
use std::collections::VecDeque;

pub const ICE_PAL: &[(char, Rgba)] =
    &[('W', hex(0xffffff)), ('L', hex(0xc8f4ff)), ('C', hex(0x8ad8f5)), ('D', hex(0x4ea2d8)), ('d', hex(0x2a5a9a))];
const CUBE_A: &[&str] = &["LLLLLD", "LWWCCD", "LWCCCD", "LCCCCD", "LCCCDD", "DDDDDd"];
const CUBE_B: &[&str] = &["..LD..", ".LWCD.", "LWCCCD", "LCCCCD", ".DCCd.", "..Dd.."];
const CUBE_C: &[&str] = &["DLLLLL", "DCCWWL", "DCCCWL", "DCCCCL", "DDCCCL", "dDDDDD"];
pub const CUBE_FRAMES: [&[&str]; 4] = [CUBE_A, CUBE_B, CUBE_C, CUBE_B];

pub struct Cube {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub g: f32,
    pub age: f32,
    trail: VecDeque<(f32, f32)>,
}

impl Cube {
    /// Launches a cube from (sx,sy) that reaches (tx,ty) after `flight` seconds.
    pub fn aimed(sx: f32, sy: f32, tx: f32, ty: f32, flight: f32, g: f32) -> Self {
        Cube {
            x: sx,
            y: sy,
            vx: (tx - sx) / flight,
            vy: (ty - sy - 0.5 * g * flight * flight) / flight,
            g,
            age: 0.0,
            trail: VecDeque::new(),
        }
    }

    pub fn step(&mut self, dt: f32) {
        self.trail.push_front((self.x, self.y));
        self.trail.truncate(7);
        self.age += dt;
        self.x += self.vx * dt;
        self.vy += self.g * dt;
        self.y += self.vy * dt;
    }

    pub fn draw(&self, c: &mut Canvas) {
        let n = self.trail.len().max(1) as f32;
        for (i, &(x, y)) in self.trail.iter().enumerate() {
            if 1.0 - i as f32 / n > bayer(x as i32, y as i32) + 0.1 {
                c.dot(x, y, if i < 2 { hex(0xc8f4ff) } else { hex(0x4ea2d8) });
            }
        }
        let frame = CUBE_FRAMES[(self.age * 16.0) as usize % 4];
        c.sprite(frame, ICE_PAL, (self.x - 3.0).round() as i32, (self.y - 3.0).round() as i32, false);
    }
}

const ICICLE: &[&str] = &["WLCD", "WLCD", ".LCD", ".LC.", ".LC.", "..C.", "..C.", "..L."];

/// An icicle from the frost mage's icicle rain: hangs and shakes at the top
/// of the screen for `delay` seconds, then drops.
pub struct Icicle {
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub delay: f32,
}

impl Icicle {
    pub fn new(x: f32, delay: f32) -> Self {
        Icicle { x, y: 0.0, vy: 0.0, delay }
    }

    pub fn step(&mut self, dt: f32) {
        if self.delay > 0.0 {
            self.delay -= dt;
            return;
        }
        self.vy += 260.0 * dt;
        self.y += self.vy * dt;
    }

    /// Tip position (bottom of the sprite).
    pub fn tip(&self) -> (f32, f32) {
        (self.x + 2.0, self.y + ICICLE.len() as f32)
    }

    pub fn draw(&self, c: &mut Canvas, time: f32) {
        let shake = if self.delay > 0.0 { ((time * 40.0 + self.x).sin() * 1.0).round() as i32 } else { 0 };
        c.sprite(ICICLE, ICE_PAL, self.x.round() as i32 + shake, self.y.round() as i32, false);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Ice fragment; bounces on the snow.
    Shard,
    /// Rises and fades.
    Steam,
    /// Magic sparkle; no gravity.
    Spark,
    /// Fire ember; floats up.
    Ember,
}

pub struct Particle {
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub g: f32,
    pub life: f32,
    pub max: f32,
    pub color: Rgba,
    pub big: bool,
    /// Resting on the ground (shards only).
    pub rest: bool,
}

impl Particle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(kind: Kind, x: f32, y: f32, vx: f32, vy: f32, life: f32, color: Rgba) -> Self {
        let g = match kind {
            Kind::Shard => 220.0,
            Kind::Steam => 0.0,
            Kind::Spark => 0.0,
            Kind::Ember => -12.0,
        };
        Particle { kind, x, y, vx, vy, g, life, max: life, color, big: false, rest: false }
    }

    pub fn big(mut self) -> Self {
        self.big = true;
        self
    }

    /// `floor(x)` = y coordinate of the surface under x.
    pub fn step(&mut self, dt: f32, floor: impl Fn(f32) -> f32) {
        self.life -= dt;
        if self.rest {
            return;
        }
        self.vy += self.g * dt;
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        if self.kind == Kind::Steam {
            self.vx *= 0.97;
        }
        if self.kind == Kind::Shard {
            let f = floor(self.x) - 1.0;
            if self.y >= f {
                self.y = f;
                self.vy *= -0.35;
                self.vx *= 0.6;
                if self.vy.abs() < 12.0 {
                    self.rest = true;
                }
            }
        }
    }

    pub fn alive(&self) -> bool {
        self.life > 0.0
    }

    pub fn draw(&self, c: &mut Canvas) {
        let a = self.life / self.max;
        let (xi, yi) = (self.x.round() as i32, self.y.round() as i32);
        let threshold = if self.kind == Kind::Shard && !self.rest { 0.0 } else { bayer(xi, yi) * 0.9 };
        if a < threshold {
            return;
        }
        c.set(xi, yi, self.color);
        if self.big {
            c.set(xi + 1, yi, if self.color == hex(0xffffff) { hex(0xc8f4ff) } else { hex(0x4ea2d8) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aimed_cube_lands_on_its_target_after_the_flight_time() {
        let mut cube = Cube::aimed(10.0, 50.0, 150.0, 80.0, 0.8, 130.0);
        let dt: f32 = 1.0 / 240.0;
        for _ in 0..(0.8_f32 / dt).round() as i32 {
            cube.step(dt);
        }
        assert!((cube.x - 150.0).abs() < 0.5, "x {}", cube.x);
        assert!((cube.y - 80.0).abs() < 1.5, "y {}", cube.y);
    }

    #[test]
    fn icicles_hang_first_then_fall() {
        let mut i = Icicle::new(10.0, 0.5);
        i.step(0.4);
        assert_eq!(i.y, 0.0, "still hanging");
        for _ in 0..30 {
            i.step(1.0 / 30.0);
        }
        assert!(i.y > 20.0);
    }

    #[test]
    fn shards_come_to_rest_on_the_floor() {
        let mut p = Particle::new(Kind::Shard, 10.0, 0.0, 5.0, 0.0, 5.0, hex(0xffffff));
        for _ in 0..600 {
            p.step(1.0 / 60.0, |_| 50.0);
        }
        assert!(p.rest);
        assert!((p.y - 49.0).abs() < 1e-3);
    }

    #[test]
    fn embers_float_up_and_expire() {
        let mut p = Particle::new(Kind::Ember, 10.0, 50.0, 0.0, -20.0, 0.5, hex(0xffc13d));
        for _ in 0..40 {
            p.step(1.0 / 60.0, |_| 100.0);
        }
        assert!(p.y < 50.0);
        assert!(!p.alive());
    }
}
