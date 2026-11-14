//! What `snowlearner model bench` runs: the clip it transcribes and the passes
//! it times. Pure, so the plan is testable without a model.

use super::resample::WHISPER_RATE;
use std::f32::consts::TAU;

/// Frames whisper's encoder sees for its full 30 s window (`n_audio_ctx`).
pub const FULL_WINDOW: i32 = 1500;
const FULL_WINDOW_SECONDS: f32 = 30.0;

/// Narrower encoder windows worth timing: whisper encodes the whole window even
/// for one word, so these show how much of a pass is the encoder.
const NARROW_WINDOWS: [i32; 2] = [750, 375];

/// Length of the built-in clip.
pub const CLIP_SECONDS: f32 = 1.5;

/// One timed transcription: how many threads whisper gets and how wide its
/// encoder window is (`audio_ctx`; 0 = the full window, what lessons use).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pass {
    pub threads: i32,
    pub audio_ctx: i32,
}

impl Pass {
    /// The encoder window in seconds, for the report.
    pub fn window_seconds(&self) -> f32 {
        let frames = if self.audio_ctx > 0 { self.audio_ctx } else { FULL_WINDOW };
        frames as f32 * FULL_WINDOW_SECONDS / FULL_WINDOW as f32
    }
}

/// The passes, in the order they run: what lessons use first (twice, so a slow
/// first pass shows), then the other thread counts, then the narrower windows
/// that still hold the whole clip.
pub fn passes(lesson_threads: i32, cores: i32, clip_seconds: f32) -> Vec<Pass> {
    let lesson = Pass { threads: lesson_threads, audio_ctx: 0 };
    let mut out = vec![lesson, lesson];
    for threads in [1, 2, 4, cores] {
        if threads <= cores && !out.iter().any(|p| p.threads == threads) {
            out.push(Pass { threads, audio_ctx: 0 });
        }
    }
    let needed = (clip_seconds * FULL_WINDOW as f32 / FULL_WINDOW_SECONDS).ceil() as i32;
    for window in NARROW_WINDOWS {
        if window >= needed {
            out.push(Pass { threads: lesson_threads, audio_ctx: window });
        }
    }
    out
}

/// Two passes run at the same time, as a lesson could run its plain and its
/// hinted pass: the threads each one gets. Lesson threads each, then half of
/// them each, since more threads on one pass made it far slower on the
/// reporting machine (16 threads: 76.84 s against 3.26 s with six). Nothing to
/// time on a single core.
pub fn pairs(lesson_threads: i32, cores: i32) -> Vec<(i32, i32)> {
    if cores < 2 {
        return Vec::new();
    }
    let half = (lesson_threads / 2).max(1);
    let mut out = vec![(lesson_threads, lesson_threads)];
    if half != lesson_threads {
        out.push((half, half));
    }
    out
}

/// The built-in clip: a voiced hum in syllable-like pulses, 16 kHz mono. It is
/// not speech, so what whisper hears in it means nothing; it is the same audio
/// on every machine, which is what makes two timings comparable.
pub fn clip() -> Vec<f32> {
    let n = (CLIP_SECONDS * WHISPER_RATE as f32) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / WHISPER_RATE as f32;
        let voice: f32 = (1..=5).map(|h| (TAU * 140.0 * h as f32 * t).sin() / h as f32).sum();
        let pulse = 0.5 - 0.5 * (TAU * 4.0 * t).cos();
        out.push(0.2 * voice * pulse);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn threads(passes: &[Pass]) -> Vec<i32> {
        passes.iter().filter(|p| p.audio_ctx == 0).map(|p| p.threads).collect()
    }

    fn windows(passes: &[Pass]) -> Vec<i32> {
        passes.iter().filter(|p| p.audio_ctx > 0).map(|p| p.audio_ctx).collect()
    }

    #[test]
    fn what_lessons_use_is_timed_first_and_twice() {
        let p = passes(6, 16, CLIP_SECONDS);
        assert_eq!(p[0], Pass { threads: 6, audio_ctx: 0 });
        assert_eq!(p[1], p[0]);
    }

    #[test]
    fn other_thread_counts_are_timed_once_and_never_above_the_cores() {
        assert_eq!(threads(&passes(6, 16, CLIP_SECONDS)), [6, 6, 1, 2, 4, 16]);
        assert_eq!(threads(&passes(4, 4, CLIP_SECONDS)), [4, 4, 1, 2]);
        assert_eq!(threads(&passes(2, 2, CLIP_SECONDS)), [2, 2, 1]);
        assert_eq!(threads(&passes(1, 1, CLIP_SECONDS)), [1, 1]);
        assert_eq!(threads(&passes(6, 6, CLIP_SECONDS)), [6, 6, 1, 2, 4]);
    }

    #[test]
    fn narrow_windows_use_the_lesson_threads_and_must_hold_the_clip() {
        let short = passes(6, 16, CLIP_SECONDS);
        assert_eq!(windows(&short), [750, 375]);
        assert!(short.iter().filter(|p| p.audio_ctx > 0).all(|p| p.threads == 6));
        assert_eq!(windows(&passes(6, 16, 7.5)), [750, 375], "7.5 s fills the narrowest window exactly");
        assert_eq!(windows(&passes(6, 16, 10.0)), [750]);
        assert!(windows(&passes(6, 16, 20.0)).is_empty());
    }

    #[test]
    fn two_passes_at_once_are_timed_with_the_lesson_threads_and_with_half() {
        assert_eq!(pairs(6, 16), [(6, 6), (3, 3)]);
        assert_eq!(pairs(4, 4), [(4, 4), (2, 2)]);
        assert_eq!(pairs(1, 2), [(1, 1)], "one thread cannot be split");
        assert!(pairs(1, 1).is_empty(), "a single core runs nothing at once");
    }

    #[test]
    fn a_window_reads_in_seconds() {
        assert_eq!(Pass { threads: 6, audio_ctx: 0 }.window_seconds(), 30.0);
        assert_eq!(Pass { threads: 6, audio_ctx: 750 }.window_seconds(), 15.0);
        assert_eq!(Pass { threads: 6, audio_ctx: 375 }.window_seconds(), 7.5);
    }

    #[test]
    fn the_builtin_clip_is_audible_never_clips_and_is_the_same_every_time() {
        let a = clip();
        assert_eq!(a.len(), 24_000);
        let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1 && peak < 1.0, "peak {peak}");
        assert_eq!(a, clip());
    }
}
