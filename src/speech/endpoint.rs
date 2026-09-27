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
        GUARD_S + self.think + LATE_ONSET_GRACE_S + self.cap() + 1.0
    }
}

const FRAMES_PER_S: usize = 50; // 20 ms frames
const GUARD_S: f32 = 0.25;
/// The noise floor is a low percentile of the recent frames (guard included):
/// the quiet moments around words, never the words themselves.
const FLOOR_WINDOW_S: f32 = 6.0;
const FLOOR_PERCENTILE: f32 = 0.2;
/// Speech is this many times louder than the floor (≈ +9.5 dB).
const SPEECH_OVER_FLOOR: f32 = 3.0;
/// Absolute floor so a silent room with a noise-free mic still needs real speech.
const MIN_THRESHOLD: f32 = 0.012;
/// A noise floor above this is probably speech, not a fan: don't go deaf.
const MAX_FLOOR: f32 = 0.03;
/// Speech starts when this many of the last `ONSET_WINDOW` frames are loud
/// (60 ms of voice within 100 ms), so a word that dips on a consonant counts.
const SPEECH_FRAMES_TO_START: usize = 3;
const ONSET_WINDOW: usize = 5;
/// A burst already under way when listening starts (the learner answered the
/// instant the voice stopped) is judged once the quiet after it shows the real
/// floor — if it is word-sized. Longer leading noise is the room, not an answer.
const OPENING_BURST_MAX_S: f32 = 1.5;
/// A word that begins as thinking time runs out may finish.
const LATE_ONSET_GRACE_S: f32 = 0.5;
const HESITATION_S: f32 = 1.8;
const END_PAUSE_S: f32 = 0.8;

