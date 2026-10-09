//! The snow pile along the bottom of the screen: one height per column.
//! Ice cubes and summons add to it, speaking melts it; a campfire melts a hollow
//! around itself but the water refreezes on its rims, so only speaking melts it for good.

use crate::render::canvas::{Rgba, bayer, hex};

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
    frozen: bool,
}

impl Snow {
    pub fn new(width: i32, cap: f32) -> Self {
        Snow { heights: vec![0.0; width.max(1) as usize], cap, frozen: false }
    }

    /// Keeps the existing pile, stretched to the new width.
    pub fn resize(&mut self, width: i32, cap: f32) {
        let old = std::mem::take(&mut self.heights);
        let n = width.max(1) as usize;
        self.heights = (0..n).map(|i| if old.is_empty() { 0.0 } else { old[i * old.len() / n].min(cap) }).collect();
        self.cap = cap;
    }

    /// A frozen pile takes no more snow and keeps its shape, peaks and all;
    /// it can still melt.
    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
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
        if self.frozen {
            return;
        }
        let spread = spread.max(1.0);
        let (lo, hi) = ((x - spread * 2.0).floor() as i64, (x + spread * 2.0).ceil() as i64);
        for i in lo.max(0)..=hi.min(self.heights.len() as i64 - 1) {
            let d = (i as f32 - x) / spread;
            let h = &mut self.heights[i as usize];
            *h = (*h + amount * (-d * d).exp()).min(self.cap);
        }
    }

    /// One settled snowflake: exactly one pixel on the column it landed on.
    /// False when it missed the pile, the column is already at the cap or the
    /// pile is frozen.
    pub fn add_grain(&mut self, x: f32) -> bool {
        if x < 0.0 || self.frozen {
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
        if self.frozen {
            return;
        }
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

    /// Local thaw (a fireball landing): up to `rate` px/s at the center, fading to `radius`.
    pub fn thaw(&mut self, x: f32, radius: f32, rate: f32, dt: f32) {
        let (lo, hi) = ((x - radius).floor() as i64, (x + radius).ceil() as i64);
        for i in lo.max(0)..=hi.min(self.heights.len() as i64 - 1) {
            let k = 1.0 - ((i as f32 - x).abs() / radius).min(1.0);
            let h = &mut self.heights[i as usize];
            *h = (*h - rate * k * dt).max(0.0);
        }
    }

    /// A campfire's hollow: melts up to `rate` px/s at `x`, fading to `radius`,
    /// and the water refreezes on the rims just outside it — the pile keeps
    /// its snow (rims at the cap pass it further out).
    pub fn hollow(&mut self, x: f32, radius: f32, rate: f32, dt: f32) {
        let n = self.heights.len() as i64;
        let (lo, hi) = ((x - radius).floor() as i64, (x + radius).ceil() as i64);
        let mut melted = 0.0;
        for i in lo.max(0)..=hi.min(n - 1) {
            let k = 1.0 - ((i as f32 - x).abs() / radius).min(1.0);
            let h = &mut self.heights[i as usize];
            let m = (rate * k * dt).min(*h);
            *h -= m;
            melted += m;
        }
        for (step, mut i) in [(-1, lo - 1), (1, hi + 1)] {
            let mut water = melted / 2.0;
            while water > 0.0 && (0..n).contains(&i) {
                let h = &mut self.heights[i as usize];
                let put = (self.cap - *h).max(0.0).min(water);
                *h += put;
                water -= put;
                i += step;
            }
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

    /// Mean height of the pile, in pixels.
    pub fn mean(&self) -> f32 {
        self.heights.iter().sum::<f32>() / self.heights.len() as f32
    }

    /// 0 = bare, 1 = every column at the cap.
    pub fn fill(&self) -> f32 {
        self.heights.iter().sum::<f32>() / (self.heights.len() as f32 * self.cap)
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

    #[test]
    fn a_campfire_melts_a_hollow_and_the_water_freezes_on_its_rims() {
        let mut s = Snow::new(100, 30.0);
        for x in 0..100 {
            for _ in 0..10 {
                s.add_grain(x as f32);
            }
        }
        let before = s.fill();
        for _ in 0..30 {
            s.hollow(50.0, 10.0, 3.0, 0.1);
        }
        assert!(s.height_at(50.0) < 9.0, "a hollow under the fire: {}", s.height_at(50.0));
        assert!(s.height_at(39.0) > 10.0 && s.height_at(61.0) > 10.0, "raised rims on both sides");
        assert!((s.fill() - before).abs() < 1e-5, "the pile keeps its snow");
    }

    #[test]
    fn a_frozen_pile_takes_no_more_snow_but_still_melts() {
        let mut s = Snow::new(50, 30.0);
        s.add(25.0, 10.0, 3.0);
        let before = s.fill();
        s.set_frozen(true);
        assert!(!s.add_grain(25.0));
        s.add(10.0, 10.0, 3.0);
        s.dust(2.0);
        assert_eq!(s.fill(), before, "it keeps its shape");
        s.melt(0.5);
        assert!(s.fill() < before, "but a right answer still melts it");
    }
}
