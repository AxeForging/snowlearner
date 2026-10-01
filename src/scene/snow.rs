//! The snow pile along the bottom of the screen: one height per column.
//! Ice cubes and summons add to it, speaking melts it, campfires thaw it locally.

use super::rng::hash01;
use crate::render::canvas::{Canvas, Rgba, bayer, hex};

/// Dark rim drawn past a pile's surface over the desktop, so it shows on light windows.
pub const OUTLINE: Rgba = hex(0x5c6fb0);

/// Color of a snow pixel `depth` px under the pile's surface: white on top,
/// bluer deeper down, dithered between shades.
pub fn shade(depth: i32, x: i32, y: i32) -> Rgba {
    if depth == 0 {
        return hex(0xffffff);
    }
    let body = [hex(0xeef5ff), hex(0xd4e2fb), hex(0xb3c3ec), hex(0x93a6da)];
    let s = (depth as f32 / 10.0).min(3.0);
    let i = s.floor() as usize;
    body[(i + usize::from(s - i as f32 > bayer(x, y))).min(3)]
}

pub struct Snow {
    heights: Vec<f32>,
    cap: f32,
}

impl Snow {
    pub fn new(width: i32, cap: f32) -> Self {
        Snow { heights: vec![0.0; width.max(1) as usize], cap }
    }

    /// Keeps the existing pile, stretched to the new width.
    pub fn resize(&mut self, width: i32, cap: f32) {
        let old = std::mem::take(&mut self.heights);
        let n = width.max(1) as usize;
        self.heights = (0..n).map(|i| if old.is_empty() { 0.0 } else { old[i * old.len() / n].min(cap) }).collect();
        self.cap = cap;
    }

    pub fn width(&self) -> usize {
        self.heights.len()
    }

    pub fn cap(&self) -> f32 {
        self.cap
    }

    pub fn height_at(&self, x: f32) -> f32 {
        let i = (x.round() as i64).clamp(0, self.heights.len() as i64 - 1) as usize;
        self.heights[i]
    }

    /// Adds `amount` px of snow centered on `x`, spread over ±`spread` columns.
    pub fn add(&mut self, x: f32, amount: f32, spread: f32) {
        let spread = spread.max(1.0);
        let (lo, hi) = ((x - spread * 2.0).floor() as i64, (x + spread * 2.0).ceil() as i64);
        for i in lo.max(0)..=hi.min(self.heights.len() as i64 - 1) {
            let d = (i as f32 - x) / spread;
            let h = &mut self.heights[i as usize];
            *h = (*h + amount * (-d * d).exp()).min(self.cap);
        }
    }

    /// One settled snowflake: exactly one pixel on the column it landed on.
    /// False when it missed the pile or the column is already at the cap.
    pub fn add_grain(&mut self, x: f32) -> bool {
        if x < 0.0 {
            return false;
        }
        match self.heights.get_mut(x as usize) {
            Some(h) if *h < self.cap => {
                *h = (*h + 1.0).min(self.cap);
                true
            }
            _ => false,
        }
    }

    /// Adds a thin even layer.
    pub fn dust(&mut self, amount: f32) {
        for h in &mut self.heights {
            *h = (*h + amount).min(self.cap);
        }
    }

    /// Removes `fraction` of all snow. Returns sample columns (x, height before)
    /// where it melted, for steam effects.
    pub fn melt(&mut self, fraction: f32) -> Vec<(f32, f32)> {
        let f = fraction.clamp(0.0, 1.0);
        let mut puffs = Vec::new();
        for (i, h) in self.heights.iter_mut().enumerate() {
            if *h > 1.0 && i % 6 == 0 {
                puffs.push((i as f32, *h));
            }
            *h *= 1.0 - f;
        }
        puffs
    }

    /// Local thaw (campfire): up to `rate` px/s at the center, fading to `radius`.
    pub fn thaw(&mut self, x: f32, radius: f32, rate: f32, dt: f32) {
        let (lo, hi) = ((x - radius).floor() as i64, (x + radius).ceil() as i64);
        for i in lo.max(0)..=hi.min(self.heights.len() as i64 - 1) {
            let k = 1.0 - ((i as f32 - x).abs() / radius).min(1.0);
            let h = &mut self.heights[i as usize];
            *h = (*h - rate * k * dt).max(0.0);
        }
    }

