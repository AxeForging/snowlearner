//! The warrior trying to survive your frozen screen: builds campfires, gives
//! tips, freezes solid if you ignore him, and thaws when you speak.

use crate::render::canvas::{Canvas, Rgba, hex};
use crate::render::font;

const PAL: &[(char, Rgba)] = &[
    ('O', hex(0xf2e6c8)),
    ('M', hex(0x9aa4b8)),
    ('m', hex(0x5c6478)),
    ('F', hex(0xf4c49a)),
    ('E', hex(0x141028)),
    ('R', hex(0xc8561e)),
    ('T', hex(0x3f8a4a)),
    ('t', hex(0x2a5e33)),
    ('B', hex(0x6b3f1d)),
    ('b', hex(0xffd64a)),
    ('P', hex(0x5a4a3a)),
    ('K', hex(0x3a2718)),
    ('S', hex(0x8b5a2b)),
    ('Y', hex(0xc9a24a)),
];

/// Same sprite, re-colored as an ice statue.
const ICE: &[(char, Rgba)] = &[
    ('O', hex(0xffffff)),
    ('M', hex(0xc8f4ff)),
    ('m', hex(0x8ad8f5)),
    ('F', hex(0xc8f4ff)),
    ('E', hex(0x2a5a9a)),
    ('R', hex(0x8ad8f5)),
    ('T', hex(0x4ea2d8)),
    ('t', hex(0x2a5a9a)),
    ('B', hex(0x4ea2d8)),
    ('b', hex(0xffffff)),
    ('P', hex(0x4ea2d8)),
    ('K', hex(0x2a5a9a)),
    ('S', hex(0x8ad8f5)),
    ('Y', hex(0xc8f4ff)),
];

const UPPER: &[&str] = &[
    "..O.....O...",
    "..OMMMMMO...",
    "...MMMMMM...",
    "..mMMMMMMm..",
    "...FFFEFF...",
    "...FFFFFF...",
    "..RRFFFFRR..",
    "..RRRRRRR...",
    ".TTRRRRRTT..",
    "YTTTTTTTTTF.",
    "SSTTTTTTTt..",
    "SSBBBbBBBB..",
    "YSTTTTTTTt..",
];
const LEGS_STAND: &[&str] = &["..PPP..PPP..", "..PPP..PPP..", "..KKK..KKK.."];
const LEGS_STEP: &[&str] = &["..PPP.PPP...", "...PP..PP...", "...KKK.KKK.."];
const LEGS_SIT: &[&str] = &["..PPPPPPPK.."];

pub const WIDTH: i32 = 12;
pub const HEIGHT: i32 = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Wander,
    /// Kneeling, striking sparks. Finishes with `Event::FireLit`.
    Build,
    /// Sitting by the fire.
    Warm,
    Frozen,
    Cheer,
}

pub enum Event {
    FireLit { x: f32 },
}

pub struct Bubble {
    pub text: String,
    pub left: f32,
}

pub struct Warrior {
    pub x: f32,
    pub dir: f32,
    pub act: Act,
    pub t: f32,
    /// 1 = toasty, 0 = frozen solid.
    pub warmth: f32,
    walk_t: f32,
    target: f32,
    pub bubble: Option<Bubble>,
}

const BUILD_TIME: f32 = 2.5;

impl Warrior {
    pub fn new(x: f32) -> Self {
        Warrior { x, dir: -1.0, act: Act::Wander, t: 0.0, warmth: 1.0, walk_t: 0.0, target: x, bubble: None }
    }

    fn set(&mut self, act: Act) {
        self.act = act;
        self.t = 0.0;
    }

    pub fn say(&mut self, text: impl Into<String>, seconds: f32) {
        if self.act != Act::Frozen {
            self.bubble = Some(Bubble { text: text.into(), left: seconds });
        }
    }

    pub fn start_building(&mut self) {
        if matches!(self.act, Act::Wander) {
            self.set(Act::Build);
        }
    }

    /// Your correct answer reached him.
    pub fn warm_burst(&mut self) {
        self.warmth = 1.0;
        self.set(Act::Cheer);
    }

