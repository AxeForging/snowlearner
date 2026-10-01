//! The snow blanket: the ground pile and the snow on the top and side edges
//! drawn as one field. Each edge is a distance to its own surface; the
//! blanket is their smooth union, so the edges meet in rounded curves and
//! every pixel is shaded by its depth under one continuous surface — the
//! snow frames the screen in one piece, with no seam where a wall meets the
//! top or the ground. Only the pixels near an edge are ever evaluated, and only
//! the rows and columns whose snow moved.

use super::frost::{Edge, Frost};
use super::rng::hash01;
use super::snow::{OUTLINE, Snow, shade};
use crate::render::canvas::{CLEAR, Canvas, Rgba, hex};

/// Radius (px) of the curve where two edges of snow meet.
const FILLET: f32 = 5.0;

/// Polynomial smooth minimum: `min(a, b)`, rounded within `k` of where the
/// two are equal. Keeps distances continuous, so shading has no seam.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// The blanket over a `w`×`h` screen whose ground line is `ground_y`: how
/// deep each edge's snow is, read once and rounded to whole pixels (a flake
/// is one pixel; less than that never shows).
#[derive(PartialEq)]
struct Field {
    w: i32,
    rows: i32,
    ground_y: f32,
    top: Vec<f32>,
    ground: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Field {
    fn new(w: i32, h: i32, snow: &Snow, frost: &Frost, ground_y: i32) -> Field {
        let along = |n: i32, f: &dyn Fn(f32) -> f32| (0..n).map(|i| f(i as f32).round()).collect();
        Field {
            w,
            // Below the ground line (window mode) is the landscape, never snow.
            rows: h.min(ground_y),
            ground_y: ground_y as f32,
            top: along(w, &|x| frost.depth_at(Edge::Top, x)),
            ground: along(w, &|x| snow.height_at(x)),
            left: along(h, &|y| frost.depth_at(Edge::Left, y)),
            right: along(h, &|y| frost.depth_at(Edge::Right, y)),
        }
    }

    /// Signed distance (px) to the blanket's surface: negative inside the snow.
    fn sdf(&self, x: i32, y: i32) -> f32 {
        let (i, j) = (x as usize, y as usize);
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let (t, g, l, r) = (self.top[i], self.ground[i], self.left[j], self.right[j]);
        let (left, right) = (px - l, (self.w as f32 - r) - px);
        let (top, ground) = (py - t, (self.ground_y - g) - py);
        // The curve is only as big as the thinner of the two snows it joins:
        // where one side has none, the other meets the glass square on.
        let (across, side) = if left < right { (left, l) } else { (right, r) };
        let (down, lid) = if top < ground { (top, t) } else { (ground, g) };
        smin(across, down, FILLET.min(side).min(lid))
    }

    fn is_snow(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.rows && self.sdf(x, y) < 0.0
    }

    /// Whole pixels under the surface (0 = the surface itself).
    #[cfg(test)]
    fn depth_px(&self, x: i32, y: i32) -> i32 {
        (-self.sdf(x, y)).floor() as i32
    }
}

/// How far from each edge snow (or its rim) can reach: the curve adds at
/// most FILLET/4 past the nearer surface, the rim one more pixel. Past this
/// the screen is always clear and is never looked at.
struct Reach {
    w: i32,
    rows: i32,
    tops: Vec<i32>,
    grounds: Vec<i32>,
    lefts: Vec<i32>,
    rights: Vec<i32>,
    top_band: i32,
    ground_band: i32,
}

impl Reach {
    fn new(f: &Field) -> Reach {
        let reach = |depth: f32| (depth + FILLET * 0.25).ceil() as i32 + 2;
        let tops: Vec<i32> = f.top.iter().map(|&t| reach(t)).collect();
        let grounds: Vec<i32> = f.ground.iter().map(|&g| f.ground_y as i32 - reach(g)).collect();
        let lefts: Vec<i32> = f.left.iter().map(|&l| reach(l).min(f.w)).collect();
        let rights = f.right.iter().zip(&lefts).map(|(&r, &l)| (f.w - reach(r)).max(l)).collect();
        Reach {
            w: f.w,
            rows: f.rows,
            top_band: tops.iter().copied().max().unwrap_or(0),
            ground_band: grounds.iter().copied().min().unwrap_or(f.rows),
            tops,
            grounds,
            lefts,
            rights,
        }
    }

