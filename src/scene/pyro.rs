//! The fire mage — your champion. He only shows up while you practice:
//! teleports in, charges a fireball while you speak, and casts it at the frost
//! mage when you get the phrase right (or fizzles when you don't).

use super::mage::{BODY, FEET, HEIGHT, WIDTH};
use crate::render::canvas::{Canvas, Rgba, bayer, hex};

const PAL: &[(char, Rgba)] = &[
    ('H', hex(0x8a1c1c)),
    ('h', hex(0xd93a2a)),
    ('S', hex(0xffd64a)),
    ('F', hex(0xf4c49a)),
    ('E', hex(0x141028)),
    ('B', hex(0x6b3f1d)),
    ('b', hex(0x4a2a14)),
    ('R', hex(0xb8341f)),
    ('r', hex(0x7a1f16)),
    ('T', hex(0xffc13d)),
    ('K', hex(0x3a2718)),
];

const ARRIVE: f32 = 0.6;
const CAST: f32 = 0.35;
const FIZZLE: f32 = 0.8;
const LEAVE: f32 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Hidden,
    Arrive,
    Ready,
    Cast,
    Fizzle,
    Leave,
}

/// What the fire mage casts for a right answer: the fuller the answer, the
/// bigger the spell. Same fireball art, scaled; one fireball per answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spell {
    /// Short answer: a small fireball.
    Spark,
    /// Complete answer: the usual fireball.
    #[default]
    Fireball,
    /// Polished answer: a big fireball.
    Blaze,
}

impl Spell {
    /// Share of `Pace::melt_fraction` this spell melts.
    pub fn melt(self) -> f32 {
        match self {
            Spell::Spark => 0.6,
            Spell::Fireball => 1.0,
            Spell::Blaze => 1.5,
        }
    }

    /// Half-size of the fireball in art pixels; also scales the impact embers.
    pub fn radius(self) -> i32 {
        match self {
            Spell::Spark => 1,
            Spell::Fireball => 2,
            Spell::Blaze => 3,
        }
    }
}

pub enum Event {
    /// Fireball leaves the staff here.
    Release { x: f32, y: f32 },
}

pub struct Pyro {
    pub x: f32,
    /// +1 faces right.
    pub dir: f32,
    pub act: Act,
    t: f32,
    /// 0..1 fireball charge, grows while you are speaking.
    pub charge: f32,
    focused: bool,
    pending_cast: bool,
}

impl Default for Pyro {
    fn default() -> Self {
        Pyro { x: 0.0, dir: 1.0, act: Act::Hidden, t: 0.0, charge: 0.0, focused: false, pending_cast: false }
    }
}

impl Pyro {
    fn set(&mut self, act: Act) {
        self.act = act;
        self.t = 0.0;
    }

    pub fn visible(&self) -> bool {
        self.act != Act::Hidden
    }

    /// Teleports in at `x`, facing `toward_x`.
    pub fn arrive(&mut self, x: f32, toward_x: f32) {
        if matches!(self.act, Act::Hidden | Act::Leave) {
            self.x = x;
            self.dir = if toward_x >= x { 1.0 } else { -1.0 };
            self.charge = 0.0;
            self.set(Act::Arrive);
        }
    }

    pub fn leave(&mut self) {
        if !matches!(self.act, Act::Hidden | Act::Leave) {
            // Let a fireball in progress finish first.
            if self.act == Act::Cast {
                return;
            }
            self.focused = false;
            self.set(Act::Leave);
        }
    }

    /// Charging while the learner speaks.
    pub fn focus(&mut self, on: bool) {
        self.focused = on;
    }

    pub fn cast(&mut self) {
        match self.act {
            Act::Ready => self.set(Act::Cast),
            Act::Arrive => self.pending_cast = true,
            _ => {}
        }
    }

    pub fn fizzle(&mut self) {
        if self.act == Act::Ready {
            self.charge = 0.0;
            self.set(Act::Fizzle);
        }
    }

    fn staff_top(&self, feet_y: f32) -> (f32, f32) {
        let x = if self.dir >= 0.0 { self.x + (WIDTH - 2) as f32 } else { self.x + 1.0 };
        (x, feet_y - HEIGHT as f32 - 6.0)
    }

    pub fn step(&mut self, dt: f32, feet_y: f32) -> Option<Event> {
        self.t += dt;
        if self.focused {
            self.charge = (self.charge + dt * 0.35).min(1.0);
        }
        match self.act {
            Act::Arrive if self.t > ARRIVE => {
                self.set(Act::Ready);
                if std::mem::take(&mut self.pending_cast) {
                    self.set(Act::Cast);
                }
                None
            }
            Act::Cast if self.t > CAST => {
                let (x, y) = self.staff_top(feet_y);
                self.charge = 0.0;
                self.set(Act::Ready);
                Some(Event::Release { x, y })
            }
            Act::Fizzle if self.t > FIZZLE => {
                self.set(Act::Ready);
                None
            }
            Act::Leave if self.t > LEAVE => {
                self.set(Act::Hidden);
                None
            }
            _ => None,
        }
    }

