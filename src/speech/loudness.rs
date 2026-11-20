//! Loudness (#162): how loud a clip sounds, and bringing a clip to a level.
//! The replayed answer is brought to the lesson voice's level, so a learner who
//! spoke softly or loudly hears it as loud as the lesson.
//!
//! Measured as ITU-R BS.1770 does: K-weighting (a high shelf and a high-pass,
//! the ear's sensitivity), 400 ms blocks every 100 ms, then an absolute gate
//! (-70 LUFS) and a relative one (10 LU under the rest), so silence and room
//! hiss do not pull the level down. The filter constants are libebur128's
//! (`ebur128_init_filter`), designed for any sample rate. Pure: nothing here
//! touches a device.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Most a clip is raised (dB): a near-silent clip stays quiet instead of
/// turning room hiss into speech.
pub const MAX_GAIN_DB: f32 = 24.0;
/// The limiter's ceiling: -1 dBFS.
pub const CEILING: f32 = 0.891;
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;
const BLOCK_S: f32 = 0.4;
const HOP_S: f32 = 0.1;
/// The limiter reaches its full reduction this long before a peak (s)…
const ATTACK_S: f32 = 0.002;
/// …and lets go over this long after it (s).
const RELEASE_S: f32 = 0.05;

/// The level of the OS voices (LUFS), which speak by themselves so the app
/// never hears them. Windows: SAPI's default voice (Microsoft Zira, volume 100)
/// rendered to WAV with `SetOutputToWaveFile` and measured with `level`:
/// -20.2 LUFS (2026-11-20). Linux and macOS were not measured (no espeak-ng,
/// spd-say or `say` where this was written): they use the Windows value.
pub const SYSTEM_VOICE: f32 = -20.2;

/// One second-order section (direct form I, f64: the 38 Hz high-pass is too
/// narrow for f32 at 48 kHz).
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let forward = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1];
        let y = forward - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The K-weighting filters for `rate`, as libebur128 designs them.
fn k_filters(rate: u32) -> [Biquad; 2] {
    let rate = rate.max(1) as f64;
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        x: [0.0; 2],
        y: [0.0; 2],
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        x: [0.0; 2],
        y: [0.0; 2],
    };
    [shelf, high_pass]
}

fn lufs(mean_square: f64) -> f64 {
    -0.691 + 10.0 * mean_square.log10()
}

/// How loud `samples` (mono at `rate` Hz) sound, in LUFS. `None` when nothing
/// passes the absolute gate (silence). A clip shorter than one block is
/// measured as one block.
pub fn level(samples: &[f32], rate: u32) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }
    let [mut shelf, mut high_pass] = k_filters(rate);
    let squares: Vec<f64> = samples
        .iter()
        .map(|&s| {
            let y = high_pass.run(shelf.run(s as f64));
            y * y
        })
        .collect();
    let block = ((rate as f32 * BLOCK_S) as usize).clamp(1, squares.len());
    let hop = ((rate as f32 * HOP_S) as usize).max(1);
    let mut blocks = Vec::new();
    let mut start = 0;
    while start + block <= squares.len() {
        blocks.push(squares[start..start + block].iter().sum::<f64>() / block as f64);
        start += hop;
    }
    let loud: Vec<f64> = blocks.into_iter().filter(|&z| z > 0.0 && lufs(z) > ABSOLUTE_GATE).collect();
    if loud.is_empty() {
        return None;
    }
    let relative = lufs(loud.iter().sum::<f64>() / loud.len() as f64) + RELATIVE_GATE;
    let kept: Vec<f64> = loud.into_iter().filter(|&z| lufs(z) > relative).collect();
    Some(lufs(kept.iter().sum::<f64>() / kept.len() as f64) as f32)
}

/// The gain (dB) that brings `samples` to `target`, at most `MAX_GAIN_DB`.
/// 0 for silence.
pub fn gain_db(samples: &[f32], rate: u32, target: f32) -> f32 {
    level(samples, rate).map_or(0.0, |l| (target - l).min(MAX_GAIN_DB))
}

/// `samples` brought to `target` LUFS: one gain for the whole clip, then a
/// limiter so no sample goes over `CEILING`. The input is left as it was.
pub fn match_level(samples: &[f32], rate: u32, target: f32) -> Vec<f32> {
    let g = 10f32.powf(gain_db(samples, rate, target) / 20.0);
    let mut out: Vec<f32> = samples.iter().map(|s| s * g).collect();
    limit(&mut out, rate);
    out
}

/// Pulls peaks down to `CEILING`. The gain each sample needs is ramped over
/// `ATTACK_S` before it and `RELEASE_S` after it, never above what that sample
/// needs, so the ceiling holds and the reduction does not click.
fn limit(samples: &mut [f32], rate: u32) {
    let need: Vec<f32> = samples.iter().map(|s| if s.abs() > CEILING { CEILING / s.abs() } else { 1.0 }).collect();
    if need.iter().all(|&g| g >= 1.0) {
        return;
    }
    let release = 1.0 / (RELEASE_S * rate.max(1) as f32);
    let attack = 1.0 / (ATTACK_S * rate.max(1) as f32);
    let mut gain = need.clone();
    for i in 1..gain.len() {
        gain[i] = gain[i].min(gain[i - 1] + release);
    }
    for i in (0..gain.len().saturating_sub(1)).rev() {
        gain[i] = gain[i].min(gain[i + 1] + attack);
    }
    for (s, g) in samples.iter_mut().zip(gain) {
        *s = (*s * g).clamp(-CEILING, CEILING);
    }
}

/// The lesson voice's level (LUFS), shared by whoever plays next to it.
#[derive(Clone)]
pub struct Reference(Arc<AtomicU32>);

