//! The comet: the reward for `COMET_COMBO` right answers in a row (#114). It
//! falls on the frost mage with a dithered tail (#135), melts `COMET_MELT` of
//! the snow on impact and leaves little fire creatures (`Ember`) that run
//! across the screen at random, melting the snow where they pass, from a
//! shared budget of `EMBER_BUDGET` of the snow left. On bare ground they melt
//! nothing; what a creature has not spent when it fades is lost. Altogether
//! the comet never melts more than the sun.

use super::rng::Rng;
use super::snow::Snow;
use crate::render::canvas::{Canvas, bayer, hex};

/// Share of the snow the impact melts at once.
pub const COMET_MELT: f32 = 0.25;
/// Share of the snow left after the impact that all the rocks together may melt.
pub const EMBER_BUDGET: f32 = 0.37;
/// How long a creature runs around (s).
pub const EMBER_LIFE: f32 = 12.0;
/// Creatures a comet leaves, at most.
pub const MAX_EMBERS: usize = 6;

/// Seconds from the top of the screen to the pile.
const FALL_S: f32 = 1.2;
/// How far from the impact the creatures land (px).
const SCATTER: f32 = 28.0;
/// A creature melts up to this far around itself (px).
const EMBER_RADIUS: f32 = 6.0;
/// A creature on deep snow spends its share in about this long (s).
const EMBER_SPEND_S: f32 = 8.0;
/// A creature's running speed range (px/s).
const EMBER_SPEED: (f32, f32) = (14.0, 26.0);
/// Seconds between a creature's chances to turn around.
const EMBER_TURN: (f32, f32) = (0.6, 2.0);
/// Its last seconds, while it cools and fades out.
const EMBER_FADE_S: f32 = 1.5;
/// Tail length, in px along the fall.
const TAIL: i32 = 44;

/// A fire creature, 7×7, facing right, in two running frames: a flame body
/// with eyes and little dark feet.
const RUN_A: &[&str] = &["...H...", "..HWH..", ".HWYWH.", "HWYEYEW", "RWYYYWR", ".RRRRR.", ".D...D."];
const RUN_B: &[&str] = &["...H...", "..HWH..", ".HWYWH.", "HWYEYEW", "RWYYYWR", ".RRRRR.", "..D.D.."];

/// A little fire creature the comet left: it runs across the screen.
pub struct Ember {
    pub x: f32,
    /// -1 = running left, 1 = right.
    pub dir: f32,
    speed: f32,
    turn_in: f32,
    budget: f32,
    rate: f32,
    life: f32,
}

pub struct Comet {
    from_x: f32,
    to_x: f32,
    t: f32,
    landed: bool,
    pub embers: Vec<Ember>,
}

/// The comet reached the pile at `x`.
pub struct Impact {
    pub x: f32,
}

impl Comet {
    /// A comet aimed at `to_x`, coming in from the side farther from it.
    pub fn new(to_x: f32, width: f32) -> Self {
        let from_x = if to_x < width / 2.0 { to_x + width * 0.45 } else { to_x - width * 0.45 };
        Comet { from_x, to_x, t: 0.0, landed: false, embers: Vec::new() }
    }

    pub fn falling(&self) -> bool {
        !self.landed
    }

    /// Keeps the comet on `x` while it falls (the frost mage walks, #135).
    pub fn aim(&mut self, x: f32) {
        if !self.landed {
            self.to_x = x;
        }
    }

    /// Where it lands, or landed.
    pub fn target(&self) -> f32 {
        self.to_x
    }

    /// Landed and every rock has faded.
    pub fn done(&self) -> bool {
        self.landed && self.embers.is_empty()
    }

    /// Head position while falling, from above the screen to the pile's surface
    /// at `surface_y`: it speeds up as it falls.
    fn head(&self, surface_y: f32) -> (f32, f32) {
        let k = (self.t / FALL_S).min(1.0);
        let k = k * k;
        (self.from_x + (self.to_x - self.from_x) * k, -12.0 + (surface_y + 12.0) * k)
    }

