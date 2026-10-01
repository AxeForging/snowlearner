//! Snow stuck to the top, left and right edges of the screen: flakes the wind
//! blows into a side wall stay on the row they hit, the top corners ice over
//! on their own, and summoned friends burst snow onto the nearest edge.
//! Speaking melts it. Drawn like the ground pile, the edge being its floor.

use super::snow::{OUTLINE, shade};
use crate::render::canvas::Canvas;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Left,
    Right,
}

/// Deepest edge snow, as a share of the screen's shorter side: enough to
/// press in from the walls without walling off the desktop.
const EDGE_CAP: f32 = 0.08;
/// Friends' bursts and the corners' creep were tuned for edges as deep as 22%
/// of the screen. Scaled to EDGE_CAP, the edges freeze — coverage, and the
/// freeze level built on it — exactly as fast as before, only thinner.
const TUNED_FOR_CAP: f32 = 0.22;

pub struct Frost {
    w: i32,
    h: i32,
    top: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    max_depth: f32,
}

impl Frost {
    pub fn new(w: i32, h: i32) -> Self {
        let max_depth = (w.min(h) as f32 * EDGE_CAP).max(8.0);
        Frost {
            w,
            h,
            top: vec![0.0; w.max(1) as usize],
            left: vec![0.0; h.max(1) as usize],
            right: vec![0.0; h.max(1) as usize],
            max_depth,
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        let stretch = |v: &[f32], n: i32| -> Vec<f32> {
            let n = n.max(1) as usize;
            (0..n).map(|i| if v.is_empty() { 0.0 } else { v[i * v.len() / n] }).collect()
        };
        self.top = stretch(&self.top, w);
        self.left = stretch(&self.left, h);
        self.right = stretch(&self.right, h);
        self.w = w;
        self.h = h;
        self.max_depth = (w.min(h) as f32 * EDGE_CAP).max(8.0);
    }

    pub fn nearest_edge(&self, x: f32, y: f32) -> Edge {
        let (dl, dr, dt) = (x, self.w as f32 - x, y);
        if dt <= dl && dt <= dr {
            Edge::Top
        } else if dl <= dr {
            Edge::Left
        } else {
            Edge::Right
        }
    }

    fn edge_mut(&mut self, e: Edge) -> &mut Vec<f32> {
        match e {
            Edge::Top => &mut self.top,
            Edge::Left => &mut self.left,
            Edge::Right => &mut self.right,
        }
    }

    /// Adds a patch of snow on `edge`, centered at `pos` (x for the top edge, y
    /// for the sides); `strength` is in the 22%-cap units it was tuned in.
    pub fn burst(&mut self, edge: Edge, pos: f32, strength: f32, radius: f32) {
        let (max, strength) = (self.max_depth, strength * EDGE_CAP / TUNED_FOR_CAP);
        let v = self.edge_mut(edge);
        let (lo, hi) = ((pos - radius * 2.0).floor() as i64, (pos + radius * 2.0).ceil() as i64);
        for i in lo.max(0)..=hi.min(v.len() as i64 - 1) {
            let d = (i as f32 - pos) / radius;
            let cell = &mut v[i as usize];
            *cell = (*cell + strength * (-d * d).exp()).min(max);
        }
    }

    /// How deep the snow on `edge` is at `pos` (x for the top, y for the sides).
    pub fn depth_at(&self, edge: Edge, pos: f32) -> f32 {
        let v = match edge {
            Edge::Top => &self.top,
            Edge::Left => &self.left,
            Edge::Right => &self.right,
        };
        if pos < 0.0 { 0.0 } else { v.get(pos as usize).copied().unwrap_or(0.0) }
    }

    /// One flake stuck to `edge` at `pos`: exactly one pixel on that row.
    /// False when it missed the wall or that row is already at the cap.
    pub fn add_grain(&mut self, edge: Edge, pos: f32) -> bool {
        let max = self.max_depth;
        if pos < 0.0 {
            return false;
        }
        match self.edge_mut(edge).get_mut(pos as usize) {
            Some(d) if *d < max => {
                *d = (*d + 1.0).min(max);
                true
            }
            _ => false,
        }
    }

    /// Corners slowly ice over on their own, faster the more frozen the screen.
    pub fn creep(&mut self, dt: f32, rate: f32) {
        let r = (self.w.min(self.h) as f32 * 0.25).max(4.0);
        let (w, h) = (self.w as f32, self.h as f32);
        let amt = rate * dt;
        self.burst(Edge::Top, 0.0, amt, r);
        self.burst(Edge::Top, w, amt, r);
        self.burst(Edge::Left, 0.0, amt, r);
        self.burst(Edge::Right, 0.0, amt, r);
        let _ = h;
    }

    /// Lets a row stuck out from its neighbors slide into them, as the ground
    /// pile does, so the walls show drifts instead of one-row whiskers.
    pub fn settle(&mut self) {
        for v in [&mut self.top, &mut self.left, &mut self.right] {
            for i in 1..v.len() {
                let diff = v[i - 1] - v[i];
                if diff.abs() > 1.0 {
                    let moved = diff * 0.35;
                    v[i - 1] -= moved;
                    v[i] += moved;
                }
            }
        }
    }

    pub fn melt(&mut self, fraction: f32) {
        let k = 1.0 - fraction.clamp(0.0, 1.0);
        for v in [&mut self.top, &mut self.left, &mut self.right] {
            v.iter_mut().for_each(|d| *d *= k);
        }
    }

    /// 0 = clear glass, 1 = every edge at max depth.
    pub fn coverage(&self) -> f32 {
        let total: f32 = self.top.iter().chain(&self.left).chain(&self.right).sum();
        let n = (self.top.len() + self.left.len() + self.right.len()) as f32;
        total / (n * self.max_depth)
    }

    /// Solid from the edge to the surface, shaded like the ground pile;
    /// `outline` adds its dark rim past the surface (over the desktop).
    pub fn draw(&self, c: &mut Canvas, outline: bool) {
        let w = self.w;
        for (i, &d) in self.top.iter().enumerate() {
            pile(c, d, outline, |k| (i as i32, k));
        }
        for (i, &d) in self.left.iter().enumerate() {
            pile(c, d, outline, |k| (k, i as i32));
        }
        for (i, &d) in self.right.iter().enumerate() {
            pile(c, d, outline, |k| (w - 1 - k, i as i32));
        }
    }
}

/// One row of edge snow, `depth` px deep; `at(k)` is the pixel `k` px in from the edge.
fn pile(c: &mut Canvas, depth: f32, outline: bool, at: impl Fn(i32) -> (i32, i32)) {
    let d = depth.round() as i32;
    if d <= 0 {
        return;
    }
    for k in 0..d {
        let (x, y) = at(k);
        c.set(x, y, shade(d - 1 - k, x, y));
    }
    if outline {
        let (x, y) = at(d);
        c.set(x, y, OUTLINE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_edge_picks_the_closest_side() {
        let f = Frost::new(200, 100);
        assert_eq!(f.nearest_edge(100.0, 5.0), Edge::Top);
        assert_eq!(f.nearest_edge(3.0, 60.0), Edge::Left);
        assert_eq!(f.nearest_edge(197.0, 60.0), Edge::Right);
    }

    #[test]
    fn burst_adds_frost_and_is_capped() {
        let mut f = Frost::new(200, 100);
        assert_eq!(f.coverage(), 0.0);
        f.burst(Edge::Left, 50.0, 10.0, 8.0);
        let once = f.coverage();
        assert!(once > 0.0);
        for _ in 0..500 {
            f.burst(Edge::Left, 50.0, 10.0, 8.0);
        }
        assert!(f.coverage() <= 1.0);
    }

    #[test]
    fn melt_reduces_coverage_proportionally() {
        let mut f = Frost::new(200, 100);
        f.burst(Edge::Top, 100.0, 12.0, 20.0);
        let before = f.coverage();
        f.melt(0.5);
        assert!((f.coverage() - before * 0.5).abs() < 1e-5);
    }

    #[test]
    fn creep_grows_frost_from_the_corners_over_time() {
        let mut f = Frost::new(200, 100);
        for _ in 0..60 {
            f.creep(1.0, 0.2);
        }
        let mut c = Canvas::new(200, 100);
        f.draw(&mut c, false);
        let corner = c.opaque_in(0, 0, 20, 20);
        let center = c.opaque_in(90, 40, 20, 20);
        assert!(corner > 0 && center == 0, "corner {corner} center {center}");
    }

    #[test]
    fn frost_is_drawn_near_edges_only() {
        let mut f = Frost::new(120, 80);
        f.burst(Edge::Right, 40.0, 30.0, 10.0);
        let mut c = Canvas::new(120, 80);
        f.draw(&mut c, false);
        assert!(c.opaque_in(100, 30, 20, 20) > 0);
        assert_eq!(c.opaque_in(0, 30, 40, 20), 0);
    }

    #[test]
    fn a_flake_on_a_wall_adds_exactly_one_pixel_to_that_row_up_to_the_cap() {
        let mut f = Frost::new(200, 100);
        assert!(f.add_grain(Edge::Left, 40.7));
        assert_eq!(f.depth_at(Edge::Left, 40.0), 1.0);
        assert_eq!(f.depth_at(Edge::Left, 41.0), 0.0, "only the row it hit");
        assert!(f.add_grain(Edge::Right, 10.0));
        assert_eq!(f.depth_at(Edge::Right, 10.0), 1.0);
        while f.add_grain(Edge::Left, 40.0) {}
        assert_eq!(f.depth_at(Edge::Left, 40.0), f.max_depth, "capped");
        assert!(!f.add_grain(Edge::Left, -1.0) && !f.add_grain(Edge::Right, 100.0), "off the wall");
    }

    #[test]
    fn wall_snow_is_solid_from_the_glass_to_its_surface_with_no_specks() {
        let mut f = Frost::new(120, 80);
        f.burst(Edge::Left, 40.0, 20.0, 8.0);
        f.burst(Edge::Top, 60.0, 12.0, 10.0);
        let mut c = Canvas::new(120, 80);
        f.draw(&mut c, false);
        for y in 0..80 {
            let d = f.depth_at(Edge::Left, y as f32).round() as i32;
            for x in 0..d {
                assert!(c.opaque_in(x, y, 1, 1) == 1, "hole at ({x},{y}) inside a {d}px pile");
            }
        }
        // Nothing past the surface but the outline (and, here, the top's pile).
        let speck = (30..80)
            .flat_map(|y| (25..120).map(move |x| (x, y)))
            .filter(|&(x, y)| c.opaque_in(x, y, 1, 1) == 1)
            .count();
        assert_eq!(speck, 0, "no specks away from the walls");
    }

    #[test]
    fn over_the_desktop_the_wall_snow_gets_an_outline_like_the_ground() {
        let mut f = Frost::new(120, 80);
        f.burst(Edge::Right, 40.0, 10.0, 6.0);
        let (mut plain, mut lined) = (Canvas::new(120, 80), Canvas::new(120, 80));
        f.draw(&mut plain, false);
        f.draw(&mut lined, true);
        assert!(lined.opaque_in(0, 0, 120, 80) > plain.opaque_in(0, 0, 120, 80));
    }
}
