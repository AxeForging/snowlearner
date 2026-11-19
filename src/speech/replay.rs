//! `replay_voice` (#116): on a right answer the learner hears what they just
//! said. Only the spoken part is played back — the silence before and after is
//! trimmed and it is capped at `MAX_SECONDS` — and it lives only in memory,
//! until the next listen starts. Nothing here touches a device or the disk.

/// Longest replay (s): a whole answer of a beginner, not a long ramble.
pub const MAX_SECONDS: f32 = 3.0;
/// Frames of this length decide what is speech (s).
const FRAME_S: f32 = 0.02;
/// Kept around the speech so its first and last sounds are not clipped (s).
const MARGIN_S: f32 = 0.1;
/// A frame is speech when its RMS reaches this share of the loudest frame,
/// and never below `FLOOR` (a quiet room is not speech).
const SHARE: f32 = 0.1;
const FLOOR: f32 = 0.01;

/// The spoken part of `samples` (mono at `rate` Hz): from the first frame of
/// speech to the last, with a short margin, at most `MAX_SECONDS`. Empty when
/// nothing reaches the speech level.
pub fn spoken_part(samples: &[f32], rate: u32) -> &[f32] {
    let frame = ((rate as f32 * FRAME_S) as usize).max(1);
    let rms: Vec<f32> =
        samples.chunks(frame).map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt()).collect();
    let gate = (rms.iter().copied().fold(0.0, f32::max) * SHARE).max(FLOOR);
    let Some(first) = rms.iter().position(|&r| r >= gate) else {
        return &[];
    };
    let last = rms.iter().rposition(|&r| r >= gate).unwrap_or(first);
    let margin = (rate as f32 * MARGIN_S) as usize;
    let start = (first * frame).saturating_sub(margin);
    let end = ((last + 1) * frame + margin).min(samples.len());
    let cap = (rate as f32 * MAX_SECONDS) as usize;
    &samples[start..end.min(start + cap)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    /// `silence` s of room hiss, `speech` s of a loud tone, then `silence` again.
    fn answer(silence: f32, speech: f32) -> Vec<f32> {
        let n = |s: f32| (s * RATE as f32) as usize;
        let hiss = |i: usize| if i % 2 == 0 { 0.002 } else { -0.002 };
        let mut v: Vec<f32> = (0..n(silence)).map(hiss).collect();
        v.extend((0..n(speech)).map(|i| 0.3 * (i as f32 * 0.07).sin()));
        v.extend((0..n(silence)).map(hiss));
        v
    }

    #[test]
    fn only_the_spoken_part_is_kept_with_a_short_margin() {
        let a = answer(1.5, 0.8);
        let kept = spoken_part(&a, RATE);
        let secs = kept.len() as f32 / RATE as f32;
        assert!((0.8..=1.05).contains(&secs), "kept {secs} s of 0.8 s of speech in 3.8 s");
        let start = kept.as_ptr() as usize - a.as_ptr() as usize;
        let start_s = start as f32 / 4.0 / RATE as f32;
        assert!((1.35..=1.5).contains(&start_s), "starts just before the speech: {start_s} s");
    }

    #[test]
    fn a_long_answer_is_capped() {
        let a = answer(0.5, 6.0);
        assert_eq!(spoken_part(&a, RATE).len(), (MAX_SECONDS * RATE as f32) as usize);
    }

    #[test]
    fn a_quiet_room_replays_nothing() {
        assert!(spoken_part(&answer(2.0, 0.0), RATE).is_empty());
        assert!(spoken_part(&[], RATE).is_empty());
    }
}
