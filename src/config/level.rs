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
    /// Snow pixels (column-height units) each shattered cube adds.
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
                throw_every: 9.0,
                snow_per_cube: 3.0,
                summon_every: 240.0,
                ask_every: 1800.0,
                walk_speed: 6.0,
                melt_fraction: 0.25,
                icicles_every: 300.0,
                mobs_every: 200.0,
            },
            Commitment::Steady => Pace {
                throw_every: 5.0,
                snow_per_cube: 4.0,
                summon_every: 120.0,
                ask_every: 1200.0,
                walk_speed: 9.0,
                melt_fraction: 0.18,
                icicles_every: 180.0,
                mobs_every: 110.0,
            },
            Commitment::Committed => Pace {
                throw_every: 3.0,
                snow_per_cube: 5.0,
                summon_every: 75.0,
                ask_every: 720.0,
                walk_speed: 12.0,
                melt_fraction: 0.14,
                icicles_every: 110.0,
                mobs_every: 70.0,
            },
            Commitment::Relentless => Pace {
                throw_every: 1.6,
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

    pub fn label_pt(self) -> &'static str {
        match self {
            Commitment::Chill => "Tranquilo",
            Commitment::Steady => "Constante",
            Commitment::Committed => "Comprometido",
            Commitment::Relentless => "Implacável",
        }
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
