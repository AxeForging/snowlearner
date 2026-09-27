//! Removes DC offset and rumble from the microphone before anything measures
//! it. Some laptop mics sit far from zero (measured: +0.044 on one channel with
//! true room noise ~0.0013): counted as "noise", that offset pushed the speech
//! gate above a normal voice and the lesson stayed on "Fale agora". Pure.

/// Well under the lowest voice fundamental (~85 Hz).
const CUTOFF_HZ: f32 = 30.0;

pub struct HighPass {
    a: f32,
    prev_x: f32,
    prev_y: f32,
    primed: bool,
}

impl HighPass {
    pub fn new(rate: u32) -> HighPass {
        let a = (-std::f32::consts::TAU * CUTOFF_HZ / rate.max(1) as f32).exp();
        HighPass { a, prev_x: 0.0, prev_y: 0.0, primed: false }
    }

    /// Filters in place; state carries across calls (mic chunks).
    pub fn process(&mut self, samples: &mut [f32]) {
        for s in samples {
            if !self.primed {
                // Start from the first sample, so an offset mic doesn't open with a thump.
                self.prev_x = *s;
                self.primed = true;
            }
            let y = *s - self.prev_x + self.a * self.prev_y;
            self.prev_x = *s;
            self.prev_y = y;
            *s = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn sine(hz: f32, amp: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin() * amp)
            .collect()
    }

    #[test]
    fn a_constant_offset_is_removed_from_the_first_sample() {
        let mut x = vec![0.044; 4_000];
        HighPass::new(RATE).process(&mut x);
        assert!(x.iter().all(|v| v.abs() < 1e-6), "no thump at the start either");
    }

    #[test]
    fn voice_frequencies_pass_through_nearly_untouched() {
        for hz in [100.0, 150.0, 300.0, 1000.0, 3000.0] {
            let mut x = sine(hz, 0.05, 1.0);
            let before = rms(&x[4_000..]);
            HighPass::new(RATE).process(&mut x);
            let after = rms(&x[4_000..]);
            assert!(after / before > 0.95, "{hz} Hz kept {:.2}", after / before);
        }
    }

    #[test]
    fn a_drifting_offset_under_a_voice_leaves_just_the_voice() {
        let voice = sine(150.0, 0.03, 1.0);
        let mut x: Vec<f32> = voice.iter().enumerate().map(|(i, v)| v + 0.07 - 0.00002 * i as f32 / 16.0).collect();
        HighPass::new(RATE).process(&mut x);
        let (got, want) = (rms(&x[4_000..]), rms(&voice[4_000..]));
        assert!((got - want).abs() / want < 0.05, "voice {want:.4} came out {got:.4}");
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let src: Vec<f32> = sine(200.0, 0.1, 0.5).iter().map(|v| v + 0.03).collect();
        let mut whole = src.clone();
        HighPass::new(RATE).process(&mut whole);
        let mut hp = HighPass::new(RATE);
        let mut chunked = src.clone();
        for c in chunked.chunks_mut(160) {
            hp.process(c);
        }
        assert_eq!(whole, chunked);
    }
}
