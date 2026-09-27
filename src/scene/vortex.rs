//! The pause black hole: the finished frame is warped into a point (shrunk and
//! spun around it), so every sprite, drift and particle gets sucked in — and
//! spat back out on resume — without any of them knowing about it.
//! Also draws the magic orb that lives in a screen corner.

use crate::render::canvas::{CLEAR, Canvas, bayer, hex};

/// Warps `src` into `dst`. `k` = 1 is the untouched frame, `k` → 0 pulls
/// everything into `hole`; `swirl` rotates it (radians, stronger near the hole).
pub fn warp(src: &Canvas, dst: &mut Canvas, hole: (f32, f32), k: f32, swirl: f32) {
    dst.clear(CLEAR);
    if k <= 0.001 {
        return;
    }
    let (hx, hy) = hole;
    for y in 0..dst.h {
        for x in 0..dst.w {
            let (dx, dy) = (x as f32 + 0.5 - hx, y as f32 + 0.5 - hy);
            let dist = (dx * dx + dy * dy).sqrt();
            // Pixels near the hole spin more: a spiral, not a plain zoom.
            let a = swirl * (1.0 / (1.0 + dist * 0.02));
            let (s, c) = a.sin_cos();
            let (rx, ry) = (dx * c - dy * s, dx * s + dy * c);
            let (sx, sy) = ((hx + rx / k).floor() as i32, (hy + ry / k).floor() as i32);
            if let Some(p) = src.get(sx, sy).filter(|p| p[3] != 0) {
                dst.set(x, y, p);
            }
        }
    }
}

/// A black hole with a glowing, spinning accretion ring. `size` in art pixels.
pub fn draw_hole(c: &mut Canvas, (cx, cy): (f32, f32), size: f32, time: f32) {
    let r = size / 2.0;
    let ring = [hex(0x2a1650), hex(0x6a2cb8), hex(0xb46cff), hex(0xffd6ff)];
    c.glow(cx, cy, r * 2.4, 0.5, hex(0x3b1a6e));
    for y in (cy - r * 1.6) as i32..=(cy + r * 1.6) as i32 {
        for x in (cx - r * 1.6) as i32..=(cx + r * 1.6) as i32 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d < r * 0.55 {
                c.set(x, y, hex(0x05030d));
            } else if d < r * 1.6 {
                // Spiral arms rotating around the hole.
                let ang = dy.atan2(dx) + time * 2.5 - d * 0.35;
                let arm = (ang * 3.0).sin() * 0.5 + 0.5;
                let fade = 1.0 - (d - r * 0.55) / (r * 1.05);
                let v = arm * fade;
                if v > bayer(x, y) * 0.9 {
                    let idx = ((v * 4.0) as usize).min(3);
                    c.set(x, y, ring[idx]);
                }
            }
        }
    }
}

/// The corner orb: a pulsing crystal ball with orbiting sparkles.
/// `paused` shows the black hole sleeping inside it.
pub fn draw_orb(c: &mut Canvas, (cx, cy): (f32, f32), r: f32, time: f32, paused: bool) {
    if paused {
        draw_hole(c, (cx, cy), r * 1.2, time);
        return;
    }
    let pulse = 0.5 + 0.5 * (time * 2.2).sin();
    c.glow(cx, cy, r * 1.9, 0.35 + pulse * 0.2, hex(0x6a2cb8));
    let body = [hex(0x2a1650), hex(0x5a2fa0), hex(0x8f5cf0), hex(0xc9a6ff)];
    for y in (cy - r) as i32..=(cy + r) as i32 {
        for x in (cx - r) as i32..=(cx + r) as i32 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt() / r;
            if d > 1.0 {
                continue;
            }
            // Light from the top-left, a swirl of mist inside.
            let light = (1.0 - d) * 0.6 + (-(dx + dy) / (r * 2.0)) * 0.5;
            let mist = ((dx * 0.5 + time * 1.5).sin() + (dy * 0.6 - time).cos()) * 0.12;
            let v = (light + mist + 0.25).clamp(0.0, 0.999);
            c.set(x, y, body[(v * 4.0) as usize]);
        }
    }
    // Rim and highlight.
    for k in 0..48 {
        let a = k as f32 / 48.0 * std::f32::consts::TAU;
        c.dot(cx + a.cos() * r, cy + a.sin() * r, hex(0x1b0f3a));
    }
    c.dot(cx - r * 0.4, cy - r * 0.45, hex(0xffffff));
    c.dot(cx - r * 0.3, cy - r * 0.55, hex(0xffffff));
    // Two sparkles orbiting.
    for i in 0..2 {
        let a = time * 1.8 + i as f32 * std::f32::consts::PI;
        c.dot(cx + a.cos() * r * 1.35, cy + a.sin() * r * 0.5, hex(0xffe6ff));
    }
}

