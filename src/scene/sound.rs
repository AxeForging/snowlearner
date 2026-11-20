//! Sounds the scene asks for (#163). The scene stays silent and pure: it only
//! queues what happened, and the app plays it (`speech::effects`).

use super::pyro::Spell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// The fire mage's spell hit the frost mage, by tier.
    Impact(Spell),
    /// The comet landed (6 in a row): the heaviest hit.
    Comet,
}

impl Sound {
    /// For the log.
    pub fn name(self) -> &'static str {
        match self {
            Sound::Impact(Spell::Spark) => "spark impact",
            Sound::Impact(Spell::Fireball) => "fireball impact",
            Sound::Impact(Spell::Blaze) => "blaze impact",
            Sound::Comet => "comet impact",
        }
    }
}
