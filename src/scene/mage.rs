//! The frost mage: patrols the bottom of the screen, throws ice, summons friends.

use super::ice::{CUBE_FRAMES, ICE_PAL};
use crate::render::canvas::{Canvas, Rgba, hex};

const PAL: &[(char, Rgba)] = &[
    ('H', hex(0x2a2f8a)),
    ('h', hex(0x4a56d8)),
    ('S', hex(0xffd64a)),
    ('F', hex(0xf4c49a)),
    ('E', hex(0x141028)),
    ('B', hex(0xeef2f8)),
    ('b', hex(0xb4c0d4)),
    ('R', hex(0x3640a8)),
    ('r', hex(0x232a78)),
    ('T', hex(0xffd64a)),
    ('K', hex(0x3a2718)),
];

pub(crate) const BODY: &[&str] = &[
    "...Hh",
    "....HH",
    "....HHh",
    ".....HHh",
    ".....HHhh",
    "....HHHShh",
    "....HHHHhhh",
    "..HHHHHHHHHHHH",
    "......FFFFEF",
    "......FFFFFFF",
    ".....BBFFBBB",
    ".....BBBBBBBb",
    "....RRBBBBBbRR",
    "...RRRRBBBbRRRr",
    "...RRRRRBBRRRRr",
    "...RRRRRRBRRRRr",
    "...RRRRRTRRRRrr",
    "..RRRRRRTRRRRRrr",
    "..RRRRRRTRRRRRrr",
    "..RRRRRRTRRRRRrr",
    ".TTTTTTTTTTTTTTT",
];
pub(crate) const FEET: [&str; 3] = ["....KK.....KK", ".....KK..KK..", "...KK......KK"];

pub const WIDTH: i32 = 16;
pub const HEIGHT: i32 = 22;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Walk,
    Charge,
    Throw,
    Recover,
    Summon,
    /// Pauses and watches while you speak.
    Watch,
    /// Hit by your correct answer.
    Stagger,
}

pub enum Event {
    /// Cube leaves the hand at this point.
    Release { x: f32, y: f32 },
    /// Summon ritual finished; spawn a friend.
    Summoned,
}

pub struct Mage {
    pub x: f32,
    pub dir: f32,
    pub act: Act,
    pub t: f32,
    walk_t: f32,
    blink: f32,
    next_blink: f32,
    pub volley: bool,
    pub bubble: Option<super::warrior::Bubble>,
}

const CHARGE: f32 = 0.7;
const THROW: f32 = 0.16;
const RECOVER: f32 = 0.3;
const SUMMON: f32 = 1.4;
const STAGGER: f32 = 1.0;