    /// `freeze` 0..1 = how frozen the screen is; `near_fire` = sitting range.
    pub fn step(&mut self, dt: f32, freeze: f32, near_fire: bool, min_x: f32, max_x: f32, roll: f32) -> Option<Event> {
        self.t += dt;
        if let Some(b) = &mut self.bubble {
            b.left -= dt;
            if b.left <= 0.0 {
                self.bubble = None;
            }
        }

        if self.act != Act::Frozen {
            let chill = 0.004 + freeze * 0.03;
            let heat = if near_fire { 0.08 } else { 0.0 };
            self.warmth = (self.warmth - chill * dt + heat * dt).clamp(0.0, 1.0);
            if self.warmth <= 0.0 {
                self.bubble = None;
                self.set(Act::Frozen);
                return None;
            }
        }

        match self.act {
            Act::Wander => {
                if near_fire && self.warmth < 0.95 {
                    self.set(Act::Warm);
                    return None;
                }
                if (self.target - self.x).abs() < 1.0 {
                    self.target = min_x + roll * (max_x - min_x);
                }
                let step = 7.0 * dt * (0.4 + self.warmth * 0.6);
                self.dir = if self.target > self.x { 1.0 } else { -1.0 };
                self.x = (self.x + self.dir * step).clamp(min_x, max_x);
                self.walk_t += dt;
                None
            }
            Act::Build if self.t > BUILD_TIME => {
                self.set(Act::Warm);
                Some(Event::FireLit { x: self.x + WIDTH as f32 / 2.0 + self.dir * 12.0 })
            }
            Act::Warm if !near_fire || self.warmth >= 1.0 => {
                self.set(Act::Wander);
                None
            }
            Act::Cheer if self.t > 1.6 => {
                self.set(Act::Wander);
                None
            }
            _ => None,
        }
    }

    pub fn draw(&self, c: &mut Canvas, feet_y: f32, time: f32) {
        let flip = self.dir < 0.0;
        let frozen = self.act == Act::Frozen;
        let pal = if frozen { ICE } else { PAL };
        let shiver = if !frozen && self.warmth < 0.3 && ((time * 30.0) as i32 % 2 == 0) { 1 } else { 0 };
        let hop = if self.act == Act::Cheer { -(((time * 10.0).sin().abs()) * 3.0).round() as i32 } else { 0 };
        let x = self.x.round() as i32 + shiver;
        let sitting = matches!(self.act, Act::Build | Act::Warm);
        let legs = if sitting {
            LEGS_SIT
        } else if self.act == Act::Wander && (self.walk_t * 5.0) as i32 % 2 == 1 {
            LEGS_STEP
        } else {
            LEGS_STAND
        };
        let body_top = feet_y.round() as i32 - UPPER.len() as i32 - legs.len() as i32 + hop;
        c.sprite(UPPER, pal, x, body_top, flip);
        c.sprite(legs, pal, x, body_top + UPPER.len() as i32, flip);

        if self.act == Act::Build && (time * 8.0) as i32 % 2 == 0 {
            let sx = if flip { x - 3 } else { x + WIDTH + 2 };
            c.set(sx, feet_y as i32 - 3, hex(0xffd64a));
            c.set(sx + 1, feet_y as i32 - 5, hex(0xfff4b0));
        }
        if frozen {
            // Ice block around the statue.
            let (bx, by, bw, bh) = (x - 3, body_top - 3, WIDTH + 6, feet_y as i32 - body_top + 3);
            for yy in by..by + bh {
                for xx in bx..bx + bw {
                    let border = xx == bx || xx == bx + bw - 1 || yy == by;
                    if border {
                        c.set(xx, yy, hex(0xe8fbff));
                    } else if crate::render::canvas::bayer(xx, yy) < 0.2 {
                        c.set(xx, yy, hex(0xbfeeff));
                    }
                }
            }
            for k in 0..5 {
                c.set(bx + 2 + k, by + 7 - k, hex(0xffffff));
            }
        }
        // Warmth meter above his head.
        if !frozen {
            let w = 12;
            let fill = (self.warmth * w as f32).round() as i32;
            let col = if self.warmth > 0.5 {
                hex(0xffb03a)
            } else if self.warmth > 0.25 {
                hex(0xff7a2a)
            } else {
                hex(0x8ad8f5)
            };
            c.rect(x - 1, body_top - 5, w + 2, 3, hex(0x1b1942));
            c.rect(x, body_top - 4, fill, 1, col);
        }
        if let Some(b) = &self.bubble {
            draw_bubble(c, x + WIDTH / 2, body_top - 8, &b.text);
        }
    }
}