    pub fn step(&mut self, dt: f32, snow: &mut Snow, width: f32, rng: &mut Rng) -> Option<Impact> {
        if !self.landed {
            self.t += dt;
            if self.t < FALL_S {
                return None;
            }
            self.landed = true;
            snow.melt(COMET_MELT);
            let n = 4 + (rng.f32() * 3.0) as usize;
            let share = EMBER_BUDGET * snow.total() / n as f32;
            for _ in 0..n.min(MAX_EMBERS) {
                let x = (self.to_x + rng.range(-SCATTER, SCATTER)).clamp(2.0, width - 3.0);
                let rate = share / (EMBER_SPEND_S * EMBER_RADIUS);
                let dir = if rng.chance(0.5) { 1.0 } else { -1.0 };
                let (speed, turn_in) = (rng.range(EMBER_SPEED.0, EMBER_SPEED.1), rng.range(EMBER_TURN.0, EMBER_TURN.1));
                let life = EMBER_LIFE * rng.range(0.9, 1.0);
                self.embers.push(Ember { x, dir, speed, turn_in, budget: share, rate, life });
            }
            return Some(Impact { x: self.to_x });
        }
        for e in &mut self.embers {
            e.life -= dt;
            e.turn_in -= dt;
            if e.turn_in <= 0.0 {
                e.turn_in = rng.range(EMBER_TURN.0, EMBER_TURN.1);
                if rng.chance(0.5) {
                    e.dir = -e.dir;
                }
            }
            e.x += e.dir * e.speed * dt;
            let (lo, hi) = (2.0, width - 3.0);
            if e.x < lo || e.x > hi {
                e.x = e.x.clamp(lo, hi);
                e.dir = -e.dir; // bumps the edge and runs back
            }
            e.budget -= snow.burn(e.x, EMBER_RADIUS, e.rate, dt, e.budget);
        }
        self.embers.retain(|e| e.life > 0.0);
        None
    }