/// Pause animation timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Open,
    /// Trembling, then sucked in. `t` seconds in.
    Closing(f32),
    Closed,
    /// Spat back out.
    Opening(f32),
}

pub const TREMBLE: f32 = 0.5;
pub const CLOSE: f32 = 1.7;
pub const OPEN: f32 = 1.2;

impl Phase {
    pub fn step(self, dt: f32) -> Phase {
        match self {
            Phase::Closing(t) if t + dt >= CLOSE => Phase::Closed,
            Phase::Closing(t) => Phase::Closing(t + dt),
            Phase::Opening(t) if t + dt >= OPEN => Phase::Open,
            Phase::Opening(t) => Phase::Opening(t + dt),
            p => p,
        }
    }

    /// (k, swirl, tremble px) for the current moment.
    pub fn params(self) -> (f32, f32, i32) {
        let ease = |x: f32| x * x * (3.0 - 2.0 * x);
        match self {
            Phase::Open => (1.0, 0.0, 0),
            Phase::Closing(t) if t < TREMBLE => (1.0, 0.0, 1 + (t / TREMBLE * 2.0) as i32),
            Phase::Closing(t) => {
                let p = ease(((t - TREMBLE) / (CLOSE - TREMBLE)).clamp(0.0, 1.0));
                (1.0 - p, p * 9.0, 1)
            }
            Phase::Closed => (0.0, 9.0, 0),
            Phase::Opening(t) => {
                let p = ease((t / OPEN).clamp(0.0, 1.0));
                (p, (1.0 - p) * 9.0, 0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Canvas {
        let mut c = Canvas::new(60, 40);
        c.rect(0, 30, 60, 10, hex(0xffffff)); // snow along the bottom
        c.rect(5, 5, 4, 4, hex(0xff0000)); // a sprite in a corner
        c
    }

    #[test]
    fn untouched_warp_is_the_identity() {
        let src = frame();
        let mut dst = Canvas::new(60, 40);
        warp(&src, &mut dst, (30.0, 20.0), 1.0, 0.0);
        assert_eq!(src.bytes(), dst.bytes());
    }

    #[test]
    fn closing_pulls_everything_toward_the_hole() {
        let src = frame();
        let mut dst = Canvas::new(60, 40);
        warp(&src, &mut dst, (50.0, 10.0), 0.2, 3.0);
        let total = dst.opaque_in(0, 0, 60, 40);
        let near = dst.opaque_in(38, 0, 22, 22);
        assert!(total > 0 && total < src.opaque_in(0, 0, 60, 40));
        assert_eq!(total, near, "everything is squeezed around the hole");
        warp(&src, &mut dst, (50.0, 10.0), 0.0, 9.0);
        assert_eq!(dst.opaque_in(0, 0, 60, 40), 0, "fully swallowed");
    }

    #[test]
    fn timeline_trembles_closes_and_reopens() {
        let mut p = Phase::Closing(0.0);
        assert!(p.params().2 >= 1, "trembles first");
        for _ in 0..60 {
            p = p.step(1.0 / 30.0);
        }
        assert_eq!(p, Phase::Closed);
        assert_eq!(p.params().0, 0.0);
        p = Phase::Opening(0.0);
        for _ in 0..40 {
            p = p.step(1.0 / 30.0);
        }
        assert_eq!(p, Phase::Open);
        assert_eq!(p.params(), (1.0, 0.0, 0));
    }

    #[test]
    fn hole_and_orb_draw_something_centered() {
        let mut c = Canvas::new(40, 40);
        draw_hole(&mut c, (20.0, 20.0), 12.0, 0.3);
        assert_eq!(c.get(20, 20), Some(hex(0x05030d)), "black core");
        let mut o = Canvas::new(40, 40);
        draw_orb(&mut o, (20.0, 20.0), 8.0, 0.3, false);
        assert!(o.opaque_in(12, 12, 16, 16) > 150);
    }
}
