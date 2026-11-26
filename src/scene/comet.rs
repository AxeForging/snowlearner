//! The comet: the reward for `COMET_COMBO` right answers in a row (#114). It
//! falls on the frost mage with a dithered tail (#135), melts `COMET_MELT` of
//! the snow on impact and leaves little fire people (`Ember`, #139) that dash
//! to the snow arms flailing, melt it, then dash to the next patch, from a
//! shared budget of `EMBER_BUDGET` of the snow left. On bare ground they melt
//! nothing; what one has not spent when it fades is lost. Altogether the comet
//! never melts more than the sun.

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
/// The pull on a thrown creature (px/s², as the scene's cubes).
const THROW_GRAVITY: f32 = 130.0;
/// A throw lasts this long, plus `THROW_PER_WIDTH` for a throw across the
/// whole screen (s): farther throws fly longer and higher.
const THROW_S: f32 = 0.6;
const THROW_PER_WIDTH: f32 = 0.8;
/// A creature melts up to this far around itself (px): a trench, not a hole (#182).
const EMBER_RADIUS: f32 = 9.0;
/// A creature on deep snow spends its share in about this long (s).
const EMBER_SPEND_S: f32 = 8.0;
/// A fire person's running speed range (px/s): a dash, not a stroll.
const EMBER_SPEED: (f32, f32) = (32.0, 46.0);
/// Its pace while it digs across a patch, as a share of its dash: slow enough
/// that one pass takes off a real layer (#182: ~19 px of a pile at the line,
/// was ~7 px at full speed).
const EMBER_DIG_PACE: f32 = 0.35;
/// At most this long on one patch before dashing to another (s).
const EMBER_RETARGET: (f32, f32) = (2.0, 3.0);
/// How far it runs across a patch it reached (px).
const EMBER_ACROSS: (f32, f32) = (8.0, 20.0);
/// Snow counts as worth running to from this height (px).
const SNOW_WORTH: f32 = 1.0;
/// How much a farther patch must be deeper to be chosen (px of snow per px away).
const DISTANCE_COST: f32 = 0.08;
/// A hop: its length (s), height (px) and the pause between hops (s).
const HOP_S: f32 = 0.35;
const HOP_H: f32 = 4.0;
const HOP_EVERY: (f32, f32) = (1.5, 3.5);
/// Its last seconds, while it cools and fades out.
const EMBER_FADE_S: f32 = 1.5;
/// Tail length, in px along the fall.
const TAIL: i32 = 44;

/// A fire person, 9×11, facing right: flame head with eyes and a body. Arms
/// and legs are drawn on top in two poses each.
#[rustfmt::skip] // kept as a picture
const PERSON: &[&str] = &[
    "....H....",
    "...HWH...",
    "..HWYWH..",
    "..WEYEW..",
    "..WYYYW..",
    "..RWYWR..",
    "..RYYYR..",
    "...RRR...",
];
/// Arms flung up, or swung down (column, row).
const ARMS_UP: [(i32, i32); 4] = [(1, 4), (0, 3), (7, 4), (8, 3)];
const ARMS_DOWN: [(i32, i32); 4] = [(1, 6), (0, 7), (7, 6), (8, 7)];
/// Legs in stride, or passing (rows 8..=10 under the body).
const LEGS_APART: [(i32, i32); 6] = [(3, 8), (5, 8), (2, 9), (6, 9), (1, 10), (7, 10)];
const LEGS_TOGETHER: [(i32, i32); 6] = [(3, 8), (5, 8), (3, 9), (5, 9), (4, 10), (5, 10)];

/// A little fire person the comet left: it dashes from snow patch to snow patch.
pub struct Ember {
    pub x: f32,
    /// -1 = facing left, 1 = right.
    pub dir: f32,
    speed: f32,
    /// Where it is running to.
    target: f32,
    retarget_in: f32,
    /// Time into the current hop (s), or None on the ground.
    hop: Option<f32>,
    hop_in: f32,
    /// Running clock for the arms and legs.
    run_t: f32,
    budget: f32,
    rate: f32,
    life: f32,
    /// Digging across a patch (slowly) rather than dashing to the next one.
    digging: bool,
    /// Thrown by the impact and still in the air (#172).
    flight: Option<Flight>,
}

