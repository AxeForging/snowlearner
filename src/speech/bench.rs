//! What `snowlearner model bench` runs: the clip it transcribes and the passes
//! it times. Pure, so the plan is testable without a model.

use super::resample::WHISPER_RATE;
use super::window;
use std::f32::consts::TAU;

/// Length of the built-in clip.
pub const CLIP_SECONDS: f32 = 1.5;

/// One timed transcription: how many threads whisper gets and how wide its
/// encoder window is, in whisper frames (`audio_ctx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pass {
    pub threads: i32,
    pub audio_ctx: i32,
}

/// The passes, in the order they run: what lessons use for audio this long
/// first (twice, so a slow first pass shows), then the other thread counts,
/// then the wider windows up to whisper's own.
pub fn passes(lesson_threads: i32, cores: i32, audio_seconds: f32) -> Vec<Pass> {
    let lesson = Pass { threads: lesson_threads, audio_ctx: window::frames_for(audio_seconds) };
    let mut out = vec![lesson, lesson];
    for threads in [1, 2, 4, cores] {
        if threads <= cores && !out.iter().any(|p| p.threads == threads) {
            out.push(Pass { threads, audio_ctx: lesson.audio_ctx });
        }
    }
    for wider in [window::FULL / 2, window::FULL] {
        if wider > lesson.audio_ctx {
            out.push(Pass { threads: lesson_threads, audio_ctx: wider });
        }
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

    /// Thread counts of the passes run at the lesson window.
    fn threads(passes: &[Pass]) -> Vec<i32> {
        passes.iter().filter(|p| p.audio_ctx == passes[0].audio_ctx).map(|p| p.threads).collect()
    }

    /// The windows wider than the lesson's.
    fn wider(passes: &[Pass]) -> Vec<i32> {
        passes.iter().filter(|p| p.audio_ctx > passes[0].audio_ctx).map(|p| p.audio_ctx).collect()
    }

    #[test]
    fn what_lessons_use_is_timed_first_and_twice() {
        let p = passes(6, 16, CLIP_SECONDS);
        assert_eq!(p[0], Pass { threads: 6, audio_ctx: window::NARROWEST });
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
    fn wider_windows_follow_with_the_lesson_threads_up_to_whispers_own() {
        let short = passes(6, 16, CLIP_SECONDS);
        assert_eq!(wider(&short), [750, window::FULL]);
        assert!(short.iter().filter(|p| p.audio_ctx > window::NARROWEST).all(|p| p.threads == 6));
        assert_eq!(wider(&passes(6, 16, 20.0)), [window::FULL], "20 s already needs more than half");
        assert!(wider(&passes(6, 16, 40.0)).is_empty(), "long audio is whisper's own window from the start");
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
