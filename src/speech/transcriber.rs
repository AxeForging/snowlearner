//! What every speech recognizer shares: one pass over the recorded answer
//! ([`Transcriber::pass`]), and the lesson's two passes on top of it, the
//! second with the vocabulary hint only when the first missed. whisper.cpp in
//! the app (`speech::stt`) and a Whisper server (`speech::remote`) both plug in here.

use super::matcher::Expect;
use super::trace;
use anyhow::Result;
use std::time::Instant;

pub trait Transcriber {
    /// One pass. `samples`: 16 kHz mono. `lang`: ISO code of the language being
    /// practiced. `prompt`: words to expect (an initial prompt), if any.
    fn pass(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Result<String>;

    /// Transcribes, and when `expect` says the answer was missed, tries once
    /// more with its vocabulary hint (see `matcher::Expect`).
    /// Each pass goes to the log with what it heard, its time and the verdict.
    fn transcribe_expecting(&self, samples: &[f32], lang: &str, expect: Option<&Expect>) -> Result<String> {
        let verdict = |text: &str| {
            expect.map(|e| {
                let m = e.judge(text);
                (m.passed(e.threshold), m.score)
            })
        };
        let started = Instant::now();
        let first = self.pass(samples, lang, None)?;
        let line = pass_line("first", &first, started.elapsed().as_secs_f32(), verdict(&first));
        trace::line(format_args!("{line}"));
        let Some(e) = expect else { return Ok(first) };
        Ok(e.pick(first, |hint| {
            let started = Instant::now();
            match self.pass(samples, lang, Some(hint)) {
                Ok(second) => {
                    let line = pass_line("hinted", &second, started.elapsed().as_secs_f32(), verdict(&second));
                    trace::line(format_args!("{line}"));
                    Some(second)
                }
                Err(err) => {
                    trace::line(format_args!("hinted pass failed: {err:#}"));
                    None
                }
            }
        }))
    }
}

/// Silence put before the answer. A word that starts right at the first
/// sample (the learner answered instantly) is misheard without it: "Sorry."
/// came back as "I'll read.", "Good morning." as "go tomorrow again."
pub const LEAD_IN: usize = 16_000 * 3 / 10;
/// Whisper needs at least ~1 s of audio; short answers are padded with silence.
pub const MIN_SAMPLES: usize = 16_000 + 1_600;

/// The audio whisper gets: a silent lead-in, the answer, silence up to the minimum.
pub fn prepare(samples: &[f32]) -> Vec<f32> {
    let mut audio = vec![0.0; LEAD_IN];
    audio.extend_from_slice(samples);
    if audio.len() < MIN_SAMPLES {
        audio.resize(MIN_SAMPLES, 0.0);
    }
    audio
}

/// The log line for one pass: what it heard, how long it took and, in a
/// lesson, how it was judged (`(accepted, score)`).
fn pass_line(pass: &str, text: &str, seconds: f32, verdict: Option<(bool, f32)>) -> String {
    let line = format!("{pass} pass {text:?} in {seconds:.1} s");
    match verdict {
        Some((true, score)) => format!("{line}: accepted (score {score:.2})"),
        Some((false, score)) => format!("{line}: not accepted (score {score:.2})"),
        None => line,
    }
}

/// Drops whisper's non-speech annotations like "[BLANK_AUDIO]" or "(music)".
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_are_removed_and_whitespace_collapsed() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean(" I'm hungry. (laughs)  "), "I'm hungry.");
        assert_eq!(clean("Tengo  frío"), "Tengo frío");
    }

    #[test]
    fn the_answer_gets_a_silent_lead_in_and_a_minimum_length() {
        let word = vec![0.5; 8_000];
        let audio = prepare(&word);
        assert!(audio[..LEAD_IN].iter().all(|&s| s == 0.0), "silence first");
        assert_eq!(&audio[LEAD_IN..LEAD_IN + word.len()], word.as_slice(), "then the answer, untouched");
        assert_eq!(audio.len(), MIN_SAMPLES);
        let long = vec![0.1; 48_000];
        assert_eq!(prepare(&long).len(), LEAD_IN + long.len(), "long answers are not cut");
        assert_eq!(prepare(&[]).len(), MIN_SAMPLES);
    }

    #[test]
    fn each_pass_is_logged_with_what_it_heard_its_time_and_the_verdict() {
        let first = pass_line("first", "Oh, please.", 3.42, Some((false, 0.734)));
        assert_eq!(first, r#"first pass "Oh, please." in 3.4 s: not accepted (score 0.73)"#);
        let hinted = pass_line("hinted", "Cold.", 3.46, Some((true, 1.0)));
        assert_eq!(hinted, r#"hinted pass "Cold." in 3.5 s: accepted (score 1.00)"#);
        assert_eq!(pass_line("first", "Hi", 3.3, None), r#"first pass "Hi" in 3.3 s"#, "no lesson, no verdict");
    }
}
