//! Local speech recognition with whisper.cpp. Runs fully offline on every OS;
//! the language is pinned to the one being practiced.

use super::matcher::Expect;
use anyhow::{Context, Result, bail};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Recognizer {
    ctx: WhisperContext,
}

impl Recognizer {
    pub fn load(model: &Path) -> Result<Recognizer> {
        if !model.exists() {
            bail!("speech model not found at {} — run `snowlearner model download`", model.display());
        }
        whisper_rs::install_logging_hooks();
        let path = model.to_str().context("model path is not valid UTF-8")?;
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .with_context(|| format!("loading speech model {}", model.display()))?;
        Ok(Recognizer { ctx })
    }

    /// Transcribes, and when `expect` says the answer was missed, tries once
    /// more with its vocabulary hint (see `matcher::Expect`).
    pub fn transcribe_expecting(&self, samples: &[f32], lang: &str, expect: Option<&Expect>) -> Result<String> {
        let first = self.transcribe(samples, lang, None)?;
        let Some(e) = expect else { return Ok(first) };
        Ok(e.pick(first, |hint| self.transcribe(samples, lang, Some(hint)).ok()))
    }

    /// `samples`: 16 kHz mono. `lang`: ISO code of the language being practiced.
    /// `prompt`: words whisper should expect (an initial prompt), if any.
    pub fn transcribe(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Result<String> {
        let mut state = self.ctx.create_state().context("creating recognizer state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let code = lang.split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase();
        params.set_language(Some(&code));
        if let Some(p) = prompt {
            params.set_initial_prompt(p);
        }
        params.set_n_threads(std::thread::available_parallelism().map(|n| n.get().min(6) as i32).unwrap_or(4));
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_suppress_blank(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        state.full(params, &prepare(samples)).context("speech recognition failed")?;
        let text: String = state
            .as_iter()
            .filter_map(|seg| seg.to_str_lossy().ok().map(|s| s.into_owned()))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(clean(&text))
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
    fn missing_model_explains_how_to_get_one() {
        let err = format!("{:#}", Recognizer::load(Path::new("/nope/ggml-base.bin")).err().unwrap());
        assert!(err.contains("snowlearner model download"), "{err}");
    }

    /// Real end-to-end check; needs `SNOWLEARNER_TEST_MODEL=/path/ggml-*.bin`.
    #[test]
    #[ignore = "needs a whisper model; set SNOWLEARNER_TEST_MODEL and run with --ignored"]
    fn silence_transcribes_to_nothing_meaningful() {
        let model = std::env::var("SNOWLEARNER_TEST_MODEL").expect("SNOWLEARNER_TEST_MODEL");
        let r = Recognizer::load(Path::new(&model)).unwrap();
        let text = r.transcribe(&vec![0.0; 16_000 * 2], "en", None).unwrap();
        assert!(crate::speech::matcher::score("I'm hungry", &text).score < 0.5, "{text:?}");
    }
}
