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
/// Crouched on his skis, sliding down a pile (#117): the skis stick out past
/// his boots and the front tip turns up (drawn with the legs).
const LEGS_SKI: &[&str] = &[".PPPP..PPPP.", ".KKKK..KKKK.", "RRRRRRRRRRRR"];
/// Skis stick out this far past the sprite in front (px).
const SKI_TIP: i32 = 2;

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
    /// Sword out, going after a mob.
    Fight,
    /// Fell at the bottom of a ski run: rolls over, lies there, gets up (#117).
    Tumble,
}

pub enum Event {
    FireLit {
        x: f32,
    },
    /// Sword swing landing around `x`.
    Strike {
        x: f32,
    },
    /// The arrow leaves the bow at `x`, `y` px above his feet (#118).
    Shoot {
        x: f32,
        y: f32,
    },
}

#[derive(Clone)]
pub struct Bubble {
    pub text: String,
    pub left: f32,
}

#[derive(Clone)]
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
    /// Center x of the mob he should fight, set by the scene each frame.
    pub foe: Option<f32>,
    swing_cd: f32,
    swing_t: f32,
    /// Time left on the finishing strike's arc (#113).
    finish_t: f32,
    /// Time left with the bow out (#118); the arrow leaves at `LOOSE_AT`.
    bow_t: f32,
    /// Sliding down a steep pile (set by the scene): crouched, no steps.
    pub sliding: bool,
}

const BUILD_TIME: f32 = 2.5;
/// A tumble: rolling (s), then lying flat until `TUMBLE_LIE`, then up at `TUMBLE_S`.
const TUMBLE_ROLL: f32 = 0.6;
const TUMBLE_LIE: f32 = 1.3;
pub const TUMBLE_S: f32 = 1.6;
/// Rolling speed at the start of a tumble (px/s), slowing to a stop.
const TUMBLE_SPEED: f32 = 22.0;
/// How long the finishing strike's arc stays on screen (s).
pub const FINISH_SHOW: f32 = 0.45;
/// How long the bow is out on a right answer (s), and the time left on it
/// when the arrow leaves: a short draw, then the bow lingers after the shot.
pub const BOW_SHOW: f32 = 0.6;
const LOOSE_AT: f32 = 0.4;
/// The bow hand, in px above his feet.
const BOW_HAND: f32 = 7.0;

