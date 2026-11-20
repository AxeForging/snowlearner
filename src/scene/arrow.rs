//! The warrior's arrow (#118): on a right answer, a bat out of sword reach is
//! shot down. The arrow flies an arc from his bow to the bat in `FLIGHT_S`,
//! following the bat while it flies; the scene kills the bat when it lands.
//! It never touches the snow.

use crate::render::canvas::{Canvas, hex};

/// Time from the bow to the bat (s).
pub const FLIGHT_S: f32 = 0.45;
/// How high the arc rises over the straight line: a base plus a share of the distance (px).
const ARC: (f32, f32) = (6.0, 0.15);
/// Shaft length (px), tip included.
const SHAFT: i32 = 8;

pub struct Arrow {
    from: (f32, f32),
    /// Where it lands: the bat it follows, re-aimed by the scene each frame.
    pub to: (f32, f32),
    t: f32,
    done: bool,
}

impl Arrow {
    pub fn new(from: (f32, f32), to: (f32, f32)) -> Self {
        Arrow { from, to, t: 0.0, done: false }
    }

    /// Follows the bat to `to`.
    pub fn aim(&mut self, to: (f32, f32)) {
        self.to = to;
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// Point on the arc at `k` (0 at the bow, 1 at the bat).
    fn at(&self, k: f32) -> (f32, f32) {
        let k = k.clamp(0.0, 1.0);
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let rise = (ARC.0 + dx.hypot(dy) * ARC.1) * 4.0 * k * (1.0 - k);
        (self.from.0 + dx * k, self.from.1 + dy * k - rise)
    }

    /// Where the tip is now.
    pub fn tip(&self) -> (f32, f32) {
        self.at(self.t / FLIGHT_S)
    }

    /// Advances; true on the frame it lands.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.done {
            return false;
        }
        self.t += dt;
        self.done = self.t >= FLIGHT_S;
        self.done
    }

    /// A shaft along the flight with a white tip and red fletching.
    pub fn draw(&self, c: &mut Canvas) {
        if self.done {
            return;
        }
        let k = self.t / FLIGHT_S;
        let (p, q) = (self.at(k), self.at(k - 0.1));
        let (mut ux, mut uy) = (p.0 - q.0, p.1 - q.1);
        if ux.hypot(uy) < 0.01 {
            (ux, uy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        }
        let len = ux.hypot(uy).max(0.01);
        let (ux, uy) = (ux / len, uy / len);
        let point = |i: i32| ((p.0 - ux * i as f32).round() as i32, (p.1 - uy * i as f32).round() as i32);
        for i in 1..SHAFT {
            let (x, y) = point(i);
            c.set(x, y, hex(0x8b5a2b));
        }
        let (x, y) = point(0);
        c.set(x, y, hex(0xffffff));
        let (x, y) = point(SHAFT);
        let (nx, ny) = (-uy.round() as i32, ux.round() as i32);
        c.set(x + nx, y + ny, hex(0xd8403a));
        c.set(x - nx, y - ny, hex(0xd8403a));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fly(a: &mut Arrow, seconds: f32) -> usize {
        (0..(seconds * 60.0) as i32).filter(|_| a.step(1.0 / 60.0)).count()
    }

    #[test]
    fn it_lands_once_on_the_bat_after_its_flight() {
        let mut a = Arrow::new((10.0, 50.0), (90.0, 30.0));
        assert_eq!(fly(&mut a, FLIGHT_S * 0.5), 0, "still flying");
        assert_eq!(fly(&mut a, FLIGHT_S), 1, "lands once");
        assert!(a.done());
        assert_eq!(fly(&mut a, 1.0), 0);
    }

    #[test]
    fn it_flies_an_arc_above_the_straight_line() {
        let mut a = Arrow::new((10.0, 50.0), (90.0, 50.0));
        fly(&mut a, FLIGHT_S * 0.5);
        let (x, y) = a.tip();
        assert!((x - 50.0).abs() < 5.0, "halfway: {x}");
        assert!(y < 50.0 - ARC.0, "rises over the line: {y}");
    }

    #[test]
    fn it_follows_the_bat_it_aims_at() {
        let mut a = Arrow::new((10.0, 50.0), (90.0, 30.0));
        fly(&mut a, FLIGHT_S * 0.5);
        a.aim((80.0, 34.0));
        while !a.step(1.0 / 60.0) {}
        assert_eq!(a.at(1.0), (80.0, 34.0));
    }

    #[test]
    fn it_is_drawn_in_one_bit_and_gone_once_landed() {
        let mut a = Arrow::new((10.0, 50.0), (90.0, 30.0));
        fly(&mut a, FLIGHT_S * 0.3);
        let mut c = Canvas::new(120, 80);
        a.draw(&mut c);
        assert!(c.bytes().chunks(4).all(|p| p[3] == 0 || p[3] == 255), "1-bit alpha");
        assert!(c.bytes().chunks(4).filter(|p| p[3] == 255).count() >= SHAFT as usize, "shaft, tip, fletching");
        fly(&mut a, FLIGHT_S);
        let mut c = Canvas::new(120, 80);
        a.draw(&mut c);
        assert_eq!(c.bytes().chunks(4).filter(|p| p[3] == 255).count(), 0);
    }
}
