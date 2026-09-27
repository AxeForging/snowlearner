//! Local speech recognition with whisper.cpp. Runs fully offline on every OS;
//! the language is pinned to the one being practiced.

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

    /// `samples`: 16 kHz mono. `lang`: ISO code of the language being practiced.
    pub fn transcribe(&self, samples: &[f32], lang: &str) -> Result<String> {
        let mut state = self.ctx.create_state().context("creating recognizer state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let code = lang.split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase();
        params.set_language(Some(&code));
        params.set_n_threads(std::thread::available_parallelism().map(|n| n.get().min(6) as i32).unwrap_or(4));
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_suppress_blank(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        // Whisper needs at least ~1 s of audio; pad short answers with silence.
        let mut audio = samples.to_vec();
        if audio.len() < 16_000 + 1_600 {
            audio.resize(16_000 + 1_600, 0.0);
        }
        state.full(params, &audio).context("speech recognition failed")?;
        let text: String = state
            .as_iter()
            .filter_map(|seg| seg.to_str_lossy().ok().map(|s| s.into_owned()))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(clean(&text))
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
        let text = r.transcribe(&vec![0.0; 16_000 * 2], "en").unwrap();
        assert!(crate::speech::matcher::score("I'm hungry", &text).score < 0.5, "{text:?}");
    }
}
