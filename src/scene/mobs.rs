//! Small frost monsters that wander in from the screen edges and go after the
//! warrior. He fights back with his sword; each bite chills him, each win warms him.

use crate::render::canvas::{Canvas, Rgba, bayer, hex};

const PAL: &[(char, Rgba)] = &[
    ('W', hex(0xffffff)),
    ('L', hex(0xc8f4ff)),
    ('C', hex(0x8ad8f5)),
    ('D', hex(0x4ea2d8)),
    ('d', hex(0x2a5a9a)),
    ('E', hex(0x141028)),
    ('R', hex(0xff6b8a)),
];

/// Everything white: the hit flash.
const HURT_PAL: &[(char, Rgba)] = &[
    ('W', hex(0xffffff)),
    ('L', hex(0xffffff)),
    ('C', hex(0xffffff)),
    ('D', hex(0xffffff)),
    ('d', hex(0xffffff)),
    ('E', hex(0xffffff)),
    ('R', hex(0xffffff)),
];

const SLIME: &[&str] = &["..LLLL..", ".LWCCCD.", "LCECCECD", "LCCCCCCD", "DCCRRCDd", ".dddddd."];
const BAT_UP: &[&str] = &["D.......D", "DD.CCC.DD", ".DDCECDD.", "...CCC...", "....d...."];
const BAT_DOWN: &[&str] = &["...CCC...", ".DDCECDD.", "DD.CCC.DD", "D...d...D", "........."];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Hops along the snow. 3 hits.
    Slime,
    /// Flutters at head height, faster. 2 hits.
    Bat,
}

impl Kind {
    pub fn width(self) -> i32 {
        match self {
            Kind::Slime => 8,
            Kind::Bat => 9,
        }
    }

    fn speed(self) -> f32 {
        match self {
            Kind::Slime => 10.0,
            Kind::Bat => 17.0,
        }
    }

    fn hp(self) -> i32 {
        match self {
            Kind::Slime => 3,
            Kind::Bat => 2,
        }
    }
}

pub enum Event {
    /// Reached the warrior and bit him.
    Bite,
}

const BITE_EVERY: f32 = 1.3;
const DEATH: f32 = 0.45;

pub struct Mob {
    pub kind: Kind,
    pub x: f32,
    pub hp: i32,
    t: f32,
    hurt: f32,
    bite_cd: f32,
    dying: Option<f32>,
}

impl Mob {
    pub fn new(kind: Kind, x: f32) -> Self {
        Mob { kind, x, hp: kind.hp(), t: 0.0, hurt: 0.0, bite_cd: BITE_EVERY * 0.5, dying: None }
    }

    pub fn center(&self) -> f32 {
        self.x + self.kind.width() as f32 / 2.0
    }

    /// Still fighting (not dying).
    pub fn fighting(&self) -> bool {
        self.dying.is_none()
    }

    /// Present on screen (fighting or playing its death poof).
    pub fn present(&self) -> bool {
        self.dying.is_none_or(|t| t < DEATH)
    }

    /// Takes a sword hit from the side of `from_x`. Returns true if it died.
    pub fn hit(&mut self, from_x: f32) -> bool {
        if !self.fighting() {
            return false;
        }
        self.hp -= 1;
        self.hurt = 0.25;
        self.x += if from_x < self.center() { 7.0 } else { -7.0 };
        if self.hp <= 0 {
            self.dying = Some(0.0);
            return true;
        }
        false
    }

    /// Walks toward `target_x` (the warrior); bites when adjacent.
    pub fn step(&mut self, dt: f32, target_x: f32) -> Option<Event> {
        self.t += dt;
        self.hurt = (self.hurt - dt).max(0.0);
        if let Some(d) = &mut self.dying {
            *d += dt;
            return None;
        }
        self.bite_cd -= dt;
        let dx = target_x - self.center();
        if dx.abs() > 5.0 {
            if self.hurt <= 0.0 {
                self.x += dx.signum() * self.kind.speed() * dt;
            }
            None
        } else if self.bite_cd <= 0.0 {
            self.bite_cd = BITE_EVERY;
            Some(Event::Bite)
        } else {
            None
        }
    }

    pub fn draw(&self, c: &mut Canvas, feet_y: f32, time: f32) {
        let x = self.x.round() as i32;
        let (rows, y) = match self.kind {
            Kind::Slime => {
                let hop = ((self.t * 8.0).sin().abs() * 3.0).round() as i32;
                (SLIME, feet_y.round() as i32 - SLIME.len() as i32 - hop)
            }
            Kind::Bat => {
                let hover = ((self.t * 3.0).sin() * 3.0).round() as i32;
                let frame = if (self.t * 10.0) as i32 % 2 == 0 { BAT_UP } else { BAT_DOWN };
                (frame, feet_y.round() as i32 - 18 + hover)
            }
        };
        if let Some(d) = self.dying {
            // Burst into ice dust.
            let k = d / DEATH;
            for (j, row) in rows.iter().enumerate() {
                for (i, ch) in row.chars().enumerate() {
                    if ch != '.' {
                        let (px, py) = (x + i as i32, y + j as i32);
                        let spread = (k * 8.0) as i32;
                        let (ox, oy) = ((i as i32 - 4) * spread / 4, (j as i32 - 3) * spread / 4 - spread);
                        if k < bayer(px, py) + 0.3 {
                            c.set(px + ox, py + oy, hex(0xc8f4ff));
                        }
                    }
                }
            }
            return;
        }
        if self.hurt > 0.0 && (time * 30.0) as i32 % 2 == 0 {
            c.sprite(rows, HURT_PAL, x, y, false);
        } else {
            c.sprite(rows, PAL, x, y, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_to_the_warrior_and_bites_on_a_cooldown() {
        let mut m = Mob::new(Kind::Slime, 0.0);
        let mut bites = 0;
        for _ in 0..(20 * 30) {
            if let Some(Event::Bite) = m.step(1.0 / 30.0, 100.0) {
                bites += 1;
            }
        }
        assert!((m.center() - 100.0).abs() <= 6.0);
        assert!((5..=12).contains(&bites), "bites {bites}");
    }

    #[test]
    fn dies_after_enough_hits_and_disappears() {
        let mut m = Mob::new(Kind::Bat, 50.0);
        assert!(!m.hit(40.0));
        assert!(m.x > 50.0, "knocked back away from the sword");
        assert!(m.hit(40.0));
        assert!(!m.fighting());
        assert!(!m.hit(40.0), "can't kill twice");
        for _ in 0..30 {
            m.step(1.0 / 30.0, 0.0);
        }
        assert!(!m.present());
    }
}
