//! Frost on the glass: fern-like ice crystals that grow in
//! from the top and side edges as the screen freezes, like a window icing
//! over. Laid out once per screen size; every crystal pixel knows when it
//! grows, so a frame only draws the ones the freeze has reached so far.

use super::rng::Rng;
use crate::render::canvas::{Canvas, Rgba, hex};

/// How far crystals reach in from an edge, as a share of the screen's
/// shorter side; the top corners grow the big ones.
const REACH: f32 = 0.14;
const CORNER_REACH: f32 = 0.26;
/// Side crystals grow down to here (share of the height): below lies the pile.
const SIDE_DOWN_TO: f32 = 0.75;

const STEM: Rgba = hex(0xeef5ff);
const BRANCH: Rgba = hex(0xd4e2fb);

/// One pixel of frost and when it shows: 0 first, 1 last.
struct Grain {
    x: i32,
    y: i32,
    when: f32,
    color: Rgba,
}

pub struct Glass {
    /// Sorted by `when`.
    grains: Vec<Grain>,
}

impl Glass {
    pub fn new(w: i32, h: i32) -> Glass {
        let mut rng = Rng::new(0x6c61_7373 ^ (w as u64) << 20 ^ h as u64);
        let short = w.min(h) as f32;
        let mut grains = Vec::new();
        let (reach, corner) = (short * REACH, short * CORNER_REACH);
        let down = std::f32::consts::FRAC_PI_2;
        // The top corners first, pointing into the screen.
        for (x, angle) in [(0.0, down * 0.5), (w as f32 - 1.0, down * 1.5)] {
            let delay = rng.range(0.0, 0.1);
            crystal(&mut grains, &mut rng, (x, 0.0), angle, corner, delay);
        }
        // Then crystals all along the top and down the sides.
        let mut seed_along = |len: f32, at: &dyn Fn(f32) -> ((f32, f32), f32), rng: &mut Rng| {
            let mut p = rng.range(10.0, 30.0);
            while p < len {
                let (start, angle) = at(p);
                let size = reach * rng.range(0.45, 1.0);
                let delay = rng.range(0.1, 0.6);
                let angle = angle + rng.range(-0.35, 0.35);
                crystal(&mut grains, rng, start, angle, size, delay);
                p += rng.range(18.0, 34.0);
            }
        };
        seed_along(w as f32, &|x| ((x, 0.0), down), &mut rng);
        seed_along(h as f32 * SIDE_DOWN_TO, &|y| ((0.0, y), 0.0), &mut rng);
        seed_along(h as f32 * SIDE_DOWN_TO, &|y| ((w as f32 - 1.0, y), down * 2.0), &mut rng);
        grains.sort_by(|a, b| a.when.total_cmp(&b.when));
        Glass { grains }
    }

    /// Draws the frost grown so far: `progress` 0 is clear glass, 1 all of it.
    pub fn draw(&self, c: &mut Canvas, progress: f32) {
        let shown = self.grains.partition_point(|g| g.when < progress);
        for g in &self.grains[..shown] {
            c.set(g.x, g.y, g.color);
        }
    }
}

/// A fern of ice: a stem from `start` along `angle`, with side branches at
/// 60° (ice grows in sixes) that shorten toward the tip, and twigs on those.
/// It starts growing at `delay` and reaches its tip at 1.
fn crystal(grains: &mut Vec<Grain>, rng: &mut Rng, start: (f32, f32), angle: f32, len: f32, delay: f32) {
    branch(grains, rng, start, angle, len, 0, delay, (1.0 - delay) / len.max(1.0));
}

#[allow(clippy::too_many_arguments)]
fn branch(
    grains: &mut Vec<Grain>,
    rng: &mut Rng,
    (mut x, mut y): (f32, f32),
    mut angle: f32,
    len: f32,
    level: u8,
    when: f32,
    per_px: f32,
) {
    let side = std::f32::consts::FRAC_PI_3;
    let mut next_fork = rng.range(2.0, 4.0);
    let mut t = 0.0;
    while t < len {
        grains.push(Grain {
            x: x.round() as i32,
            y: y.round() as i32,
            when: when + t * per_px,
            color: if level == 0 { STEM } else { BRANCH },
        });
        if level < 2 && t >= next_fork {
            let rest = (len - t) * if level == 0 { 0.5 } else { 0.4 };
            for turn in [-side, side] {
                if rng.chance(0.85) {
                    let at = when + t * per_px;
                    let len = rest * rng.range(0.7, 1.0);
                    branch(grains, rng, (x, y), angle + turn, len, level + 1, at, per_px);
                }
            }
            next_fork = t + rng.range(3.0, 6.0) + level as f32 * 2.0;
        }
        angle += rng.range(-0.06, 0.06);
        x += angle.cos();
        y += angle.sin();
        t += 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(progress: f32) -> Canvas {
        let mut c = Canvas::new(480, 270);
        Glass::new(480, 270).draw(&mut c, progress);
        c
    }

    #[test]
    fn clear_glass_until_the_screen_starts_to_freeze() {
        assert_eq!(drawn(0.0).opaque_in(0, 0, 480, 270), 0);
    }

    #[test]
    fn frost_grows_in_from_the_edges_and_never_reaches_the_middle() {
        let (some, all) = (drawn(0.3), drawn(1.0));
        let near_edges =
            |c: &Canvas| c.opaque_in(0, 0, 480, 20) + c.opaque_in(0, 0, 20, 200) + c.opaque_in(460, 0, 20, 200);
        assert!(near_edges(&some) > 0, "it starts at the edges");
        assert!(all.opaque_in(0, 0, 480, 270) > some.opaque_in(0, 0, 480, 270) * 2, "and keeps growing");
        assert_eq!(all.opaque_in(140, 90, 200, 100), 0, "the middle of the screen stays clear");
    }

    #[test]
    fn the_frost_is_thin_crystals_the_desktop_shows_between() {
        let c = drawn(1.0);
        let band = c.opaque_in(0, 0, 480, 20);
        assert!(band > 480, "frosty along the top: {band}");
        assert!(band < 480 * 20 / 2, "but the desktop still shows through: {band} of {}", 480 * 20);
    }
}
