//! Snow stuck to the top, left and right edges of the screen: flakes the wind
//! blows into a side wall, the corners icing over on their own, and summoned
//! friends bursting snow onto the nearest edge. Snow only stays where
//! something holds it — the ground pile, the wall snow under it, a wall that
//! reached the top corner — and slides toward that support otherwise, so the
//! walls fill up from the pile and the top ices over from the corners.
//! Speaking melts it. Drawn with the ground pile as one blanket (`blanket.rs`).

use super::rng::hash01;

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
    /// Rows where the ground pile's surface meets the left and right walls.
    pile: (f32, f32),
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
            pile: (h as f32, h as f32),
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
        self.pile = (h as f32, h as f32);
    }

    /// Where the ground pile's surface meets the left and right walls (rows):
    /// wall snow just above that rests on the pile.
    pub fn set_pile(&mut self, left: f32, right: f32) {
        self.pile = (left, right);
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

    /// How deep row/column `i` of `edge` may get: the cap rises and falls a
    /// little along the edge, so a full wall reads as drifts, not a ruler.
    fn cap_at(&self, edge: Edge, i: usize) -> f32 {
        let salt = edge as i32 * 17 + 5;
        let (cell, t) = ((i / 24) as i32, (i % 24) as f32 / 24.0);
        let s = t * t * (3.0 - 2.0 * t);
        self.max_depth * (0.8 + 0.2 * (hash01(cell, salt) * (1.0 - s) + hash01(cell + 1, salt) * s))
    }

    /// Adds a patch of snow on `edge`, centered at `pos` (x for the top edge, y
    /// for the sides); `strength` is in the 22%-cap units it was tuned in.
    pub fn burst(&mut self, edge: Edge, pos: f32, strength: f32, radius: f32) {
        let strength = strength * EDGE_CAP / TUNED_FOR_CAP;
        let len = self.edge_mut(edge).len() as i64;
        let (lo, hi) = ((pos - radius * 2.0).floor() as i64, (pos + radius * 2.0).ceil() as i64);
        for i in lo.max(0)..=hi.min(len - 1) {
            let d = (i as f32 - pos) / radius;
            self.deposit(edge, i as usize, strength * (-d * d).exp());
        }
    }

    /// Lays `amount` px of snow at `i` of `edge`; what that spot cannot hold
    /// slides toward what holds it — down a wall, or along the top to the
    /// nearer corner and down that wall — until something does. Snow with
    /// nowhere left to go (a wall full down to the pile) is lost.
    fn deposit(&mut self, mut edge: Edge, mut i: usize, mut amount: f32) {
        loop {
            let room = self.holds_up_to(edge, i) - self.edge_mut(edge)[i];
            if room > 0.0 {
                let put = room.min(amount);
                self.edge_mut(edge)[i] += put;
                amount -= put;
            }
            if amount <= 1e-6 {
                return;
            }
            (edge, i) = match edge {
                Edge::Top => self.toward_wall(i),
                _ if i + 1 < self.h.max(1) as usize => (edge, i + 1),
                _ => return,
            };
        }
    }

    /// How deep row/column `i` of `edge` may get, given what holds it: a wall
    /// row resting on the pile, up to its cap; any other spot up to its cap
    /// once what holds it is full — the wall row under it; for the top, the
    /// column toward the nearer wall, or that wall's top row — and otherwise
    /// one pixel less than that, so the snow climbs a wall leaning like a
    /// drift and the top ices over only from a corner the wall snow reached.
    fn holds_up_to(&self, edge: Edge, i: usize) -> f32 {
        let cap = self.cap_at(edge, i);
        let under = match edge {
            Edge::Top => self.toward_wall(i),
            _ => {
                let pile = if edge == Edge::Left { self.pile.0 } else { self.pile.1 };
                if i as f32 + 1.0 >= pile || i + 1 >= self.h.max(1) as usize {
                    return cap;
                }
                (edge, i + 1)
            }
        };
        let depth = self.depth_at(under.0, under.1 as f32);
        if depth >= self.cap_at(under.0, under.1) - 0.01 { cap } else { (depth - 1.0).clamp(0.0, cap) }
    }

    /// What holds top column `x`: its neighbor toward the nearer wall, or
    /// that wall's top row for a corner column.
    fn toward_wall(&self, x: usize) -> (Edge, usize) {
        let n = self.top.len();
        match x {
            0 => (Edge::Left, 0),
            _ if x + 1 == n => (Edge::Right, 0),
            _ if x < n / 2 => (Edge::Top, x - 1),
            _ => (Edge::Top, x + 1),
        }
    }

    /// Whether a flake on `edge` at `pos` would stay there: that row holds
    /// one more pixel. Otherwise it slides on.
    pub fn holds(&self, edge: Edge, pos: f32) -> bool {
        let n = match edge {
            Edge::Top => self.top.len(),
            _ => self.left.len(),
        };
        pos >= 0.0 && (pos as usize) < n && self.holds_up_to(edge, pos as usize) - self.depth_at(edge, pos) >= 1.0
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

    /// One flake stuck to `edge` at `pos`: exactly one pixel on that row (the
    /// flake slid there first, see `holds`). False when it missed the wall or
    /// that row is already at the cap.
    pub fn add_grain(&mut self, edge: Edge, pos: f32) -> bool {
        if pos < 0.0 {
            return false;
        }
        let cap = self.cap_at(edge, pos as usize);
        match self.edge_mut(edge).get_mut(pos as usize) {
            Some(d) if *d < cap => {
                *d = (*d + 1.0).min(cap);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::canvas::Canvas;

    /// The edges alone, drawn the way the scene draws them.
    fn draw(c: &mut Canvas, f: &Frost, outline: bool) {
        let snow = super::super::snow::Snow::new(c.w, 1.0);
        super::super::blanket::Blanket::default().draw(c, &snow, f, c.h, 0.0, outline);
    }

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
    fn creep_ices_the_corners_over_time_building_up_from_the_bottom() {
        let mut f = Frost::new(200, 100);
        for _ in 0..60 {
            f.creep(1.0, 0.2);
        }
        let mut c = Canvas::new(200, 100);
        draw(&mut c, &f, false);
        let (low, high) = (c.opaque_in(0, 80, 20, 20), c.opaque_in(0, 0, 20, 20));
        let center = c.opaque_in(90, 40, 20, 20);
        assert!(low > 0 && high == 0 && center == 0, "low corner {low} high corner {high} center {center}");
    }

    #[test]
    fn frost_is_drawn_near_edges_only() {
        let mut f = Frost::new(120, 80);
        f.set_pile(50.0, 50.0);
        f.burst(Edge::Right, 40.0, 30.0, 10.0);
        let mut c = Canvas::new(120, 80);
        draw(&mut c, &f, false);
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
        let full = f.depth_at(Edge::Left, 40.0);
        assert!(full <= f.max_depth && full >= f.max_depth * 0.8, "capped, a little below or at the max: {full}");
        assert!(!f.add_grain(Edge::Left, -1.0) && !f.add_grain(Edge::Right, 100.0), "off the wall");
    }

    #[test]
    fn wall_snow_is_solid_from_the_glass_to_its_surface_with_no_specks() {
        let mut f = Frost::new(120, 80);
        f.burst(Edge::Left, 40.0, 20.0, 8.0);
        f.burst(Edge::Top, 30.0, 12.0, 10.0);
        let mut c = Canvas::new(120, 80);
        draw(&mut c, &f, false);
        for y in 0..80 {
            let d = f.depth_at(Edge::Left, y as f32).round() as i32;
            for x in 0..d {
                assert!(c.opaque_in(x, y, 1, 1) == 1, "hole at ({x},{y}) inside a {d}px pile");
            }
        }
        // Nothing past the surface but the outline (the top's snow slid down the left wall).
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
        draw(&mut plain, &f, false);
        draw(&mut lined, &f, true);
        assert!(lined.opaque_in(0, 0, 120, 80) > plain.opaque_in(0, 0, 120, 80));
    }

    fn total(f: &Frost, e: Edge, n: i32) -> f32 {
        (0..n).map(|i| f.depth_at(e, i as f32)).sum()
    }

    #[test]
    fn a_wall_row_holds_snow_only_on_what_is_under_it() {
        let mut f = Frost::new(120, 80);
        f.set_pile(70.0, 70.0);
        assert!(f.holds(Edge::Left, 69.0), "right on the pile");
        assert!(!f.holds(Edge::Left, 10.0) && !f.holds(Edge::Left, 68.0), "nothing under it yet");
        f.add_grain(Edge::Left, 69.0);
        f.add_grain(Edge::Left, 69.0);
        assert!(f.holds(Edge::Left, 68.0), "on a drift two pixels deep");
        assert!(!f.holds(Edge::Left, 67.0), "but not on one pixel");
        while f.add_grain(Edge::Left, 69.0) {}
        f.add_grain(Edge::Left, 68.0);
        assert!(f.holds(Edge::Left, 68.0), "a row on a full one holds up to its own cap");
    }

    #[test]
    fn snow_sprayed_on_a_bare_wall_slides_down_onto_the_pile() {
        let mut f = Frost::new(120, 80);
        f.set_pile(70.0, 70.0);
        f.burst(Edge::Left, 10.0, 20.0, 4.0);
        assert!(total(&f, Edge::Left, 80) > 5.0);
        assert_eq!(total(&f, Edge::Left, 50), 0.0, "none stayed up where it hit");
        assert!(f.depth_at(Edge::Left, 69.0) >= 1.0, "it rests on the pile");
    }

    #[test]
    fn wall_snow_climbs_from_the_pile_thickest_at_the_bottom() {
        let mut f = Frost::new(120, 80);
        f.set_pile(70.0, 70.0);
        for i in 0..300 {
            f.burst(Edge::Right, (i * 37 % 70) as f32, 0.5, 2.0);
            f.settle();
        }
        let at = |y: i32| f.depth_at(Edge::Right, y as f32);
        assert!(at(69) > at(20) + 1.0, "thicker on the pile ({}) than up the wall ({})", at(69), at(20));
        assert!((0..69).all(|y| at(y) <= at(y + 1) + 1.0), "every row rests on the one under it");
    }

    #[test]
    fn the_top_only_ices_over_from_a_corner_the_wall_reached() {
        let mut f = Frost::new(120, 80);
        f.set_pile(70.0, 70.0);
        f.burst(Edge::Top, 60.0, 30.0, 10.0);
        assert_eq!(total(&f, Edge::Top, 120), 0.0, "with bare walls nothing holds on the top");
        assert!(total(&f, Edge::Left, 80) > 1.0 && total(&f, Edge::Right, 80) > 1.0, "it slid down both walls");

        let mut f = Frost::new(120, 80);
        f.set_pile(70.0, 70.0);
        for y in 0..80 {
            while f.add_grain(Edge::Left, y as f32) {}
        }
        f.burst(Edge::Top, 10.0, 30.0, 10.0);
        assert!(f.depth_at(Edge::Top, 2.0) >= 1.0, "the top holds where the left wall reached it");
        assert_eq!(f.depth_at(Edge::Top, 110.0), 0.0, "but not over the bare right wall");
    }
}
