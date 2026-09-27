//! Decides when the learner has finished speaking, from raw mic samples —
//! tuned for learners, not fluent speakers:
//! - ignores the first 250 ms (echo of the voice that just read the cue),
//! - gives thinking time before you start (more when recalling from memory),
//! - calibrates on the *quietest* frames, so talking right away doesn't deafen it,
//! - lets you hesitate mid-sentence until you've said roughly what's expected,
//! - caps the turn relative to when you started speaking, not when the mic opened,
//! - can be finished by hand (hotkey / orb click).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Listening,
    /// Stopped. `speech` is false when nothing above the noise floor was heard.
    Done {
        speech: bool,
    },
}

/// How long to wait for, and how long to expect, this answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListenPlan {
    /// Seconds allowed before speech starts.
    pub think: f32,
    /// Roughly how long saying the phrase takes a learner.
    pub expected: f32,
}

impl ListenPlan {
    pub fn for_phrase(words: usize, recall: bool) -> ListenPlan {
        ListenPlan { think: if recall { 10.0 } else { 6.0 }, expected: 0.45 * words.max(1) as f32 + 0.6 }
    }

    /// Longest turn once speech has started.
    pub fn cap(&self) -> f32 {
        (self.expected * 2.5 + 2.0).clamp(4.0, 20.0)
    }

    /// Worst-case total time the mic may stay open.
    pub fn max_total(&self) -> f32 {
        GUARD_S + self.think + self.cap() + 1.0
    }
}

const FRAMES_PER_S: usize = 50; // 20 ms frames
const GUARD_S: f32 = 0.25;
const CALIBRATION_FRAMES: usize = 15;
/// Absolute floor so a silent room with a noise-free mic still needs real speech.
const MIN_THRESHOLD: f32 = 0.012;
/// A noise floor above this is probably speech, not a fan: don't go deaf.
const MAX_FLOOR: f32 = 0.03;
const SPEECH_FRAMES_TO_START: usize = 3;
const HESITATION_S: f32 = 1.8;
const END_PAUSE_S: f32 = 0.8;

pub struct Endpointer {
    frame_len: usize,
    frame: Vec<f32>,
    frames_seen: usize,
    guard: usize,
    calibration: Vec<f32>,
    noise_floor: Option<f32>,
    onset: usize,
    started_at: Option<usize>,
    spoken: usize,
    silence: usize,
    think: usize,
    expected: usize,
    cap: usize,
    level: f32,
    status: Status,
}

fn frames(seconds: f32) -> usize {
    (seconds * FRAMES_PER_S as f32).round() as usize
}

impl Endpointer {
    pub fn new(sample_rate: u32, plan: ListenPlan) -> Self {
        Self {
            frame_len: (sample_rate as usize / FRAMES_PER_S).max(1),
            frame: Vec::new(),
            frames_seen: 0,
            guard: frames(GUARD_S),
            calibration: Vec::with_capacity(CALIBRATION_FRAMES),
            noise_floor: None,
            onset: 0,
            started_at: None,
            spoken: 0,
            silence: 0,
            think: frames(plan.think),
            expected: frames(plan.expected),
            cap: frames(plan.cap()),
            level: 0.0,
            status: Status::Listening,
        }
    }

    pub fn status(&self) -> Status {
        self.status
    }

    /// Last frame's loudness (RMS), for a level meter.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// The learner has started answering.
    pub fn started(&self) -> bool {
        self.started_at.is_some()
    }

    /// Ends the turn now (learner pressed "done").
    pub fn finish(&mut self) -> Status {
        if self.status == Status::Listening {
            self.status = Status::Done { speech: self.started() };
        }
        self.status
    }

