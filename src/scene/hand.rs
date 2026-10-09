//! The magic hand: hold Ctrl+Alt (or `snowlearner grab`) to pick the mage or
//! the warrior up, carry them around and drop them into the snow.

use crate::render::canvas::{Canvas, Rgba, hex};

const PAL: &[(char, Rgba)] = &[('O', hex(0x3b1a6e)), ('P', hex(0xb46cff)), ('L', hex(0xe6d0ff)), ('W', hex(0xffffff))];

const OPEN: &[&str] = &[
    "..O.O.O..",
    ".OPOPOPO.",
    ".OPOPOPO.",
    ".OPOPOPOO",
    "OOPPPPPPO",
    "OPLPPPPO.",
    "OPPPPPPO.",
    ".OPPPPO..",
    "..OOOO...",
];

const CLOSED: &[&str] = &[
    ".........",
    "..OOOOO..",
    ".OPLPLPO.",
    "OPPPPPPPO",
    "OPLPPPPPO",
    "OPPPPPPO.",
    ".OPPPPO..",
    "..OOOO...",
    ".........",
];

/// Who is in the hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Mage,
    Warrior,
}

/// A character off the ground: held, or falling after release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lift {
    /// Feet position while lifted.
    pub y: f32,
    pub vy: f32,
    pub held: bool,
    /// Seconds until the next complaint while held.
    pub talk_in: f32,
}

impl Lift {
    pub fn new(y: f32) -> Lift {
        Lift { y, vy: 0.0, held: true, talk_in: 2.5 }
    }

    /// Falls toward `ground`; returns true on the frame it lands.
    pub fn fall(&mut self, dt: f32, ground: f32) -> bool {
        if self.held {
            return false;
        }
        self.vy += 420.0 * dt;
        self.y += self.vy * dt;
        if self.y >= ground {
            self.y = ground;
            return true;
        }
        false
    }
}

pub fn draw_hand(c: &mut Canvas, x: f32, y: f32, closed: bool, time: f32) {
    let glow = 0.25 + 0.1 * (time * 5.0).sin();
    c.glow(x, y, 7.0, glow, hex(0x6a2cb8));
    let rows = if closed { CLOSED } else { OPEN };
    c.sprite(rows, PAL, x.round() as i32 - 4, y.round() as i32 - 4, false);
    // Sparkle orbiting the hand.
    let a = time * 6.0;
    c.dot(x + a.cos() * 6.0, y + a.sin() * 6.0, hex(0xffffff));
}

/// The warrior's embarrassed pose while held: blushing cheeks and index
/// fingers poking together in front of his chest (👉👈). `top` is the sprite top.
pub fn draw_shy(c: &mut Canvas, x: i32, top: i32, time: f32) {
    let blush = hex(0xff7aa8);
    for (dx, dy) in [(3, 5), (4, 6), (8, 5), (7, 6)] {
        c.set(x + dx, top + dy, blush);
    }
    // Fingertips tap: apart, touch, apart…
    let gap = if (time * 4.0) as i32 % 2 == 0 { 1 } else { 0 };
    let (skin, edge) = (hex(0xf2c29b), hex(0x7a4a2e));
    let y = top + 9;
    for (fx, dir) in [(5 - gap, -1), (6 + gap, 1)] {
        for i in 0..3 {
            c.set(fx + x + dir * i, y, skin);
            c.set(fx + x + dir * i, y + 1, edge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_released_character_falls_and_lands_on_the_ground() {
        let mut l = Lift::new(20.0);
        assert!(!l.fall(0.1, 100.0), "held characters don't fall");
        l.held = false;
        let mut landed = false;
        for _ in 0..60 {
            landed |= l.fall(1.0 / 30.0, 100.0);
        }
        assert!(landed);
        assert_eq!(l.y, 100.0);
    }

    #[test]
    fn the_warrior_gets_shy_fingers_in_speech_and_in_his_pose() {
        use crate::lang::{Lines, Native};
        for n in Native::ALL {
            assert!(Lines::WarriorHeld.get(n).iter().any(|l| l.contains("👉👈")), "{n:?}");
            let mage = Lines::MageHeld.get(n).iter().chain(Lines::MageLanded.get(n));
            assert!(!mage.clone().any(|l| l.contains('👉')), "the mage is not shy ({n:?})");
        }
        let tip = |t: f32| {
            let mut c = Canvas::new(20, 20);
            draw_shy(&mut c, 0, 0, t);
            c.get(5, 9).is_some_and(|p| p[3] > 0)
        };
        assert!(!tip(0.0), "fingers apart");
        assert!(tip(0.3), "fingertips touching");
    }
}
