//! Whisper's encoder window. whisper.cpp encodes a fixed 30 s window whatever
//! the length of the audio, and for a short answer that is nearly all of the
//! time recognition takes: an answer only needs the part of the window it fills.
//! Pure.

/// Frames in the full window (whisper's `n_audio_ctx`).
pub const FULL: i32 = 1500;
/// What the full window holds.
pub const FULL_SECONDS: f32 = 30.0;
/// The narrowest window lessons use, 7.5 s: a one-word answer gets it.
pub const NARROWEST: i32 = FULL / 4;

/// Silence kept in the window after the audio ends, so the last word is not at its edge.
const MARGIN_SECONDS: f32 = 1.0;

/// The window for `seconds` of audio: the audio and a margin, never narrower
/// than `NARROWEST`, never wider than the full window.
pub fn frames_for(seconds: f32) -> i32 {
    let frames = ((seconds + MARGIN_SECONDS) * FULL as f32 / FULL_SECONDS).ceil() as i32;
    frames.clamp(NARROWEST, FULL)
}

/// How many seconds a window of `frames` holds.
pub fn seconds(frames: i32) -> f32 {
    frames as f32 * FULL_SECONDS / FULL as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_answer_gets_the_narrowest_window() {
        assert_eq!(frames_for(0.0), NARROWEST);
        assert_eq!(frames_for(1.5), NARROWEST);
        assert_eq!(frames_for(6.5), NARROWEST, "6.5 s and its margin fill 7.5 s exactly");
    }

    #[test]
    fn a_longer_answer_gets_its_length_and_a_margin() {
        assert_eq!(frames_for(7.0), 400);
        assert_eq!(frames_for(14.0), 750);
        assert!(seconds(frames_for(12.3)) >= 13.3);
    }

    #[test]
    fn the_window_never_exceeds_whispers_own() {
        assert_eq!(frames_for(29.5), FULL);
        assert_eq!(frames_for(90.0), FULL);
    }

    #[test]
    fn a_window_reads_in_seconds() {
        assert_eq!(seconds(FULL), 30.0);
        assert_eq!(seconds(750), 15.0);
        assert_eq!(seconds(NARROWEST), 7.5);
    }
}