    /// Lets steep steps slide so piles look like snow, not spikes.
    pub fn settle(&mut self) {
        for i in 1..self.heights.len() {
            let (a, b) = (self.heights[i - 1], self.heights[i]);
            let diff = a - b;
            if diff.abs() > 1.0 {
                let moved = diff * 0.35;
                self.heights[i - 1] -= moved;
                self.heights[i] += moved;
            }
        }
    }

    /// 0 = bare, 1 = every column at the cap.
    pub fn fill(&self) -> f32 {
        self.heights.iter().sum::<f32>() / (self.heights.len() as f32 * self.cap)
    }

    /// `outline` draws a darker rim on the surface so the pile stays visible
    /// over light desktop windows in overlay mode.
    pub fn draw(&self, c: &mut Canvas, ground_y: i32, time: f32, outline: bool) {
        for (x, &h) in self.heights.iter().enumerate() {
            let x = x as i32;
            let hh = h.round() as i32;
            if hh <= 0 {
                continue;
            }
            let top = ground_y - hh;
            if outline {
                c.set(x, top - 1, OUTLINE);
            }
            for y in top..ground_y {
                c.set(x, y, shade(y - top, x, y));
            }
            // Twinkling ice crystals near the surface.
            let n = hash01(x, 91);
            if n > 0.93 && ((time * 2.0 + n * 10.0).sin() > 0.6) {
                c.set(x, top + 1, hex(0xffffff));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grain_adds_exactly_one_pixel_to_its_column_up_to_the_cap() {
        let mut s = Snow::new(10, 3.0);
        assert!(s.add_grain(4.7));
        assert_eq!(s.height_at(4.0), 1.0);
        assert_eq!(s.fill(), 1.0 / 30.0, "only that column");
        s.add_grain(4.2);
        s.add_grain(4.9);
        assert!(!s.add_grain(4.5), "full column takes no more");
        assert_eq!(s.height_at(4.0), 3.0);
        assert!(!s.add_grain(-3.0) && !s.add_grain(10.0), "off the pile");
    }

    #[test]
    fn add_piles_snow_around_the_impact_point() {
        let mut s = Snow::new(100, 40.0);
        s.add(50.0, 6.0, 4.0);
        assert!(s.height_at(50.0) > 5.0);
        assert!(s.height_at(50.0) > s.height_at(56.0));
        assert_eq!(s.height_at(10.0), 0.0);
    }

    #[test]
    fn snow_never_exceeds_the_cap() {
        let mut s = Snow::new(20, 10.0);
        for _ in 0..100 {
            s.add(10.0, 8.0, 3.0);
        }
        assert!((s.height_at(10.0) - 10.0).abs() < 1e-4);
        assert!(s.fill() <= 1.0);
    }

    #[test]
    fn melt_removes_the_requested_fraction_and_reports_puffs() {
        let mut s = Snow::new(60, 40.0);
        s.dust(20.0);
        let before = s.fill();
        let puffs = s.melt(0.5);
        assert!((s.fill() - before * 0.5).abs() < 1e-4);
        assert!(!puffs.is_empty());
        s.melt(5.0); // clamps to 100%
        assert_eq!(s.fill(), 0.0);
    }

    #[test]
    fn thaw_is_local_and_never_negative() {
        let mut s = Snow::new(100, 40.0);
        s.dust(5.0);
        s.thaw(50.0, 10.0, 100.0, 1.0);
        assert_eq!(s.height_at(50.0), 0.0);
        assert_eq!(s.height_at(80.0), 5.0);
    }

    #[test]
    fn settle_smooths_spikes_without_losing_snow() {
        let mut s = Snow::new(10, 40.0);
        s.add(5.0, 30.0, 1.0);
        let total_before: f32 = (0..10).map(|i| s.height_at(i as f32)).sum();
        let peak_before = s.height_at(5.0);
        for _ in 0..20 {
            s.settle();
        }
        let total_after: f32 = (0..10).map(|i| s.height_at(i as f32)).sum();
        assert!(s.height_at(5.0) < peak_before);
        assert!((total_before - total_after).abs() < 1e-3);
    }

    #[test]
    fn resize_keeps_the_pile_shape() {
        let mut s = Snow::new(100, 40.0);
        s.add(75.0, 10.0, 3.0);
        s.resize(200, 40.0);
        assert!(s.height_at(150.0) > 5.0);
        assert_eq!(s.height_at(20.0), 0.0);
    }
}
