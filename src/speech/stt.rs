//! Local speech recognition with whisper.cpp. Runs fully offline on every OS;
//! the language is pinned to the one being practiced.

use super::bench::Pass;
use super::transcriber::{Transcriber, clean, prepare};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::sync::OnceLock;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Threads lessons give whisper: every core, up to six.
pub fn lesson_threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get().min(6) as i32).unwrap_or(4)
}

/// What whisper.cpp was built with and found on this CPU (AVX2, FMA…), in its own words.
///
/// Asked once per process and kept: whisper.cpp writes the text into one static
/// buffer, so two threads asking at the same moment read it half written (it
/// came back as invalid UTF-8 when the speech tests loaded models in parallel).
pub fn system_info() -> &'static str {
    static INFO: OnceLock<String> = OnceLock::new();
    INFO.get_or_init(|| whisper_rs::print_system_info().trim().to_string())
}

/// The whisper.cpp version this build carries.
pub fn engine_version() -> &'static str {
    whisper_rs::get_whisper_version()
}

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
        super::trace::line(format_args!("speech engine: {} threads, {}", lesson_threads(), system_info()));
        Ok(Recognizer { ctx })
    }

    /// `samples`: 16 kHz mono. `lang`: ISO code of the language being practiced.
    /// `prompt`: words whisper should expect (an initial prompt), if any.
    pub fn transcribe(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Result<String> {
        self.run(samples, lang, prompt, Pass { threads: lesson_threads(), audio_ctx: 0 })
    }

    /// One pass run the way `pass` says, for `snowlearner model bench`.
    pub fn transcribe_as(&self, samples: &[f32], lang: &str, pass: Pass) -> Result<String> {
        self.run(samples, lang, None, pass)
    }

    fn run(&self, samples: &[f32], lang: &str, prompt: Option<&str>, pass: Pass) -> Result<String> {
        let mut state = self.ctx.create_state().context("creating recognizer state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let code = lang.split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase();
        params.set_language(Some(&code));
        if let Some(p) = prompt {
            params.set_initial_prompt(p);
        }
        params.set_n_threads(pass.threads);
        if pass.audio_ctx > 0 {
            params.set_audio_ctx(pass.audio_ctx);
        }
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

impl Transcriber for Recognizer {
    fn pass(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Result<String> {
        self.transcribe(samples, lang, prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_describes_itself_the_same_way_from_every_thread() {
        let ask = || {
            let mut info = system_info();
            for _ in 0..200 {
                info = system_info();
            }
            info
        };
        let threads: Vec<_> = (0..8).map(|_| std::thread::spawn(ask)).collect();
        let here = system_info();
        assert!(!here.is_empty());
        for t in threads {
            assert_eq!(t.join().expect("asking from a thread must not panic"), here);
        }
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
