//! Commitment level (*compromisso*): how hard the mage pushes you.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Commitment {
    Chill,
    #[default]
    Steady,
    Committed,
    Relentless,
}

/// Game pacing derived from the commitment level. Times are in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// Pause between ice throws.
    pub throw_every: f32,
    /// How hard it snows: the share of the flake pool in the air, the same
    /// all the time (#180). Real snow is light, moderate or heavy by
    /// visibility (over 1 km, 0.5-1 km, under 0.5 km), and the flakes in view
    /// grow as the visibility drops: 1 : 2 : 4.
    pub snowfall: f32,
    /// Share of what lands (flakes, cubes, icicles, snowballs) that stays on
    /// the pile: a flake stands for less snow, so the screen fills slowly while
    /// the snowfall looks the same (#187).
    pub settle_share: f32,
    /// Snow the thrown things (cubes, icicles, snowballs, a friend's spray) lay per pixel the
    /// snowfall lays, measured on the 480x270 scene the pace was tuned on
    /// (#181). A finer scene makes up for them with flakes: they keep their
    /// size while the screen holds more pixels.
    pub thrown_ratio: f32,
    /// Mean wind while it blows, px/s (the scene is ~10 px per metre: the
    /// warrior, 16 px, is a 1.7 m adult), from a breeze to a blizzard (#183).
    /// A finer scene than 480x270 blows its flakes as fast on screen (#181).
    pub wind: f32,
    /// How far the gusts swing around that mean, px/s.
    pub gust: f32,
    /// How hard the frost sprayed onto the edges bites (a cube itself leaves
    /// only its own pixels on the pile, #180).
    pub snow_per_cube: f32,
    /// Time between friend summons (snowman, penguin).
    pub summon_every: f32,
    /// Time between the mage nudging you to practice.
    pub ask_every: f32,
    /// Mage walking speed in virtual px/s.
    pub walk_speed: f32,
    /// Share of snow + frost one correct phrase melts (the fire mage's fireball).
    /// Deliberately small: digging out takes a real practice session.
    pub melt_fraction: f32,
    /// Time between icicle rains (icicles fall from the top of the screen).
    pub icicles_every: f32,
    /// Time between frost mob waves attacking the warrior.
    pub mobs_every: f32,
}

impl Commitment {
    pub fn pace(self) -> Pace {
        match self {
            Commitment::Chill => Pace {
                // 9.0 before #180, when the cubes were the snowfall.
                throw_every: 30.0,
                snowfall: 0.25,
                settle_share: 0.5,
                // A light breeze, ~1 m/s.
                thrown_ratio: 0.84,
                wind: 10.0,
                gust: 5.0,
                snow_per_cube: 3.0,
                summon_every: 240.0,
                ask_every: 1800.0,
                walk_speed: 6.0,
                melt_fraction: 0.25,
                icicles_every: 300.0,
                mobs_every: 200.0,
            },
            Commitment::Steady => Pace {
                // 5.0 refilled the screen too fast for a learner (#170); 6.0
                // until the snowfall carried the fill (#180).
                throw_every: 15.0,
                snowfall: 0.5,
                settle_share: 0.5,
                // A gentle breeze, ~2 m/s.
                thrown_ratio: 0.74,
                wind: 20.0,
                gust: 12.0,
                snow_per_cube: 4.0,
                summon_every: 120.0,
                ask_every: 1200.0,
                walk_speed: 9.0,
                melt_fraction: 0.18,
                icicles_every: 180.0,
                mobs_every: 110.0,
            },
            Commitment::Committed => Pace {
                throw_every: 6.0,
                snowfall: 1.0,
                settle_share: 0.8,
                // A fresh wind, ~5 m/s.
                thrown_ratio: 0.7,
                wind: 50.0,
                gust: 25.0,
                snow_per_cube: 5.0,
                summon_every: 75.0,
                ask_every: 720.0,
                walk_speed: 12.0,
                melt_fraction: 0.14,
                icicles_every: 110.0,
                mobs_every: 70.0,
            },
            Commitment::Relentless => Pace {
                // A blizzard: heavy snow, its wind comes with #183.
                throw_every: 3.0,
                snowfall: 1.0,
                settle_share: 0.8,
                // A blizzard: 35 mph (15.6 m/s) or more (NWS).
                thrown_ratio: 1.22,
                wind: 160.0,
                gust: 50.0,
                snow_per_cube: 6.0,
                summon_every: 45.0,
                ask_every: 360.0,
                walk_speed: 16.0,
                melt_fraction: 0.12,
                icicles_every: 70.0,
                mobs_every: 45.0,
            },
        }
    }

    pub fn label(self, native: crate::lang::Native) -> &'static str {
        use crate::lang::T;
        match self {
            Commitment::Chill => T::CommitChill,
            Commitment::Steady => T::CommitSteady,
            Commitment::Committed => T::CommitCommitted,
            Commitment::Relentless => T::CommitRelentless,
        }
        .get(native)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [Commitment; 4] =
        [Commitment::Chill, Commitment::Steady, Commitment::Committed, Commitment::Relentless];

    #[test]
    fn higher_commitment_is_strictly_more_demanding() {
        for pair in ORDER.windows(2) {
            let (easy, hard) = (pair[0].pace(), pair[1].pace());
            assert!(hard.throw_every < easy.throw_every, "{:?}", pair);
            assert!(hard.snowfall >= easy.snowfall, "{:?}", pair);
            assert!(hard.settle_share >= easy.settle_share, "{:?}", pair);
            assert!(hard.wind > easy.wind && hard.gust > easy.gust, "{:?}", pair);
            assert!(hard.snow_per_cube > easy.snow_per_cube, "{:?}", pair);
            assert!(hard.summon_every < easy.summon_every, "{:?}", pair);
            assert!(hard.ask_every < easy.ask_every, "{:?}", pair);
            assert!(hard.melt_fraction < easy.melt_fraction, "{:?}", pair);
            assert!(hard.icicles_every < easy.icicles_every, "{:?}", pair);
            assert!(hard.mobs_every < easy.mobs_every, "{:?}", pair);
        }
    }

    #[test]
    fn every_level_can_be_dug_out_of() {
        for level in ORDER {
            let p = level.pace();
            assert!(p.melt_fraction > 0.05 && p.melt_fraction <= 0.3, "one answer must never clear the screen");
            assert!(p.throw_every > 0.5);
            assert!(p.snowfall > 0.0 && p.snowfall <= 1.0, "a share of the flake pool");
            assert!(p.settle_share > 0.0 && p.settle_share <= 1.0, "a share of what lands");
        }
    }

    #[test]
    fn levels_round_trip_through_config_names() {
        #[derive(Deserialize)]
        struct W {
            c: Commitment,
        }
        let w: W = toml::from_str("c = 'relentless'").unwrap();
        assert_eq!(w.c, Commitment::Relentless);
        assert!(toml::from_str::<W>("c = 'extreme'").is_err());
    }
}
