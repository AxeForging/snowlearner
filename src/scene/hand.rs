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

pub const MAGE_HELD: &[&str] = &[
    "Me solta, aprendiz!",
    "Isso é humilhante!",
    "Eu sou um MAGO, não um brinquedo!",
    "Vou congelar sua tela inteira!",
    "Meu chapéu! Cuidado com o chapéu!",
];
pub const MAGE_LANDED: &[&str] = &["Hmpf!", "Você vai pagar por isso!", "Ai... minha dignidade."];
pub const WARRIOR_HELD: &[&str] = &[
    "Ei... isso é constrangedor...",
    "Me coloca no chão, por favor...",
    "Todo mundo tá olhando...",
    "Eu tenho medo de altura!",
];
pub const WARRIOR_LANDED: &[&str] = &["Ufa... obrigado?", "Chão, doce chão.", "Não conta pra ninguém, tá?"];

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
    fn every_line_renders_in_the_pixel_font() {
        for l in MAGE_HELD.iter().chain(MAGE_LANDED).chain(WARRIOR_HELD).chain(WARRIOR_LANDED) {
            assert!(crate::render::font::supports(l), "{l}");
        }
    }
}