impl Reference {
    /// Starts at the OS voice's level until a voice is heard.
    pub fn new() -> Reference {
        Reference(Arc::new(AtomicU32::new(SYSTEM_VOICE.to_bits())))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// The lesson voice just spoke: `Some(level)` measured from the audio the
    /// app played, `None` when the OS voice spoke by itself.
    pub fn heard(&self, level: Option<f32>) {
        self.0.store(level.unwrap_or(SYSTEM_VOICE).to_bits(), Ordering::Relaxed);
    }
}

impl Default for Reference {
    fn default() -> Reference {
        Reference::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    /// A voice-like clip: a few harmonics with a syllable rhythm, between
    /// silences, scaled so its peak is `peak_db` dBFS.
    fn voice(peak_db: f32) -> Vec<f32> {
        let n = |s: f32| (s * RATE as f32) as usize;
        let mut v = vec![0.0; n(0.3)];
        v.extend((0..n(1.2)).map(|i| {
            let t = i as f32 / RATE as f32;
            let tone: f32 = [(140.0, 1.0), (280.0, 0.6), (560.0, 0.4), (1200.0, 0.2)]
                .iter()
                .map(|(f, a)| a * (std::f32::consts::TAU * f * t).sin())
                .sum();
            tone * (0.55 + 0.45 * (std::f32::consts::TAU * 4.0 * t).sin().abs())
        }));
        v.extend(vec![0.0; n(0.3)]);
        let peak = v.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let g = 10f32.powf(peak_db / 20.0) / peak;
        v.iter().map(|s| s * g).collect()
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn a_full_scale_1khz_tone_reads_about_minus_3_lufs() {
        // BS.1770's own check: a 0 dBFS 997 Hz sine reads -3.01 LUFS on one channel.
        let sine = |rate: u32| -> Vec<f32> {
            (0..rate * 2).map(|i| (std::f32::consts::TAU * 997.0 * i as f32 / rate as f32).sin()).collect()
        };
        for rate in [RATE, 48_000] {
            let l = level(&sine(rate), rate).unwrap();
            assert!((l + 3.01).abs() < 0.2, "{l} LUFS at {rate} Hz");
        }
    }

    #[test]
    fn silence_has_no_level_and_is_left_alone() {
        assert_eq!(level(&[], RATE), None);
        assert_eq!(level(&vec![0.0; RATE as usize], RATE), None);
        assert_eq!(match_level(&vec![0.0; 100], RATE, -20.0), vec![0.0; 100]);
    }

    #[test]
    fn a_soft_and_a_loud_answer_are_replayed_at_the_reference() {
        let target = -20.0;
        for peak_db in [-30.0, -6.0] {
            let out = match_level(&voice(peak_db), RATE, target);
            let l = level(&out, RATE).unwrap();
            assert!((l - target).abs() <= 1.0, "peak {peak_db} dB came out at {l} LUFS");
        }
    }

    #[test]
    fn a_near_silent_clip_gets_at_most_the_maximum_gain() {
        let quiet = voice(-50.0);
        let before = level(&quiet, RATE).unwrap();
        assert_eq!(gain_db(&quiet, RATE, -20.0), MAX_GAIN_DB);
        let after = level(&match_level(&quiet, RATE, -20.0), RATE).unwrap();
        assert!((after - before - MAX_GAIN_DB).abs() < 0.1, "{before} → {after}");
        assert!(after < -20.0 - 10.0, "hiss-level clip stays well under speech: {after}");
    }

    #[test]
    fn nothing_goes_over_the_ceiling_and_a_clip_at_the_reference_is_kept() {
        // Raised hard, a peaky clip needs the limiter.
        let mut spiky = voice(-20.0);
        spiky[RATE as usize / 2] = 0.3;
        let loud = match_level(&spiky, RATE, -5.0);
        assert!(peak(&loud) <= CEILING + 1e-6, "peak {}", peak(&loud));
        let at = voice(-12.0);
        let reference = level(&at, RATE).unwrap();
        let kept = match_level(&at, RATE, reference);
        assert!((level(&kept, RATE).unwrap() - reference).abs() <= 0.5);
        assert!((peak(&kept) - peak(&at)).abs() < 1e-4, "no limiting below the ceiling");
    }

    #[test]
    fn the_limiter_ramps_instead_of_clicking() {
        let mut v = vec![0.5f32; RATE as usize / 10];
        v[800] = 1.8;
        limit(&mut v, RATE);
        assert!(v[800] <= CEILING + 1e-6);
        // Around the peak the gain moves at most one attack step per sample.
        let gain: Vec<f32> = v.iter().enumerate().filter(|(i, _)| *i != 800).map(|(_, s)| s / 0.5).collect();
        let step = gain.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(step <= 1.0 / (ATTACK_S * RATE as f32) + 1e-4, "gain jumped {step} in one sample");
        assert!(gain.iter().cloned().fold(1.0, f32::min) < 0.6, "the samples next to the peak are pulled down too");
        assert!((v[0] - 0.5).abs() < 1e-6 && (v[v.len() - 1] - 0.5).abs() < 1e-6, "untouched far from the peak");
    }

    #[test]
    fn the_reference_follows_the_last_voice_heard() {
        let r = Reference::new();
        assert_eq!(r.get(), SYSTEM_VOICE);
        let shared = r.clone();
        r.heard(Some(-14.5));
        assert_eq!(shared.get(), -14.5, "clones share it");
        r.heard(None);
        assert_eq!(shared.get(), SYSTEM_VOICE, "the OS voice speaking puts it back");
    }
}