pub struct Endpointer {
    frame_len: usize,
    frame: Vec<f32>,
    /// RMS of every frame since the mic opened (guard included).
    history: Vec<f32>,
    guard: usize,
    /// Threshold frozen when speech starts.
    gate: Option<f32>,
    started_at: Option<usize>,
    last_loud: usize,
    spoken: usize,
    think: usize,
    grace: usize,
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
            history: Vec::new(),
            guard: frames(GUARD_S),
            gate: None,
            started_at: None,
            last_loud: 0,
            spoken: 0,
            think: frames(plan.think),
            grace: frames(LATE_ONSET_GRACE_S),
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
            if !self.started() {
                self.detect_onset();
            }
            self.status = Status::Done { speech: self.started() };
        }
        self.status
    }

    fn noise_floor(&self) -> f32 {
        let recent = &self.history[self.history.len().saturating_sub(frames(FLOOR_WINDOW_S))..];
        if recent.is_empty() {
            return 0.0;
        }
        let mut sorted = recent.to_vec();
        sorted.sort_by(f32::total_cmp);
        sorted[((sorted.len() - 1) as f32 * FLOOR_PERCENTILE) as usize]
    }

    fn threshold(&self) -> f32 {
        self.gate.unwrap_or_else(|| (self.noise_floor().min(MAX_FLOOR) * SPEECH_OVER_FLOOR).max(MIN_THRESHOLD))
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

    /// Looks for the start of speech against the current floor: in the last
    /// few frames, or in a word-sized burst that was already going when the
    /// mic opened (only now does the quiet after it reveal the real floor).
    fn detect_onset(&mut self) {
        let n = self.history.len();
        if n <= self.guard {
            return;
        }
        let gate = self.threshold();
        let loud = |i: usize| self.history[i] > gate;
        let recent = n.saturating_sub(ONSET_WINDOW).max(self.guard);
        let start = if (recent..n).filter(|&i| loud(i)).count() >= SPEECH_FRAMES_TO_START {
            (recent..n).find(|&i| loud(i))
        } else {
            let burst = (self.guard..n).take_while(|&i| loud(i)).count();
            let ended = self.guard + burst < n;
            (ended && (SPEECH_FRAMES_TO_START..=frames(OPENING_BURST_MAX_S)).contains(&burst)).then_some(self.guard)
        };
        if let Some(start) = start {
            self.gate = Some(gate);
            self.started_at = Some(start);
            self.spoken = (start..n).filter(|&i| loud(i)).count();
            self.last_loud = (start..n).rev().find(|&i| loud(i)).unwrap_or(start);
        }
    }

    fn on_frame(&mut self, rms: f32) {
        self.history.push(rms);
        self.level = rms;
        let n = self.history.len();
        if n <= self.guard {
            return;
        }
        let now = n - self.guard;
        match self.started_at {
            None => {
                self.detect_onset();
                if !self.started() && now >= self.think {
                    let gate = self.threshold();
                    let voice_now =
                        self.history[n.saturating_sub(ONSET_WINDOW).max(self.guard)..].iter().any(|&r| r > gate);
                    if !voice_now || now >= self.think + self.grace {
                        self.status = Status::Done { speech: false };
                    }
                }
            }
            Some(_) => {
                if rms > self.threshold() {
                    self.spoken += 1;
                    self.last_loud = n - 1;
                }
            }
        }
        if let Some(start) = self.started_at {
            let silence = n - 1 - self.last_loud;
            let said_enough = self.spoken as f32 >= self.expected as f32 * 0.6;
            let allowed = frames(if said_enough { END_PAUSE_S } else { HESITATION_S });
            if silence >= allowed || n - start >= self.cap {
                self.status = Status::Done { speech: true };
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

    /// Room noise: deterministic white noise at the given RMS.
    fn room(seconds: f32, rms: f32) -> Vec<f32> {
        let mut x: u32 = 0x9e37_79b9;
        (0..(seconds * RATE as f32) as usize)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * rms * 3f32.sqrt()
            })
            .collect()
    }

    /// A voiced, word-like burst (150 Hz with harmonics, 40 ms fade in/out) at
    /// roughly the given RMS.
    fn word(seconds: f32, rms: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let fade = ((0.04 * RATE as f32) as usize).min(n / 8).max(1);
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let voice =
                    (1..=4).map(|h| (std::f32::consts::TAU * 150.0 * h as f32 * t).sin() / h as f32).sum::<f32>();
                let env = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
                voice * env * rms / 0.85
            })
            .collect()
    }

    fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    /// Feeds `samples` in 10 ms chunks (like a mic callback) and returns the
    /// status plus whether "falando" was lit while the samples were playing.
    fn play(e: &mut Endpointer, samples: &[f32]) -> Status {
        let mut status = e.status();
        for chunk in samples.chunks(160) {
            status = e.feed(chunk);
        }
        status
    }

    const NOISE: f32 = 0.003;

    /// This laptop's real mic, measured (AMD "HiFi Mic1", stereo 48 kHz): the
    /// left channel sits at +0.044 DC, the right near 0, true room noise ~0.0013.
    /// Returns what the mic path (`mic::record`) hands the endpointer.
    fn through_offset_mic(voice: &[f32]) -> Vec<f32> {
        let stereo: Vec<f32> = voice.iter().flat_map(|&v| [v + 0.044, v]).collect();
        let mut mono = crate::speech::resample::to_mono(&stereo, 2);
        crate::speech::highpass::HighPass::new(RATE).process(&mut mono);
        mono
    }

    #[test]
    fn a_mic_with_a_dc_offset_still_hears_a_normal_voice() {
        let mut e = Endpointer::new(RATE, short());
        let mono = through_offset_mic(&cat(&[room(1.5, 0.0013), word(0.7, 0.03), room(3.0, 0.0013)]));
        assert_eq!(play(&mut e, &mono), Status::Done { speech: true }, "the DC offset was taken for room noise");
    }

    #[test]
    fn a_short_word_said_right_away_is_heard() {
        // "Yes." starting 100 ms after the mic opens: inside the echo guard
        // and all over the calibration window.
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(1, false));
        play(&mut e, &cat(&[room(0.1, NOISE), word(0.4, 0.05)]));
        assert!(e.started(), "\"falando\" should light while the word is said");
        assert_eq!(play(&mut e, &room(4.0, NOISE)), Status::Done { speech: true });
    }

    #[test]
    fn a_word_already_underway_when_the_mic_opens_is_heard() {
        // The learner answered the moment the voice stopped; the mic opened late.
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(1, false));
        play(&mut e, &word(0.6, 0.05));
        assert_eq!(play(&mut e, &room(4.0, NOISE)), Status::Done { speech: true });
    }

    #[test]
    fn humming_before_answering_does_not_make_it_deaf() {
        // "hmmm…" just under the gate for a few seconds, then a normal-volume word.
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(1, true));
        play(&mut e, &cat(&[room(1.0, NOISE), room(3.0, 0.011)]));
        assert!(!e.started(), "a hum is not the answer");
        play(&mut e, &word(0.5, 0.03));
        assert!(e.started(), "the word after the hum must be heard");
    }

    #[test]
    fn a_word_whose_loudness_flickers_near_the_gate_is_heard() {
        // Near the gate a real word dips below it every few frames (consonants).
        let mut e = Endpointer::new(RATE, short());
        play(&mut e, &room(0.8, NOISE));
        let mut flicker = Vec::new();
        for _ in 0..8 {
            flicker.extend(word(0.04, 0.03));
            flicker.extend(room(0.02, NOISE));
        }
        play(&mut e, &flicker);
        assert!(e.started());
    }

    #[test]
    fn a_word_started_as_thinking_time_runs_out_is_heard() {
        let plan = ListenPlan { think: 3.0, expected: 1.05 };
        let mut e = Endpointer::new(RATE, plan);
        // guard 0.25 s + 2.98 s: the word begins 20 ms before time is up.
        assert_eq!(play(&mut e, &room(0.25 + 2.98, NOISE)), Status::Listening);
        play(&mut e, &word(0.4, 0.05));
        assert_eq!(play(&mut e, &room(4.0, NOISE)), Status::Done { speech: true });
    }

    #[test]
    fn a_quiet_voice_on_a_quiet_mic_is_heard() {
        // Laptop mic at conversation distance: room ~0.002, voice ~0.02 RMS.
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(1, false));
        play(&mut e, &room(1.0, 0.002));
        play(&mut e, &word(0.4, 0.02));
        assert!(e.started());
    }

    #[test]
    fn noise_that_stops_is_not_mistaken_for_speech_afterwards() {
        // A fan that switches off must not turn its past noise into an "answer".
        let mut e = Endpointer::new(RATE, ListenPlan { think: 10.0, expected: 1.05 });
        play(&mut e, &room(2.0, 0.02));
        play(&mut e, &room(2.0, 0.001));
        assert!(!e.started(), "the fan was the room, not an answer");
        play(&mut e, &word(0.5, 0.05));
        assert!(e.started(), "the answer after it is still heard");
        assert_eq!(play(&mut e, &room(3.0, 0.001)), Status::Done { speech: true });
    }

    #[test]
    fn finishing_by_hand_right_after_a_quick_answer_counts_it() {
        let mut e = Endpointer::new(RATE, ListenPlan::for_phrase(1, false));
        play(&mut e, &cat(&[word(0.5, 0.05), room(0.2, NOISE)]));
        assert_eq!(e.finish(), Status::Done { speech: true });
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