    /// `ground_y` = the bottom of the pile; the rocks sit on the snow's surface.
    pub fn draw(&self, c: &mut Canvas, ground_y: f32, snow: &Snow, time: f32) {
        if !self.landed {
            let (hx, hy) = self.head(ground_y - snow.height_at(self.to_x));
            let (dx, dy) = (self.to_x - self.from_x, ground_y + 12.0);
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            let (ux, uy) = (dx / len, dy / len);
            for k in 1..=TAIL {
                let f = k as f32 / TAIL as f32;
                // Wide at the head, thinning toward the end.
                let half = ((1.0 - f) * 3.5).round() as i32;
                for side in -half..=half {
                    let (px, py) = (hx - ux * k as f32 - uy * side as f32, hy - uy * k as f32 + ux * side as f32);
                    let (px, py) = (px.round() as i32, py.round() as i32);
                    let edge = 1.0 - side.abs() as f32 / (half as f32 + 1.0);
                    if bayer(px, py) < (1.0 - f) * edge {
                        c.set(px, py, hex([0xfff4b0, 0xffc13d, 0xff7a2a, 0xd93a2a][(f * 3.99) as usize]));
                    }
                }
            }
            c.glow(hx, hy, 12.0, 1.0, hex(0xffc13d));
            let (x, y) = (hx.round() as i32, hy.round() as i32);
            for oy in -3..=3 {
                for ox in -3..=3 {
                    let d = ox * ox + oy * oy;
                    if d <= 10 {
                        let rgb = match d {
                            0..=2 => 0xffffff,
                            3..=5 => 0xfff4b0,
                            _ => 0xffc13d,
                        };
                        c.set(x + ox, y + oy, hex(rgb));
                    }
                }
            }
            return;
        }
        for (i, e) in self.embers.iter().enumerate() {
            let x = e.x.round() as i32;
            let y = (ground_y - snow.height_at(e.x)).round() as i32;
            let heat = (e.life / EMBER_FADE_S).min(1.0);
            let flick = ((time * 9.0 + i as f32 * 1.3).sin() * 0.5 + 0.5) > 0.5;
            let (hot, warm) = if flick { (0xfff4b0, 0xffc13d) } else { (0xffc13d, 0xff7a2a) };
            // Its feet swap as it runs; it faces where it goes.
            let frame = if (e.x / 3.0) as i32 % 2 == 0 { RUN_A } else { RUN_B };
            let flip = e.dir < 0.0;
            for (row, line) in frame.iter().enumerate() {
                for (col, ch) in line.chars().enumerate() {
                    let rgb = match ch {
                        'H' => hot,
                        'W' => warm,
                        'Y' => 0xffd64a,
                        'R' => 0xd93a2a,
                        'E' => 0x1b1942,
                        'D' => 0x4a2216,
                        _ => continue,
                    };
                    let col = if flip { 6 - col as i32 } else { col as i32 };
                    let (px, py) = (x - 3 + col, y - frame.len() as i32 + row as i32);
                    if bayer(px, py) < heat {
                        c.set(px, py, hex(rgb));
                    }
                }
            }
            if heat >= 1.0 {
                c.glow(e.x, y as f32 - 4.0, 5.0, 0.6, hex(warm));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pile() -> Snow {
        let mut s = Snow::new(200, 60.0);
        s.add(100.0, 30.0, 200.0);
        s
    }

    fn run(c: &mut Comet, snow: &mut Snow, seconds: f32) -> Option<Impact> {
        let mut rng = Rng::new(7);
        let mut hit = None;
        for _ in 0..(seconds * 30.0) as i32 {
            hit = c.step(1.0 / 30.0, snow, 200.0, &mut rng).or(hit);
        }
        hit
    }

    #[test]
    fn it_falls_then_lands_melting_its_share_and_leaving_rocks() {
        let mut snow = pile();
        let before = snow.total();
        let mut c = Comet::new(100.0, 200.0);
        assert!(run(&mut c, &mut snow, 1.0).is_none(), "still falling");
        assert!((snow.total() - before).abs() < 1e-3, "nothing melts mid-air");
        assert!(run(&mut c, &mut snow, 0.3).is_some());
        assert!(!c.falling());
        assert!((4..=MAX_EMBERS).contains(&c.embers.len()));
        assert!(c.embers.iter().all(|e| (e.x - 100.0).abs() <= SCATTER));
        assert!(snow.total() <= before * (1.0 - COMET_MELT) + 1e-2);
    }

    #[test]
    fn its_total_melt_is_at_most_the_suns() {
        let mut snow = pile();
        let before = snow.total();
        let mut c = Comet::new(100.0, 200.0);
        run(&mut c, &mut snow, 20.0);
        assert!(c.done());
        let comet = 1.0 - snow.total() / before;
        let sun = super::super::sun_melt_share();
        assert!(comet <= sun + 1e-4, "comet {comet} > sun {sun}");
        assert!(comet > COMET_MELT + 0.1, "the rocks melt something: {comet}");
    }

    #[test]
    fn after_the_last_rock_fades_the_snow_never_drops_again() {
        let mut snow = pile();
        let mut c = Comet::new(100.0, 200.0);
        run(&mut c, &mut snow, FALL_S + EMBER_LIFE + 0.5);
        assert!(c.done());
        let after = snow.total();
        run(&mut c, &mut snow, 10.0);
        assert_eq!(snow.total(), after);
    }

    #[test]
    fn a_creature_on_bare_ground_melts_nothing() {
        let mut snow = Snow::new(200, 60.0);
        snow.add(5.0, 30.0, 1.0); // snow far from where they land and can run in 2 s
        let before = snow.total();
        let mut c = Comet::new(160.0, 200.0);
        run(&mut c, &mut snow, 1.3);
        let landed = snow.total();
        assert!(!c.embers.is_empty());
        run(&mut c, &mut snow, 2.0);
        assert_eq!(snow.total(), landed, "they run on bare ground");
        assert!(landed <= before);
    }

    #[test]
    fn the_creatures_run_about_stay_on_screen_and_repeat_for_a_seed() {
        let path = || {
            let (mut snow, mut c) = (pile(), Comet::new(100.0, 200.0));
            let mut rng = Rng::new(3);
            let mut xs = Vec::new();
            for _ in 0..(11.0 * 30.0) as i32 {
                c.step(1.0 / 30.0, &mut snow, 200.0, &mut rng);
                xs.push(c.embers.iter().map(|e| e.x).collect::<Vec<f32>>());
            }
            xs
        };
        let xs = path();
        assert_eq!(xs, path(), "deterministic for a seed");
        let (first, last) = (&xs[40], &xs[xs.len() - 1]);
        assert!(!first.is_empty() && first.len() == last.len());
        assert!(first.iter().zip(last).any(|(a, b)| (a - b).abs() > 20.0), "they run about");
        assert!(xs.iter().flatten().all(|x| (2.0..=197.0).contains(x)), "never off screen");
    }

    #[test]
    fn it_follows_its_target_only_while_falling() {
        let mut snow = pile();
        let mut c = Comet::new(100.0, 200.0);
        c.aim(60.0);
        assert_eq!(c.target(), 60.0);
        run(&mut c, &mut snow, 1.3);
        c.aim(150.0);
        assert_eq!(c.target(), 60.0, "landed: it stays");
    }

    #[test]
    fn it_draws_the_fall_and_the_rocks_in_one_bit() {
        let mut snow = pile();
        let mut c = Comet::new(100.0, 200.0);
        let shot = |c: &Comet, snow: &Snow| {
            let mut canvas = Canvas::new(200, 120);
            c.draw(&mut canvas, 120.0, snow, 0.0);
            canvas.bytes().to_vec()
        };
        run(&mut c, &mut snow, 0.6);
        let falling = shot(&c, &snow);
        run(&mut c, &mut snow, 1.0);
        let rocks = shot(&c, &snow);
        for px in [&falling, &rocks] {
            assert!(px.chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
            assert!(px.chunks(4).filter(|p| p[3] == 255).count() >= 10, "something drawn");
        }
    }
}
