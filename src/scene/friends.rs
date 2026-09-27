//! The mage's summoned friends. They pop out of the snow, wave, and burst frost
//! onto the screen edges before sinking back.

use crate::render::canvas::{Canvas, Rgba, hex};

const PAL: &[(char, Rgba)] = &[
    ('H', hex(0x1b1b2a)),
    ('h', hex(0x3a3a55)),
    ('W', hex(0xf4f8ff)),
    ('w', hex(0xc8d6f0)),
    ('E', hex(0x141028)),
    ('N', hex(0xff8a2a)),
    ('S', hex(0xd93a2a)),
    ('s', hex(0x9a2020)),
    ('K', hex(0x1d2033)),
    ('O', hex(0xffa53a)),
];

const SNOWMAN: &[&str] = &[
    "....HHH....",
    "....HHH....",
    "...hHHHh...",
    "...WWWWw...",
    "..WWEWEWw..",
    "..WWWNNWw..",
    "...WWWWw...",
    "..SSSSSSs..",
    ".WWWWsWWWw.",
    "WWWWWEWWWww",
    "WWWWWWWWWww",
    ".WWWWEWWWw.",
    "WWWWWWWWWww",
    "WWWWWWWWWww",
    ".WWWWWWWww.",
];

const PENGUIN: &[&str] = &[
    "...KKK...",
    "..KKKKK..",
    "..KWKWK..",
    "..KKOKK..",
    ".KKWWWKK.",
    ".KWWWWWK.",
    "KKWWWWWKK",
    ".KWWWWWK.",
    ".KWWWWWK.",
    "..KWWWK..",
    "..OO.OO..",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Snowman,
    Penguin,
}

impl Kind {
    fn sprite(self) -> &'static [&'static str] {
        match self {
            Kind::Snowman => SNOWMAN,
            Kind::Penguin => PENGUIN,
        }
    }

    pub fn height(self) -> i32 {
        self.sprite().len() as i32
    }

    pub fn width(self) -> i32 {
        self.sprite()[0].len() as i32
    }

    pub fn shout(self) -> &'static str {
        match self {
            Kind::Snowman => "BRRR!",
            Kind::Penguin => "QUÁ!",
        }
    }
}

const RISE: f32 = 0.7;
const WAVE: f32 = 1.2;
const BURST: f32 = 0.8;
const SINK: f32 = 0.7;

pub enum Event {
    /// Spray frost now, from this point.
    Burst { x: f32, y: f32 },
}

pub struct Friend {
    pub kind: Kind,
    pub x: f32,
    pub t: f32,
    burst_done: bool,
}

impl Friend {
    pub fn new(kind: Kind, x: f32) -> Self {
        Friend { kind, x, t: 0.0, burst_done: false }
    }

    pub fn alive(&self) -> bool {
        self.t < RISE + WAVE + BURST + SINK
    }

    pub fn step(&mut self, dt: f32, feet_y: f32) -> Option<Event> {
        self.t += dt;
        if !self.burst_done && self.t > RISE + WAVE {
            self.burst_done = true;
            return Some(Event::Burst {
                x: self.x + self.kind.width() as f32 / 2.0,
                y: feet_y - self.kind.height() as f32,
            });
        }
        None
    }

    /// How far out of the snow it is, 0..1.
    fn emerged(&self) -> f32 {
        let end = RISE + WAVE + BURST;
        if self.t < RISE {
            self.t / RISE
        } else if self.t < end {
            1.0
        } else {
            (1.0 - (self.t - end) / SINK).max(0.0)
        }
    }

    pub fn draw(&self, c: &mut Canvas, feet_y: f32, time: f32) {
        let h = self.kind.height();
        let shown = (h as f32 * self.emerged()).round() as i32;
        if shown <= 0 {
            return;
        }
        let x = self.x.round() as i32;
        let hop = if self.kind == Kind::Penguin && self.t > RISE && self.t < RISE + WAVE {
            -(((time * 12.0).sin().abs()) * 2.0).round() as i32
        } else {
            0
        };
        let top = feet_y.round() as i32 - shown + hop;
        // Only the emerged rows are drawn, so it rises out of the snow.
        let rows = &self.kind.sprite()[..shown as usize];
        c.sprite(rows, PAL, x, top, false);

        if self.kind == Kind::Snowman && shown > 9 {
            // Stick arms, waving during the wave phase.
            let wave =
                if self.t > RISE && self.t < RISE + WAVE { ((time * 10.0).sin() * 2.0).round() as i32 } else { 0 };
            let arm_y = top + 9;
            for i in 0..4 {
                c.set(x - 1 - i, arm_y - i / 2 + if i == 3 { wave } else { 0 }, hex(0x6b3f1d));
                c.set(x + 11 + i, arm_y - i + if i >= 2 { -wave } else { 0 }, hex(0x6b3f1d));
            }
        }
        if self.t > RISE + WAVE * 0.4 && self.t < RISE + WAVE + BURST {
            crate::scene::warrior::draw_bubble(c, x + self.kind.width() / 2, top - 2, self.kind.shout());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_exactly_once_then_leaves() {
        let mut f = Friend::new(Kind::Snowman, 40.0);
        let mut bursts = 0;
        for _ in 0..(5.0 * 60.0) as i32 {
            if f.step(1.0 / 60.0, 100.0).is_some() {
                bursts += 1;
            }
        }
        assert_eq!(bursts, 1);
        assert!(!f.alive());
    }

    #[test]
    fn rises_out_of_the_snow_progressively() {
        let mut early = Canvas::new(40, 40);
        let mut later = Canvas::new(40, 40);
        let mut f = Friend::new(Kind::Penguin, 10.0);
        f.step(0.2, 40.0);
        f.draw(&mut early, 40.0, 0.0);
        f.step(0.6, 40.0);
        f.draw(&mut later, 40.0, 0.0);
        assert!(early.opaque_in(0, 0, 40, 40) < later.opaque_in(0, 0, 40, 40));
    }
}
