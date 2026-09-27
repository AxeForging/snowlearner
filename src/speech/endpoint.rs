//! Decides when the learner has finished speaking, from raw mic samples.
//! Energy-based: calibrates on the first frames, waits for speech, then stops
//! after a stretch of silence (or a hard time cap).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Listening,
    /// Stopped. `speech` is false when nothing above the noise floor was heard.
    Done {
        speech: bool,
    },
}

pub struct Endpointer {
    frame_len: usize,
    frame: Vec<f32>,
    frames_seen: usize,
    calibration_frames: usize,
    noise_floor: f32,
    speech_frames: usize,
    silence_frames: usize,
    silence_to_stop: usize,
    no_speech_timeout: usize,
    max_frames: usize,
    status: Status,
}

/// Absolute floor so a silent room with a noise-free mic still needs real speech.
const MIN_THRESHOLD: f32 = 0.012;
const SPEECH_FRAMES_TO_START: usize = 3;

impl Endpointer {
    pub fn new(sample_rate: u32, max_seconds: f32) -> Self {
        let frame_len = (sample_rate as usize / 50).max(1); // 20 ms
        let per_sec = 50;
        Self {
            frame_len,
            frame: Vec::with_capacity(frame_len),
            frames_seen: 0,
            calibration_frames: 10, // 200 ms
            noise_floor: 0.0,
            speech_frames: 0,
            silence_frames: 0,
            silence_to_stop: per_sec * 9 / 10, // 0.9 s
            no_speech_timeout: per_sec * 4,    // 4 s
            max_frames: (max_seconds * per_sec as f32) as usize,
            status: Status::Listening,
        }
    }

    pub fn status(&self) -> Status {
        self.status
    }

    fn threshold(&self) -> f32 {
        (self.noise_floor * 3.0).max(MIN_THRESHOLD)
    }

    pub fn feed(&mut self, samples: &[f32]) -> Status {
        for &s in samples {
            if self.status != Status::Listening {
                break;
            }
            self.frame.push(s);
            if self.frame.len() == self.frame_len {
                let rms = (self.frame.iter().map(|v| v * v).sum::<f32>() / self.frame_len as f32).sqrt();
                self.frame.clear();
                self.on_frame(rms);
            }
        }
        self.status
    }

    fn on_frame(&mut self, rms: f32) {
        self.frames_seen += 1;
        if self.frames_seen <= self.calibration_frames {
            let n = self.frames_seen as f32;
            self.noise_floor += (rms - self.noise_floor) / n;
            return;
        }
        let loud = rms > self.threshold();
        let speaking = self.speech_frames >= SPEECH_FRAMES_TO_START;
        if loud {
            self.speech_frames += 1;
            self.silence_frames = 0;
        } else if speaking {
            self.silence_frames += 1;
        } else {
            self.speech_frames = 0;
        }
        let speaking = self.speech_frames >= SPEECH_FRAMES_TO_START;

        if speaking && self.silence_frames >= self.silence_to_stop {
            self.status = Status::Done { speech: true };
        } else if !speaking && self.frames_seen >= self.no_speech_timeout {
            self.status = Status::Done { speech: false };
        } else if self.frames_seen >= self.max_frames {
            self.status = Status::Done { speech: speaking };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn tone(seconds: f32, amp: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize).map(|i| (i as f32 * 0.07).sin() * amp).collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        tone(seconds, 0.0005)
    }

    #[test]
    fn stops_after_speech_followed_by_silence() {
        let mut e = Endpointer::new(RATE, 8.0);
        assert_eq!(e.feed(&silence(0.5)), Status::Listening);
        assert_eq!(e.feed(&tone(1.0, 0.3)), Status::Listening);
        assert_eq!(e.feed(&silence(0.5)), Status::Listening, "short pause must not cut the answer");
        assert_eq!(e.feed(&silence(0.6)), Status::Done { speech: true });
    }

    #[test]
    fn gives_up_when_nobody_speaks() {
        let mut e = Endpointer::new(RATE, 8.0);
        assert_eq!(e.feed(&silence(4.2)), Status::Done { speech: false });
    }

    #[test]
    fn a_single_click_is_not_speech() {
        let mut e = Endpointer::new(RATE, 8.0);
        e.feed(&silence(0.5));
        e.feed(&tone(0.02, 0.5)); // one loud frame
        assert_eq!(e.feed(&silence(3.8)), Status::Done { speech: false });
    }

    #[test]
    fn hard_cap_stops_endless_talking() {
        let mut e = Endpointer::new(RATE, 2.0);
        e.feed(&silence(0.3));
        assert_eq!(e.feed(&tone(3.0, 0.3)), Status::Done { speech: true });
    }

    #[test]
    fn noisy_room_raises_the_threshold() {
        let mut e = Endpointer::new(RATE, 8.0);
        e.feed(&tone(0.3, 0.05)); // calibrate on steady fan noise
        // Same noise level afterwards is not speech.
        assert_eq!(e.feed(&tone(4.0, 0.05)), Status::Done { speech: false });
    }

    #[test]
    fn feeding_after_done_is_a_no_op() {
        let mut e = Endpointer::new(RATE, 8.0);
        e.feed(&silence(4.2));
        assert_eq!(e.feed(&tone(1.0, 0.5)), Status::Done { speech: false });
    }
}