    /// Calls `f` for every pixel of row `y` that snow could reach.
    fn row(&self, y: i32, mut f: impl FnMut(i32)) {
        let j = y as usize;
        let (left, right) = (self.lefts[j], self.rights[j]);
        (0..left).chain(right..self.w).for_each(&mut f);
        // Between the walls only the top and the ground bands hold snow.
        if y < self.top_band || y >= self.ground_band {
            (left..right).filter(|&x| y < self.tops[x as usize] || y >= self.grounds[x as usize]).for_each(f);
        }
    }

    /// Calls `f` for every pixel of column `x` that snow could reach.
    fn column(&self, x: i32, mut f: impl FnMut(i32)) {
        let i = x as usize;
        for y in 0..self.rows {
            let j = y as usize;
            if x < self.lefts[j] || x >= self.rights[j] || y < self.tops[i] || y >= self.grounds[i] {
                f(y);
            }
        }
    }
}

/// The blanket as last worked out. A pixel depends only on the snow of its
/// own row and column (and its neighbors', for the rim), so when a flake
/// lands only those few lines are worked out again; the rest is copied.
#[derive(Default)]
pub struct Blanket {
    drawn: Option<(Field, bool)>,
    /// Snow or rim color of each pixel above the ground line, CLEAR elsewhere.
    layer: Vec<Rgba>,
    /// Surface pixels that twinkle now and then.
    glint: Vec<bool>,
}

impl Blanket {
    /// Draws the ground pile and the edge snow as one blanket: white at the
    /// surface, bluer inside, twinkling here and there; `outline` (over the
    /// desktop) adds the dark rim around the whole of it.
    pub fn draw(&mut self, c: &mut Canvas, snow: &Snow, frost: &Frost, ground_y: i32, time: f32, outline: bool) {
        let field = Field::new(c.w, c.h, snow, frost, ground_y);
        let reach = Reach::new(&field);
        let (w, rows) = (field.w, field.rows);
        let paint = |layer: &mut [Rgba], glint: &mut [bool], x: i32, y: i32| {
            let i = (y * w + x) as usize;
            let d = field.sdf(x, y);
            let (col, shines) = if d < 0.0 {
                let depth = (-d) as i32;
                (shade(depth, x, y), depth == 1 && hash01(x, y) > 0.985)
            } else if outline
                && d < 1.5
                && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| field.is_snow(x + dx, y + dy))
            {
                (OUTLINE, false)
            } else {
                (CLEAR, false)
            };
            layer[i] = col;
            glint[i] = shines;
        };
        let (layer, glint) = (&mut self.layer, &mut self.glint);
        match &self.drawn {
            Some((old, o)) if *o == outline && (old.w, old.rows, old.ground_y) == (w, rows, field.ground_y) => {
                let moved = |a: &[f32], b: &[f32], c: &[f32], d: &[f32]| -> Vec<usize> {
                    let n = a.len();
                    // A neighbor's rim depends on this line too.
                    (0..n)
                        .filter(|&i| (i.saturating_sub(1)..(i + 2).min(n)).any(|k| a[k] != b[k] || c[k] != d[k]))
                        .collect()
                };
                for j in moved(&old.left, &field.left, &old.right, &field.right) {
                    let y = j as i32;
                    layer[j * w as usize..(j + 1) * w as usize].fill(CLEAR);
                    glint[j * w as usize..(j + 1) * w as usize].fill(false);
                    reach.row(y, |x| paint(layer, glint, x, y));
                }
                for i in moved(&old.top, &field.top, &old.ground, &field.ground) {
                    let x = i as i32;
                    for y in 0..rows as usize {
                        layer[y * w as usize + i] = CLEAR;
                        glint[y * w as usize + i] = false;
                    }
                    reach.column(x, |y| paint(layer, glint, x, y));
                }
            }
            _ => {
                *layer = vec![CLEAR; (w * rows) as usize];
                *glint = vec![false; (w * rows) as usize];
                for y in 0..rows {
                    reach.row(y, |x| paint(layer, glint, x, y));
                }
            }
        }
        for y in 0..rows {
            reach.row(y, |x| {
                let i = (y * w + x) as usize;
                if self.layer[i] != CLEAR {
                    let shines = self.glint[i] && (time * 2.0 + hash01(x, y) * 40.0).sin() > 0.6;
                    c.set(x, y, if shines { hex(0xffffff) } else { self.layer[i] });
                }
            });
        }
        self.drawn = Some((field, outline));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(c: &mut Canvas, snow: &Snow, frost: &Frost, ground_y: i32, time: f32, outline: bool) {
        Blanket::default().draw(c, snow, frost, ground_y, time, outline);
    }