/// Speech bubble with a tail pointing down at (anchor_x, bottom_y).
pub fn draw_bubble(c: &mut Canvas, anchor_x: i32, bottom_y: i32, text: &str) {
    let lines = font::wrap(text, 120.min(c.w - 12));
    let tw = lines.iter().map(|l| font::text_width(l)).max().unwrap_or(0);
    let (pw, ph) = (tw + 8, lines.len() as i32 * (font::LINE_H - 2) + 6);
    let px = (anchor_x - pw / 2).clamp(2, (c.w - pw - 2).max(2));
    let py = (bottom_y - ph - 3).max(2);
    let (ink, paper) = (hex(0x1b1942), hex(0xf4f7ff));
    c.rect(px + 1, py, pw - 2, ph, paper);
    c.rect(px, py + 1, pw, ph - 2, paper);
    c.rect(px + 1, py - 1, pw - 2, 1, ink);
    c.rect(px + 1, py + ph, pw - 2, 1, ink);
    c.rect(px - 1, py + 1, 1, ph - 2, ink);
    c.rect(px + pw, py + 1, 1, ph - 2, ink);
    c.set(px, py, ink);
    c.set(px + pw - 1, py, ink);
    c.set(px, py + ph - 1, ink);
    c.set(px + pw - 1, py + ph - 1, ink);
    let tail_x = anchor_x.clamp(px + 3, px + pw - 4);
    c.rect(tail_x, py + ph, 3, 1, paper);
    c.rect(tail_x + 1, py + ph + 1, 1, 1, paper);
    c.set(tail_x - 1, py + ph, ink);
    c.set(tail_x + 3, py + ph, ink);
    c.set(tail_x, py + ph + 1, ink);
    c.set(tail_x + 2, py + ph + 1, ink);
    c.set(tail_x + 1, py + ph + 2, ink);
    for (i, l) in lines.iter().enumerate() {
        font::draw(c, px + 4, py + 1 + i as i32 * (font::LINE_H - 2) - 1, l, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(w: &mut Warrior, seconds: f32, freeze: f32, near_fire: bool) -> Vec<Event> {
        let mut out = Vec::new();
        let dt = 1.0 / 30.0;
        for i in 0..(seconds / dt) as i32 {
            if let Some(e) = w.step(dt, freeze, near_fire, 0.0, 300.0, (i % 7) as f32 / 7.0) {
                out.push(e);
            }
        }
        out
    }

    #[test]
    fn a_frozen_screen_freezes_him_but_a_clear_one_does_not() {
        let mut cold = Warrior::new(100.0);
        run(&mut cold, 60.0, 1.0, false);
        assert_eq!(cold.act, Act::Frozen);

        let mut fine = Warrior::new(100.0);
        run(&mut fine, 60.0, 0.0, false);
        assert_ne!(fine.act, Act::Frozen);
    }

    #[test]
    fn the_fire_keeps_him_alive() {
        let mut w = Warrior::new(100.0);
        run(&mut w, 120.0, 1.0, true);
        assert_ne!(w.act, Act::Frozen);
    }

    #[test]
    fn building_lights_a_fire_next_to_him() {
        let mut w = Warrior::new(100.0);
        w.start_building();
        let events = run(&mut w, 3.0, 0.0, false);
        let lit: Vec<f32> = events
            .iter()
            .map(|e| match e {
                Event::FireLit { x } => *x,
            })
            .collect();
        assert_eq!(lit.len(), 1);
        assert!((lit[0] - 106.0).abs() <= 13.0);
    }

    #[test]
    fn only_a_correct_answer_unfreezes_him() {
        let mut w = Warrior::new(100.0);
        run(&mut w, 60.0, 1.0, false);
        assert_eq!(w.act, Act::Frozen);
        run(&mut w, 30.0, 0.0, true); // even a fire can't thaw a statue
        assert_eq!(w.act, Act::Frozen);
        w.warm_burst();
        assert_eq!(w.act, Act::Cheer);
        assert_eq!(w.warmth, 1.0);
    }

    #[test]
    fn frozen_warriors_do_not_talk() {
        let mut w = Warrior::new(100.0);
        run(&mut w, 60.0, 1.0, false);
        w.say("oi", 3.0);
        assert!(w.bubble.is_none());
    }
}
