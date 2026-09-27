//! Mic audio → 16 kHz mono f32, the format whisper expects.

pub const WHISPER_RATE: u32 = 16_000;

pub fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
}

/// Band-limited resampler: when going down (48 kHz mic → 16 kHz), a
/// windowed-sinc low-pass keeps everything above the new Nyquist (mic hiss,
/// the top of an "s") from folding back into the speech band as noise.
/// Going up it interpolates linearly.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    if from < to {
        return linear(input, from, to);
    }
    let ratio = from as f64 / to as f64;
    let cutoff = 0.5 / ratio * 0.92; // cycles per input sample, just under the new Nyquist
    let half = (6.0 * ratio).ceil() as isize;
    let last = input.len() as isize - 1;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let center = pos.floor() as isize;
            let (mut acc, mut norm) = (0.0f64, 0.0f64);
            for k in center - half + 1..=center + half {
                let x = pos - k as f64;
                let arg = 2.0 * cutoff * x;
                let sinc = if arg.abs() < 1e-9 {
                    1.0
                } else {
                    (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
                };
                let hann = 0.5 + 0.5 * (std::f64::consts::PI * x / half as f64).cos();
                let w = sinc * hann;
                acc += input[k.clamp(0, last) as usize] as f64 * w;
                norm += w;
            }
            (acc / norm) as f32
        })
        .collect()
}

/// Linear interpolation, for going up.
fn linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = input.get(idx + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_averaged_into_mono() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(to_mono(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn resampling_keeps_duration() {
        let one_second_48k = vec![0.0; 48_000];
        assert_eq!(resample(&one_second_48k, 48_000, WHISPER_RATE).len(), 16_000);
        let one_second_44k = vec![0.0; 44_100];
        assert_eq!(resample(&one_second_44k, 44_100, WHISPER_RATE).len(), 16_000);
    }

    fn sine(hz: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn resampling_preserves_a_constant_signal_and_a_ramp() {
        assert!(resample(&[0.25; 300], 48_000, 16_000).iter().all(|v| (v - 0.25).abs() < 1e-5));
        let ramp: Vec<f32> = (0..300).map(|i| i as f32).collect();
        let out = resample(&ramp, 3, 1);
        assert_eq!(out.len(), 100);
        for (i, v) in out.iter().enumerate().skip(10).take(80) {
            assert!((v - 3.0 * i as f32).abs() < 1e-2, "no time shift: {i} → {v}");
        }
    }

    #[test]
    fn speech_frequencies_survive_downsampling() {
        let out = resample(&sine(1_000.0, 48_000, 0.5), 48_000, WHISPER_RATE);
        let r = rms(&out[800..7_200]);
        assert!((r - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "1 kHz kept at full level: {r}");
    }

    #[test]
    fn mic_hiss_above_the_new_nyquist_does_not_fold_into_the_speech_band() {
        // A 12 kHz component at 48 kHz would alias to 4 kHz at 16 kHz — right
        // where consonants live — if it weren't filtered out first.
        for hz in [9_000.0, 12_000.0, 15_000.0] {
            let out = resample(&sine(hz, 48_000, 0.5), 48_000, WHISPER_RATE);
            let r = rms(&out[800..7_200]);
            assert!(r < 0.05, "{hz} Hz leaked through at {r}");
        }
        let out = resample(&sine(15_000.0, 44_100, 0.5), 44_100, WHISPER_RATE);
        assert!(rms(&out[800..7_200]) < 0.05, "44.1 kHz input too");
    }

    #[test]
    fn same_rate_and_empty_input_are_passthrough() {
        assert_eq!(resample(&[0.1, 0.2], 16_000, 16_000), vec![0.1, 0.2]);
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }
}