    fn threshold(&self) -> f32 {
        let floor = self.noise_floor.unwrap_or_else(|| self.calibration.iter().copied().fold(f32::MAX, f32::min));
        (floor.min(MAX_FLOOR) * 3.0).max(MIN_THRESHOLD)
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
        self.level = rms;
        if self.frames_seen <= self.guard {
            return;
        }
        if self.noise_floor.is_none() {
            self.calibration.push(rms);
            if self.calibration.len() >= CALIBRATION_FRAMES {
                // The quietest moments are the room, not the voice.
                let mut sorted = self.calibration.clone();
                sorted.sort_by(f32::total_cmp);
                self.noise_floor = Some(sorted[sorted.len() / 5]);
            }
        }
        let loud = rms > self.threshold();
        let now = self.frames_seen - self.guard;

        match self.started_at {
            None => {
                if loud {
                    self.onset += 1;
                    if self.onset >= SPEECH_FRAMES_TO_START {
                        self.started_at = Some(now);
                        self.spoken = self.onset;
                    }
                } else {
                    self.onset = 0;
                    if let Some(f) = &mut self.noise_floor {
                        *f = *f * 0.98 + rms * 0.02; // follow a changing room
                    }
                }
                if self.started_at.is_none() && now >= self.think {
                    self.status = Status::Done { speech: false };
                }
            }
            Some(start) => {
                if loud {
                    self.spoken += 1;
                    self.silence = 0;
                } else {
                    self.silence += 1;
                }
                let said_enough = self.spoken as f32 >= self.expected as f32 * 0.6;
                let allowed = frames(if said_enough { END_PAUSE_S } else { HESITATION_S });
                if self.silence >= allowed || now - start >= self.cap {
                    self.status = Status::Done { speech: true };
                }
            }
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

    fn short() -> ListenPlan {
        ListenPlan::for_phrase(2, false)
    }

    #[test]
    fn stops_after_the_answer_and_a_short_pause() {
        let mut e = Endpointer::new(RATE, short());
        assert_eq!(e.feed(&silence(0.6)), Status::Listening);
        assert_eq!(e.feed(&tone(1.2, 0.3)), Status::Listening);
        assert_eq!(e.feed(&silence(0.5)), Status::Listening);
        assert_eq!(e.feed(&silence(0.4)), Status::Done { speech: true });
    }

    #[test]
    fn a_hesitation_early_in_a_long_phrase_does_not_cut_the_answer() {
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(7, false)); // ~3.75 s expected
        e.feed(&silence(0.6));
        e.feed(&tone(0.5, 0.3)); // "I'm…"
        assert_eq!(e.feed(&silence(1.4)), Status::Listening, "thinking mid-sentence is fine");
        e.feed(&tone(2.2, 0.3)); // "…running a few minutes late"
        assert_eq!(e.feed(&silence(1.0)), Status::Done { speech: true });
    }

    #[test]
    fn recall_gets_more_thinking_time_than_repeat() {
        let mut repeat = Endpointer::new(RATE, ListenPlan::for_phrase(3, false));
        let mut recall = Endpointer::new(RATE, ListenPlan::for_phrase(3, true));
        assert_eq!(repeat.feed(&silence(6.6)), Status::Done { speech: false });
        assert_eq!(recall.feed(&silence(6.6)), Status::Listening);
        assert_eq!(recall.feed(&silence(4.0)), Status::Done { speech: false });
    }

    #[test]
    fn the_tail_of_the_tts_voice_is_not_taken_as_speech() {
        let mut e = Endpointer::new(RATE, short());
        e.feed(&tone(0.2, 0.4)); // echo inside the guard window
        assert!(!e.started());
        assert_eq!(e.feed(&silence(6.5)), Status::Done { speech: false });
    }

    #[test]
    fn talking_right_away_still_works() {
        let mut e = Endpointer::new(RATE, short());
        e.feed(&silence(0.26));
        e.feed(&tone(1.5, 0.3)); // speaking through the calibration window
        assert!(e.started());
        assert_eq!(e.feed(&silence(1.0)), Status::Done { speech: true });
    }

    #[test]
    fn the_cap_counts_from_when_you_start_speaking() {
        let plan = ListenPlan::for_phrase(2, false);
        let mut e = Endpointer::new(RATE, plan);
        e.feed(&silence(5.0)); // thinking for a while
        assert_eq!(e.feed(&tone(plan.cap() - 0.5, 0.3)), Status::Listening);
        assert_eq!(e.feed(&tone(1.0, 0.3)), Status::Done { speech: true });
    }

    #[test]
    fn a_single_click_is_not_speech() {
        let mut e = Endpointer::new(RATE, short());
        e.feed(&silence(0.6));
        e.feed(&tone(0.02, 0.5));
        assert!(!e.started());
    }

    #[test]
    fn steady_fan_noise_is_not_speech() {
        let mut e = Endpointer::new(RATE, short());
        assert_eq!(e.feed(&tone(7.0, 0.01)), Status::Done { speech: false });
    }

    #[test]
    fn finishing_by_hand_ends_the_turn() {
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(8, true));
        e.feed(&silence(0.6));
        e.feed(&tone(0.8, 0.3));
        assert_eq!(e.finish(), Status::Done { speech: true });
        assert_eq!(e.feed(&tone(1.0, 0.5)), Status::Done { speech: true }, "no-op afterwards");
        let mut quiet = Endpointer::new(RATE, short());
        assert_eq!(quiet.finish(), Status::Done { speech: false });
    }

    #[test]
    fn level_follows_the_voice() {
        let mut e = Endpointer::new(RATE, short());
        e.feed(&silence(0.5));
        let quiet = e.level();
        e.feed(&tone(0.1, 0.3));
        assert!(e.level() > quiet * 10.0);
    }

    #[test]
    fn plans_scale_with_phrase_length_within_bounds() {
        assert!(ListenPlan::for_phrase(10, false).cap() > ListenPlan::for_phrase(2, false).cap());
        assert_eq!(ListenPlan { think: 6.0, expected: 0.5 }.cap(), 4.0, "never shorter than 4 s");
        assert_eq!(ListenPlan::for_phrase(60, false).cap(), 20.0);
    }
}