fn lerp2(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

impl Mage {
    pub fn new(x: f32) -> Self {
        Mage {
            x,
            dir: 1.0,
            act: Act::Walk,
            t: 0.0,
            walk_t: 0.0,
            blink: 0.0,
            next_blink: 2.0,
            volley: false,
            bubble: None,
        }
    }

    fn set(&mut self, act: Act) {
        self.act = act;
        self.t = 0.0;
    }

    pub fn busy(&self) -> bool {
        !matches!(self.act, Act::Walk | Act::Watch)
    }

    pub fn start_throw(&mut self, volley: bool) {
        if !self.busy() {
            self.volley = volley;
            self.set(Act::Charge);
        }
    }

    pub fn start_summon(&mut self) {
        if !self.busy() {
            self.set(Act::Summon);
        }
    }

    pub fn watch(&mut self, on: bool) {
        match (on, self.act) {
            (true, Act::Walk) => self.set(Act::Watch),
            (false, Act::Watch) => self.set(Act::Walk),
            _ => {}
        }
    }

    pub fn say(&mut self, text: impl Into<String>, seconds: f32) {
        self.bubble = Some(super::warrior::Bubble { text: text.into(), left: seconds });
    }

    pub fn stagger(&mut self) {
        self.set(Act::Stagger);
    }

    /// Advances animation and walking. `min_x..max_x` is the patrol range.
    pub fn step(&mut self, dt: f32, speed: f32, min_x: f32, max_x: f32, blink_roll: f32) -> Option<Event> {
        self.t += dt;
        if let Some(b) = &mut self.bubble {
            b.left -= dt;
            if b.left <= 0.0 {
                self.bubble = None;
            }
        }
        self.next_blink -= dt;
        self.blink -= dt;
        if self.next_blink < 0.0 {
            self.blink = 0.12;
            self.next_blink = 1.5 + blink_roll * 3.0;
        }
        match self.act {
            Act::Walk => {
                self.walk_t += dt;
                self.x += self.dir * speed * dt;
                if self.x < min_x {
                    self.x = min_x;
                    self.dir = 1.0;
                } else if self.x > max_x {
                    self.x = max_x;
                    self.dir = -1.0;
                }
                None
            }
            Act::Charge if self.t > CHARGE => {
                self.set(Act::Throw);
                None
            }
            Act::Throw if self.t > THROW => {
                let (x, y) = self.hand(0.0);
                self.set(Act::Recover);
                Some(Event::Release { x, y })
            }
            Act::Recover if self.t > RECOVER => {
                self.set(Act::Walk);
                None
            }
            Act::Summon if self.t > SUMMON => {
                self.set(Act::Walk);
                Some(Event::Summoned)
            }
            Act::Stagger => {
                // Knocked back, away from where he faces.
                self.x = (self.x - self.dir * 18.0 * dt * (1.0 - self.t / STAGGER).max(0.0)).clamp(min_x, max_x);
                if self.t > STAGGER {
                    self.set(Act::Walk);
                }
                None
            }
            _ => None,
        }
    }

    /// 0..1 progress of the current charge (drives glow and cube size).
    pub fn charge_level(&self) -> f32 {
        match self.act {
            Act::Charge => (self.t / CHARGE).min(1.0),
            Act::Throw => 1.0,
            Act::Summon => (self.t / SUMMON).min(1.0),
            _ => 0.0,
        }
    }

    /// Hand position (local sprite coords, facing right) relative to the sprite origin.
    fn hand_local(&self) -> (f32, f32) {
        let idle = (13.0, 17.0);
        let back = (8.0, 8.0);
        let fwd = (17.0, 11.0);
        match self.act {
            Act::Charge => lerp2(idle, back, self.t / 0.25),
            Act::Throw => lerp2(back, fwd, self.t / THROW),
            Act::Recover => lerp2(fwd, idle, self.t / RECOVER),
            Act::Summon => (13.0, 9.0),
            Act::Stagger => (14.0, 12.0),
            _ => idle,
        }
    }

    fn origin(&self, feet_y: f32) -> (f32, f32) {
        let bob = if self.act == Act::Walk && (self.walk_t * 6.0).sin() < 0.0 { 1.0 } else { 0.0 };
        (self.x.round(), (feet_y - HEIGHT as f32 + bob).round())
    }

    fn world(&self, ox: f32, lx: f32) -> f32 {
        if self.dir >= 0.0 { ox + lx } else { ox + (WIDTH - 1) as f32 - lx }
    }

    /// Hand position in world space; `feet_y` 0 gives a position relative to feet.
    pub fn hand(&self, feet_y: f32) -> (f32, f32) {
        let (ox, oy) = self.origin(feet_y);
        let (lx, ly) = self.hand_local();
        (self.world(ox, lx), oy + ly)
    }

    pub fn draw(&self, c: &mut Canvas, feet_y: f32, time: f32) {
        let (ox, oy) = self.origin(feet_y);
        let flip = self.dir < 0.0;
        let charge = self.charge_level();
        let glow_c = hex(0x3d7fd0);

        // Staff (behind the body), raised during a summon.
        let staff_lx = 1.0;
        let lift = if self.act == Act::Summon { -4.0 * (self.t / 0.3).min(1.0) } else { 0.0 };
        let sx = self.world(ox, staff_lx);
        let crystal_y = oy - 5.0 + lift;
        c.glow(sx, crystal_y, 4.0 + charge * 6.0, 0.35 + (time * 4.0).sin() * 0.1 + charge * 0.5, glow_c);
        for y in (oy - 2.0 + lift) as i32..(feet_y as i32) {
            c.set(sx as i32, y, if y % 5 == 0 { hex(0xa8703c) } else { hex(0x7a4a24) });
        }
        c.sprite(&[".L.", "LWC", "CCD", "CCD", ".D."], ICE_PAL, sx as i32 - 1, crystal_y as i32 - 3, false);

        let (hx, hy) = self.hand(feet_y);
        if matches!(self.act, Act::Charge | Act::Throw) {
            c.glow(hx, hy - 4.0, 3.0 + charge * 5.0, 0.25 + charge * 0.5, glow_c);
        }

        // Body + walking feet.
        let stagger_x = if self.act == Act::Stagger { ((time * 40.0).sin() * 1.5).round() } else { 0.0 };
        let bx = (ox + stagger_x) as i32;
        c.sprite(BODY, PAL, bx, oy as i32, flip);
        let feet = if self.act == Act::Walk { FEET[1 + ((self.walk_t * 6.0) as usize % 2)] } else { FEET[0] };
        c.sprite(&[feet], PAL, bx, oy as i32 + 21, flip);
        if self.blink > 0.0 || self.act == Act::Stagger {
            let ex = self.world(ox, 10.0) + stagger_x;
            c.set(ex as i32, oy as i32 + 8, if self.act == Act::Stagger { hex(0x141028) } else { hex(0xf4c49a) });
        }
        // Back hand holding the staff.
        c.rect(sx as i32, oy as i32 + 12, 1, 2, hex(0xf4c49a));

        // Throwing arm: a 2px sleeve from shoulder to hand.
        let shoulder = (self.world(ox, 11.0) + stagger_x, oy + 13.0);
        let n = ((hx - shoulder.0).hypot(hy - shoulder.1)).round().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let (x, y) = (shoulder.0 + (hx - shoulder.0) * t, shoulder.1 + (hy - shoulder.1) * t);
            c.dot(x, y, hex(0x4a56d8));
            c.dot(x, y + 1.0, hex(0x2f39a0));
        }
        c.dot(hx, hy, hex(0xffd64a));
        c.rect(hx.round() as i32, hy.round() as i32 - 1, 2, 2, hex(0xf4c49a));

        // Ice cube forming in the hand.
        if matches!(self.act, Act::Charge | Act::Throw) {
            let size = if self.act == Act::Throw { 6 } else { 1 + (charge.min(0.8) / 0.8 * 5.0) as i32 };
            let (bx, by) = ((hx + 1.0 - size as f32 / 2.0).round() as i32, (hy - 2.0).round() as i32 - size);
            if size >= 6 {
                c.sprite(CUBE_FRAMES[if (time * 10.0) as i32 % 2 == 0 { 0 } else { 2 }], ICE_PAL, bx, by, false);
            } else {
                c.rect(bx, by, size, size, hex(0x8ad8f5));
                c.set(bx, by, hex(0xffffff));
                if size >= 3 {
                    c.rect(bx, by + size - 1, size, 1, hex(0x4ea2d8));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &mut Mage, seconds: f32) -> Vec<Event> {
        let mut out = Vec::new();
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds / dt) as i32 {
            if let Some(e) = m.step(dt, 10.0, 0.0, 200.0, 0.5) {
                out.push(e);
            }
        }
        out
    }

    #[test]
    fn walks_and_turns_around_at_the_patrol_edges() {
        let mut m = Mage::new(195.0);
        run(&mut m, 2.0);
        assert_eq!(m.dir, -1.0);
        assert!(m.x < 200.0);
    }

    #[test]
    fn throw_releases_exactly_one_cube_then_walks_again() {
        let mut m = Mage::new(50.0);
        m.start_throw(false);
        let events = run(&mut m, 2.0);
        assert_eq!(events.iter().filter(|e| matches!(e, Event::Release { .. })).count(), 1);
        assert_eq!(m.act, Act::Walk);
    }

    #[test]
    fn cannot_start_a_throw_mid_summon() {
        let mut m = Mage::new(50.0);
        m.start_summon();
        m.start_throw(false);
        assert_eq!(m.act, Act::Summon);
        let events = run(&mut m, 2.0);
        assert!(matches!(events.as_slice(), [Event::Summoned]));
    }

    #[test]
    fn watching_stops_walking() {
        let mut m = Mage::new(50.0);
        m.watch(true);
        run(&mut m, 1.0);
        assert_eq!(m.x, 50.0);
        m.watch(false);
        run(&mut m, 1.0);
        assert!(m.x > 50.0);
    }

    #[test]
    fn hand_is_mirrored_when_facing_left() {
        let mut m = Mage::new(100.0);
        let (right, _) = m.hand(100.0);
        m.dir = -1.0;
        let (left, _) = m.hand(100.0);
        assert!(right > 100.0 + WIDTH as f32 / 2.0);
        assert!(left < 100.0 + WIDTH as f32 / 2.0);
    }
}
