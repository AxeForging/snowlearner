//! Sound effects (#163), synthesized in code: nothing to download or license,
//! the same sound every time. An impact is a low thump whose pitch falls (the
//! body of the hit), a short burst of crackling noise (the fire) and, for the
//! heaviest ones, a rumble. Heavier spells last longer and sit lower; the comet
//! is the heaviest. Each is brought to the lesson voice's level before it
//! plays (`loudness`). Pure: the app plays what this returns.

use super::loudness;
use crate::scene::pyro::Spell;
use crate::scene::rng::Rng;
use crate::scene::sound::Sound;
use std::f32::consts::TAU;

/// Sample rate of every effect (Hz).
pub const RATE: u32 = 24_000;
/// The thump's attack (s): fast enough to hit, slow enough not to click.
const ATTACK_S: f32 = 0.003;
/// Fade at the very end so the last sample is silent (s).
const FADE_S: f32 = 0.02;

struct Recipe {
    secs: f32,
    /// The thump's pitch falls from this (Hz)…
    from_hz: f32,
    /// …to this (Hz).
    to_hz: f32,
    /// How fast the thump dies away (s).
    thump_tau: f32,
    /// Share of crackle against the thump.
    crackle: f32,
    crackle_tau: f32,
    /// Share of low rumble (0 = none).
    rumble: f32,
    seed: u64,
}

fn recipe(sound: Sound) -> Recipe {
    match sound {
        Sound::Impact(Spell::Spark) => Recipe {
            secs: 0.35,
            from_hz: 240.0,
            to_hz: 95.0,
            thump_tau: 0.06,
            crackle: 0.6,
            crackle_tau: 0.05,
            rumble: 0.0,
            seed: 1,
        },
        Sound::Impact(Spell::Fireball) => Recipe {
            secs: 0.6,
            from_hz: 180.0,
            to_hz: 62.0,
            thump_tau: 0.12,
            crackle: 0.45,
            crackle_tau: 0.08,
            rumble: 0.0,
            seed: 2,
        },
        Sound::Impact(Spell::Blaze) => Recipe {
            secs: 0.9,
            from_hz: 150.0,
            to_hz: 48.0,
            thump_tau: 0.2,
            crackle: 0.4,
            crackle_tau: 0.12,
            rumble: 0.3,
            seed: 3,
        },
        Sound::Comet => Recipe {
            secs: 1.8,
            from_hz: 120.0,
            to_hz: 36.0,
            thump_tau: 0.35,
            crackle: 0.35,
            crackle_tau: 0.2,
            rumble: 0.7,
            seed: 4,
        },
    }
}

/// One-pole low-pass coefficient for `hz` at `RATE`.
fn pole(hz: f32) -> f32 {
    (-TAU * hz / RATE as f32).exp()
}

/// The raw sound, mono at `RATE`, before any level matching.
pub fn render(sound: Sound) -> Vec<f32> {
    let r = recipe(sound);
    let mut rng = Rng::new(r.seed);
    let n = (r.secs * RATE as f32) as usize;
    let (hiss_pole, rumble_pole) = (pole(1500.0), pole(90.0));
    let (mut phase, mut low, mut rumble, mut pop) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let fade = (FADE_S * RATE as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            // The thump: a sine sweeping down, decaying.
            let hz = r.to_hz + (r.from_hz - r.to_hz) * (-t / (r.thump_tau * 0.7)).exp();
            phase = (phase + TAU * hz / RATE as f32) % TAU;
            let thump = phase.sin() * (-t / r.thump_tau).exp();
            // The crackle: high-passed noise plus sparse pops, dying away.
            let white = rng.range(-1.0, 1.0);
            low = hiss_pole * low + (1.0 - hiss_pole) * white;
            if rng.chance(0.004 * (-t / r.crackle_tau).exp()) {
                pop = rng.range(-1.0, 1.0);
            }
            pop *= 0.96;
            let crackle = ((white - low) * 0.6 + pop) * (-t / r.crackle_tau).exp();
            // The rumble: low-passed noise that swells and fades over the sound.
            rumble = rumble_pole * rumble + (1.0 - rumble_pole) * white;
            let swell = (1.0 - (-t / 0.03).exp()) * (-t / (r.secs / 3.0)).exp();
            let tail = if i + fade >= n { (n - i) as f32 / fade as f32 } else { 1.0 };
            let attack = 1.0 - (-t / ATTACK_S).exp();
            (thump + r.crackle * crackle + r.rumble * 12.0 * rumble * swell) * tail * attack
        })
        .collect()
}

/// The sound at `reference` LUFS (the lesson voice's level), never clipping.
pub fn at_level(sound: Sound, reference: f32) -> Vec<f32> {
    loudness::match_level(&render(sound), RATE, reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Sound; 4] =
        [Sound::Impact(Spell::Spark), Sound::Impact(Spell::Fireball), Sound::Impact(Spell::Blaze), Sound::Comet];

    /// Share of the energy under ~150 Hz.
    fn low_share(v: &[f32]) -> f32 {
        let p = pole(150.0);
        let mut lp = 0.0f32;
        let (mut low, mut all) = (0.0f32, 0.0f32);
        for &s in v {
            lp = p * lp + (1.0 - p) * s;
            low += lp * lp;
            all += s * s;
        }
        low / all
    }

    #[test]
    fn heavier_hits_last_longer_and_sit_lower() {
        let lens: Vec<usize> = ALL.iter().map(|&s| render(s).len()).collect();
        assert!(lens.windows(2).all(|w| w[0] < w[1]), "lengths {lens:?}");
        let lows: Vec<f32> = ALL.iter().map(|&s| low_share(&render(s))).collect();
        assert!(lows.windows(2).all(|w| w[0] < w[1]), "low-end shares {lows:?}");
    }

    #[test]
    fn every_sound_plays_at_the_voice_level_without_clipping() {
        // SAPI's level and the learner's Kokoro voice, measured for #162.
        for reference in [loudness::SYSTEM_VOICE, -25.0] {
            for s in ALL {
                let v = at_level(s, reference);
                let l = loudness::level(&v, RATE).unwrap();
                assert!((l - reference).abs() <= 1.0, "{s:?} at {l} LUFS for {reference}");
                let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
                assert!(peak <= loudness::CEILING + 1e-6, "{s:?} peaks at {peak}");
            }
        }
    }

    #[test]
    fn a_sound_starts_and_ends_without_a_click() {
        for s in ALL {
            let v = render(s);
            assert!(v[0].abs() < 0.05 && v[v.len() - 1].abs() < 1e-3, "{s:?}: {} … {}", v[0], v[v.len() - 1]);
        }
    }

    #[test]
    fn the_same_sound_is_the_same_every_time() {
        assert_eq!(render(Sound::Comet), render(Sound::Comet));
        assert_ne!(render(Sound::Comet), render(Sound::Impact(Spell::Blaze)));
    }
}