    #[test]
    fn a_kept_blanket_still_shows_the_flake_that_just_landed() {
        let (mut snow, mut frost) = corner(6, 40, 4);
        let mut kept = Blanket::default();
        let mut c = Canvas::new(120, 80);
        kept.draw(&mut c, &snow, &frost, 80, 0.0, true);
        frost.add_grain(Edge::Right, 30.0);
        snow.add_grain(90.0);
        kept.draw(&mut Canvas::new(120, 80), &snow, &frost, 80, 0.0, true);
        frost.melt(0.5);
        frost.add_grain(Edge::Right, 30.0);
        let (mut again, mut fresh) = (Canvas::new(120, 80), Canvas::new(120, 80));
        kept.draw(&mut again, &snow, &frost, 80, 0.0, true);
        draw(&mut fresh, &snow, &frost, 80, 0.0, true);
        assert!(again.opaque_in(119, 30, 1, 1) == 1 && again.opaque_in(90, 79, 1, 1) == 1);
        assert!(again.bytes() == fresh.bytes(), "the kept blanket matches one worked out from scratch");
    }

    /// A screen with snow `n` px deep along the top and the left for the first
    /// `run` px, and a ground pile `g` px high under the left wall.
    fn corner(n: usize, run: usize, g: usize) -> (Snow, Frost) {
        let mut frost = Frost::new(120, 80);
        for i in 0..run {
            for _ in 0..n {
                frost.add_grain(Edge::Top, i as f32);
                frost.add_grain(Edge::Left, i as f32);
            }
        }
        let mut snow = Snow::new(120, 30.0);
        for x in 0..run {
            for _ in 0..g {
                snow.add_grain(x as f32);
            }
        }
        (snow, frost)
    }

    #[test]
    fn where_two_edges_meet_the_snow_curves_instead_of_a_corner() {
        let (snow, frost) = corner(6, 40, 0);
        let f = Field::new(120, 80, &snow, &frost, 80);
        assert!(f.is_snow(6, 6), "the inside corner is filled by the curve");
        assert!(!f.is_snow(20, 20), "but the open screen stays open");
        assert!(f.is_snow(3, 30) && f.is_snow(30, 3), "both edges are there");
    }

    #[test]
    fn the_shading_has_no_seam_anywhere_in_the_blanket() {
        let (snow, frost) = corner(6, 40, 8);
        let f = Field::new(120, 80, &snow, &frost, 80);
        for y in 0..80 {
            for x in 0..120 {
                for (dx, dy) in [(1, 0), (0, 1)] {
                    if f.is_snow(x, y) && f.is_snow(x + dx, y + dy) {
                        let step = (f.depth_px(x, y) - f.depth_px(x + dx, y + dy)).abs();
                        assert!(step <= 1, "depth jumps by {step} at ({x},{y}) — a seam");
                    }
                }
            }
        }
    }

    #[test]
    fn the_ground_and_a_wall_join_in_one_piece() {
        let (snow, frost) = corner(6, 79, 8);
        let f = Field::new(120, 80, &snow, &frost, 80);
        assert!(f.is_snow(6, 71) && f.is_snow(7, 72), "the bottom corner is filled where wall meets pile");
        assert_eq!(f.depth_px(3, 79), f.depth_px(3, 79).max(1), "deep down in the corner, not surface");
    }

    #[test]
    fn white_at_the_surface_bluer_inside_and_a_rim_only_over_the_desktop() {
        let (snow, frost) = corner(10, 60, 0);
        let (mut plain, mut lined) = (Canvas::new(120, 80), Canvas::new(120, 80));
        draw(&mut plain, &snow, &frost, 80, 0.0, false);
        draw(&mut lined, &snow, &frost, 80, 0.0, true);
        let d = frost.depth_at(Edge::Left, 40.0).round() as i32;
        assert!(d >= 6, "a real wall to look at: {d}");
        assert_eq!(plain.get(d - 1, 40), Some(hex(0xffffff)), "surface of the left wall");
        assert_ne!(plain.get(0, 40), Some(hex(0xffffff)), "deep against the glass");
        assert_eq!(plain.opaque_in(d, 40, 1, 1), 0);
        assert_eq!(lined.get(d, 40), Some(OUTLINE), "rim just outside the surface");
        assert_eq!(plain.opaque_in(30, 30, 60, 40), 0, "the middle of the screen stays clear");
    }

    #[test]
    fn window_mode_leaves_the_landscape_below_the_ground_line_alone() {
        let (snow, frost) = corner(6, 40, 5);
        let mut c = Canvas::new(120, 80);
        draw(&mut c, &snow, &frost, 70, 0.0, true);
        assert_eq!(c.opaque_in(0, 70, 120, 10), 0, "nothing drawn on the land band");
        assert!(c.opaque_in(0, 60, 40, 10) > 0, "the pile above it is");
    }
}