/// A creature's arc from the impact to where it lands.
struct Flight {
    from: f32,
    to: f32,
    t: f32,
    dur: f32,
}

impl Flight {
    /// Height above the snow under it (px): a parabola under `THROW_GRAVITY`
    /// that starts and ends on the snow.
    fn height(&self) -> f32 {
        0.5 * THROW_GRAVITY * self.t * (self.dur - self.t)
    }

    fn x(&self) -> f32 {
        self.from + (self.to - self.from) * (self.t / self.dur).min(1.0)
    }
}

impl Ember {
    /// Arms up (else down) and legs apart (else passing) right now: both
    /// keep swapping while it runs.
    pub fn pose(&self) -> (bool, bool) {
        ((self.run_t * 8.0) as i32 % 2 == 0, (self.run_t * 10.0) as i32 % 2 == 0)
    }

    /// Height off the snow while thrown or hopping (px).
    fn lift(&self) -> f32 {
        if let Some(f) = &self.flight {
            return f.height();
        }
        self.hop.map_or(0.0, |t| (std::f32::consts::PI * t / HOP_S).sin() * HOP_H)
    }

    /// Still flying from the impact.
    pub fn flying(&self) -> bool {
        self.flight.is_some()
    }
}

/// The snow worth running to from `x`: deep and near win. With no snow left
/// anywhere it runs to the far edge.
fn pick_target(x: f32, snow: &Snow, width: f32, rng: &mut Rng) -> f32 {
    let (lo, hi) = (2.0, width - 3.0);
    let best = (lo as i32..=hi as i32)
        .step_by(4)
        .map(|c| c as f32)
        .filter(|&c| snow.height_at(c) >= SNOW_WORTH)
        .map(|c| (c, snow.height_at(c) - (c - x).abs() * DISTANCE_COST + rng.range(0.0, 2.0)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match best {
        Some((c, _)) => c,
        None if x < width / 2.0 => hi,
        None => lo,
    }
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

    /// The way it travels across the screen: -1 = leftward, 1 = rightward.
    pub fn travel(&self) -> f32 {
        if self.to_x >= self.from_x { 1.0 } else { -1.0 }
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
            let n = (4 + (rng.f32() * 3.0) as usize).min(MAX_EMBERS);
            let share = EMBER_BUDGET * snow.total() / n as f32;
            // Thrown across the screen (#172): one per slot of its width, so the
            // left, the middle and the right all get some, a little ahead of
            // where the comet was going.
            let (lo, hi) = (2.0, width - 3.0);
            let slot = (hi - lo) / n as f32;
            for k in 0..n {
                let centre = lo + slot * (k as f32 + 0.5) + self.travel() * slot / 6.0;
                let to = (centre + rng.range(-slot / 3.0, slot / 3.0)).clamp(lo, hi);
                let dur = THROW_S + THROW_PER_WIDTH * (to - self.to_x).abs() / width;
                let flight = Some(Flight { from: self.to_x, to, t: 0.0, dur });
                let rate = share / (EMBER_SPEND_S * EMBER_RADIUS);
                let speed = rng.range(EMBER_SPEED.0, EMBER_SPEED.1);
                let dir = if to >= self.to_x { 1.0 } else { -1.0 };
                let retarget_in = rng.range(EMBER_RETARGET.0, EMBER_RETARGET.1);
                let hop_in = rng.range(0.3, HOP_EVERY.1);
                let (life, run_t) = (EMBER_LIFE * rng.range(0.9, 1.0), rng.range(0.0, 1.0));
                let (x, target, budget) = (self.to_x, to, share);
                let hop = None;
                let digging = false;
                let e = Ember { x, dir, speed, target, retarget_in, hop, hop_in, run_t, budget, rate, life, digging, flight };
                self.embers.push(e);
            }
            return Some(Impact { x: self.to_x });
        }
        let (lo, hi) = (2.0, width - 3.0);
        for e in &mut self.embers {
            e.run_t += dt;
            if let Some(f) = &mut e.flight {
                // In the air: it melts nothing and its life has not started.
                f.t += dt;
                e.x = f.x();
                if f.t >= f.dur {
                    e.flight = None;
                }
                continue;
            }
            e.life -= dt;
            e.retarget_in -= dt;
            if (e.target - e.x).abs() < 1.5 || e.retarget_in <= 0.0 {
                // Reached it: run across what is left here, or dash to the next patch.
                e.digging = snow.height_at(e.x) >= SNOW_WORTH && e.retarget_in > 0.0;
                e.target = if e.digging {
                    let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
                    (e.x + side * rng.range(EMBER_ACROSS.0, EMBER_ACROSS.1)).clamp(lo, hi)
                } else {
                    e.retarget_in = rng.range(EMBER_RETARGET.0, EMBER_RETARGET.1);
                    pick_target(e.x, snow, width, rng)
                };
            }
            let gap = e.target - e.x;
            if gap.abs() > 0.5 {
                e.dir = gap.signum();
                let speed = if e.digging { e.speed * EMBER_DIG_PACE } else { e.speed };
                e.x = (e.x + e.dir * (speed * dt).min(gap.abs())).clamp(lo, hi);
            }
            e.hop_in -= dt;
            e.hop = match e.hop {
                Some(t) if t + dt < HOP_S => Some(t + dt),
                Some(_) => None,
                None if e.hop_in <= 0.0 => {
                    e.hop_in = rng.range(HOP_EVERY.0, HOP_EVERY.1);
                    Some(0.0)
                }
                None => None,
            };
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
            let feet = (ground_y - snow.height_at(e.x) - e.lift()).round() as i32;
            let top = feet - 11;
            let heat = (e.life / EMBER_FADE_S).min(1.0);
            let flick = ((time * 9.0 + i as f32 * 1.3).sin() * 0.5 + 0.5) > 0.5;
            let (hot, warm) = if flick { (0xfff4b0, 0xffc13d) } else { (0xffc13d, 0xff7a2a) };
            let flip = e.dir < 0.0;
            let mut put = |col: i32, row: i32, rgb: u32| {
                let col = if flip { 8 - col } else { col };
                let (px, py) = (x - 4 + col, top + row);
                if bayer(px, py) < heat {
                    c.set(px, py, hex(rgb));
                }
            };
            for (row, line) in PERSON.iter().enumerate() {
                for (col, ch) in line.chars().enumerate() {
                    let rgb = match ch {
                        'H' => hot,
                        'W' => warm,
                        'Y' => 0xffd64a,
                        'R' => 0xd93a2a,
                        'E' => 0x1b1942,
                        _ => continue,
                    };
                    put(col as i32, row as i32, rgb);
                }
            }
            // Flailing arms and running legs, swapping as it runs.
            let (arms_up, legs_apart) = e.pose();
            for (col, row) in if arms_up { ARMS_UP } else { ARMS_DOWN } {
                put(col, row, warm);
            }
            for (col, row) in if legs_apart { LEGS_APART } else { LEGS_TOGETHER } {
                put(col, row, 0xd93a2a);
            }
            if heat >= 1.0 {
                c.glow(e.x, top as f32 + 3.0, 5.0, 0.6, hex(warm));
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
        assert!(c.embers.iter().all(|e| e.flying() && (e.x - 100.0).abs() < 10.0), "thrown from the impact");
        assert!(snow.total() <= before * (1.0 - COMET_MELT) + 1e-2);
    }

    #[test]
    fn they_are_thrown_across_the_screen_and_land_in_every_third() {
        let w = 480.0;
        for (seed, at) in [(1, 60.0), (2, 240.0), (3, 420.0)] {
            let mut snow = Snow::new(w as i32, 60.0);
            snow.add(w / 2.0, 20.0, w);
            let mut c = Comet::new(at, w);
            let mut rng = Rng::new(seed);
            let dt = 1.0 / 30.0;
            while c.falling() {
                c.step(dt, &mut snow, w, &mut rng);
            }
            let after_impact = snow.total();
            let mut landed: Vec<Option<(f32, f32)>> = vec![None; c.embers.len()];
            let mut t = 0.0;
            while landed.iter().any(Option::is_none) && t < 3.0 {
                if c.embers.iter().all(Ember::flying) {
                    assert_eq!(snow.total(), after_impact, "nothing melts while they fly");
                }
                c.step(dt, &mut snow, w, &mut rng);
                t += dt;
                for (l, e) in landed.iter_mut().zip(&c.embers) {
                    if l.is_none() && !e.flying() {
                        *l = Some((e.x, t));
                    }
                }
            }
            let landed: Vec<(f32, f32)> = landed.into_iter().map(|l| l.expect("every one lands")).collect();
            for third in 0..3 {
                let (a, b) = (w * third as f32 / 3.0, w * (third + 1) as f32 / 3.0);
                assert!(
                    landed.iter().any(|(x, _)| (a..b).contains(x)),
                    "comet at {at}: none in [{a}, {b}): {landed:?}"
                );
            }
            for (x, t) in &landed {
                let arc = THROW_S + THROW_PER_WIDTH * (x - at).abs() / w;
                assert!(*t >= arc - dt, "landed at {x} after {t} s, before its {arc} s arc");
            }
        }
    }

    #[test]
    fn one_pass_over_deep_snow_digs_a_trench_not_a_film() {
        // #182: a pile at the ice line on 480x270 (81 px), one creature digging alone.
        let mut snow = Snow::new(480, 200.0);
        snow.add(240.0, 81.0, 5000.0);
        let mut c = Comet::new(240.0, 480.0);
        let mut rng = Rng::new(7);
        let mut step = |c: &mut Comet, snow: &mut Snow| c.step(1.0 / 30.0, snow, 480.0, &mut rng);
        while c.embers.is_empty() || c.embers.iter().any(Ember::flying) {
            step(&mut c, &mut snow);
        }
        for e in c.embers.iter_mut().skip(1) {
            e.budget = 0.0;
        }
        while !c.embers[0].digging {
            step(&mut c, &mut snow);
        }
        let (from, to) = (c.embers[0].x, c.embers[0].target);
        let mid = (from + to) / 2.0;
        let before = snow.height_at(mid);
        while c.embers[0].target == to {
            step(&mut c, &mut snow);
        }
        let dug = before - snow.height_at(mid);
        assert!(dug >= 12.0, "one pass from {from:.0} to {to:.0} dug {dug:.1} px of {before:.0}");
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
        // Its life starts when it lands (#172): after the fall and the longest throw.
        run(&mut c, &mut snow, FALL_S + THROW_S + THROW_PER_WIDTH + EMBER_LIFE + 0.5);
        assert!(c.done());
        let after = snow.total();
        run(&mut c, &mut snow, 10.0);
        assert_eq!(snow.total(), after);
    }

    #[test]
    fn on_bare_ground_they_run_edge_to_edge_and_melt_nothing() {
        let mut snow = Snow::new(200, 60.0);
        let mut c = Comet::new(100.0, 200.0);
        run(&mut c, &mut snow, FALL_S + THROW_S + THROW_PER_WIDTH + 0.1);
        assert!(!c.embers.iter().any(Ember::flying), "all landed");
        let start: Vec<f32> = c.embers.iter().map(|e| e.x).collect();
        run(&mut c, &mut snow, 1.5);
        assert_eq!(snow.total(), 0.0);
        assert!(c.embers.iter().zip(&start).all(|(e, s)| (e.x - s).abs() > 30.0), "they keep running");
    }

    #[test]
    fn they_dash_to_the_snow() {
        let mut snow = Snow::new(200, 60.0);
        snow.add(185.0, 30.0, 5.0); // snow only at the right edge
        let mut c = Comet::new(60.0, 200.0);
        run(&mut c, &mut snow, 1.25);
        let landed: f32 = c.embers.iter().map(|e| e.x).sum::<f32>() / c.embers.len() as f32;
        run(&mut c, &mut snow, 5.0);
        assert!(c.embers.iter().all(|e| e.x > 150.0), "all ran to the snow from about {landed}");
    }

    #[test]
    fn their_arms_and_legs_keep_moving() {
        let mut snow = pile();
        let mut c = Comet::new(100.0, 200.0);
        run(&mut c, &mut snow, 1.3);
        let mut rng = Rng::new(9);
        let mut poses = std::collections::HashSet::new();
        for _ in 0..15 {
            c.step(1.0 / 30.0, &mut snow, 200.0, &mut rng);
            poses.insert(c.embers[0].pose());
        }
        assert_eq!(poses.len(), 4, "arms up and down, legs apart and passing: {poses:?}");
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
        assert!(xs.iter().flatten().all(|x| x.is_finite()));
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
