//! Mic audio → 16 kHz mono f32, the format whisper expects.

pub const WHISPER_RATE: u32 = 16_000;

pub fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
}

/// Linear interpolation resampler — plenty for speech recognition input.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
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

    #[test]
    fn resampling_preserves_a_constant_signal_and_a_ramp() {
        assert!(resample(&[0.25; 300], 48_000, 16_000).iter().all(|v| (v - 0.25).abs() < 1e-6));
        let ramp: Vec<f32> = (0..9).map(|i| i as f32).collect();
        assert_eq!(resample(&ramp, 3, 1), vec![0.0, 3.0, 6.0]);
    }

    #[test]
    fn same_rate_and_empty_input_are_passthrough() {
        assert_eq!(resample(&[0.1, 0.2], 16_000, 16_000), vec![0.1, 0.2]);
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }
}
