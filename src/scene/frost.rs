//! Window frost creeping in from the top, left and right edges of the screen.
//! Summoned friends burst frost onto the nearest edge; speaking melts it.

use super::rng::hash01;
use crate::render::canvas::{Canvas, bayer, hex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Left,
    Right,
}

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
        let max_depth = (w.min(h) as f32 * 0.22).max(8.0);
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
        self.max_depth = (w.min(h) as f32 * 0.22).max(8.0);
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

    /// Adds a patch of frost `strength` px deep on `edge`, centered at `pos`
    /// (x for the top edge, y for the sides).
    pub fn burst(&mut self, edge: Edge, pos: f32, strength: f32, radius: f32) {
        let max = self.max_depth;
        let v = self.edge_mut(edge);
        let (lo, hi) = ((pos - radius * 2.0).floor() as i64, (pos + radius * 2.0).ceil() as i64);
        for i in lo.max(0)..=hi.min(v.len() as i64 - 1) {
            let d = (i as f32 - pos) / radius;
            let cell = &mut v[i as usize];
            *cell = (*cell + strength * (-d * d).exp()).min(max);
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

    pub fn draw(&self, c: &mut Canvas) {
        let w = self.w;
        for (i, &d) in self.top.iter().enumerate() {
            for k in 0..d.ceil() as i32 {
                paint(c, i as i32, 0, k, d, i as i32, k);
            }
        }
        for (i, &d) in self.left.iter().enumerate() {
            for k in 0..d.ceil() as i32 {
                paint(c, i as i32, 1, k, d, k, i as i32);
            }
        }
        for (i, &d) in self.right.iter().enumerate() {
            for k in 0..d.ceil() as i32 {
                paint(c, i as i32, 2, k, d, w - 1 - k, i as i32);
            }
        }
    }
}

/// One frost pixel `k` px into the screen at position `i` along an edge.
/// Depth varies with coarse + fine noise so the edge grows fern-like spikes.
fn paint(c: &mut Canvas, i: i32, salt: i32, k: i32, depth: f32, x: i32, y: i32) {
    let spike = 0.55 + 0.35 * hash01(i / 3, 7 + salt * 13) + 0.25 * hash01(i, 3 + salt * 13);
    let d = depth * spike;
    if (k as f32) >= d {
        return;
    }
    let t = k as f32 / d; // 0 at the edge → 1 at the frost tip
    let density = 1.05 - t * 0.9 + (hash01(x, y) - 0.5) * 0.35;
    if density > bayer(x, y) {
        let col = if t < 0.3 {
            hex(0xffffff)
        } else if t < 0.75 {
            hex(0xc8f4ff)
        } else {
            hex(0x8ad8f5)
        };
        c.set(x, y, col);
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
        f.draw(&mut c);
        let corner = c.opaque_in(0, 0, 20, 20);
        let center = c.opaque_in(90, 40, 20, 20);
        assert!(corner > 0 && center == 0, "corner {corner} center {center}");
    }

    #[test]
    fn frost_is_drawn_near_edges_only() {
        let mut f = Frost::new(120, 80);
        f.burst(Edge::Right, 40.0, 30.0, 10.0);
        let mut c = Canvas::new(120, 80);
        f.draw(&mut c);
        assert!(c.opaque_in(100, 30, 20, 20) > 0);
        assert_eq!(c.opaque_in(0, 30, 40, 20), 0);
    }
}