impl Warrior {
    pub fn new(x: f32) -> Self {
        Warrior {
            x,
            dir: -1.0,
            act: Act::Wander,
            t: 0.0,
            warmth: 1.0,
            walk_t: 0.0,
            target: x,
            bubble: None,
            foe: None,
            swing_cd: 0.0,
            swing_t: 0.0,
            finish_t: 0.0,
            bow_t: 0.0,
            sliding: false,
        }
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

    /// Only ages the speech bubble (while held by the magic hand).
    pub fn tick_bubble(&mut self, dt: f32) {
        if let Some(b) = &mut self.bubble {
            b.left -= dt;
            if b.left <= 0.0 {
                self.bubble = None;
            }
        }
    }

    pub fn start_building(&mut self) {
        if matches!(self.act, Act::Wander) {
            self.set(Act::Build);
        }
    }

    /// Falls at the end of a ski run, when he was only wandering (#117).
    pub fn tumble(&mut self) {
        if self.act == Act::Wander {
            self.set(Act::Tumble);
        }
    }

    /// Cheers for a phrase you just learned (#119): a hop, no warmth for free.
    pub fn cheer(&mut self) {
        if self.act != Act::Frozen {
            self.set(Act::Cheer);
        }
    }

    /// Your correct answer reached him.
    pub fn warm_burst(&mut self) {
        self.warmth = 1.0;
        self.set(Act::Cheer);
    }

    /// A right answer lets him finish the mob at `foe_x` (#113): he faces it
    /// and a wide arc of the sword shows for `FINISH_SHOW`.
    pub fn finish(&mut self, foe_x: f32) {
        self.dir = if foe_x >= self.x + WIDTH as f32 / 2.0 { 1.0 } else { -1.0 };
        self.finish_t = FINISH_SHOW;
    }

    /// A right answer lets him shoot the bat at `bat_x` (#118): he faces it
    /// and draws the bow; `Event::Shoot` follows when the arrow leaves.
    pub fn shoot(&mut self, bat_x: f32) {
        self.dir = if bat_x >= self.x + WIDTH as f32 / 2.0 { 1.0 } else { -1.0 };
        self.bow_t = BOW_SHOW;
    }

    /// The bow is out.
    pub fn aiming(&self) -> bool {
        self.bow_t > 0.0
    }

    /// The finishing strike's arc is on screen.
    pub fn finishing(&self) -> bool {
        self.finish_t > 0.0
    }

    /// `freeze` 0..1 = how frozen the screen is; `near_fire` = sitting range.
    pub fn step(&mut self, dt: f32, freeze: f32, near_fire: bool, min_x: f32, max_x: f32, roll: f32) -> Option<Event> {
        self.t += dt;
        self.finish_t = (self.finish_t - dt).max(0.0);
        let drawn = self.bow_t;
        self.bow_t = (self.bow_t - dt).max(0.0);
        if drawn > LOOSE_AT && self.bow_t <= LOOSE_AT {
            self.tick_bubble(dt);
            let x = self.x + WIDTH as f32 / 2.0 + self.dir * (WIDTH as f32 / 2.0 + 2.0);
            return Some(Event::Shoot { x, y: BOW_HAND });
        }
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

        self.swing_cd -= dt;
        self.swing_t = (self.swing_t - dt).max(0.0);
        if self.foe.is_some() && matches!(self.act, Act::Wander | Act::Warm) {
            self.set(Act::Fight);
        }
        match self.act {
            Act::Fight => {
                let Some(foe) = self.foe else {
                    self.set(Act::Wander);
                    return None;
                };
                let center = self.x + WIDTH as f32 / 2.0;
                let dx = foe - center;
                self.dir = if dx >= 0.0 { 1.0 } else { -1.0 };
                if dx.abs() > 9.0 {
                    self.x = (self.x + self.dir * 12.0 * dt).clamp(min_x, max_x);
                    self.walk_t += dt;
                    None
                } else if self.swing_cd <= 0.0 {
                    self.swing_cd = 0.6;
                    self.swing_t = 0.25;
                    Some(Event::Strike { x: center + self.dir * 8.0 })
                } else {
                    None
                }
            }
            Act::Wander if self.bow_t > 0.0 => None, // stands still to shoot
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
            Act::Tumble => {
                let roll = (1.0 - self.t / TUMBLE_ROLL).max(0.0);
                self.x = (self.x + self.dir * TUMBLE_SPEED * roll * dt).clamp(min_x, max_x);
                if self.t > TUMBLE_S {
                    self.set(Act::Wander);
                }
                None
            }
            Act::Cheer if self.t > 1.6 => {
                self.set(Act::Wander);
                None
            }
            _ => None,
        }
    }

    /// Draws as if standing at `x` (dangling from the magic hand).
    pub fn draw_at(&self, c: &mut Canvas, x: f32, feet_y: f32, time: f32) {
        let mut shown = self.clone();
        shown.x = x;
        shown.draw(c, feet_y, time);
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
        } else if self.sliding {
            LEGS_SKI
        } else if self.act == Act::Wander && (self.walk_t * 5.0) as i32 % 2 == 1 {
            LEGS_STEP
        } else {
            LEGS_STAND
        };
        let body_top = feet_y.round() as i32 - UPPER.len() as i32 - legs.len() as i32 + hop;
        if let Some(turns) = self.tumble_turns() {
            draw_turned(c, pal, x, feet_y.round() as i32, turns, flip);
        } else {
            c.sprite(UPPER, pal, x, body_top, flip);
            c.sprite(legs, pal, x, body_top + UPPER.len() as i32, flip);
        }
        if self.sliding {
            // The skis run on in front, tip turned up.
            let ski = hex(0xc8561e);
            let front = if flip { x - SKI_TIP } else { x + WIDTH };
            let y = feet_y.round() as i32 - 1;
            c.rect(front, y, SKI_TIP, 1, ski);
            c.set(if flip { front - 1 } else { front + SKI_TIP }, y - 1, ski);
        }

        if self.act == Act::Fight {
            // Sword: raised between swings, slashing forward on a strike.
            let hand_x = if flip { x + 1 } else { x + WIDTH - 2 };
            let hand_y = body_top + 9;
            let dir = if flip { -1 } else { 1 };
            let (blade, hilt) = (hex(0xdfe6f0), hex(0xffd64a));
            c.set(hand_x, hand_y, hilt);
            if self.swing_t > 0.0 {
                for k in 1..=7 {
                    c.set(hand_x + dir * k, hand_y, blade);
                }
                for k in 0..5 {
                    c.set(hand_x + dir * (3 + k), hand_y - 4 + k, hex(0x9be8ff));
                }
            } else {
                for k in 1..=6 {
                    c.set(hand_x, hand_y - k, blade);
                }
                c.set(hand_x - 1, hand_y - 1, hilt);
                c.set(hand_x + 1, hand_y - 1, hilt);
            }
        }
        if self.finish_t > 0.0 {
            // Finishing strike (#113): a wide arc from overhead down to the
            // front, a solid edge with a dithered trail that thins as it fades.
            let dir = if flip { -1.0 } else { 1.0 };
            let (cx, cy) = (x as f32 + WIDTH as f32 / 2.0, body_top as f32 + 8.0);
            let fade = self.finish_t / FINISH_SHOW;
            for i in 0..=48 {
                let a = -1.9 + 2.5 * i as f32 / 48.0;
                for r in [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0] {
                    let (px, py) = ((cx + dir * a.cos() * r).round() as i32, (cy + a.sin() * r).round() as i32);
                    if r >= 16.0 {
                        c.set(px, py, hex(0xffffff));
                    } else if crate::render::canvas::bayer(px, py) < fade * (r - 10.0) / 6.0 {
                        c.set(px, py, hex(0x9be8ff));
                    }
                }
            }
        }
        if self.bow_t > 0.0 {
            self.draw_bow(c, x, feet_y.round() as i32 + hop, flip);
        }
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

impl Warrior {
    /// The bow held out in front: a curved stave and its string, with the
    /// arrow nocked until it leaves.
    fn draw_bow(&self, c: &mut Canvas, x: i32, feet_y: i32, flip: bool) {
        let dir = if flip { -1 } else { 1 };
        let hand = if flip { x - 2 } else { x + WIDTH + 1 };
        let y = feet_y - BOW_HAND as i32;
        let (wood, string) = (hex(0x8b5a2b), hex(0xdfe6f0));
        for j in -4..=4_i32 {
            let bend = match j.abs() {
                0 | 1 => 1,
                2 | 3 => 0,
                _ => -1,
            };
            c.set(hand + dir * bend, y + j, wood);
        }
        let pull = if self.bow_t > LOOSE_AT { 2 } else { 0 };
        for j in -3..=3_i32 {
            let back = pull * (3 - j.abs()) / 3;
            c.set(hand - dir * (1 + back), y + j, string);
        }
        if self.bow_t > LOOSE_AT {
            for k in 0..5 {
                let color = if k == 4 { hex(0xffffff) } else { wood };
                c.set(hand - dir * (1 + pull) + dir * k, y, color);
            }
        }
    }

    /// While tumbling, how many quarter turns the whole body is drawn at:
    /// rolling over and over, then flat on his back; None once he is up.
    fn tumble_turns(&self) -> Option<u8> {
        match self.act {
            Act::Tumble if self.t < TUMBLE_ROLL => Some(1 + (self.t / TUMBLE_ROLL * 6.0) as u8 % 4),
            Act::Tumble if self.t < TUMBLE_LIE => Some(1),
            _ => None,
        }
    }
}

/// The standing warrior turned `turns` quarter turns forward, resting on the
/// ground at `feet_y`, centered where he stands at `x`.
fn draw_turned(c: &mut Canvas, pal: &[(char, Rgba)], x: i32, feet_y: i32, turns: u8, flip: bool) {
    let rows: Vec<&str> = UPPER.iter().chain(LEGS_STAND).copied().collect();
    let (h, w) = (rows.len() as i32, WIDTH);
    let (tw, th) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let (left, top) = (x + w / 2 - tw / 2, feet_y - th);
    for (r, line) in rows.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let Some(&(_, color)) = pal.iter().find(|(k, _)| *k == ch) else { continue };
            let (r, col) = (r as i32, if flip { w - 1 - col as i32 } else { col as i32 });
            let (tx, ty) = match turns % 4 {
                0 => (col, r),
                1 => (h - 1 - r, col),
                2 => (w - 1 - col, h - 1 - r),
                _ => (r, w - 1 - col),
            };
            let tx = if flip { tw - 1 - tx } else { tx };
            c.set(left + tx, top + ty, color);
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
            .filter_map(|e| match e {
                Event::FireLit { x } => Some(*x),
                Event::Strike { .. } | Event::Shoot { .. } => None,
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
    fn draws_his_sword_goes_to_the_foe_and_strikes_repeatedly() {
        let mut w = Warrior::new(100.0);
        w.foe = Some(160.0);
        let strikes = run(&mut w, 8.0, 0.0, false).iter().filter(|e| matches!(e, Event::Strike { .. })).count();
        assert!(w.x > 130.0, "walked to the mob");
        assert!(strikes >= 5, "strikes {strikes}");
        w.foe = None;
        run(&mut w, 0.2, 0.0, false);
        assert_eq!(w.act, Act::Wander, "sheathes the sword when the fight is over");
    }

    #[test]
    fn the_finishing_arc_faces_the_foe_is_one_bit_and_fades_out() {
        let mut w = Warrior::new(100.0);
        w.dir = -1.0;
        w.finish(130.0);
        assert_eq!(w.dir, 1.0, "turns to the mob");
        let draw = |w: &Warrior| {
            let mut c = Canvas::new(160, 60);
            w.draw(&mut c, 50.0, 0.0);
            c.bytes().to_vec()
        };
        let with_arc = draw(&w);
        assert!(with_arc.chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
        fn ahead(px: &[u8]) -> usize {
            (0..60 * 160).filter(|i| (117..126).contains(&(i % 160)) && px[i * 4 + 3] == 255).count()
        }
        assert!(ahead(&with_arc) >= 10, "the arc is drawn in front of him");
        run(&mut w, FINISH_SHOW + 0.05, 0.0, false);
        assert!(!w.finishing());
        assert_eq!(ahead(&draw(&w)), 0, "gone once it fades");
    }

    #[test]
    fn frozen_warriors_do_not_talk() {
        let mut w = Warrior::new(100.0);
        run(&mut w, 60.0, 1.0, false);
        w.say("oi", 3.0);
        assert!(w.bubble.is_none());
    }

    #[test]
    fn sliding_is_its_own_pose_without_a_walk_cycle() {
        let draw = |w: &Warrior| {
            let mut c = Canvas::new(40, 40);
            w.draw(&mut c, 30.0, 0.0);
            c.bytes().to_vec()
        };
        let mut w = Warrior::new(10.0);
        let walking: Vec<Vec<u8>> = [0.1, 0.3]
            .iter()
            .map(|t| {
                w.walk_t = *t;
                draw(&w)
            })
            .collect();
        assert_ne!(walking[0], walking[1], "walking moves the legs");
        w.sliding = true;
        let sliding: Vec<Vec<u8>> = [0.1, 0.3]
            .iter()
            .map(|t| {
                w.walk_t = *t;
                draw(&w)
            })
            .collect();
        assert_eq!(sliding[0], sliding[1], "no steps while sliding");
        assert!(!walking.contains(&sliding[0]), "a pose of its own");
    }

    #[test]
    fn a_tumble_rolls_lies_down_and_gets_up_in_one_bit() {
        let mut w = Warrior::new(50.0);
        w.tumble();
        assert_eq!(w.act, Act::Tumble);
        let mut poses = Vec::new();
        for _ in 0..(TUMBLE_S * 30.0) as i32 + 2 {
            let mut c = Canvas::new(80, 40);
            w.draw(&mut c, 34.0, 0.0);
            assert!(c.bytes().chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
            let px = c.bytes().to_vec();
            if !poses.contains(&px) {
                poses.push(px);
            }
            w.step(1.0 / 30.0, 0.0, false, 0.0, 200.0, 0.5);
        }
        assert!(poses.len() >= 4, "rolls through several poses: {}", poses.len());
        assert_eq!(w.act, Act::Wander, "back on his feet");
        w.set(Act::Fight);
        w.tumble();
        assert_eq!(w.act, Act::Fight, "only a wandering warrior falls");
    }

    #[test]
    fn on_skis_the_tip_runs_past_his_boots() {
        let mut w = Warrior::new(20.0);
        w.dir = 1.0;
        w.sliding = true;
        let mut c = Canvas::new(60, 40);
        w.draw(&mut c, 30.0, 0.0);
        assert!(c.bytes().chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
        assert!(c.opaque_in(20 + WIDTH, 28, SKI_TIP + 1, 2) >= 2, "the ski sticks out in front");
    }

    #[test]
    fn the_bow_comes_out_looses_one_arrow_and_is_drawn_in_one_bit() {
        let mut w = Warrior::new(50.0);
        w.shoot(10.0);
        assert!(w.aiming() && w.dir < 0.0, "faces the bat");
        let mut c = Canvas::new(80, 40);
        w.draw(&mut c, 34.0, 0.0);
        assert!(c.bytes().chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
        assert!(c.opaque_in(44, 20, 6, 14) >= 9, "the bow in front of him");
        let shots: Vec<(f32, f32)> = (0..30)
            .filter_map(|_| match w.step(1.0 / 30.0, 0.0, false, 0.0, 200.0, 0.5) {
                Some(Event::Shoot { x, y }) => Some((x, y)),
                _ => None,
            })
            .collect();
        assert_eq!(shots.len(), 1, "one arrow");
        assert!(shots[0].0 < 50.0, "from the bow, in front: {:?}", shots[0]);
        assert!(!w.aiming(), "the bow goes away");
    }
}