    /// 0..1 how "materialized" he is (teleport effects).
    fn presence(&self) -> f32 {
        match self.act {
            Act::Hidden => 0.0,
            Act::Arrive => self.t / ARRIVE,
            Act::Leave => 1.0 - self.t / LEAVE,
            _ => 1.0,
        }
    }

    pub fn draw(&self, c: &mut Canvas, feet_y: f32, time: f32) {
        if !self.visible() {
            return;
        }
        let presence = self.presence().clamp(0.0, 1.0);
        let flip = self.dir < 0.0;
        let (ox, oy) = (self.x.round() as i32, (feet_y - HEIGHT as f32).round() as i32);

        // Pillar of fire while teleporting.
        if matches!(self.act, Act::Arrive | Act::Leave) {
            let cx = ox + WIDTH / 2;
            for y in (oy - 12)..(feet_y as i32) {
                for dx in -4..=4 {
                    let k = 1.0 - (dx as f32).abs() / 5.0;
                    let flicker = ((time * 20.0 + y as f32 * 0.7).sin() + 1.0) * 0.25;
                    if (1.0 - presence) * k + flicker * 0.3 > bayer(cx + dx, y) + 0.2 {
                        let col = if dx.abs() < 2 { hex(0xfff4b0) } else { hex(0xff7a2a) };
                        c.set(cx + dx, y, col);
                    }
                }
            }
        }

        // Staff with a flame crystal; its glow grows with the charge.
        let (sx, sy) = self.staff_top(feet_y);
        let glow = 0.3 + self.charge * 0.6 + if self.act == Act::Cast { 0.4 } else { 0.0 };
        c.glow(sx, sy, 3.0 + self.charge * 7.0, glow * presence, hex(0xff7a2a));
        if presence > 0.5 {
            for y in (sy as i32 + 3)..(feet_y as i32) {
                c.set(sx as i32, y, if y % 5 == 0 { hex(0x8b5a2b) } else { hex(0x5a3418) });
            }
            let flame = [hex(0xfff4b0), hex(0xffc13d), hex(0xff7a2a)];
            for k in 0..4 {
                let h = 2 + ((time * 12.0 + k as f32).sin() * 1.5) as i32;
                for j in 0..h.max(1) {
                    c.set(sx as i32 + (k % 3) - 1, sy as i32 + 2 - j, flame[(j as usize).min(2)]);
                }
            }
        }

        // Body, dissolving in and out through the dither pattern.
        let mut body = Canvas::new(WIDTH, HEIGHT);
        body.sprite(BODY, PAL, 0, 0, flip);
        body.sprite(&[FEET[0]], PAL, 0, HEIGHT - 1, flip);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if let Some(p) = body.get(x, y).filter(|p| p[3] != 0)
                    && presence > bayer(ox + x, oy + y)
                {
                    c.set(ox + x, oy + y, p);
                }
            }
        }
        if self.act == Act::Fizzle {
            // Sad puff of smoke above the staff.
            for i in 0..5 {
                let y = sy - self.t * 12.0 - i as f32 * 2.0;
                c.dot(sx + ((time * 6.0 + i as f32).sin() * 2.0), y, hex(0x8f96a8));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &mut Pyro, seconds: f32) -> usize {
        let mut released = 0;
        for _ in 0..(seconds * 60.0) as i32 {
            if let Some(Event::Release { .. }) = p.step(1.0 / 60.0, 100.0) {
                released += 1;
            }
        }
        released
    }

    #[test]
    fn appears_only_for_a_lesson_and_leaves_afterwards() {
        let mut p = Pyro::default();
        assert!(!p.visible());
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        assert_eq!(p.act, Act::Ready);
        assert_eq!(p.dir, 1.0, "faces the frost mage");
        p.leave();
        run(&mut p, 1.0);
        assert!(!p.visible());
    }

    #[test]
    fn a_cast_right_after_arriving_still_fires_once() {
        let mut p = Pyro::default();
        p.arrive(20.0, 0.0);
        p.cast();
        assert_eq!(run(&mut p, 2.0), 1);
    }

    #[test]
    fn leaving_mid_cast_waits_for_the_fireball() {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.cast();
        p.leave();
        assert_eq!(run(&mut p, 0.5), 1);
    }

    #[test]
    fn speaking_charges_the_fireball_and_a_miss_fizzles_it() {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.focus(true);
        run(&mut p, 2.0);
        assert!(p.charge > 0.5);
        p.fizzle();
        assert_eq!(p.charge, 0.0);
        assert_eq!(run(&mut p, 1.0), 0);
    }
}
