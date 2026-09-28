//! Small frost monsters that wander in from the screen edges and go after the
//! warrior. He fights back with his sword; each bite chills him, each win warms him.

use super::rng::Rng;
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

    pub fn speed(self) -> f32 {
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
/// Bite distances a mob can pick (px from the warrior's center). Centered on
/// the 5 px every mob used, so waves bite as often as before, and always
/// inside the 9 px he swings from, or a mob at the screen edge bites where he
/// can't reach it and the fight never ends.
pub const REACH: (f32, f32) = (3.0, 7.0);
/// How fast crowded mobs of a kind step apart (px/s): a shuffle, not a jump.
pub const SEPARATION_SPEED: f32 = 12.0;

pub struct Mob {
    pub kind: Kind,
    pub x: f32,
    pub hp: i32,
    /// Own speed factor (×0.8–1.2), so a wave doesn't move as one block.
    pub pace: f32,
    /// Own offset into the hop/hover/wing cycle.
    pub phase: f32,
    /// Bats: own flight height above the snow (px).
    pub height: f32,
    /// How close it gets to the warrior before biting (px from his center).
    pub reach: f32,
    t: f32,
    hurt: f32,
    bite_cd: f32,
    dying: Option<f32>,
}

impl Mob {
    pub fn new(kind: Kind, x: f32, rng: &mut Rng) -> Self {
        Mob {
            kind,
            x,
            hp: kind.hp(),
            pace: rng.range(0.8, 1.2),
            phase: rng.range(0.0, std::f32::consts::TAU),
            height: rng.range(14.0, 26.0),
            reach: rng.range(REACH.0, REACH.1),
            t: 0.0,
            hurt: 0.0,
            bite_cd: BITE_EVERY * 0.5,
            dying: None,
        }
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
        if dx.abs() > self.reach {
            if self.hurt <= 0.0 {
                self.x += dx.signum() * self.kind.speed() * self.pace * dt;
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
        let t = self.t + self.phase;
        let (rows, y) = match self.kind {
            Kind::Slime => {
                let hop = ((t * 8.0).sin().abs() * 3.0).round() as i32;
                (SLIME, feet_y.round() as i32 - SLIME.len() as i32 - hop)
            }
            Kind::Bat => {
                let hover = ((t * 3.0).sin() * 3.0).round() as i32;
                let frame = if (t * 10.0) as i32 % 2 == 0 { BAT_UP } else { BAT_DOWN };
                (frame, (feet_y - self.height).round() as i32 + hover)
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

/// The separation rule of boids (Reynolds, 1987), along x: fighting mobs of
/// the same kind that overlap step apart, each by up to half the overlap and
/// at most `SEPARATION_SPEED`, so a wave spreads out instead of stacking on
/// the warrior. Kinds don't crowd each other: a bat flies over a slime.
pub fn separate(mobs: &mut [Mob], dt: f32) {
    let max_step = SEPARATION_SPEED * dt;
    for i in 0..mobs.len() {
        for j in i + 1..mobs.len() {
            let (a, b) = (&mobs[i], &mobs[j]);
            if a.kind != b.kind || !a.fighting() || !b.fighting() {
                continue;
            }
            let room = a.kind.width() as f32;
            let gap = b.center() - a.center();
            if gap.abs() >= room {
                continue;
            }
            // Equal spots: the earlier one gives way to the left.
            let dir = if gap >= 0.0 { 1.0 } else { -1.0 };
            let step = ((room - gap.abs()) / 2.0).min(max_step);
            mobs[i].x -= dir * step;
            mobs[j].x += dir * step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::rng::Rng;

    #[test]
    fn walks_to_the_warrior_and_bites_on_a_cooldown() {
        let mut m = Mob::new(Kind::Slime, 0.0, &mut Rng::new(1));
        let mut bites = 0;
        for _ in 0..(20 * 30) {
            if let Some(Event::Bite) = m.step(1.0 / 30.0, 100.0) {
                bites += 1;
            }
        }
        assert!((m.center() - 100.0).abs() <= m.reach + 1.0, "stopped within its own reach");
        assert!((5..=12).contains(&bites), "bites {bites}");
    }

    #[test]
    fn dies_after_enough_hits_and_disappears() {
        let mut m = Mob::new(Kind::Bat, 50.0, &mut Rng::new(2));
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

    #[test]
    fn each_mob_has_its_own_pace_rhythm_height_and_reach() {
        let mut rng = Rng::new(3);
        let bats: Vec<Mob> = (0..40).map(|_| Mob::new(Kind::Bat, 0.0, &mut rng)).collect();
        for b in &bats {
            assert!((0.8..=1.2).contains(&b.pace), "pace {}", b.pace);
            assert!((14.0..=26.0).contains(&b.height), "height {}", b.height);
            assert!((REACH.0..=REACH.1).contains(&b.reach), "reach {}", b.reach);
        }
        let spread = |f: fn(&Mob) -> f32| {
            let (lo, hi) = bats.iter().map(f).fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(v), h.max(v)));
            hi - lo
        };
        assert!(spread(|m| m.pace) > 0.3, "not one shared speed");
        assert!(spread(|m| m.height) > 8.0, "not one flight height");
        assert!(spread(|m| m.phase) > 4.0, "wings and hover out of step");
    }

    #[test]
    fn mobs_of_a_kind_make_room_for_each_other_but_a_bat_may_pass_over_a_slime() {
        let mut rng = Rng::new(4);
        let mut mobs = vec![
            Mob::new(Kind::Slime, 50.0, &mut rng),
            Mob::new(Kind::Slime, 50.0, &mut rng),
            Mob::new(Kind::Slime, 51.0, &mut rng),
            Mob::new(Kind::Bat, 50.0, &mut rng),
        ];
        for _ in 0..60 {
            separate(&mut mobs, 1.0 / 30.0);
        }
        let mut slimes: Vec<f32> = mobs[..3].iter().map(Mob::center).collect();
        slimes.sort_by(f32::total_cmp);
        for w in slimes.windows(2) {
            assert!(w[1] - w[0] >= Kind::Slime.width() as f32, "slimes still overlap: {slimes:?}");
        }
        assert_eq!(mobs[3].x, 50.0, "a bat flies over the slimes, no need to move");
    }

    #[test]
    fn making_room_is_gradual_not_a_jump() {
        let mut rng = Rng::new(5);
        let mut mobs = vec![Mob::new(Kind::Slime, 50.0, &mut rng), Mob::new(Kind::Slime, 50.0, &mut rng)];
        separate(&mut mobs, 1.0 / 30.0);
        let moved = (mobs[0].x - 50.0).abs().max((mobs[1].x - 50.0).abs());
        assert!(moved > 0.0 && moved <= SEPARATION_SPEED / 30.0 + 1e-4, "moved {moved}");
    }
}
